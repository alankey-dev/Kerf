//! Launch modes. `--open` puts a project file in a tab. `--bench` plays [`Script`] over one and writes
//! each phase's frame times (spec 38). The script drives the app only through its own commands and view
//! state, the way a menu pick does, so no input event is ever made up.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::Context;

use super::Kerf;
use super::commands::Command;
use crate::bench::report::{FrameStats, Metrics, Phase, PhaseFrames, now_unix_ms};
use crate::design::Design;
use crate::edit::{AppCommand, Edit};

impl Kerf {
    /// Opens the project file at `path` in a new tab, under its file stem as its id. A failure shows in
    /// the error banner and is returned.
    pub fn open_file(&mut self, path: &Path) -> Result<(), String> {
        let opened = std::fs::read_to_string(path).and_then(|text| crate::project::parse(&text));
        let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match opened {
            Ok((mut project, _)) => {
                project.id = id;
                tracing::info!(target: "save", path = %path.display(), "open file");
                self.push_tab(super::document::Document::open(project));
                Ok(())
            }
            Err(e) => {
                let message = format!("Couldn't open {}: {e}", path.display());
                self.report(message.clone());
                Err(message)
            }
        }
    }
}

/// The phases [`Script`] plays, in order. `Bench` plays the two at-rest phases before them.
pub(super) const SCRIPTED: [Phase; 8] =
    [Phase::Idle, Phase::DragShape, Phase::DragGroup, Phase::Zoom, Phase::SelectAll, Phase::UndoRedo, Phase::MakePrepare, Phase::CutPreview];

/// How far one drag frame moves its subject; it turns back every `SWAY_FRAMES` frames.
const DRAG_STEP_IN: f32 = 0.02;
const SWAY_FRAMES: usize = 30;
/// Zoom steps in this many times, then out as many.
const ZOOM_STEPS: usize = 4;

/// The fixed sequence a benchmark plays over a loaded design. Each phase lasts `frames` frames and at least
/// `min` of wall time, so a once-a-second sampler sees every phase.
pub(super) struct Script {
    phase: usize,
    frame: usize,
    started: Option<Instant>,
    frames: usize,
    min: Duration,
    /// The lone shape the shape drag moves, and the members of the group the group drag moves.
    shape: Vec<u64>,
    group: Vec<u64>,
}

impl Script {
    pub(super) fn new(design: &Design, frames: usize, min: Duration) -> Self {
        let lone = design.shapes.iter().find(|s| s.parent.is_none() && s.link.is_none() && !s.holds_children() && !s.lock_position);
        let grouped = design.shapes.iter().find(|s| s.parent.is_none() && s.link.is_some_and(|l| l.kind == crate::links::LinkKind::Group));
        Self {
            phase: 0,
            frame: 0,
            started: None,
            frames,
            min,
            shape: lone.map(|s| vec![s.id]).unwrap_or_default(),
            group: grouped.map(|s| design.expand(&[s.id])).unwrap_or_default(),
        }
    }

    /// Sets the app up for its next frame and returns the phase that frame counts towards, or `None` once
    /// the script is over.
    pub(super) fn step(&mut self, app: &mut Kerf) -> Option<Phase> {
        let phase = loop {
            let phase = *SCRIPTED.get(self.phase)?;
            let started = *self.started.get_or_insert_with(Instant::now);
            if self.frame < self.frames || started.elapsed() < self.min {
                break phase;
            }
            leave(app, phase);
            (self.phase, self.frame, self.started) = (self.phase + 1, 0, None);
        };
        let i = self.frame;
        self.frame += 1;
        let command = match phase {
            Phase::Start | Phase::Rest => None,
            Phase::Idle => {
                app.doc.selection.clear();
                None
            }
            Phase::DragShape => drag(app, &self.shape, i),
            Phase::DragGroup => drag(app, &self.group, i),
            Phase::Zoom => Some(Command::App(if (i / ZOOM_STEPS).is_multiple_of(2) { AppCommand::ZoomIn } else { AppCommand::ZoomOut })),
            Phase::SelectAll => Some(Command::Edit(if i.is_multiple_of(2) { Edit::SelectAll } else { Edit::Deselect })),
            Phase::UndoRedo => Some(Command::App(if i.is_multiple_of(2) { AppCommand::Undo } else { AppCommand::Redo })),
            Phase::MakePrepare => (i == 0).then_some(Command::App(AppCommand::Make)),
            Phase::CutPreview => (i == 0).then_some(Command::App(AppCommand::CutPreview)),
        };
        app.scripted = command;
        Some(phase)
    }
}

/// Frame `i` of a drag of `ids`: the first selects them, and each moves them a step.
fn drag(app: &mut Kerf, ids: &[u64], i: usize) -> Option<Command> {
    if i == 0 {
        app.doc.selection = ids.to_vec();
    }
    let dx_in = if (i / SWAY_FRAMES).is_multiple_of(2) { DRAG_STEP_IN } else { -DRAG_STEP_IN };
    Some(Command::Edit(Edit::Nudge { dx_in, dy_in: 0.0, board_in: app.doc.view.artboard_in }))
}

/// Puts back what a phase opened, so the next starts from the plain canvas.
fn leave(app: &mut Kerf, phase: Phase) {
    match phase {
        Phase::Zoom => app.doc.view.zoom = 1.0,
        Phase::SelectAll => app.doc.selection.clear(),
        Phase::MakePrepare => {
            app.doc.flow = None;
            app.close_device();
        }
        Phase::CutPreview => app.doc.cut_preview = None,
        _ => {}
    }
}

/// Each phase's clock window and frame times, a frame's time taken from the egui clock at the start of the
/// frame after it. At-rest phases keep only their window.
#[derive(Default)]
struct Recorder {
    phases: Vec<(Phase, u64, u64, Vec<f32>)>,
}

impl Recorder {
    fn record(&mut self, ms: f32) {
        if let Some((phase, _, end, times)) = self.phases.last_mut() {
            if !phase.at_rest() {
                times.push(ms);
            }
            *end = now_unix_ms();
        }
    }

    fn enter(&mut self, phase: Phase) {
        if self.phases.last().is_none_or(|(p, ..)| *p != phase) {
            let now = now_unix_ms();
            self.phases.push((phase, now, now, Vec::new()));
        }
    }

    fn phases(&self) -> Vec<PhaseFrames> {
        let row = |(phase, start_unix_ms, end_unix_ms, times): &(Phase, u64, u64, Vec<f32>)| PhaseFrames {
            phase: *phase,
            start_unix_ms: *start_unix_ms,
            end_unix_ms: *end_unix_ms,
            frames: FrameStats::of(times),
        };
        self.phases.iter().map(row).collect()
    }
}

/// `kerf --bench`: the blank app at rest, the project opened and timed, the design at rest, then the
/// script; the metrics are written and the window closes.
pub struct Bench {
    app: Kerf,
    project: PathBuf,
    out: PathBuf,
    stage: Stage,
    recorder: Recorder,
    /// The phase the previous frame counted towards.
    last: Option<Phase>,
}

enum Stage {
    Start { started: Instant },
    Rest { started: Instant, loaded: Loaded },
    Script { script: Script, loaded: Loaded },
    Done,
}

#[derive(Clone, Copy)]
struct Loaded {
    load_ms: f32,
    counts: crate::bench::Counts,
}

impl Bench {
    /// Frames in each scripted phase, and the least wall time any phase lasts.
    pub const FRAMES: usize = 120;
    pub const MIN_PHASE: Duration = Duration::from_secs(3);

    pub fn new(app: Kerf, project: PathBuf, out: PathBuf) -> Self {
        Self { app, project, out, stage: Stage::Start { started: Instant::now() }, recorder: Recorder::default(), last: None }
    }

    /// This frame's phase, after moving the stage on when the current one is over. An at-rest phase asks
    /// to be woken when it is due to end.
    fn next_phase(&mut self, ctx: &Context) -> Option<Phase> {
        let wake = |started: Instant| ctx.request_repaint_after(Self::MIN_PHASE.saturating_sub(started.elapsed()));
        let resting = |started: Instant| started.elapsed() < Self::MIN_PHASE;
        match &mut self.stage {
            Stage::Start { started } if resting(*started) => {
                wake(*started);
                return Some(Phase::Start);
            }
            Stage::Start { .. } => {
                return match self.load() {
                    Ok(loaded) => {
                        let started = Instant::now();
                        self.stage = Stage::Rest { started, loaded };
                        wake(started);
                        Some(Phase::Rest)
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        self.finish(ctx, None)
                    }
                };
            }
            Stage::Rest { started, .. } if resting(*started) => {
                wake(*started);
                return Some(Phase::Rest);
            }
            &mut Stage::Rest { loaded, .. } => self.stage = Stage::Script { script: Script::new(&self.app.doc.design, Self::FRAMES, Self::MIN_PHASE), loaded },
            Stage::Script { .. } | Stage::Done => {}
        }
        let Stage::Script { script, loaded } = &mut self.stage else { return None };
        let Loaded { load_ms, counts } = *loaded;
        match script.step(&mut self.app) {
            Some(phase) => Some(phase),
            None => self.finish(ctx, Some(Metrics { counts, load_ms, phases: self.recorder.phases() })),
        }
    }

    /// Opens the project and evaluates every shape, timed.
    fn load(&mut self) -> Result<Loaded, String> {
        let clock = Instant::now();
        self.app.open_file(&self.project)?;
        crate::bench::evaluate(&self.app.doc.design);
        Ok(Loaded { load_ms: clock.elapsed().as_secs_f32() * 1000.0, counts: crate::bench::counts(&self.app.doc.design) })
    }

    fn finish(&mut self, ctx: &Context, metrics: Option<Metrics>) -> Option<Phase> {
        self.stage = Stage::Done;
        tracing::info!(target: "session", done = metrics.is_some(), "bench finished");
        if let Some(metrics) = metrics {
            let json = serde_json::to_string_pretty(&metrics).expect("metrics serialise");
            if let Err(e) = std::fs::write(&self.out, json) {
                eprintln!("Couldn't write {}: {e}", self.out.display());
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        None
    }
}

impl eframe::App for Bench {
    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.app.on_exit(gl);
    }

    /// A scripted phase asks for the next frame at once, so frames run flat out; an at-rest phase only
    /// asks to be woken when it is over, and otherwise repaints when the app itself asks to.
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.last.is_some() {
            self.recorder.record(ctx.input(|i| i.unstable_dt) * 1000.0);
        }
        self.last = self.next_phase(ctx);
        if let Some(phase) = self.last {
            self.recorder.enter(phase);
            if !phase.at_rest() {
                ctx.request_repaint();
            }
        }
        self.app.update(ctx, frame);
    }
}

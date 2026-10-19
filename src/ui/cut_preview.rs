//! The cut preview: each mat's toolpath animated in machine order, with its time and material use.
//! It plays the strokes `output::mat_toolpath` would send, so what it shows is what the machine does.

use egui::{Align2, Area, Color32, Context, Id, Order, Pos2, Rect, Sense, Shape, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2};

use crate::composition::Mat;
use crate::design::Design;
use crate::machine::MatSize;
use crate::make_flow::{MakeFlow, MatSetup};
use crate::material::{MATERIALS, Material, PressureAdjust};
use crate::output::OutputDevice;
use crate::output::estimate::{self, Estimate, MoveKind, Usage};
use crate::output::profile::{MM_PER_IN, ToolKind};
use crate::output::toolpath::Toolpath;
use crate::units::Units;

use super::chrome::icon_button;
use super::flow_kit::{self as kit, Kind};
use super::fonts;
use super::icons::Icon;
use super::op_style;
use super::tokens::*;
use super::tooltip::tip;

const SPEEDS: [f32; 4] = [1.0, 4.0, 16.0, 64.0];
const CARD_W: f32 = 760.0;
const SHEET_H: f32 = 420.0;
const HEAD_R: f32 = 4.0;
const TRAVEL_DASH: (f32, f32) = (4.0, 4.0);
const ORIGINAL_ALPHA: f32 = 0.4;

/// The open preview: every mat's run worked out once when it opens, and the playback over them. It
/// lives beside the app, never in `MakeFlow` or the design, so playing adds no undo steps.
pub struct CutPreview {
    mats: Vec<MatRun>,
    /// The simulated machine publishes no speeds, so its times are nominal.
    nominal: bool,
    material: &'static str,
    /// Set has not chosen a material yet, so the times assume the first in the list.
    assumed: bool,
    mat: usize,
    pub t: f32,
    pub playing: bool,
    speed: f32,
    show_original: bool,
}

struct MatRun {
    estimate: Estimate,
    /// The outline before blade compensation, when a drag knife changes it.
    original: Option<Toolpath>,
    usage: Usage,
    size: MatSize,
    colour: Color32,
}

/// The Set step's material, or the first in the list before Set has run; true when assumed.
fn material(flow: &MakeFlow) -> (&'static Material, bool) {
    match flow.material {
        Some(m) => (m, false),
        None => (&MATERIALS[0], true),
    }
}

/// How long `mat` takes on the flow's device, from the same toolpath the preview plays.
pub fn mat_estimate(flow: &MakeFlow, mat: &Mat) -> Estimate {
    let (path, rates) = estimate::mat_run(&flow.design, mat, &flow.setup, &flow.device, material(flow).0, flow.pressure);
    estimate::estimate(&path, &rates)
}

/// What each mat's time depends on, held with the times worked out from it.
struct Times {
    design: Design,
    mats: Vec<Mat>,
    setup: MatSetup,
    device: OutputDevice,
    material: Option<&'static str>,
    pressure: PressureAdjust,
    seconds: Vec<f32>,
}

impl Times {
    fn current(&self, flow: &MakeFlow) -> bool {
        (&self.design, &self.mats, self.setup, &self.device, self.material, self.pressure)
            == (&flow.design, &flow.mats, flow.setup, &flow.device, flow.material.map(|m| m.name), flow.pressure)
    }
}

thread_local! {
    static TIMES: std::cell::RefCell<Option<Times>> = const { std::cell::RefCell::new(None) };
}

/// Each mat's estimated seconds. The flow repaints every poll, too often to reorder every mat's
/// strokes each frame, so the times are kept until something they depend on changes. A design holds
/// an `Rc` cache, so it cannot live in egui's memory; the UI thread keeps it instead.
pub fn mat_times(flow: &MakeFlow) -> Vec<f32> {
    TIMES.with_borrow_mut(|cached| {
        if !cached.as_ref().is_some_and(|c| c.current(flow)) {
            *cached = Some(Times {
                design: flow.design.clone(),
                mats: flow.mats.clone(),
                setup: flow.setup,
                device: flow.device.clone(),
                material: flow.material.map(|m| m.name),
                pressure: flow.pressure,
                seconds: flow.mats.iter().map(|m| mat_estimate(flow, m).total_s).collect(),
            });
        }
        cached.as_ref().map(|c| c.seconds.clone()).unwrap_or_default()
    })
}

impl CutPreview {
    /// Opens on the flow's selected mat.
    pub fn new(flow: &MakeFlow) -> Self {
        let (material, assumed) = material(flow);
        let knife = matches!(flow.profile().map(|p| &p.tool), Some(ToolKind::DragKnife { .. }));
        let mats = flow
            .mats
            .iter()
            .map(|mat| MatRun {
                estimate: mat_estimate(flow, mat),
                original: knife.then(|| crate::output::mat_toolpath(&flow.design, mat, &flow.setup, None, 1)),
                usage: estimate::usage(&flow.design, mat, flow.setup.mat_size),
                size: flow.setup.mat_size,
                colour: mat.colour,
            })
            .collect();
        let nominal = matches!(flow.device, OutputDevice::Simulator);
        Self { mats, nominal, material: material.name, assumed, mat: flow.current, t: 0.0, playing: false, speed: SPEEDS[0], show_original: false }
    }

    fn run(&self) -> Option<&MatRun> {
        self.mats.get(self.mat)
    }

    pub fn total_s(&self) -> f32 {
        self.run().map_or(0.0, |r| r.estimate.total_s)
    }

    /// Moves playback on by one frame of `dt` seconds, stopping at the end.
    pub fn advance(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        self.t = (self.t + dt * self.speed).min(self.total_s());
        if self.t >= self.total_s() {
            self.playing = false;
        }
    }

    /// Shows the preview over everything; false once it is closed.
    pub fn show(&mut self, ctx: &Context, units: Units) -> bool {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        self.advance(dt);
        if self.playing {
            ctx.request_repaint();
        }
        let screen = ctx.screen_rect();
        Area::new(Id::new("cut-preview-scrim")).order(Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(28));
        });
        let mut open = !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        Area::new(Id::new("cut-preview")).order(Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            kit::card_frame().inner_margin(egui::Margin::same(20)).show(ui, |ui| {
                kit::themed(ui, |ui| {
                    ui.set_width(CARD_W);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Cut preview").font(fonts::semibold(15.0)).color(TEXT_PRIMARY));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Close, "Close cut preview", true).clicked() {
                                open = false;
                            }
                            self.mat_picker(ui);
                        });
                    });
                    ui.add_space(8.0);
                    self.sheet(ui);
                    ui.add_space(8.0);
                    self.scrubber(ui);
                    ui.add_space(4.0);
                    self.readout(ui, units);
                    ui.add_space(8.0);
                    self.controls(ui);
                });
            });
        });
        open
    }

    fn mat_picker(&mut self, ui: &mut Ui) {
        let count = self.mats.len();
        if count < 2 {
            return;
        }
        let next = kit::button(ui, "Next mat", Kind::Secondary, self.mat + 1 < count);
        if next.clicked() {
            self.pick_mat(self.mat + 1);
        }
        tip(next, "Next mat", "Preview the next mat.", None);
        kit::body_text(ui, format!("Mat {} of {count}", self.mat + 1), false);
        let previous = kit::button(ui, "Previous mat", Kind::Secondary, self.mat > 0);
        if previous.clicked() {
            self.pick_mat(self.mat - 1);
        }
        tip(previous, "Previous mat", "Preview the mat before.", None);
    }

    fn pick_mat(&mut self, mat: usize) {
        tracing::info!(target: "cmd", mat = mat + 1, "cut preview mat");
        (self.mat, self.t, self.playing) = (mat, 0.0, false);
    }

    /// The mat, every move up to the head solid in its operation's style, the rest faint, travel dashed.
    fn sheet(&self, ui: &mut Ui) {
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), SHEET_H), Sense::hover());
        let Some(run) = self.run() else { return };
        let grid = kit::mat_sheet(ui, area.shrink(8.0), run.size.width_in, run.size.height_in);
        let at = |(x, y): (f32, f32)| grid.to_screen((x / MM_PER_IN, y / MM_PER_IN));
        let painter = ui.painter_at(area);
        if self.show_original
            && let Some(original) = &run.original
        {
            for s in &original.strokes {
                let mut points: Vec<Pos2> = s.points.iter().map(|&p| at(p)).collect();
                if s.closed {
                    points.push(points[0]);
                }
                painter.add(Shape::line(points, Stroke::new(1.0_f32, TEXT_PRIMARY.gamma_multiply(ORIGINAL_ALPHA))));
            }
        }
        let (head, now) = estimate::position_at(&run.estimate, self.t);
        let faint = Stroke::new(1.0_f32, TEXT_MUTED.gamma_multiply(0.35));
        for (i, m) in run.estimate.motions.iter().enumerate() {
            match m.kind {
                MoveKind::Travel => {
                    painter.extend(Shape::dashed_line(&[at(m.from), at(m.to)], Stroke::new(1.0_f32, TEXT_MUTED), TRAVEL_DASH.0, TRAVEL_DASH.1));
                }
                MoveKind::Settle => {}
                MoveKind::Work(op) => {
                    let split = match i.cmp(&now) {
                        std::cmp::Ordering::Less => m.to,
                        std::cmp::Ordering::Equal => head,
                        std::cmp::Ordering::Greater => m.from,
                    };
                    if split != m.from {
                        painter.line_segment([at(m.from), at(split)], work_stroke(op, run.colour, grid.scale));
                    }
                    if split != m.to {
                        painter.line_segment([at(split), at(m.to)], faint);
                    }
                }
            }
        }
        painter.circle(at(head), HEAD_R, ACCENT_PRIMARY, Stroke::new(1.5_f32, BG_PANEL));
    }

    fn scrubber(&mut self, ui: &mut Ui) {
        let total = self.total_s();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
        let fraction = if total > 0.0 { self.t / total } else { 0.0 };
        resp.widget_info(|| WidgetInfo::slider(true, f64::from(self.t), "Scrubber"));
        if let Some(p) = resp.interact_pointer_pos() {
            self.t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * total;
            self.playing = false;
        }
        let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
        let painter = ui.painter();
        painter.rect_filled(track, 2, MAT_GREY);
        painter.rect_filled(track.with_max_x(track.left() + track.width() * fraction), 2, ACCENT_PRIMARY);
        painter.circle(pos2(track.left() + track.width() * fraction, track.center().y), 7.0, BG_PANEL, Stroke::new(1.5_f32, ACCENT_PRIMARY));
        tip(resp, "Scrubber", "Drag to any moment of the cut.", None);
    }

    fn readout(&self, ui: &mut Ui, units: Units) {
        let Some(run) = self.run() else { return };
        let est = &run.estimate;
        let time = format!("{} of {}", estimate::clock(self.t), estimate::clock(est.total_s));
        let lengths = format!("Work {}, travel {}", units.len(est.work_mm / MM_PER_IN), units.len(est.travel_mm / MM_PER_IN));
        let u = &run.usage;
        let (x0, y0, x1, y1) = u.bounds_in;
        let used = format!("Material used {} ({} % of mat)", units.size(x1 - x0, y1 - y0), (u.mat_fraction * 100.0).round());
        let mut lines = vec![(time, TEXT_PRIMARY), (lengths, TEXT_MUTED), (used, TEXT_MUTED)];
        if self.nominal {
            lines.push(("Estimate: the simulated machine publishes no speeds, so these times are nominal".into(), TEXT_MUTED));
        }
        if self.assumed {
            lines.push((format!("Timed for {} until you choose a material", self.material), TEXT_MUTED));
        }
        for (line, ink) in lines {
            let resp = ui.label(egui::RichText::new(&line).font(fonts::regular(13.0)).color(ink));
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &line));
        }
    }

    fn controls(&mut self, ui: &mut Ui) {
        let has_original = self.run().is_some_and(|r| r.original.is_some());
        kit::controls(ui, |ui| {
            let play = if self.playing { "Pause" } else { "Play" };
            let resp = kit::button(ui, play, Kind::Primary, self.total_s() > 0.0);
            if resp.clicked() {
                if !self.playing && self.t >= self.total_s() {
                    self.t = 0.0;
                }
                self.playing = !self.playing;
                tracing::info!(target: "cmd", playing = self.playing, "cut preview play");
            }
            tip(resp, play, "Start or stop the animation.", None);
            let resp = kit::button(ui, "Restart", Kind::Secondary, true);
            if resp.clicked() {
                self.t = 0.0;
            }
            tip(resp, "Restart", "Back to the start of the mat.", None);
            ui.add_space(8.0);
            for speed in SPEEDS {
                let label = format!("{speed}x");
                let kind = if self.speed == speed { Kind::Accent } else { Kind::Secondary };
                let resp = kit::button(ui, &label, kind, true);
                if resp.clicked() {
                    self.speed = speed;
                }
                tip(resp, &label, "Playback speed.", None);
            }
        });
        if has_original {
            let resp = kit::toggle_row(ui, "Show original path", &mut self.show_original);
            tip(resp, "Show original path", "Overlay the outline before blade offset compensation.", None);
        }
    }
}

/// A solid stroke in `op`'s made style, as the canvas draws it in As Made.
fn work_stroke(op: crate::design::Operation, colour: Color32, px_per_in: f32) -> Stroke {
    let look = op_style::look(op, colour, crate::pens::Pen::default(), px_per_in, op_style::Mode::AsMade);
    look.edge.map_or(Stroke::new(1.5_f32, TEXT_PRIMARY), |e| Stroke::new(e.width.max(1.5), e.colour))
}

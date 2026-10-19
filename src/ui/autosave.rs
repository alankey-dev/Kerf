//! Autosave, version history and crash recovery as the app drives them. Each frame ticks the scheduler
//! and hands cloned projects to the saver thread; nothing here writes the disk. The title bar's Save
//! state, the recovery dialog, the offer to restore an autosave on open and the Version history panel
//! all hang off [`Saving`], the app's one field for this, and each document's [`DocSave`].

use std::time::Duration;

use egui::Context;
use jiff::tz::TimeZone;

use super::document::Document;
use super::modal::{self, Choice};
use super::{Kerf, recovery, versions_panel};
use crate::design::Design;
use crate::project::{self, Project};
use crate::store::autosave::{Autosave, Due};
use crate::store::saver::{Done, Job, Saver};
use crate::store::session;
use crate::store::{self as store, VersionKind};

/// The wall clock and the zone times show in. Injected so tests read fixed times.
pub(super) struct Clock {
    pub(super) now_ms: fn() -> u64,
    pub(super) zone: TimeZone,
}

impl Clock {
    pub(super) fn system() -> Self {
        let now_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        Self { now_ms, zone: TimeZone::system() }
    }

    /// Monday 12 October 2026, 10:42 UTC.
    #[cfg(test)]
    pub(super) fn fixed() -> Self {
        Self { now_ms: || FIXED_MS, zone: TimeZone::UTC }
    }

    pub(super) fn now(&self) -> u64 {
        (self.now_ms)()
    }

    fn format(&self, ms: u64, pattern: &str) -> String {
        let at = jiff::Timestamp::from_millisecond(ms as i64).unwrap_or_default();
        at.to_zoned(self.zone.clone()).strftime(pattern).to_string()
    }

    /// "10:42".
    pub(super) fn time(&self, ms: u64) -> String {
        self.format(ms, "%H:%M")
    }

    /// "Monday 12 October": absolute, so nothing shown depends on today's date.
    pub(super) fn day(&self, ms: u64) -> String {
        self.format(ms, "%A %-d %B")
    }
}

/// Longer than a kittest step, so a pending write never keeps `Harness::run` from settling.
const MIN_WAKE_S: f64 = 0.5;

#[cfg(test)]
pub(super) const FIXED_MS: u64 = 1_791_801_720_000;

/// Autosave as the app runs it, for every document: the saver thread, the clock, crash recovery.
pub(super) struct Saving {
    pub(super) clock: Clock,
    /// `None` without a projects folder: nothing is kept then.
    saver: Option<Saver>,
    /// The frame time, in seconds, of the last frame.
    time: f64,
    /// Set by a failed background write and cleared by the next success, so the banner shows once.
    failing: bool,
    recovery: Option<recovery::Dialog>,
}

impl Saving {
    /// Reads what the last session left and starts the saver.
    pub(super) fn start(dir: Option<&std::path::Path>, fonts: &crate::system_fonts::FontCatalogue) -> Self {
        let recovery = dir.and_then(|d| recovery::Dialog::new(session::recovery_list(d, session::pid_alive), d));
        let saver = dir.map(|d| Saver::spawn(d.to_owned(), fonts.clone()));
        Self { clock: Clock::system(), saver, time: 0.0, failing: false, recovery }
    }

    fn send(&self, job: Job) {
        if let Some(saver) = &self.saver {
            saver.send(job);
        }
    }

    pub(super) fn saver(&self) -> Option<&Saver> {
        self.saver.as_ref()
    }

    /// The ids of the projects the recovery dialog is still asking about.
    pub(super) fn recovering(&self) -> impl Iterator<Item = &str> {
        self.recovery.iter().flat_map(|d| d.ids())
    }

    /// Writes `doc`'s owed autosave now rather than after quiet, as a tab leaving the foreground or
    /// closing, or the app quitting, does. A second flush with nothing new does nothing.
    pub(super) fn flush(&self, doc: &mut Document) {
        let changed = doc.note_change();
        if doc.autosave.schedule.flush(changed) {
            self.write(doc, false);
        }
    }

    fn write(&self, doc: &mut Document, version: bool) {
        if doc.transient {
            return;
        }
        if !doc.has_unsaved_changes() {
            self.send(Job::ClearAutosave { id: doc.autosave.id.clone() });
            return;
        }
        let now_ms = self.clock.now();
        let project = doc.current_project(now_ms);
        doc.autosave.seq += 1;
        tracing::debug!(target: "save", id = %project.id, seq = doc.autosave.seq, version, "autosave sent");
        self.send(Job::Autosave { project: (&project).into(), seq: doc.autosave.seq });
        if version && doc.project.is_some() {
            self.send(Job::Version { project: (&project).into(), kind: VersionKind::Auto, now_ms });
        }
        doc.autosave.autosaved_at = Some(now_ms);
    }
}

/// One document's autosave: its schedule, what was last sent, and its version history panel.
pub(super) struct DocSave {
    /// The document's id: its project's, or one minted for a canvas that was never saved.
    pub(super) id: String,
    schedule: Autosave,
    /// The design and title the last frame ended with, to see a change from any source.
    seen: (Design, String),
    seq: u64,
    /// When the autosave of the current changes was sent.
    autosaved_at: Option<u64>,
    /// An autosave newer than the project just opened, waiting on Restore or Discard.
    pub(super) offer: Option<Project>,
    pub(super) versions: Option<versions_panel::Panel>,
}

impl DocSave {
    /// The autosave of a document opened as `design` titled `title`, which is its baseline.
    pub(super) fn new(id: String, design: &Design, title: &str) -> Self {
        Self { id, schedule: Autosave::new(0.0), seen: (design.clone(), title.to_owned()), seq: 0, autosaved_at: None, offer: None, versions: None }
    }

    /// The Version history panel's preview, which the canvas shows in place of the design.
    pub(super) fn preview(&self) -> Option<&Design> {
        self.versions.as_ref().and_then(|v| v.preview.as_ref())
    }
}

impl Document {
    /// Whether the design or title changed since the last look. A change makes the autosave stale.
    fn note_change(&mut self) -> bool {
        let changed = self.design != self.autosave.seen.0 || self.title != self.autosave.seen.1;
        if changed {
            self.autosave.seen = (self.design.clone(), self.title.clone());
            self.autosave.autosaved_at = None;
        }
        changed
    }

    /// The design as a project to keep: the open project's identity, or a new one under the canvas's id.
    pub(super) fn current_project(&self, now_ms: u64) -> Project {
        let base = self.project.clone().unwrap_or_else(|| Project { id: self.autosave.id.clone(), ..Project::new(Design::default()) });
        Project { design: self.design.clone(), name: self.title.clone(), modified: now_ms / 1000, ..base }
    }

    /// The title bar's Save state.
    pub(super) fn save_status(&self, clock: &Clock) -> String {
        match self.autosave.autosaved_at {
            _ if self.transient => "Not saved".into(),
            _ if !self.has_unsaved_changes() => "Saved".into(),
            Some(at) => format!("Autosaved {}", clock.time(at)),
            None => "Edited".into(),
        }
    }

    /// Makes `autosave` the canvas as one undo step over what is open.
    pub(super) fn apply_autosave(&mut self, autosave: Project) {
        let mut design = autosave.design;
        design.settle(None);
        let before = std::mem::replace(&mut self.design, design);
        self.title = autosave.name;
        self.history.observe(before, &self.design, false);
    }
}

impl Kerf {
    #[cfg(test)]
    pub(super) fn with_clock(mut self, clock: Clock) -> Self {
        self.saving.clock = clock;
        self
    }

    fn send_session(&self, pid: Option<u32>) {
        self.saving.send(Job::Session(self.session(pid)));
    }

    /// Claims the session for this process, so a crash leaves it to recover.
    pub(super) fn claim_session(&self) {
        self.send_session(Some(std::process::id()));
    }

    /// Ticks the autosave scheduler and reads the saver's results. Runs every frame outside the Make flow,
    /// for the active document only: the others have nothing pending, since leaving the foreground wrote it.
    pub(super) fn autosave_frame(&mut self, ctx: &Context) {
        if self.doc.flow.is_some() {
            return;
        }
        self.poll_saver();
        let now = ctx.input(|i| i.time);
        self.saving.time = now;
        let changed = self.doc.note_change();
        let gesture_open = self.doc.history.gesture_open();
        if let Due::Write { version } = self.doc.autosave.schedule.tick(now, changed, gesture_open) {
            self.saving.write(&mut self.doc, version);
        }
        // An open gesture ends on input, which wakes the frame anyway. egui repaints at once when the wait
        // is under a frame, so a short wait is rounded up: writing a fraction of a second late is harmless.
        if let Some(wait) = self.doc.autosave.schedule.wake_in(now).filter(|_| !gesture_open) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait.max(MIN_WAKE_S)));
        }
    }

    fn poll_saver(&mut self) {
        while let Some(done) = self.saving.saver.as_ref().and_then(Saver::try_recv) {
            match done {
                Done::Autosaved { .. } | Done::Versioned { .. } => self.saving.failing = false,
                Done::Failed { what, error } => {
                    self.doc.autosave.autosaved_at = None;
                    if !std::mem::replace(&mut self.saving.failing, true) {
                        self.report(format!("Couldn't write the {what}: {error}"));
                    }
                }
            }
        }
    }

    /// Writes the canvas as its project through the saver, recording a Manual version.
    pub(super) fn save_project(&mut self, project: &Project) -> std::io::Result<()> {
        let saver = self.saving.saver.as_ref().ok_or_else(|| std::io::Error::other("there is no projects folder"))?;
        saver.save(project, self.saving.clock.now())?;
        self.doc.autosave.schedule.saved(self.saving.time);
        Ok(())
    }

    pub(super) fn now_ms(&self) -> u64 {
        self.saving.clock.now()
    }

    /// The app is quitting on purpose: the next launch offers no recovery.
    pub(super) fn clean_exit(&self) {
        tracing::info!(target: "session", "clean quit");
        self.send_session(None);
    }

    /// The app is quitting: every tab's owed autosave is written, then the session marked clean. Asks
    /// nothing, and running it again changes nothing.
    pub(super) fn quit(&mut self) {
        let Self { doc, shelf, saving, .. } = self;
        for d in shelf.with_active_mut(doc) {
            saving.flush(d);
        }
        self.clean_exit();
    }

    /// After a clean quit, reopens the tabs it left, in order and with the same one active. A saved
    /// project opens from its file and offers a newer autosave; a never-saved one with an autosave opens
    /// blank under its id and offers that; an id with neither is gone, and so is its tab.
    pub(super) fn restore_session(&mut self) {
        let Some(dir) = self.projects_dir.clone() else { return };
        let Some(last) = session::read(&dir).filter(|s| s.pid.is_none()) else { return };
        let mut docs = Vec::new();
        let mut active = 0;
        for (i, id) in last.tabs.iter().enumerate() {
            if i == last.active {
                active = docs.len();
            }
            let doc = if project::json_path(&dir, id).exists() {
                self.load(id).map(|p| {
                    let offer = self.newer_autosave(id, Some(p.modified));
                    let mut doc = Document::open(p);
                    doc.autosave.offer = offer;
                    doc
                })
            } else {
                self.newer_autosave(id, None).map(|autosave| {
                    let mut doc = Document::blank(autosave.name.clone());
                    doc.autosave.id = id.clone();
                    doc.autosave.offer = Some(autosave);
                    doc
                })
            };
            docs.extend(doc);
        }
        tracing::info!(target: "session", tabs = docs.len(), of = last.tabs.len(), "session restored");
        self.restore_tabs(docs, active);
    }

    /// The autosave of `id` when it is at least as new as the file saved at `saved` (unix seconds), or
    /// there is no file.
    pub(super) fn newer_autosave(&self, id: &str, saved: Option<u64>) -> Option<Project> {
        let (Some(dir), Some(saver)) = (&self.projects_dir, &self.saving.saver) else { return None };
        saver.flush();
        match store::read_autosave(dir, id) {
            Ok(autosave) => autosave.filter(|a| saved.is_none_or(|s| a.modified >= s)),
            Err(e) => {
                tracing::warn!(target: "save", id, error = %e, "autosave unreadable");
                None
            }
        }
    }

    /// The recovery dialog and the offer to restore an autosave, ahead of everything else.
    pub(super) fn autosave_dialogs(&mut self, ctx: &Context) {
        self.recovery_dialog(ctx);
        let Some(offer) = &self.doc.autosave.offer else { return };
        let detail = format!("Unsaved changes from {}", self.saving.clock.time(offer.modified * 1000));
        let choices = [Choice { label: "Discard", a11y: "Discard autosave", primary: false }, Choice { label: "Restore", a11y: "Restore autosave", primary: true }];
        let Some(choice) = modal::show(ctx, "autosave-offer", &format!("Restore {}?", offer.name), &detail, &choices, 0) else { return };
        let offer = self.doc.autosave.offer.take().expect("shown above");
        tracing::info!(target: "cmd", id = %offer.id, choice = ["discard", "restore"][choice], "autosave offer");
        if choice == 1 {
            self.doc.apply_autosave(offer);
        } else if self.doc.project.is_some() {
            self.saving.send(Job::DiscardAutosave { id: offer.id, now_ms: self.now_ms() });
        } else {
            self.saving.send(Job::ClearAutosave { id: offer.id });
        }
    }

    fn recovery_dialog(&mut self, ctx: &Context) {
        let Some(dialog) = &mut self.saving.recovery else { return };
        let Some(answer) = dialog.show(ctx, &self.saving.clock) else { return };
        let (chosen, rest) = self.saving.recovery.take().expect("shown above").resolve(answer);
        tracing::info!(target: "cmd", ?answer, chosen = chosen.len(), discarded = rest.len(), "recovery");
        let now_ms = self.now_ms();
        for gone in rest {
            let id = gone.project.id;
            self.saving.send(if gone.saved { Job::DiscardAutosave { id, now_ms } } else { Job::ClearAutosave { id } });
        }
        let mut docs = Vec::new();
        for unsaved in chosen {
            let saved = if unsaved.saved { self.load(&unsaved.project.id) } else { None };
            let mut doc = saved.map_or_else(|| self.blank_under(unsaved.project.id.clone()), Document::open);
            doc.apply_autosave(unsaved.project);
            docs.push(doc);
        }
        if !docs.is_empty() {
            self.show_canvas();
        }
        self.restore_tabs(docs, 0);
    }

    /// File > Version history, over whatever is queued for the saver.
    pub(super) fn open_versions(&mut self) {
        let (Some(dir), Some(saver)) = (&self.projects_dir, &self.saving.saver) else { return };
        saver.flush();
        match versions_panel::Panel::load(dir, &self.doc.autosave.id) {
            Ok(panel) => self.doc.autosave.versions = Some(panel),
            Err(e) => self.report(format!("Couldn't read the version history: {e}")),
        }
    }

    /// The Version history panel, when open. Restore lands here, before `History::observe`, so it is one
    /// undo step. Returns whether the panel is open.
    pub(super) fn versions_frame(&mut self, ctx: &Context) -> bool {
        let Some(panel) = &mut self.doc.autosave.versions else { return false };
        let Some(event) = panel.show(ctx, &self.saving.clock) else { return true };
        tracing::info!(target: "cmd", event = ?event, "version history");
        let Some(dir) = self.projects_dir.clone() else { return true };
        let id = self.doc.autosave.id.clone();
        match event {
            versions_panel::Event::Select(v) => match store::read_version(&dir, &id, v) {
                Ok(p) => panel.select(v, p.design),
                Err(e) => self.report(format!("Couldn't open that version: {e}")),
            },
            versions_panel::Event::Name(version, name) => {
                panel.name(version, &name);
                self.saving.send(Job::NameVersion { id, version, name });
            }
            versions_panel::Event::Close => self.doc.autosave.versions = None,
            versions_panel::Event::Restore(v) => {
                let now_ms = self.now_ms();
                let current = self.doc.current_project(now_ms);
                let restored = self.saving.saver.as_ref().map(|s| s.restore(&current, v, now_ms));
                match restored {
                    Some(Ok(p)) => {
                        self.doc.design = p.design;
                        self.doc.design.settle(None);
                        self.doc.selection.clear();
                        self.open_versions();
                    }
                    Some(Err(e)) => self.report(format!("Couldn't restore that version: {e}")),
                    None => {}
                }
            }
        }
        self.doc.autosave.versions.is_some()
    }
}

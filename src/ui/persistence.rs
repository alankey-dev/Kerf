//! Opening and saving projects. Each document remembers its design as last saved, so it knows what
//! is unsaved; nothing asks before leaving it, since its autosave keeps the work. Failures reach the
//! user through one error banner.

use egui::Context;

use super::document::Document;
use super::{Kerf, home, modal};
use crate::design::Design;
use crate::project::{self, Project};
use crate::store::saver::Job;
use crate::store::unsaved;

/// The design and title as last saved or opened: what "unsaved changes" is measured against.
pub(super) struct Saved {
    design: Design,
    title: String,
}

impl Saved {
    pub(super) fn new(design: &Design, title: &str) -> Self {
        Self { design: design.clone(), title: title.to_owned() }
    }
}

impl Document {
    /// Whether the canvas differs from the project it was opened or last saved as.
    pub(super) fn has_unsaved_changes(&self) -> bool {
        !self.design.eq_closed(&self.saved.design) || self.title != self.saved.title
    }

    pub(super) fn mark_saved(&mut self) {
        self.saved = Saved { design: self.design.clone(), title: self.title.clone() };
    }
}

impl Kerf {
    /// Quits on a close request without asking, and shows the dialogs and the error banner.
    pub(super) fn persistence_dialogs(&mut self, ctx: &Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            self.quit();
        }
        self.autosave_dialogs(ctx);
        self.one_make_dialog(ctx);
        if let Some(message) = &self.error
            && modal::error_banner(ctx, message)
        {
            self.error = None;
        }
    }

    /// Home > Make: the project's tab, and the Make flow on it.
    pub(super) fn open_and_make(&mut self, id: &str) {
        if self.open_tab(id) {
            self.start_make();
        }
    }

    pub(super) fn load(&mut self, id: &str) -> Option<Project> {
        let dir = self.projects_dir.clone()?;
        let (project, repairs) =
            crate::debug_log::project_io("load", &dir, id, project::open(&dir, id)).map_err(|e| self.report(format!("Couldn't open the project: {e}"))).ok()?;
        if !repairs.is_empty() {
            self.report(format!("Opened {} but {repairs}.", project.name));
        }
        Some(project)
    }

    /// Shows `message` in the error banner until the user dismisses it.
    pub(super) fn report(&mut self, message: String) {
        tracing::error!(target: "save", error = %message, "error banner");
        self.error = Some(message);
    }

    /// Every id some tab has, or the recovery dialog is about to open: none of them is an unsaved card.
    fn open_ids(&self) -> Vec<String> {
        self.docs().map(|d| d.autosave.id.clone()).chain(self.saving.recovering().map(str::to_owned)).collect()
    }

    /// Rebuilds Home's lists from disk: the project files and the closed never-saved designs. Called
    /// when Home is entered or something it lists changes, never per frame. Expired unsaved designs are
    /// deleted here, on the saver thread.
    pub(super) fn refresh_home(&mut self) {
        let (Some(dir), Some(saver)) = (&self.projects_dir, self.saving.saver()) else {
            self.home_projects.clear();
            self.home_unsaved.clear();
            return;
        };
        saver.flush();
        self.home_projects = project::list(dir);
        let open = self.open_ids();
        let scan = unsaved::scan(dir, &open, self.now_ms());
        for id in scan.expired {
            saver.send(Job::DeleteUnsaved { id, open: open.clone() });
        }
        self.home_unsaved = scan.listed;
    }

    pub(super) fn home_screen(&mut self, ctx: &Context) {
        let Some(dir) = self.projects_dir.clone() else { return };
        let Some(saver) = self.saving.saver() else { return };
        let now = self.now_ms() / 1000;
        let Some(event) = home::show(ctx, &self.home_projects, &self.home_unsaved, &mut self.home_confirm, now) else { return };
        tracing::info!(target: "cmd", event = ?event, "home");
        match event {
            home::HomeEvent::New => self.new_tab(),
            home::HomeEvent::Edit(id) => _ = self.open_tab(&id),
            home::HomeEvent::Make(id) => self.open_and_make(&id),
            home::HomeEvent::OpenUnsaved(id) => self.open_unsaved(&id),
            home::HomeEvent::DeleteUnsaved(id) => {
                saver.send(Job::DeleteUnsaved { id, open: self.open_ids() });
                self.refresh_home();
            }
            home::HomeEvent::Delete(id) => {
                if let Err(e) = crate::debug_log::project_io("delete", &dir, &id, saver.delete(id.clone())) {
                    self.report(format!("Couldn't delete the project: {e}"));
                }
                self.drop_tabs(|d| d.project.as_ref().is_some_and(|p| p.id == id));
                self.refresh_home();
            }
            home::HomeEvent::ClearAll => {
                if let Err(e) = crate::debug_log::project_io("clear_all", &dir, "*", saver.clear_all()) {
                    self.report(format!("Couldn't delete every project: {e}"));
                }
                self.drop_tabs(|d| d.project.is_some());
                self.refresh_home();
            }
        }
    }

    /// Writes the canvas to its project, creating one on first save. On failure the project keeps its
    /// identity, the old file stays as it was and the banner says why. Returns whether it saved.
    pub(super) fn save_to_disk(&mut self) -> bool {
        let Some(dir) = self.projects_dir.clone() else {
            self.report("Couldn't save: there is no projects folder.".into());
            return false;
        };
        let project = self.doc.current_project(self.now_ms());
        match crate::debug_log::project_io("save", &dir, &project.id, self.save_project(&project)) {
            Ok(()) => {
                self.doc.project = Some(project);
                self.doc.mark_saved();
                true
            }
            Err(e) => {
                self.doc.project.get_or_insert(project);
                self.report(format!("Couldn't save {}: {e}", self.doc.title));
                false
            }
        }
    }
}

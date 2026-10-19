//! Upload (spec 28): an SVG imports as editable shapes on the spot; a PNG, JPG or WebP goes to the image
//! worker and opens the trace panel when the worker answers.

use std::path::Path;

use egui::Context;

use super::Kerf;
use super::trace_panel::TracePanel;
use crate::trace::worker::{Answer, Done, Job, Source};

impl Kerf {
    /// Asks for a file (or takes the injected `upload_file`) and imports it by its extension.
    pub(super) fn upload(&mut self) {
        let picked = self
            .upload_file
            .clone()
            .or_else(|| rfd::FileDialog::new().add_filter("Images", &["svg", "png", "jpg", "jpeg", "webp"]).add_filter("SVG", &["svg"]).pick_file());
        let Some(path) = picked else { return };
        tracing::info!(target: "cmd", path = %path.display(), "upload");
        if is_svg(&path) {
            self.import_svg(&path);
        } else {
            self.image_owner = Some(self.doc.autosave.id.clone());
            self.images.submit(Job::Import(Source::Path(path)));
        }
    }

    /// Takes what the image worker finished, if anything: a decoded upload opens the trace panel, a
    /// preview refreshes it, and a full trace is placed. This runs before `History::observe`, so a
    /// placed trace is one undo step.
    pub(super) fn collect_upload(&mut self, ctx: &Context) {
        let Some(done) = self.images.poll() else { return };
        if self.image_owner.as_deref() != Some(self.doc.autosave.id.as_str()) {
            tracing::info!(target: "session", owner = ?self.image_owner, "image answer for a document no longer open dropped");
            return;
        }
        if let Some((on, target)) = self.doc.pattern_upload.take() {
            return self.pattern_landed(on, target, done);
        }
        let Done { result, .. } = done;
        match (result, self.doc.trace.as_mut()) {
            (Ok(Answer::Imported(imported)), _) => {
                tracing::info!(target: "cmd", asset = %imported.asset, px = ?imported.px, "trace panel open");
                self.doc.trace = Some(TracePanel::open(&imported));
            }
            (Ok(Answer::Preview(preview)), Some(panel)) => panel.landed(ctx, preview),
            (Ok(Answer::Traced(traced)), Some(_)) => self.place_trace(ctx, traced),
            (Ok(Answer::Preview(_) | Answer::Traced(_)), None) => {}
            (Err(e), Some(panel)) if panel.applying() => {
                panel.apply_failed();
                tracing::error!(target: "error", error = %e, "full trace failed");
                self.report(format!("Couldn't trace the image: {e}."));
            }
            (Err(e), Some(_)) => tracing::error!(target: "error", error = %e, "trace preview failed"),
            (Err(e), None) => {
                tracing::error!(target: "error", error = %e, "image upload failed");
                self.report(format!("Couldn't upload the image: {e}."));
            }
        }
    }

    /// An SVG font becomes a writing font (spec 29); any other SVG's paths are added to the canvas.
    fn import_svg(&mut self, path: &Path) {
        if let Some(dir) = self.projects_dir.clone() {
            match self.fonts.install_svg_font(&dir, path) {
                Ok(Some(font)) => {
                    self.doc.view.notice = Some(super::notice::Notice::new(format!("Added writing font {}", font.name), false));
                    return;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(target: "font", path = %path.display(), error = %e, "writing font upload failed");
                    return self.report(format!("Couldn't add the writing font: {e}"));
                }
            }
        }
        match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| crate::svg_import::convert(&b)) {
            Ok(crate::svg_import::Converted { roots, report }) => {
                let added = self.doc.design.add_imported_tree(roots);
                self.doc.selection = added.roots;
                let repairs = self.doc.design.sanitise();
                if !repairs.is_empty() {
                    self.doc.selection.retain(|id| !repairs.dropped.contains(id));
                }
                let strokes: Vec<(u64, f32)> = added.strokes.into_iter().filter(|(id, _)| self.doc.design.shapes.iter().any(|s| s.id == *id)).collect();
                self.doc.view.import = Some(super::import_notice::ImportNotice::new(&report, strokes));
            }
            Err(e) => {
                tracing::error!(target: "error", path = ?path.display().to_string(), error = %e, "svg import failed");
                self.report(format!("Couldn't import the SVG: {e}"));
            }
        }
    }
}

fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case("svg"))
}

/// The image worker for an app keeping projects in `projects_dir`, waking `ctx` when a result lands.
/// Its first act is to delete the images no saved project, autosave or version uses. The worker is the
/// only thread that stores images, so none it stores this session can be collected by mistake.
pub(super) fn worker(ctx: &Context, projects_dir: Option<&Path>) -> crate::trace::worker::Worker {
    let wake = ctx.clone();
    let dir = projects_dir.map(Path::to_owned);
    let collect = move || {
        let Some(dir) = dir else { return };
        match crate::store::collect_asset_garbage(&dir, crate::assets::references) {
            Ok(gone) if !gone.is_empty() => tracing::info!(target: "save", count = gone.len(), "unused images deleted"),
            Ok(_) => {}
            Err(e) => tracing::warn!(target: "save", error = %e, "unused images not collected"),
        }
    };
    crate::trace::worker::Worker::spawn(projects_dir.map(crate::assets::dir), collect, move || wake.request_repaint())
}

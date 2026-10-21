//! Upload (spec 28): an SVG imports as editable shapes on the spot; a PNG, JPG or WebP goes to the image
//! worker and opens the trace panel when the worker answers.

use std::path::Path;

use egui::Context;

use super::Kerf;
use super::trace_panel::TracePanel;
use crate::trace::worker::{Answer, Done, Job, Source};

impl Kerf {
    /// Asks for a file (or takes the injected `upload_file`) and imports it by its extension.
    pub(super) fn upload(&mut self, ctx: &Context) {
        let picked = self
            .upload_file
            .clone()
            .or_else(|| rfd::FileDialog::new().add_filter("Images", &["svg", "png", "jpg", "jpeg", "webp"]).add_filter("SVG", &["svg"]).pick_file());
        let Some(path) = picked else { return };
        tracing::info!(target: "cmd", path = %path.display(), "upload");
        if is_svg(&path) {
            self.import_svg(ctx, &path);
        } else {
            self.image_owner = Some(self.doc.autosave.id.clone());
            self.images.submit(Job::Import(Source::Path(path)));
        }
    }

    /// Takes what the image worker finished, if anything: a decoded upload opens the trace panel, a
    /// preview refreshes it, and a full trace is placed. This runs before `History::observe`, so a
    /// placed trace is one undo step.
    pub(super) fn collect_upload(&mut self, ctx: &Context) {
        self.collect_svg();
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

    /// Reads and parses an SVG on its own thread, so a large or hostile file never freezes the UI. An
    /// SVG font becomes a writing font (spec 29); any other SVG's paths are added to the canvas. The
    /// answer is taken by `collect_upload`.
    fn import_svg(&mut self, ctx: &Context, path: &Path) {
        let (path, probe_font, wake) = (path.to_owned(), self.projects_dir.is_some(), ctx.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let thread_path = path.clone();
        std::thread::Builder::new()
            .name("svg-import".into())
            .spawn(move || {
                drop(tx.send(read_svg(&thread_path, probe_font)));
                wake.request_repaint();
            })
            .expect("spawn svg import thread");
        self.svg_import = Some(SvgJob {
            rx,
            path,
            owner: self.doc.autosave.id.clone(),
            #[cfg(test)]
            held: None,
        });
    }

    /// Blocks until a running SVG import has answered, leaving its result for `collect_svg`. Tests wait
    /// on the thread's own reply; the guard only turns a hang into a failure.
    #[cfg(test)]
    pub(super) fn wait_svg_answered(&mut self) {
        if let Some(job) = self.svg_import.as_mut().filter(|j| j.held.is_none()) {
            job.held = Some(job.rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the SVG import answered promptly"));
        }
    }

    /// Takes the SVG import's answer if it has landed: a font is installed, artwork is added to the
    /// document that was open when Upload ran.
    fn collect_svg(&mut self) {
        let Some(job) = self.svg_import.as_mut() else { return };
        let Some(read) = job.take() else { return };
        let SvgJob { path, owner, .. } = self.svg_import.take().expect("the job was just polled");
        if owner != self.doc.autosave.id {
            tracing::info!(target: "session", owner, "svg import for a document no longer open dropped");
            return;
        }
        match read {
            Ok(Read::Font { text, font }) => {
                let dir = self.projects_dir.clone().expect("a font is only read with a projects directory");
                match self.fonts.install_svg_font(&dir, &path, &text, font) {
                    Ok(font) => self.doc.view.notice = Some(super::notice::Notice::new(format!("Added writing font {}", font.name), false)),
                    Err(e) => {
                        tracing::error!(target: "font", path = %path.display(), error = %e, "writing font upload failed");
                        self.report(format!("Couldn't add the writing font: {e}"));
                    }
                }
            }
            Ok(Read::Art(crate::svg_import::Converted { roots, report })) => {
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

/// An SVG upload being read on its own thread.
pub(super) struct SvgJob {
    rx: std::sync::mpsc::Receiver<Result<Read, String>>,
    path: std::path::PathBuf,
    /// The id of the document Upload ran in; its answer goes there.
    owner: String,
    /// A result a test took off the channel while waiting.
    #[cfg(test)]
    held: Option<Result<Read, String>>,
}

impl SvgJob {
    fn take(&mut self) -> Option<Result<Read, String>> {
        #[cfg(test)]
        if let Some(held) = self.held.take() {
            return Some(held);
        }
        self.rx.try_recv().ok()
    }
}

/// What an SVG file turned out to be.
pub(super) enum Read {
    Font { text: String, font: crate::stroke_font::StrokeFont },
    Art(crate::svg_import::Converted),
}

/// Reads `path` under the SVG size cap and parses it as a writing font (when `probe_font`) or artwork.
/// A file that is neither says why, in the words of the converter.
fn read_svg(path: &Path, probe_font: bool) -> Result<Read, String> {
    use std::io::Read as _;
    let limit = crate::svg_import::MAX_SVG_BYTES as u64;
    let mut bytes = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(limit + 1).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if probe_font
        && let Ok(text) = std::str::from_utf8(&bytes)
        && let Ok(font) = crate::system_fonts::parse_svg_font(text)
    {
        return Ok(Read::Font { text: text.to_owned(), font });
    }
    crate::svg_import::convert(&bytes).map(Read::Art)
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

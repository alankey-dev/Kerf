//! The one image worker thread (spec 28), fed by an `mpsc` channel like the simulated machine. The UI
//! thread never decodes or traces: it submits a [`Job`] and polls for its [`Done`].
//!
//! Every job carries a generation. Submitting a job makes it the latest; the worker skips queued jobs
//! older than the latest, a running trace stops at its next stage once superseded (the [`Cancel`] it
//! holds), and [`Worker::poll`] hands back only the latest job's result, so a stale one never reaches
//! the UI. [`Worker::cancel`] supersedes the running job without queueing another.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use image::RgbaImage;

use super::raster::{Recipe, clean, downscale};
use super::{Cancel, Cancelled, Colours, Palette, TraceSettings, TracedLayer, palette, silhouette, trace};
use crate::assets::{self, AssetId};

/// The long side of the copy the live preview traces.
pub const PREVIEW_PX: u32 = 768;
/// The long side Apply traces at; larger images are traced on a downscaled copy.
pub const FULL_PX: u32 = 4096;

/// Where an upload's bytes come from.
#[derive(Debug)]
pub enum Source {
    Path(PathBuf),
    #[cfg_attr(not(test), expect(dead_code, reason = "drag and drop and paste will hand over bytes"))]
    Bytes(Vec<u8>),
}

/// What Apply makes from the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Product {
    /// One filled Basic Cut shape per colour, grouped.
    CutLayers,
    /// One Basic Cut shape: the Threshold layer, or in Posterise the outline of everything kept.
    SingleCut,
    /// The cleaned image, printed, cut along the outline of everything kept.
    PrintThenCut,
}

/// A trace of `image` after `recipe`, for the preview or for Apply.
#[derive(Debug)]
pub struct TraceJob {
    pub image: Arc<RgbaImage>,
    pub recipe: Recipe,
    pub settings: TraceSettings,
    pub product: Product,
    /// Builds a Posterise palette of this many colours from the cleaned image first and traces with
    /// it, replacing `settings.colours`.
    pub new_palette: Option<u8>,
}

/// What the worker can be asked to do.
#[derive(Debug)]
pub enum Job {
    /// Decode an uploaded image and keep it in the asset store.
    Import(Source),
    /// Trace the preview copy for the trace panel.
    Preview(TraceJob),
    /// Trace for Apply on a copy no larger than [`FULL_PX`]. Print Then Cut also stores the cleaned
    /// copy as an asset.
    Full(TraceJob),
}

/// A finished [`Job::Import`]: the stored asset, its size in pixels, and the decoded image with its
/// preview copy for the trace panel.
pub struct Imported {
    pub asset: AssetId,
    pub px: (u32, u32),
    pub image: Arc<RgbaImage>,
    pub preview: Arc<RgbaImage>,
}

/// A finished [`Job::Preview`]: the cleaned preview to show (removed pixels transparent) and its cut
/// lines in the preview's pixels.
pub struct Preview {
    pub shown: egui::ColorImage,
    pub layers: Vec<TracedLayer>,
    /// The palette `new_palette` asked for.
    pub palette: Option<Palette>,
}

/// A finished [`Job::Full`]: layers in the pixels of the traced copy, `px` in size.
pub struct Traced {
    pub px: (u32, u32),
    pub layers: Vec<TracedLayer>,
    pub product: Product,
    /// Print Then Cut's cleaned copy, stored; it is `px` in size.
    pub stored: Option<Stored>,
}

/// A stored image and its canvas texture, built here so the UI thread only uploads it.
pub struct Stored {
    pub asset: AssetId,
    pub display: egui::ColorImage,
}

pub enum Answer {
    Imported(Imported),
    Preview(Preview),
    Traced(Traced),
}

/// A job's result, tagged with the generation it was submitted as. Errors are messages for the banner.
pub struct Done {
    pub generation: u64,
    pub result: Result<Answer, String>,
}

struct Request {
    generation: u64,
    job: Job,
}

/// How long a test waits on a worker before calling it hung; never reached by a passing run.
#[cfg(test)]
const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(120);

pub struct Worker {
    jobs: Sender<Request>,
    results: Receiver<Done>,
    latest: Arc<AtomicU64>,
    answered: u64,
    /// Results a test took off the channel while waiting, for `poll` to hand out first.
    #[cfg(test)]
    held: Vec<Done>,
}

impl Worker {
    /// Starts the thread. It runs `first` before any job, so work there is ordered before every asset
    /// the worker stores. Imports are stored under `assets_dir`; `wake` runs whenever a result is ready,
    /// so the app repaints to collect it.
    pub fn spawn(assets_dir: Option<PathBuf>, first: impl FnOnce() + Send + 'static, wake: impl Fn() + Send + 'static) -> Self {
        let (jobs, inbox) = mpsc::channel::<Request>();
        let (outbox, results) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let shared = latest.clone();
        std::thread::Builder::new()
            .name("image-worker".into())
            .spawn(move || {
                first();
                while let Ok(Request { generation, job }) = inbox.recv() {
                    let cancel = Cancel::new(shared.clone(), generation);
                    if cancel.cancelled() {
                        tracing::debug!(target: "flow", generation, "image job superseded before it ran");
                        continue;
                    }
                    let result = run(job, assets_dir.as_deref(), &cancel);
                    if outbox.send(Done { generation, result }).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("spawn image worker thread");
        Self {
            jobs,
            results,
            latest,
            answered: 0,
            #[cfg(test)]
            held: Vec::new(),
        }
    }

    /// Queues `job` as the newest generation, superseding everything before it. Returns its generation.
    pub fn submit(&mut self, job: Job) -> u64 {
        let generation = self.latest.fetch_add(1, Ordering::AcqRel) + 1;
        if self.jobs.send(Request { generation, job }).is_err() {
            tracing::error!(target: "error", "the image worker has stopped");
        }
        generation
    }

    /// The latest job's result once it lands. Results of superseded jobs are dropped here.
    pub fn poll(&mut self) -> Option<Done> {
        let latest = self.latest.load(Ordering::Acquire);
        let arrived = self.results.try_iter();
        #[cfg(test)]
        let arrived = std::mem::take(&mut self.held).into_iter().chain(arrived);
        let done = arrived.filter(|d| d.generation == latest).last()?;
        self.answered = done.generation;
        Some(done)
    }

    /// Blocks until the latest job has answered, leaving its result for `poll`. Tests wait on the
    /// worker's own reply instead of stepping frames and sleeping; the guard only turns a hang into a failure.
    #[cfg(test)]
    pub fn wait_answered(&mut self) {
        let latest = self.latest.load(Ordering::Acquire);
        while self.answered != latest && !self.held.iter().any(|d| d.generation == latest) {
            let done = self.results.recv_timeout(HANG_GUARD).expect("the image worker answered");
            self.held.push(done);
        }
    }

    /// Whether the latest job is still to answer.
    pub fn busy(&self) -> bool {
        self.answered != self.latest.load(Ordering::Acquire)
    }

    /// Supersedes whatever is queued or running: a trace stops at its next stage and its result is
    /// dropped. Nothing is left to answer.
    pub fn cancel(&mut self) {
        self.answered = self.latest.fetch_add(1, Ordering::AcqRel) + 1;
    }
}

fn run(job: Job, assets_dir: Option<&Path>, cancel: &Cancel) -> Result<Answer, String> {
    let cancelled = |_: Cancelled| "cancelled".to_owned();
    match job {
        Job::Import(source) => import(source, assets_dir).map(Answer::Imported),
        Job::Preview(job) => preview(job, cancel).map(Answer::Preview).map_err(cancelled),
        Job::Full(job) => full(job, assets_dir, cancel)?.map(Answer::Traced).map_err(cancelled),
    }
}

fn import(source: Source, assets_dir: Option<&Path>) -> Result<Imported, String> {
    let bytes = match source {
        Source::Path(path) => std::fs::read(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?,
        Source::Bytes(bytes) => bytes,
    };
    let img = assets::decode(&bytes).map_err(|e| e.to_string())?;
    let dir = assets_dir.ok_or("there is no projects folder to keep the image in")?;
    let asset = assets::store(dir, &img).map_err(|e| format!("couldn't save the image: {e}"))?;
    let preview = Arc::new(downscale(&img, PREVIEW_PX));
    Ok(Imported { asset, px: img.dimensions(), preview, image: Arc::new(img) })
}

/// The cut lines `product` makes: every colour layer for Cut layers, else one black layer.
fn cut_lines(cleaned: &RgbaImage, settings: &TraceSettings, product: Product, cancel: &Cancel) -> Result<Vec<TracedLayer>, Cancelled> {
    let outline = match (product, &settings.colours) {
        (Product::CutLayers, _) | (Product::SingleCut, Colours::Threshold { .. }) => return trace(cleaned, settings, cancel),
        (Product::SingleCut | Product::PrintThenCut, _) => silhouette(cleaned, settings, cancel)?,
    };
    Ok(if outline.is_empty() { Vec::new() } else { vec![TracedLayer { colour: egui::Color32::BLACK, contours: outline }] })
}

fn preview(job: TraceJob, cancel: &Cancel) -> Result<Preview, Cancelled> {
    let TraceJob { image, recipe, mut settings, product, new_palette } = job;
    let cleaned = clean(&image, &recipe);
    let palette = new_palette.map(|count| palette(&cleaned, count));
    if let Some(p) = &palette {
        settings.colours = Colours::Posterise(p.clone());
    }
    let layers = cut_lines(&cleaned, &settings, product, cancel)?;
    let shown = egui::ColorImage::from_rgba_unmultiplied([cleaned.width() as usize, cleaned.height() as usize], cleaned.as_raw());
    Ok(Preview { shown, layers, palette })
}

/// The outer error is a message for the banner; the inner one a cancel.
fn full(job: TraceJob, assets_dir: Option<&Path>, cancel: &Cancel) -> Result<Result<Traced, Cancelled>, String> {
    let TraceJob { image, recipe, settings, product, .. } = job;
    let copy = downscale(&image, FULL_PX);
    if cancel.cancelled() {
        return Ok(Err(Cancelled));
    }
    let cleaned = clean(&copy, &recipe);
    let layers = match cut_lines(&cleaned, &settings, product, cancel) {
        Ok(layers) => layers,
        Err(c) => return Ok(Err(c)),
    };
    let stored = match product {
        Product::PrintThenCut => {
            let dir = assets_dir.ok_or("there is no projects folder to keep the image in")?;
            let asset = assets::store(dir, &cleaned).map_err(|e| format!("couldn't save the image: {e}"))?;
            Some(Stored { asset, display: assets::display_image(&cleaned) })
        }
        Product::CutLayers | Product::SingleCut => None,
    };
    Ok(Ok(Traced { px: cleaned.dimensions(), layers, product, stored }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::fixtures as fx;
    use crate::trace::raster::Step;

    const DISC: &[u8] = include_bytes!("../../tests/fixtures/trace/disc.png");

    /// Waits for the latest job to answer, then drains whatever else arrives.
    fn settle(worker: &mut Worker) -> Vec<Done> {
        worker.wait_answered();
        worker.poll().into_iter().collect()
    }

    fn imported(done: &Done) -> &Imported {
        match &done.result {
            Ok(Answer::Imported(i)) => i,
            Ok(_) => panic!("not an import"),
            Err(e) => panic!("{e}"),
        }
    }

    fn job(image: RgbaImage, product: Product) -> TraceJob {
        TraceJob { image: Arc::new(image), recipe: Recipe::default(), settings: TraceSettings::default(), product, new_palette: None }
    }

    #[test]
    fn an_import_stores_the_image_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::spawn(Some(dir.path().to_owned()), || {}, || {});
        worker.submit(Job::Import(Source::Bytes(DISC.to_vec())));
        let first = settle(&mut worker);
        let a = imported(&first[0]);
        assert_eq!(a.px, (200, 200));
        assert_eq!(a.image.dimensions(), (200, 200));
        assert_eq!(a.preview.dimensions(), (200, 200), "a small image previews at its own size");
        worker.submit(Job::Import(Source::Bytes(DISC.to_vec())));
        let second = settle(&mut worker);
        assert_eq!(imported(&second[0]).asset, a.asset, "the same image is the same asset");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "one file for both imports");
    }

    #[test]
    fn only_the_newest_generation_reaches_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::spawn(Some(dir.path().to_owned()), || {}, || {});
        let old = worker.submit(Job::Import(Source::Bytes(DISC.to_vec())));
        let new = worker.submit(Job::Import(Source::Bytes(b"not an image".to_vec())));
        assert!(new > old);
        let results = settle(&mut worker);
        assert_eq!(results.iter().map(|d| d.generation).collect::<Vec<_>>(), [new], "the stale result is dropped");
        assert!(results[0].result.is_err());
        assert!(worker.poll().is_none(), "nothing more arrives");
    }

    #[test]
    fn a_stale_preview_is_dropped_when_a_newer_one_was_requested() {
        let mut worker = Worker::spawn(None, || {}, || {});
        let img = fx::load("three");
        let stale = TraceJob { new_palette: Some(3), ..job(img.clone(), Product::CutLayers) };
        let fresh = TraceJob { new_palette: Some(2), ..job(img, Product::CutLayers) };
        let old = worker.submit(Job::Preview(stale));
        let new = worker.submit(Job::Preview(fresh));
        let results = settle(&mut worker);
        assert_eq!(results.iter().map(|d| d.generation).collect::<Vec<_>>(), [new], "generation {old} never reaches the panel");
        let Ok(Answer::Preview(p)) = &results[0].result else { panic!("a preview") };
        assert_eq!(p.palette.as_ref().map(|p| p.swatches.len()), Some(2), "the newer request's palette");
        assert_eq!(p.layers.len(), 2);
        assert!(worker.poll().is_none(), "the stale preview never arrives later either");
    }

    #[test]
    fn a_preview_shows_removed_pixels_transparent() {
        let mut worker = Worker::spawn(None, || {}, || {});
        let recipe = Recipe { steps: vec![Step::RemoveColour { at: (0.0, 0.0), tolerance: 10.0, contiguous: true }], edges: false };
        worker.submit(Job::Preview(TraceJob { recipe, ..job(fx::load("disc"), Product::SingleCut) }));
        let Ok(Answer::Preview(p)) = &settle(&mut worker)[0].result else { panic!("a preview") };
        assert_eq!(p.shown.pixels[0].a(), 0, "the corner was removed");
        assert_eq!(p.shown.pixels[64 * 128 + 30].a(), 255, "the disc stays");
        assert_eq!(p.layers.len(), 1);
    }

    #[test]
    fn full_honours_cancel() {
        let layers = job(fx::three_fixture(1600), Product::CutLayers);
        let cancel = Cancel::new(Arc::new(AtomicU64::new(1)), 0);
        assert!(matches!(full(layers, None, &cancel), Ok(Err(Cancelled))), "a cancelled trace stops");

        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::spawn(Some(dir.path().to_owned()), || {}, || {});
        let ptc = job(fx::three_fixture(1600), Product::PrintThenCut);
        worker.submit(Job::Full(ptc));
        worker.cancel();
        assert!(!worker.busy(), "nothing is left to answer");
        assert!(settle(&mut worker).is_empty());
        let after = worker.submit(Job::Import(Source::Bytes(b"not an image".to_vec())));
        let results = settle(&mut worker);
        assert_eq!(results.iter().map(|d| d.generation).collect::<Vec<_>>(), [after], "the cancelled trace's result is dropped");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "the cancelled Print Then Cut stopped before storing its image");
    }

    #[test]
    fn full_traces_a_copy_capped_at_4096_px_and_print_then_cut_stores_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::spawn(Some(dir.path().to_owned()), || {}, || {});
        let mut wide = RgbaImage::from_pixel(5000, 40, image::Rgba([255, 255, 255, 255]));
        for x in 1000..4000 {
            for y in 0..40 {
                wide.put_pixel(x, y, image::Rgba([255, 255, 255, 0]));
            }
        }
        worker.submit(Job::Full(job(wide, Product::PrintThenCut)));
        let Ok(Answer::Traced(t)) = &settle(&mut worker)[0].result else { panic!("a trace") };
        assert_eq!(t.px, (FULL_PX, 33));
        let stored = t.stored.as_ref().expect("Print Then Cut stores the cleaned image");
        assert_eq!(stored.display.size, [FULL_PX as usize, 33]);
        assert_eq!(assets::load(dir.path(), &stored.asset).unwrap().dimensions(), t.px);
        assert_eq!(t.layers.len(), 1);
        assert_eq!(t.layers[0].contours.len(), 2, "the two kept ends");
    }

    #[test]
    fn an_unreadable_file_comes_back_as_a_message_naming_it() {
        let mut worker = Worker::spawn(None, || {}, || {});
        let path = PathBuf::from("/nonexistent/kerf/photo.png");
        worker.submit(Job::Import(Source::Path(path)));
        let Err(e) = &settle(&mut worker)[0].result else { panic!("a missing file fails") };
        assert!(e.contains("photo.png"), "{e}");
    }
}

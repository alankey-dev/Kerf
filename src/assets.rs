//! The content-addressed asset store (spec 28). Pixels never live in a `Design`, which History snapshots
//! every frame: each imported image is written once to `<projects>/assets/<blake3 hex>.png` and shapes
//! hold its [`AssetId`]. [`Textures`] is the canvas's in-memory cache of the decoded images, filled by a
//! loader thread so the UI thread never decodes.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};

/// 64 lowercase hex characters of a blake3 hash. Parsed at the serde boundary, so a crafted project can
/// never name a path outside the asset folder.
#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetId(String);

impl TryFrom<String> for AssetId {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let hex = s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if hex { Ok(Self(s)) } else { Err(format!("\"{s}\" is not an asset id")) }
    }
}

impl From<AssetId> for String {
    fn from(id: AssetId) -> String {
        id.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The largest image Upload accepts, in pixels.
pub const MAX_PIXELS: u64 = 50_000_000;
/// What a decoder may allocate: a `MAX_PIXELS` RGBA buffer plus room for its own working state.
const MAX_ALLOC: u64 = MAX_PIXELS * 4 + 64 * 1024 * 1024;
/// The canvas texture's long side. Larger images are shown downscaled; the stored file keeps every pixel.
pub const DISPLAY_MAX: u32 = 4096;

#[derive(Debug, PartialEq)]
pub enum DecodeError {
    TooLarge { width: u32, height: u32 },
    Unreadable(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { width, height } => {
                let mp = f64::from(*width) * f64::from(*height) / 1e6;
                write!(f, "the image is {width} x {height} pixels ({mp:.1} megapixels); the limit is {} megapixels", MAX_PIXELS / 1_000_000)
            }
            Self::Unreadable(e) => write!(f, "the file is not a PNG, JPG or WebP image Kerf can read ({e})"),
        }
    }
}

/// Reads the header first and refuses anything over [`MAX_PIXELS`] before allocating, then decodes under
/// `image::Limits` (so a lying header cannot allocate more) and stands the image upright by its EXIF
/// orientation.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, DecodeError> {
    let unreadable = |e: &dyn fmt::Display| DecodeError::Unreadable(e.to_string());
    let reader = || ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| unreadable(&e));
    let (width, height) = reader()?.into_dimensions().map_err(|e| unreadable(&e))?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(DecodeError::TooLarge { width, height });
    }
    let mut reader = reader()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| unreadable(&e))?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| unreadable(&e))?;
    img.apply_orientation(orientation);
    Ok(img.into_rgba8())
}

/// blake3 over the width, the height and the RGBA bytes: the same pixels always get the same id.
pub fn id_of(img: &RgbaImage) -> AssetId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&img.width().to_le_bytes());
    hasher.update(&img.height().to_le_bytes());
    hasher.update(img.as_raw());
    AssetId(hasher.finalize().to_hex().to_string())
}

/// `<projects>/assets`.
pub fn dir(projects_dir: &Path) -> PathBuf {
    projects_dir.join("assets")
}

/// The asset file names `design` uses, Flattened parts and pattern fills included: what spec 33's asset collection keeps.
pub fn references(design: &crate::design::Design) -> Vec<String> {
    ids(design).iter().map(|id| format!("{id}.png")).collect()
}

/// The assets `design` uses, Flattened parts and pattern fills included.
pub fn ids(design: &crate::design::Design) -> Vec<AssetId> {
    fn walk(shapes: &[crate::design::Shape], out: &mut Vec<AssetId>) {
        for s in shapes {
            out.extend(s.pattern.as_ref().map(|p| p.asset.clone()));
            match &s.kind {
                crate::design::ShapeKind::Bitmap { asset, .. } => out.push(asset.clone()),
                crate::design::ShapeKind::Flattened { parts, .. } => walk(parts, out),
                crate::design::ShapeKind::Instance { overrides, .. } => out.extend(overrides.values().filter_map(|o| o.pattern.clone().flatten()).map(|p| p.asset)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&design.shapes, &mut out);
    design.components.iter().for_each(|c| walk(&c.shapes, &mut out));
    out
}

/// Decoded images by asset: what the pure writers and the thumbnail draw from. An asset that is not
/// here prints and exports as its cut line only.
pub type Pixels = HashMap<AssetId, Arc<RgbaImage>>;

/// Decodes `ids` from the store at `dir` for an export or a print sheet. A missing or unreadable asset
/// is logged and left out.
pub fn pixels<'a>(dir: Option<&Path>, ids: impl IntoIterator<Item = &'a AssetId>) -> Pixels {
    let Some(dir) = dir else { return Pixels::new() };
    let mut out = Pixels::new();
    for id in ids {
        if out.contains_key(id) {
            continue;
        }
        match load(dir, id) {
            Ok(img) => _ = out.insert(id.clone(), Arc::new(img)),
            Err(e) => tracing::warn!(target: "error", asset = %id, error = %e, "image asset missing for export"),
        }
    }
    out
}

/// A thumbnail's images' long side. Thumbnails are a few hundred pixels across.
const THUMB_MAX: u32 = 512;

/// The images `design` uses, downscaled for a thumbnail. Assets never change under their id, so each is
/// decoded once per run and kept: every save draws a thumbnail.
pub fn thumbnail_pixels(dir: &Path, design: &crate::design::Design) -> Pixels {
    static CACHE: Mutex<Option<Pixels>> = Mutex::new(None);
    let mut out = Pixels::new();
    for id in ids(design) {
        let cached = CACHE.lock().expect("thumbnail pixels").get_or_insert_default().get(&id).cloned();
        let img = match cached {
            Some(img) => img,
            None => {
                let Ok(img) = load(dir, &id) else { continue };
                let small = Arc::new(fit(img, THUMB_MAX));
                CACHE.lock().expect("thumbnail pixels").get_or_insert_default().insert(id.clone(), small.clone());
                small
            }
        };
        out.insert(id, img);
    }
    out
}

fn path(dir: &Path, id: &AssetId) -> PathBuf {
    dir.join(format!("{id}.png"))
}

/// Writes `<dir>/<id>.png` once, as a PNG with its alpha kept. An existing file is reused, so storing
/// the same image twice writes nothing the second time.
pub fn store(dir: &Path, img: &RgbaImage) -> io::Result<AssetId> {
    let id = id_of(img);
    let file = path(dir, &id);
    if file.exists() {
        return Ok(id);
    }
    std::fs::create_dir_all(dir)?;
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).map_err(io::Error::other)?;
    crate::store::write_atomically(&file, &png)?;
    Ok(id)
}

/// Decodes a stored asset. For the worker and the loader thread only.
pub fn load(dir: &Path, id: &AssetId) -> io::Result<RgbaImage> {
    let bytes = std::fs::read(path(dir, id))?;
    decode(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// The image as the canvas texture: downscaled to [`DISPLAY_MAX`] on the long side, so any upload fits
/// the GPU's texture limit. Runs off the UI thread.
pub fn display_image(img: &RgbaImage) -> egui::ColorImage {
    let shown = fit(img.clone(), DISPLAY_MAX);
    egui::ColorImage::from_rgba_unmultiplied([shown.width() as usize, shown.height() as usize], shown.as_raw())
}

/// `img` downscaled so its long side is at most `max`.
fn fit(img: RgbaImage, max: u32) -> RgbaImage {
    let long = img.width().max(img.height());
    if long <= max {
        return img;
    }
    let scale = f64::from(max) / f64::from(long);
    let size = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
    image::imageops::resize(&img, size(img.width()), size(img.height()), image::imageops::FilterType::Triangle)
}

/// How long a test waits on the loader before calling it hung; never reached by a passing run.
#[cfg(test)]
const HANG_GUARD: std::time::Duration = std::time::Duration::from_secs(120);

/// A texture as the canvas finds it this frame.
#[derive(Clone)]
pub enum Texture {
    Loading,
    Ready(egui::TextureHandle),
    /// The asset file is gone or unreadable; the canvas draws a placeholder.
    Missing,
}

/// Keeps [`Textures`] installed while it lives.
pub struct Installed(egui::Context);

impl Drop for Installed {
    fn drop(&mut self) {
        self.0.data_mut(|d| d.remove::<Arc<Textures>>(egui::Id::NULL));
    }
}

/// The canvas's textures keyed by asset and wrap. The first `get` of one queues it for the loader thread
/// and answers `Loading`; the loader uploads it and asks for a repaint. Installed in the egui context by
/// `Kerf::new`, so painting code reaches it through [`Textures::of`].
pub struct Textures {
    slots: Arc<Mutex<HashMap<Slot, Texture>>>,
    queue: Option<Sender<Slot>>,
    /// Signalled by the loader each time a texture lands, so a test can wait on it.
    #[cfg(test)]
    landed: Arc<std::sync::Condvar>,
}

/// An asset's texture clamps at its edges, so a picture's border pixels never bleed into the opposite
/// edge; a repeating one wraps, for a tiled pattern.
type Slot = (AssetId, Wrap);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Wrap {
    Clamp,
    Repeat,
}

impl Wrap {
    fn options(self) -> egui::TextureOptions {
        match self {
            Self::Clamp => egui::TextureOptions::LINEAR,
            Self::Repeat => egui::TextureOptions::LINEAR_REPEAT,
        }
    }
}

impl Textures {
    /// Starts the loader over `dir`. With no directory every asset is `Missing`. The loader holds the
    /// context, which holds its queue, so the returned guard's drop removes the cache to let it stop.
    #[must_use]
    pub fn install(ctx: &egui::Context, dir: Option<PathBuf>) -> Installed {
        let slots: Arc<Mutex<HashMap<Slot, Texture>>> = Arc::default();
        #[cfg(test)]
        let landed = Arc::<std::sync::Condvar>::default();
        let queue = dir.map(|dir| {
            let (queue, requests) = mpsc::channel::<Slot>();
            let (slots, ctx) = (slots.clone(), ctx.clone());
            #[cfg(test)]
            let landed = landed.clone();
            std::thread::Builder::new()
                .name("asset-loader".into())
                .spawn(move || {
                    for (id, wrap) in requests {
                        let texture = match load(&dir, &id) {
                            Ok(img) => Texture::Ready(ctx.load_texture(id.to_string(), display_image(&img), wrap.options())),
                            Err(e) => {
                                tracing::warn!(target: "error", asset = %id, error = %e, "image asset missing");
                                Texture::Missing
                            }
                        };
                        slots.lock().expect("texture slots").insert((id, wrap), texture);
                        #[cfg(test)]
                        landed.notify_all();
                        ctx.request_repaint();
                    }
                })
                .expect("spawn asset loader thread");
            queue
        });
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::NULL,
                Arc::new(Self {
                    slots,
                    queue,
                    #[cfg(test)]
                    landed,
                }),
            )
        });
        Installed(ctx.clone())
    }

    pub fn of(ctx: &egui::Context) -> Option<Arc<Textures>> {
        ctx.data(|d| d.get_temp::<Arc<Textures>>(egui::Id::NULL))
    }

    pub fn get(&self, id: &AssetId, wrap: Wrap) -> Texture {
        let slot = (id.clone(), wrap);
        let mut slots = self.slots.lock().expect("texture slots");
        if let Some(t) = slots.get(&slot) {
            return t.clone();
        }
        let Some(queue) = &self.queue else { return Texture::Missing };
        let state = if queue.send(slot.clone()).is_ok() { Texture::Loading } else { Texture::Missing };
        slots.insert(slot, state.clone());
        state
    }

    /// Asks for the texture and blocks until the loader has answered, so tests wait on the loader's
    /// own signal instead of sleeping and asking again. The guard only turns a hang into a failure.
    #[cfg(test)]
    pub fn wait_loaded(&self, id: &AssetId, wrap: Wrap) -> Texture {
        let first = self.get(id, wrap);
        if !matches!(first, Texture::Loading) {
            return first;
        }
        let slot = (id.clone(), wrap);
        let guard = self.slots.lock().expect("texture slots");
        let pending = |slots: &mut HashMap<Slot, Texture>| matches!(slots.get(&slot), Some(Texture::Loading) | None);
        let (slots, waited) = self.landed.wait_timeout_while(guard, HANG_GUARD, pending).expect("texture slots");
        assert!(!waited.timed_out(), "the asset loader answered for {id}");
        slots[&slot].clone()
    }

    /// Seeds the cache with an image the caller already decoded, such as a fresh upload, so it shows at
    /// once without a trip to disk.
    pub fn insert(&self, ctx: &egui::Context, id: &AssetId, image: egui::ColorImage, wrap: Wrap) {
        let handle = ctx.load_texture(id.to_string(), image, wrap.options());
        self.slots.lock().expect("texture slots").insert((id.clone(), wrap), Texture::Ready(handle));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn crc(bytes: &[u8]) -> u32 {
        let mut c = 0xffff_ffff_u32;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    /// A PNG whose IHDR claims `width` x `height` but whose data is one empty chunk: a few dozen bytes
    /// that a decoder would try to fill with gigabytes.
    pub(crate) fn lying_png(width: u32, height: u32) -> Vec<u8> {
        let chunk = |kind: &[u8], data: &[u8]| {
            let body: Vec<u8> = [kind, data].concat();
            [&(data.len() as u32).to_be_bytes()[..], &body, &crc(&body).to_be_bytes()].concat()
        };
        let ihdr = [&width.to_be_bytes()[..], &height.to_be_bytes(), &[8, 6, 0, 0, 0]].concat();
        [&b"\x89PNG\r\n\x1a\n"[..], &chunk(b"IHDR", &ihdr), &chunk(b"IDAT", &[]), &chunk(b"IEND", &[])].concat()
    }

    #[test]
    fn a_60_megapixel_header_is_refused_before_decoding() {
        let bytes = lying_png(7746, 7746);
        assert!(bytes.len() < 100, "{} bytes", bytes.len());
        let err = decode(&bytes).unwrap_err();
        assert_eq!(err, DecodeError::TooLarge { width: 7746, height: 7746 });
        let message = err.to_string();
        assert!(message.contains("7746 x 7746") && message.contains("60.0 megapixels"), "{message}");
    }

    #[test]
    fn a_header_under_the_limit_that_lies_about_its_data_is_an_error_not_a_panic() {
        assert!(matches!(decode(&lying_png(4000, 4000)), Err(DecodeError::Unreadable(_))));
        assert!(matches!(decode(b"not an image"), Err(DecodeError::Unreadable(_))));
    }

    #[test]
    fn asset_ids_are_hex_hashes_and_nothing_else_parses() {
        let id = id_of(&RgbaImage::new(2, 2));
        assert_eq!(id.0.len(), 64);
        assert_eq!(AssetId::try_from(id.0.clone()), Ok(id.clone()));
        for bad in ["../../etc/passwd", "", &"A".repeat(64), &"0".repeat(63)] {
            assert!(AssetId::try_from(bad.to_owned()).is_err(), "{bad:?}");
        }
        let json = format!("\"../{}\"", "0".repeat(61));
        assert!(serde_json::from_str::<AssetId>(&json).is_err(), "serde parses through the same check");
    }

    #[test]
    fn storing_an_image_twice_writes_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let img = RgbaImage::from_fn(5, 3, |x, y| image::Rgba([x as u8 * 40, y as u8 * 80, 7, if x == 0 { 0 } else { 255 }]));
        let a = store(dir.path(), &img).unwrap();
        let written = std::fs::metadata(path(dir.path(), &a)).unwrap().modified().unwrap();
        let again = decode(&png(&img)).unwrap();
        let b = store(dir.path(), &again).unwrap();
        assert_eq!(a, b, "the same pixels hash the same");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file, no second copy");
        assert_eq!(std::fs::metadata(path(dir.path(), &b)).unwrap().modified().unwrap(), written, "the file is reused, not rewritten");
        assert_eq!(load(dir.path(), &a).unwrap(), img, "alpha survives the round trip");
    }

    #[test]
    fn a_missing_asset_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path(), &id_of(&RgbaImage::new(1, 1))).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn exif_orientation_stands_the_photo_upright() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trace/rotated-exif.jpg")).unwrap();
        let img = decode(&bytes).unwrap();
        assert_eq!(img.dimensions(), (16, 32), "stored 32 x 16 with orientation 6 (turn 90 degrees clockwise)");
        let [r, _, b, _] = img.get_pixel(8, 4).0;
        assert!(r > 200 && b < 60, "the red left half is now on top: {:?}", img.get_pixel(8, 4));
        let [r, _, b, _] = img.get_pixel(8, 28).0;
        assert!(b > 200 && r < 60, "the blue right half is now below: {:?}", img.get_pixel(8, 28));
    }

    #[test]
    fn display_images_fit_the_texture_limit() {
        let big = RgbaImage::new(DISPLAY_MAX * 2, 10);
        assert_eq!(display_image(&big).size, [DISPLAY_MAX as usize, 5]);
        assert_eq!(display_image(&RgbaImage::new(3, 2)).size, [3, 2]);
    }

    /// Writes the committed images under `tests/fixtures/trace/`:
    /// `cargo test regenerate_trace_fixtures -- --ignored`.
    #[test]
    #[ignore = "rewrites committed fixtures"]
    fn regenerate_trace_fixtures() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/trace");
        std::fs::create_dir_all(&out).unwrap();
        let (white, red) = (image::Rgba([255, 255, 255, 255]), image::Rgba([220, 30, 40, 255]));
        let within = |x: u32, y: u32, (cx, cy): (f32, f32), r: f32| (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) <= r;

        // A red disc on white with a white island inside it: Contiguous off removes the island too.
        let disc = RgbaImage::from_fn(200, 200, |x, y| match (within(x, y, (100.0, 100.0), 70.0), within(x, y, (100.0, 100.0), 15.0)) {
            (true, false) => red,
            _ => white,
        });
        disc.save(out.join("disc.png")).unwrap();

        // Four flat bands and a dark circle: easy to recognise on the canvas, small as a PNG.
        let bands = [[250, 200, 60], [90, 180, 120], [70, 130, 210], [150, 90, 190]];
        let photo = RgbaImage::from_fn(240, 160, |x, y| match within(x, y, (120.0, 80.0), 45.0) {
            true => image::Rgba([40, 40, 50, 255]),
            false => {
                let [r, g, b] = bands[(y / 40) as usize];
                image::Rgba([r, g, b, 255])
            }
        });
        photo.save(out.join("upload-bands.png")).unwrap();

        // 32 x 16, red left half and blue right half, tagged EXIF orientation 6 (turn 90 degrees clockwise).
        let sideways = image::RgbImage::from_fn(32, 16, |x, _| if x < 16 { image::Rgb([230, 20, 20]) } else { image::Rgb([20, 20, 230]) });
        let exif: Vec<u8> = [&b"MM\0*\0\0\0\x08"[..], &[0, 1], &[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0], &[0, 0, 0, 0]].concat();
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95);
        image::ImageEncoder::set_exif_metadata(&mut encoder, exif).unwrap();
        image::ImageEncoder::write_image(encoder, sideways.as_raw(), 32, 16, image::ExtendedColorType::Rgb8).unwrap();
        std::fs::write(out.join("rotated-exif.jpg"), jpeg).unwrap();
    }

    #[test]
    fn the_texture_cache_loads_from_disk_and_leaves_with_its_guard() {
        let dir = tempfile::tempdir().unwrap();
        let id = store(dir.path(), &RgbaImage::new(4, 4)).unwrap();
        let ctx = egui::Context::default();
        let installed = Textures::install(&ctx, Some(dir.path().to_owned()));
        let textures = Textures::of(&ctx).expect("installed");
        assert!(matches!(textures.get(&id, Wrap::Clamp), Texture::Loading), "the first ask queues the load");
        assert!(matches!(textures.wait_loaded(&id, Wrap::Clamp), Texture::Ready(_)), "the loader uploads it");
        assert!(matches!(textures.get(&id, Wrap::Repeat), Texture::Loading), "a repeating texture is its own upload");
        assert!(matches!(textures.wait_loaded(&id, Wrap::Repeat), Texture::Ready(_)));
        let options = |wrap| match textures.get(&id, wrap) {
            Texture::Ready(handle) => ctx.tex_manager().read().meta(handle.id()).map(|m| m.options),
            _ => None,
        };
        assert_eq!(options(Wrap::Repeat).map(|o| o.wrap_mode), Some(egui::TextureWrapMode::Repeat));
        assert_eq!(options(Wrap::Clamp).map(|o| o.wrap_mode), Some(egui::TextureWrapMode::ClampToEdge));
        drop((textures, installed));
        assert!(Textures::of(&ctx).is_none(), "dropping the guard removes the cache, so the loader's queue closes");
    }
}

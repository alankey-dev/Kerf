//! The font families installed on this machine. The app owns a [`FontCatalogue`]: in production one
//! background scan fills it, so start-up and the first frame never wait on disk; until it lands, text
//! in a system family lays out in Inter and the font menu shows "Loading fonts...". Tests inject a
//! fixed catalogue instead. Faces load on first use and stay cached.
//!
//! The catalogue also holds the writing (stroke) fonts: the bundled ones, parsed once per process and
//! always present, and SVG fonts the user uploaded, which can arrive at any time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use fontdb::{Database, Family, Query, Source, Style, Weight};
use ttf_parser::{Face, Tag};

use crate::stroke_font::{StrokeFont, StrokeFontId};
use crate::text::{FaceSource, FontBytes, FontStyle};

/// Listed under Kerf fonts already. An installed copy would be a confusing duplicate, and a system
/// "Inter" would save as the bundled one.
const BUNDLED: [&str; 2] = ["Inter", "Space Grotesk"];

/// Display families bundled for the Monogram Maker's themes (spec 42). Text names them as system
/// families, and they resolve on every machine, before the scan lands and whether or not a copy is
/// installed, so a monogram looks the same everywhere.
pub const DISPLAY: [(&str, &[u8]); 3] = [
    ("DM Serif Display", include_bytes!("../assets/fonts/DMSerifDisplay-Regular.ttf")),
    ("Great Vibes", include_bytes!("../assets/fonts/GreatVibes-Regular.ttf")),
    ("Oswald", include_bytes!("../assets/fonts/Oswald-Bold.ttf")),
];

/// The bundled display families as a catalogue of their own.
fn display() -> &'static Catalogue {
    static DISPLAY_FACES: OnceLock<Catalogue> = OnceLock::new();
    DISPLAY_FACES.get_or_init(|| Catalogue::from_db(fixed_db(&DISPLAY)))
}

/// egui keeps its own copy of a preview font's file, so bigger files (large CJK collections) are not
/// previewed.
const PREVIEW_MAX_BYTES: u64 = 4 << 20;

/// Every catalogue, every scan landing and every writing font added takes the next number, so a cache
/// keyed on it never mixes two sets of fonts.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

fn next_generation() -> u64 {
    GENERATIONS.fetch_add(1, Ordering::Relaxed)
}

thread_local! {
    /// The catalogue text layout reads on this thread. Layout is reached from model code
    /// (`ShapeKind::outline`) with no UI context, so the app installs its catalogue here rather than
    /// threading it through every geometry call.
    static CURRENT: RefCell<FontCatalogue> = RefCell::new(FontCatalogue::empty());
}

/// The system families one app can use. Cloning shares the same catalogue.
#[derive(Clone)]
pub struct FontCatalogue(Arc<Shared>);

struct Shared {
    catalogue: OnceLock<Catalogue>,
    previews: Previews,
    /// Changes whenever layout could: the scan landing or a writing font being added.
    generation: AtomicU64,
    /// Uploaded writing fonts, sorted by name.
    strokes: RwLock<Vec<(StrokeFontId, Arc<StrokeFont>)>>,
    missing: Mutex<MissingCache>,
}

/// The last answer of [`FontCatalogue::missing_for`]. It holds while the catalogue's generation and the
/// families the design names are the same, so a frame that changed neither never asks the catalogue.
#[derive(Default)]
struct MissingCache {
    generation: u64,
    used: Vec<String>,
    missing: Arc<[String]>,
}

/// A writing font as the font menu lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WritingFont {
    pub id: StrokeFontId,
    pub name: String,
}

/// The bundled writing fonts by id, in menu order.
const BUNDLED_STROKES: [(&str, &str); 6] = [
    ("hershey-simplex", include_str!("../assets/fonts/hershey/hershey-simplex.odsstroke.json")),
    ("hershey-duplex", include_str!("../assets/fonts/hershey/hershey-duplex.odsstroke.json")),
    ("hershey-script-complex", include_str!("../assets/fonts/hershey/hershey-script-complex.odsstroke.json")),
    ("hershey-script-simplex", include_str!("../assets/fonts/hershey/hershey-script-simplex.odsstroke.json")),
    ("hershey-gothic-english", include_str!("../assets/fonts/hershey/hershey-gothic-english.odsstroke.json")),
    ("relief-singleline", include_str!("../assets/fonts/relief-singleline/relief-singleline.odsstroke.json")),
];

/// The writing font text falls back to when its own is not available.
pub const FALLBACK_STROKE: &str = BUNDLED_STROKES[0].0;

fn bundled_strokes() -> &'static [(StrokeFontId, Arc<StrokeFont>)] {
    static PARSED: OnceLock<Vec<(StrokeFontId, Arc<StrokeFont>)>> = OnceLock::new();
    PARSED.get_or_init(|| {
        BUNDLED_STROKES
            .iter()
            .map(|(id, json)| (StrokeFontId((*id).to_owned()), Arc::new(StrokeFont::from_json(json).unwrap_or_else(|e| panic!("bundled {id}: {e}")))))
            .collect()
    })
}

/// The id an uploaded SVG font file registers under: `svg-` and its file stem in lowercase words.
pub fn svg_font_id(stem: &str) -> StrokeFontId {
    let words: Vec<String> = stem.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    StrokeFontId(format!("svg-{}", words.join("-")))
}

/// How the font menu's preview faces load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Previews {
    /// On a background thread with one repaint per batch, so scrolling never waits on disk.
    Background,
    /// On the UI thread at once, for in-memory catalogues whose faces cost nothing to read.
    Inline,
}

impl FontCatalogue {
    /// Scans the installed families on a background thread. `ready` runs on that thread once they are
    /// known, for example to repaint the UI.
    pub fn scan(ready: impl FnOnce() + Send + 'static) -> Self {
        let fonts = Self::pending(Previews::Background);
        let landing = fonts.clone();
        std::thread::Builder::new()
            .name("font scan".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                tracing::info!(target: "font", "scan-start");
                let catalogue = Catalogue::from_db(system_db());
                let families = catalogue.families.len();
                landing.land(catalogue);
                tracing::info!(target: "font", families, ms = started.elapsed().as_millis() as u64, "scan-finish");
                ready();
            })
            .expect("spawn the font scan thread");
        fonts
    }

    /// No system families at all: what layout reads on a thread no app has claimed.
    pub fn empty() -> Self {
        Self::fixed(&[])
    }

    /// A ready catalogue of in-memory faces, each listed under the family name given with it.
    pub fn fixed(faces: &[(&str, &'static [u8])]) -> Self {
        let fonts = Self::pending(Previews::Inline);
        fonts.land(Catalogue::from_db(fixed_db(faces)));
        fonts
    }

    /// The catalogue kittests run against: two families that outline differently from Inter.
    #[cfg(test)]
    pub fn fixture() -> Self {
        Self::fixed(&Self::fixture_faces())
    }

    #[cfg(test)]
    fn fixture_faces() -> [(&'static str, &'static [u8]); 2] {
        let grotesk: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/SpaceGrotesk.ttf"));
        let display: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/InterDisplay-SemiBold.ttf"));
        [(FIXTURE_FAMILIES[0], grotesk), (FIXTURE_FAMILIES[1], display)]
    }

    /// A catalogue whose scan has not landed, for [`Self::land_fixture`] to complete.
    #[cfg(test)]
    pub fn scanning() -> Self {
        Self::pending(Previews::Inline)
    }

    /// Lands the fixture families in a [`Self::scanning`] catalogue, as a finished scan would.
    #[cfg(test)]
    pub fn land_fixture(&self) {
        self.land(Catalogue::from_db(fixed_db(&Self::fixture_faces())));
    }

    fn pending(previews: Previews) -> Self {
        Self(Arc::new(Shared {
            catalogue: OnceLock::new(),
            previews,
            generation: AtomicU64::new(next_generation()),
            strokes: RwLock::default(),
            missing: Mutex::default(),
        }))
    }

    fn land(&self, catalogue: Catalogue) {
        if self.0.catalogue.set(catalogue).is_ok() {
            self.0.generation.store(next_generation(), Ordering::Relaxed);
        }
    }

    /// Differs between any two moments whose fonts could lay text out differently: before and after the
    /// scan lands, before and after a writing font is added, and between two catalogues.
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Relaxed)
    }

    /// The writing font `id` names: a bundled one, else one the user added.
    pub fn stroke(&self, id: &StrokeFontId) -> Option<Arc<StrokeFont>> {
        let bundled = bundled_strokes().iter().find(|(b, _)| b == id).map(|(_, f)| f.clone());
        bundled.or_else(|| self.0.strokes.read().unwrap_or_else(PoisonError::into_inner).iter().find(|(u, _)| u == id).map(|(_, f)| f.clone()))
    }

    /// Every writing font: the bundled ones in menu order, then the user's by name.
    pub fn writing(&self) -> Vec<WritingFont> {
        let user = self.0.strokes.read().unwrap_or_else(PoisonError::into_inner);
        bundled_strokes().iter().chain(user.iter()).map(|(id, font)| WritingFont { id: id.clone(), name: font.name.clone() }).collect()
    }

    /// Registers a writing font, replacing any with the same id, so text naming it lays out again.
    pub fn add_stroke_font(&self, id: StrokeFontId, font: Arc<StrokeFont>) {
        let mut user = self.0.strokes.write().unwrap_or_else(PoisonError::into_inner);
        user.retain(|(u, _)| *u != id);
        user.push((id, font));
        user.sort_by_cached_key(|(id, f)| (f.name.to_lowercase(), id.clone()));
        self.0.generation.store(next_generation(), Ordering::Relaxed);
    }

    /// Registers every SVG font in `<projects_dir>/fonts`. A file that fails to parse is logged and
    /// skipped. Returns how many registered.
    pub fn load_user_fonts(&self, projects_dir: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(projects_dir.join(USER_FONTS)) else { return 0 };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("svg"))).collect();
        paths.sort();
        let mut loaded = 0;
        for path in paths {
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| parse_svg_font(&text)) {
                Ok(font) => {
                    let id = svg_font_id(&file_stem(&path));
                    tracing::info!(target: "font", id = %id.0, name = %font.name, "writing font loaded");
                    self.add_stroke_font(id, Arc::new(font));
                    loaded += 1;
                }
                Err(e) => tracing::error!(target: "font", path = %path.display(), error = %e, "writing font skipped"),
            }
        }
        loaded
    }

    /// Adds the SVG font at `source` to the user's writing fonts: copies it into `<projects_dir>/fonts`
    /// and registers it. `Ok(None)` when the file is SVG artwork rather than a font.
    pub fn install_svg_font(&self, projects_dir: &Path, source: &Path) -> Result<Option<WritingFont>, String> {
        let text = std::fs::read_to_string(source).map_err(|e| e.to_string())?;
        let Ok(font) = parse_svg_font(&text) else { return Ok(None) };
        let stem = file_stem(source);
        let dir = projects_dir.join(USER_FONTS);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{stem}.svg")), &text).map_err(|e| e.to_string())?;
        let added = WritingFont { id: svg_font_id(&stem), name: font.name.clone() };
        tracing::info!(target: "font", id = %added.id.0, name = %added.name, "writing font added");
        self.add_stroke_font(added.id.clone(), Arc::new(font));
        Ok(Some(added))
    }

    pub fn previews(&self) -> Previews {
        self.0.previews
    }

    /// Blocks until the catalogue has landed, and returns its families.
    pub fn wait(&self) -> &[String] {
        &self.0.catalogue.wait().families
    }

    /// Blocks until the scan lands when `design` has text in a system family, so Make never cuts Inter
    /// in place of a family that is still loading.
    pub fn settle_for(&self, design: &crate::design::Design) {
        if design.shapes.iter().any(|s| s.kind.system_family().is_some()) {
            self.wait();
        }
    }

    /// Whether `family` is a system family this catalogue does not hold, so text set in it lays out in
    /// Inter. False while the scan is still running, when nothing can be said yet.
    pub fn is_missing(&self, family: &str) -> bool {
        self.0.catalogue.get().is_some() && self.face(family, FontStyle::Regular).is_none()
    }

    /// The system families `design`'s text names that are not installed, sorted, each once. Empty until
    /// the scan has landed. Cheap enough for every frame: the answer is kept until the catalogue's
    /// generation or the set of families in the design changes.
    pub fn missing_for(&self, design: &crate::design::Design) -> Arc<[String]> {
        let mut used: Vec<&str> = design.shapes.iter().filter_map(|s| s.kind.system_family()).collect();
        used.sort_unstable();
        used.dedup();
        let generation = self.generation();
        let mut cache = self.0.missing.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.generation != generation || !cache.used.iter().map(String::as_str).eq(used.iter().copied()) {
            cache.missing = used.iter().copied().filter(|family| self.is_missing(family)).map(str::to_owned).collect();
            cache.used = used.iter().map(|&family| family.to_owned()).collect();
            cache.generation = generation;
        }
        cache.missing.clone()
    }

    /// Makes this the catalogue text layout reads on the calling thread.
    pub fn make_current(&self) {
        CURRENT.with_borrow_mut(|current| {
            if !Arc::ptr_eq(&current.0, &self.0) {
                *current = self.clone();
            }
        });
    }

    /// The installed families, or `None` while the scan is still running.
    pub fn families(&self) -> Option<&[String]> {
        self.0.catalogue.get().map(|c| c.families.as_slice())
    }

    /// The installed face of `family` closest to `style`. `None` when the family is not installed or
    /// the scan has not finished.
    pub fn face(&self, family: &str, style: FontStyle) -> Option<FaceSource> {
        if DISPLAY.iter().any(|(name, _)| *name == family) {
            return display().resolve(&mut HashMap::new(), family, style);
        }
        let catalogue = self.0.catalogue.get()?;
        let mut faces = catalogue.faces.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = faces.resolved.get(family).and_then(|styles| styles.get(&style)) {
            return found.clone();
        }
        let found = catalogue.resolve(&mut faces.files, family, style);
        faces.resolved.entry(family.to_owned()).or_default().insert(style, found.clone());
        found
    }

    /// `family`'s regular face, to draw its name in the font menu. `None` for large files and for
    /// faces missing a letter of the name. A scanned catalogue reads from disk here, so call it off
    /// the UI thread unless [`Self::previews`] is `Inline`.
    pub fn preview(&self, family: &str) -> Option<PreviewFace> {
        let catalogue = self.0.catalogue.get()?;
        let info = catalogue.db.face(catalogue.db.query(&Query { families: &[Family::Name(family)], ..Query::default() })?)?;
        let bytes = match &info.source {
            Source::File(path) => {
                if std::fs::metadata(path).ok()?.len() > PREVIEW_MAX_BYTES {
                    return None;
                }
                std::fs::read(path).ok()?
            }
            Source::Binary(data) | Source::SharedFile(_, data) => (**data).as_ref().to_vec(),
        };
        let face = Face::parse(&bytes, info.index).ok()?;
        family.chars().filter(|c| !c.is_whitespace()).all(|c| face.glyph_index(c).is_some()).then_some(())?;
        Some(PreviewFace { bytes, index: info.index })
    }
}

/// Where uploaded writing fonts are kept, under the projects directory.
const USER_FONTS: &str = "fonts";

/// An SVG font with at least one glyph.
fn parse_svg_font(text: &str) -> Result<StrokeFont, String> {
    let font = StrokeFont::from_svg_font(text)?;
    if font.glyph_count() == 0 {
        return Err("the font has no glyphs".into());
    }
    Ok(font)
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(|| "font".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The families [`FontCatalogue::fixture`] lists.
#[cfg(test)]
pub const FIXTURE_FAMILIES: [&str; 2] = ["Fixture Grotesk", "Fixture Display"];

/// The catalogue text layout reads on this thread: the app's, once one has started here.
pub fn current() -> FontCatalogue {
    CURRENT.with_borrow(Clone::clone)
}

/// [`FontCatalogue::face`] on this thread's catalogue.
pub fn face(family: &str, style: FontStyle) -> Option<FaceSource> {
    CURRENT.with_borrow(|fonts| fonts.face(family, style))
}

/// [`FontCatalogue::is_missing`] on this thread's catalogue.
pub fn is_missing(family: &str) -> bool {
    CURRENT.with_borrow(|fonts| fonts.is_missing(family))
}

/// [`FontCatalogue::missing_for`] on this thread's catalogue.
pub fn missing_for(design: &crate::design::Design) -> Arc<[String]> {
    CURRENT.with_borrow(|fonts| fonts.missing_for(design))
}

/// [`FontCatalogue::stroke`] on this thread's catalogue.
pub fn stroke(id: &StrokeFontId) -> Option<Arc<StrokeFont>> {
    CURRENT.with_borrow(|fonts| fonts.stroke(id))
}

/// A whole font file, to hand to egui, and which face in it to use.
pub struct PreviewFace {
    pub bytes: Vec<u8>,
    pub index: u32,
}

struct Catalogue {
    db: Database,
    /// Family names for the menu, sorted case-insensitively.
    families: Vec<String>,
    faces: Mutex<Faces>,
}

#[derive(Default)]
struct Faces {
    /// File contents by path, shared by the faces of one collection.
    files: HashMap<PathBuf, FontBytes>,
    /// Resolved faces by family and style. `None` caches a family that is not installed.
    resolved: HashMap<String, HashMap<FontStyle, Option<FaceSource>>>,
}

/// The operating systems [`font_dirs`] knows the font folders of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Mac,
    Linux,
}

impl Platform {
    const HOST: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Linux
    };
}

/// The folders installed fonts live in on `platform`, reading its environment through `var`. A folder that
/// does not exist is skipped by the scan. macOS also keeps downloaded fonts under
/// `/System/Library/AssetsV2`, which [`system_db`] lists itself.
fn font_dirs(platform: Platform, var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let var = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = var("HOME");
    match platform {
        Platform::Windows => {
            let windows = var("WINDIR").or_else(|| var("SYSTEMROOT")).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let local = var("LOCALAPPDATA").or_else(|| var("USERPROFILE").map(|p| p.join("AppData").join("Local")));
            [Some(windows.join("Fonts")), local.map(|l| l.join("Microsoft").join("Windows").join("Fonts"))].into_iter().flatten().collect()
        }
        Platform::Mac => {
            let mut dirs: Vec<PathBuf> = ["/Library/Fonts", "/System/Library/Fonts", "/Network/Library/Fonts"].map(PathBuf::from).into();
            dirs.extend(home.map(|h| h.join("Library/Fonts")));
            dirs
        }
        Platform::Linux => {
            let mut dirs: Vec<PathBuf> = ["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from).into();
            dirs.extend(var("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local/share"))).map(|d| d.join("fonts")));
            dirs
        }
    }
}

/// macOS's downloaded fonts: one `com_apple_MobileAsset_Font*` folder per font asset, whose names
/// change between releases.
fn macos_asset_dirs() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/System/Library/AssetsV2") else { return Vec::new() };
    entries.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("com_apple_MobileAsset_Font")).map(|e| e.path()).collect()
}

fn system_db() -> Database {
    let mut dirs = font_dirs(Platform::HOST, |name| std::env::var_os(name));
    if Platform::HOST == Platform::Mac {
        dirs.extend(macos_asset_dirs());
    }
    let mut db = Database::new();
    for dir in &dirs {
        db.load_fonts_dir(dir);
    }
    tracing::info!(target: "font", faces = db.len(), dirs = ?dirs, "system fonts found");
    db
}

fn fixed_db(faces: &[(&str, &'static [u8])]) -> Database {
    let mut db = Database::new();
    for &(family, bytes) in faces {
        for id in db.load_font_source(Source::Binary(Arc::new(bytes))) {
            let mut info = db.face(id).expect("just loaded").clone();
            db.remove_face(id);
            info.families = vec![(family.to_owned(), fontdb::Language::English_UnitedStates)];
            db.push_face_info(info);
        }
    }
    db
}

impl Catalogue {
    fn from_db(mut db: Database) -> Self {
        let unusable: Vec<_> = db.faces().filter(|f| !db.with_face_data(f.id, has_outlines).unwrap_or(false)).map(|f| f.id).collect();
        for id in unusable {
            db.remove_face(id);
        }
        let mut families: Vec<String> = db
            .faces()
            .filter_map(|f| f.families.first())
            .map(|(name, _)| name.clone())
            .filter(|name| !name.starts_with('.') && !BUNDLED.contains(&name.as_str()))
            .collect();
        families.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
        families.dedup();
        Self { db, families, faces: Mutex::default() }
    }

    /// CSS-style closest match on weight and italic, then whatever the face lacks is synthesised: the
    /// `wght` axis of a variable face, else synthetic bold, and the italic shear for an upright face.
    fn resolve(&self, files: &mut HashMap<PathBuf, FontBytes>, family: &str, style: FontStyle) -> Option<FaceSource> {
        let wanted = style.weight().wght();
        let slant = if style.italic() { Style::Italic } else { Style::Normal };
        let query = Query { families: &[Family::Name(family)], weight: Weight(wanted as u16), style: slant, ..Query::default() };
        let info = self.db.face(self.db.query(&query)?)?;
        let bytes = match &info.source {
            Source::Binary(data) | Source::SharedFile(_, data) => data.clone(),
            Source::File(path) => match files.get(path) {
                Some(bytes) => bytes.clone(),
                None => {
                    let bytes: FontBytes = Arc::new(std::fs::read(path).ok()?);
                    files.insert(path.clone(), bytes.clone());
                    bytes
                }
            },
        };
        let face = Face::parse((*bytes).as_ref(), info.index).ok()?;
        let wght = face.variation_axes().into_iter().find(|a| a.tag == Tag::from_bytes(b"wght")).map(|a| wanted.clamp(a.min_value, a.max_value));
        Some(FaceSource {
            index: info.index,
            wght,
            embolden: wght.is_none() && wanted >= 600.0 && info.weight.0 < 600,
            shear: style.italic() && info.style == Style::Normal,
            bytes,
        })
    }
}

/// Whether text layout can cut this face: vector glyphs, and no colour bitmaps standing in for them
/// as in Apple Color Emoji, whose `glyf` table holds only placeholder boxes.
fn has_outlines(data: &[u8], index: u32) -> bool {
    let Ok(face) = Face::parse(data, index) else { return false };
    let tables = face.tables();
    (tables.glyf.is_some() || tables.cff.is_some() || tables.cff2.is_some()) && tables.sbix.is_none() && tables.cbdt.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Contour, Design, ShapeKind};
    use crate::text::{FontRef, TextSpec, layout};

    /// This machine's fonts, scanned once for the tests that need real installed families.
    fn system() -> &'static FontCatalogue {
        static SYSTEM: OnceLock<FontCatalogue> = OnceLock::new();
        let fonts = SYSTEM.get_or_init(|| FontCatalogue::scan(|| {}));
        fonts.wait();
        fonts.make_current();
        fonts
    }

    fn project(font: &str) -> String {
        format!(
            r#"{{"shapes":[{{"id":1,"kind":{{"Text":{{"content":"Hi","font":"{font}","size_in":1.0}}}},"x_in":2.0,"y_in":2.0,"width_in":1.2,"height_in":1.1,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"Pen","hidden":false,"contour_hidden":[]}}]}}"#
        )
    }

    fn text_font(design: &Design) -> &FontRef {
        let ShapeKind::Text { font, .. } = &design.shapes[0].kind else { panic!("not text") };
        font
    }

    fn outlines(font: FontRef, style: FontStyle) -> Vec<Contour> {
        layout(&TextSpec { content: "Hamburgefonts", font: &font, style, ..Default::default() }).expect("lays out").contours()
    }

    /// The first of `names` installed here, or `None` (with a note) so the test can skip.
    fn installed(names: &[&str]) -> Option<String> {
        let found = names.iter().find(|n| system().wait().iter().any(|f| f == *n)).map(|n| n.to_string());
        if found.is_none() {
            eprintln!("skipped: none of {names:?} is installed");
        }
        found
    }

    fn text_in(design: &mut Design, family: &str) -> u64 {
        design.add_text("Hi".into(), FontRef::System(family.into()), 1.0, egui::Color32::BLACK, 1.0, 1.0)
    }

    #[test]
    fn missing_for_lists_each_family_the_design_names_and_the_catalogue_lacks() {
        let fonts = FontCatalogue::scanning();
        let mut design = Design::default();
        let ids: Vec<u64> = ["Zapfino", FIXTURE_FAMILIES[0], "Zapfino", "Papyrus"].iter().map(|family| text_in(&mut design, family)).collect();
        assert!(fonts.missing_for(&design).is_empty(), "nothing can be said while the scan runs");
        assert!(!fonts.is_missing("Zapfino"));

        fonts.land_fixture();
        assert_eq!(*fonts.missing_for(&design), ["Papyrus".to_owned(), "Zapfino".to_owned()], "sorted, each once, installed families left out");
        assert!(fonts.is_missing("Zapfino") && !fonts.is_missing(FIXTURE_FAMILIES[0]));

        design.shape_mut(ids[0]).unwrap().kind = ShapeKind::Square;
        assert_eq!(fonts.missing_for(&design).len(), 2, "one Zapfino text is still there");
        design.shape_mut(ids[2]).unwrap().kind = ShapeKind::Square;
        assert_eq!(*fonts.missing_for(&design), ["Papyrus".to_owned()], "the answer follows the design");
    }

    #[test]
    fn missing_for_is_kept_until_the_design_or_the_catalogue_changes() {
        let fonts = FontCatalogue::fixture();
        let mut design = Design::default();
        text_in(&mut design, "Zapfino");
        let first = fonts.missing_for(&design);
        assert!(Arc::ptr_eq(&first, &fonts.missing_for(&design)), "an unchanged design is answered from the cache");
        let mut other = design.clone();
        text_in(&mut other, "Papyrus");
        assert!(!Arc::ptr_eq(&first, &fonts.missing_for(&other)), "a new family is looked up");
    }

    #[test]
    fn projects_saved_before_system_fonts_load_the_bundled_fonts() {
        for (saved, bundled) in [("Inter", FontRef::Inter), ("SpaceGrotesk", FontRef::SpaceGrotesk)] {
            let design: Design = serde_json::from_str(&project(saved)).expect("old projects load");
            assert_eq!(text_font(&design), &bundled);
            assert!(serde_json::to_string(&design).unwrap().contains(&format!(r#""font":"{saved}""#)), "{saved} saves as before");
        }
    }

    #[test]
    fn a_missing_family_outlines_in_inter_and_keeps_its_name() {
        FontCatalogue::fixture().make_current();
        let design: Design = serde_json::from_str(&project("Not A Real Family 15")).expect("loads");
        assert_eq!(text_font(&design), &FontRef::System("Not A Real Family 15".into()));
        let inter = layout(&TextSpec { content: "Hi", ..Default::default() }).unwrap();
        assert_eq!(design.shapes[0].kind.text_layout().unwrap(), inter, "renders with Inter");
        let saved = serde_json::to_string(&design).unwrap();
        assert!(saved.contains(r#""font":"Not A Real Family 15""#), "the name round-trips: {saved}");
    }

    #[test]
    fn layout_changes_once_the_catalogue_lands() {
        let family = FontRef::System(FIXTURE_FAMILIES[0].into());
        let fonts = FontCatalogue::pending(Previews::Inline);
        fonts.make_current();
        assert!(fonts.families().is_none(), "still scanning");
        let pending = fonts.generation();
        let scanning = outlines(family.clone(), FontStyle::Regular);
        assert_eq!(scanning, outlines(FontRef::Inter, FontStyle::Regular), "a scanning catalogue lays out in Inter");

        let fixture = FontCatalogue::fixture();
        fonts.land(Catalogue::from_db(fixed_db(&[(FIXTURE_FAMILIES[0], include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/SpaceGrotesk.ttf")))])));
        assert!(fonts.families().is_some(), "landed");
        assert_ne!(fonts.generation(), pending, "landing changes the generation");
        assert_ne!(fixture.generation(), fonts.generation(), "two catalogues never share a generation");
        assert_ne!(outlines(family, FontStyle::Regular), scanning, "the same text outlines in its own face once ready");
    }

    #[test]
    fn make_waits_for_a_pending_scan_only_when_the_design_has_system_text() {
        let fonts = FontCatalogue::pending(Previews::Inline);
        fonts.settle_for(&serde_json::from_str(&project("Inter")).unwrap());
        assert!(fonts.families().is_none(), "a design without system text never waits");

        let design: Design = serde_json::from_str(&project(FIXTURE_FAMILIES[0])).unwrap();
        let landing = fonts.clone();
        let lands = std::thread::spawn(move || landing.land(Catalogue::from_db(Database::new())));
        fonts.settle_for(&design);
        assert!(fonts.families().is_some(), "Make waits until the catalogue lands");
        lands.join().unwrap();
    }

    #[test]
    fn each_thread_reads_its_own_catalogue() {
        FontCatalogue::fixture().make_current();
        assert!(face(FIXTURE_FAMILIES[0], FontStyle::Regular).is_some());
        let elsewhere = std::thread::spawn(|| face(FIXTURE_FAMILIES[0], FontStyle::Regular).is_some()).join().unwrap();
        assert!(!elsewhere, "a thread no app has claimed has no system fonts");
    }

    #[test]
    fn a_common_system_family_outlines_differently_from_inter() {
        let Some(family) = installed(&["Helvetica", "Arial", "DejaVu Sans", "Liberation Sans"]) else { return };
        let system = outlines(FontRef::System(family.clone()), FontStyle::Regular);
        assert!(!system.is_empty());
        assert_ne!(system, outlines(FontRef::Inter, FontStyle::Regular), "{family} is not Inter");
    }

    #[test]
    fn styles_resolve_to_the_closest_installed_face() {
        let Some(family) = installed(&["Helvetica", "Arial"]) else { return };
        let bold = system().face(&family, FontStyle::Bold).expect("installed");
        assert!(!bold.embolden, "{family} has a real bold face");
        assert!(!system().face(&family, FontStyle::Italic).unwrap().shear, "{family} has an italic or oblique face");
        assert_ne!(outlines(FontRef::System(family.clone()), FontStyle::Bold), outlines(FontRef::System(family), FontStyle::Regular));
    }

    #[test]
    fn a_family_without_a_bold_face_is_emboldened() {
        let fonts = system();
        let Some(family) = fonts.wait().iter().find(|f| fonts.face(f, FontStyle::Bold).is_some_and(|s| s.embolden)) else {
            eprintln!("skipped: every installed family has a bold face");
            return;
        };
        let regular = outlines(FontRef::System(family.clone()), FontStyle::Regular);
        assert_ne!(outlines(FontRef::System(family.clone()), FontStyle::Bold), regular, "{family} bold differs");
    }

    #[test]
    fn the_scan_lists_sorted_cuttable_families_only() {
        let families = system().wait();
        assert!(!families.iter().any(|f| f.starts_with('.') || f == "Apple Color Emoji" || f == "Inter"), "hidden, bitmap and bundled families are left out");
        assert!(families.windows(2).all(|w| w[0].to_lowercase() <= w[1].to_lowercase()), "sorted alphabetically");
    }

    /// An SVG font whose "H" is two crossing strokes.
    const SVG_FONT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><font id="Quill" horiz-adv-x="600">
<font-face font-family="Quill Hand" units-per-em="1000" ascent="800" descent="-200"/>
<glyph unicode="H" d="M100 0 L100 700 M500 0 L500 700 M100 350 L500 350"/>
<glyph unicode="i" horiz-adv-x="200" d="M100 0 L100 500"/>
</font></defs></svg>"#;

    fn hershey_text_layout(content: &str) -> crate::text::TextLayout {
        layout(&TextSpec { content, font: &FontRef::Stroke(StrokeFontId(FALLBACK_STROKE.into())), ..Default::default() }).unwrap()
    }

    #[test]
    fn a_project_with_a_writing_font_round_trips_and_an_unknown_one_renders_in_hershey_simplex() {
        FontCatalogue::fixture().make_current();
        let design: Design = serde_json::from_str(&project("stroke:hershey-simplex")).expect("loads");
        assert_eq!(text_font(&design), &FontRef::Stroke(StrokeFontId("hershey-simplex".into())));
        assert!(serde_json::to_string(&design).unwrap().contains(r#""font":"stroke:hershey-simplex""#), "saves as it was read");

        let unknown: Design = serde_json::from_str(&project("stroke:svg-gone-hand")).expect("loads");
        assert_eq!(unknown.shapes[0].kind.text_layout().unwrap(), hershey_text_layout("Hi"), "renders in Hershey Simplex");
        assert!(serde_json::to_string(&unknown).unwrap().contains(r#""font":"stroke:svg-gone-hand""#), "keeps its name");

        let old: Design = serde_json::from_str(&project("Inter")).expect("an old project loads");
        assert_eq!(text_font(&old), &FontRef::Inter);
    }

    #[test]
    fn adding_a_writing_font_refreshes_text_that_named_it() {
        let fonts = FontCatalogue::fixture();
        fonts.make_current();
        let id = svg_font_id("Quill Hand");
        let mut d = Design::default();
        let text = d.add_text("Hi".into(), FontRef::Stroke(id.clone()), 1.0, egui::Color32::BLACK, 1.0, 1.0);
        let shape = d.shapes.iter().find(|s| s.id == text).unwrap().clone();
        let fallback: Vec<Contour> = d.geometry(&shape).contours().cloned().collect();
        let before = fonts.generation();

        fonts.add_stroke_font(id, Arc::new(StrokeFont::from_svg_font(SVG_FONT).unwrap()));
        assert_ne!(fonts.generation(), before, "registering a font is a new generation");
        let refreshed: Vec<Contour> = d.geometry(&shape).contours().cloned().collect();
        assert_ne!(refreshed, fallback, "the same design lays out in the new font");
        assert_eq!(refreshed.len(), 4, "H's three strokes and i's one");
    }

    #[test]
    fn an_uploaded_svg_font_is_kept_and_registered_and_artwork_is_not() {
        let projects = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let font_file = source.path().join("Quill Hand.svg");
        std::fs::write(&font_file, SVG_FONT).unwrap();
        let fonts = FontCatalogue::fixture();

        let added = fonts.install_svg_font(projects.path(), &font_file).unwrap().expect("an SVG font");
        assert_eq!(added, WritingFont { id: StrokeFontId("svg-quill-hand".into()), name: "Quill Hand".into() });
        assert!(projects.path().join("fonts/Quill Hand.svg").is_file(), "copied into the projects directory");
        assert_eq!(fonts.writing().last(), Some(&added), "listed after the bundled writing fonts");
        assert_eq!(fonts.writing().len(), BUNDLED_STROKES.len() + 1);

        let artwork = source.path().join("square.svg");
        std::fs::write(&artwork, r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 H10 V10 Z"/></svg>"#).unwrap();
        assert_eq!(fonts.install_svg_font(projects.path(), &artwork), Ok(None), "artwork imports as shapes instead");

        let next_launch = FontCatalogue::fixture();
        assert_eq!(next_launch.load_user_fonts(projects.path()), 1);
        assert!(next_launch.stroke(&added.id).is_some(), "loaded again on the next start");
    }

    #[test]
    fn bundled_writing_fonts_exist_on_a_thread_with_no_catalogue() {
        let names = std::thread::spawn(|| current().writing().into_iter().map(|w| w.name).collect::<Vec<_>>()).join().unwrap();
        assert_eq!(
            names,
            ["Hershey Simplex Roman", "Hershey Duplex Roman", "Hershey Complex Script", "Hershey Script Simplex", "Hershey Gothic English", "Relief SingleLine"]
        );
    }

    /// An environment of `vars`, for [`font_dirs`].
    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, OsString)> = vars.iter().map(|&(k, v)| (k.to_owned(), v.into())).collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn the_host_is_the_platform_this_was_built_for() {
        #[cfg(windows)]
        assert_eq!(Platform::HOST, Platform::Windows);
        #[cfg(target_os = "macos")]
        assert_eq!(Platform::HOST, Platform::Mac);
        #[cfg(target_os = "linux")]
        assert_eq!(Platform::HOST, Platform::Linux);
    }

    #[test]
    fn windows_scans_the_system_fonts_and_the_per_user_fonts() {
        // Joined the way `font_dirs` joins them, so the expected paths match on any host.
        let user_fonts = |local: PathBuf| local.join("Microsoft").join("Windows").join("Fonts");
        let windows = PathBuf::from(r"D:\Win");
        let dirs = font_dirs(Platform::Windows, env(&[("WINDIR", r"D:\Win"), ("LOCALAPPDATA", r"C:\Users\ada\AppData\Local")]));
        assert_eq!(dirs, [windows.join("Fonts"), user_fonts(PathBuf::from(r"C:\Users\ada\AppData\Local"))]);
        let dirs = font_dirs(Platform::Windows, env(&[("USERPROFILE", r"C:\Users\ada")]));
        let local = PathBuf::from(r"C:\Users\ada").join("AppData").join("Local");
        assert_eq!(dirs, [PathBuf::from(r"C:\Windows").join("Fonts"), user_fonts(local)], "without WINDIR or LOCALAPPDATA, the defaults");
        assert_eq!(font_dirs(Platform::Windows, env(&[("SYSTEMROOT", r"D:\Win")])), [windows.join("Fonts")], "no profile, no per-user folder");
    }

    #[test]
    fn macos_scans_the_library_folders() {
        let dirs = font_dirs(Platform::Mac, env(&[("HOME", "/Users/ada")]));
        let expected = ["/Library/Fonts", "/System/Library/Fonts", "/Network/Library/Fonts", "/Users/ada/Library/Fonts"].map(PathBuf::from);
        assert_eq!(dirs, expected);
    }

    #[test]
    fn linux_scans_the_fontconfig_folders() {
        let dirs = font_dirs(Platform::Linux, env(&[("HOME", "/home/ada")]));
        assert_eq!(dirs, ["/usr/share/fonts", "/usr/local/share/fonts", "/home/ada/.local/share/fonts"].map(PathBuf::from));
        let dirs = font_dirs(Platform::Linux, env(&[("HOME", "/home/ada"), ("XDG_DATA_HOME", "/data/ada")]));
        assert_eq!(dirs[2], PathBuf::from("/data/ada/fonts"), "XDG_DATA_HOME moves the user's folder");
        assert_eq!(font_dirs(Platform::Linux, env(&[])).len(), 2, "no home, no user folder");
    }

    /// Prints how many faces this machine's scan finds, for the CI log of each platform. Written to
    /// stderr directly so the test harness does not swallow it.
    #[test]
    fn system_fonts_are_found_on_this_platform() {
        use std::io::Write;
        let found = system_db().len();
        let dirs = font_dirs(Platform::HOST, |name| std::env::var_os(name));
        _ = writeln!(std::io::stderr(), "system fonts found: {found} ({:?}) in {dirs:?}", Platform::HOST);
        let always_has_fonts = cfg!(any(windows, target_os = "macos")) || Path::new("/usr/share/fonts").is_dir();
        assert!(found > 0 || !always_has_fonts, "no fonts in {dirs:?}");
    }

    #[test]
    fn the_fixture_lists_its_families_and_previews_inline() {
        let fonts = FontCatalogue::fixture();
        assert_eq!(fonts.families().unwrap(), ["Fixture Display", "Fixture Grotesk"]);
        assert_eq!(fonts.previews(), Previews::Inline);
        assert!(fonts.preview(FIXTURE_FAMILIES[0]).is_some());
    }
}

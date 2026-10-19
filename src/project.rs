//! Local projects: a named design persisted as JSON under the OS app-data directory, with a PNG
//! thumbnail rendered beside it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::design::Design;
use crate::sanitise::Repairs;
use crate::store;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub design: Design,
    pub created: u64,
    pub modified: u64,
}

impl Project {
    pub fn new(design: Design) -> Self {
        let now = now();
        Self { id: new_id(), name: "Untitled".into(), design, created: now, modified: now }
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A per-process sequence mixed into every id. The nanosecond clock is coarse on macOS, so two ids
/// minted in the same tick would otherwise collide and the second save would overwrite the first.
static ID_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn new_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seq = ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{seq:x}")
}

/// Set to a directory to keep projects there instead of the OS app-data directory.
pub const DIR_ENV: &str = "KERF_PROJECTS_DIR";

/// The name this variable had before the rename to Kerf. Still honoured.
const OLD_DIR_ENV: &str = "ODS_PROJECTS_DIR";

/// The app-data folder name before the rename to Kerf. Migrated on first launch.
const OLD_APP_DIR: &str = "OpenDesignSpace";

/// The projects directory the app opens with: `KERF_PROJECTS_DIR` (or the old `ODS_PROJECTS_DIR`) if set,
/// else `~/Library/Application Support/Kerf/projects` (or the platform equivalent). Created if missing.
/// On first launch after the rename, an existing old app-data folder is moved to `Kerf`.
/// Only `main` calls this; everything else receives its directory as a value.
pub fn projects_dir() -> Option<PathBuf> {
    let env = std::env::var_os(DIR_ENV).filter(|v| !v.is_empty()).or_else(|| std::env::var_os(OLD_DIR_ENV));
    let dir = pick_dir(env, || Some(migrated_projects_dir(&app_data_dir(OLD_APP_DIR)?, &app_data_dir("Kerf")?)))?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The OS app-data folder for `app`: `~/Library/Application Support/<app>` on macOS,
/// `%APPDATA%\<app>\data` on Windows and `$XDG_DATA_HOME/<app>` (lowercase) on Linux.
fn app_data_dir(app: &str) -> Option<PathBuf> {
    Some(directories::ProjectDirs::from("", "", app)?.data_dir().to_path_buf())
}

/// `new_root/projects`, after moving `old_root` (projects and `settings.json`) to `new_root` when only
/// the old folder exists. A `new_root` that exists is never touched.
fn migrated_projects_dir(old_root: &Path, new_root: &Path) -> PathBuf {
    if old_root.exists() && !new_root.exists() {
        match move_dir(old_root, new_root) {
            Ok(Moved::Renamed) => tracing::info!(target: "session", from = %old_root.display(), to = %new_root.display(), "migrated app data to Kerf"),
            Ok(Moved::Copied) => {
                tracing::info!(target: "session", from = %old_root.display(), to = %new_root.display(), "copied app data to Kerf; the old folder is left in place")
            }
            Err(e) => tracing::error!(target: "error", error = %e, "app data migration failed"),
        }
    }
    new_root.join("projects")
}

enum Moved {
    Renamed,
    Copied,
}

/// Rename `from` to `to`; if the rename fails (for example across devices) copy it instead and leave `from`.
fn move_dir(from: &Path, to: &Path) -> std::io::Result<Moved> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(Moved::Renamed);
    }
    if let Err(e) = copy_dir(from, to) {
        let _ = std::fs::remove_dir_all(to);
        return Err(e);
    }
    Ok(Moved::Copied)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn pick_dir(env: Option<std::ffi::OsString>, platform: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    match env {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => platform(),
    }
}

pub fn json_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

pub fn thumb_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.png"))
}

/// The project file format this build writes. Older files upgrade on load; newer ones are refused.
/// 1 (no `format_version` key): built-in shapes saved as their position in the list.
/// 2: built-in shapes saved by name.
/// 3: a shape's warp is the first entry of its effect stack (`effects`), upgraded per shape on parse.
/// 4: the stack also holds Offset and Sticker; hidden contours are a `contours` mask with an eye, and a
/// text curve is `curve: { diameter_in, enabled }` rather than a `curve_in` number (0 straight).
/// 5: an optimised mat layout (`design.nesting`) and per-shape `grain_lock`. Both default when absent,
/// so a format 4 file needs no upgrade step.
/// 6: Bitmap shapes, pixels in `assets/` beside the projects. Nothing to upgrade: older files cannot hold one.
/// 7: a `Compound` shape kind (spec 21 phase 2): effect groups and boolean groups. Additive, so older files
/// need no upgrade step.
/// 8: per-shape `lock_aspect`, `lock_position` and `locked`, each false when absent. The Offset effect
/// stores `direction` and `join` (Round, Miter with a limit, Bevel) with a positive distance; older signed
/// distances and Round or Sharp corners upgrade on parse.
/// 9: a Print Then Cut shape's `pattern` fill, absent when it prints its flat colour. Additive, so older
/// files need no upgrade step.
/// 10: components (spec 31): `design.components` holds masters, and an `Instance` shape kind places one.
/// Additive, so older files need no upgrade step.
/// 11: layer names (spec 40): a shape's `name` and `design.link_names`, absent when unnamed. Additive, so
/// older files need no upgrade step.
/// 12: ruler guides (spec 45): `design.guides`, `guides_locked` and `guides_hidden`, absent when there
/// are none. Additive, so older files need no upgrade step.
/// 13: the editable grid (spec 46): `design.grid`, absent until the user sets it, and `snap_grid` and
/// `snap_objects`, on when absent. Additive, so older files need no upgrade step.
/// 14: snap points (spec 47): `design.snap` holds a switch per target class and the Shift angle step.
/// The old `snap_grid`/`snap_objects` switches fold into it (grid and shape edges), and the new classes
/// default on, so an older file keeps today's behaviour with nodes and the rest added.
/// 15: live shape parameters (spec 49): the fixed free kinds (Star, Pentagon, Rectangle, Arch and the
/// rest) become `Parametric` presets that draw the same outline, upgraded per shape on parse.
/// 16: text on a path (spec 51): a text shape's `path` holds its own copy of the guide path it follows.
/// Additive, absent when the text is straight or on a circle curve, so older files need no upgrade step.
/// 17: live corners (spec 55): a `Path` gains a per-node `corners` list. A file saved before corners
/// simply has none, so the `#[serde(default)]` field needs no migration.
/// 18: skew and pivot (spec 52): a shape's `skew_deg` and `pivot`, absent when it is unsheared and its
/// pivot is the centre. Additive, so older files need no upgrade step.
/// 19: a per-project palette (spec 56): `design.swatches`, absent when the project carries none.
/// Additive, so older files need no upgrade step.
pub const FORMAT_VERSION: u64 = 19;

#[derive(Serialize)]
struct OnDisk<'a> {
    format_version: u64,
    #[serde(flatten)]
    project: &'a Project,
}

/// Writes the project, then its thumbnail. The JSON replaces the old file in one rename, so a failed
/// or interrupted save leaves the previous version intact. The thumbnail is best effort: failing to
/// write it never fails the save, and an empty design removes the stale one.
pub fn save(dir: &Path, project: &Project) -> io::Result<()> {
    checked(&project.id)?;
    std::fs::create_dir_all(dir)?;
    store::write_atomically(&json_path(dir, &project.id), to_json(project)?.as_bytes())?;
    store::write_thumbnail(dir, &thumb_path(dir, &project.id), &project.design);
    Ok(())
}

/// The project as this build writes it, for its own file and for autosave and version copies.
pub(crate) fn to_json(project: &Project) -> io::Result<String> {
    serde_json::to_string_pretty(&OnDisk { format_version: FORMAT_VERSION, project }).map_err(io::Error::other)
}

/// Opens `<id>.json`. The file name is the project's identity: whatever `id` the JSON holds is replaced,
/// so a crafted file can never point Save or Delete outside the directory. The design comes back
/// sanitised, with what that changed.
pub fn open(dir: &Path, id: &str) -> io::Result<(Project, Repairs)> {
    let (mut project, repairs) = parse(&std::fs::read_to_string(json_path(dir, checked(id)?))?)?;
    project.id = id.to_owned();
    Ok((project, repairs))
}

#[cfg(test)]
pub fn load(dir: &Path, id: &str) -> io::Result<Project> {
    open(dir, id).map(|(project, _)| project)
}

/// A project id is a file stem: one plain path component that cannot climb out of the directory.
pub(crate) fn checked(id: &str) -> io::Result<&str> {
    let mut parts = Path::new(id).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) if !id.starts_with('.') => Ok(id),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("\"{id}\" is not a project id"))),
    }
}

/// Reads a project file of any format up to [`FORMAT_VERSION`], upgrading older ones, and sanitises
/// its design.
pub fn parse(text: &str) -> io::Result<(Project, Repairs)> {
    let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
    let version = value.get("format_version").map_or(Some(1), serde_json::Value::as_u64).ok_or_else(|| invalid("format_version is not a number".into()))?;
    if version > FORMAT_VERSION {
        return Err(invalid(format!("saved by a newer version of Kerf (format {version})")));
    }
    if version < 2 {
        builtins_by_name(&mut value).map_err(invalid)?;
    }
    if version < 14 {
        migrate_snap_switches(&mut value);
    }
    let mut project: Project = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
    if version < 15 {
        project.design.upgrade_to_parametric();
    }
    let repairs = project.design.sanitise();
    if !repairs.is_empty() {
        tracing::warn!(target: "save", id = %project.id, %repairs, "project repaired on load");
    }
    Ok((project, repairs))
}

/// Up to format 14: the two `snap_grid`/`snap_objects` switches become the per-class `snap` options
/// (spec 47). Grid and shape edges take the old values, the mat follows object snapping, and the new
/// classes default on; a file missing the old keys had them on, so every class ends up on.
fn migrate_snap_switches(value: &mut serde_json::Value) {
    let Some(design) = value.get_mut("design").and_then(|d| d.as_object_mut()) else {
        return;
    };
    let was_on = |v: Option<serde_json::Value>| v.and_then(|v| v.as_bool()).unwrap_or(true);
    let grid = was_on(design.remove("snap_grid"));
    let objects = was_on(design.remove("snap_objects"));
    design.insert(
        "snap".into(),
        serde_json::json!({
            "grid": grid,
            "shape_edges": objects,
            "mat": objects,
            "guides": true,
            "nodes": true,
            "intersections": true,
            "spacing": true,
            "angle_step_deg": 45.0,
        }),
    );
}

/// Format 1 to 2: `{"Builtin": 7}` becomes `{"Builtin": "Teardrop"}`, through the frozen format 1 order.
fn builtins_by_name(value: &mut serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            if let Some(Value::Number(n)) = map.get("Builtin") {
                let name = n.as_u64().and_then(|i| crate::shape_library::FORMAT_1_BUILTINS.get(i as usize)).ok_or_else(|| format!("unknown built-in shape {n}"))?;
                map.insert("Builtin".into(), Value::String((*name).into()));
            }
            map.values_mut().try_for_each(builtins_by_name)
        }
        Value::Array(items) => items.iter_mut().try_for_each(builtins_by_name),
        _ => Ok(()),
    }
}

/// A project file as Home shows it.
#[allow(clippy::large_enum_variant, reason = "Home lists a handful of entries, built once per visit")]
pub enum Entry {
    Ready {
        project: Project,
        thumbnail: Option<Thumbnail>,
    },
    /// A file that would not load (torn, hand-edited or from a newer Kerf), shown so it can be deleted.
    Unreadable {
        id: String,
        reason: String,
    },
}

/// A thumbnail's PNG, read once when the list is built. `key` changes whenever the bytes do, so an
/// image cache keyed on it never shows a stale picture.
pub struct Thumbnail {
    pub png: std::sync::Arc<[u8]>,
    pub key: u64,
}

impl Entry {
    #[cfg(test)]
    pub fn project(&self) -> Option<&Project> {
        match self {
            Self::Ready { project, .. } => Some(project),
            Self::Unreadable { .. } => None,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Ready { project, .. } => &project.id,
            Self::Unreadable { id, .. } => id,
        }
    }
}

/// Every project file, newest first, then the unreadable ones.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|path| path.extension().is_some_and(|x| x == "json") && path != &store::session_path(dir))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some(match std::fs::read_to_string(&path).and_then(|text| parse(&text)) {
                Ok((mut project, _)) => {
                    project.id = stem;
                    let thumbnail = std::fs::read(thumb_path(dir, &project.id)).ok().map(|png| Thumbnail { key: hash(&png), png: png.into() });
                    Entry::Ready { project, thumbnail }
                }
                Err(e) => {
                    tracing::warn!(target: "save", path = ?path.display().to_string(), error = %e, "project list found an unreadable project file");
                    Entry::Unreadable { id: stem, reason: e.to_string() }
                }
            })
        })
        .collect();
    entries.sort_by(|a, b| match (a, b) {
        (Entry::Ready { project: a, .. }, Entry::Ready { project: b, .. }) => b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)),
        (Entry::Ready { .. }, Entry::Unreadable { .. }) => std::cmp::Ordering::Less,
        (Entry::Unreadable { .. }, Entry::Ready { .. }) => std::cmp::Ordering::Greater,
        (Entry::Unreadable { id: a, .. }, Entry::Unreadable { id: b, .. }) => a.cmp(b),
    });
    entries
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Renders the design's visible shapes to a square PNG, fitting the content with a margin. Images draw
/// from `pixels`, clipped to their cut lines; one not there fills its cut line in the shape's colour.
pub(crate) fn render_thumbnail(design: &Design, size: u32, pixels: &crate::assets::Pixels) -> Option<Vec<u8>> {
    use crate::effects::{Ink, Paint as PiecePaint, Placement, image_corners};
    use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
    let mut pixmap = Pixmap::new(size, size)?;
    pixmap.fill(tiny_skia::Color::WHITE);

    let bounds = design.bounds_in()?;
    let (w, h) = (bounds.2 - bounds.0, bounds.3 - bounds.1);
    let margin = 0.1 * size as f32;
    let scale = ((size as f32 - 2.0 * margin) / w.max(h).max(0.01)).max(0.0);
    let ox = margin + (size as f32 - 2.0 * margin - w * scale) / 2.0;
    let oy = margin + (size as f32 - 2.0 * margin - h * scale) / 2.0;
    let map = |(x, y): (f32, f32)| (ox + (x - bounds.0) * scale, oy + (y - bounds.1) * scale);

    let mut fill = |lines: &crate::effects::Lines, ink: &Ink, fallback: egui::Color32| {
        let mut pb = PathBuilder::new();
        for (ring, _) in lines.iter().filter(|(pts, closed)| *closed && pts.len() > 2) {
            let (sx, sy) = map(ring[0]);
            pb.move_to(sx, sy);
            for &p in &ring[1..] {
                let (x, y) = map(p);
                pb.line_to(x, y);
            }
            pb.close();
        }
        let Some(path) = pb.finish() else { return };
        let image = match ink {
            Ink::Image { asset, corners, repeat } => pixels.get(asset).and_then(|img| Some((crate::export::picture::premultiplied(img)?, corners.map(map), *repeat))),
            Ink::Colour(_) => None,
        };
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        match (&image, ink) {
            (Some((img, corners, repeat)), _) => paint.shader = crate::export::picture::image_shader(img, *corners, *repeat),
            (None, Ink::Colour(colour)) => paint.set_color_rgba8(colour.r(), colour.g(), colour.b(), colour.a()),
            (None, Ink::Image { .. }) => paint.set_color_rgba8(fallback.r(), fallback.g(), fallback.b(), fallback.a()),
        }
        pixmap.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
    };
    for shape in design.drawn().filter(|s| !s.hidden) {
        let geometry = design.geometry(shape);
        for piece in geometry.pieces() {
            let colour = piece.style.fill;
            match &piece.paint {
                PiecePaint::Fill => fill(&piece.lines, &Ink::Colour(colour), colour),
                PiecePaint::Image(asset) => {
                    let corners = image_corners((shape.width_in, shape.height_in), Placement::of(shape).placer());
                    fill(&piece.lines, &Ink::Image { asset: asset.clone(), corners, repeat: false }, colour);
                }
                PiecePaint::Print(layers) => layers.iter().for_each(|(lines, ink)| fill(lines, ink, colour)),
            }
        }
    }
    pixmap.encode_png().ok()
}

/// Makes a save of `file` into `dir` fail, or lets it succeed again. Unix refuses new files in a
/// read-only folder. Windows ignores a folder's read-only flag but refuses to rename over a read-only file.
#[cfg(test)]
pub(crate) fn refuse_writes(dir: &Path, file: &Path, refuse: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        _ = file;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if refuse { 0o555 } else { 0o755 })).unwrap();
    }
    #[cfg(windows)]
    {
        _ = dir;
        let mut permissions = std::fs::metadata(file).unwrap().permissions();
        permissions.set_readonly(refuse);
        std::fs::set_permissions(file, permissions).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::store::{clear_all, delete_project as delete};

    fn saved(dir: &Path, name: &str, kind: ShapeKind) -> Project {
        let mut design = Design::default();
        design.add_shape(kind);
        let project = Project { name: name.into(), ..Project::new(design) };
        save(dir, &project).unwrap();
        project
    }

    #[test]
    fn projects_saved_in_the_same_second_list_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        let stamp = now();
        let mut ids = Vec::new();
        for name in ["A", "B", "C", "D"] {
            let mut design = Design::default();
            design.add_shape(ShapeKind::Square);
            let project = Project { name: name.into(), modified: stamp, ..Project::new(design) };
            save(dir.path(), &project).unwrap();
            ids.push(project.id);
        }
        ids.sort();
        let listed: Vec<String> = list(dir.path())
            .into_iter()
            .map(|e| match e {
                Entry::Ready { project, .. } => project.id,
                Entry::Unreadable { id, .. } => id,
            })
            .collect();
        assert_eq!(listed, ids, "a tie on modified time falls back to id order");
    }

    #[test]
    fn save_then_load_round_trips_a_design() {
        let dir = tempfile::tempdir().unwrap();
        let mut design = Design::default();
        design.add_shape(ShapeKind::Square);
        design.add_shape(ShapeKind::Circle);
        design.shapes[1].x_in = 4.0;
        let project = Project { name: "Round Trip".into(), ..Project::new(design.clone()) };

        save(dir.path(), &project).unwrap();
        assert!(thumb_path(dir.path(), &project.id).exists(), "thumbnail written");
        let loaded = load(dir.path(), &project.id).unwrap();
        assert_eq!(loaded.name, "Round Trip");
        assert_eq!(loaded.design, design);
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[test]
    fn a_path_saved_before_corners_loads_with_none_and_corners_round_trip() {
        use crate::corners::{Corner, CornerStyle};
        use crate::design::{Contour, Seg, ShapeKind};
        let dir = tempfile::tempdir().unwrap();
        let square = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((2.0, 0.0)), Seg::Line((2.0, 2.0)), Seg::Line((0.0, 2.0))], closed: true };
        let path = crate::design::shape_from_contours(1, vec![square], egui::Color32::BLACK, crate::design::Operation::Pen).unwrap();
        let mut design = Design::default();
        design.shapes.push(path);
        let project = Project { name: "Corners".into(), ..Project::new(design.clone()) };

        // A path with no corners writes no `corners` key, so an older file that never had one still loads.
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), &project.id)).unwrap();
        assert!(!text.contains("corners"), "a path with no corners stores nothing: {text}");
        let loaded = load(dir.path(), &project.id).unwrap();
        assert!(matches!(&loaded.design.shapes[0].kind, ShapeKind::Path { corners, .. } if corners.is_empty()), "loads with no corners");

        // Set corners, and they round-trip through a save.
        let mut cornered = project.clone();
        if let ShapeKind::Path { node_kinds, corners, .. } = &mut cornered.design.shapes[0].kind {
            let nodes = node_kinds.first().map_or(4, Vec::len);
            *corners = vec![vec![Corner { radius_in: 0.3, style: CornerStyle::Inverted }; nodes]];
        }
        save(dir.path(), &cornered).unwrap();
        assert!(std::fs::read_to_string(json_path(dir.path(), &cornered.id)).unwrap().contains("Inverted"), "a set corner is stored");
        assert_eq!(load(dir.path(), &cornered.id).unwrap().design, cornered.design, "corners round-trip");
    }

    #[test]
    fn back_to_back_projects_get_distinct_ids_and_both_persist() {
        let dir = tempfile::tempdir().unwrap();
        let a = Project::new(Design::default());
        let b = Project::new(Design::default());
        assert_ne!(a.id, b.id, "ids minted in the same tick still differ");
        save(dir.path(), &a).unwrap();
        save(dir.path(), &b).unwrap();
        let ids: std::collections::HashSet<String> = list(dir.path()).iter().map(|e| e.id().to_owned()).collect();
        assert_eq!(ids.len(), 2, "both projects survive, neither overwrote the other");
    }

    #[test]
    fn delete_removes_both_files_and_drops_the_project_from_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        let keep = saved(dir.path(), "Keep", ShapeKind::Star);
        let gone = saved(dir.path(), "Gone", ShapeKind::Heart);
        assert_eq!(list(dir.path()).len(), 2);
        assert!(json_path(dir.path(), &gone.id).exists() && thumb_path(dir.path(), &gone.id).exists());

        delete(dir.path(), &gone.id).unwrap();

        assert!(!json_path(dir.path(), &gone.id).exists());
        assert!(!thumb_path(dir.path(), &gone.id).exists());
        let names: Vec<String> = list(dir.path()).iter().filter_map(Entry::project).map(|p| p.name.clone()).collect();
        assert_eq!(names, ["Keep"]);
        assert!(json_path(dir.path(), &keep.id).exists(), "other projects are untouched");
        delete(dir.path(), &gone.id).unwrap();
    }

    #[test]
    fn clear_all_empties_the_directory_of_projects() {
        let dir = tempfile::tempdir().unwrap();
        saved(dir.path(), "A", ShapeKind::Star);
        saved(dir.path(), "B", ShapeKind::Heart);
        assert_eq!(clear_all(dir.path()).unwrap(), 2);
        assert!(list(dir.path()).is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "thumbnails go too");
    }

    #[test]
    fn the_env_override_beats_the_platform_directory() {
        let platform = || Some(PathBuf::from("/platform"));
        assert_eq!(pick_dir(Some("/tmp/projects".into()), platform), Some(PathBuf::from("/tmp/projects")));
        assert_eq!(pick_dir(Some("".into()), platform), Some(PathBuf::from("/platform")));
        assert_eq!(pick_dir(None, platform), Some(PathBuf::from("/platform")));
    }

    /// The projects folder `projects_dir` picks on this platform when no override is set, printed for
    /// the CI log. Written to stderr directly so the test harness does not swallow it.
    fn platform_projects_dir() -> PathBuf {
        use std::io::Write;
        let dir = app_data_dir("Kerf").expect("this machine has a home folder").join("projects");
        _ = writeln!(std::io::stderr(), "projects directory: {}", dir.display());
        dir
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn projects_live_in_application_support_on_macos() {
        let dir = platform_projects_dir();
        assert!(dir.ends_with("Library/Application Support/Kerf/projects"), "{}", dir.display());
    }

    #[cfg(windows)]
    #[test]
    fn projects_live_in_roaming_app_data_on_windows() {
        let dir = platform_projects_dir();
        assert!(dir.ends_with(r"AppData\Roaming\Kerf\data\projects"), "{}", dir.display());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn projects_live_in_the_xdg_data_folder_on_linux() {
        let dir = platform_projects_dir();
        let home = || PathBuf::from(std::env::var_os("HOME").expect("HOME is set")).join(".local/share");
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|d| d.is_absolute()).unwrap_or_else(home);
        assert_eq!(dir, data.join("kerf/projects"));
    }

    fn roots(root: &Path) -> (PathBuf, PathBuf) {
        (root.join("OpenDesignSpace"), root.join("Kerf"))
    }

    #[test]
    fn only_the_old_folder_moves_projects_and_settings_to_kerf() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = roots(root.path());
        std::fs::create_dir_all(old.join("projects")).unwrap();
        std::fs::write(old.join("projects/a.json"), "A").unwrap();
        std::fs::write(old.join("settings.json"), "S").unwrap();
        assert_eq!(migrated_projects_dir(&old, &new), new.join("projects"));
        assert_eq!(std::fs::read_to_string(new.join("projects/a.json")).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(new.join("settings.json")).unwrap(), "S");
        assert!(!old.exists(), "a rename leaves nothing behind");
    }

    #[test]
    fn an_existing_kerf_folder_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = roots(root.path());
        std::fs::create_dir_all(old.join("projects")).unwrap();
        std::fs::write(old.join("projects/a.json"), "OLD").unwrap();
        std::fs::create_dir_all(new.join("projects")).unwrap();
        std::fs::write(new.join("projects/b.json"), "NEW").unwrap();
        migrated_projects_dir(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("projects/b.json")).unwrap(), "NEW");
        assert!(!new.join("projects/a.json").exists());
        assert!(old.join("projects/a.json").exists(), "the old folder is left alone");
    }

    #[test]
    fn with_neither_folder_kerf_projects_is_created() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = roots(root.path());
        let dir = migrated_projects_dir(&old, &new);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(new.join("projects").is_dir());
        assert!(!old.exists());
    }

    #[test]
    fn the_copy_fallback_keeps_the_old_folder() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = roots(root.path());
        std::fs::create_dir_all(old.join("projects")).unwrap();
        std::fs::write(old.join("projects/a.json"), "A").unwrap();
        copy_dir(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(new.join("projects/a.json")).unwrap(), "A");
        assert!(old.join("projects/a.json").exists());
    }

    /// A project as format 1 wrote it: no `format_version`, a built-in saved as its position.
    fn format_1(id: &str, kind: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Old","design":{{"shapes":[{{"id":1,"kind":{kind},"x_in":1.0,"y_in":1.0,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]}}]}},"created":1,"modified":1}}"#
        )
    }

    #[test]
    fn a_failed_save_leaves_the_previous_file_intact() {
        let dir = tempfile::tempdir().unwrap();
        let first = saved(dir.path(), "Keep", ShapeKind::Star);
        let before = std::fs::read_to_string(json_path(dir.path(), &first.id)).unwrap();

        refuse_writes(dir.path(), &json_path(dir.path(), &first.id), true);
        let mut changed = first.clone();
        changed.design.add_shape(ShapeKind::Heart);
        let result = save(dir.path(), &changed);
        refuse_writes(dir.path(), &json_path(dir.path(), &first.id), false);

        assert!(result.is_err(), "the save reports its failure");
        assert_eq!(std::fs::read_to_string(json_path(dir.path(), &first.id)).unwrap(), before, "the old file is untouched");
        assert_eq!(load(dir.path(), &first.id).unwrap().design, first.design);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "no temp file is left behind");
    }

    #[test]
    fn unreadable_files_are_listed_and_cleared_not_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let good = saved(dir.path(), "Good", ShapeKind::Star);
        std::fs::write(dir.path().join("torn.json"), r#"{"id":"torn","name":"Torn","des"#).unwrap();
        std::fs::write(dir.path().join("future.json"), r#"{"format_version":99,"id":"future"}"#).unwrap();

        let entries = list(dir.path());
        let ids: Vec<&str> = entries.iter().map(Entry::id).collect();
        assert_eq!(ids, [good.id.as_str(), "future", "torn"], "readable first, then the rest by name");
        let Entry::Unreadable { reason, .. } = &entries[1] else { panic!("future.json is unreadable") };
        assert!(reason.contains("newer version"), "{reason}");
        assert!(load(dir.path(), "torn").is_err());

        assert_eq!(clear_all(dir.path()).unwrap(), 3);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "unreadable files are cleared too");
    }

    #[test]
    fn format_1_projects_upgrade_builtins_from_position_to_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(json_path(dir.path(), "old"), format_1("old", r#"{"Builtin":3}"#)).unwrap();
        let old = load(dir.path(), "old").expect("format 1 loads");
        let ShapeKind::Builtin(b) = old.design.shapes[0].kind else { panic!("a built-in") };
        assert_eq!(b.get().name, "Rounded Rectangle", "position 3 in format 1");

        save(dir.path(), &old).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "old")).unwrap();
        assert!(text.contains(r#""Builtin": "Rounded Rectangle""#), "saved by name: {text}");
        assert!(text.contains(&format!(r#""format_version": {FORMAT_VERSION}"#)));
        assert_eq!(load(dir.path(), "old").unwrap().design, old.design, "round-trips");
    }

    #[test]
    fn a_format_2_warp_loads_as_the_first_effect_and_saves_in_the_stack() {
        let warp = r#""warp":{"style":"Wave","bend":0.25,"vertical":true}"#;
        let old = format_1("w", r#""Star""#).replacen('{', r#"{"format_version":2,"#, 1).replace(r#""contour_hidden":[]"#, &format!(r#""contour_hidden":[],{warp}"#));
        let (project, _) = parse(&old).expect("format 2 loads");
        let expected = crate::warp::Warp { style: crate::warp::WarpStyle::Wave, bend: 0.25, vertical: true };
        assert_eq!(project.design.shapes[0].effects.warp(), Some(expected));
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "w")).unwrap();
        assert!(!text.contains(r#""warp":"#) && text.contains(r#""effects":"#), "{text}");
        assert_eq!(load(dir.path(), "w").unwrap().design, project.design, "round-trips");
    }

    #[test]
    fn a_format_3_curve_and_hidden_contours_load_into_their_models() {
        let text = r#"{"Text":{"content":"Hi","font":"Inter","size_in":1.0,"curve_in":3.0}}"#;
        let old = format_1("c", text).replacen('{', r#"{"format_version":3,"#, 1).replace(r#""contour_hidden":[]"#, r#""contour_hidden":[true]"#);
        let (project, _) = parse(&old).expect("format 5 loads");
        let shape = &project.design.shapes[0];
        assert_eq!(shape.kind.active_curve(), Some(3.0));
        assert_eq!(shape.contours, crate::effects::ContourMask { hidden: vec![true], enabled: true });
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let saved = std::fs::read_to_string(json_path(dir.path(), "c")).unwrap();
        assert!(!saved.contains("curve_in") && !saved.contains("contour_hidden") && saved.contains(r#""diameter_in": 3.0"#), "{saved}");
        assert_eq!(load(dir.path(), "c").unwrap().design, project.design, "round-trips");
        let straight = format_1("s", &text.replace("3.0", "0.0")).replacen('{', r#"{"format_version":3,"#, 1);
        assert!(matches!(parse(&straight).unwrap().0.design.shapes[0].kind, crate::design::ShapeKind::Text { curve: None, .. }), "0 is straight");
    }

    #[test]
    fn a_format_4_project_loads_with_no_nesting_and_no_grain_lock() {
        let old = format_1("n", r#""Star""#).replacen('{', r#"{"format_version":4,"#, 1);
        let (project, repairs) = parse(&old).expect("format 5 loads");
        assert!(repairs.is_empty());
        assert_eq!(project.design.nesting, None);
        assert!(project.design.shapes.iter().all(|s| !s.grain_lock));
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "n")).unwrap();
        assert!(!text.contains("nesting") && text.contains(r#""grain_lock": false"#), "{text}");
    }

    #[test]
    fn a_format_7_project_loads_with_nothing_locked() {
        let old = format_1("l", r#""Star""#).replacen('{', r#"{"format_version":7,"#, 1);
        let (project, repairs) = parse(&old).expect("format 7 loads");
        assert!(repairs.is_empty());
        assert!(project.design.shapes.iter().all(|s| !s.lock_aspect && !s.lock_position && !s.locked));
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "l")).unwrap();
        assert!(text.contains(r#""lock_position": false"#) && text.contains(r#""locked": false"#), "{text}");
    }

    #[test]
    fn a_format_8_project_loads_with_no_pattern_and_a_pattern_round_trips() {
        let old = format_1("p", r#""Heart""#).replacen('{', r#"{"format_version":8,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 8 loads");
        assert!(repairs.is_empty());
        assert!(project.design.shapes.iter().all(|s| s.pattern.is_none()));
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        assert!(!std::fs::read_to_string(json_path(dir.path(), "p")).unwrap().contains("pattern"), "no pattern, no key");

        let asset = crate::assets::id_of(&image::RgbaImage::new(2, 2));
        let fill = crate::patterns::PatternFill::new(asset.clone(), 1.0, crate::patterns::PatternMode::Tile);
        project.design.shapes[0].pattern = Some(fill.clone());
        save(dir.path(), &project).unwrap();
        assert_eq!(load(dir.path(), "p").unwrap().design.shapes[0].pattern, Some(fill));
        assert_eq!(crate::assets::references(&project.design), [format!("{asset}.png")], "autosave's collection keeps the pattern's image");
    }

    #[test]
    fn a_format_18_project_loads_with_no_swatches_and_a_palette_round_trips() {
        let old = format_1("pal", r#""Star""#).replacen('{', r#"{"format_version":18,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 18 loads");
        assert!(repairs.is_empty());
        assert!(project.design.swatches.is_empty(), "a file before 19 loads with an empty palette");
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        assert!(!std::fs::read_to_string(json_path(dir.path(), "pal")).unwrap().contains("swatches"), "no palette, no key");

        project.design.swatches = vec![
            crate::settings::Swatch { name: "Navy".into(), colour: egui::Color32::from_rgb(0x1C, 0x2E, 0x6B) },
            crate::settings::Swatch { name: "Gold".into(), colour: egui::Color32::from_rgb(0xC9, 0xA2, 0x3A) },
        ];
        save(dir.path(), &project).unwrap();
        assert_eq!(load(dir.path(), "pal").unwrap().design.swatches, project.design.swatches, "the palette round-trips at format 19");
    }

    #[test]
    fn a_format_10_project_loads_unnamed_and_names_round_trip() {
        let old = format_1("names", r#""Star""#).replacen('{', r#"{"format_version":10,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 10 loads");
        assert!(repairs.is_empty());
        assert!(project.design.shapes.iter().all(|s| s.name.is_none()) && project.design.link_names.is_empty());
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "names")).unwrap();
        assert!(!text.contains("\"name\": null") && !text.contains("link_names"), "unnamed, no keys: {text}");

        let star = project.design.shapes[0].id;
        project.design.add_shape(ShapeKind::Square);
        let square = project.design.shapes[1].id;
        let link = project.design.group(&[star, square]).unwrap();
        project.design.apply(&crate::edit::Edit::Rename(Some("Speech Bubble".into())), &[star]).unwrap();
        project.design.apply(&crate::edit::Edit::Rename(Some("Badge".into())), &[star, square]).unwrap();
        save(dir.path(), &project).unwrap();
        let loaded = load(dir.path(), "names").unwrap().design;
        assert_eq!(loaded.shapes[0].name.as_deref(), Some("Speech Bubble"));
        assert_eq!(loaded.link_names.get(&link).map(String::as_str), Some("Badge"));
    }

    #[test]
    fn a_format_12_project_loads_with_the_default_grid_and_a_quarter_inch_grid_round_trips() {
        use crate::grid::{GridSettings, GridVisibility, PRESETS};
        use crate::units::Units;
        let old = format_1("grid12", r#""Star""#).replacen('{', r#"{"format_version":12,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 12 loads");
        assert!(repairs.is_empty());
        assert_eq!(project.design.grid, None, "an untouched grid follows the units");
        assert_eq!(project.design.grid_or(GridSettings::for_units(Units::Imperial)), GridSettings::for_units(Units::Imperial));
        assert_eq!(project.design.snap, crate::snap::SnapOptions::default(), "every snap class stays on");
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "grid12")).unwrap();
        assert!(!text.contains("\"grid\""), "no grid key until one is set: {text}");

        let quarter = GridSettings { origin_in: (0.5, 0.25), visible: GridVisibility::Major, ..GridSettings::for_units(Units::Imperial).with_preset(&PRESETS[1]) };
        project.design.apply(&crate::edit::Edit::Grid(quarter), &[]).unwrap();
        project.design.snap.grid = false;
        save(dir.path(), &project).unwrap();
        let loaded = load(dir.path(), "grid12").unwrap().design;
        assert_eq!(loaded.grid, Some(quarter));
        assert_eq!(loaded.grid_or(GridSettings::for_units(Units::Metric)).snap_in(), 0.25, "the pitch stays put in centimetres");
        assert!(!loaded.snap.grid && loaded.snap.shape_edges);
    }

    #[test]
    fn a_format_13_project_migrates_the_old_snap_switches_into_snap_options() {
        // A format 13 design that had Snap to grid off but object snapping on.
        let old =
            format_1("snap13", r#""Star""#).replacen('{', r#"{"format_version":13,"#, 1).replacen(r#""shapes""#, r#""snap_grid":false,"snap_objects":true,"shapes""#, 1);
        let (project, repairs) = parse(&old).expect("format 13 loads");
        assert!(repairs.is_empty());
        let snap = project.design.snap;
        assert!(!snap.grid, "Snap to grid stays off");
        assert!(snap.shape_edges && snap.mat, "object and mat snapping stay on");
        assert_eq!((snap.nodes, snap.guides, snap.angle_step_deg), (true, true, 45.0), "the new classes default on");

        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "snap13")).unwrap();
        assert!(text.contains("\"snap\"") && !text.contains("snap_grid"), "written as snap options, old keys gone: {text}");
        assert_eq!(load(dir.path(), "snap13").unwrap().design.snap, snap);
    }

    #[test]
    fn a_format_13_pentagon_upgrades_to_a_parametric_polygon() {
        use crate::parametric::Params;
        let old = format_1("penta13", r#""Pentagon""#).replacen('{', r#"{"format_version":13,"#, 1);
        let (project, repairs) = parse(&old).expect("format 13 loads");
        assert!(repairs.is_empty());
        let shape = &project.design.shapes[0];
        assert!(matches!(&shape.kind, ShapeKind::Parametric(Params::Polygon { sides: 5, .. })), "{:?}", shape.kind);
        assert_eq!(shape.name.as_deref(), Some("Pentagon"), "Polygon reads differently, so the name is kept");
        // A new save pins the new format number, and a reload leaves it parametric.
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "penta13")).unwrap();
        assert!(text.contains(&format!(r#""format_version": {FORMAT_VERSION}"#)) && text.contains("Parametric"), "{text}");
        assert_eq!(load(dir.path(), "penta13").unwrap().design, project.design, "round-trips");
    }

    #[test]
    fn a_format_11_project_loads_with_no_guides_and_guides_round_trip() {
        use crate::guides::{Axis, GuideEdit};
        let old = format_1("eleven", r#""Star""#).replacen('{', r#"{"format_version":11,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 11 loads");
        assert!(repairs.is_empty());
        assert!(project.design.guides.is_empty() && !project.design.guides_locked && !project.design.guides_hidden);
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        assert!(!std::fs::read_to_string(json_path(dir.path(), "eleven")).unwrap().contains("guides\""), "no guides, no keys");

        for edit in [GuideEdit::Add { axis: Axis::Vertical, at_in: 3.0 }, GuideEdit::Add { axis: Axis::Horizontal, at_in: 1.5 }, GuideEdit::LockAll] {
            project.design.apply(&crate::edit::Edit::Guide(edit), &[]).unwrap();
        }
        save(dir.path(), &project).unwrap();
        let loaded = load(dir.path(), "eleven").unwrap().design;
        assert_eq!(loaded.guides, project.design.guides);
        assert!(loaded.guides_locked);
    }

    #[test]
    fn a_format_14_text_loads_with_no_path_and_a_path_round_trips_at_format_16() {
        let kind = r#"{"Text":{"content":"Hi","font":"Inter","size_in":1.0}}"#;
        let old = format_1("fourteen", kind).replacen('{', r#"{"format_version":14,"#, 1);
        let (mut project, repairs) = parse(&old).expect("format 14 loads");
        assert!(repairs.is_empty());
        assert!(matches!(project.design.shapes[0].kind, crate::design::ShapeKind::Text { path: None, .. }), "a format 14 text has no path");

        let path = crate::text_path::TextPath::circle(3.0);
        if let crate::design::ShapeKind::Text { path: p, .. } = &mut project.design.shapes[0].kind {
            *p = Some(path);
        }
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &project).unwrap();
        let text = std::fs::read_to_string(json_path(dir.path(), "fourteen")).unwrap();
        assert!(text.contains(&format!(r#""format_version": {FORMAT_VERSION}"#)) && text.contains(r#""path""#), "{text}");
        assert_eq!(load(dir.path(), "fourteen").unwrap().design, project.design, "the path round-trips");
    }

    #[test]
    fn a_format_5_project_loads_with_no_upgrade_step() {
        let old = format_1("five", r#""Star""#).replacen('{', r#"{"format_version":5,"#, 1);
        let (project, repairs) = parse(&old).expect("format 5 loads");
        assert!(repairs.is_empty());
        assert!(project.design.shapes.iter().all(|s| s.compound().is_none()));
    }

    #[test]
    fn a_nested_compound_round_trips_and_a_warp_on_one_is_dropped() {
        use crate::design::{Combine, CompoundOp};
        use crate::edit::Edit;
        let dir = tempfile::tempdir().unwrap();
        let mut project = saved(dir.path(), "Groups", ShapeKind::Square);
        let d = &mut project.design;
        let square = d.shapes[0].id;
        let ids: Vec<u64> = [ShapeKind::Circle, ShapeKind::Heart].into_iter().map(|k| d.add_shape(k)).chain([square]).collect();
        let weld = d.apply(&Edit::Combine(Combine::Weld), &ids[..2]).unwrap();
        let offset = crate::effects::Effect::Offset(crate::effects::Offset {
            distance_in: 0.2,
            direction: crate::effects::Direction::Outset,
            join: crate::geometry::Join::Miter { limit: crate::geometry::DEFAULT_MITER_LIMIT },
            style: None,
        });
        let group = d.apply(&Edit::AddEffect { effect: offset, reach: crate::effects::Reach::Together }, &[weld[0], ids[2]]).unwrap();
        save(dir.path(), &project).unwrap();
        let loaded = load(dir.path(), &project.id).unwrap();
        assert_eq!(loaded.design, project.design, "round-trips");
        let kinds: Vec<_> = loaded.design.shapes.iter().filter_map(crate::design::Shape::compound).collect();
        assert_eq!(kinds, [CompoundOp::Effects, CompoundOp::Boolean(Combine::Weld)]);
        assert_eq!(loaded.design.geometry(&loaded.design.shapes[0]).pieces().len(), 3, "the ring, the weld and the square");

        let mut warped = serde_json::to_value(&project.design.shapes[0]).unwrap();
        warped["effects"] = serde_json::json!({"entries":[{"id":7,"effect":{"Warp":{"style":"Arch","bend":0.5,"vertical":false}}}],"next":8});
        let shape: crate::design::Shape = serde_json::from_value(warped).unwrap();
        assert!(shape.effects.is_empty(), "a Compound never loads a Warp");
        assert_eq!(shape.id, group[0]);
    }

    #[test]
    fn unknown_builtins_are_load_errors() {
        assert!(parse(&format_1("a", r#"{"Builtin":999}"#)).unwrap_err().to_string().contains("unknown built-in shape 999"));
        let named = format_1("b", r#"{"Builtin":"No Such Shape"}"#).replacen('{', r#"{"format_version":2,"#, 1);
        assert!(parse(&named).unwrap_err().to_string().contains("No Such Shape"));
    }

    #[test]
    fn every_builtin_saves_by_a_unique_name() {
        let names: std::collections::HashSet<&str> = crate::shape_library::BuiltinId::all().map(|b| b.get().name).collect();
        assert_eq!(names.len(), crate::shape_library::BUILTIN.len());
        for name in crate::shape_library::FORMAT_1_BUILTINS {
            assert!(crate::shape_library::BuiltinId::named(name).is_some(), "format 1's {name} still exists");
        }
    }

    const HUGE_VALUES: &str = include_str!("../tests/fixtures/malformed/huge-values.json");
    const ESCAPING_ID: &str = include_str!("../tests/fixtures/malformed/escaping-id.json");

    #[test]
    fn huge_and_overflowing_values_load_clamped_or_dropped_with_a_report() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(json_path(dir.path(), "huge"), HUGE_VALUES).unwrap();
        let (project, repairs) = open(dir.path(), "huge").expect("loads rather than failing");
        assert_eq!(repairs.dropped, [1], "1e39 overflows f32 to infinity, so that square goes");
        assert_eq!(repairs.clamped, [2]);
        let shape = &project.design.shapes[0];
        assert!(shape.width_in <= crate::transform::MAX_SIZE_IN && shape.height_in <= crate::transform::MAX_SIZE_IN);
        let points: usize = shape.world_rings(crate::geometry::FLATTEN_TOL_IN).iter().map(Vec::len).sum();
        assert!(points < 100_000, "the outline flattens to a bounded {points} points");
        let Entry::Ready { project: listed, .. } = &list(dir.path())[0] else { panic!("listed") };
        assert_eq!(listed.design, project.design, "Home lists the same repaired design");
    }

    #[test]
    fn the_file_name_not_the_json_id_decides_every_path() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("projects");
        std::fs::create_dir_all(&dir).unwrap();
        let outside = root.path().join("escape.json");
        std::fs::write(&outside, "not a project").unwrap();
        std::fs::write(dir.join("crafted.json"), ESCAPING_ID).unwrap();
        std::fs::write(dir.join("copy.json"), ESCAPING_ID).unwrap();

        let ids: Vec<String> = list(&dir).iter().map(|e| e.id().to_owned()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"crafted".to_owned()) && ids.contains(&"copy".to_owned()), "{ids:?}");
        let (opened, _) = open(&dir, "crafted").unwrap();
        assert_eq!(opened.id, "crafted");
        save(&dir, &opened).unwrap();
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "not a project", "Save stayed inside the directory");

        delete(&dir, "copy").unwrap();
        assert_eq!(list(&dir).len(), 1, "a copied file deletes on its own");
        assert_eq!(clear_all(&dir).unwrap(), 1);
        assert!(outside.exists(), "Clear all touched nothing outside the directory");
        for bad in ["../escape", "/abs", "a/b", "..", ".hidden", ""] {
            assert!(open(&dir, bad).is_err() && delete(&dir, bad).is_err(), "{bad:?} is refused");
        }
    }

    #[test]
    fn thumbnails_follow_the_design_and_go_when_it_empties() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = saved(dir.path(), "Thumb", ShapeKind::Star);
        let key = |dir: &Path| match &list(dir)[0] {
            Entry::Ready { thumbnail, .. } => thumbnail.as_ref().map(|t| t.key),
            Entry::Unreadable { .. } => panic!("readable"),
        };
        let star = key(dir.path()).expect("a thumbnail");

        project.design.shapes[0].kind = ShapeKind::Heart;
        save(dir.path(), &project).unwrap();
        assert_ne!(key(dir.path()), Some(star), "a re-save gives the card a new image key");

        project.design.shapes.clear();
        save(dir.path(), &project).unwrap();
        assert!(!thumb_path(dir.path(), &project.id).exists(), "an empty design leaves no stale thumbnail");
        assert_eq!(key(dir.path()), None);
    }

    fn bitmap(asset: crate::assets::AssetId) -> Design {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Bitmap { asset, px: (200, 200), clip: Vec::new() });
        design
    }

    #[test]
    fn a_bitmap_project_keeps_one_asset_file_beside_the_projects() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("projects");
        let assets = crate::assets::dir(&dir);
        let disc = include_bytes!("../tests/fixtures/trace/disc.png");
        let asset = crate::assets::store(&assets, &crate::assets::decode(disc).unwrap()).unwrap();
        let project = Project::new(bitmap(asset.clone()));
        save(&dir, &project).unwrap();
        let again = crate::assets::store(&assets, &crate::assets::decode(disc).unwrap()).unwrap();
        assert_eq!(again, asset, "a second import of the same image reuses the asset");
        assert_eq!(std::fs::read_dir(&assets).unwrap().count(), 1, "one asset file");
        let loaded = load(&dir, &project.id).unwrap();
        assert_eq!(loaded.design, project.design);
        assert_eq!(loaded.design.shapes[0].operation, crate::design::Operation::PrintThenCut);
        assert_eq!(list(&dir).len(), 1, "the asset folder is not a project");
    }

    #[test]
    fn a_project_whose_asset_is_gone_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let gone = crate::assets::id_of(&image::RgbaImage::new(3, 3));
        let project = Project::new(bitmap(gone.clone()));
        save(dir.path(), &project).unwrap();
        let loaded = load(dir.path(), &project.id).expect("loads without its pixels");
        assert!(matches!(&loaded.design.shapes[0].kind, ShapeKind::Bitmap { asset, .. } if *asset == gone));
    }

    #[test]
    fn a_format_5_project_loads_with_no_bitmaps() {
        let old = format_1("five", r#""Heart""#).replacen('{', r#"{"format_version":5,"#, 1);
        let (project, repairs) = parse(&old).expect("format 5 loads");
        assert!(repairs.is_empty());
        assert_eq!(project.design.shapes.len(), 1);
        assert!(!project.design.shapes.iter().any(|s| matches!(s.kind, ShapeKind::Bitmap { .. })));
    }

    #[test]
    fn a_crafted_asset_id_or_an_empty_image_does_not_load() {
        let crafted = format_1("c", r#"{"Bitmap":{"asset":"../escape","px":[2,2]}}"#).replacen('{', r#"{"format_version":6,"#, 1);
        assert!(parse(&crafted).is_err(), "an asset id that is not a hash is refused");
        let id = "0".repeat(64);
        let empty = format_1("e", &format!(r#"{{"Bitmap":{{"asset":"{id}","px":[0,2]}}}}"#)).replacen('{', r#"{"format_version":6,"#, 1);
        let (project, repairs) = parse(&empty).unwrap();
        assert!(project.design.shapes.is_empty() && repairs.dropped == [1], "a zero-pixel image is dropped");
    }

    #[test]
    fn a_thumbnail_paints_a_red_image_inside_its_cut_line_only() {
        let red = image::RgbaImage::from_pixel(200, 200, image::Rgba([255, 0, 0, 255]));
        let asset = crate::assets::id_of(&red);
        use crate::design::{Contour, Seg};
        let diamond = Contour { start: (100.0, 0.0), segs: vec![Seg::Line((200.0, 100.0)), Seg::Line((100.0, 200.0)), Seg::Line((0.0, 100.0))], closed: true };
        let mut design = Design::default();
        design.add_shape(ShapeKind::Bitmap { asset: asset.clone(), px: (200, 200), clip: vec![diamond] });
        let pixels = crate::assets::Pixels::from([(asset, std::sync::Arc::new(red))]);
        let size = 100;
        let png = render_thumbnail(&design, size, &pixels).unwrap();
        let thumb = image::load_from_memory(&png).unwrap().into_rgba8();
        let (centre, margin) = (size as f32 / 2.0, 0.1 * size as f32);
        let half = centre - margin;
        let (mut inside, mut outside) = (0, 0);
        for (x, y, p) in thumb.enumerate_pixels() {
            let [r, g, b, _] = p.0;
            if r > 200 && g < 60 && b < 60 {
                let reach = ((x as f32 + 0.5 - centre).abs() + (y as f32 + 0.5 - centre).abs()) / half;
                if reach < 1.0 + 1.5 / half { inside += 1 } else { outside += 1 }
            }
        }
        assert!(inside > 2000, "the picture fills the diamond: {inside} red pixels");
        assert_eq!(outside, 0, "nothing red outside the cut line");
        assert_eq!(thumb.get_pixel(size / 2, size / 2).0, [255, 0, 0, 255]);
        assert_eq!(thumb.get_pixel(13, 13).0, [255, 255, 255, 255], "the image's corner is outside its cut line");
    }

    #[test]
    fn a_thumbnail_tiles_a_pattern_inside_its_cut_line_only() {
        use crate::patterns::{PatternMode, fixture};
        let design = fixture::circle(PatternMode::Tile);
        let size = 100;
        let thumb = image::load_from_memory(&render_thumbnail(&design, size, &fixture::pixels()).unwrap()).unwrap().into_rgba8();
        assert_eq!(thumb.get_pixel(11, 11).0, [255, 255, 255, 255], "white outside the circle");
        let row: Vec<[u8; 4]> = (20..80).map(|x| thumb.get_pixel(x, 50).0).collect();
        let (red, blue) = (row.iter().filter(|p| **p == fixture::RED).count(), row.iter().filter(|p| **p == fixture::BLUE).count());
        assert!(red > 15 && blue > 15, "both halves of the tile across the circle: {red} red, {blue} blue");
        assert!(row.windows(2).filter(|w| w[0] != w[1]).count() >= 6, "repeating, not one stretched image");
    }

    #[test]
    fn a_project_saved_before_skew_and_pivot_loads_with_neither() {
        let old = format!(r#"{{"format_version":16,{}"#, &format_1("p", r#""Star""#)[1..]);
        let (project, _) = parse(&old).expect("format 16 loads");
        let shape = &project.design.shapes[0];
        assert_eq!(shape.skew_deg, (0.0, 0.0), "no skew");
        assert_eq!(shape.pivot, None, "the pivot is the centre");

        let mut sheared = project.clone();
        sheared.design.shapes[0].skew_deg = (20.0, 0.0);
        sheared.design.shapes[0].pivot = Some((0.1, 0.2));
        let text = to_json(&sheared).unwrap();
        assert!(text.contains(r#""skew_deg""#) && text.contains(r#""pivot""#), "both save");
        let (reloaded, _) = parse(&text).unwrap();
        assert_eq!(reloaded.design.shapes[0].skew_deg, (20.0, 0.0));
        assert_eq!(reloaded.design.shapes[0].pivot, Some((0.1, 0.2)));
    }

    #[test]
    fn without_its_pixels_a_thumbnail_fills_the_cut_line_in_the_shape_colour() {
        let mut design = bitmap(crate::assets::id_of(&image::RgbaImage::new(3, 3)));
        design.shapes[0].fill = egui::Color32::BLUE;
        let thumb = image::load_from_memory(&render_thumbnail(&design, 50, &crate::assets::Pixels::new()).unwrap()).unwrap().into_rgba8();
        assert_eq!(thumb.get_pixel(25, 25).0, [0, 0, 255, 255]);
    }
}

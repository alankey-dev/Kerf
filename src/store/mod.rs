//! The projects directory: its layout and every write to it. Only the saver thread calls the writers
//! while the app runs, so no two writes race on a file.
//!
//! ```text
//! <id>.json, <id>.png              the saved project and its thumbnail (project.rs)
//! autosave/<id>.json               the latest unsaved state of an open project, or what a closed
//!                                  never-saved tab left (unsaved.rs)
//! versions/<id>/<ms>.json, .png    one version and its thumbnail
//! versions/<id>/index.json         the version list, oldest first
//! session.json                     the open project and the running app's pid
//! assets/                          spec 28's asset store
//! ```
//!
//! Autosave and version files are ordinary project files, so `project::parse` reads them. Nothing here
//! reads the wall clock: callers pass `now_ms`.

pub mod autosave;
pub mod saver;
pub mod session;
pub mod unsaved;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::design::Design;
use crate::project::{self, Project};

pub const HOUR_MS: u64 = 3_600_000;
pub const DAY_MS: u64 = 24 * HOUR_MS;
/// Versions kept per project after pruning.
pub const MAX_VERSIONS: usize = 100;
const THUMB_PX: u32 = 220;

/// A version's time in unix milliseconds, unique within its project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionKind {
    /// Recorded by Save.
    Manual,
    /// Recorded by ten minutes of editing, or by discarding an autosave.
    Auto,
    /// The design as it was just before a Restore replaced it.
    BeforeRestore,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub id: VersionId,
    pub kind: VersionKind,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

/// A project's versions, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub versions: Vec<Version>,
}

pub fn autosave_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("autosave").join(format!("{id}.json"))
}

fn versions_dir(dir: &Path, id: &str) -> PathBuf {
    dir.join("versions").join(id)
}

pub fn version_json(dir: &Path, id: &str, v: VersionId) -> PathBuf {
    versions_dir(dir, id).join(format!("{}.json", v.0))
}

pub fn version_png(dir: &Path, id: &str, v: VersionId) -> PathBuf {
    versions_dir(dir, id).join(format!("{}.png", v.0))
}

fn index_path(dir: &Path, id: &str) -> PathBuf {
    versions_dir(dir, id).join("index.json")
}

pub fn session_path(dir: &Path) -> PathBuf {
    dir.join("session.json")
}

pub fn assets_dir(dir: &Path) -> PathBuf {
    dir.join("assets")
}

/// Writes `<path>.tmp`, then renames it over `path`, so a crash mid-write never leaves a torn file.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    stage(path, bytes)?.commit()
}

/// A complete temporary file waiting to replace its target.
pub(crate) struct Staged {
    tmp: PathBuf,
    path: PathBuf,
}

pub(crate) fn stage(path: &Path, bytes: &[u8]) -> io::Result<Staged> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let staged = Staged { tmp: PathBuf::from(tmp), path: path.to_owned() };
    std::fs::write(&staged.tmp, bytes).inspect_err(|_| _ = std::fs::remove_file(&staged.tmp))?;
    Ok(staged)
}

impl Staged {
    pub(crate) fn commit(self) -> io::Result<()> {
        std::fs::rename(&self.tmp, &self.path).inspect_err(|_| _ = std::fs::remove_file(&self.tmp))
    }
}

/// Renders the design's thumbnail to `path`, images drawn from the store under `projects`, or removes a
/// stale one when the design is empty. Best effort: a thumbnail never fails the write it accompanies.
pub(crate) fn write_thumbnail(projects: &Path, path: &Path, design: &Design) {
    let pixels = crate::assets::thumbnail_pixels(&crate::assets::dir(projects), design);
    let written = match project::render_thumbnail(design, THUMB_PX, &pixels) {
        Some(png) => write_atomically(path, &png),
        None => remove_if_present(path),
    };
    if let Err(e) = written {
        tracing::warn!(target: "save", path = ?path.display().to_string(), error = %e, "thumbnail not written");
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn remove_dir_if_present(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn write_project(path: &Path, project: &Project) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomically(path, project::to_json(project)?.as_bytes())
}

/// Reads a project-format file; the id comes from the caller, never from the file.
fn read_project(path: &Path, id: &str) -> io::Result<Project> {
    let (mut project, _) = project::parse(&std::fs::read_to_string(path)?)?;
    project.id = id.to_owned();
    Ok(project)
}

/// Save: writes `<id>.json` and its thumbnail, records a Manual version and removes the autosave. Only
/// the project file decides success; a version or autosave that fails afterwards is logged.
pub fn save_project(dir: &Path, project: &Project, now_ms: u64) -> io::Result<()> {
    project::save(dir, project)?;
    if let Err(e) = record_version(dir, project, VersionKind::Manual, now_ms) {
        tracing::warn!(target: "save", id = %project.id, error = %e, "manual version not recorded");
    }
    if let Err(e) = clear_autosave(dir, &project.id) {
        tracing::warn!(target: "save", id = %project.id, error = %e, "autosave not cleared");
    }
    Ok(())
}

pub fn write_autosave(dir: &Path, project: &Project) -> io::Result<()> {
    project::checked(&project.id)?;
    write_project(&autosave_path(dir, &project.id), project)
}

/// The autosave of `id`, or `None` when there is none.
pub fn read_autosave(dir: &Path, id: &str) -> io::Result<Option<Project>> {
    match read_project(&autosave_path(dir, project::checked(id)?), id) {
        Ok(p) => Ok(Some(p)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn clear_autosave(dir: &Path, id: &str) -> io::Result<()> {
    remove_if_present(&autosave_path(dir, project::checked(id)?))
}

/// Moves the autosave into the project's versions as an Auto version, so a Discard can be undone from
/// Version history. Without an autosave it does nothing.
pub fn discard_autosave(dir: &Path, id: &str, now_ms: u64) -> io::Result<()> {
    let Some(autosave) = read_autosave(dir, id)? else { return Ok(()) };
    record_version(dir, &autosave, VersionKind::Auto, now_ms)?;
    clear_autosave(dir, id)
}

/// The version index, empty when the project has none yet.
pub fn read_index(dir: &Path, id: &str) -> io::Result<Index> {
    match std::fs::read_to_string(index_path(dir, project::checked(id)?)) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Index::default()),
        Err(e) => Err(e),
    }
}

fn write_index(dir: &Path, id: &str, index: &Index) -> io::Result<()> {
    write_atomically(&index_path(dir, id), serde_json::to_string_pretty(index).map_err(io::Error::other)?.as_bytes())
}

/// Records `project` as a new version, then prunes. Ids only grow: one not after the newest becomes the
/// newest plus one.
pub fn record_version(dir: &Path, project: &Project, kind: VersionKind, now_ms: u64) -> io::Result<VersionId> {
    let id = project::checked(&project.id)?;
    let mut index = read_index(dir, id)?;
    let newest = index.versions.last().map_or(0, |v| v.id.0 + 1);
    let version = VersionId(now_ms.max(newest));
    write_project(&version_json(dir, id, version), project)?;
    write_thumbnail(dir, &version_png(dir, id, version), &project.design);
    index.versions.push(Version { id: version, kind, name: None, pinned: false });
    let gone = prune(&index, VersionId(now_ms.max(version.0)));
    index.versions.retain(|v| !gone.contains(&v.id));
    write_index(dir, id, &index)?;
    for v in gone {
        remove_if_present(&version_json(dir, id, v))?;
        remove_if_present(&version_png(dir, id, v))?;
    }
    tracing::info!(target: "save", id, version = version.0, ?kind, kept = index.versions.len(), "version recorded");
    Ok(version)
}

pub fn read_version(dir: &Path, id: &str, version: VersionId) -> io::Result<Project> {
    read_project(&version_json(dir, project::checked(id)?, version), id)
}

/// Names a version, which also pins it against pruning.
pub fn name_version(dir: &Path, id: &str, version: VersionId, name: String) -> io::Result<()> {
    let mut index = read_index(dir, id)?;
    let v = index.versions.iter_mut().find(|v| v.id == version).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such version"))?;
    v.name = Some(name);
    v.pinned = true;
    write_index(dir, id, &index)
}

/// Records `current` as BeforeRestore, then returns `version`'s project for the caller to apply. The
/// version is read first, so pruning after the new record can never remove it underneath.
pub fn restore(dir: &Path, current: &Project, version: VersionId, now_ms: u64) -> io::Result<Project> {
    let restored = read_version(dir, &current.id, version)?;
    record_version(dir, current, VersionKind::BeforeRestore, now_ms)?;
    Ok(restored)
}

/// Removes everything kept for `id`: its file, thumbnail, versions and autosave. Missing files are
/// fine, so deleting twice is safe.
pub fn delete_project(dir: &Path, id: &str) -> io::Result<()> {
    project::checked(id)?;
    for path in [project::json_path(dir, id), project::thumb_path(dir, id), autosave_path(dir, id)] {
        remove_if_present(&path)?;
    }
    remove_dir_if_present(&versions_dir(dir, id))?;
    // Leave no empty folders behind; one still holding another project's files stays.
    for parent in ["autosave", "versions"] {
        _ = std::fs::remove_dir(dir.join(parent));
    }
    Ok(())
}

/// Deletes every listed project, readable or not; returns how many.
pub fn clear_all(dir: &Path) -> io::Result<usize> {
    let all = project::list(dir);
    for entry in &all {
        delete_project(dir, entry.id())?;
    }
    Ok(all.len())
}

/// The versions to delete so `index` keeps: every version up to an hour old; the newest in each UTC
/// hour among those up to a day old; the newest in each UTC day among those up to 30 days old; every
/// pinned or Manual version up to 90 days old. Of those, the newest [`MAX_VERSIONS`]. Pruning the kept
/// set again at the same `now` deletes nothing.
pub fn prune(index: &Index, now: VersionId) -> Vec<VersionId> {
    let age = |v: &Version| now.0.saturating_sub(v.id.0);
    let newest_per = |older_than: u64, up_to: u64, bucket: u64| {
        let mut newest: HashMap<u64, VersionId> = HashMap::new();
        for v in index.versions.iter().filter(|v| (older_than + 1..=up_to).contains(&age(v))) {
            let slot = newest.entry(v.id.0 / bucket).or_insert(v.id);
            *slot = (*slot).max(v.id);
        }
        newest.into_values().collect::<HashSet<_>>()
    };
    let hourly = newest_per(HOUR_MS, DAY_MS, HOUR_MS);
    let daily = newest_per(DAY_MS, 30 * DAY_MS, DAY_MS);
    let mut kept: Vec<VersionId> = index
        .versions
        .iter()
        .filter(|v| age(v) <= HOUR_MS || hourly.contains(&v.id) || daily.contains(&v.id) || ((v.pinned || v.kind == VersionKind::Manual) && age(v) <= 90 * DAY_MS))
        .map(|v| v.id)
        .collect();
    kept.sort_unstable();
    let kept: HashSet<VersionId> = kept.split_off(kept.len().saturating_sub(MAX_VERSIONS)).into_iter().collect();
    index.versions.iter().map(|v| v.id).filter(|id| !kept.contains(id)).collect()
}

/// Deletes every file in `assets/` that no project, autosave or version references, by the file names
/// `references` finds in a design. Returns what it deleted. A file it cannot read might reference
/// anything, so then it deletes nothing.
pub fn collect_asset_garbage(dir: &Path, references: impl Fn(&Design) -> Vec<String>) -> io::Result<Vec<PathBuf>> {
    let json_in = |folder: PathBuf| {
        std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect::<Vec<_>>()
    };
    let version_dirs = std::fs::read_dir(dir.join("versions")).into_iter().flatten().flatten().map(|e| e.path());
    let files = json_in(dir.to_owned())
        .into_iter()
        .filter(|p| *p != session_path(dir))
        .chain(json_in(dir.join("autosave")))
        .chain(version_dirs.flat_map(json_in).filter(|p| p.file_name().is_some_and(|n| n != "index.json")));
    let mut used = HashSet::new();
    for path in files {
        match std::fs::read_to_string(&path).and_then(|text| project::parse(&text)) {
            Ok((p, _)) => used.extend(references(&p.design)),
            Err(e) => {
                tracing::warn!(target: "save", path = ?path.display().to_string(), error = %e, "asset collection skipped: unreadable project");
                return Ok(Vec::new());
            }
        }
    }
    let mut deleted = Vec::new();
    for entry in std::fs::read_dir(assets_dir(dir)).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file() && !path.file_name().is_some_and(|n| used.contains(&*n.to_string_lossy())) {
            std::fs::remove_file(&path)?;
            deleted.push(path);
        }
    }
    deleted.sort();
    Ok(deleted)
}

#[cfg(test)]
mod tests;

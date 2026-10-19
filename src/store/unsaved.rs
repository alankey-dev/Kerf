//! Work whose tab was closed before it was ever saved: an autosave with no project file and no open
//! tab. Nothing else reaches it, so Home lists it, and it is pruned after [`TTL_MS`].

use std::io;
use std::path::Path;
use std::sync::Arc;

use super::{DAY_MS, autosave_path, read_project, remove_dir_if_present, remove_if_present, versions_dir};
use crate::project::{self, Project};

/// An unsaved design is kept this long after its last change.
pub const TTL_MS: u64 = 30 * DAY_MS;

/// An autosave Home offers back.
#[allow(clippy::large_enum_variant, reason = "Home lists a handful of entries, built once per visit")]
pub enum Unsaved {
    Ready {
        project: Project,
        /// The design rendered at list time, so Home draws nothing but the bytes.
        thumbnail: Option<Arc<[u8]>>,
    },
    /// An autosave that would not load, shown so it can be deleted.
    Unreadable { id: String, reason: String },
}

impl Unsaved {
    #[cfg(test)]
    pub fn id(&self) -> &str {
        match self {
            Self::Ready { project, .. } => &project.id,
            Self::Unreadable { id, .. } => id,
        }
    }
}

/// What a scan of `autosave/` found.
#[derive(Default)]
pub struct Scan {
    /// Newest first, then the unreadable ones.
    pub listed: Vec<Unsaved>,
    /// Ids past [`TTL_MS`], or holding an empty design, for the caller to [`delete`].
    pub expired: Vec<String>,
}

/// The autosaves in `dir` that no project file and no tab in `open` accounts for. One read of the
/// folder, so the caller builds this when Home's list is built, as it does the project list.
pub fn scan(dir: &Path, open: &[String], now_ms: u64) -> Scan {
    let mut found = Scan::default();
    let files = std::fs::read_dir(dir.join("autosave")).into_iter().flatten().flatten();
    for file in files.filter(|f| f.path().extension().is_some_and(|x| x == "json")) {
        let path = file.path();
        let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        if project::checked(&id).is_err() || open.contains(&id) || project::json_path(dir, &id).exists() {
            continue;
        }
        match read_project(&path, &id) {
            Ok(p) if p.design.shapes.is_empty() && p.design.components.is_empty() => found.expired.push(id),
            Ok(p) if now_ms.saturating_sub(p.modified * 1000) > TTL_MS => found.expired.push(id),
            Ok(project) => {
                let pixels = crate::assets::thumbnail_pixels(&crate::assets::dir(dir), &project.design);
                let thumbnail = project::render_thumbnail(&project.design, super::THUMB_PX, &pixels).map(Arc::from);
                found.listed.push(Unsaved::Ready { project, thumbnail });
            }
            Err(e) => {
                tracing::warn!(target: "save", path = ?path.display().to_string(), error = %e, "unsaved list found an unreadable autosave");
                let age = file.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
                if age.is_some_and(|a| a.as_millis() > u128::from(TTL_MS)) {
                    found.expired.push(id);
                } else {
                    found.listed.push(Unsaved::Unreadable { id, reason: e.to_string() });
                }
            }
        }
    }
    found.listed.sort_by_cached_key(|u| match u {
        Unsaved::Ready { project, .. } => (false, std::cmp::Reverse(project.modified), project.id.clone()),
        Unsaved::Unreadable { id, .. } => (true, std::cmp::Reverse(0), id.clone()),
    });
    found.expired.sort();
    found
}

/// Removes the autosave and anything else kept for `id`, unless a project file now exists or a tab in
/// `open` has it. Missing files are fine, so deleting twice is safe.
pub fn delete(dir: &Path, id: &str, open: &[String]) -> io::Result<()> {
    project::checked(id)?;
    if open.iter().any(|o| o == id) || project::json_path(dir, id).exists() {
        return Ok(());
    }
    remove_if_present(&autosave_path(dir, id))?;
    remove_dir_if_present(&versions_dir(dir, id))?;
    _ = std::fs::remove_dir(dir.join("autosave"));
    Ok(())
}

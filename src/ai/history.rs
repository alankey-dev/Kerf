//! Past generations, one JSON file per design under `<projects>/ai/`, so a design can be found and
//! inserted again offline. Written through `store` with its atomic writes.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Detail, Style};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub prompt: String,
    pub style: Style,
    pub detail: Detail,
    /// Milliseconds since the Unix epoch.
    pub created: i64,
    pub svg: String,
    /// The candidate's title; the layer name when inserted.
    #[serde(default)]
    pub title: String,
}

pub fn dir(projects_dir: &Path) -> PathBuf {
    projects_dir.join("ai")
}

/// A file name from the id, which Kerf makes; anything else is refused rather than written.
fn path(projects_dir: &Path, id: &str) -> io::Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "history id"));
    }
    Ok(dir(projects_dir).join(format!("{id}.json")))
}

pub fn save(projects_dir: &Path, entry: &Entry) -> io::Result<()> {
    let path = path(projects_dir, &entry.id)?;
    std::fs::create_dir_all(dir(projects_dir))?;
    crate::store::write_atomically(&path, serde_json::to_string_pretty(entry).map_err(io::Error::other)?.as_bytes())
}

/// Every readable entry, newest first. A missing directory is an empty history.
pub fn load(projects_dir: &Path) -> Vec<Entry> {
    let Ok(files) = std::fs::read_dir(dir(projects_dir)) else { return Vec::new() };
    let mut entries: Vec<Entry> = files
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
        .collect();
    entries.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, created: i64) -> Entry {
        Entry { id: id.into(), prompt: "a star".into(), style: Style::Sticker, detail: Detail::Simple, created, svg: "<svg/>".into(), title: "Star".into() }
    }

    #[test]
    fn entries_round_trip_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        assert!(load(&projects).is_empty(), "no directory yet");
        save(&projects, &entry("100-0", 100)).unwrap();
        save(&projects, &entry("200-0", 200)).unwrap();
        std::fs::write(dir(&projects).join("junk.json"), "{ not json").unwrap();
        assert_eq!(load(&projects), [entry("200-0", 200), entry("100-0", 100)]);
    }

    #[test]
    fn an_id_cannot_leave_the_history_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(save(root.path(), &entry("../escape", 1)).is_err());
        assert!(!root.path().join("escape.json").exists());
    }

    #[test]
    fn entries_from_before_titles_load() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir(root.path())).unwrap();
        let old = r#"{ "id": "1-0", "prompt": "p", "style": "Stencil", "detail": "Medium", "created": 1, "svg": "<svg/>" }"#;
        std::fs::write(dir(root.path()).join("1-0.json"), old).unwrap();
        assert_eq!(load(root.path())[0].title, "");
    }
}

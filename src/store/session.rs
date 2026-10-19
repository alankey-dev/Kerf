//! `session.json`: the open projects and the running app's pid. A pid that is no longer running at the
//! next launch means the app did not quit cleanly, and the recovery list says what it left unsaved.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{read_autosave, session_path, write_atomically};
use crate::project::{self, Project};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<String>,
    pub active: usize,
    /// The running app's pid; a clean quit clears it.
    #[serde(default)]
    pub pid: Option<u32>,
}

pub fn read(dir: &Path) -> Option<Session> {
    let text = std::fs::read_to_string(session_path(dir)).ok()?;
    serde_json::from_str(&text).inspect_err(|e| tracing::warn!(target: "session", error = %e, "session.json unreadable")).ok()
}

pub fn write(dir: &Path, session: &Session) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_atomically(&session_path(dir), serde_json::to_string_pretty(session).map_err(io::Error::other)?.as_bytes())
}

/// Whether a process with this pid is running, asked of `kill -0` since `unsafe_code` is denied.
#[cfg(not(windows))]
pub fn pid_alive(pid: u32) -> bool {
    let probe = std::process::Command::new("kill").args(["-0", &pid.to_string()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    probe.is_ok_and(|s| s.success())
}

/// Whether a process with this pid is running, asked of `tasklist`, which prints one CSV row per match
/// (`"kerf.exe","1234",...`) and otherwise a message with no quoted pid. It runs without a console
/// window, which a GUI-subsystem app would otherwise flash.
#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let filter = format!("PID eq {pid}");
    let out = std::process::Command::new("tasklist").args(["/FI", &filter, "/FO", "CSV", "/NH"]).creation_flags(CREATE_NO_WINDOW).output();
    out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!(",\"{pid}\",")))
}

/// An open project's unsaved work, left by a session that did not quit cleanly.
#[derive(Clone, Debug)]
pub struct Recoverable {
    /// The autosave, carrying the project's id and name.
    pub project: Project,
    /// The autosave's time, unix seconds.
    pub modified: u64,
    /// Whether the project also has a saved file.
    pub saved: bool,
}

/// After an unclean exit, each session tab whose autosave is at least as new as its saved file (or that
/// was never saved). Empty after a clean quit or while the session's app is still running.
pub fn recovery_list(dir: &Path, alive: impl Fn(u32) -> bool) -> Vec<Recoverable> {
    let Some(session) = read(dir) else { return Vec::new() };
    let Some(pid) = session.pid.filter(|&pid| !alive(pid)) else { return Vec::new() };
    tracing::info!(target: "session", pid, tabs = session.tabs.len(), "last session did not quit cleanly");
    session
        .tabs
        .iter()
        .filter_map(|id| {
            let autosave = read_autosave(dir, id).inspect_err(|e| tracing::warn!(target: "session", id, error = %e, "autosave unreadable")).ok()??;
            let saved = project::open(dir, id).ok().map(|(p, _)| p.modified);
            (saved.is_none_or(|s| autosave.modified >= s)).then(|| Recoverable { modified: autosave.modified, saved: saved.is_some(), project: autosave })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, ShapeKind};
    use crate::store::write_autosave;

    fn project(id: &str, modified: u64) -> Project {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Star);
        Project { id: id.into(), name: id.into(), design, created: 1, modified }
    }

    fn crashed_with(dir: &Path, pid: Option<u32>) {
        project::save(dir, &project("a", 100)).unwrap();
        write_autosave(dir, &project("a", 200)).unwrap();
        write_autosave(dir, &project("b", 300)).unwrap();
        project::save(dir, &project("c", 500)).unwrap();
        write_autosave(dir, &project("c", 400)).unwrap();
        write(dir, &Session { tabs: vec!["a".into(), "b".into(), "c".into(), "d".into()], active: 0, pid }).unwrap();
    }

    #[test]
    fn a_dead_pid_with_two_newer_autosaves_yields_two() {
        let dir = tempfile::tempdir().unwrap();
        crashed_with(dir.path(), Some(4_000_000));
        let list = recovery_list(dir.path(), |_| false);
        let ids: Vec<(&str, u64, bool)> = list.iter().map(|r| (r.project.id.as_str(), r.modified, r.saved)).collect();
        assert_eq!(ids, [("a", 200, true), ("b", 300, false)], "c's autosave is older than its file; d has none");
    }

    #[test]
    fn a_clean_session_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        crashed_with(dir.path(), None);
        assert!(recovery_list(dir.path(), |_| false).is_empty());
    }

    #[test]
    fn a_live_pid_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        crashed_with(dir.path(), Some(7));
        assert!(recovery_list(dir.path(), |_| true).is_empty());
    }

    #[test]
    fn no_session_file_yields_none() {
        assert!(recovery_list(tempfile::tempdir().unwrap().path(), |_| false).is_empty());
    }

    #[test]
    fn this_process_is_alive_and_a_pid_past_the_limit_is_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(4_000_000));
    }
}

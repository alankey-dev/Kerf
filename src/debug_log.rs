//! The debug event log: plain `key=value` lines appended to a file, so someone who cannot see the
//! screen can follow a manual test session. Off unless `KERF_LOG` is set; see `init`.
//!
//! Every line reads `<ms since start>ms f=<frame> <LEVEL> <target>: <event> key=value ...`. Targets:
//! `session`, `cmd`, `design`, `select`, `flow`, `bridge`, `input`, `save`, `font`, `tutorial`, `ai`, `error`.
//!
//! Anything shaped like an API key or a bearer token is redacted before a line reaches the file.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use tracing::level_filters::LevelFilter;
use tracing::{Level, enabled};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

use crate::bridge::BridgeMessage;
use crate::design::{Design, Shape};
use crate::make_flow::MakeStep;

/// A filter: a bare level (`debug`, `trace`) for the app's targets, or full `EnvFilter` directives.
pub const LEVEL_ENV: &str = "KERF_LOG";
/// Overrides the log file path.
pub const FILE_ENV: &str = "KERF_LOG_FILE";

/// The names these variables had before the rename to Kerf. Still honoured.
const OLD_LEVEL_ENV: &str = "ODS_LOG";
const OLD_FILE_ENV: &str = "ODS_LOG_FILE";

const TARGETS: [&str; 12] = ["session", "cmd", "design", "select", "flow", "bridge", "input", "save", "font", "tutorial", "ai", "error"];

static FRAME: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();

/// Starts logging when `KERF_LOG` (or the old `ODS_LOG`) is set and installs the panic hook. Call once,
/// from `main` only, so tests never install the global subscriber.
pub fn init(projects_dir: Option<&Path>, window: [f32; 2]) {
    let Some(level) = env_with_fallback(LEVEL_ENV, OLD_LEVEL_ENV, |n| std::env::var_os(n)).and_then(|v| v.into_string().ok()) else { return };
    let Some(path) = env_with_fallback(FILE_ENV, OLD_FILE_ENV, |n| std::env::var_os(n)).map(PathBuf::from).or_else(default_path) else { return };
    let subscriber = match subscriber(&path, &level) {
        Ok(s) => s,
        Err(e) => return eprintln!("debug log disabled: cannot open {}: {e}", path.display()),
    };
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        return;
    }
    install_panic_hook();
    tracing::info!(
        target: "session",
        version = env!("CARGO_PKG_VERSION"),
        commit = option_env!("KERF_GIT_COMMIT").unwrap_or("unknown"),
        projects_dir = ?projects_dir.map_or_else(|| "none".into(), |d| d.display().to_string()),
        window = %format_args!("{}x{}", window[0], window[1]),
        log = ?path.display().to_string(),
        filter = %level,
        "session-start"
    );
}

/// The non-empty value of `name`, else of the pre-rename `old` name. `get` is `std::env::var_os` outside tests.
fn env_with_fallback(name: &str, old: &str, get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<std::ffi::OsString> {
    [name, old].into_iter().find_map(|n| get(n).filter(|v| !v.is_empty()))
}

/// `~/Library/Logs/Kerf/debug.log` on macOS; the platform's local data dir elsewhere.
fn default_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        directories::BaseDirs::new().map(|d| d.home_dir().join("Library/Logs/Kerf/debug.log"))
    } else {
        directories::ProjectDirs::from("", "", "Kerf").map(|d| d.data_local_dir().join("logs/debug.log"))
    }
}

/// An appending, unbuffered file writer: each event is one `write` call, so every line lands at once
/// and [`redact`] sees it whole.
pub(crate) fn subscriber(path: &Path, level: &str) -> io::Result<impl tracing::Subscriber + Send + Sync + 'static> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file: File = OpenOptions::new().create(true).append(true).open(path)?;
    START.get_or_init(Instant::now);
    Ok(tracing_subscriber::fmt().with_env_filter(filter(level)).with_writer(Mutex::new(Redacting(file))).with_ansi(false).with_timer(Clock).finish())
}

/// What starts a secret, and how many key characters must follow before it counts: API keys from
/// Anthropic (`sk-ant-`), OpenAI (`sk-`, `sk-proj-`) and OpenRouter (`sk-or-`), and any bearer token.
const SECRETS: [(&str, usize); 2] = [("sk-", 16), ("Bearer ", 8)];

fn key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

/// Where `marker` first starts a word in `text`, so `task-...` is not taken for `sk-...`.
fn find_marker(text: &str, marker: &str) -> Option<usize> {
    text.match_indices(marker).map(|(at, _)| at).find(|&at| !text[..at].chars().next_back().is_some_and(key_char))
}

/// `text` with every secret in [`SECRETS`] replaced, so a key that reaches a log event by mistake never
/// reaches the file.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, marker, min)) = SECRETS.iter().filter_map(|&(m, min)| Some((find_marker(rest, m)?, m, min))).min_by_key(|s| s.0) {
        let after = &rest[at + marker.len()..];
        let len = after.find(|c: char| !key_char(c)).unwrap_or(after.len());
        out.push_str(&rest[..at + marker.len()]);
        out.push_str(if len >= min { "[redacted]" } else { &after[..len] });
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

/// A writer that passes each write through [`redact`].
struct Redacting<W>(W);

impl<W: io::Write> io::Write for Redacting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_all(redact(&String::from_utf8_lossy(buf)).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// A bare level applies to the app's own targets and keeps dependencies (wgpu, winit) at `warn`.
fn filter(level: &str) -> EnvFilter {
    let directives = if level.contains(['=', ',']) {
        level.to_owned()
    } else {
        std::iter::once("warn".to_owned()).chain(TARGETS.iter().map(|t| format!("{t}={level}"))).collect::<Vec<_>>().join(",")
    };
    EnvFilter::try_new(&directives).unwrap_or_else(|_| EnvFilter::new("warn"))
}

struct Clock;

impl FormatTime for Clock {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let ms = START.get().map_or(0, |s| s.elapsed().as_millis());
        write!(w, "{ms:>8}ms f={}", FRAME.load(Ordering::Relaxed))
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message =
            info.payload().downcast_ref::<&str>().copied().or_else(|| info.payload().downcast_ref::<String>().map(String::as_str)).unwrap_or("non-string payload");
        let location = info.location().map_or_else(|| "unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
        tracing::error!(target: "error", %location, %thread, message, "panic");
        default(info);
    }));
}

/// Ids as `1,2,3`, or `-` when empty, so one field never contains a space.
pub fn ids(ids: &[u64]) -> String {
    if ids.is_empty() {
        return "-".into();
    }
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

fn off() -> bool {
    LevelFilter::current() == LevelFilter::OFF
}

pub fn frame_start() {
    FRAME.fetch_add(1, Ordering::Relaxed);
}

/// What `frame_end` compares against: the state at the end of the last logged frame.
#[derive(Default)]
struct Seen {
    selection: Vec<u64>,
    flow: Option<(MakeStep, usize)>,
    window: Option<egui::Vec2>,
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

/// Logs what changed since the last frame: the selection, the Make flow's step and mat, the window size.
/// `flow` is `(step, mat index, mat count)`.
pub fn frame_end(ctx: &egui::Context, selection: &[u64], flow: Option<(MakeStep, usize, usize)>) {
    if off() {
        return;
    }
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let seen = seen.get_or_insert_with(Seen::default);
    if seen.selection != selection {
        tracing::debug!(target: "select", from = %ids(&seen.selection), to = %ids(selection), "selection");
        seen.selection = selection.to_vec();
    }
    let now = flow.map(|(step, mat, _)| (step, mat));
    if seen.flow != now {
        match (seen.flow, flow) {
            (_, Some((step, mat, mats))) => {
                let from = seen.flow.map_or_else(|| "none".into(), |(s, _)| format!("{s:?}"));
                tracing::info!(target: "flow", %from, to = ?step, mat = %format_args!("{}/{mats}", mat + 1), "step");
            }
            (Some((step, _)), None) => tracing::info!(target: "flow", last = ?step, "closed"),
            (None, None) => {}
        }
        seen.flow = now;
    }
    let window = ctx.screen_rect().size();
    if seen.window != Some(window) {
        tracing::debug!(target: "input", size = %format_args!("{}x{}", window.x, window.y), "window");
        seen.window = Some(window);
    }
}

/// Pointer buttons, keys and text as the app receives them.
pub fn input(raw: &egui::RawInput) {
    if !enabled!(target: "input", Level::TRACE) {
        return;
    }
    for event in &raw.events {
        match event {
            egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                tracing::trace!(target: "input", ?button, pressed, x = pos.x.round(), y = pos.y.round(), mods = %mods(*modifiers), "pointer")
            }
            egui::Event::Key { key, pressed, repeat: false, modifiers, .. } => tracing::trace!(target: "input", ?key, pressed, mods = %mods(*modifiers), "key"),
            egui::Event::Text(text) => tracing::trace!(target: "input", text = ?text, "text"),
            _ => {}
        }
    }
}

fn mods(m: egui::Modifiers) -> String {
    let names = [(m.command, "cmd"), (m.ctrl && !m.command, "ctrl"), (m.alt, "alt"), (m.shift, "shift")];
    let held: Vec<_> = names.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
    if held.is_empty() { "-".into() } else { held.join("+") }
}

/// One machine message, `send` or `recv`: its status at debug, the full JSON at trace.
pub fn bridge(direction: &str, msg: &BridgeMessage) {
    if !enabled!(target: "bridge", Level::DEBUG) {
        return;
    }
    tracing::debug!(target: "bridge", dir = direction, status = msg.status.wire_name(), code = msg.status.code(), "message");
    if enabled!(target: "bridge", Level::TRACE) {
        tracing::trace!(target: "bridge", dir = direction, json = %serde_json::to_string(msg).unwrap_or_default(), "payload");
    }
}

/// Logs a project file operation's outcome and hands the result back untouched.
pub fn project_io<T, E: std::fmt::Display>(op: &str, dir: &Path, id: &str, result: Result<T, E>) -> Result<T, E> {
    let path = crate::project::json_path(dir, id);
    match &result {
        Ok(_) => tracing::info!(target: "save", op, path = ?path.display().to_string(), result = "ok", "project"),
        Err(e) => tracing::error!(target: "save", op, path = ?path.display().to_string(), error = %e, "project"),
    }
    result
}

/// The coarse difference between two designs; built only when the `design` target is on.
pub struct DesignDiff {
    added: Vec<u64>,
    removed: Vec<u64>,
    modified: Vec<String>,
    reordered: bool,
    shapes: usize,
}

impl DesignDiff {
    pub fn between(before: &Design, after: &Design) -> Option<Self> {
        if !enabled!(target: "design", Level::DEBUG) || before == after {
            return None;
        }
        let find = |d: &Design, id| d.shapes.iter().find(|s| s.id == id).cloned();
        let added = after.shapes.iter().map(|s| s.id).filter(|&id| find(before, id).is_none()).collect();
        let removed = before.shapes.iter().map(|s| s.id).filter(|&id| find(after, id).is_none()).collect();
        let modified = after
            .shapes
            .iter()
            .filter_map(|a| Some((a, before.shapes.iter().find(|b| b.id == a.id)?)))
            .filter(|(a, b)| a != b)
            .map(|(a, b)| format!("{}:{}", a.id, changed_fields(b, a).join("+")))
            .collect();
        let order = |d: &Design| {
            d.shapes.iter().map(|s| s.id).filter(|id| before.shapes.iter().any(|s| s.id == *id) && after.shapes.iter().any(|s| s.id == *id)).collect::<Vec<_>>()
        };
        Some(Self { added, removed, modified, reordered: order(before) != order(after), shapes: after.shapes.len() })
    }

    /// `cause` names what changed it (`edit`, `Undo`, `Redo`, `New`); `undo` is whether an undo step was recorded.
    pub fn log(&self, cause: &str, undo: Option<bool>) {
        let undo = undo.map_or("n/a", |u| if u { "recorded" } else { "merged" });
        let modified = if self.modified.is_empty() { "-".into() } else { self.modified.join(",") };
        tracing::debug!(
            target: "design",
            cause,
            added = %ids(&self.added),
            removed = %ids(&self.removed),
            modified = %modified,
            reordered = self.reordered,
            shapes = self.shapes,
            undo,
            "changed"
        );
    }
}

fn changed_fields(b: &Shape, a: &Shape) -> Vec<&'static str> {
    let checks = [
        ("pos", a.x_in != b.x_in || a.y_in != b.y_in),
        ("size", a.width_in != b.width_in || a.height_in != b.height_in),
        ("rotation", a.rotation_deg != b.rotation_deg),
        ("flip", a.flip_x != b.flip_x || a.flip_y != b.flip_y),
        ("kind", a.kind != b.kind),
        ("operation", a.operation != b.operation),
        ("fill", a.fill != b.fill),
        ("pen", a.pen != b.pen),
        ("effects", a.effects != b.effects),
        ("link", a.link != b.link),
        ("parent", a.parent != b.parent),
        ("container", a.container != b.container),
        ("hidden", a.hidden != b.hidden || a.contours != b.contours),
        ("name", a.name != b.name),
    ];
    let fields: Vec<_> = checks.iter().filter(|(_, c)| *c).map(|(n, _)| *n).collect();
    if fields.is_empty() { vec!["other"] } else { fields }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_log_variable_names_still_work_and_the_new_ones_win() {
        let get = |vars: &'static [(&'static str, &'static str)]| move |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).into());
        let level = |vars| env_with_fallback(LEVEL_ENV, OLD_LEVEL_ENV, get(vars));
        assert_eq!(level(&[]), None);
        assert_eq!(level(&[("ODS_LOG", "debug")]), Some("debug".into()));
        assert_eq!(level(&[("KERF_LOG", "trace"), ("ODS_LOG", "debug")]), Some("trace".into()));
        assert_eq!(level(&[("KERF_LOG", ""), ("ODS_LOG", "debug")]), Some("debug".into()));
        assert_eq!(env_with_fallback(FILE_ENV, OLD_FILE_ENV, get(&[("ODS_LOG_FILE", "/tmp/o.log")])), Some("/tmp/o.log".into()));
    }

    #[test]
    fn events_land_in_the_file_one_per_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs/debug.log");
        let subscriber = subscriber(&path, "debug").unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "cmd", action = "Save", sel = %ids(&[1, 2]), "menu");
            tracing::debug!(target: "select", from = %ids(&[]), to = %ids(&[3]), "selection");
            tracing::trace!(target: "input", "filtered out at debug");
            tracing::debug!(target: "wgpu", "dependency noise stays at warn");
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("INFO cmd: menu action=\"Save\" sel=1,2"), "{}", lines[0]);
        assert!(lines[1].contains("DEBUG select: selection from=- to=3"), "{}", lines[1]);
        assert!(lines.iter().all(|l| l.contains("ms f=")), "every line carries time and frame");
    }

    #[test]
    fn an_api_key_never_reaches_the_file() {
        assert_eq!(redact("key=sk-ant-api03-AbC_d-9xyz next"), "key=sk-[redacted] next");
        assert_eq!(redact("sk-proj-0123456789abcdef,sk-or-v1-0123456789abcdef"), "sk-[redacted],sk-[redacted]");
        assert_eq!(redact("authorization: Bearer local.token-123"), "authorization: Bearer [redacted]");
        assert_eq!(redact("task-0123456789abcdefgh and sk-short"), "task-0123456789abcdefgh and sk-short", "ordinary words stay");
        assert_eq!(redact("no key here"), "no key here");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("debug.log");
        tracing::subscriber::with_default(subscriber(&path, "debug").unwrap(), || {
            tracing::info!(target: "ai", header = "sk-ant-api03-secret", "oops");
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("INFO ai: oops") && !text.contains("secret"), "{text}");
    }

    #[test]
    fn a_design_diff_names_the_changed_fields() {
        let mut before = Design::default();
        let a = before.add_shape(crate::design::ShapeKind::Square);
        let mut after = before.clone();
        after.shape_mut(a).unwrap().x_in += 1.0;
        let diff = changed_fields(before.shapes.iter().find(|s| s.id == a).unwrap(), after.shapes.iter().find(|s| s.id == a).unwrap());
        assert_eq!(diff, ["pos"]);
    }
}

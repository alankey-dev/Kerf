// Release builds on Windows open no console window; the command-line modes borrow the terminal they
// were started from instead (`attach_console`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod proto_enum;

mod ai;
mod assets;
mod bench;
mod bridge;
mod clipboard;
mod components;
mod composition;
mod containers;
mod corners;
mod debug_log;
mod design;
mod device;
mod edit;
mod effects;
mod export;
mod expr;
mod flatten;
mod geometry;
mod grid;
mod guides;
mod knife;
mod layers_dnd;
mod links;
mod machine;
mod make_flow;
mod material;
mod measure;
mod monogram;
mod names;
mod nesting;
mod output;
mod palette;
mod palettes;
mod parametric;
mod path_edit;
mod patterns;
mod pens;
mod print_sheet;
mod print_then_cut;
mod project;
mod repeat;
mod sanitise;
mod select;
mod settings;
mod shape_library;
mod snap;
mod store;
#[cfg_attr(not(test), expect(dead_code, reason = "the JSON writer serves tools/stroke_font and pen_up_distance serves tests"))]
mod stroke_font;
mod svg_import;
mod system_fonts;
mod text;
mod text_ops;
mod text_path;
mod trace;
mod transform;
mod ui;
mod units;
mod warp;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use device::SimulatedMachine;

const WINDOW: [f32; 2] = [1600.0, 1000.0];

const USAGE: &str = "usage: kerf [--open <project.json>]
       kerf --version
       kerf --bench <project.json> --bench-out <metrics.json>
       kerf --bench-generate <dir> [--bench-size <objects>]
       kerf --bench-report <metrics.json> <samples.tsv> <results-dir>";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Launch {
    App { open: Option<PathBuf> },
    Version,
    Bench { project: PathBuf, out: PathBuf },
    Generate { dir: PathBuf, size: usize },
    Report { metrics: PathBuf, samples: PathBuf, results: PathBuf },
}

/// Parses the arguments after the program name. macOS adds `-psn_*` to apps opened from Finder; it is ignored.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Launch, &'static str> {
    let args: Vec<String> = args.into_iter().filter(|a| !a.starts_with("-psn_")).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let p = PathBuf::from;
    Ok(match args[..] {
        [] => Launch::App { open: None },
        ["--open", project] => Launch::App { open: Some(p(project)) },
        ["--version" | "-V"] => Launch::Version,
        ["--bench", project, "--bench-out", out] | ["--bench-out", out, "--bench", project] => Launch::Bench { project: p(project), out: p(out) },
        ["--bench-generate", dir] => Launch::Generate { dir: p(dir), size: bench::DEFAULT_OBJECTS },
        ["--bench-generate", dir, "--bench-size", size] | ["--bench-size", size, "--bench-generate", dir] => {
            Launch::Generate { dir: p(dir), size: size.parse().ok().filter(|&n| n > 0).ok_or(USAGE)? }
        }
        ["--bench-report", metrics, samples, results] => Launch::Report { metrics: p(metrics), samples: p(samples), results: p(results) },
        _ => return Err(USAGE),
    })
}

fn main() -> ExitCode {
    let launch = parse_args(std::env::args().skip(1));
    if !matches!(launch, Ok(Launch::App { .. })) {
        attach_console();
    }
    let launch = match launch {
        Ok(launch) => launch,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::from(2);
        }
    };
    let done = match launch {
        Launch::App { open } => run_app(open, None),
        Launch::Version => Ok(version()),
        Launch::Bench { project, out } => run_app(None, Some((project, out))),
        Launch::Generate { dir, size } => generate(&dir, size),
        Launch::Report { metrics, samples, results } => bench::report::report_command(&metrics, &samples, &results),
    };
    match done {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `kerf 0.1.0 (abc1234)`: the package version and the commit `build.rs` stamped, or `unknown` when
/// it was built outside a git checkout.
fn version() -> String {
    format!("kerf {} ({})\n", env!("CARGO_PKG_VERSION"), option_env!("KERF_GIT_COMMIT").unwrap_or("unknown"))
}

/// A Windows release build has no console of its own, so the modes that print attach to the terminal
/// that started them, if there is one, before writing anything.
#[cfg(windows)]
#[expect(unsafe_code, reason = "AttachConsole is a Win32 call with no safe wrapper in std")]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole takes a process id by value and touches no memory of ours. It fails
    // harmlessly when the parent has no console or this process has one already.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
fn attach_console() {}

/// The app, opening `open` in a tab, or playing the benchmark over `bench`'s project with vsync off so
/// every frame renders as soon as the last is done.
fn run_app(open: Option<PathBuf>, bench: Option<(PathBuf, PathBuf)>) -> Result<String, String> {
    let projects = project::projects_dir();
    debug_log::init(projects.as_deref(), WINDOW);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kerf")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../tools/release/icons/kerf-256.png")).expect("the embedded Kerf icon decodes"))
            .with_inner_size(WINDOW)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        vsync: bench.is_none(),
        ..Default::default()
    };
    let transport = Box::new(SimulatedMachine::spawn(SimulatedMachine::PROGRESS_TICK));
    eframe::run_native(
        "Kerf",
        options,
        Box::new(move |cc| {
            let repaint = cc.egui_ctx.clone();
            let fonts = system_fonts::FontCatalogue::scan(move || repaint.request_repaint());
            let mut app = ui::Kerf::new(&cc.egui_ctx, transport, projects, fonts);
            if let Some(path) = open {
                _ = app.open_file(&path);
            }
            if bench.is_some() {
                app.show_canvas();
            }
            Ok(match bench {
                Some((project, out)) => Box::new(ui::bench_mode::Bench::new(app, project, out)),
                None => Box::new(app),
            })
        }),
    )
    .map(|()| String::new())
    .map_err(|e| e.to_string())
}

/// `--bench-generate`: the benchmark scenario's files in `dir`, and its image in the projects' asset
/// store so the app draws it.
fn generate(dir: &Path, size: usize) -> Result<String, String> {
    let design = bench::build(bench::DEFAULT_SEED, size);
    let counts = bench::counts(&design);
    let written = bench::write(dir, &design).map_err(|e| format!("Couldn't write the scenario to {}: {e}", dir.display()))?;
    if let Some(projects) = project::projects_dir() {
        bench::install_image(&projects).map_err(|e| format!("Couldn't store the image in {}: {e}", projects.display()))?;
    }
    let mut out = format!("Scenario: {} objects, {} layers\n", counts.objects, counts.layers);
    out += &format!("Kerf project: {}\nWhole-design SVG: {}\nPer-mat files ({}):\n", written.project.display(), written.svg.display(), written.mats.len());
    for mat in &written.mats {
        out += &format!("  {}\n", mat.display());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guide's module table names every `mod` declared above. A row is `` `name.rs` `` or
    /// `` `name/` ``; `tools/...` rows are not modules and match nothing.
    #[test]
    fn every_module_has_a_row_in_the_guide_table() {
        let main = include_str!("main.rs");
        let readme = include_str!("../docs/guide.md");
        let rows: Vec<&str> = readme
            .lines()
            .filter_map(|l| l.strip_prefix("| `"))
            .filter_map(|l| l.split('`').next())
            .map(|cell| cell.trim_end_matches(".rs").trim_end_matches('/'))
            .collect();
        let missing: Vec<&str> = main.lines().filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';')).filter(|m| !rows.contains(m)).collect();
        assert!(missing.is_empty(), "modules with no row in the docs/guide.md module table: {missing:?}");
    }

    fn parse(args: &[&str]) -> Result<Launch, &'static str> {
        parse_args(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn the_command_line_picks_a_launch_mode_and_anything_else_prints_the_usage() {
        assert_eq!(parse(&[]), Ok(Launch::App { open: None }));
        assert_eq!(parse(&["-psn_0_1234"]), Ok(Launch::App { open: None }));
        assert_eq!(parse(&["--open", "a.json"]), Ok(Launch::App { open: Some("a.json".into()) }));
        assert_eq!(parse(&["--version"]), Ok(Launch::Version));
        assert_eq!(parse(&["-V"]), Ok(Launch::Version));
        assert_eq!(parse(&["--version", "--open", "a.json"]), Err(USAGE));
        let bench = || Launch::Bench { project: "p.json".into(), out: "m.json".into() };
        assert_eq!(parse(&["--bench", "p.json", "--bench-out", "m.json"]), Ok(bench()));
        assert_eq!(parse(&["--bench-out", "m.json", "--bench", "p.json"]), Ok(bench()));
        let generate = |size| Launch::Generate { dir: "d".into(), size };
        assert_eq!(parse(&["--bench-generate", "d"]), Ok(generate(150)));
        assert_eq!(parse(&["--bench-generate", "d", "--bench-size", "1000"]), Ok(generate(1000)));
        assert_eq!(parse(&["--bench-size", "500", "--bench-generate", "d"]), Ok(generate(500)));
        assert_eq!(parse(&["--bench-generate", "d", "--bench-size", "x"]), Err(USAGE));
        assert_eq!(parse(&["--bench-report", "m", "s", "r"]), Ok(Launch::Report { metrics: "m".into(), samples: "s".into(), results: "r".into() }));
        assert_eq!(parse(&["--bench", "p.json"]), Err(USAGE));
        assert_eq!(parse(&["--frobnicate"]), Err(USAGE));
    }

    #[test]
    fn version_names_the_package_version_and_the_commit() {
        use std::io::Write;
        let out = version();
        _ = writeln!(std::io::stderr(), "kerf --version: {}", out.trim_end());
        let rest = out.strip_prefix(&format!("kerf {} (", env!("CARGO_PKG_VERSION"))).expect("starts with the name and version");
        let commit = rest.strip_suffix(")\n").expect("ends with the commit in brackets");
        assert!(!commit.is_empty() && !commit.contains(char::is_whitespace), "one word: {commit:?}");
    }
}

//! The benchmark's numbers: per-phase frame times from `kerf --bench`, process samples the scripts take
//! once a second, the runs kept under `bench-results/`, and the tables the scripts print.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Counts;

/// The stretches of `kerf --bench`, in the order it plays them. The two at-rest phases draw only when the
/// app asks to, as it does in use; every other phase draws flat out, a frame as soon as the last is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The blank canvas the app starts on, at rest, before the project opens.
    Start,
    /// The design on the canvas, at rest.
    Rest,
    Idle,
    DragShape,
    DragGroup,
    Zoom,
    SelectAll,
    UndoRedo,
    MakePrepare,
    CutPreview,
}

impl Phase {
    pub const ALL: [Phase; 10] =
        [Self::Start, Self::Rest, Self::Idle, Self::DragShape, Self::DragGroup, Self::Zoom, Self::SelectAll, Self::UndoRedo, Self::MakePrepare, Self::CutPreview];

    pub fn at_rest(self) -> bool {
        matches!(self, Self::Start | Self::Rest)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "blank, at rest",
            Self::Rest => "loaded, at rest",
            Self::Idle => "idle",
            Self::DragShape => "drag shape",
            Self::DragGroup => "drag group",
            Self::Zoom => "zoom in/out",
            Self::SelectAll => "select all",
            Self::UndoRedo => "undo/redo",
            Self::MakePrepare => "Make Prepare",
            Self::CutPreview => "cut preview",
        }
    }
}

/// Frame times of one phase, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameStats {
    pub frames: usize,
    /// The phase's first frame: the one that opens Make or the cut preview.
    pub first_ms: f32,
    pub median_ms: f32,
    pub p95_ms: f32,
    pub max_ms: f32,
}

impl FrameStats {
    pub fn of(ms: &[f32]) -> Option<Self> {
        let first_ms = *ms.first()?;
        let mut sorted = ms.to_vec();
        sorted.sort_by(f32::total_cmp);
        let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q).round() as usize];
        Some(Self { frames: ms.len(), first_ms, median_ms: at(0.5), p95_ms: at(0.95), max_ms: at(1.0) })
    }
}

/// What `kerf --bench` writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub counts: Counts,
    /// Reading, parsing and sanitising the project, opening its tab and evaluating every shape.
    pub load_ms: f32,
    pub phases: Vec<PhaseFrames>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseFrames {
    pub phase: Phase,
    pub start_unix_ms: u64,
    pub end_unix_ms: u64,
    /// `None` at rest, where the time between frames is the app waiting, not drawing.
    pub frames: Option<FrameStats>,
}

/// One line of a sampler's TSV: `unix_ms  phase  processes  rss_kb  cpu_percent`, summed over every process
/// of the app. Kerf's sampler writes `-` for the phase; its phases come from the metrics' clock instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub unix_ms: u64,
    pub phase: String,
    pub procs: u32,
    pub rss_kb: u64,
    pub cpu: f32,
}

pub fn parse_samples(tsv: &str) -> Result<Vec<Sample>, String> {
    let line = |(n, line): (usize, &str)| -> Result<Sample, String> {
        let bad = |what: &str| format!("samples line {}: {what}: {line:?}", n + 1);
        let f: Vec<&str> = line.split('\t').collect();
        let [unix_ms, phase, procs, rss_kb, cpu] = f[..] else { return Err(bad("not five fields")) };
        Ok(Sample {
            unix_ms: unix_ms.parse().map_err(|_| bad("bad time"))?,
            phase: phase.to_owned(),
            procs: procs.parse().map_err(|_| bad("bad process count"))?,
            rss_kb: rss_kb.parse().map_err(|_| bad("bad RSS"))?,
            cpu: cpu.parse().map_err(|_| bad("bad CPU"))?,
        })
    };
    tsv.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()).map(line).collect()
}

/// Memory and CPU over one phase's samples. CPU is `ps`'s percentage of one core, summed over processes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcStats {
    pub samples: usize,
    pub peak_rss_mb: f32,
    pub mean_cpu: f32,
    pub peak_cpu: f32,
    pub procs: u32,
}

impl ProcStats {
    pub fn of<'a>(samples: impl IntoIterator<Item = &'a Sample>) -> Option<Self> {
        let samples: Vec<&Sample> = samples.into_iter().collect();
        let n = samples.len();
        (n > 0).then(|| Self {
            samples: n,
            peak_rss_mb: samples.iter().map(|s| s.rss_kb).max().unwrap_or(0) as f32 / 1024.0,
            mean_cpu: samples.iter().map(|s| s.cpu).sum::<f32>() / n as f32,
            peak_cpu: samples.iter().map(|s| s.cpu).fold(0.0, f32::max),
            procs: samples.iter().map(|s| s.procs).max().unwrap_or(0),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum App {
    Kerf,
}

/// One benchmark run as `bench-results/<date>.json` keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub app: App,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<Counts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_ms: Option<f32>,
    pub phases: Vec<PhaseRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseRow {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<FrameStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcStats>,
}

impl Run {
    /// Kerf's run: each phase's frame times, and the samples taken while it played.
    pub fn kerf(metrics: &Metrics, samples: &[Sample], at_unix_ms: u64) -> Self {
        let phases = metrics
            .phases
            .iter()
            .map(|p| {
                let during = samples.iter().filter(|s| (p.start_unix_ms..p.end_unix_ms).contains(&s.unix_ms));
                PhaseRow { name: phase_name(p.phase), frames: p.frames, process: ProcStats::of(during) }
            })
            .collect();
        Self { app: App::Kerf, at_unix_ms, counts: Some(metrics.counts), load_ms: Some(metrics.load_ms), phases }
    }
}

fn phase_name(phase: Phase) -> String {
    serde_json::to_value(phase).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn label_of(name: &str) -> &str {
    Phase::ALL.into_iter().find(|&p| phase_name(p) == name).map_or(name, |p| p.label())
}

fn cell(v: Option<f32>, digits: usize) -> String {
    v.map_or_else(|| "-".to_owned(), |v| format!("{v:.digits$}"))
}

/// The table `kerf --bench-report` prints.
pub fn kerf_table(run: &Run) -> String {
    let mut out = String::new();
    if let (Some(c), Some(load)) = (run.counts, run.load_ms) {
        out += &format!("Kerf: {} objects, {} layers; load {load:.1} ms\n", c.objects, c.layers);
    }
    out += &format!(
        "{:<16} {:>6} {:>9} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9} {:>5}\n",
        "phase", "frames", "median ms", "p95 ms", "max ms", "first ms", "RSS MB", "mean CPU", "peak CPU", "procs"
    );
    for p in &run.phases {
        let f = p.frames;
        let pr = p.process;
        out += &format!(
            "{:<16} {:>6} {:>9} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9} {:>5}\n",
            label_of(&p.name),
            f.map_or("-".into(), |f| f.frames.to_string()),
            cell(f.map(|f| f.median_ms), 2),
            cell(f.map(|f| f.p95_ms), 2),
            cell(f.map(|f| f.max_ms), 2),
            cell(f.map(|f| f.first_ms), 2),
            cell(pr.map(|p| p.peak_rss_mb), 0),
            cell(pr.map(|p| p.mean_cpu), 1),
            cell(pr.map(|p| p.peak_cpu), 1),
            pr.map_or("-".into(), |p| p.procs.to_string()),
        );
    }
    out += "The at-rest rows draw only when the app asks to, as in use; every other row draws flat out with\n";
    out += "vsync off, so its CPU is the cost of drawing as fast as possible. RSS is the peak over the phase;\n";
    out += "CPU is ps's percent of one core, summed over processes. A phase shorter than a second may have\n";
    out += "no sample.\n";
    out
}

/// Appends `run` to `<dir>/<today>.json`, a JSON array, and returns the file.
pub fn append(dir: &Path, run: &Run) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(format!("{}.json", jiff::Zoned::now().date()));
    let mut runs = read_runs(&file)?;
    runs.push(run.clone());
    std::fs::write(&file, serde_json::to_string_pretty(&runs).map_err(io::Error::other)?)?;
    Ok(file)
}

fn read_runs(file: &Path) -> io::Result<Vec<Run>> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", file.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `kerf --bench-report`: Kerf's table from its metrics and samples, appended to the results.
pub fn report_command(metrics: &Path, samples: &Path, results: &Path) -> Result<String, String> {
    let metrics: Metrics =
        serde_json::from_str(&std::fs::read_to_string(metrics).map_err(|e| format!("{}: {e}", metrics.display()))?).map_err(|e| format!("{}: {e}", metrics.display()))?;
    let samples = parse_samples(&std::fs::read_to_string(samples).map_err(|e| format!("{}: {e}", samples.display()))?)?;
    let run = Run::kerf(&metrics, &samples, now_unix_ms());
    let file = append(results, &run).map_err(|e| e.to_string())?;
    Ok(format!("{}Appended to {}\n", kerf_table(&run), file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: &str =
        "1000\tstart\t5\t409600\t12.5\n2000\tstart\t6\t512000\t30.0\n3000\tcanvas\t7\t716800\t4.0\n\n4000\tdrag\t7\t819200\t88.0\n5000\tmat_preview\t8\t921600\t20.0\n";

    fn frames(median_ms: f32) -> FrameStats {
        FrameStats { frames: 120, first_ms: 9.0, median_ms, p95_ms: median_ms + 1.0, max_ms: median_ms + 2.0 }
    }

    fn kerf_metrics() -> Metrics {
        let phase = |phase, start_unix_ms, end_unix_ms| PhaseFrames { phase, start_unix_ms, end_unix_ms, frames: (!Phase::at_rest(phase)).then(|| frames(4.0)) };
        Metrics {
            counts: Counts { objects: 150, layers: 420 },
            load_ms: 80.0,
            phases: vec![
                phase(Phase::Start, 0, 1000),
                phase(Phase::Rest, 1000, 2000),
                phase(Phase::DragShape, 2000, 3000),
                phase(Phase::DragGroup, 3000, 4000),
                phase(Phase::MakePrepare, 4000, 5000),
            ],
        }
    }

    fn kerf_samples() -> Vec<Sample> {
        let s = |unix_ms, rss_kb, cpu| Sample { unix_ms, phase: "-".into(), procs: 1, rss_kb, cpu };
        vec![s(500, 102_400, 10.0), s(1500, 204_800, 20.0), s(2500, 307_200, 50.0), s(3500, 204_800, 70.0), s(4500, 256_000, 30.0)]
    }

    #[test]
    fn frame_stats_take_the_first_median_p95_and_max() {
        let ms: Vec<f32> = [30.0].into_iter().chain((1..=19).map(|i| i as f32)).collect();
        assert_eq!(FrameStats::of(&ms), Some(FrameStats { frames: 20, first_ms: 30.0, median_ms: 11.0, p95_ms: 19.0, max_ms: 30.0 }));
        assert_eq!(FrameStats::of(&[]), None);
    }

    #[test]
    fn samples_parse_and_a_bad_line_names_its_number() {
        let samples = parse_samples(SAMPLES).unwrap();
        assert_eq!(samples.len(), 5);
        assert_eq!(samples[1], Sample { unix_ms: 2000, phase: "start".into(), procs: 6, rss_kb: 512_000, cpu: 30.0 });
        assert_eq!(parse_samples("1\tx\t2\tlots\t3").unwrap_err(), "samples line 1: bad RSS: \"1\\tx\\t2\\tlots\\t3\"");
    }

    #[test]
    fn kerf_samples_fall_into_the_phase_whose_clock_window_holds_them() {
        let run = Run::kerf(&kerf_metrics(), &kerf_samples(), 9);
        let process = |name: &str| run.phases.iter().find(|p| p.name == name).unwrap().process.unwrap();
        assert_eq!(process("rest"), ProcStats { samples: 1, peak_rss_mb: 200.0, mean_cpu: 20.0, peak_cpu: 20.0, procs: 1 });
        assert_eq!(run.phases[2].name, "drag_shape");
        assert_eq!(process("make_prepare").peak_rss_mb, 250.0);
    }
}

//! Converts Hershey `.jhf` fonts and SVG fonts to ODS stroke font JSON (`*.odsstroke.json`).
//!
//! cargo run --example stroke_font -- hershey <in.jhf> <out.json> --name "<Name>"
//! cargo run --example stroke_font -- svg <in.svg> <out.json> [--name "<Name>"]

#[allow(dead_code, reason = "the converter uses only the loaders and the writer")]
#[path = "../../src/stroke_font.rs"]
mod stroke_font;

use std::process::ExitCode;

use stroke_font::{Stroke, StrokeFont, StrokeGlyph, StrokeSeg};

const USAGE: &str = "usage:
  stroke_font hershey <in.jhf> <out.json> --name <Name>
  stroke_font svg <in.svg> <out.json> [--name <Name>]";

/// Hershey coordinates grow downwards; these fonts sit on y = 9.
const HERSHEY_BASELINE: i32 = 9;
const HERSHEY_UNITS_PER_EM: f32 = 32.0;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (positional, name) = split_name(args)?;
    let [kind, input, output] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let text = std::fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let font = match kind.as_str() {
        "hershey" => hershey_font(&text, name.ok_or("hershey needs --name")?)?,
        "svg" => {
            let (mut font, report) = StrokeFont::from_svg_font_with_report(&text)?;
            println!("subpaths: {report}");
            if let Some(name) = name {
                font.name = name;
            }
            font
        }
        _ => return Err(USAGE.to_string()),
    };
    println!("{}: units_per_em {}, ascender {}, descender {}", font.name, font.units_per_em, font.ascender, font.descender);
    std::fs::write(output, font.to_json()).map_err(|e| format!("{output}: {e}"))
}

fn split_name(args: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    let mut positional = Vec::new();
    let mut name = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--name" {
            name = Some(args.next().ok_or("--name needs a value")?.clone());
        } else {
            positional.push(arg.clone());
        }
    }
    Ok((positional, name))
}

/// One `.jhf` record: the left and right bounds, then pen-down polylines, in Hershey coordinates.
struct HersheyGlyph {
    left: i32,
    right: i32,
    strokes: Vec<Vec<(i32, i32)>>,
}

/// Each record is a 5-character id, a 3-character vertex count, then that many coordinate pairs,
/// each character offset from 'R'. The first pair is the bounds and " R" lifts the pen. A record
/// may wrap onto further lines.
fn parse_jhf(text: &str) -> Result<Vec<HersheyGlyph>, String> {
    let mut glyphs = Vec::new();
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let header = line.get(..8).ok_or_else(|| format!("short record {line:?}"))?;
        let count: usize = header[5..].trim().parse().map_err(|_| format!("bad vertex count in {line:?}"))?;
        let mut data: Vec<u8> = line.as_bytes()[8..].to_vec();
        while data.len() < 2 * count {
            let more = lines.next().ok_or_else(|| format!("record {:?} ends early", header.trim()))?;
            data.extend_from_slice(more.as_bytes());
        }
        let mut pairs = data[..2 * count].chunks(2).map(|pair| (i32::from(pair[0]) - 82, i32::from(pair[1]) - 82));
        let (left, right) = pairs.next().ok_or("record has no bounds")?;
        let mut strokes: Vec<Vec<(i32, i32)>> = vec![Vec::new()];
        for (x, y) in pairs {
            if (x, y) == (i32::from(b' ') - 82, 0) {
                strokes.push(Vec::new());
            } else {
                strokes.last_mut().expect("never empty").push((x, y));
            }
        }
        strokes.retain(|s| !s.is_empty());
        glyphs.push(HersheyGlyph { left, right, strokes });
    }
    Ok(glyphs)
}

fn hershey_font(text: &str, name: String) -> Result<StrokeFont, String> {
    let records = parse_jhf(text)?;
    let ascii = ' '..='~';
    // The files carry a 96th record after '~', which these fonts leave out.
    if records.len() < ascii.clone().count() {
        return Err(format!("expected a record per printable ASCII character, found {}", records.len()));
    }
    let records = &records[..ascii.clone().count()];
    let point = |left: i32, (x, y): (i32, i32)| ((x - left) as f32, (HERSHEY_BASELINE - y) as f32);
    let glyphs: Vec<(char, StrokeGlyph)> = ascii
        .zip(records)
        .map(|(ch, record)| {
            let strokes = record
                .strokes
                .iter()
                .map(|line| Stroke { start: point(record.left, line[0]), segs: line[1..].iter().map(|&p| StrokeSeg::Line(point(record.left, p))).collect() })
                .collect();
            let advance = (record.right - record.left) as f32;
            (ch, StrokeGlyph { advance, strokes })
        })
        .collect();
    let ys = || records.iter().flat_map(|r| r.strokes.iter().flatten()).map(|&(_, y)| HERSHEY_BASELINE - y);
    let ascender = ys().max().unwrap_or(0) as f32;
    let descender = ys().min().unwrap_or(0) as f32;
    StrokeFont::from_glyphs(name, HERSHEY_UNITS_PER_EM, ascender, descender, glyphs, &[])
}

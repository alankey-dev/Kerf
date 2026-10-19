//! The DXF writer: ASCII R12 (`AC1009`), coordinates in the chosen unit with Y up from the sheet's bottom
//! left. Lines are polyline vertices, cubics become bulged vertices through `biarc::fit`, and a contour
//! that fits one circle is a `CIRCLE`. R12 has no layer hierarchy, so groups are not written.

use std::collections::HashSet;
use std::fmt::Write as _;

use egui::Color32;

use crate::design::{Contour, Pt, Seg};
use crate::units::Units;

use super::biarc::{self, Arc};
use super::{APP_NAME, Sheet};

/// How far an arc may stray from the cubic it replaces, in inches.
pub const ARC_TOLERANCE_IN: f32 = 0.001;
const R12_NAME_MAX: usize = 31;

pub fn document(sheet: &Sheet, units: Units) -> String {
    let names = layer_names(sheet);
    let (w, h) = sheet.size_in;
    let len = |v: f32| num(units.file_from_in(v));
    let mut out = Writer::default();
    out.pair(999, APP_NAME);
    out.section("HEADER");
    out.pair(9, "$ACADVER");
    out.pair(1, "AC1009");
    out.pair(9, "$INSUNITS");
    out.pair(70, if units == Units::Imperial { "1" } else { "4" });
    out.pair(9, "$EXTMIN");
    out.pair(10, "0");
    out.pair(20, "0");
    out.pair(9, "$EXTMAX");
    out.pair(10, &len(w));
    out.pair(20, &len(h));
    out.end_section();
    out.section("TABLES");
    out.table("LTYPE", 1);
    for (code, value) in [(0, "LTYPE"), (2, "CONTINUOUS"), (70, "0"), (3, "Solid line"), (72, "65"), (73, "0"), (40, "0")] {
        out.pair(code, value);
    }
    out.pair(0, "ENDTAB");
    out.table("LAYER", sheet.layers.len());
    for (layer, name) in sheet.layers.iter().zip(&names) {
        for (code, value) in [(0, "LAYER"), (2, name.as_str()), (70, "0"), (62, &aci(layer.colour).to_string()), (6, "CONTINUOUS")] {
            out.pair(code, value);
        }
    }
    out.pair(0, "ENDTAB");
    out.end_section();
    out.section("ENTITIES");
    for (layer, name) in sheet.layers.iter().zip(&names) {
        for contour in &layer.contours {
            let vertices = vertices(contour, h);
            match circle(&vertices, contour.closed) {
                Some((centre, radius)) => {
                    out.pair(0, "CIRCLE");
                    out.pair(8, name);
                    out.pair(10, &len(centre.0));
                    out.pair(20, &len(centre.1));
                    out.pair(40, &len(radius));
                }
                None => {
                    out.pair(0, "POLYLINE");
                    out.pair(8, name);
                    out.pair(66, "1");
                    out.pair(10, "0");
                    out.pair(20, "0");
                    out.pair(30, "0");
                    out.pair(70, if contour.closed { "1" } else { "0" });
                    for (p, bulge) in &vertices {
                        out.pair(0, "VERTEX");
                        out.pair(8, name);
                        out.pair(10, &len(p.0));
                        out.pair(20, &len(p.1));
                        if *bulge != 0.0 {
                            out.pair(42, &num(*bulge));
                        }
                    }
                    out.pair(0, "SEQEND");
                    out.pair(8, name);
                }
            }
        }
    }
    out.end_section();
    out.pair(0, "EOF");
    out.0
}

#[derive(Default)]
struct Writer(String);

impl Writer {
    fn pair(&mut self, code: i32, value: &str) {
        _ = write!(self.0, "{code:>3}\n{value}\n");
    }

    fn section(&mut self, name: &str) {
        self.pair(0, "SECTION");
        self.pair(2, name);
    }

    fn end_section(&mut self) {
        self.pair(0, "ENDSEC");
    }

    fn table(&mut self, name: &str, entries: usize) {
        self.pair(0, "TABLE");
        self.pair(2, name);
        self.pair(70, &entries.to_string());
    }
}

/// Each vertex in Y-up inches with the bulge of the span leaving it.
fn vertices(c: &Contour, height_in: f32) -> Vec<(Pt, f32)> {
    let up = |(x, y): Pt| (x, height_in - y);
    let mut out = vec![(up(c.start), 0.0)];
    for seg in &c.segs {
        let from = out.last().expect("starts with a vertex").0;
        match *seg {
            Seg::Line(p) => out.push((up(p), 0.0)),
            Seg::Cubic(a, b, p) => {
                for arc in biarc::fit(from, up(a), up(b), up(p), ARC_TOLERANCE_IN) {
                    out.last_mut().expect("starts with a vertex").1 = arc.bulge;
                    out.push((arc.end, 0.0));
                }
            }
        }
    }
    let (first, last) = (out[0].0, out[out.len() - 1].0);
    if c.closed && out.len() > 1 && (first.0 - last.0).abs() < 1e-6 && (first.1 - last.1).abs() < 1e-6 {
        out.pop();
    }
    out
}

/// The centre and radius when a closed contour is one whole circle of arcs.
fn circle(vertices: &[(Pt, f32)], closed: bool) -> Option<(Pt, f32)> {
    if !closed || vertices.len() < 2 || vertices.iter().any(|(_, b)| *b == 0.0) {
        return None;
    }
    let spans: Vec<(Pt, f32)> =
        vertices.iter().enumerate().map(|(i, &(p, bulge))| biarc::centre_radius(p, Arc { end: vertices[(i + 1) % vertices.len()].0, bulge })).collect::<Option<_>>()?;
    let (centre, radius) = spans[0];
    let same = spans.iter().all(|&(c, r)| (r - radius).abs() < ARC_TOLERANCE_IN && (c.0 - centre.0).hypot(c.1 - centre.1) < ARC_TOLERANCE_IN);
    let sweep: f32 = vertices.iter().map(|(_, b)| 4.0 * b.atan()).sum();
    (same && (sweep.abs() - std::f32::consts::TAU).abs() < 1e-3).then_some((centre, radius))
}

/// R12 layer names allow letters, digits, `$`, `-` and `_`, up to 31 characters. Spaces between words
/// become `_` and are dropped beside punctuation, so "Score - Heart" becomes `Score-Heart`.
pub fn r12_name(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '$' | '-' | '_') {
            if gap && c.is_ascii_alphanumeric() && out.ends_with(|p: char| p.is_ascii_alphanumeric()) {
                out.push('_');
            }
            out.push(c);
            gap = false;
        } else {
            gap = true;
        }
    }
    out.truncate(R12_NAME_MAX);
    if out.is_empty() { "0".into() } else { out }
}

/// `r12_name` for each layer, kept unique when truncation or dropped characters collide.
fn layer_names(sheet: &Sheet) -> Vec<String> {
    let mut used = HashSet::new();
    sheet
        .layers
        .iter()
        .map(|layer| {
            let base = r12_name(&layer.name);
            let name = (1..)
                .map(|n| match n {
                    1 => base.clone(),
                    _ => {
                        let tail = format!("_{n}");
                        format!("{}{tail}", &base[..base.len().min(R12_NAME_MAX - tail.len())])
                    }
                })
                .find(|name| !used.contains(name))
                .expect("an unused name");
            used.insert(name.clone());
            name
        })
        .collect()
}

/// The nearest AutoCAD Colour Index entry. Index 7 draws black on a white background, so black maps there.
pub fn aci(colour: Color32) -> u8 {
    let distance = |(r, g, b): (u8, u8, u8)| [(r, colour.r()), (g, colour.g()), (b, colour.b())].iter().map(|&(a, b)| (a as i32 - b as i32).pow(2)).sum::<i32>();
    (1..=255).min_by_key(|&i| distance(aci_rgb(i))).unwrap_or(7)
}

/// The standard palette: seven named colours, two greys, 24 hues in five shades and two saturations, then
/// six greys.
fn aci_rgb(i: u8) -> (u8, u8, u8) {
    const NAMED: [(u8, u8, u8); 9] = [(255, 0, 0), (255, 255, 0), (0, 255, 0), (0, 255, 255), (0, 0, 255), (255, 0, 255), (0, 0, 0), (128, 128, 128), (192, 192, 192)];
    const GREYS: [u8; 6] = [51, 91, 132, 173, 214, 255];
    match i {
        1..=9 => NAMED[i as usize - 1],
        10..=249 => {
            let hue = f32::from((i - 10) / 10) * 15.0;
            let shade = (i - 10) % 10;
            let value = [1.0, 0.65, 0.5, 0.3, 0.15][usize::from(shade / 2)];
            let saturation = if shade.is_multiple_of(2) { 1.0 } else { 0.5 };
            hsv(hue, saturation, value)
        }
        _ => {
            let g = GREYS[usize::from(i.saturating_sub(250)).min(5)];
            (g, g, g)
        }
    }
}

fn hsv(hue: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let channel = |n: f32| {
        let k = (n + hue / 60.0) % 6.0;
        let c = v - v * s * k.min(4.0 - k).clamp(0.0, 1.0);
        (c * 255.0).round() as u8
    };
    (channel(5.0), channel(3.0), channel(1.0))
}

/// Six decimals, trailing zeros trimmed.
fn num(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::design::{Design, Operation, ShapeKind};
    use crate::export::tests::{add, mat, request, sampler};
    use crate::export::{Format, Scope, plan};

    /// One DXF record: the code 0 entity name and the pairs that follow it.
    struct Record {
        kind: String,
        pairs: Vec<(i32, String)>,
    }

    impl Record {
        fn text(&self, code: i32) -> Option<&str> {
            self.pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
        }

        fn num(&self, code: i32) -> f32 {
            self.text(code).map_or(0.0, |v| v.parse().expect("a number"))
        }
    }

    fn parse(dxf: &str) -> Vec<Record> {
        let lines: Vec<&str> = dxf.lines().collect();
        let mut records: Vec<Record> = Vec::new();
        for pair in lines.chunks(2) {
            let (code, value) = (pair[0].trim().parse::<i32>().expect("a group code"), pair[1].trim().to_owned());
            match code {
                0 => records.push(Record { kind: value, pairs: Vec::new() }),
                _ => records.last_mut().into_iter().for_each(|r| r.pairs.push((code, value.clone()))),
            }
        }
        records
    }

    /// Each POLYLINE and CIRCLE as dense points, bulges expanded back to arcs.
    fn outlines(records: &[Record]) -> Vec<Vec<Pt>> {
        let mut out = Vec::new();
        for (i, record) in records.iter().enumerate() {
            match record.kind.as_str() {
                "CIRCLE" => {
                    let (c, r) = ((record.num(10), record.num(20)), record.num(40));
                    out.push((0..=256).map(|k| k as f32 * std::f32::consts::TAU / 256.0).map(|a| (c.0 + r * a.cos(), c.1 + r * a.sin())).collect());
                }
                "POLYLINE" => {
                    let closed = record.num(70) as i32 & 1 == 1;
                    let vertices: Vec<(Pt, f32)> = records[i + 1..].iter().take_while(|r| r.kind == "VERTEX").map(|r| ((r.num(10), r.num(20)), r.num(42))).collect();
                    let mut pts = vec![vertices[0].0];
                    let spans = if closed { vertices.len() } else { vertices.len() - 1 };
                    for (k, &(from, bulge)) in vertices.iter().enumerate().take(spans) {
                        pts.extend(biarc::sample(from, Arc { end: vertices[(k + 1) % vertices.len()].0, bulge }, 32));
                    }
                    out.push(pts);
                }
                _ => {}
            }
        }
        out
    }

    fn distance_to_polyline(p: Pt, line: &[Pt]) -> f32 {
        line.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
                (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
            })
            .fold(f32::MAX, f32::min)
    }

    fn cubic_at(p0: Pt, c1: Pt, c2: Pt, p3: Pt, t: f32) -> Pt {
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (a * p0.0 + b * c1.0 + c * c2.0 + d * p3.0, a * p0.1 + b * c1.1 + c * c2.1 + d * p3.1)
    }

    /// The source contour sampled densely, in the DXF's Y-up inches.
    fn source_samples(c: &Contour, height_in: f32) -> Vec<Pt> {
        let up = |(x, y): Pt| (x, height_in - y);
        let mut pts = vec![up(c.start)];
        let mut last = c.start;
        for seg in &c.segs {
            last = match *seg {
                Seg::Line(p) => {
                    pts.push(up(p));
                    p
                }
                Seg::Cubic(a, b, p) => {
                    pts.extend((1..=64).map(|k| up(cubic_at(last, a, b, p, k as f32 / 64.0))));
                    p
                }
            };
        }
        pts
    }

    #[test]
    fn exported_polylines_and_arcs_sample_back_to_the_source_contours() {
        let d = sampler();
        let [file] = &plan(&d, &request(Format::Dxf, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
        let sheet = &file.sheets()[0];
        let read = outlines(&parse(&String::from_utf8(file.bytes(&Default::default())).unwrap()));
        let source: Vec<&Contour> = sheet.layers.iter().flat_map(|l| &l.contours).collect();
        assert_eq!(read.len(), source.len(), "one entity per contour");
        for (i, (line, contour)) in read.iter().zip(source).enumerate() {
            for p in source_samples(contour, sheet.size_in.1) {
                let off = distance_to_polyline(p, line);
                assert!(off <= ARC_TOLERANCE_IN, "contour {i}: {p:?} is {off} from the file");
            }
        }
    }

    #[test]
    fn a_circle_is_one_circle_entity() {
        let mut d = Design::default();
        add(&mut d, ShapeKind::Circle, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
        let [file] = &plan(&d, &request(Format::Dxf, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
        let records = parse(&String::from_utf8(file.bytes(&Default::default())).unwrap());
        let circles: Vec<&Record> = records.iter().filter(|r| r.kind == "CIRCLE").collect();
        assert_eq!(circles.len(), 1);
        assert!((circles[0].num(40) - 1.0).abs() < ARC_TOLERANCE_IN, "radius {}", circles[0].num(40));
        assert!(!records.iter().any(|r| r.kind == "POLYLINE"));
    }

    #[test]
    fn the_header_is_r12_and_layer_names_are_r12_safe() {
        let mut d = Design::default();
        add(&mut d, ShapeKind::Heart, Operation::Score, Color32::RED, (1.0, 1.0));
        add(&mut d, ShapeKind::Builtin(crate::shape_library::BuiltinId::named("Ring").unwrap()), Operation::BasicCut, Color32::BLACK, (4.0, 1.0));
        let [file] = &plan(&d, &request(Format::Dxf, Scope::Design, Units::Metric), "p", mat())[..] else { panic!("one file") };
        let dxf = String::from_utf8(file.bytes(&Default::default())).unwrap();
        let records = parse(&dxf);
        let header = records.iter().find(|r| r.text(2) == Some("HEADER")).expect("a header");
        assert_eq!(header.text(1), Some("AC1009"));
        assert_eq!(header.text(70), Some("4"), "$INSUNITS millimetres");
        let layers: Vec<(&str, &str)> = records.iter().filter(|r| r.kind == "LAYER").map(|r| (r.text(2).unwrap(), r.text(62).unwrap())).collect();
        assert_eq!(layers, [("Score-Heart", "1"), ("Basic_Cut-Ring", "7")]);
        assert!(dxf.starts_with("999\nKerf\n"), "names its maker");
    }

    #[test]
    fn r12_names_drop_spaces_and_stay_within_31_characters() {
        assert_eq!(r12_name("Score - Heart 2"), "Score-Heart_2");
        assert_eq!(r12_name("Basic Cut - Rounded Square"), "Basic_Cut-Rounded_Square");
        let long = r12_name("Medium Foil - A Very Long Built-in Shape Name");
        assert!(long.len() <= 31 && long.chars().all(|c| c.is_ascii_alphanumeric() || "$-_".contains(c)), "{long}");
    }
}

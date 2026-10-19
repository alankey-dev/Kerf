//! Single-line ("stroke") fonts: each glyph is a list of open strokes a pen draws once.
//!
//! The app reads one format, the bundled stroke-font JSON (`*.odsstroke.json`), and SVG fonts a user uploads,
//! which convert in memory to the same model. Everything here is pure: no files, no caches, no app
//! types, so `tools/stroke_font/` can include this module to convert Hershey and SVG font data.
//!
//! Coordinates are font units, y up, with the pen origin at (0, 0) on the baseline, as in an OpenType
//! glyph. Text layout scales them by `size / units_per_em`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::Deserialize;
use svgtypes::{PathParser, PathSegment, SimplePathSegment, SimplifyingPathParser};

/// A point in font units, y up.
pub type P = (f32, f32);

/// Names a stroke font. Saved in projects as `"stroke:<id>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StrokeFontId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum StrokeSeg {
    Line(P),
    Cubic(P, P, P),
}

/// One pen-down stroke. Always open: a loop such as Hershey's "o" ends where it starts, so the pen
/// draws it once and no closing segment is implied.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub start: P,
    pub segs: Vec<StrokeSeg>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeGlyph {
    pub advance: f32,
    /// In the font's drawing order.
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeFont {
    /// The family name the font menu shows.
    pub name: String,
    pub units_per_em: f32,
    /// Above the baseline, positive.
    pub ascender: f32,
    /// Below the baseline, negative.
    pub descender: f32,
    glyphs: Vec<StrokeGlyph>,
    /// Glyph index by character.
    index: HashMap<char, u16>,
    /// Advance adjustment in font units by glyph index pair; from `<hkern>` in SVG fonts.
    kerning: HashMap<(u16, u16), f32>,
}

/// How `from_svg_font_with_report` treated the font's subpaths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SvgFontReport {
    /// Subpaths that were already open strokes.
    pub open: usize,
    /// Closed subpaths whose halves coincide, now their centrelines.
    pub collapsed: usize,
    /// Closed subpaths kept as loops drawn once.
    pub loops: usize,
}

impl fmt::Display for SvgFontReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} open, {} collapsed to centrelines, {} closed loops kept", self.open, self.collapsed, self.loops)
    }
}

impl StrokeFont {
    /// Builds a font from glyphs in any order and pair kerning by character. The first value given
    /// for a pair wins.
    pub fn from_glyphs(
        name: String,
        units_per_em: f32,
        ascender: f32,
        descender: f32,
        mut glyphs: Vec<(char, StrokeGlyph)>,
        kerning: &[(char, char, f32)],
    ) -> Result<Self, String> {
        if !(units_per_em.is_finite() && units_per_em > 0.0) {
            return Err(format!("units per em must be positive, not {units_per_em}"));
        }
        glyphs.sort_by_key(|(ch, _)| *ch);
        if let Some(pair) = glyphs.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(format!("character {:?} has two glyphs", pair[0].0));
        }
        if glyphs.len() > usize::from(u16::MAX) {
            return Err(format!("{} glyphs is more than a font can hold", glyphs.len()));
        }
        let index: HashMap<char, u16> = glyphs.iter().zip(0u16..).map(|((ch, _), i)| (*ch, i)).collect();
        let mut pairs = HashMap::new();
        for &(left, right, value) in kerning {
            let lookup = |ch: char| index.get(&ch).copied().ok_or_else(|| format!("kerning names {ch:?}, which has no glyph"));
            pairs.entry((lookup(left)?, lookup(right)?)).or_insert(value);
        }
        Ok(Self { name, units_per_em, ascender, descender, glyphs: glyphs.into_iter().map(|(_, glyph)| glyph).collect(), index, kerning: pairs })
    }

    /// Parses `*.odsstroke.json`.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: FontFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let glyphs = file
            .glyphs
            .into_iter()
            .map(|glyph| {
                let strokes = glyph.strokes.iter().map(|d| parse_stroke(d).map_err(|e| format!("glyph {:?}: {e}", glyph.ch))).collect::<Result<_, _>>()?;
                let advance = glyph.advance;
                Ok((glyph.ch, StrokeGlyph { advance, strokes }))
            })
            .collect::<Result<_, String>>()?;
        let mut kerning = Vec::new();
        for k in &file.kerning {
            if k.left.is_empty() || k.right.is_empty() {
                return Err("a kerning entry needs at least one left and one right character".to_string());
            }
            kerning.extend(k.left.chars().flat_map(|l| k.right.chars().map(move |r| (l, r, k.value))));
        }
        Self::from_glyphs(file.name, file.units_per_em, file.ascender, file.descender, glyphs, &kerning)
    }

    /// Writes `*.odsstroke.json`: glyphs sorted by code point, one glyph per line, so a change to a
    /// glyph is a one-line diff. Kerning is grouped: each entry applies `value` to every pair in
    /// `left` x `right`.
    pub fn to_json(&self) -> String {
        let chars = self.chars_by_index();
        let mut out = String::from("{\n");
        out += &format!("  \"name\": {},\n", json_string(&self.name));
        out += &format!("  \"units_per_em\": {},\n", num(self.units_per_em));
        out += &format!("  \"ascender\": {},\n", num(self.ascender));
        out += &format!("  \"descender\": {},\n", num(self.descender));
        out += "  \"glyphs\": [\n";
        let glyph_lines: Vec<String> = chars
            .iter()
            .zip(&self.glyphs)
            .map(|(ch, glyph)| {
                let strokes: Vec<String> = glyph.strokes.iter().map(|s| json_string(&stroke_d(s))).collect();
                format!("    {{\"char\": {}, \"advance\": {}, \"strokes\": [{}]}}", json_string(&ch.to_string()), num(glyph.advance), strokes.join(", "))
            })
            .collect();
        out += &glyph_lines.join(",\n");
        out += "\n  ]";
        if !self.kerning.is_empty() {
            let kern_lines: Vec<String> = self
                .kerning_groups(&chars)
                .iter()
                .map(|(l, r, v)| format!("    {{\"left\": {}, \"right\": {}, \"value\": {}}}", json_string(l), json_string(r), num(*v)))
                .collect();
            out += ",\n  \"kerning\": [\n";
            out += &kern_lines.join(",\n");
            out += "\n  ]";
        }
        out += "\n}\n";
        out
    }

    /// Parses an SVG font (`<font>` with `<font-face>` and `<glyph unicode d>`). Each subpath is a
    /// stroke; a closed one whose two halves coincide (the "fake single line" trick) collapses to its
    /// open centreline. Glyphs whose `unicode` is more than one character are skipped.
    pub fn from_svg_font(text: &str) -> Result<Self, String> {
        Self::from_svg_font_with_report(text).map(|(font, _)| font)
    }

    /// `from_svg_font`, also saying how many subpaths collapsed.
    pub fn from_svg_font_with_report(text: &str) -> Result<(Self, SvgFontReport), String> {
        let options = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: 4_000_000 };
        let doc = roxmltree::Document::parse_with_options(text, options).map_err(|e| e.to_string())?;
        let font = doc.descendants().find(|n| n.has_tag_name("font")).ok_or("no <font> element")?;
        let face = font.children().find(|n| n.has_tag_name("font-face"));
        let face_num = |attr: &str| face.and_then(|f| f.attribute(attr)).and_then(parse_num);
        let units_per_em = face_num("units-per-em").unwrap_or(1000.0);
        let ascender = face_num("ascent").unwrap_or(0.8 * units_per_em);
        let descender = -face_num("descent").unwrap_or(0.2 * units_per_em).abs();
        let name = face.and_then(|f| f.attribute("font-family")).or_else(|| font.attribute("id")).unwrap_or("SVG font").to_string();
        let default_advance = font.attribute("horiz-adv-x").and_then(parse_num).unwrap_or(units_per_em / 2.0);
        let tolerance = 0.005 * units_per_em;

        let mut report = SvgFontReport::default();
        let mut glyphs: Vec<(char, StrokeGlyph)> = Vec::new();
        let mut by_name: HashMap<&str, char> = HashMap::new();
        let mut present: HashSet<char> = HashSet::new();
        for node in font.children().filter(|n| n.has_tag_name("glyph")) {
            let Some(ch) = node.attribute("unicode").and_then(single_char) else {
                continue;
            };
            if !present.insert(ch) {
                continue;
            }
            if let Some(glyph_name) = node.attribute("glyph-name") {
                by_name.entry(glyph_name).or_insert(ch);
            }
            let advance = node.attribute("horiz-adv-x").and_then(parse_num).unwrap_or(default_advance);
            let strokes = match node.attribute("d") {
                Some(d) => svg_path_strokes(d, tolerance, &mut report).map_err(|e| format!("glyph {ch:?}: {e}"))?,
                None => Vec::new(),
            };
            glyphs.push((ch, StrokeGlyph { advance, strokes }));
        }

        let mut kerning = Vec::new();
        let mut seen = HashSet::new();
        for node in font.children().filter(|n| n.has_tag_name("hkern")) {
            let Some(k) = node.attribute("k").and_then(parse_num) else {
                continue;
            };
            let side = |u: &str, g: &str| -> Vec<char> {
                let mut chars: Vec<char> = node.attribute(u).into_iter().flat_map(|list| list.split(',')).filter_map(|item| single_char(item.trim())).collect();
                chars.extend(node.attribute(g).into_iter().flat_map(|list| list.split(',')).filter_map(|item| by_name.get(item.trim()).copied()));
                chars
            };
            let lefts = side("u1", "g1");
            let rights = side("u2", "g2");
            for &left in lefts.iter().filter(|c| present.contains(c)) {
                for &right in rights.iter().filter(|c| present.contains(c)) {
                    // SVG fonts apply the first matching <hkern>.
                    if seen.insert((left, right)) && k != 0.0 {
                        kerning.push((left, right, -k));
                    }
                }
            }
        }

        let font = Self::from_glyphs(name, units_per_em, ascender, descender, glyphs, &kerning)?;
        Ok((font, report))
    }

    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        self.index.get(&ch).copied()
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    pub fn glyph(&self, index: u16) -> &StrokeGlyph {
        &self.glyphs[usize::from(index)]
    }

    /// Pair kerning in font units; 0 when the font has none for the pair.
    pub fn kerning(&self, left: u16, right: u16) -> f32 {
        self.kerning.get(&(left, right)).copied().unwrap_or(0.0)
    }

    /// Kerning as `(lefts, rights, value)` entries: rights grouped per left and value, then lefts
    /// merged where they share rights and value. Sorted, so the output is deterministic.
    fn kerning_groups(&self, chars: &[char]) -> Vec<(String, String, f32)> {
        let mut pairs: Vec<(char, u32, char)> = self.kerning.iter().map(|(&(l, r), &v)| (chars[usize::from(l)], v.to_bits(), chars[usize::from(r)])).collect();
        pairs.sort_unstable();
        let mut by_left: Vec<(char, u32, String)> = Vec::new();
        for (l, v, r) in pairs {
            match by_left.last_mut() {
                Some((last_l, last_v, rights)) if *last_l == l && *last_v == v => rights.push(r),
                _ => by_left.push((l, v, r.to_string())),
            }
        }
        let mut merged: BTreeMap<(String, u32), String> = BTreeMap::new();
        for (l, v, rights) in by_left {
            merged.entry((rights, v)).or_default().push(l);
        }
        let mut groups: Vec<(String, String, f32)> = merged.into_iter().map(|((rights, v), lefts)| (lefts, rights, f32::from_bits(v))).collect();
        groups.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        groups
    }

    fn chars_by_index(&self) -> Vec<char> {
        let mut chars = vec!['\0'; self.glyphs.len()];
        for (&ch, &i) in &self.index {
            chars[usize::from(i)] = ch;
        }
        chars
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontFile {
    name: String,
    units_per_em: f32,
    ascender: f32,
    descender: f32,
    glyphs: Vec<GlyphFile>,
    #[serde(default)]
    kerning: Vec<KernFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlyphFile {
    #[serde(rename = "char")]
    ch: char,
    advance: f32,
    strokes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KernFile {
    left: String,
    right: String,
    value: f32,
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serialises")
}

/// Shortest text that reads back as the same `f32`; integral values have no `.0`.
fn num(v: f32) -> String {
    if v == 0.0 { "0".to_string() } else { v.to_string() }
}

fn stroke_d(stroke: &Stroke) -> String {
    let pt = |(x, y): P| format!("{} {}", num(x), num(y));
    let mut d = format!("M{}", pt(stroke.start));
    for seg in &stroke.segs {
        match *seg {
            StrokeSeg::Line(p) => d += &format!(" L{}", pt(p)),
            StrokeSeg::Cubic(c1, c2, p) => {
                d += &format!(" C{} {} {}", pt(c1), pt(c2), pt(p));
            }
        }
    }
    d
}

/// Reads the JSON stroke subset of SVG path data: one absolute `M`, then absolute `L` and `C`.
fn parse_stroke(d: &str) -> Result<Stroke, String> {
    let mut parser = PathParser::from(d);
    let start = match parser.next() {
        Some(Ok(PathSegment::MoveTo { abs: true, x, y })) => (x as f32, y as f32),
        _ => return Err(format!("stroke {d:?} must start with an absolute M")),
    };
    let segs = parser
        .map(|seg| match seg.map_err(|e| e.to_string())? {
            PathSegment::LineTo { abs: true, x, y } => Ok(StrokeSeg::Line((x as f32, y as f32))),
            PathSegment::CurveTo { abs: true, x1, y1, x2, y2, x, y } => Ok(StrokeSeg::Cubic((x1 as f32, y1 as f32), (x2 as f32, y2 as f32), (x as f32, y as f32))),
            _ => Err(format!("stroke {d:?} may only use absolute L and C after M")),
        })
        .collect::<Result<_, _>>()?;
    Ok(Stroke { start, segs })
}

fn parse_num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

/// Splits SVG path data into strokes, collapsing fake single lines and opening closed subpaths.
fn svg_path_strokes(d: &str, tolerance: f32, report: &mut SvgFontReport) -> Result<Vec<Stroke>, String> {
    let mut subpaths: Vec<(Stroke, bool)> = Vec::new();
    let mut current: Option<(Stroke, bool)> = None;
    let mut pen: P = (0.0, 0.0);
    for seg in SimplifyingPathParser::from(d) {
        match seg.map_err(|e| e.to_string())? {
            SimplePathSegment::MoveTo { x, y } => {
                subpaths.extend(current.take());
                pen = (x as f32, y as f32);
                current = Some((Stroke { start: pen, segs: Vec::new() }, false));
            }
            SimplePathSegment::LineTo { x, y } => {
                pen = (x as f32, y as f32);
                push_seg(&mut current, StrokeSeg::Line(pen))?;
            }
            SimplePathSegment::CurveTo { x1, y1, x2, y2, x, y } => {
                let end = (x as f32, y as f32);
                push_seg(&mut current, StrokeSeg::Cubic((x1 as f32, y1 as f32), (x2 as f32, y2 as f32), end))?;
                pen = end;
            }
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                let q = (x1 as f32, y1 as f32);
                let end = (x as f32, y as f32);
                let c1 = lerp(pen, q, 2.0 / 3.0);
                let c2 = lerp(end, q, 2.0 / 3.0);
                push_seg(&mut current, StrokeSeg::Cubic(c1, c2, end))?;
                pen = end;
            }
            SimplePathSegment::ClosePath => {
                if let Some((stroke, closed)) = current.as_mut() {
                    *closed = true;
                    pen = stroke.start;
                }
            }
        }
    }
    subpaths.extend(current);

    let mut strokes = Vec::new();
    for (mut stroke, z) in subpaths.into_iter().filter(|(s, _)| !s.segs.is_empty()) {
        let end = stroke_end(&stroke);
        if !z && dist(end, stroke.start) > tolerance {
            report.open += 1;
            strokes.push(stroke);
            continue;
        }
        if let Some(centre) = collapse_fake_single_line(&flatten(&stroke, tolerance), tolerance) {
            report.collapsed += 1;
            strokes.push(Stroke { start: centre[0], segs: centre[1..].iter().map(|&p| StrokeSeg::Line(p)).collect() });
            continue;
        }
        report.loops += 1;
        if z && end != stroke.start {
            stroke.segs.push(StrokeSeg::Line(stroke.start));
        }
        strokes.push(stroke);
    }
    Ok(strokes)
}

fn push_seg(current: &mut Option<(Stroke, bool)>, seg: StrokeSeg) -> Result<(), String> {
    let (stroke, _) = current.as_mut().ok_or("path data must start with M")?;
    stroke.segs.push(seg);
    Ok(())
}

fn stroke_end(stroke: &Stroke) -> P {
    match stroke.segs.last() {
        Some(StrokeSeg::Line(p) | StrokeSeg::Cubic(_, _, p)) => *p,
        None => stroke.start,
    }
}

/// The stroke as a polyline, curves split finely enough for `tolerance`.
fn flatten(stroke: &Stroke, tolerance: f32) -> Vec<P> {
    let mut points = vec![stroke.start];
    for seg in &stroke.segs {
        let pen = points[points.len() - 1];
        match *seg {
            StrokeSeg::Line(p) => points.push(p),
            StrokeSeg::Cubic(c1, c2, p) => {
                let hull = dist(pen, c1) + dist(c1, c2) + dist(c2, p);
                let steps = (hull / tolerance).ceil().clamp(1.0, 64.0) as usize;
                let at = |i: usize| cubic_at(pen, c1, c2, p, i as f32 / steps as f32);
                points.extend((1..=steps).map(at));
            }
        }
    }
    points
}

fn cubic_at(p0: P, c1: P, c2: P, p3: P, t: f32) -> P {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p0.0 + b * c1.0 + c * c2.0 + d * p3.0, a * p0.1 + b * c1.1 + c * c2.1 + d * p3.1)
}

fn dist(a: P, b: P) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn lerp(a: P, b: P, t: f32) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn nearest_on_segment(p: P, a: P, b: P) -> P {
    let ab = (b.0 - a.0, b.1 - a.1);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 == 0.0 {
        return a;
    }
    let t = (((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / len2).clamp(0.0, 1.0);
    lerp(a, b, t)
}

fn nearest_on_polyline(p: P, line: &[P]) -> P {
    if line.len() == 1 {
        return line[0];
    }
    line.windows(2).map(|w| nearest_on_segment(p, w[0], w[1])).min_by(|a, b| dist(p, *a).total_cmp(&dist(p, *b))).expect("a polyline has a point")
}

/// A closed polyline with extra points so no gap exceeds a spacing, and the arc length at each.
struct Ring {
    points: Vec<P>,
    arc: Vec<f32>,
    len: f32,
}

impl Ring {
    fn new(loop_points: &[P], spacing: f32) -> Option<Self> {
        let mut corners: Vec<P> = Vec::with_capacity(loop_points.len());
        for &p in loop_points {
            if corners.last() != Some(&p) {
                corners.push(p);
            }
        }
        while corners.len() > 1 && corners.last() == corners.first() {
            corners.pop();
        }
        if corners.len() < 2 {
            return None;
        }
        let perimeter: f32 = (0..corners.len()).map(|i| dist(corners[i], corners[(i + 1) % corners.len()])).sum();
        let spacing = spacing.max(perimeter / 4000.0);
        let (mut points, mut arc, mut len) = (Vec::new(), Vec::new(), 0.0);
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let seg = dist(a, b);
            let steps = (seg / spacing).ceil().max(1.0) as usize;
            for k in 0..steps {
                let t = k as f32 / steps as f32;
                points.push(lerp(a, b, t));
                arc.push(len + seg * t);
            }
            len += seg;
        }
        Some(Self { points, arc, len })
    }

    fn at(&self, s: f32) -> P {
        let s = s.rem_euclid(self.len);
        let i = self.arc.partition_point(|&a| a <= s) - 1;
        let j = (i + 1) % self.points.len();
        let next_arc = if j == 0 { self.len } else { self.arc[j] };
        let span = next_arc - self.arc[i];
        let t = if span > 0.0 { (s - self.arc[i]) / span } else { 0.0 };
        lerp(self.points[i], self.points[j], t)
    }

    /// Points from index `from` forwards to index `to`, both included.
    fn run(&self, from: usize, to: usize) -> Vec<P> {
        let n = self.points.len();
        let count = (to + n - from) % n + 1;
        (0..count).map(|k| self.points[(from + k) % n]).collect()
    }
}

/// Collapses a closed polyline that runs out along a line and back over itself to that line's open
/// centreline. `None` unless every point of one half lies within `tolerance` of the other half.
pub fn collapse_fake_single_line(loop_points: &[P], tolerance: f32) -> Option<Vec<P>> {
    let ring = Ring::new(loop_points, tolerance / 2.0)?;
    let n = ring.points.len();
    if n < 3 {
        return None;
    }
    // At a turnaround the path folds back on itself, so points a little before and after it meet.
    let reach = 2.0 * tolerance;
    let fold: Vec<f32> = ring.arc.iter().map(|&s| dist(ring.at(s - reach), ring.at(s + reach))).collect();
    let mut candidates: Vec<usize> = (0..n).filter(|&i| fold[i] <= 2.0 * tolerance && fold[i] <= fold[(i + n - 1) % n] && fold[i] <= fold[(i + 1) % n]).collect();
    candidates.sort_by(|&a, &b| fold[a].total_cmp(&fold[b]));
    candidates.truncate(8);

    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (k, &a) in candidates.iter().enumerate() {
        for &b in &candidates[k + 1..] {
            let apart = (ring.arc[b] - ring.arc[a]).rem_euclid(ring.len);
            if (0.3..=0.7).contains(&(apart / ring.len)) {
                pairs.push((a, b));
            }
        }
    }
    pairs.sort_by(|p, q| (fold[p.0] + fold[p.1]).total_cmp(&(fold[q.0] + fold[q.1])));

    pairs.into_iter().find_map(|(a, b)| {
        let out = ring.run(a, b);
        let back = ring.run(b, a);
        let coincide = |from: &[P], to: &[P]| from.iter().all(|&p| dist(p, nearest_on_polyline(p, to)) <= tolerance);
        if !(coincide(&out, &back) && coincide(&back, &out)) {
            return None;
        }
        let centre: Vec<P> = out.iter().map(|&p| lerp(p, nearest_on_polyline(p, &back), 0.5)).collect();
        Some(simplify(&centre, tolerance / 4.0))
    })
}

/// Douglas-Peucker: drops points within `epsilon` of the line through their neighbours' survivors.
fn simplify(points: &[P], epsilon: f32) -> Vec<P> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let last = points.len() - 1;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[last] = true;
    let mut spans = vec![(0, last)];
    while let Some((a, b)) = spans.pop() {
        let far = (a + 1..b).map(|i| (i, dist(points[i], nearest_on_segment(points[i], points[a], points[b])))).max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, d)) = far
            && d > epsilon
        {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    points.iter().zip(keep).filter_map(|(&p, k)| k.then_some(p)).collect()
}

/// The direction to draw each stroke in, given each stroke's `(start, end)` in drawing order:
/// `true` reverses it. Order is kept; only directions change, chosen to minimise the total pen-up
/// travel, so the result is never worse than drawing every stroke forwards.
pub fn pen_directions(ends: &[(P, P)]) -> Vec<bool> {
    let entry = |i: usize, rev: bool| if rev { ends[i].1 } else { ends[i].0 };
    let exit = |i: usize, rev: bool| if rev { ends[i].0 } else { ends[i].1 };
    // cost[d] is the least travel up to the current stroke drawn in direction d; back[i][d] is the
    // previous stroke's direction on that best route.
    let mut cost = [0.0f32; 2];
    let mut back: Vec<[bool; 2]> = Vec::with_capacity(ends.len());
    for i in 1..ends.len() {
        let mut next = [0.0f32; 2];
        let mut from = [false; 2];
        for (d, rev) in [false, true].into_iter().enumerate() {
            let via = |prev: bool| cost[usize::from(prev)] + dist(exit(i - 1, prev), entry(i, rev));
            let (fwd, bwd) = (via(false), via(true));
            (next[d], from[d]) = if bwd < fwd { (bwd, true) } else { (fwd, false) };
        }
        cost = next;
        back.push(from);
    }
    if ends.is_empty() {
        return Vec::new();
    }
    let mut rev = cost[1] < cost[0];
    let mut dirs = vec![rev];
    for from in back.iter().rev() {
        rev = from[usize::from(rev)];
        dirs.push(rev);
    }
    dirs.reverse();
    dirs
}

/// Total pen-up travel drawing `ends` in order with the given directions.
pub fn pen_up_distance(ends: &[(P, P)], reversed: &[bool]) -> f32 {
    debug_assert_eq!(ends.len(), reversed.len());
    let oriented: Vec<(P, P)> = ends.iter().zip(reversed).map(|(&(a, b), &rev)| if rev { (b, a) } else { (a, b) }).collect();
    oriented.windows(2).map(|w| dist(w[0].1, w[1].0)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERSHEY_SIMPLEX: &str = include_str!("../assets/fonts/hershey/hershey-simplex.odsstroke.json");

    fn svg_font(glyphs: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs>
<font id="T" horiz-adv-x="500"><font-face font-family="Test Font" units-per-em="1000" ascent="800" descent="-200"/>
{glyphs}
</font></defs></svg>"#
        )
    }

    fn polyline(stroke: &Stroke) -> Vec<P> {
        let mut points = vec![stroke.start];
        for seg in &stroke.segs {
            match seg {
                StrokeSeg::Line(p) => points.push(*p),
                StrokeSeg::Cubic(..) => panic!("expected lines only"),
            }
        }
        points
    }

    fn glyph_of(font: &StrokeFont, ch: char) -> &StrokeGlyph {
        font.glyph(font.glyph_index(ch).expect("glyph present"))
    }

    fn assert_near_vertical(points: &[P], x: f32, y_ends: (f32, f32), tolerance: f32) {
        for p in points {
            assert!((p.0 - x).abs() <= tolerance, "{p:?} strays from x = {x}");
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        let (lo, hi) = if first.1 < last.1 { (first, last) } else { (last, first) };
        assert!((lo.1 - y_ends.0).abs() <= tolerance, "low end {lo:?}");
        assert!((hi.1 - y_ends.1).abs() <= tolerance, "high end {hi:?}");
    }

    #[test]
    fn fake_single_line_l_from_svg_font_collapses_to_its_centreline() {
        // A zero-width "l" whose loop starts mid-stroke, and a 2-unit wide one that starts at a corner.
        let text = svg_font(
            r#"<glyph unicode="l" horiz-adv-x="250" d="M100 300 L100 700 L100 0 Z"/>
<glyph unicode="I" horiz-adv-x="250" d="M99 0 L101 0 L101 700 L99 700 Z"/>"#,
        );
        let (font, report) = StrokeFont::from_svg_font_with_report(&text).unwrap();
        assert_eq!(report.collapsed, 2, "{report}");
        let em = 0.005 * 1000.0;
        for ch in ['l', 'I'] {
            let glyph = glyph_of(&font, ch);
            assert_eq!(glyph.strokes.len(), 1);
            let points = polyline(&glyph.strokes[0]);
            assert_eq!(points.len(), 2, "{ch}: {points:?}");
            assert_near_vertical(&points, 100.0, (0.0, 700.0), em);
        }
        assert_eq!(glyph_of(&font, 'l').advance, 250.0);
    }

    #[test]
    fn fake_single_line_curve_collapses_to_a_polyline_on_the_curve() {
        let text = svg_font(r#"<glyph unicode="n" d="M0 0 C0 300 300 300 300 0 C300 300 0 300 0 0 Z"/>"#);
        let font = StrokeFont::from_svg_font(&text).unwrap();
        let strokes = &glyph_of(&font, 'n').strokes;
        assert_eq!(strokes.len(), 1);
        let points = polyline(&strokes[0]);
        let (first, last) = (points[0], points[points.len() - 1]);
        let (left, right) = if first.0 < last.0 { (first, last) } else { (last, first) };
        assert!(dist(left, (0.0, 0.0)) <= 5.0, "{left:?}");
        assert!(dist(right, (300.0, 0.0)) <= 5.0, "{right:?}");
        let arch: Vec<P> = (0..=200)
            .map(|i| {
                let t = i as f32 / 200.0;
                cubic_at((0.0, 0.0), (0.0, 300.0), (300.0, 300.0), (300.0, 0.0), t)
            })
            .collect();
        for p in &points {
            assert!(dist(*p, nearest_on_polyline(*p, &arch)) <= 5.0, "{p:?}");
        }
    }

    #[test]
    fn bundled_relief_json_is_a_fresh_conversion_of_its_svg_font() {
        let svg = include_str!("../assets/fonts/relief-singleline/ReliefSingleLineSVG-Regular.svg");
        let json = include_str!("../assets/fonts/relief-singleline/relief-singleline.odsstroke.json");
        let mut converted = StrokeFont::from_svg_font(svg).unwrap();
        converted.name = "Relief SingleLine".to_string();
        assert_eq!(StrokeFont::from_json(json).unwrap(), converted);
        assert_eq!(converted.to_json(), json);
    }

    #[test]
    fn collapse_finds_the_turnarounds_wherever_the_loop_starts() {
        let centre = [(0.0, 0.0), (300.0, 0.0), (300.0, 400.0)];
        let mut out_and_back: Vec<P> = centre.to_vec();
        out_and_back.extend(centre[1..centre.len() - 1].iter().rev());
        for shift in 0..out_and_back.len() {
            let mut ring = out_and_back.clone();
            ring.rotate_left(shift);
            let line = collapse_fake_single_line(&ring, 2.0).expect("collapses");
            assert_eq!(line.len(), 3, "shift {shift}: {line:?}");
            let forwards = line.iter().zip(&centre).all(|(a, b)| dist(*a, *b) < 1.0);
            let backwards = line.iter().rev().zip(&centre).all(|(a, b)| dist(*a, *b) < 1.0);
            assert!(forwards || backwards, "shift {shift}: {line:?}");
        }
    }

    #[test]
    fn collapse_rejects_a_real_loop() {
        let square = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        assert_eq!(collapse_fake_single_line(&square, 5.0), None);
        let sliver = [(0.0, 0.0), (300.0, 0.0), (300.0, 20.0), (0.0, 20.0)];
        assert_eq!(collapse_fake_single_line(&sliver, 5.0), None);
    }

    #[test]
    fn svg_font_keeps_open_paths_and_opens_real_loops() {
        let text = svg_font(
            r#"<glyph unicode="o" d="M0 0 L100 0 L100 100 L0 100 Z"/>
<glyph unicode="c" d="M100 0 h-100 v100 h100"/>
<glyph unicode="s" d="M0 0 Q50 100 100 0"/>
<glyph unicode=" " horiz-adv-x="300"/>
<glyph unicode="fi" d="M0 0 L1 1"/>"#,
        );
        let (font, report) = StrokeFont::from_svg_font_with_report(&text).unwrap();
        assert_eq!(report, SvgFontReport { open: 2, collapsed: 0, loops: 1 });
        let o = &glyph_of(&font, 'o').strokes[0];
        assert_eq!(o.segs.len(), 4);
        assert_eq!(stroke_end(o), o.start);
        let c = &glyph_of(&font, 'c').strokes[0];
        assert_eq!(polyline(c), vec![(100.0, 0.0), (0.0, 0.0), (0.0, 100.0), (100.0, 100.0)]);
        let s = &glyph_of(&font, 's').strokes[0];
        assert!(matches!(s.segs[..], [StrokeSeg::Cubic(_, _, (100.0, 0.0))]));
        assert_eq!(glyph_of(&font, ' ').advance, 300.0);
        assert!(glyph_of(&font, ' ').strokes.is_empty());
        assert_eq!(glyph_of(&font, 'o').advance, 500.0);
        assert_eq!(font.name, "Test Font");
        assert_eq!((font.ascender, font.descender), (800.0, -200.0));
        assert_eq!(font.glyph_index('f'), None);
    }

    #[test]
    fn svg_font_kerning_reads_characters_and_glyph_names() {
        let text = svg_font(
            r#"<glyph unicode="A" glyph-name="A" d="M0 0 L1 1"/>
<glyph unicode="&#xc1;" glyph-name="Aacute" d="M0 0 L1 1"/>
<glyph unicode="V" glyph-name="V" d="M0 0 L1 1"/>
<glyph unicode="T" glyph-name="T" d="M0 0 L1 1"/>
<hkern u1="T" u2="A" k="40"/>
<hkern g1="A,Aacute" g2="V,T" k="90"/>
<hkern u1="T" u2="A" k="10"/>"#,
        );
        let font = StrokeFont::from_svg_font(&text).unwrap();
        let kern = |l: char, r: char| font.kerning(font.glyph_index(l).unwrap(), font.glyph_index(r).unwrap());
        assert_eq!(kern('T', 'A'), -40.0);
        assert_eq!(kern('Á', 'V'), -90.0);
        assert_eq!(kern('A', 'T'), -90.0);
        assert_eq!(kern('V', 'A'), 0.0);
    }

    #[test]
    fn json_round_trips() {
        let glyphs = vec![
            (
                'b',
                StrokeGlyph {
                    advance: 12.5,
                    strokes: vec![Stroke { start: (1.0, -2.0), segs: vec![StrokeSeg::Line((3.0, 4.0)), StrokeSeg::Cubic((0.1, 1.0 / 3.0), (5.0, 6.0), (-7.25, 8.0))] }],
                },
            ),
            (' ', StrokeGlyph { advance: 8.0, strokes: Vec::new() }),
            ('"', StrokeGlyph { advance: 4.0, strokes: vec![Stroke { start: (1.0, 20.0), segs: vec![StrokeSeg::Line((1.0, 16.0))] }] }),
        ];
        let font = StrokeFont::from_glyphs(
            "Round \"Trip\"".to_string(),
            32.0,
            24.0,
            -8.0,
            glyphs,
            &[('b', '"', -2.0), ('b', 'b', -2.0), ('"', '"', -2.0), ('"', 'b', -2.0), (' ', 'b', 1.5)],
        )
        .unwrap();
        let json = font.to_json();
        assert_eq!(StrokeFont::from_json(&json).unwrap(), font);
        assert!(json.contains(r#"{"char": "b", "advance": 12.5, "strokes": ["M1 -2 L3 4 C0.1 0.33333334 5 6 -7.25 8"]}"#), "{json}");
        let lines: Vec<&str> = json.lines().collect();
        let glyph_lines: Vec<&&str> = lines.iter().filter(|l| l.contains("\"char\"")).collect();
        assert_eq!(glyph_lines.len(), 3);
        assert!(glyph_lines[0].contains(r#""char": " ""#), "sorted by code point");
        let kern_lines: Vec<&str> = lines.iter().copied().filter(|l| l.contains("\"left\"")).collect();
        assert_eq!(kern_lines, [r#"    {"left": " ", "right": "b", "value": 1.5},"#, r#"    {"left": "\"b", "right": "\"b", "value": -2}"#], "{json}");
    }

    #[test]
    fn json_kerning_expands_groups_and_the_first_value_wins() {
        let json = r#"{"name": "N", "units_per_em": 32, "ascender": 24, "descender": -8,
            "glyphs": [{"char": "A", "advance": 18, "strokes": []}, {"char": "T", "advance": 18, "strokes": []}, {"char": "V", "advance": 18, "strokes": []}],
            "kerning": [{"left": "AT", "right": "TV", "value": -3}, {"left": "A", "right": "V", "value": 5}]}"#;
        let font = StrokeFont::from_json(json).unwrap();
        let kern = |l: char, r: char| font.kerning(font.glyph_index(l).unwrap(), font.glyph_index(r).unwrap());
        assert_eq!([kern('A', 'T'), kern('A', 'V'), kern('T', 'T'), kern('T', 'V'), kern('V', 'A')], [-3.0, -3.0, -3.0, -3.0, 0.0]);
        let empty = json.replace(r#""left": "A""#, r#""left": """#);
        assert!(StrokeFont::from_json(&empty).is_err());
    }

    #[test]
    fn json_without_kerning_reads_and_writes_none() {
        let json = r#"{"name": "N", "units_per_em": 32, "ascender": 24, "descender": -8,
            "glyphs": [{"char": "A", "advance": 18, "strokes": ["M9 21 L1 0", "M9 21 L17 0", "M4 7 L14 7"]}]}"#;
        let font = StrokeFont::from_json(json).unwrap();
        assert_eq!(glyph_of(&font, 'A').strokes.len(), 3);
        assert!(!font.to_json().contains("kerning"));
    }

    #[test]
    fn json_rejects_paths_outside_the_stroke_subset() {
        for d in ["m9 21 L1 0", "M9 21 l1 0", "M9 21 L1 0 Z", "M9 21 L1 0 M3 3 L4 4", "L1 0"] {
            let json = format!(
                r#"{{"name": "N", "units_per_em": 32, "ascender": 24, "descender": -8,
                "glyphs": [{{"char": "A", "advance": 18, "strokes": ["{d}"]}}]}}"#
            );
            assert!(StrokeFont::from_json(&json).is_err(), "{d}");
        }
    }

    #[test]
    fn hershey_simplex_l_is_one_open_stroke() {
        let font = StrokeFont::from_json(HERSHEY_SIMPLEX).unwrap();
        let l = glyph_of(&font, 'l');
        assert_eq!(l.strokes.len(), 1);
        assert_ne!(stroke_end(&l.strokes[0]), l.strokes[0].start);
    }

    #[test]
    fn hershey_simplex_o_is_one_loop_drawn_once() {
        let font = StrokeFont::from_json(HERSHEY_SIMPLEX).unwrap();
        let o = glyph_of(&font, 'o');
        assert_eq!(o.strokes.len(), 1);
        let points = polyline(&o.strokes[0]);
        assert_eq!(points.first(), points.last());
        let mut edges: Vec<(P, P)> = points.windows(2).map(|w| if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) }).collect();
        let count = edges.len();
        edges.sort_by(|a, b| a.partial_cmp(b).unwrap());
        edges.dedup();
        assert_eq!(edges.len(), count, "a segment is drawn twice");
    }

    #[test]
    fn hershey_simplex_covers_printable_ascii() {
        let font = StrokeFont::from_json(HERSHEY_SIMPLEX).unwrap();
        assert!((' '..='~').all(|ch| font.glyph_index(ch).is_some()));
        assert!(glyph_of(&font, ' ').strokes.is_empty());
        assert_eq!(font.units_per_em, 32.0);
    }

    fn all_forwards(ends: &[(P, P)]) -> f32 {
        pen_up_distance(ends, &vec![false; ends.len()])
    }

    #[test]
    fn pen_directions_never_beat_by_drawing_all_forwards() {
        let cases: Vec<Vec<(P, P)>> = vec![
            vec![],
            vec![((0.0, 0.0), (1.0, 0.0))],
            vec![((0.0, 0.0), (10.0, 0.0)), ((10.0, 1.0), (0.0, 1.0))],
            vec![((0.0, 0.0), (0.0, 10.0)), ((5.0, 0.0), (5.0, 10.0)), ((10.0, 0.0), (10.0, 10.0)), ((15.0, 10.0), (15.0, 0.0))],
            vec![((3.0, 7.0), (9.0, 2.0)), ((1.0, 1.0), (8.0, 8.0)), ((4.0, 0.0), (4.0, 5.0)), ((9.0, 9.0), (0.0, 3.0)), ((2.0, 6.0), (6.0, 2.0))],
        ];
        for ends in cases {
            let dirs = pen_directions(&ends);
            assert_eq!(dirs.len(), ends.len());
            let best = pen_up_distance(&ends, &dirs);
            assert!(best <= all_forwards(&ends) + 1e-4, "{ends:?}");
            let brute = (0..1u32 << ends.len())
                .map(|mask| {
                    let rev: Vec<bool> = (0..ends.len()).map(|i| mask >> i & 1 == 1).collect();
                    pen_up_distance(&ends, &rev)
                })
                .fold(f32::INFINITY, f32::min);
            assert!((best - brute).abs() < 1e-4, "{best} vs optimum {brute}");
        }
    }

    #[test]
    fn pen_directions_reverse_a_stroke_when_that_saves_travel() {
        // Three parallel strokes all drawn upwards: drawing the middle one downwards saves two climbs.
        let ends = [((0.0, 0.0), (0.0, 10.0)), ((5.0, 0.0), (5.0, 10.0)), ((10.0, 0.0), (10.0, 10.0))];
        let dirs = pen_directions(&ends);
        assert_eq!(dirs, vec![false, true, false]);
        assert!(pen_up_distance(&ends, &dirs) < all_forwards(&ends));
        assert_eq!(pen_up_distance(&ends, &dirs), 10.0);
    }
}

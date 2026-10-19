//! Lays text out into bezier contours in inch space, so text flows through `to_svg_path_d`, the
//! geometry layer and Make exactly like any path. Outline fonts give closed contours from `ttf-parser`;
//! writing fonts give open strokes from `stroke_font`, turned so the pen travels least between them.
//!
//! The text box is typographic, not the ink bounds: its height is a whole number of lines from the
//! font's ascender, descender and line gap, so it does not jump as descenders come and go.

use std::borrow::Cow;
use std::sync::Arc;

use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

use crate::design::{Contour, Pt, Seg};
use crate::stroke_font::{StrokeFont, StrokeFontId, StrokeSeg};

const INTER: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Inter-Regular.ttf"));
const INTER_MEDIUM: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Inter-Medium.ttf"));
const INTER_SEMIBOLD: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/Inter-SemiBold.ttf"));
const SPACE_GROTESK: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/SpaceGrotesk.ttf"));

/// No italic faces are bundled, so italic styles shear the upright outlines by this angle. System
/// families without an italic or oblique face get the same shear.
const ITALIC_SHEAR_DEG: f32 = 12.0;

/// Synthetic bold widens every stem by this fraction of the em, half on each side.
const SYNTHETIC_BOLD_EM: f32 = 1.0 / 24.0;

/// The font a text is set in: one of the bundled faces, an installed system family by name, or a writing
/// font. Saved as a bare string, so projects from before system fonts still read `"Inter"` and
/// `"SpaceGrotesk"`, a system family keeps its name on a machine that lacks it, and a writing font saves
/// as `"stroke:<id>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
pub enum FontRef {
    #[default]
    Inter,
    SpaceGrotesk,
    System(String),
    Stroke(StrokeFontId),
}

const STROKE_PREFIX: &str = "stroke:";

impl From<String> for FontRef {
    fn from(name: String) -> Self {
        match name.as_str() {
            "Inter" => Self::Inter,
            "SpaceGrotesk" => Self::SpaceGrotesk,
            _ => match name.strip_prefix(STROKE_PREFIX) {
                Some(id) => Self::Stroke(StrokeFontId(id.to_owned())),
                None => Self::System(name),
            },
        }
    }
}

impl From<FontRef> for String {
    fn from(font: FontRef) -> Self {
        match font {
            FontRef::Inter => "Inter".into(),
            FontRef::SpaceGrotesk => "SpaceGrotesk".into(),
            FontRef::System(name) => name,
            FontRef::Stroke(id) => format!("{STROKE_PREFIX}{}", id.0),
        }
    }
}

impl FontRef {
    pub const BUNDLED: [FontRef; 2] = [Self::Inter, Self::SpaceGrotesk];

    /// The name the toolbar and menu show. A writing font that is not available shows its id.
    pub fn label(&self) -> Cow<'_, str> {
        match self {
            Self::Inter => "Inter".into(),
            Self::SpaceGrotesk => "Space Grotesk".into(),
            Self::System(name) => name.into(),
            Self::Stroke(id) => crate::system_fonts::stroke(id).map_or_else(|| id.0.as_str().into(), |f| f.name.clone().into()),
        }
    }

    /// A single-line font a pen draws once.
    pub fn is_writing(&self) -> bool {
        matches!(self, Self::Stroke(_))
    }

    /// Where `style`'s glyphs come from. Inter ships one static face per weight, with SemiBold standing
    /// in for Bold; Space Grotesk is a variable font, so its `wght` axis is set instead. A system family
    /// that is not installed, or not yet discovered, falls back to Inter, and a writing font that is not
    /// available to the fallback writing font.
    fn glyphs(&self, style: FontStyle) -> GlyphSource {
        let outline = GlyphSource::Outline;
        match self {
            Self::Inter => outline(inter_face(style)),
            Self::SpaceGrotesk => {
                outline(FaceSource { bytes: Arc::new(SPACE_GROTESK), index: 0, wght: Some(style.weight().wght()), embolden: false, shear: style.italic() })
            }
            Self::System(family) => outline(crate::system_fonts::face(family, style).unwrap_or_else(|| inter_face(style))),
            Self::Stroke(id) => {
                let fallback =
                    || crate::system_fonts::stroke(&StrokeFontId(crate::system_fonts::FALLBACK_STROKE.into())).expect("bundled writing fonts are always present");
                GlyphSource::Stroke { font: crate::system_fonts::stroke(id).unwrap_or_else(fallback), shear: style.italic() }
            }
        }
    }
}

fn inter_face(style: FontStyle) -> FaceSource {
    let bytes: &'static [u8] = match style.weight() {
        Weight::Regular => INTER,
        Weight::Medium => INTER_MEDIUM,
        Weight::Bold => INTER_SEMIBOLD,
    };
    FaceSource { bytes: Arc::new(bytes), index: 0, wght: None, embolden: false, shear: style.italic() }
}

/// Outline faces from `ttf-parser`, or a writing font's strokes. Writing fonts have one weight.
enum GlyphSource {
    Outline(FaceSource),
    Stroke { font: Arc<StrokeFont>, shear: bool },
}

pub type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// A face's bytes plus whatever the requested style needs that the face cannot supply itself. Layout
/// treats bundled and system faces alike through this.
#[derive(Clone)]
pub struct FaceSource {
    pub bytes: FontBytes,
    /// The face's index within a collection (`.ttc`).
    pub index: u32,
    /// Sets the `wght` axis of a variable face.
    pub wght: Option<f32>,
    /// Synthetic bold, for a family with no bold face.
    pub embolden: bool,
    /// Synthetic italic, for a family with no italic or oblique face.
    pub shear: bool,
}

impl FaceSource {
    fn parse(&self) -> Option<Face<'_>> {
        let mut face = Face::parse((*self.bytes).as_ref(), self.index).ok()?;
        if let Some(wght) = self.wght {
            face.set_variation(Tag::from_bytes(b"wght"), wght)?;
        }
        Some(face)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum FontStyle {
    #[default]
    Regular,
    Medium,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    pub(crate) fn weight(self) -> Weight {
        match self {
            Self::Regular | Self::Italic => Weight::Regular,
            Self::Medium => Weight::Medium,
            Self::Bold | Self::BoldItalic => Weight::Bold,
        }
    }

    pub(crate) fn italic(self) -> bool {
        matches!(self, Self::Italic | Self::BoldItalic)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Weight {
    Regular,
    Medium,
    Bold,
}

impl Weight {
    pub(crate) fn wght(self) -> f32 {
        match self {
            Self::Regular => 400.0,
            Self::Medium => 500.0,
            Self::Bold => 700.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Centre,
    Right,
}

/// Where the lines sit when the box is taller than the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// Everything that shapes the outlines, borrowed from a `ShapeKind::Text`.
#[derive(Clone, Copy, Debug)]
pub struct TextSpec<'a> {
    pub content: &'a str,
    pub font: &'a FontRef,
    /// The em height in inches.
    pub size_in: f32,
    pub style: FontStyle,
    /// Thousandths of an em added after each glyph except the last on a line.
    pub letter_space: f32,
    /// Thousandths of an em added between lines.
    pub line_space: f32,
    pub align: TextAlign,
    /// Wraps words at this width when set.
    pub wrap_in: Option<f32>,
    /// A signed circle diameter in inches that bends a single unwrapped line: 0 is straight, positive
    /// sets the text on top of the circle and negative underneath it. Ignored when `path` is set.
    pub curve_in: f32,
    /// A path a single unwrapped line follows, the general form of `curve_in`. When set it takes over
    /// from the curve.
    pub path: Option<&'a crate::text_path::TextPath>,
}

impl Default for TextSpec<'_> {
    fn default() -> Self {
        Self {
            content: "",
            font: &FontRef::Inter,
            size_in: 1.0,
            style: FontStyle::Regular,
            letter_space: 0.0,
            line_space: 0.0,
            align: TextAlign::Left,
            wrap_in: None,
            curve_in: 0.0,
            path: None,
        }
    }
}

/// One character's outline in the box's local frame (inches, y down), with the line it sits on and
/// its index among `content.chars()`, so text can split into letters and lines.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphOutline {
    pub contours: Vec<Contour>,
    pub line: usize,
    pub char_index: usize,
    /// Where the glyph's pen origin landed on its baseline.
    pub pen: Pt,
    /// The glyph's turn about `pen` in radians, clockwise on the canvas; non-zero only on a curve.
    pub angle: f32,
}

/// One laid-out line: its characters as typed, less any space a wrap broke at, and the top left of
/// its pen run in the box.
#[derive(Clone, Debug, PartialEq)]
pub struct LineSpan {
    pub text: String,
    pub origin: Pt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    /// Only characters with ink; spaces and blank lines leave no entry.
    pub glyphs: Vec<GlyphOutline>,
    pub lines: Vec<LineSpan>,
    /// The text box. Ink can overflow it (accents, a word wider than `wrap_in`).
    pub size: Pt,
    /// Set when a path text runs off the end of an open path, so the canvas can mark the overflow.
    pub overflow: bool,
    /// How the run sits on its path, for the canvas guide line and handles; `None` for straight text.
    pub path: Option<crate::text_path::PathFrame>,
}

impl TextLayout {
    pub fn contours(&self) -> Vec<Contour> {
        self.glyphs.iter().flat_map(|g| g.contours.iter().cloned()).collect()
    }
}

/// Lays `spec` out into a box at the origin. `None` only when the font fails to parse.
pub fn layout(spec: &TextSpec) -> Option<TextLayout> {
    let source = spec.font.glyphs(spec.style);
    let (metrics, sheared) = match &source {
        GlyphSource::Outline(face) => (Metrics::new(Glyphs::Outline(face.parse()?), spec, face.embolden), face.shear),
        GlyphSource::Stroke { font, shear } => (Metrics::new(Glyphs::Stroke(font.clone()), spec, false), *shear),
    };
    let lines: Vec<Vec<(usize, char)>> = paragraphs(spec.content).into_iter().flat_map(|p| metrics.wrap(p, spec.wrap_in)).collect();

    let widths: Vec<f32> = lines.iter().map(|l| metrics.pen_positions(l).1).collect();
    let box_w = spec.wrap_in.unwrap_or_else(|| widths.iter().copied().fold(0.0, f32::max)).max(0.01);
    let line_gap = spec.line_space / 1000.0 * spec.size_in;
    let line_h = metrics.line_height();
    let box_h = (lines.len() as f32 * line_h + (lines.len() - 1) as f32 * line_gap).max(0.01);

    let shear = if sheared { ITALIC_SHEAR_DEG.to_radians().tan() } else { 0.0 };
    let mut glyphs = Vec::new();
    let mut spans = Vec::new();
    for (line, (chars, &width)) in lines.iter().zip(&widths).enumerate() {
        let offset = match spec.align {
            TextAlign::Left => 0.0,
            TextAlign::Centre => (box_w - width) / 2.0,
            TextAlign::Right => box_w - width,
        };
        let top = line as f32 * (line_h + line_gap);
        spans.push(LineSpan { text: chars.iter().map(|&(_, ch)| ch).collect(), origin: (offset, top) });
        let (xs, _) = metrics.pen_positions(chars);
        for (&(char_index, ch), x) in chars.iter().zip(xs) {
            let Some(gid) = metrics.glyph_index(ch) else { continue };
            let pen = (offset + x + metrics.embolden, top + metrics.ascender());
            let mut builder = Builder { contours: Vec::new(), cur: None, last: (0.0, 0.0), scale: metrics.scale, shear, origin: pen };
            metrics.outline(gid, &mut builder);
            builder.flush();
            if !builder.contours.is_empty() {
                embolden(&mut builder.contours, metrics.embolden);
                glyphs.push(GlyphOutline { contours: builder.contours, line, char_index, pen, angle: 0.0 });
            }
        }
    }
    let straight = TextLayout { glyphs, lines: spans, size: (box_w, box_h), overflow: false, path: None };
    let mut laid = match &lines[..] {
        [chars] if spec.wrap_in.is_none() && !chars.is_empty() => {
            if let Some(path) = spec.path {
                let (xs, _) = metrics.pen_positions(chars);
                let cells: Vec<crate::text_path::Cell> =
                    chars.iter().zip(&xs).map(|(&(i, ch), &x)| crate::text_path::Cell { char_index: i, x, advance: metrics.advance(metrics.glyph_index(ch)) }).collect();
                crate::text_path::place(straight, path, &cells, metrics.ascender(), metrics.line_height(), widths[0])
            } else if spec.curve_in != 0.0 {
                curve(straight, &metrics, chars, widths[0], spec.curve_in)
            } else {
                straight
            }
        }
        _ => straight,
    };
    if matches!(source, GlyphSource::Stroke { .. }) {
        order_for_pen(&mut laid.glyphs);
    }
    Some(laid)
}

/// Turns strokes round, keeping their order (glyphs in reading order, each glyph's strokes in the
/// font's order), so the pen travels least between them.
fn order_for_pen(glyphs: &mut [GlyphOutline]) {
    let ends: Vec<(Pt, Pt)> = glyphs.iter().flat_map(|g| &g.contours).map(|c| (c.start, contour_end(c))).collect();
    let reversed = crate::stroke_font::pen_directions(&ends);
    for (contour, reverse) in glyphs.iter_mut().flat_map(|g| &mut g.contours).zip(reversed) {
        if reverse {
            *contour = reverse_contour(contour);
        }
    }
}

fn contour_end(c: &Contour) -> Pt {
    match c.segs.last() {
        Some(Seg::Line(p) | Seg::Cubic(_, _, p)) => *p,
        None => c.start,
    }
}

/// The same path drawn from its end back to its start.
fn reverse_contour(c: &Contour) -> Contour {
    let mut from = c.start;
    let mut backwards = Vec::with_capacity(c.segs.len());
    for seg in &c.segs {
        match *seg {
            Seg::Line(p) => {
                backwards.push(Seg::Line(from));
                from = p;
            }
            Seg::Cubic(a, b, p) => {
                backwards.push(Seg::Cubic(b, a, from));
                from = p;
            }
        }
    }
    backwards.reverse();
    Contour { start: from, segs: backwards, closed: c.closed }
}

/// Bends a single line onto a circle of signed diameter `curve_in`. Each character's cell keeps its
/// advance as arc length, its baseline centre on the arc and its upright along the radius, and the
/// box refits to the turned cells.
fn curve(straight: TextLayout, metrics: &Metrics, chars: &[(usize, char)], width: f32, curve_in: f32) -> TextLayout {
    let radius = curve_in.abs() / 2.0;
    let side = curve_in.signum();
    let (ascender, line_h) = (metrics.ascender(), metrics.line_height());
    let (xs, _) = metrics.pen_positions(chars);
    // Each character's index, the turn carrying its straight cell `x..x + advance` onto the arc, and
    // that cell's corners after the turn.
    let cells: Vec<(usize, Turn, [Pt; 4])> = chars
        .iter()
        .zip(xs)
        .map(|(&(i, ch), x)| {
            let advance = metrics.advance(metrics.glyph_index(ch));
            let mid = x + advance / 2.0;
            let theta = (mid - width / 2.0) / radius;
            let turn = Turn { angle: side * theta, from: (mid, ascender), to: (radius * theta.sin(), side * radius * (1.0 - theta.cos())) };
            (i, turn, [(x, 0.0), (x + advance, 0.0), (x, line_h), (x + advance, line_h)].map(|p| turn.apply(p)))
        })
        .collect();
    let corners = cells.iter().flat_map(|c| c.2);
    let (min, max) = corners.fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), p| ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1))));
    let glyphs = straight
        .glyphs
        .into_iter()
        .map(|g| {
            let turn = cells.iter().find(|c| c.0 == g.char_index).expect("every glyph has a cell").1;
            let place = |p: Pt| {
                let q = turn.apply(p);
                (q.0 - min.0, q.1 - min.1)
            };
            let contours = g.contours.iter().map(|c| map_contour(c, &place)).collect();
            GlyphOutline { contours, pen: place(g.pen), angle: turn.angle, ..g }
        })
        .collect();
    TextLayout { glyphs, lines: straight.lines, size: (max.0 - min.0, max.1 - min.1), overflow: false, path: None }
}

/// A rotation by `angle` (clockwise on the canvas) that carries the point `from` onto `to`.
#[derive(Clone, Copy)]
struct Turn {
    angle: f32,
    from: Pt,
    to: Pt,
}

impl Turn {
    fn apply(self, (x, y): Pt) -> Pt {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (x - self.from.0, y - self.from.1);
        (self.to.0 + dx * cos - dy * sin, self.to.1 + dx * sin + dy * cos)
    }
}

fn map_contour(c: &Contour, f: &impl Fn(Pt) -> Pt) -> Contour {
    let segs = c
        .segs
        .iter()
        .map(|s| match *s {
            Seg::Line(p) => Seg::Line(f(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(f(a), f(b), f(p)),
        })
        .collect();
    Contour { start: f(c.start), segs, closed: c.closed }
}

/// Splits on `'\n'`, keeping each character's index into the whole string. Always yields at least one
/// (possibly empty) paragraph, so empty text still has one line's height.
fn paragraphs(content: &str) -> Vec<Vec<(usize, char)>> {
    let mut out = vec![Vec::new()];
    for (i, ch) in content.chars().enumerate() {
        match ch {
            '\n' => out.push(Vec::new()),
            '\r' => {}
            _ => out.last_mut().unwrap().push((i, ch)),
        }
    }
    out
}

/// Where a layout's glyphs come from. A writing font's glyph indices are carried as `GlyphId`.
#[expect(clippy::large_enum_variant, reason = "one lives on the stack per layout")]
enum Glyphs<'a> {
    Outline(Face<'a>),
    Stroke(Arc<StrokeFont>),
}

/// A face at a size, with its kerning lookups resolved once per layout.
struct Metrics<'a> {
    glyphs: Glyphs<'a>,
    /// Font units to inches.
    scale: f32,
    letter_space: f32,
    space_advance: f32,
    /// How far synthetic bold pushes each edge out, in inches. Zero for a real face.
    embolden: f32,
    /// GPOS `kern` lookups, each a list of pair subtables. Empty when the font has none.
    gpos_kern: Vec<Vec<PairAdjustment<'a>>>,
}

impl<'a> Metrics<'a> {
    fn new(glyphs: Glyphs<'a>, spec: &TextSpec, embolden: bool) -> Self {
        let (units_per_em, gpos_kern) = match &glyphs {
            Glyphs::Outline(face) => (face.units_per_em() as f32, gpos_kern_lookups(face)),
            Glyphs::Stroke(font) => (font.units_per_em, Vec::new()),
        };
        let scale = spec.size_in / units_per_em;
        let embolden = if embolden { spec.size_in * SYNTHETIC_BOLD_EM / 2.0 } else { 0.0 };
        let mut metrics = Self { glyphs, scale, letter_space: spec.letter_space / 1000.0 * spec.size_in, space_advance: 0.0, embolden, gpos_kern };
        metrics.space_advance = metrics.glyph_index(' ').and_then(|g| metrics.units_advance(g)).unwrap_or(units_per_em * 0.3) * scale;
        metrics
    }

    fn glyph_index(&self, ch: char) -> Option<GlyphId> {
        match &self.glyphs {
            Glyphs::Outline(face) => face.glyph_index(ch),
            Glyphs::Stroke(font) => font.glyph_index(ch).map(GlyphId),
        }
    }

    fn units_advance(&self, gid: GlyphId) -> Option<f32> {
        match &self.glyphs {
            Glyphs::Outline(face) => face.glyph_hor_advance(gid).map(f32::from),
            Glyphs::Stroke(font) => Some(font.glyph(gid.0).advance),
        }
    }

    /// Draws glyph `gid` into `builder`. A writing font's strokes are never closed.
    fn outline(&self, gid: GlyphId, builder: &mut Builder) {
        match &self.glyphs {
            Glyphs::Outline(face) => _ = face.outline_glyph(gid, builder),
            Glyphs::Stroke(font) => {
                for stroke in &font.glyph(gid.0).strokes {
                    builder.move_to(stroke.start.0, stroke.start.1);
                    for seg in &stroke.segs {
                        match *seg {
                            StrokeSeg::Line((x, y)) => builder.line_to(x, y),
                            StrokeSeg::Cubic((x1, y1), (x2, y2), (x, y)) => builder.curve_to(x1, y1, x2, y2, x, y),
                        }
                    }
                }
            }
        }
    }

    fn ascender(&self) -> f32 {
        let units = match &self.glyphs {
            Glyphs::Outline(face) => face.ascender() as f32,
            Glyphs::Stroke(font) => font.ascender,
        };
        units * self.scale
    }

    /// A writing font has no line gap: its line runs from ascender to descender.
    fn line_height(&self) -> f32 {
        let units = match &self.glyphs {
            Glyphs::Outline(face) => face.ascender() as f32 - face.descender() as f32 + face.line_gap() as f32,
            Glyphs::Stroke(font) => font.ascender - font.descender,
        };
        units * self.scale
    }

    fn advance(&self, gid: Option<GlyphId>) -> f32 {
        gid.and_then(|g| self.units_advance(g)).map_or(self.space_advance, |a| a * self.scale + 2.0 * self.embolden)
    }

    /// Pair kerning in inches: a writing font's own pairs, else GPOS pair adjustment when the font has
    /// it, else the legacy `kern` table.
    fn kerning(&self, left: GlyphId, right: GlyphId) -> f32 {
        let face = match &self.glyphs {
            Glyphs::Outline(face) => face,
            Glyphs::Stroke(font) => return font.kerning(left.0, right.0) * self.scale,
        };
        let units: i32 = if self.gpos_kern.is_empty() {
            let Some(kern) = face.tables().kern else { return 0.0 };
            kern.subtables.into_iter().filter(|t| t.horizontal && !t.variable).filter_map(|t| t.glyphs_kerning(left, right)).map(i32::from).sum()
        } else {
            self.gpos_kern.iter().filter_map(|lookup| lookup.iter().find_map(|pair| pair_x_advance(pair, left, right))).map(i32::from).sum()
        };
        units as f32 * self.scale
    }

    /// Each character's pen x on a line, and the line's advance width.
    fn pen_positions(&self, chars: &[(usize, char)]) -> (Vec<f32>, f32) {
        let gids: Vec<Option<GlyphId>> = chars.iter().map(|&(_, ch)| self.glyph_index(ch)).collect();
        let mut xs = Vec::with_capacity(chars.len());
        let mut pen = 0.0;
        for (i, &gid) in gids.iter().enumerate() {
            xs.push(pen);
            pen += self.advance(gid);
            if let Some(&next) = gids.get(i + 1) {
                pen += self.letter_space;
                if let (Some(a), Some(b)) = (gid, next) {
                    pen += self.kerning(a, b);
                }
            }
        }
        (xs, pen)
    }

    /// Greedy word wrap at spaces. A word wider than `wrap_in` keeps a line to itself and overflows.
    fn wrap(&self, para: Vec<(usize, char)>, wrap_in: Option<f32>) -> Vec<Vec<(usize, char)>> {
        let Some(max) = wrap_in else { return vec![para] };
        let words = word_spans(&para);
        let mut lines = Vec::new();
        let (mut start, mut end) = (0, None);
        for (s, e) in words {
            if let Some(prev_end) = end
                && self.pen_positions(&para[start..e]).1 > max
            {
                lines.push(para[start..prev_end].to_vec());
                start = s;
            }
            end = Some(e);
        }
        lines.push(para[start..].to_vec());
        lines
    }
}

/// `(start, end)` index ranges of the runs of non-space characters.
fn word_spans(chars: &[(usize, char)]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, &(_, ch)) in chars.iter().enumerate() {
        match (ch == ' ', start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                spans.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, chars.len()));
    }
    spans
}

fn gpos_kern_lookups<'a>(face: &Face<'a>) -> Vec<Vec<PairAdjustment<'a>>> {
    let Some(gpos) = face.tables().gpos else { return Vec::new() };
    let mut indices: Vec<u16> = gpos.features.into_iter().filter(|f| f.tag == Tag::from_bytes(b"kern")).flat_map(|f| f.lookup_indices).collect();
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .filter_map(|i| gpos.lookups.get(i))
        .map(|lookup| {
            lookup
                .subtables
                .into_iter::<PositioningSubtable>()
                .filter_map(|t| match t {
                    PositioningSubtable::Pair(pair) => Some(pair),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|pairs| !pairs.is_empty())
        .collect()
}

/// The first glyph's advance adjustment, when this subtable covers the pair.
fn pair_x_advance(pair: &PairAdjustment, left: GlyphId, right: GlyphId) -> Option<i16> {
    match pair {
        PairAdjustment::Format1 { coverage, sets } => sets.get(coverage.get(left)?)?.get(right).map(|(v, _)| v.x_advance),
        PairAdjustment::Format2 { coverage, classes, matrix } => {
            coverage.contains(left).then_some(())?;
            matrix.get((classes.0.get(left), classes.1.get(right))).map(|(v, _)| v.x_advance)
        }
    }
}

/// Synthetic bold: pushes every edge of one glyph out by `by` inches, so its strokes thicken and its
/// counters shrink. Each point, control points included, moves along the mitred normal of the edges
/// to its neighbours, as FreeType's `FT_Outline_Embolden` does.
fn embolden(contours: &mut [Contour], by: f32) {
    if by == 0.0 {
        return;
    }
    // Ink lies on the same side of travel on every contour, and the largest (outer) contours decide
    // which side that is.
    let area: f32 = contours.iter().map(|c| signed_area(&points(c))).sum();
    let by = by * area.signum();
    for c in contours {
        let mut pts = points(c);
        let returns = pts.len() > 1 && pts.first() == pts.last();
        if returns {
            pts.pop();
        }
        let moved = offset_points(&pts, by);
        let mut it = moved.iter().copied().chain(returns.then_some(moved[0]));
        c.start = it.next().unwrap_or(c.start);
        for seg in &mut c.segs {
            match seg {
                Seg::Line(p) => *p = it.next().unwrap_or(*p),
                Seg::Cubic(a, b, p) => {
                    *a = it.next().unwrap_or(*a);
                    *b = it.next().unwrap_or(*b);
                    *p = it.next().unwrap_or(*p);
                }
            }
        }
    }
}

fn points(c: &Contour) -> Vec<Pt> {
    let mut pts = vec![c.start];
    for seg in &c.segs {
        match *seg {
            Seg::Line(p) => pts.push(p),
            Seg::Cubic(a, b, p) => pts.extend([a, b, p]),
        }
    }
    pts
}

fn signed_area(pts: &[Pt]) -> f32 {
    let n = pts.len();
    (0..n).map(|i| pts[i].0 * pts[(i + 1) % n].1 - pts[(i + 1) % n].0 * pts[i].1).sum::<f32>() / 2.0
}

/// Moves each point of a closed polygon by `by` along the right-hand normal of travel (left when
/// negative), mitring at corners and skipping repeated points.
fn offset_points(pts: &[Pt], by: f32) -> Vec<Pt> {
    let n = pts.len();
    let unit_normal = |from: Pt, to: Pt| {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let len = dx.hypot(dy);
        (len > 1e-6).then(|| (dy / len, -dx / len))
    };
    (0..n)
        .map(|i| {
            let p = pts[i];
            let incoming = (1..n).find_map(|k| unit_normal(pts[(i + n - k) % n], p));
            let outgoing = (1..n).find_map(|k| unit_normal(p, pts[(i + k) % n]));
            let (Some(a), Some(b)) = (incoming, outgoing) else { return p };
            let scale = by / (1.0 + a.0 * b.0 + a.1 * b.1).max(0.25);
            (p.0 + (a.0 + b.0) * scale, p.1 + (a.1 + b.1) * scale)
        })
        .collect()
}

struct Builder {
    contours: Vec<Contour>,
    cur: Option<Contour>,
    last: Pt,
    scale: f32,
    /// Horizontal shift per unit of height, for the synthetic italic.
    shear: f32,
    /// Pen position on the baseline, in inches.
    origin: Pt,
}

impl Builder {
    /// Glyph (y-up font units) to inch (y-down) coordinates at the pen position.
    fn map(&self, x: f32, y: f32) -> Pt {
        (self.origin.0 + (x + y * self.shear) * self.scale, self.origin.1 - y * self.scale)
    }

    fn flush(&mut self) {
        if let Some(c) = self.cur.take() {
            self.contours.push(c);
        }
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.last = self.map(x, y);
        self.cur = Some(Contour { start: self.last, segs: Vec::new(), closed: false });
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.last = self.map(x, y);
        if let Some(c) = self.cur.as_mut() {
            c.segs.push(Seg::Line(self.last));
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last;
        let q = self.map(x1, y1);
        let end = self.map(x, y);
        let c1 = (p0.0 + 2.0 / 3.0 * (q.0 - p0.0), p0.1 + 2.0 / 3.0 * (q.1 - p0.1));
        let c2 = (end.0 + 2.0 / 3.0 * (q.0 - end.0), end.1 + 2.0 / 3.0 * (q.1 - end.1));
        self.last = end;
        if let Some(c) = self.cur.as_mut() {
            c.segs.push(Seg::Cubic(c1, c2, end));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, end) = (self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        self.last = end;
        if let Some(c) = self.cur.as_mut() {
            c.segs.push(Seg::Cubic(c1, c2, end));
        }
    }

    fn close(&mut self) {
        if let Some(c) = self.cur.as_mut() {
            c.closed = true;
        }
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content: &str) -> TextSpec<'_> {
        TextSpec { content, ..TextSpec::default() }
    }

    fn lay(spec: TextSpec) -> TextLayout {
        layout(&spec).expect("bundled fonts parse")
    }

    fn line_count(l: &TextLayout) -> usize {
        l.glyphs.iter().map(|g| g.line + 1).max().unwrap_or(0)
    }

    fn ink_x_range(g: &GlyphOutline) -> (f32, f32) {
        g.contours.iter().flat_map(|c| c.flatten()).fold((f32::MAX, f32::MIN), |(lo, hi), (x, _)| (lo.min(x), hi.max(x)))
    }

    #[test]
    fn layout_yields_closed_glyphs_with_a_plausible_box() {
        let l = lay(text("AB"));
        assert_eq!(l.glyphs.len(), 2);
        assert!(l.glyphs.iter().flat_map(|g| &g.contours).all(|c| c.closed), "glyph outlines are closed");
        let (w, h) = l.size;
        assert!(w > 0.8 && w < 2.5 && h > 1.0 && h < 1.5, "box {w} x {h}");
    }

    #[test]
    fn kerning_pulls_av_closer_than_the_sum_of_its_advances() {
        let source = inter_face(FontStyle::Regular);
        let face = source.parse().unwrap();
        let scale = 1.0 / face.units_per_em() as f32;
        let adv = |ch| face.glyph_hor_advance(face.glyph_index(ch).unwrap()).unwrap() as f32 * scale;
        let (w, _) = lay(text("AV")).size;
        assert!(w < adv('A') + adv('V') - 0.01, "AV is {w} wide, advances sum to {}", adv('A') + adv('V'));
    }

    #[test]
    fn the_box_height_does_not_depend_on_which_glyphs_have_ink() {
        assert_eq!(lay(text("ace")).size.1, lay(text("gyp")).size.1);
    }

    #[test]
    fn line_space_is_added_between_lines() {
        let one = lay(text("a")).size.1;
        let line_space = 250.0;
        let two = lay(TextSpec { line_space, ..text("a\nb") });
        assert_eq!(line_count(&two), 2);
        let expected = 2.0 * one + line_space / 1000.0;
        assert!((two.size.1 - expected).abs() < 1e-4, "two lines are {} high, expected {expected}", two.size.1);
    }

    #[test]
    fn centred_lines_share_the_box_centre() {
        let l = lay(TextSpec { align: TextAlign::Centre, ..text("a\nabc") });
        let a = l.glyphs.iter().find(|g| g.line == 0).unwrap();
        let (lo, hi) = ink_x_range(a);
        let centre = l.size.0 / 2.0;
        assert!(((lo + hi) / 2.0 - centre).abs() < 0.02, "a centres at {}, box centre {centre}", (lo + hi) / 2.0);
    }

    #[test]
    fn right_alignment_ends_each_line_at_the_box_edge() {
        let l = lay(TextSpec { align: TextAlign::Right, ..text("i\nmmm") });
        let (_, hi) = ink_x_range(l.glyphs.iter().find(|g| g.line == 0).unwrap());
        assert!(hi > l.size.0 - 0.15 && hi <= l.size.0, "i ends at {hi}, box width {}", l.size.0);
    }

    #[test]
    fn wrapping_at_one_inch_puts_each_word_on_its_own_line() {
        let l = lay(TextSpec { wrap_in: Some(1.0), ..text("one two three") });
        assert_eq!(line_count(&l), 3);
        assert_eq!(l.size.0, 1.0);
        let lines: Vec<usize> = l.glyphs.iter().map(|g| g.line).collect();
        assert_eq!(lines, [0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2]);
    }

    #[test]
    fn wrapping_keeps_words_together_when_they_fit() {
        let l = lay(TextSpec { wrap_in: Some(2.5), size_in: 0.5, ..text("one two three") });
        assert_eq!(line_count(&l), 2, "one two | three");
    }

    #[test]
    fn letter_space_widens_every_gap_but_the_last() {
        let plain = lay(text("abc")).size.0;
        let spaced = lay(TextSpec { letter_space: 100.0, ..text("abc") }).size.0;
        assert!((spaced - plain - 0.2).abs() < 1e-4, "{plain} -> {spaced}");
    }

    #[test]
    fn bold_space_grotesk_outlines_differ_from_regular() {
        let spec = TextSpec { font: &FontRef::SpaceGrotesk, ..text("Hi") };
        let regular = lay(spec);
        let bold = lay(TextSpec { style: FontStyle::Bold, ..spec });
        assert!(!regular.glyphs.is_empty());
        assert_ne!(regular.glyphs, bold.glyphs);
    }

    #[test]
    fn italic_shears_without_changing_the_box() {
        let upright = lay(text("l"));
        let italic = lay(TextSpec { style: FontStyle::Italic, ..text("l") });
        assert_eq!(upright.size, italic.size);
        let top_x = |l: &TextLayout| l.glyphs[0].contours[0].flatten().into_iter().fold((0.0, f32::MAX), |best, p| if p.1 < best.1 { p } else { best }).0;
        assert!(top_x(&italic) > top_x(&upright) + 0.05, "the top of the italic l leans right");
    }

    #[test]
    fn glyphs_report_their_line_and_char_index() {
        let l = lay(text("ab\nc"));
        let found: Vec<(usize, usize)> = l.glyphs.iter().map(|g| (g.line, g.char_index)).collect();
        assert_eq!(found, [(0, 0), (0, 1), (1, 3)]);
    }

    #[test]
    fn unknown_characters_advance_by_a_space() {
        assert_eq!(lay(text("a\u{10FFFD}b")).size.0, lay(text("a b")).size.0);
    }

    #[test]
    fn empty_text_still_has_one_line_of_height() {
        let l = lay(text(""));
        assert!(l.glyphs.is_empty());
        assert_eq!(l.size.1, lay(text("x")).size.1);
    }

    fn ink_top(g: &GlyphOutline) -> f32 {
        g.contours.iter().flat_map(|c| c.flatten()).map(|p| p.1).fold(f32::MAX, f32::min)
    }

    #[test]
    fn zero_curve_lays_out_as_straight_text() {
        let straight = lay(text("Hello"));
        let zero = lay(TextSpec { curve_in: 0.0, ..text("Hello") });
        assert_eq!(straight, zero);
        assert!(straight.glyphs.iter().all(|g| g.angle == 0.0));
    }

    #[test]
    fn a_positive_curve_puts_the_middle_glyph_highest() {
        let l = lay(TextSpec { curve_in: 3.0, ..text("abcde") });
        let tops: Vec<f32> = l.glyphs.iter().map(ink_top).collect();
        let middle = tops[2];
        assert!(tops.iter().enumerate().all(|(i, &t)| i == 2 || t > middle), "the middle glyph tops the arc: {tops:?}");
        assert!(l.glyphs[0].angle < 0.0 && l.glyphs[4].angle > 0.0, "the ends tilt away from the middle");
    }

    #[test]
    fn a_negative_curve_puts_the_middle_glyph_lowest() {
        let l = lay(TextSpec { curve_in: -3.0, ..text("abcde") });
        let tops: Vec<f32> = l.glyphs.iter().map(ink_top).collect();
        assert!(tops.iter().enumerate().all(|(i, &t)| i == 2 || t < tops[2]), "the middle glyph sits lowest: {tops:?}");
    }

    #[test]
    fn the_curved_box_contains_every_glyph() {
        let l = lay(TextSpec { curve_in: 2.0, ..text("curved") });
        let pts: Vec<Pt> = l.contours().iter().flat_map(Contour::flatten).collect();
        assert!(pts.iter().all(|&(x, y)| x >= -1e-3 && y >= -1e-3 && x <= l.size.0 + 1e-3 && y <= l.size.1 + 1e-3), "box {:?}", l.size);
        assert!(l.size.1 > lay(text("curved")).size.1, "the arc is taller than the straight line");
    }

    #[test]
    fn curve_ignores_text_with_several_lines() {
        assert_eq!(lay(TextSpec { curve_in: 3.0, ..text("a\nb") }), lay(text("a\nb")));
    }

    #[test]
    fn each_glyph_reports_its_pen_and_each_line_its_text_and_origin() {
        let l = lay(TextSpec { align: TextAlign::Right, ..text("ab\nc d") });
        assert_eq!(l.glyphs[0].pen, (l.lines[0].origin.0, lay(text("a")).glyphs[0].pen.1));
        assert_eq!(l.lines.iter().map(|ln| ln.text.as_str()).collect::<Vec<_>>(), ["ab", "c d"]);
        assert!(l.lines[0].origin.0 > 0.0 && l.lines[1].origin.1 > 0.5, "{:?}", l.lines);
    }

    #[test]
    fn synthetic_bold_grows_the_outline_and_shrinks_the_counter() {
        let square = |lo: f32, hi: f32, reverse: bool| {
            let mut pts = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)];
            if reverse {
                pts.reverse();
            }
            Contour { start: pts[0], segs: pts[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true }
        };
        let width = |c: &Contour| points(c).iter().map(|p| p.0).fold(f32::MIN, f32::max) - points(c).iter().map(|p| p.0).fold(f32::MAX, f32::min);
        for reverse in [false, true] {
            let mut glyph = vec![square(0.0, 1.0, reverse), square(0.4, 0.6, !reverse)];
            embolden(&mut glyph, 0.05);
            assert!((width(&glyph[0]) - 1.1).abs() < 1e-5, "outer grows: {}", width(&glyph[0]));
            assert!((width(&glyph[1]) - 0.1).abs() < 1e-5, "counter shrinks: {}", width(&glyph[1]));
        }
    }

    fn writing(id: &str) -> FontRef {
        FontRef::Stroke(StrokeFontId(id.into()))
    }

    /// Each segment as an unordered pair of rounded end points, so a segment drawn twice in either
    /// direction counts twice.
    fn segments(c: &Contour) -> Vec<[(i32, i32); 2]> {
        let key = |p: Pt| ((p.0 * 1e4).round() as i32, (p.1 * 1e4).round() as i32);
        let pts = c.flatten();
        pts.windows(2)
            .map(|w| {
                let mut pair = [key(w[0]), key(w[1])];
                pair.sort_unstable();
                pair
            })
            .collect()
    }

    #[test]
    fn hershey_simplex_l_is_one_open_contour_and_o_one_loop_drawn_once() {
        let simplex = writing("hershey-simplex");
        let l = lay(TextSpec { font: &simplex, ..text("l") });
        assert_eq!(l.glyphs[0].contours.len(), 1, "one stroke");
        assert!(!l.glyphs[0].contours[0].closed, "and it is open");

        let o = lay(TextSpec { font: &simplex, ..text("o") });
        let [ring] = &o.glyphs[0].contours[..] else { panic!("o is one stroke: {:?}", o.glyphs[0].contours.len()) };
        assert!(!ring.closed, "the loop is an open stroke that ends where it starts");
        let (start, end) = (ring.start, contour_end(ring));
        assert!((start.0 - end.0).abs() < 1e-5 && (start.1 - end.1).abs() < 1e-5, "it closes on itself: {start:?} {end:?}");
        let mut segs = segments(ring);
        let drawn = segs.len();
        segs.sort_unstable();
        segs.dedup();
        assert_eq!(segs.len(), drawn, "no segment is drawn twice");
    }

    #[test]
    fn hello_in_a_writing_font_is_open_strokes_in_an_ascender_to_descender_box() {
        let font = writing("hershey-simplex");
        let size_in = 0.75;
        let l = lay(TextSpec { font: &font, size_in, ..text("Hello") });
        assert_eq!(l.glyphs.len(), 5);
        assert!(l.contours().iter().all(|c| !c.closed), "every contour is an open stroke");
        let simplex = crate::system_fonts::stroke(&StrokeFontId("hershey-simplex".into())).unwrap();
        let expected = (simplex.ascender - simplex.descender) / simplex.units_per_em * size_in;
        assert!((l.size.1 - expected).abs() < 1e-5, "the box is {} high, ascender to descender is {expected}", l.size.1);
    }

    #[test]
    fn stroke_order_for_ab_draws_a_then_b_with_no_more_pen_travel_than_font_order() {
        let id = StrokeFontId("hershey-simplex".into());
        let font = crate::system_fonts::stroke(&id).unwrap();
        // At one inch per font unit the layout is the font's own coordinates, mirrored in y.
        let l = lay(TextSpec { font: &FontRef::Stroke(id), size_in: font.units_per_em, ..text("ab") });
        assert_eq!(l.glyphs.iter().map(|g| g.char_index).collect::<Vec<_>>(), [0, 1], "every stroke of a comes before b");
        let a = font.glyph(font.glyph_index('a').unwrap());
        assert_eq!(l.glyphs[0].contours.len(), a.strokes.len(), "a keeps its strokes");

        let ascender = font.ascender;
        let mut font_order = Vec::new();
        let mut pen_x = 0.0;
        for ch in ['a', 'b'] {
            let glyph = font.glyph(font.glyph_index(ch).unwrap());
            for stroke in &glyph.strokes {
                let end = match stroke.segs.last() {
                    Some(StrokeSeg::Line(p) | StrokeSeg::Cubic(_, _, p)) => *p,
                    None => stroke.start,
                };
                let place = |(x, y): (f32, f32)| (pen_x + x, ascender - y);
                font_order.push((place(stroke.start), place(end)));
            }
            pen_x += glyph.advance;
        }
        let laid: Vec<(Pt, Pt)> = l.contours().iter().map(|c| (c.start, contour_end(c))).collect();
        let unchanged = vec![false; laid.len()];
        let travel = crate::stroke_font::pen_up_distance(&laid, &unchanged);
        let as_fonted = crate::stroke_font::pen_up_distance(&font_order, &unchanged);
        assert!(travel <= as_fonted + 1e-3, "pen-up travel {travel} exceeds font order {as_fonted}");
    }

    #[test]
    fn single_line_text_has_no_closepath_in_its_svg_path() {
        let font = writing("relief-singleline");
        let contours = lay(TextSpec { font: &font, ..text("Hello world") }).contours();
        let d = crate::design::to_svg_path_d(&contours);
        assert!(!d.is_empty() && !d.contains('Z') && !d.contains('z'), "{d}");
    }

    #[test]
    fn writing_fonts_save_as_stroke_ids_and_label_with_their_names() {
        let font = writing("relief-singleline");
        assert_eq!(String::from(font.clone()), "stroke:relief-singleline");
        assert_eq!(FontRef::from("stroke:relief-singleline".to_owned()), font);
        assert_eq!(font.label(), "Relief SingleLine");
        assert_eq!(writing("svg-gone").label(), "svg-gone", "an unavailable font shows its id");
    }

    #[test]
    fn bold_lays_out_as_regular_and_italic_shears_in_a_writing_font() {
        let font = writing("hershey-duplex");
        let regular = lay(TextSpec { font: &font, ..text("Hi") });
        assert_eq!(lay(TextSpec { font: &font, style: FontStyle::Bold, ..text("Hi") }), regular, "no synthetic bold");
        assert_ne!(lay(TextSpec { font: &font, style: FontStyle::Italic, ..text("Hi") }).glyphs, regular.glyphs);
    }

    #[test]
    fn reversing_a_contour_draws_the_same_path_backwards() {
        let c = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((1.0, 0.0)), Seg::Cubic((1.0, 1.0), (2.0, 1.0), (2.0, 2.0))], closed: false };
        let r = reverse_contour(&c);
        assert_eq!(r.start, (2.0, 2.0));
        assert_eq!(r.segs, [Seg::Cubic((2.0, 1.0), (1.0, 1.0), (1.0, 0.0)), Seg::Line((0.0, 0.0))]);
        assert_eq!(reverse_contour(&r), c);
    }
}

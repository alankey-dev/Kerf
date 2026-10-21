//! The Monogram Maker (spec 42): up to three initials in a layout style, inside a frame, inserted as one
//! live group. [`layout`] is pure: it turns a [`MonogramSpec`] into the group's children in inches, the
//! frame first and then the letters as live text. The group is a Compound whose kind follows the spec's
//! output, so Knockout and Welded stay editable booleans, and it keeps its spec in `Shape::monogram`.
//!
//! Themes are data in `assets/monogram/themes.json`; frames are original SVG artwork in
//! `assets/monogram/frames/`. A frame SVG names the region the letters fit in with a `data-content`
//! attribute on its root, draws its main outlines as stroked closed centrelines (the bands, whose width is
//! the Thickness adjustment) and everything else as filled shapes (the decoration).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, OnceLock, PoisonError};

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::design::{Combine, CompoundOp, Contour, Design, Operation, Pt, Seg, Shape, ShapeKind};
use crate::edit::Blocked;
use crate::geometry::{Boolean, FLATTEN_TOL_IN, Join, Ring};
use crate::snap::Aabb;
use crate::stroke_font::StrokeFontId;
use crate::system_fonts::FontCatalogue;
use crate::text::FontRef;

/// How the initials are arranged.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Single,
    ClassicThree,
    EvenThree,
    Stacked,
    Interlocking,
    Circle,
}

/// Classic three and Circle set the first and middle letters at this fraction of the centre letter.
const SIDE_SCALE: f32 = 0.7;
/// Circle's arc letters, as a fraction of the centre letter.
const ARC_SCALE: f32 = 0.5;
/// Stacked letters, as a fraction of the centre size, so a column of three is not three times as tall.
const STACK_SCALE: f32 = 0.6;
/// Interlocking letters overlap by this fraction of the narrower letter's ink width.
const OVERLAP: f32 = 0.22;
/// How far round from the top Circle's arc letters sit, each side.
const ARC_DEG: f32 = 42.0;

impl Style {
    pub const ALL: [Style; 6] = [Self::Single, Self::ClassicThree, Self::EvenThree, Self::Stacked, Self::Interlocking, Self::Circle];

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::ClassicThree => "Classic three",
            Self::EvenThree => "Even three",
            Self::Stacked => "Stacked",
            Self::Interlocking => "Interlocking",
            Self::Circle => "Circle",
        }
    }

    /// The help line under the Letters field.
    pub fn help(self) -> &'static str {
        match self {
            Self::Single => "One initial.",
            Self::ClassicThree => "First, Last, Middle. The last name's initial sits in the centre, larger.",
            Self::EvenThree => "Three initials at one size, in the order typed.",
            Self::Stacked => "Two or three initials in a column.",
            Self::Interlocking => "Two or three initials overlapping, ready to weld.",
            Self::Circle => "First, Last, Middle. First and middle curve round the last.",
        }
    }

    fn max_letters(self) -> usize {
        match self {
            Self::Single => 1,
            _ => 3,
        }
    }

    /// Whether some letters set in the side font rather than the centre one.
    pub fn has_sides(self) -> bool {
        matches!(self, Self::ClassicThree | Self::EvenThree | Self::Circle)
    }
}

/// The frame around the initials.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    None,
    Circle,
    DoubleCircle,
    Square,
    Diamond,
    ScallopedCircle,
    Laurel,
    Shield,
    Banner,
    LeafWreath,
}

impl Frame {
    pub const ALL: [Frame; 10] =
        [Self::None, Self::Circle, Self::DoubleCircle, Self::Square, Self::Diamond, Self::ScallopedCircle, Self::Laurel, Self::Shield, Self::Banner, Self::LeafWreath];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Circle => "Circle",
            Self::DoubleCircle => "Double circle",
            Self::Square => "Square",
            Self::Diamond => "Diamond",
            Self::ScallopedCircle => "Scalloped circle",
            Self::Laurel => "Laurel",
            Self::Shield => "Shield",
            Self::Banner => "Banner",
            Self::LeafWreath => "Leaf wreath",
        }
    }

    fn svg(self) -> Option<&'static [u8]> {
        macro_rules! frame {
            ($file:literal) => {
                Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/monogram/frames/", $file)))
            };
        }
        match self {
            Self::None => None,
            Self::Circle => frame!("circle.svg"),
            Self::DoubleCircle => frame!("double-circle.svg"),
            Self::Square => frame!("square.svg"),
            Self::Diamond => frame!("diamond.svg"),
            Self::ScallopedCircle => frame!("scalloped-circle.svg"),
            Self::Laurel => frame!("laurel.svg"),
            Self::Shield => frame!("shield.svg"),
            Self::Banner => frame!("banner.svg"),
            Self::LeafWreath => frame!("leaf-wreath.svg"),
        }
    }

    /// The parsed artwork, read once per frame per process.
    fn art(self) -> Option<&'static FrameArt> {
        static ART: OnceLock<Vec<Option<FrameArt>>> = OnceLock::new();
        let all = ART
            .get_or_init(|| Self::ALL.iter().map(|f| f.svg().map(|svg| FrameArt::parse(svg).unwrap_or_else(|e| panic!("bundled frame {}: {e}", f.label())))).collect());
        all[Self::ALL.iter().position(|f| *f == self)?].as_ref()
    }

    /// The frame's outline for a tile, its viewBox scaled to 1 across its longer side, with a band 5 %
    /// of that wide. Built once per frame per process.
    pub fn outline(self) -> &'static [Ring] {
        static OUTLINES: OnceLock<Vec<Vec<Ring>>> = OnceLock::new();
        let all = OUTLINES.get_or_init(|| {
            Self::ALL.iter().map(|f| f.art().map_or_else(Vec::new, |art| art.rings(1.0 / art.size.0.max(art.size.1), (0.0, 0.0), 0.05, false))).collect()
        });
        Self::ALL.iter().position(|f| *f == self).map_or(&[], |i| &all[i])
    }
}

/// The font for each letter role. Single, Stacked and Interlocking set every letter in `centre`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Fonts {
    pub centre: FontRef,
    pub sides: FontRef,
}

/// The four lengths the Adjust sliders set, in inches.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Adjust {
    /// Between letters: the gap in a row or column, less the overlap when interlocking, beyond the
    /// centre letter on an arc.
    pub spacing_in: f32,
    /// The em size of the centre (or only) letter.
    pub centre_in: f32,
    /// The clearance between the letters and the frame's inner edge.
    pub padding_in: f32,
    /// The width of the frame's outline.
    pub thickness_in: f32,
}

impl Adjust {
    pub const SPACING_IN: std::ops::RangeInclusive<f32> = -0.5..=1.5;
    pub const CENTRE_IN: std::ops::RangeInclusive<f32> = 0.5..=6.0;
    pub const PADDING_IN: std::ops::RangeInclusive<f32> = 0.0..=1.5;
    pub const THICKNESS_IN: std::ops::RangeInclusive<f32> = 0.04..=0.5;

    /// Each value within its slider's range, so a typed or saved value cannot break the layout.
    pub fn clamped(self) -> Adjust {
        let clamp = |v: f32, r: &std::ops::RangeInclusive<f32>| if v.is_finite() { v.clamp(*r.start(), *r.end()) } else { *r.start() };
        Adjust {
            spacing_in: clamp(self.spacing_in, &Self::SPACING_IN),
            centre_in: clamp(self.centre_in, &Self::CENTRE_IN),
            padding_in: clamp(self.padding_in, &Self::PADDING_IN),
            thickness_in: clamp(self.thickness_in, &Self::THICKNESS_IN),
        }
    }
}

impl Default for Adjust {
    fn default() -> Self {
        Self { spacing_in: 0.1, centre_in: 2.0, padding_in: 0.2, thickness_in: 0.12 }
    }
}

/// What the group makes of its frame and letters.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// Frame and letters stay separate shapes.
    Layered,
    /// The letters cut out of a solid frame.
    Knockout,
    /// Frame and letters joined into one outline.
    Welded,
}

impl Output {
    pub const ALL: [Output; 3] = [Self::Layered, Self::Knockout, Self::Welded];

    pub fn label(self) -> &'static str {
        match self {
            Self::Layered => "Layered",
            Self::Knockout => "Knockout",
            Self::Welded => "Welded",
        }
    }
}

/// How the monogram is made. Pen sets every font that has a writing font in it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MakeAs {
    BasicCut,
    Pen,
    PrintThenCut,
}

impl MakeAs {
    pub const ALL: [MakeAs; 3] = [Self::BasicCut, Self::Pen, Self::PrintThenCut];

    pub fn label(self) -> &'static str {
        match self {
            Self::BasicCut => "Basic Cut",
            Self::Pen => "Pen",
            Self::PrintThenCut => "Print Then Cut",
        }
    }

    pub fn operation(self) -> Operation {
        match self {
            Self::BasicCut => Operation::BasicCut,
            Self::Pen => Operation::Pen,
            Self::PrintThenCut => Operation::PrintThenCut,
        }
    }
}

/// Everything a monogram is made from, kept on its group so it can be edited later.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MonogramSpec {
    /// As typed: 1 to 3 characters, any Unicode. Whitespace is ignored.
    pub letters: String,
    /// The theme style, frame and fonts were last set from; `None` once one of them changed (Custom).
    #[serde(default)]
    pub theme: Option<String>,
    pub style: Style,
    pub frame: Frame,
    pub fonts: Fonts,
    pub adjust: Adjust,
    pub output: Output,
    pub operation: MakeAs,
}

impl Default for MonogramSpec {
    fn default() -> Self {
        let mut spec = Self {
            letters: String::new(),
            theme: None,
            style: Style::ClassicThree,
            frame: Frame::Circle,
            fonts: Fonts { centre: FontRef::Inter, sides: FontRef::Inter },
            adjust: Adjust::default(),
            output: Output::Layered,
            operation: MakeAs::BasicCut,
        };
        if let Some(first) = themes().first() {
            spec.apply_theme(first);
        }
        spec
    }
}

impl MonogramSpec {
    /// Sets style, frame and fonts from `theme`, keeping everything else.
    pub fn apply_theme(&mut self, theme: &Theme) {
        self.style = theme.style;
        self.frame = theme.frame;
        self.fonts = theme.fonts.clone();
        self.theme = Some(theme.name.clone());
    }

    /// The letters the style lays out: the typed characters less whitespace, at most as many as it takes.
    pub fn initials(&self) -> Vec<char> {
        self.letters.chars().filter(|c| !c.is_whitespace()).take(self.style.max_letters()).collect()
    }

    /// The group's name in Layers.
    pub fn group_name(&self) -> String {
        format!("Monogram {}", self.letters.chars().filter(|c| !c.is_whitespace()).collect::<String>())
    }

    /// The output actually made: a pen draws lines, so Pen keeps frame and letters Layered.
    pub fn output_made(&self) -> Output {
        match self.operation {
            MakeAs::Pen => Output::Layered,
            _ => self.output,
        }
    }

    /// The group's Compound kind.
    pub fn compound(&self) -> CompoundOp {
        match self.output_made() {
            Output::Layered => CompoundOp::Effects,
            Output::Knockout => CompoundOp::Boolean(Combine::Subtract),
            Output::Welded => CompoundOp::Boolean(Combine::Weld),
        }
    }

    /// The theme's colours, or the default ones once Custom.
    fn colours(&self) -> Colours {
        self.theme.as_deref().and_then(theme).map_or(Colours::default(), |t| t.colours)
    }
}

/// A preset of style, frame and fonts, and the colours a new monogram in it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub style: Style,
    pub frame: Frame,
    pub fonts: Fonts,
    colours: Colours,
}

/// The fills of a monogram's frame and letters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Colours {
    frame: Color32,
    letters: Color32,
}

impl Default for Colours {
    fn default() -> Self {
        Self { frame: Color32::from_rgb(0x2b, 0x2b, 0x2b), letters: Color32::from_rgb(0x2b, 0x2b, 0x2b) }
    }
}

#[derive(Deserialize)]
struct ThemeFile {
    themes: Vec<ThemeEntry>,
    /// Font name (as a project saves it) to the writing font Pen sets it in.
    writing: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ThemeEntry {
    name: String,
    style: Style,
    frame: Frame,
    centre: String,
    sides: String,
    frame_colour: String,
    letter_colour: String,
}

fn theme_file() -> &'static (Vec<Theme>, BTreeMap<String, StrokeFontId>) {
    static FILE: OnceLock<(Vec<Theme>, BTreeMap<String, StrokeFontId>)> = OnceLock::new();
    FILE.get_or_init(|| {
        let file: ThemeFile = serde_json::from_str(include_str!("../assets/monogram/themes.json")).expect("bundled themes.json parses");
        let themes = file
            .themes
            .into_iter()
            .map(|t| Theme {
                fonts: Fonts { centre: theme_font(&t.centre), sides: theme_font(&t.sides) },
                colours: Colours { frame: hex(&t.frame_colour), letters: hex(&t.letter_colour) },
                name: t.name,
                style: t.style,
                frame: t.frame,
            })
            .collect();
        (themes, file.writing.into_iter().map(|(font, id)| (font, StrokeFontId(id))).collect())
    })
}

/// The themes, in tile order.
pub fn themes() -> &'static [Theme] {
    &theme_file().0
}

pub fn theme(name: &str) -> Option<&'static Theme> {
    themes().iter().find(|t| t.name == name)
}

/// A theme's font by name: a bundled face, or one of the bundled display families. Anything else would
/// look different on every machine, so it falls back to Inter, logged once.
fn theme_font(name: &str) -> FontRef {
    match FontRef::from(name.to_owned()) {
        FontRef::System(family) if !crate::system_fonts::DISPLAY.iter().any(|(d, _)| *d == family) => {
            static LOGGED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
            if LOGGED.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_default().insert(family.clone()) {
                tracing::warn!(target: "font", family = %family, "monogram theme font missing, using Inter");
            }
            FontRef::Inter
        }
        font => font,
    }
}

fn hex(s: &str) -> Color32 {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0x2b2b2b);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The writing font Pen sets `font` in, when it has one and it is available.
fn writing_for(font: &FontRef, fonts: &FontCatalogue) -> Option<FontRef> {
    let id = theme_file().1.get(&String::from(font.clone()))?;
    fonts.stroke(id).map(|_| FontRef::Stroke(id.clone()))
}

/// A monogram laid out: the children of its group, back to front, in a box at the origin.
#[derive(Clone, Debug, PartialEq)]
pub struct MonogramLayout {
    /// The frame when there is one, then the letters as text. Ids count from 0; the design gives them
    /// fresh ones.
    pub children: Vec<Shape>,
    pub size: Pt,
    /// One line for each character a font lacked, naming the font set in its place.
    pub notices: Vec<String>,
}

/// Lays `spec` out. Never fails: a character no font has is set in Inter anyway and noted.
pub fn layout(spec: &MonogramSpec, fonts: &FontCatalogue) -> MonogramLayout {
    let adjust = spec.adjust.clamped();
    let colours = spec.colours();
    let operation = spec.operation.operation();
    let mut notices = Vec::new();
    let role = |font: &FontRef| match spec.operation {
        MakeAs::Pen => writing_for(font, fonts).unwrap_or_else(|| font.clone()),
        _ => font.clone(),
    };
    let (centre_font, side_font) = (role(&spec.fonts.centre), role(&spec.fonts.sides));
    let initials = spec.initials();
    let letter = |chars: &[char], font: &FontRef, size_in: f32, notices: &mut Vec<String>| -> Cell {
        Cell::of(text(chars.iter().collect(), covering(font, chars, notices), size_in, 0.0, None))
    };

    let c = adjust.centre_in;
    let gap = adjust.spacing_in;
    let placed: Vec<(Cell, Pt)> = match (spec.style, initials.as_slice()) {
        (_, []) => Vec::new(),
        (Style::Single, [a, ..]) | (Style::ClassicThree | Style::Circle, [a]) => {
            let cell = letter(&[*a], &centre_font, c, &mut notices);
            vec![cell.ink_centred_at((0.0, 0.0))]
        }
        (Style::ClassicThree, [first, last, rest @ ..]) => {
            let centre = letter(&[*last], &centre_font, c, &mut notices);
            let half = centre.ink.width() / 2.0;
            let mut out = vec![letter(&[*first], &side_font, c * SIDE_SCALE, &mut notices).ink_beside(-half - gap, true)];
            out.push(centre.ink_centred_at((0.0, 0.0)));
            if let [middle] = rest {
                out.push(letter(&[*middle], &side_font, c * SIDE_SCALE, &mut notices).ink_beside(half + gap, false));
            }
            out
        }
        (Style::Circle, [first, last, rest @ ..]) => {
            let centre = letter(&[*last], &centre_font, c, &mut notices);
            let radius = centre.ink.width().hypot(centre.ink.height()) / 2.0 + gap.max(0.0);
            let arc_chars: Vec<char> = std::iter::once(*first).chain(rest.iter().copied()).collect();
            let font = covering(&side_font, &arc_chars, &mut notices);
            let arc = arc_cell(&arc_chars, font, c * ARC_SCALE, radius);
            let at = (-arc.1.0, -arc.1.1);
            vec![arc.0.at(at), centre.ink_centred_at((0.0, 0.0))]
        }
        (Style::EvenThree, letters) => {
            let fonts_of = |i: usize| if letters.len() == 3 && i != 1 { &side_font } else { &centre_font };
            let cells: Vec<Cell> = letters.iter().enumerate().map(|(i, ch)| letter(&[*ch], fonts_of(i), c, &mut notices)).collect();
            row(cells, |_, _| gap)
        }
        (Style::Interlocking, letters) => {
            let cells: Vec<Cell> = letters.iter().map(|ch| letter(&[*ch], &centre_font, c, &mut notices)).collect();
            row(cells, |a, b| gap - OVERLAP * a.ink.width().min(b.ink.width()))
        }
        (Style::Stacked, letters) => {
            let mut top = 0.0;
            letters
                .iter()
                .map(|ch| {
                    let cell = letter(&[*ch], &centre_font, c * STACK_SCALE, &mut notices);
                    let centre = (0.0, top + cell.ink.height() / 2.0);
                    top += cell.ink.height() + gap;
                    cell.ink_centred_at(centre)
                })
                .collect()
        }
    };

    let ink = placed.iter().fold(Aabb::EMPTY, |acc, (cell, at)| acc.union(cell.ink.translate(at.0, at.1)));
    let mut children: Vec<Shape> = Vec::new();
    if let Some(art) = spec.frame.art().filter(|_| !ink.is_empty()) {
        let inset = adjust.thickness_in / 2.0 + adjust.padding_in;
        let scale = art.content.fit_scale(ink.width(), ink.height(), inset);
        let centre = art.content.centre();
        let origin = (ink.centre().0 - centre.0 * scale, ink.centre().1 - centre.1 * scale);
        let rings = art.rings(scale, origin, adjust.thickness_in, spec.output_made() == Output::Knockout);
        if let Some(frame) = crate::design::shape_from_rings(0, rings, colours.frame, operation) {
            children.push(Shape { name: Some(format!("{} frame", spec.frame.label())), ..frame });
        }
    }
    for (cell, (x, y)) in placed {
        let (width_in, height_in) = cell.size;
        let pen = crate::pens::Pen::nearest(colours.letters, Default::default());
        children.push(Shape { x_in: x, y_in: y, width_in, height_in, fill: colours.letters, operation, pen, ..Shape::new(0, cell.kind) });
    }
    let bounds = children.iter().fold(Aabb::EMPTY, |acc, s| acc.union(Aabb { min_x: s.x_in, min_y: s.y_in, max_x: s.x_in + s.width_in, max_y: s.y_in + s.height_in }));
    if bounds.is_empty() {
        return MonogramLayout { children, size: (0.0, 0.0), notices };
    }
    for (i, s) in children.iter_mut().enumerate() {
        s.id = i as u64;
        s.x_in -= bounds.min_x;
        s.y_in -= bounds.min_y;
    }
    notices.dedup();
    MonogramLayout { children, size: (bounds.width(), bounds.height()), notices }
}

/// Letters in a row, baselines level, each ink box `gap(prev, next)` after the one before.
fn row(cells: Vec<Cell>, gap: impl Fn(&Cell, &Cell) -> f32) -> Vec<(Cell, Pt)> {
    let mut out: Vec<(Cell, Pt)> = Vec::new();
    let mut right = 0.0;
    for cell in cells {
        let left = match out.last() {
            Some((prev, _)) => right + gap(prev, &cell),
            None => 0.0,
        };
        right = left + cell.ink.width();
        let at = (left - cell.ink.min_x, -cell.baseline);
        out.push((cell, at));
    }
    out
}

/// A text kind as the Text tool would make it, with these settings.
fn text(content: String, font: FontRef, size_in: f32, letter_space: f32, curve: Option<crate::effects::Curve>) -> ShapeKind {
    ShapeKind::Text {
        content,
        font,
        size_in,
        style: Default::default(),
        letter_space,
        line_space: 0.0,
        align: Default::default(),
        wrap_in: None,
        v_align: Default::default(),
        curve,
        path: None,
    }
}

/// `font` when it has a glyph for each of `chars`, else a fallback that does, with a notice for each
/// character it lacked: a writing font falls back to the default writing font, anything else to Inter.
fn covering(font: &FontRef, chars: &[char], notices: &mut Vec<String>) -> FontRef {
    let missing: Vec<char> = chars.iter().copied().filter(|&ch| !has_glyph(font, ch)).collect();
    if missing.is_empty() {
        return font.clone();
    }
    let writing = FontRef::Stroke(StrokeFontId(crate::system_fonts::FALLBACK_STROKE.into()));
    let fallback = match font.is_writing() && chars.iter().all(|&ch| has_glyph(&writing, ch)) {
        true => writing,
        false => FontRef::Inter,
    };
    for ch in missing {
        notices.push(format!("{} has no '{ch}'. Kerf used {} for it.", font.label(), fallback.label()));
    }
    fallback
}

/// Whether `font` draws `ch`, read from a layout of it alone.
fn has_glyph(font: &FontRef, ch: char) -> bool {
    let content = ch.to_string();
    crate::text::layout(&crate::text::TextSpec { content: &content, font, ..Default::default() }).is_some_and(|l| !l.glyphs.is_empty())
}

/// One laid-out text: its kind, its box, where its ink sits in the box, and its first baseline.
struct Cell {
    kind: ShapeKind,
    size: Pt,
    ink: Aabb,
    baseline: f32,
}

impl Cell {
    fn of(kind: ShapeKind) -> Cell {
        let laid = kind.text_layout();
        let size = laid.as_ref().map_or((0.5, 0.5), |l| l.size);
        let ink = laid.as_ref().map_or(Aabb::EMPTY, |l| contours_bounds(&l.contours()));
        let ink = if ink.is_empty() { Aabb { min_x: 0.0, min_y: 0.0, max_x: size.0, max_y: size.1 } } else { ink };
        let baseline = laid.as_ref().and_then(|l| l.glyphs.first()).map_or(size.1 * 0.8, |g| g.pen.1);
        Cell { kind, size, ink, baseline }
    }

    fn at(self, at: Pt) -> (Cell, Pt) {
        (self, at)
    }

    /// Placed so its ink box is centred on `(x, y)`.
    fn ink_centred_at(self, (x, y): Pt) -> (Cell, Pt) {
        let at = (x - self.ink.width() / 2.0 - self.ink.min_x, y - self.ink.height() / 2.0 - self.ink.min_y);
        (self, at)
    }

    /// Placed level with the origin, its ink's right edge (`left_of`) or left edge at `edge`.
    fn ink_beside(self, edge: f32, left_of: bool) -> (Cell, Pt) {
        let x = if left_of { edge - self.ink.width() / 2.0 } else { edge + self.ink.width() / 2.0 };
        self.ink_centred_at((x, 0.0))
    }
}

/// Circle's arc: `chars` (one or two) curved over a circle of `radius` whose centre the returned point
/// gives in the text's box, spaced so two letters sit `ARC_DEG` either side of the top.
fn arc_cell(chars: &[char], font: FontRef, size_in: f32, radius: f32) -> (Cell, Pt) {
    let curve = Some(crate::effects::Curve { diameter_in: 2.0 * radius, enabled: true });
    let content: String = chars.iter().collect();
    let advance = |ch: &char| Cell::of(text(ch.to_string(), font.clone(), size_in, 0.0, None)).size.0;
    let letter_space = match chars {
        [a, b] => {
            let natural = Cell::of(text(content.clone(), font.clone(), size_in, 0.0, None)).size.0;
            let between = natural - (advance(a) + advance(b)) / 2.0;
            let wanted = 2.0 * ARC_DEG.to_radians() * radius;
            (wanted - between) / size_in * 1000.0
        }
        _ => 0.0,
    };
    let cell = Cell::of(text(content, font.clone(), size_in, letter_space, curve));
    let laid = cell.kind.text_layout();
    let centre = laid
        .and_then(|l| {
            let g = l.glyphs.first()?;
            let half = advance(&chars[g.char_index.min(chars.len() - 1)]) / 2.0;
            let (sin, cos) = g.angle.sin_cos();
            let on_arc = (g.pen.0 + half * cos, g.pen.1 + half * sin);
            Some((on_arc.0 - radius * sin, on_arc.1 + radius * cos))
        })
        .unwrap_or((cell.size.0 / 2.0, cell.size.1 + radius));
    (cell, centre)
}

fn contours_bounds(contours: &[Contour]) -> Aabb {
    contours.iter().flat_map(|c| crate::geometry::flatten(c, FLATTEN_TOL_IN)).fold(Aabb::EMPTY, |acc, (x, y)| acc.union(Aabb { min_x: x, min_y: y, max_x: x, max_y: y }))
}

/// Where a frame's letters go, in its SVG's user units, on the band centreline.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Content {
    Ellipse { centre: Pt, radii: Pt },
    Rect { min: Pt, size: Pt },
    Diamond { centre: Pt, radii: Pt },
}

impl Content {
    fn parse(s: &str) -> Result<Content, String> {
        let mut words = s.split_whitespace();
        let kind = words.next().ok_or("data-content is empty")?;
        let n: Vec<f32> = words.map(|w| w.parse::<f32>().map_err(|e| format!("data-content {w}: {e}"))).collect::<Result<_, _>>()?;
        let [a, b, c, d] = n[..] else { return Err(format!("data-content needs four numbers, got {}", n.len())) };
        match kind {
            "ellipse" => Ok(Self::Ellipse { centre: (a, b), radii: (c, d) }),
            "rect" => Ok(Self::Rect { min: (a, b), size: (c, d) }),
            "diamond" => Ok(Self::Diamond { centre: (a, b), radii: (c, d) }),
            other => Err(format!("unknown data-content kind {other}")),
        }
    }

    fn centre(self) -> Pt {
        match self {
            Self::Ellipse { centre, .. } | Self::Diamond { centre, .. } => centre,
            Self::Rect { min, size } => (min.0 + size.0 / 2.0, min.1 + size.1 / 2.0),
        }
    }

    /// Whether a `w` by `h` box centred on the region fits inside it, scaled by `scale` and inset by
    /// `inset` inches.
    fn holds(self, scale: f32, w: f32, h: f32, inset: f32) -> bool {
        match self {
            Self::Ellipse { radii, .. } => {
                let (a, b) = (radii.0 * scale - inset, radii.1 * scale - inset);
                a > 0.0 && b > 0.0 && (w / 2.0 / a).powi(2) + (h / 2.0 / b).powi(2) <= 1.0
            }
            Self::Rect { size, .. } => size.0 * scale - 2.0 * inset >= w && size.1 * scale - 2.0 * inset >= h,
            Self::Diamond { radii, .. } => {
                let (a, b) = (radii.0 * scale, radii.1 * scale);
                w / 2.0 / a + h / 2.0 / b <= 1.0 - inset * (1.0 / (a * a) + 1.0 / (b * b)).sqrt()
            }
        }
    }

    /// The smallest scale, in inches per unit, at which a `w` by `h` box fits with `inset` to spare.
    fn fit_scale(self, w: f32, h: f32, inset: f32) -> f32 {
        let mut hi = 1e-3;
        while !self.holds(hi, w, h, inset) && hi < 1e6 {
            hi *= 2.0;
        }
        let mut lo = hi / 2.0;
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if self.holds(mid, w, h, inset) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    }
}

/// A frame's artwork in its SVG's user units.
struct FrameArt {
    /// The viewBox size.
    size: Pt,
    content: Content,
    /// Closed centrelines, made into outlines as wide as the Thickness adjustment.
    bands: Vec<Contour>,
    /// Filled closed shapes, scaled with the frame.
    decorations: Vec<Contour>,
}

impl FrameArt {
    fn parse(svg: &[u8]) -> Result<FrameArt, String> {
        use usvg::NodeExt;
        let text = std::str::from_utf8(svg).map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        let content = Content::parse(doc.root_element().attribute("data-content").ok_or("no data-content on the root")?)?;
        let tree = crate::svg_import::parse_tree(svg)?;
        let (mut bands, mut decorations) = (Vec::new(), Vec::new());
        for node in tree.root.descendants() {
            let usvg::NodeKind::Path(path) = &*node.borrow() else { continue };
            let Some(data) = path.data.as_ref().clone().transform(node.abs_transform()) else { continue };
            let contours = contours_of(&data);
            match (path.fill.is_some(), path.stroke.is_some()) {
                (false, true) => bands.extend(contours),
                (true, _) => decorations.extend(contours),
                (false, false) => {}
            }
        }
        if bands.is_empty() && decorations.is_empty() {
            return Err("nothing drawn".into());
        }
        Ok(FrameArt { size: (tree.view_box.rect.width(), tree.view_box.rect.height()), content, bands, decorations })
    }

    /// The frame's closed rings in inches: `scale` inches per unit, the art's origin at `origin`, each
    /// band `thickness` wide, welded with the decoration. `solid` fills each band's inside instead, for
    /// Knockout.
    fn rings(&self, scale: f32, origin: Pt, thickness: f32, solid: bool) -> Vec<Ring> {
        let place = |c: &Contour| -> Ring {
            let to = |(x, y): Pt| (origin.0 + x * scale, origin.1 + y * scale);
            let inches = Contour {
                start: to(c.start),
                segs: c
                    .segs
                    .iter()
                    .map(|s| match *s {
                        Seg::Line(p) => Seg::Line(to(p)),
                        Seg::Cubic(a, b, p) => Seg::Cubic(to(a), to(b), to(p)),
                    })
                    .collect(),
                closed: true,
            };
            crate::geometry::flatten(&inches, FLATTEN_TOL_IN / 2.0)
        };
        let half = thickness / 2.0;
        let mut groups: Vec<Vec<Ring>> = Vec::new();
        for band in &self.bands {
            let centreline = vec![place(band)];
            let outer = crate::geometry::offset(&centreline, half, Join::Round);
            groups.push(match solid {
                true => outer,
                false => crate::geometry::boolean(&[outer, crate::geometry::offset(&centreline, -half, Join::Round)], Boolean::Subtract),
            });
        }
        groups.extend(self.decorations.iter().map(|d| vec![place(d)]));
        crate::geometry::boolean(&groups, Boolean::Weld)
    }
}

/// The contours of one usvg path, every subpath taken as closed.
fn contours_of(data: &usvg::tiny_skia_path::Path) -> Vec<Contour> {
    use usvg::tiny_skia_path::PathSegment;
    let at = |p: usvg::tiny_skia_path::Point| (p.x, p.y);
    let mut contours: Vec<Contour> = Vec::new();
    let mut last = (0.0, 0.0);
    for seg in data.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                last = at(p);
                contours.push(Contour { start: last, segs: Vec::new(), closed: true });
            }
            PathSegment::LineTo(p) => {
                last = at(p);
                if let Some(c) = contours.last_mut() {
                    c.segs.push(Seg::Line(last));
                }
            }
            PathSegment::QuadTo(q, p) => {
                let (q, end) = (at(q), at(p));
                let lerp = |a: Pt, b: Pt| (a.0 + (b.0 - a.0) * 2.0 / 3.0, a.1 + (b.1 - a.1) * 2.0 / 3.0);
                if let Some(c) = contours.last_mut() {
                    c.segs.push(Seg::Cubic(lerp(last, q), lerp(end, q), end));
                }
                last = end;
            }
            PathSegment::CubicTo(a, b, p) => {
                last = at(p);
                if let Some(c) = contours.last_mut() {
                    c.segs.push(Seg::Cubic(at(a), at(b), last));
                }
            }
            PathSegment::Close => {}
        }
    }
    contours.retain(|c| !c.segs.is_empty());
    contours
}

impl Design {
    /// Adds a monogram group made from `spec`, centred at `at` or in the next free slot. Returns its id.
    pub(crate) fn insert_monogram(&mut self, spec: &MonogramSpec, at: Option<Pt>) -> u64 {
        let laid = layout(spec, &crate::system_fonts::current());
        let (w, h) = laid.size;
        let centre = at.unwrap_or_else(|| {
            let (x, y) = self.next_free_slot(w.max(h));
            (x + w / 2.0, y + h / 2.0)
        });
        let (x0, y0) = (centre.0 - w / 2.0, centre.1 - h / 2.0);
        let colours = spec.colours();
        let id = self.next_id();
        let group = Shape {
            x_in: x0,
            y_in: y0,
            width_in: w.max(0.01),
            height_in: h.max(0.01),
            fill: colours.frame,
            operation: spec.operation.operation(),
            pen: crate::pens::Pen::nearest(colours.frame, Default::default()),
            name: Some(spec.group_name()),
            monogram: Some(Box::new(spec.clone())),
            ..Shape::new(id, ShapeKind::Compound(spec.compound()))
        };
        self.shapes.push(group);
        let children: Vec<Shape> = laid.children.into_iter().map(|s| Shape { x_in: s.x_in + x0, y_in: s.y_in + y0, ..s }).collect();
        self.adopt(id, self.shapes.len(), children);
        self.settle(None);
        id
    }

    /// Lays monogram `id` out again from `spec`, in place: it keeps its id, centre, rotation and flips,
    /// and fits its old box keeping the new proportions. Frame and letters keep their colours.
    pub(crate) fn update_monogram(&mut self, id: u64, spec: &MonogramSpec) {
        let Some(g) = self.shapes.iter().find(|s| s.id == id).cloned() else { return };
        let laid = layout(spec, &crate::system_fonts::current());
        let (w0, h0) = (laid.size.0.max(0.01), laid.size.1.max(0.01));
        let fit = (g.width_in / w0).min(g.height_in / h0);
        let (w, h) = (w0 * fit, h0 * fit);
        let (cx, cy) = g.centre_in();
        let was = Shape { width_in: w0, height_in: h0, ..Shape::new(0, ShapeKind::Compound(CompoundOp::Effects)) };
        let now = Shape { x_in: cx - w / 2.0, y_in: cy - h / 2.0, width_in: w, height_in: h, ..g.clone() };
        let old: Vec<&Shape> = self.children(Some(id)).collect();
        let frame_fill = old.iter().find(|s| !matches!(s.kind, ShapeKind::Text { .. })).map(|s| s.fill);
        let letter_fill = old.iter().find(|s| matches!(s.kind, ShapeKind::Text { .. })).map(|s| s.fill);
        let children: Vec<Shape> = laid
            .children
            .into_iter()
            .map(|mut s| {
                crate::containers::remap(&mut s, &was, &now);
                let fill = match s.kind {
                    ShapeKind::Text { .. } => letter_fill.unwrap_or(s.fill),
                    _ => frame_fill.unwrap_or(s.fill),
                };
                Shape { fill, pen: crate::pens::Pen::nearest(fill, s.pen.size), ..s }
            })
            .collect();
        let gone = self.with_descendants(&self.children(Some(id)).map(|s| s.id).collect::<Vec<_>>());
        self.shapes.retain(|s| !gone.contains(&s.id));
        let Some(at) = self.shapes.iter().position(|s| s.id == id) else { return };
        let renamed = match &g.monogram {
            Some(before) if g.name.as_deref() != Some(before.group_name().as_str()) => g.name.clone(),
            _ => Some(spec.group_name()),
        };
        self.shapes[at] =
            Shape { kind: ShapeKind::Compound(spec.compound()), operation: spec.operation.operation(), name: renamed, monogram: Some(Box::new(spec.clone())), ..now };
        self.adopt(id, at + 1, children);
        self.settle(None);
    }

    /// Inserts `children` at `index` under fresh ids, as children of `parent`.
    fn adopt(&mut self, parent: u64, index: usize, children: Vec<Shape>) {
        let ids = self.next_id()..;
        let children: Vec<Shape> = ids.zip(children).map(|(id, s)| Shape { id, parent: Some(parent), link: None, container: None, ..s }).collect();
        self.shapes.splice(index..index, children);
    }

    /// Whether monogram group `g`'s children are still what its spec lays out, up to the group's own
    /// move, resize, turn and flips. Fills, pens and operations may change; anything else is a hand edit.
    pub fn monogram_intact(&self, g: &Shape) -> bool {
        let Some(spec) = &g.monogram else { return false };
        let laid = layout(spec, &crate::system_fonts::current());
        let kids: Vec<&Shape> = self.children(Some(g.id)).collect();
        if kids.len() != laid.children.len() {
            return false;
        }
        let (w0, h0) = (laid.size.0.max(0.01), laid.size.1.max(0.01));
        let tol = 1e-3 * g.width_in.max(g.height_in).max(1.0);
        let in_group = |(x, y): Pt| g.to_world((x / w0 * g.width_in, y / h0 * g.height_in));
        kids.iter().zip(&laid.children).all(|(k, e)| {
            let corners = |s: &Shape| [(0.0, 0.0), (s.width_in, 0.0), (s.width_in, s.height_in), (0.0, s.height_in)];
            let placed = corners(k).map(|c| k.to_world(c));
            let wanted = corners(e).map(|(x, y)| in_group((e.x_in + x, e.y_in + y)));
            k.kind == e.kind
                && k.effects.is_empty()
                && !self.shapes.iter().any(|s| s.parent == Some(k.id))
                && placed.iter().zip(wanted).all(|(p, q)| (p.0 - q.0).hypot(p.1 - q.1) <= tol)
        })
    }

    /// The monogram `on` names, when it is one monogram group that still follows its spec.
    pub fn editable_monogram(&self, on: &[u64]) -> Result<u64, Blocked> {
        let [id] = on else { return Err(Blocked::NotMonogram) };
        let g = self.shapes.iter().find(|s| s.id == *id && s.monogram.is_some()).ok_or(Blocked::NotMonogram)?;
        if self.monogram_intact(g) { Ok(*id) } else { Err(Blocked::MonogramEdited) }
    }

    /// Whether `Edit::Monogram` may lay `id` out again from `spec`.
    pub(crate) fn check_monogram(&self, id: u64, spec: &MonogramSpec) -> Result<(), Blocked> {
        if !self.shapes.iter().any(|s| s.id == id && s.monogram.is_some()) {
            return Err(Blocked::NotMonogram);
        }
        if spec.initials().is_empty() { Err(Blocked::NoLetters) } else { Ok(()) }
    }

    /// The monogram groups among `on`.
    pub(crate) fn monograms_in(&self, on: &[u64]) -> Vec<u64> {
        self.shapes.iter().filter(|s| on.contains(&s.id) && s.monogram.is_some()).map(|s| s.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(letters: &str, style: Style, frame: Frame) -> MonogramSpec {
        crate::system_fonts::FontCatalogue::fixture().make_current();
        MonogramSpec { letters: letters.into(), style, frame, ..MonogramSpec::default() }
    }

    fn laid(spec: &MonogramSpec) -> MonogramLayout {
        layout(spec, &crate::system_fonts::current())
    }

    fn texts(l: &MonogramLayout) -> Vec<&Shape> {
        l.children.iter().filter(|s| matches!(s.kind, ShapeKind::Text { .. })).collect()
    }

    fn content(s: &Shape) -> &str {
        match &s.kind {
            ShapeKind::Text { content, .. } => content,
            _ => "",
        }
    }

    fn size_in(s: &Shape) -> f32 {
        match s.kind {
            ShapeKind::Text { size_in, .. } => size_in,
            _ => 0.0,
        }
    }

    /// World points along a shape's outline.
    fn ink(s: &Shape) -> Vec<Pt> {
        s.kind.outline(s.width_in, s.height_in).iter().flat_map(|c| crate::geometry::flatten(c, 0.01)).map(|p| s.to_world(p)).collect()
    }

    fn bounds(pts: &[Pt]) -> Aabb {
        pts.iter().fold(Aabb::EMPTY, |acc, &(x, y)| acc.union(Aabb { min_x: x, min_y: y, max_x: x, max_y: y }))
    }

    fn inside(ring: &[Pt], (px, py): Pt) -> bool {
        let mut hit = false;
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if (a.1 > py) != (b.1 > py) && px < a.0 + (py - a.1) * (b.0 - a.0) / (b.1 - a.1) {
                hit = !hit;
            }
        }
        hit
    }

    fn distance_to_ring(ring: &[Pt], p: Pt) -> f32 {
        (0..ring.len())
            .map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
                (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
            })
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn each_style_has_its_number_of_children() {
        let expected = [(Style::Single, 1), (Style::ClassicThree, 3), (Style::EvenThree, 3), (Style::Stacked, 3), (Style::Interlocking, 3), (Style::Circle, 2)];
        for (style, letters) in expected {
            let framed = laid(&spec("ABC", style, Frame::Circle));
            assert_eq!(texts(&framed).len(), letters, "{style:?} texts");
            assert_eq!(framed.children.len(), letters + 1, "{style:?} with a frame");
            assert_eq!(laid(&spec("ABC", style, Frame::None)).children.len(), letters, "{style:?} without a frame");
        }
        assert_eq!(texts(&laid(&spec("AB", Style::Stacked, Frame::None))).len(), 2);
        assert_eq!(texts(&laid(&spec("A B", Style::EvenThree, Frame::None))).len(), 2, "spaces are ignored");
    }

    #[test]
    fn classic_three_puts_the_second_letter_in_the_centre_at_full_size() {
        let l = laid(&spec("FLM", Style::ClassicThree, Frame::None));
        let [first, centre, middle] = texts(&l)[..] else { panic!("three letters") };
        assert_eq!([content(first), content(centre), content(middle)], ["F", "L", "M"]);
        assert_eq!(size_in(centre), Adjust::default().centre_in);
        assert!((size_in(first) - SIDE_SCALE * size_in(centre)).abs() < 1e-6 && size_in(middle) == size_in(first), "sides at 70 %");
        let (f, c, m) = (bounds(&ink(first)), bounds(&ink(centre)), bounds(&ink(middle)));
        assert!(f.max_x < c.min_x && c.max_x < m.min_x, "first left of the centre, middle right of it");
        assert!((f.centre().1 - c.centre().1).abs() < 1e-3, "sides centred on the centre letter");
    }

    #[test]
    fn even_three_and_interlocking_set_one_size() {
        for style in [Style::EvenThree, Style::Interlocking] {
            let l = laid(&spec("ABC", style, Frame::None));
            assert!(texts(&l).iter().all(|t| size_in(t) == Adjust::default().centre_in), "{style:?}");
        }
        let stacked = laid(&spec("ABC", Style::Stacked, Frame::None));
        let boxes: Vec<Aabb> = texts(&stacked).iter().map(|t| bounds(&ink(t))).collect();
        assert!(boxes.windows(2).all(|w| w[0].max_y < w[1].min_y), "stacked top to bottom");
    }

    #[test]
    fn interlocking_letters_overlap() {
        let l = laid(&spec("ABC", Style::Interlocking, Frame::None));
        let boxes: Vec<Aabb> = texts(&l).iter().map(|t| bounds(&ink(t))).collect();
        for w in boxes.windows(2) {
            let overlap = w[0].max_x - w[1].min_x;
            assert!(overlap > 0.05 * w[0].width().min(w[1].width()), "overlap {overlap}");
        }
        let even = laid(&spec("ABC", Style::EvenThree, Frame::None));
        let boxes: Vec<Aabb> = texts(&even).iter().map(|t| bounds(&ink(t))).collect();
        assert!(boxes.windows(2).all(|w| w[0].max_x < w[1].min_x), "even three does not overlap");
    }

    #[test]
    fn circle_curves_first_and_middle_round_the_centre() {
        let l = laid(&spec("FLM", Style::Circle, Frame::None));
        let [arc, centre] = texts(&l)[..] else { panic!("an arc and a centre") };
        assert_eq!((content(arc), content(centre)), ("FM", "L"));
        assert!(matches!(arc.kind, ShapeKind::Text { curve: Some(c), .. } if c.diameter_in > 0.0), "curved text");
        let (a, c) = (bounds(&ink(arc)), bounds(&ink(centre)));
        assert!(a.min_x < c.min_x && a.max_x > c.max_x, "the arc spans the centre letter");
        assert!(a.min_y < c.min_y, "the arc rises above it");
    }

    #[test]
    fn every_layout_fits_inside_its_frame_with_padding() {
        for frame in Frame::ALL.into_iter().filter(|f| *f != Frame::None) {
            for style in Style::ALL {
                let s = spec("WMQ", style, frame);
                let l = laid(&s);
                let rings: Vec<Ring> = {
                    let f = &l.children[0];
                    assert!(!matches!(f.kind, ShapeKind::Text { .. }), "{frame:?} comes first");
                    f.kind.outline(f.width_in, f.height_in).iter().map(|c| crate::geometry::flatten(c, 0.005).into_iter().map(|p| f.to_world(p)).collect()).collect()
                };
                let clearance = s.adjust.padding_in * 0.95;
                for text in texts(&l) {
                    for p in ink(text) {
                        let around = rings.iter().filter(|r| inside(r, p)).count();
                        assert!(around >= 1 && around % 2 == 0, "{frame:?} {style:?}: {p:?} must be in the frame's hole");
                        let near = rings.iter().map(|r| distance_to_ring(r, p)).fold(f32::MAX, f32::min);
                        assert!(near >= clearance, "{frame:?} {style:?}: {p:?} is only {near} from the frame");
                    }
                }
            }
        }
    }

    #[test]
    fn a_missing_glyph_falls_back_to_inter_with_a_notice() {
        let vibes = FontRef::System("Great Vibes".into());
        let ch = ['Ж', 'Ω', 'Ə', 'Ʃ', 'Ѯ', 'Δ']
            .into_iter()
            .find(|&c| !has_glyph(&vibes, c) && has_glyph(&FontRef::Inter, c))
            .expect("a letter Great Vibes lacks and Inter has");
        let mut s = spec(&format!("A{ch}C"), Style::EvenThree, Frame::Circle);
        s.fonts = Fonts { centre: vibes.clone(), sides: vibes };
        let l = laid(&s);
        let letters = texts(&l);
        assert_eq!(letters.len(), 3, "no letter goes missing");
        assert!(matches!(&letters[1].kind, ShapeKind::Text { font: FontRef::Inter, content, .. } if *content == ch.to_string()));
        assert!(!ink(letters[1]).is_empty(), "the fallback draws it");
        assert_eq!(l.notices, [format!("Great Vibes has no '{ch}'. Kerf used Inter for it.")]);
    }

    #[test]
    fn pen_sets_fonts_with_a_writing_font_in_it() {
        let mut s = spec("ABC", Style::ClassicThree, Frame::Circle);
        s.apply_theme(theme("Botanical").unwrap());
        s.output = Output::Welded;
        s.operation = MakeAs::Pen;
        let l = laid(&s);
        let fonts: Vec<FontRef> = texts(&l)
            .iter()
            .filter_map(|t| match &t.kind {
                ShapeKind::Text { font, .. } => Some(font.clone()),
                _ => None,
            })
            .collect();
        let stroke = |id: &str| FontRef::Stroke(StrokeFontId(id.into()));
        assert_eq!(fonts, [stroke("hershey-duplex"), stroke("hershey-script-complex"), stroke("hershey-duplex")]);
        assert!(l.children.iter().all(|c| c.operation == Operation::Pen));
        assert_eq!(s.compound(), CompoundOp::Effects, "a pen keeps the output layered");
        let mut user = s.clone();
        user.fonts.centre = FontRef::System(crate::system_fonts::FIXTURE_FAMILIES[0].into());
        let kept = laid(&user);
        assert!(
            matches!(&texts(&kept)[1].kind, ShapeKind::Text { font: FontRef::System(f), .. } if f == crate::system_fonts::FIXTURE_FAMILIES[0]),
            "no writing font, no change"
        );
    }

    #[test]
    fn themes_load_with_their_fonts() {
        let names: Vec<&str> = themes().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Classic", "Modern", "Script", "Sports", "Botanical"]);
        for t in themes() {
            for font in [&t.fonts.centre, &t.fonts.sides] {
                assert!(!matches!(font, FontRef::System(f) if crate::system_fonts::face(f, Default::default()).is_none()), "{} font {font:?}", t.name);
            }
        }
        assert_eq!(theme_font("No Such Family"), FontRef::Inter);
    }

    #[test]
    fn every_frame_parses() {
        for frame in Frame::ALL.into_iter().filter(|f| *f != Frame::None) {
            let art = frame.art().unwrap();
            assert!(!art.bands.is_empty(), "{frame:?} has a band");
            assert!(!frame.outline().is_empty(), "{frame:?} outlines");
        }
    }

    #[test]
    fn knockout_and_welded_are_live_booleans() {
        let mut s = spec("ABC", Style::ClassicThree, Frame::Circle);
        assert_eq!(s.compound(), CompoundOp::Effects);
        s.output = Output::Knockout;
        assert_eq!(s.compound(), CompoundOp::Boolean(Combine::Subtract));
        let solid = laid(&s);
        let frame_area = |l: &MonogramLayout| {
            let f = &l.children[0];
            crate::geometry::even_odd_area(&f.kind.outline(f.width_in, f.height_in).iter().map(|c| crate::geometry::flatten(c, 0.01)).collect::<Vec<_>>())
        };
        s.output = Output::Welded;
        assert_eq!(s.compound(), CompoundOp::Boolean(Combine::Weld));
        assert!(frame_area(&solid) > 2.0 * frame_area(&laid(&s)), "Knockout's frame is solid, the others a band");
    }

    fn inserted(s: &MonogramSpec) -> (Design, u64) {
        let mut d = Design::default();
        let id = d.insert_monogram(s, Some((6.0, 6.0)));
        (d, id)
    }

    #[test]
    fn insert_makes_one_named_group_holding_its_spec() {
        let s = spec("ABC", Style::ClassicThree, Frame::Circle);
        let (d, id) = inserted(&s);
        let g = d.shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(g.name.as_deref(), Some("Monogram ABC"));
        assert_eq!(g.monogram.as_deref(), Some(&s));
        assert_eq!(d.children(Some(id)).count(), 4);
        assert!((g.centre_in().0 - 6.0).abs() < 1e-3 && (g.centre_in().1 - 6.0).abs() < 1e-3, "centred where asked");
        assert_eq!(d.editable_monogram(&[id]), Ok(id));
    }

    #[test]
    fn moving_turning_and_sizing_the_group_keep_it_editable_but_editing_a_child_does_not() {
        let (mut d, id) = inserted(&spec("ABC", Style::ClassicThree, Frame::Circle));
        let before = d.clone();
        let g = d.shape_mut(id).unwrap();
        g.x_in += 2.0;
        g.rotation_deg = 30.0;
        g.width_in *= 1.5;
        g.flip_x = true;
        d.settle(Some(&before));
        assert_eq!(d.editable_monogram(&[id]), Ok(id), "the group's own pose is free");

        let letter = d.children(Some(id)).nth(2).unwrap().id;
        let before = d.clone();
        d.shape_mut(letter).unwrap().x_in += 0.3;
        d.settle(Some(&before));
        assert_eq!(d.editable_monogram(&[id]), Err(Blocked::MonogramEdited));
        assert_eq!(Blocked::MonogramEdited.reason(), "This group was edited by hand.");
        assert_eq!(d.editable_monogram(&[letter]), Err(Blocked::NotMonogram));
    }

    #[test]
    fn update_keeps_the_group_and_its_pose() {
        let (mut d, id) = inserted(&spec("ABC", Style::ClassicThree, Frame::Circle));
        let g = d.shape_mut(id).unwrap();
        g.rotation_deg = 20.0;
        let before = d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let next = MonogramSpec { style: Style::Circle, theme: None, ..before.monogram.as_deref().unwrap().clone() };
        d.update_monogram(id, &next);
        let after = d.shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(after.monogram.as_deref(), Some(&next));
        assert_eq!(after.rotation_deg, 20.0);
        assert!((after.centre_in().0 - before.centre_in().0).abs() < 1e-3 && (after.centre_in().1 - before.centre_in().1).abs() < 1e-3);
        assert!(after.width_in <= before.width_in + 1e-3 && after.height_in <= before.height_in + 1e-3, "fits its old box");
        assert_eq!(d.children(Some(id)).count(), 3, "frame, arc and centre");
        assert_eq!(d.editable_monogram(&[id]), Ok(id), "laid out as its spec says");
    }

    #[test]
    fn welded_output_composes_to_one_cut_path() {
        let mut s = spec("ABC", Style::Interlocking, Frame::Circle);
        s.output = Output::Welded;
        let (d, id) = inserted(&s);
        let mats = crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0].items.iter().map(|i| i.piece.owner).collect::<Vec<_>>(), [id], "one welded piece");
    }

    #[test]
    fn a_monogram_round_trips_through_a_project_file_and_old_files_load() {
        let (d, id) = inserted(&spec("ABC", Style::Stacked, Frame::Shield));
        let project = crate::project::Project::new(d);
        let (back, _) = crate::project::parse(&crate::project::to_json(&project).unwrap()).unwrap();
        let g = back.design.shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(g.monogram, project.design.shapes.iter().find(|s| s.id == id).unwrap().monogram);
        assert_eq!(back.design.editable_monogram(&[id]), Ok(id));

        let old = r#"{"format_version":11,"id":"x","name":"Old","created":0,"modified":0,"design":{"shapes":[
            {"id":1,"kind":"Square","x_in":1.0,"y_in":1.0,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,
             "fill":[0,0,0,255],"operation":"BasicCut","hidden":false}]}}"#;
        let (old, _) = crate::project::parse(old).unwrap();
        assert!(old.design.shapes[0].monogram.is_none());
    }
}

use egui::Color32;

use crate::bridge::ArtType;
use crate::material::{Clamp, Material, Tool};
use crate::shape_library::{self, Outline};
use crate::snap::Aabb;

/// Serialises an egui colour as its four premultiplied bytes.
pub(crate) mod color_serde {
    use egui::Color32;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(c: &Color32, s: S) -> Result<S::Ok, S::Error> {
        c.to_array().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let [r, g, b, a] = <[u8; 4]>::deserialize(d)?;
        Ok(Color32::from_rgba_premultiplied(r, g, b, a))
    }
}

pub const POINTS_PER_INCH: f32 = 72.0;

/// The free shapes in Kerf's Shapes panel, in panel order, plus an imported SVG.
/// `Imported` carries its own geometry (in inches, in a `view_box`-sized local frame), so `ShapeKind`
/// is not `Copy`. We keep the parsed contours rather than a `path_d` string to avoid re-parsing each frame.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum ShapeKind {
    Line,
    TriangleDown,
    Triangle,
    Diamond,
    Star,
    StarThin,
    Octagon,
    Pentagon,
    Hexagon,
    Heart,
    Plus,
    Equals,
    Circle,
    Rectangle,
    Square,
    Pill,
    RoundedSquare,
    Oval,
    Arch,
    HalfCircle,
    /// A procedurally generated free shape from `shape_library::BUILTIN`, saved by name.
    Builtin(shape_library::BuiltinId),
    /// A live parametric shape (spec 49): a rectangle, ellipse, polygon, star, arc, ring, line or plus
    /// whose fields stay editable after it is placed. `crate::parametric::outline` draws it.
    Parametric(crate::parametric::Params),
    Imported {
        contours: Vec<Contour>,
        view_box: Pt,
    },
    /// A path drawn with the Pen or Pencil. Each contour's own `closed` flag decides open vs closed.
    Path {
        contours: Vec<Contour>,
        view_box: Pt,
        /// Per contour, one kind per node as `path_edit` counts them. Hints checked against the geometry
        /// on read; empty in projects saved before node kinds.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        node_kinds: Vec<Vec<crate::path_edit::NodeKind>>,
        /// Per contour, one live corner per node, matching `node_kinds` (spec 55). A set corner rounds,
        /// chamfers or scoops its node when the outline is evaluated, leaving the node sharp. Empty, or
        /// all unset, in projects saved before corners.
        #[serde(default, skip_serializing_if = "crate::corners::all_clear")]
        corners: Vec<Vec<crate::corners::Corner>>,
    },
    /// Text, outlined from a bundled or system font on demand. `content` may hold several lines.
    Text {
        content: String,
        font: crate::text::FontRef,
        /// The em height in inches.
        size_in: f32,
        #[serde(default)]
        style: crate::text::FontStyle,
        /// Thousandths of an em after each glyph, -200..=1000.
        #[serde(default)]
        letter_space: f32,
        /// Thousandths of an em between lines, -500..=2000.
        #[serde(default)]
        line_space: f32,
        #[serde(default)]
        align: crate::text::TextAlign,
        #[serde(default)]
        wrap_in: Option<f32>,
        #[serde(default)]
        v_align: crate::text::VAlign,
        /// The circle the text follows; `None` is straight. Only a single unwrapped line curves. Read
        /// through `active_curve`.
        #[serde(default, alias = "curve_in", deserialize_with = "crate::effects::curve_compat")]
        curve: Option<crate::effects::Curve>,
        /// A path a single unwrapped line follows (spec 51), the general form of `curve`. When set it
        /// takes over from the curve. Its contours are the text's own copy of the guide path, so the
        /// text stays whole when the source shape goes.
        #[serde(default)]
        path: Option<crate::text_path::TextPath>,
    },
    /// Flatten: `parts` keep their own fills inside a `view_box`-sized local frame, and the welded
    /// outline is cut as Print Then Cut. See `flatten.rs`.
    Flattened {
        parts: Vec<Shape>,
        view_box: Pt,
    },
    /// A placed image (spec 28); its pixels live in the asset store. `clip` is the cut line in the image's
    /// pixel frame (`0..px`); empty means the image rectangle.
    Bitmap {
        asset: crate::assets::AssetId,
        px: (u32, u32),
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        clip: Vec<Contour>,
    },
    /// A live group whose children stay editable (spec 21 phase 2). It has no outline of its own: what it
    /// draws is evaluated from its children; see `effects::compound`.
    Compound(CompoundOp),
    /// An instance of a component (spec 31): it draws its component's master in its own box, with
    /// `overrides` keyed by master shape id. See `components.rs`.
    Instance {
        component: u64,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        overrides: std::collections::BTreeMap<u64, crate::components::PartOverride>,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompoundOp {
    /// Children draw as themselves; the group's own stack (Offset, Sticker; never Warp) sees all of them.
    Effects,
    /// Children are the inputs of the boolean and draw only through its result.
    Boolean(Combine),
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoilWeight {
    Fine,
    Medium,
    Bold,
}

/// The canvas operation dropdown. Each maps to a `PBArtType` and a `ToolType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    ToFront,
    Forward,
    Backward,
    ToBack,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Slice,
    Weld,
    Unite,
    Subtract,
    Intersect,
    Exclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    HCentre,
    Right,
    Top,
    VCentre,
    Bottom,
    DistributeH,
    DistributeV,
    /// `HCentre` then `VCentre`: every centre lands on one point.
    Centre,
}

impl Combine {
    /// In the order the Combine popover and the Layers op dropdown list them.
    pub const ALL: [Combine; 6] = [Self::Slice, Self::Weld, Self::Unite, Self::Subtract, Self::Intersect, Self::Exclude];

    pub fn label(self) -> &'static str {
        match self {
            Self::Slice => "Slice",
            Self::Weld => "Weld",
            Self::Unite => "Unite",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
            Self::Exclude => "Exclude",
        }
    }
}

impl Align {
    /// Units (lone shapes or whole links) the operation needs before it does anything.
    pub fn min_units(self) -> usize {
        match self {
            Align::DistributeH | Align::DistributeV => 3,
            _ => 2,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Operation {
    #[default]
    BasicCut,
    WavyCut,
    Perforate,
    Pen,
    Foil(FoilWeight),
    Score,
    Deboss,
    Engrave,
    PrintThenCut,
    Guide,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "crate::effects::ShapeRepr")]
pub struct Shape {
    pub id: u64,
    pub kind: ShapeKind,
    pub x_in: f32,
    pub y_in: f32,
    pub width_in: f32,
    pub height_in: f32,
    pub rotation_deg: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    #[serde(with = "color_serde")]
    pub fill: Color32,
    pub operation: Operation,
    pub hidden: bool,
    /// Which contours are hidden; see `effects::ContourMask`.
    pub contours: crate::effects::ContourMask,
    /// The Group or Attach link this shape belongs to; see `links.rs`.
    #[serde(default)]
    pub link: Option<crate::links::Link>,
    /// The pen a Pen layer draws with; ignored for every other operation.
    #[serde(default)]
    pub pen: crate::pens::Pen,
    /// What was done to this layer, in order; see `effects`.
    #[serde(default, skip_serializing_if = "crate::effects::Effects::is_empty")]
    pub effects: crate::effects::Effects,
    /// Present when this shape is a container; see `containers.rs`.
    #[serde(default)]
    pub container: Option<crate::containers::ContainerRole>,
    /// The container holding this shape, the only record of parenthood; see `containers.rs`.
    #[serde(default)]
    pub parent: Option<u64>,
    /// Nesting may only turn this shape's unit 0 or 180 degrees (spec 30).
    #[serde(default)]
    pub grain_lock: bool,
    /// Skew in degrees along the shape's own X and Y axes, -89 to 89, applied between scale and
    /// rotation so the frame leans as a parallelogram (spec 52). Zero when absent.
    #[serde(default, skip_serializing_if = "is_zero_skew")]
    pub skew_deg: (f32, f32),
    /// The point rotation and Rotate copies turn about, in the shape's local frame; `None` is the
    /// centre (spec 52). Saved with the shape, absent on an older project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<Pt>,
    /// W and H change together: typed sizes and side handles keep the proportions (spec 34).
    #[serde(default)]
    pub lock_aspect: bool,
    /// The shape cannot be moved, resized, rotated or flipped, but takes every other edit (spec 34).
    #[serde(default)]
    pub lock_position: bool,
    /// The canvas cannot select this layer by click, marquee or Select All; Layers still can (spec 34).
    #[serde(default)]
    pub locked: bool,
    /// An image painted inside the shape when it is Print Then Cut; `fill` still picks its mat (spec 34).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<crate::patterns::PatternFill>,
    /// The name the user gave this layer, shown in place of its label (spec 40); see `names.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What a Monogram Maker group was made from, so it can be edited again (spec 42); see `monogram.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monogram: Option<Box<crate::monogram::MonogramSpec>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Design {
    pub shapes: Vec<Shape>,
    /// The optimised mat layout, used while it matches the design; see `composition::nested`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nesting: Option<crate::nesting::NestedLayout>,
    /// Reusable masters (spec 31); see `components.rs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<crate::components::Component>,
    /// The component whose master `shapes` holds while it is open for editing; see `components.rs`.
    #[serde(skip)]
    pub editing: Option<u64>,
    /// The names the user gave Group and Attach links, by link id (spec 40); see `names.rs`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub link_names: std::collections::BTreeMap<u64, String>,
    /// Ruler guides (spec 45); see `guides.rs`. Make, export and composition never read them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<crate::guides::RulerGuide>,
    /// View > Lock guides: no guide can be grabbed, so clicks pass through to the shapes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guides_locked: bool,
    /// View > Show guides, off: guides neither draw nor snap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guides_hidden: bool,
    /// The canvas grid (spec 46), `None` until the user sets it, so it follows the units shown; see `grid.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<crate::grid::GridSettings>,
    /// What the magnet snaps to and the angle Shift locks to (spec 47); `#[serde(default)]` loads an
    /// older project with every class on. The old `snap_grid`/`snap_objects` switches migrate into it.
    #[serde(default, skip_serializing_if = "crate::snap::SnapOptions::is_default")]
    pub snap: crate::snap::SnapOptions,
    /// The palette that belongs to this job (spec 56): swatches saved with the project, moved to and
    /// from the user's settings palette. Absent when the project carries none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub swatches: Vec<crate::settings::Swatch>,
    /// Evaluated geometry, shared by every clone; see `effects::Cache`.
    #[serde(skip)]
    pub(crate) cache: crate::effects::Cache,
}

pub type Pt = (f32, f32);

/// A shape with no skew leaves the key out of its JSON, so an older project round-trips unchanged.
fn is_zero_skew(skew: &(f32, f32)) -> bool {
    *skew == (0.0, 0.0)
}

/// Which shapes count as one object when an operation counts or moves objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKey {
    /// An Attach link is one unit; every other shape, Group members included, is its own (Combine).
    Attach,
    /// Any link is one unit (Align, Distribute, Group, Attach, container layout).
    AnyLink,
}

/// What an operation counts or moves as one: a lone shape, or the members of one link, with the union of
/// their world bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub ids: Vec<u64>,
    pub bounds: Aabb,
}

/// `shapes` partitioned by `key`, each unit in the order of its first member, bounds unioned. A shape
/// with no outline adds `Aabb::EMPTY`, so a unit made only of such shapes has empty bounds.
pub fn units_by<'a>(design: &Design, shapes: impl IntoIterator<Item = &'a Shape>, key: UnitKey) -> Vec<Unit> {
    let mut units: Vec<(Option<u64>, Unit)> = Vec::new();
    for s in shapes {
        let link = s.link.filter(|l| key == UnitKey::AnyLink || l.kind == crate::links::LinkKind::Attach).map(|l| l.id);
        let bounds = design.geometry(s).bounds();
        match units.iter_mut().find(|(k, _)| link.is_some() && *k == link) {
            Some((_, u)) => {
                u.ids.push(s.id);
                u.bounds = u.bounds.union(bounds);
            }
            None => units.push((link, Unit { ids: vec![s.id], bounds })),
        }
    }
    units.into_iter().map(|(_, u)| u).collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(Pt),
    Cubic(Pt, Pt, Pt),
}

/// One subpath of a shape outline, in inches in the shape's local frame (origin top left).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Contour {
    pub start: Pt,
    pub segs: Vec<Seg>,
    pub closed: bool,
}

const SLOT_STEP_IN: f32 = 3.0;
const SLOT_ORIGIN_IN: f32 = 0.5;
const SLOTS_PER_ROW: usize = 4;
const CUBIC_STEPS: usize = 12;

impl ShapeKind {
    const PRESETS: [ShapeKind; 20] = [
        Self::Line,
        Self::TriangleDown,
        Self::Triangle,
        Self::Diamond,
        Self::Star,
        Self::StarThin,
        Self::Octagon,
        Self::Pentagon,
        Self::Hexagon,
        Self::Heart,
        Self::Plus,
        Self::Equals,
        Self::Circle,
        Self::Rectangle,
        Self::Square,
        Self::Pill,
        Self::RoundedSquare,
        Self::Oval,
        Self::Arch,
        Self::HalfCircle,
    ];

    /// Every free shape for the Shapes panel: the 20 SVG presets then the procedural built-ins.
    pub fn free() -> Vec<ShapeKind> {
        Self::PRESETS.iter().cloned().chain(shape_library::BuiltinId::all().map(Self::Builtin)).collect()
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::TriangleDown => "Triangle Down",
            Self::Triangle => "Triangle",
            Self::Diamond => "Diamond",
            Self::Star => "Star",
            Self::StarThin => "Thin Star",
            Self::Octagon => "Octagon",
            Self::Pentagon => "Pentagon",
            Self::Hexagon => "Hexagon",
            Self::Heart => "Heart",
            Self::Plus => "Plus",
            Self::Equals => "Equals",
            Self::Circle => "Circle",
            Self::Rectangle => "Rectangle",
            Self::Square => "Square",
            Self::Pill => "Pill",
            Self::RoundedSquare => "Rounded Square",
            Self::Oval => "Oval",
            Self::Arch => "Arch",
            Self::HalfCircle => "Half Circle",
            Self::Builtin(b) => b.get().name,
            Self::Parametric(p) => p.kind().label(),
            Self::Imported { .. } => "Imported",
            Self::Path { .. } => "Path",
            Self::Text { .. } => "Text",
            Self::Flattened { .. } => "Flattened",
            Self::Bitmap { .. } => "Image",
            Self::Compound(CompoundOp::Effects) => "Effect group",
            Self::Compound(CompoundOp::Boolean(op)) => op.label(),
            Self::Instance { .. } => "Component",
        }
    }

    /// `imageSourceHeightDefault` in inches; the width follows the native aspect ratio.
    fn default_height_in(&self) -> f32 {
        match self {
            Self::Line | Self::TriangleDown | Self::Diamond | Self::Arch | Self::HalfCircle => 2.5,
            Self::Triangle | Self::Hexagon => 1.7319,
            Self::Star | Self::StarThin | Self::Pentagon => 1.9028,
            Self::Heart => 1.7681,
            Self::Equals => 1.0722,
            Self::Rectangle | Self::Pill => 1.6667,
            Self::Oval => 1.7667,
            Self::RoundedSquare => 2.00009,
            _ => 2.0,
        }
    }

    pub fn default_size_in(&self) -> (f32, f32) {
        if let Self::Imported { view_box, .. } | Self::Path { view_box, .. } | Self::Flattened { view_box, .. } = self {
            return *view_box;
        }
        if let Self::Builtin(b) = self {
            return b.get().size_in;
        }
        if let Self::Bitmap { px, .. } = self {
            return bitmap_size_in(*px);
        }
        if matches!(self, Self::Text { .. }) {
            return self.text_layout().map_or((0.5, 0.5), |l| l.size);
        }
        if matches!(self, Self::Compound(_) | Self::Instance { .. }) {
            return (1.0, 1.0);
        }
        if let Self::Parametric(params) = self {
            return crate::parametric::default_size_in(params.kind());
        }
        let (w, h) = self.library().size_pt;
        let height = self.default_height_in();
        (height * w / h, height)
    }

    /// The stored layer fill: black, except the Line, which is a grey score line.
    pub fn default_fill(&self) -> Color32 {
        match self {
            Self::Line => Color32::from_rgb(0x4a, 0x49, 0x4a),
            _ => Color32::BLACK,
        }
    }

    pub fn default_operation(&self) -> Operation {
        match self {
            Self::Line => Operation::Score,
            Self::Flattened { .. } | Self::Bitmap { .. } => Operation::PrintThenCut,
            _ => Operation::BasicCut,
        }
    }

    fn library(&self) -> &'static Outline {
        let index = Self::PRESETS.iter().position(|k| k == self).unwrap();
        &shape_library::FREE[index]
    }

    /// The natural layout of a text shape, in its unscaled box. `None` for other kinds.
    pub fn text_layout(&self) -> Option<crate::text::TextLayout> {
        let curve_in = self.active_curve().unwrap_or(0.0);
        let Self::Text { content, font, size_in, style, letter_space, line_space, align, wrap_in, v_align: _, curve: _, path } = self else { return None };
        crate::text::layout(&crate::text::TextSpec {
            content,
            font,
            size_in: *size_in,
            style: *style,
            letter_space: *letter_space,
            line_space: *line_space,
            align: *align,
            wrap_in: *wrap_in,
            curve_in,
            path: path.as_ref(),
        })
    }

    /// The installed-font family this text is set in, when it names one.
    pub fn system_family(&self) -> Option<&str> {
        match self {
            Self::Text { font: crate::text::FontRef::System(family), .. } => Some(family),
            _ => None,
        }
    }

    /// The signed diameter text lays out on: its curve while the curve's eye is on.
    pub fn active_curve(&self) -> Option<f32> {
        match self {
            Self::Text { curve: Some(c), .. } if c.enabled => Some(c.diameter_in),
            _ => None,
        }
    }

    /// Outline for a `w` x `h` inch bounding box.
    pub fn outline(&self, w: f32, h: f32) -> Vec<Contour> {
        match self {
            Self::Imported { contours, view_box } | Self::Path { contours, view_box, .. } => scale_contours(contours, *view_box, (w, h)),
            Self::Builtin(b) => {
                let b = b.get();
                scale_contours(&b.contours, b.size_in, (w, h))
            }
            Self::Text { .. } => self.text_layout().map_or_else(Vec::new, |l| scale_contours(&l.contours(), l.size, (w, h))),
            Self::Flattened { parts, view_box } => crate::flatten::outline(parts, *view_box, (w, h)),
            Self::Bitmap { clip, .. } if clip.is_empty() => {
                vec![Contour { start: (0.0, 0.0), segs: vec![Seg::Line((w, 0.0)), Seg::Line((w, h)), Seg::Line((0.0, h))], closed: true }]
            }
            Self::Bitmap { px, clip, .. } => scale_contours(clip, (px.0 as f32, px.1 as f32), (w, h)),
            Self::Compound(_) | Self::Instance { .. } => Vec::new(),
            Self::Parametric(params) => crate::parametric::outline(params, (w, h)),
            _ => {
                let o = self.library();
                scale_contours(&o.contours, o.size_pt, (w, h))
            }
        }
    }
}

/// An image's first size: 100 pixels per inch, scaled down to fit 6 x 9 in (inside the Print Then Cut
/// area), keeping its aspect.
fn bitmap_size_in((w, h): (u32, u32)) -> Pt {
    let (w, h) = (w as f32 / 100.0, h as f32 / 100.0);
    let fit = (6.0 / w).min(9.0 / h).min(1.0);
    (w * fit, h * fit)
}

/// Builds a free-form shape from world-space rings: bounds become the `view_box`, rings are moved to
/// a local frame at the origin, and the shape is placed at the bounds' minimum.
pub(crate) fn shape_from_rings(id: u64, rings: Vec<Vec<Pt>>, fill: Color32, operation: Operation) -> Option<Shape> {
    let pts: Vec<Pt> = rings.iter().flatten().copied().collect();
    if pts.len() < 3 {
        return None;
    }
    let (min_x, min_y) = pts.iter().fold((f32::MAX, f32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
    let (max_x, max_y) = pts.iter().fold((f32::MIN, f32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
    let (w, h) = ((max_x - min_x).max(0.01), (max_y - min_y).max(0.01));
    let contours: Vec<Contour> = rings
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|r| {
            let local: Vec<Pt> = r.into_iter().map(|(x, y)| (x - min_x, y - min_y)).collect();
            Contour { start: local[0], segs: local[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true }
        })
        .collect();
    Some(Shape {
        x_in: min_x,
        y_in: min_y,
        width_in: w,
        height_in: h,
        fill,
        operation,
        pen: crate::pens::Pen::nearest(fill, Default::default()),
        ..Shape::new(id, ShapeKind::Imported { contours, view_box: (w, h) })
    })
}

/// Builds a `Path` shape from world-space contours (open or closed), preserving beziers. Bounds come
/// from the flattened points so curve control points outside the box are still contained.
pub fn shape_from_contours(id: u64, contours: Vec<Contour>, fill: Color32, operation: Operation) -> Option<Shape> {
    let pts: Vec<Pt> = contours.iter().flat_map(|c| c.flatten()).collect();
    if pts.len() < 2 {
        return None;
    }
    let (min_x, min_y) = pts.iter().fold((f32::MAX, f32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
    let (max_x, max_y) = pts.iter().fold((f32::MIN, f32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
    let (w, h) = ((max_x - min_x).max(0.01), (max_y - min_y).max(0.01));
    let off = |(x, y): Pt| (x - min_x, y - min_y);
    let local: Vec<Contour> = contours
        .into_iter()
        .map(|c| Contour {
            start: off(c.start),
            segs: c
                .segs
                .iter()
                .map(|s| match *s {
                    Seg::Line(p) => Seg::Line(off(p)),
                    Seg::Cubic(a, b, p) => Seg::Cubic(off(a), off(b), off(p)),
                })
                .collect(),
            closed: c.closed,
        })
        .collect();
    Some(Shape {
        x_in: min_x,
        y_in: min_y,
        width_in: w,
        height_in: h,
        fill,
        operation,
        pen: crate::pens::Pen::nearest(fill, Default::default()),
        ..Shape::new(id, ShapeKind::Path { contours: local, view_box: (w, h), node_kinds: Vec::new(), corners: Vec::new() })
    })
}

/// Scales contours from a `from`-sized local frame to a `to`-sized one. A zero-width source axis
/// keeps the source coordinates (so a vertical Line stays a line).
pub fn scale_contours(contours: &[Contour], from: Pt, to: Pt) -> Vec<Contour> {
    let factor = |size: f32, native: f32| if native > 0.0 { size / native } else { 1.0 };
    let (sx, sy) = (factor(to.0, from.0), factor(to.1, from.1));
    let p = |(x, y): Pt| (x * sx, y * sy);
    let seg = |s: &Seg| match *s {
        Seg::Line(a) => Seg::Line(p(a)),
        Seg::Cubic(a, b, c) => Seg::Cubic(p(a), p(b), p(c)),
    };
    contours.iter().map(|c| Contour { start: p(c.start), segs: c.segs.iter().map(seg).collect(), closed: c.closed }).collect()
}

impl Contour {
    pub fn flatten(&self) -> Vec<Pt> {
        let mut pts = vec![self.start];
        for seg in &self.segs {
            match *seg {
                Seg::Line(p) => pts.push(p),
                Seg::Cubic(c1, c2, end) => {
                    let p0 = *pts.last().unwrap();
                    pts.extend((1..=CUBIC_STEPS).map(|i| cubic_at(p0, c1, c2, end, i as f32 / CUBIC_STEPS as f32)));
                }
            }
        }
        if self.closed && pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        pts
    }
}

fn cubic_at(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
}

impl FoilWeight {
    pub const ALL: [FoilWeight; 3] = [Self::Fine, Self::Medium, Self::Bold];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fine => "Fine",
            Self::Medium => "Medium",
            Self::Bold => "Bold",
        }
    }
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::BasicCut => "Basic Cut",
            Self::WavyCut => "Wavy Cut",
            Self::Perforate => "Perforate",
            Self::Pen => "Pen",
            Self::Foil(FoilWeight::Fine) => "Fine Foil",
            Self::Foil(FoilWeight::Medium) => "Medium Foil",
            Self::Foil(FoilWeight::Bold) => "Bold Foil",
            Self::Score => "Score",
            Self::Deboss => "Deboss",
            Self::Engrave => "Engrave",
            Self::PrintThenCut => "Print Then Cut",
            Self::Guide => "Guide",
        }
    }

    /// `None` for Guide, which never leaves the canvas.
    pub fn art_type(self) -> Option<ArtType> {
        Some(match self {
            Self::BasicCut | Self::PrintThenCut => ArtType::Cut,
            Self::WavyCut => ArtType::Wave,
            Self::Perforate => ArtType::Perforate,
            Self::Pen => ArtType::Draw,
            Self::Foil(FoilWeight::Fine) => ArtType::MidasFine,
            Self::Foil(FoilWeight::Medium) => ArtType::MidasRegular,
            Self::Foil(FoilWeight::Bold) => ArtType::MidasBold,
            Self::Score => ArtType::Score,
            Self::Deboss => ArtType::Deboss,
            Self::Engrave => ArtType::Engrave,
            Self::Guide => return None,
        })
    }

    /// Cut operations use the material's blade; every other tool sits in Clamp B except the pen.
    pub fn load(self, material: &Material) -> (Tool, Clamp) {
        let tool = match self {
            Self::BasicCut | Self::PrintThenCut | Self::Guide => return (material.tool, material.clamp),
            Self::WavyCut => Tool::Wave,
            Self::Perforate => Tool::PerforationWheel,
            Self::Pen => return (Tool::Pen, Clamp::A),
            Self::Foil(FoilWeight::Fine) => Tool::MidasFine,
            Self::Foil(FoilWeight::Medium) => Tool::MidasRegular,
            Self::Foil(FoilWeight::Bold) => Tool::MidasBold,
            Self::Score => Tool::ScoringWheel,
            Self::Deboss => Tool::Debossing,
            Self::Engrave => Tool::Engrave,
        };
        (tool, Clamp::B)
    }

    pub fn fiducial_id(self) -> i32 {
        i32::from(self == Self::PrintThenCut)
    }
}

impl Shape {
    /// `kind` in a 1 x 1 in box at the origin, in the kind's default fill, operation and pen: visible,
    /// unrotated, unflipped, with no link, parent, container or effects. Every literal builds on it, so a new
    /// field touches this function rather than every literal.
    pub fn new(id: u64, kind: ShapeKind) -> Shape {
        let fill = kind.default_fill();
        Shape {
            id,
            operation: kind.default_operation(),
            kind,
            x_in: 0.0,
            y_in: 0.0,
            width_in: 1.0,
            height_in: 1.0,
            rotation_deg: 0.0,
            flip_x: false,
            flip_y: false,
            fill,
            hidden: false,
            contours: Default::default(),
            link: None,
            pen: crate::pens::Pen::nearest(fill, Default::default()),
            effects: Default::default(),
            container: None,
            parent: None,
            skew_deg: (0.0, 0.0),
            pivot: None,
            grain_lock: false,
            lock_aspect: false,
            lock_position: false,
            locked: false,
            pattern: None,
            name: None,
            monogram: None,
        }
    }

    pub fn centre_in(&self) -> Pt {
        (self.x_in + self.width_in / 2.0, self.y_in + self.height_in / 2.0)
    }

    /// A local-frame point on the canvas, after flip and rotation about the centre.
    pub fn to_world(&self, p: Pt) -> Pt {
        crate::effects::Placement::of(self).placer()(p)
    }

    /// The inverse of `to_world`.
    pub fn to_local(&self, p: Pt) -> Pt {
        crate::effects::Placement::of(self).to_local(p)
    }

    /// Whether a pattern can fill this shape: it prints, and has no artwork or children of its own.
    pub fn takes_pattern(&self) -> bool {
        self.operation == Operation::PrintThenCut
            && !matches!(self.kind, ShapeKind::Bitmap { .. } | ShapeKind::Flattened { .. } | ShapeKind::Compound(_) | ShapeKind::Instance { .. })
    }

    /// Whether this shape can be a parent: a container, or a Compound.
    pub fn holds_children(&self) -> bool {
        self.container.is_some() || self.compound().is_some()
    }

    pub fn compound(&self) -> Option<CompoundOp> {
        match self.kind {
            ShapeKind::Compound(op) => Some(op),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn hex_colour(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.fill.r(), self.fill.g(), self.fill.b())
    }
}

/// Converts one shape's fixed free kind to its parametric preset in place, recursing into Flattened
/// parts. An upgraded shape stays unnamed, as it was before, and reads by its parametric family label.
fn upgrade_shape_parametric(s: &mut Shape) {
    if let ShapeKind::Flattened { parts, .. } = &mut s.kind {
        for p in parts {
            upgrade_shape_parametric(p);
        }
    }
    if let Some(params) = crate::parametric::from_fixed(&s.kind, (s.width_in, s.height_in)) {
        let live = ShapeKind::Parametric(params);
        // Keep the old name only where the parametric family label would read differently, so a Square
        // stays "Square" while a Star, already labelled "Star", stays unnamed as it was.
        if s.name.is_none() && live.label() != s.kind.label() {
            s.name = Some(s.kind.label().to_owned());
        }
        s.kind = live;
    }
}

/// Slice pieces smaller than this, in square inches, are boolean noise and are dropped.
pub(crate) const MIN_PIECE_AREA_IN2: f32 = 1e-4;

impl Design {
    pub fn add_shape(&mut self, kind: ShapeKind) -> u64 {
        let size = kind.default_size_in();
        self.add_shape_at(kind, size)
    }

    /// Adds `kind` in the next free slot at an explicit size. The Shapes panel and palette size a free
    /// shape here before `crate::parametric::insert_as` turns it into its live parametric preset, so a
    /// placed star or rectangle stays editable while a direct `add_shape` keeps the fixed kind.
    pub fn add_shape_at(&mut self, kind: ShapeKind, (width_in, height_in): Pt) -> u64 {
        let id = self.next_id();
        let (x_in, y_in) = self.next_free_slot(width_in.max(height_in));
        let (fill, operation) = (kind.default_fill(), kind.default_operation());
        let shape = Shape { x_in, y_in, width_in, height_in, fill, operation, pen: crate::pens::Pen::nearest(fill, Default::default()), ..Shape::new(id, kind) };
        self.shapes.push(shape);
        id
    }

    /// Upgrades every fixed free shape (Star, Pentagon, Rectangle, Arch and the rest) saved before spec
    /// 49 to the parametric preset that draws the same outline, keeping the old label as a name. Walks
    /// masters and Flattened parts too, so a nested old shape upgrades as well.
    pub fn upgrade_to_parametric(&mut self) {
        for s in &mut self.shapes {
            upgrade_shape_parametric(s);
        }
        for c in &mut self.components {
            for s in &mut c.shapes {
                upgrade_shape_parametric(s);
            }
        }
    }

    pub fn remove_many(&mut self, ids: &[u64]) {
        let ids = &self.with_descendants(ids);
        self.shapes.retain(|s| !ids.contains(&s.id));
        self.prune_links();
    }

    /// One counter for shape, link and component ids, across the canvas and every master, so none collide.
    pub(crate) fn next_id(&self) -> u64 {
        let shapes = self.shapes.iter().chain(self.components.iter().flat_map(|c| &c.shapes));
        let shape_ids = shapes.flat_map(|s| [Some(s.id), s.link.map(|l| l.id)]).flatten();
        // A named link's id stays taken after the link goes, so a new link never inherits its name.
        shape_ids.chain(self.components.iter().map(|c| c.id)).chain(self.link_names.keys().copied()).max().unwrap_or(0) + 1
    }

    /// Adds a text object at `(x_in, y_in)` with the Pen draw operation. Returns the new id.
    pub fn add_text(&mut self, content: String, font: crate::text::FontRef, size_in: f32, fill: Color32, x_in: f32, y_in: f32) -> u64 {
        let id = self.next_id();
        let kind = ShapeKind::Text {
            content,
            font,
            size_in,
            style: Default::default(),
            letter_space: 0.0,
            line_space: 0.0,
            align: Default::default(),
            wrap_in: None,
            v_align: Default::default(),
            curve: None,
            path: None,
        };
        let (width_in, height_in) = kind.default_size_in();
        let shape =
            Shape { x_in, y_in, width_in, height_in, fill, operation: Operation::Pen, pen: crate::pens::Pen::nearest(fill, Default::default()), ..Shape::new(id, kind) };
        self.shapes.push(shape);
        id
    }

    /// Re-fits a text shape to its new natural size after an edit, given its kind before the edit.
    /// The shape keeps its scale over the natural size, so editing resized text does not snap it back.
    /// A leading warp comes off against the old words and goes back on against the new, so the warped
    /// frame follows the new outline.
    pub fn refit_text(&mut self, id: u64, before: &ShapeKind) {
        let Some(s) = self.shape_mut(id).filter(|s| matches!(s.kind, ShapeKind::Text { .. })) else { return };
        let warp = s.effects.leading_warp();
        let after = std::mem::replace(&mut s.kind, before.clone());
        s.refit_warp(warp, None);
        s.kind = after;
        let (old_w, old_h) = before.default_size_in();
        let (new_w, new_h) = s.kind.default_size_in();
        (s.width_in, s.height_in) = (new_w * s.width_in / old_w, new_h * s.height_in / old_h);
        s.refit_warp(None, warp);
    }

    /// Adds a drawn path (world-space contour) as a shape with the given operation and colour.
    pub fn add_path(&mut self, contour: Contour, fill: Color32, operation: Operation) -> Option<u64> {
        let id = self.next_id();
        let shape = shape_from_contours(id, vec![contour], fill, operation)?;
        self.shapes.push(shape);
        Some(id)
    }

    /// Adds each imported SVG path as its own shape, preserving the relative layout. Returns the new ids.
    pub fn add_imported(&mut self, imports: Vec<crate::shape_library::Import>) -> Vec<u64> {
        let mut ids = Vec::new();
        for (id, imp) in (self.next_id()..).zip(imports) {
            let shape = Shape {
                x_in: imp.x_in,
                y_in: imp.y_in,
                width_in: imp.view_box.0.max(0.01),
                height_in: imp.view_box.1.max(0.01),
                fill: imp.fill,
                operation: imp.operation,
                pen: imp.pen,
                ..Shape::new(id, ShapeKind::Imported { contours: imp.contours, view_box: imp.view_box })
            };
            ids.push(id);
            self.shapes.push(shape);
        }
        ids
    }

    /// Adds an imported tree: each group becomes a Guide-rectangle container holding its children,
    /// nested as `roots` nests and named by its SVG id; each leaf an Imported shape. Settles so the
    /// container invariants hold. Returns the top-level ids and the stroke leaves (id and width).
    pub fn add_imported_tree(&mut self, roots: Vec<crate::svg_import::Node>) -> Imported {
        let mut next = self.next_id();
        let mut shapes = Vec::new();
        let mut added = Imported::default();
        for node in roots {
            let id = build_import_node(node, None, &mut next, &mut shapes, &mut added);
            added.roots.push(id);
        }
        self.shapes.extend(shapes);
        self.settle(None);
        added
    }

    /// Copies each shape, offset by `off_in`, appending them on top. Returns the new ids.
    pub fn duplicate(&mut self, ids: &[u64], off_in: f32) -> Vec<u64> {
        let ids = &self.with_descendants(ids);
        let copies: Vec<Shape> = self.shapes.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
        let mut new_ids = Vec::new();
        let mut pairs = Vec::new();
        for (next, mut copy) in (self.next_id()..).zip(copies) {
            pairs.push((copy.id, next));
            copy.id = next;
            copy.x_in += off_in;
            copy.y_in += off_in;
            new_ids.push(next);
            self.shapes.push(copy);
        }
        self.relink_copies(&new_ids);
        self.reparent_copies(&pairs);
        new_ids
    }

    /// Adds each copy set on top under fresh ids, keeping its links and parenthood closed within the set,
    /// as `duplicate` does for one copy. The shapes already carry their transformed poses (spec 52's
    /// Step and repeat and Rotate copies). Returns every new id, set by set.
    pub(crate) fn place_copies(&mut self, sets: Vec<Vec<Shape>>) -> Vec<Vec<u64>> {
        let mut all = Vec::new();
        for set in sets {
            let mut ids = Vec::new();
            let mut pairs = Vec::new();
            for mut copy in set {
                let next = self.next_id();
                pairs.push((copy.id, next));
                copy.id = next;
                ids.push(next);
                self.shapes.push(copy);
            }
            self.relink_copies(&ids);
            self.reparent_copies(&pairs);
            all.push(ids);
        }
        all
    }

    pub fn shape_mut(&mut self, id: u64) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }

    pub fn hit_test(&self, px: f32, py: f32) -> Option<u64> {
        self.drawn().rev().find(|s| self.geometry(s).contains((px, py))).map(|s| s.id)
    }

    /// The shape a canvas click at this point picks: the top drawn shape under it that is not a locked
    /// layer. A locked layer lets the click through to whatever lies beneath.
    pub fn pick_at(&self, px: f32, py: f32) -> Option<u64> {
        self.drawn().rev().filter(|s| !s.locked).find(|s| self.geometry(s).contains((px, py))).map(|s| s.id)
    }

    /// Axis-aligned bounds `(min_x, min_y, max_x, max_y)` in inches.
    pub fn bounds_in(&self) -> Option<(f32, f32, f32, f32)> {
        let geometries: Vec<_> = self.drawn().map(|s| self.geometry(s)).collect();
        let pts = geometries.iter().flat_map(|g| g.lines()).flat_map(|(pts, _)| pts);
        pts.fold(None, |acc, &(x, y)| {
            let (a, b, c, d) = acc.unwrap_or((x, y, x, y));
            Some((a.min(x), b.min(y), c.max(x), d.max(y)))
        })
    }

    /// Combines the selected shapes live: they become the children of a boolean group at the back-most
    /// one's z slot, and the group draws what combining them makes. Slice needs exactly two units. Returns
    /// the group's id.
    pub fn combine(&mut self, ids: &[u64], op: Combine) -> Vec<u64> {
        if op == Combine::Slice && self.units_by(ids, UnitKey::Attach).len() != 2 {
            return Vec::new();
        }
        self.wrap(ids, CompoundOp::Boolean(op), Default::default()).into_iter().collect()
    }

    /// The shapes among `ids` as units by `key`, back to front by each unit's back-most member.
    pub fn units_by(&self, ids: &[u64], key: UnitKey) -> Vec<Unit> {
        units_by(self, self.shapes.iter().filter(|s| ids.contains(&s.id)), key)
    }

    /// Consumes `ids` with their descendants and puts `shapes` where the back-most of them stood.
    /// Returns the new ids; with nothing to put back, nothing is consumed.
    pub(crate) fn replace_with(&mut self, ids: &[u64], shapes: Vec<Shape>) -> Vec<u64> {
        if shapes.is_empty() {
            return Vec::new();
        }
        let consumed = self.consume(ids);
        self.insert_at(consumed.at, consumed.parent, shapes)
    }

    /// Aligns or distributes `ids` by their world bounds, moving each link as one unit.
    pub fn align(&mut self, ids: &[u64], op: Align) {
        let units = self.units_by(&self.expand(ids), UnitKey::AnyLink);
        if units.len() < op.min_units() {
            return;
        }
        let all = units.iter().fold(Aabb::EMPTY, |acc, u| acc.union(u.bounds));
        let (mid_x, mid_y) = all.centre();
        match op {
            Align::DistributeH | Align::DistributeV => self.distribute(&units, op),
            _ => {
                for u in &units {
                    let b = u.bounds;
                    let (cx, cy) = b.centre();
                    let (dx, dy) = match op {
                        Align::Left => (all.min_x - b.min_x, 0.0),
                        Align::HCentre => (mid_x - cx, 0.0),
                        Align::Right => (all.max_x - b.max_x, 0.0),
                        Align::Top => (0.0, all.min_y - b.min_y),
                        Align::VCentre => (0.0, mid_y - cy),
                        Align::Bottom => (0.0, all.max_y - b.max_y),
                        Align::Centre => (mid_x - cx, mid_y - cy),
                        Align::DistributeH | Align::DistributeV => unreachable!("handled above"),
                    };
                    self.shift(&u.ids, dx, dy);
                }
            }
        }
    }

    /// Whether any of `ids` is position-locked (spec 34).
    pub fn position_locked(&self, ids: &[u64]) -> bool {
        self.shapes.iter().any(|s| ids.contains(&s.id) && s.lock_position)
    }

    /// Moves one Align or Distribute unit; a unit holding a position-locked shape is measured but stays.
    fn shift(&mut self, ids: &[u64], dx: f32, dy: f32) {
        if self.position_locked(ids) {
            return;
        }
        for s in self.shapes.iter_mut().filter(|s| ids.contains(&s.id)) {
            s.x_in += dx;
            s.y_in += dy;
        }
    }

    fn distribute(&mut self, units: &[Unit], op: Align) {
        let horizontal = matches!(op, Align::DistributeH);
        let centre = |b: &Aabb| if horizontal { b.centre().0 } else { b.centre().1 };
        let mut order: Vec<&Unit> = units.iter().collect();
        order.sort_by(|a, b| centre(&a.bounds).total_cmp(&centre(&b.bounds)));
        let (first, last) = (centre(&order[0].bounds), centre(&order[order.len() - 1].bounds));
        let step = (last - first) / (order.len() - 1) as f32;
        for (i, u) in order.iter().enumerate() {
            let delta = first + step * i as f32 - centre(&u.bounds);
            let (dx, dy) = if horizontal { (delta, 0.0) } else { (0.0, delta) };
            self.shift(&u.ids, dx, dy);
        }
    }

    pub(crate) fn next_free_slot(&self, size_in: f32) -> (f32, f32) {
        let slot = |i: usize| {
            let (col, row) = (i % SLOTS_PER_ROW, i / SLOTS_PER_ROW);
            (SLOT_ORIGIN_IN + col as f32 * SLOT_STEP_IN, SLOT_ORIGIN_IN + row as f32 * SLOT_STEP_IN)
        };
        let free = |(x, y): (f32, f32)| !self.shapes.iter().any(|s| overlaps(s, x, y, size_in));
        (0..SLOTS_PER_ROW * SLOTS_PER_ROW).map(slot).find(|&p| free(p)).unwrap_or((SLOT_ORIGIN_IN, SLOT_ORIGIN_IN))
    }
}

fn overlaps(s: &Shape, x: f32, y: f32, size: f32) -> bool {
    s.x_in < x + size && x < s.x_in + s.width_in && s.y_in < y + size && y < s.y_in + s.height_in
}

/// Undo and redo over whole-design snapshots. A change made while the pointer stays down
/// (a drag, a slider scrub) folds into the entry its gesture opened. At most [`History::CAP`] steps
/// are kept; the oldest goes first.
#[derive(Default)]
pub struct History {
    past: std::collections::VecDeque<Design>,
    future: Vec<Design>,
    gesture_open: bool,
}

impl History {
    pub const CAP: usize = 200;

    pub fn observe(&mut self, before: Design, after: &Design, pointer_down: bool) {
        let changed = before != *after;
        if changed && !self.gesture_open {
            if self.past.len() == Self::CAP {
                self.past.pop_front();
            }
            self.past.push_back(before);
            self.future.clear();
        }
        self.gesture_open = pointer_down && (self.gesture_open || changed);
    }

    /// How many undo steps are stored; the debug log compares it to tell whether `observe` recorded one.
    pub fn depth(&self) -> usize {
        self.past.len()
    }

    /// Whether a drag is still adding to the last undo step.
    pub fn gesture_open(&self) -> bool {
        self.gesture_open
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Undo and Redo end any open gesture, so what the pointer does next starts a step of its own.
    pub fn undo(&mut self, design: &mut Design) {
        self.gesture_open = false;
        if let Some(prev) = self.past.pop_back() {
            self.future.push(std::mem::replace(design, prev));
        }
    }

    pub fn redo(&mut self, design: &mut Design) {
        self.gesture_open = false;
        if let Some(next) = self.future.pop() {
            self.past.push_back(std::mem::replace(design, next));
        }
    }
}

/// `contours` (a piece's, in its shape's local frame) in points; placement on the mat travels in the
/// `Matrix` transform.
pub fn to_svg_path_d(contours: &[Contour]) -> String {
    let pt = |(x, y): Pt| format!("{} {}", fmt_pt(x * POINTS_PER_INCH), fmt_pt(y * POINTS_PER_INCH));
    let contour = |c: &Contour| {
        let mut d = format!("M{}", pt(c.start));
        for seg in &c.segs {
            match *seg {
                Seg::Line(p) => d += &format!(" L{}", pt(p)),
                Seg::Cubic(a, b, p) => d += &format!(" C{} {} {}", pt(a), pt(b), pt(p)),
            }
        }
        if c.closed {
            d += " Z";
        }
        d
    };
    contours.iter().map(contour).collect::<Vec<_>>().join(" ")
}

fn fmt_pt(v: f32) -> String {
    let rounded = (v * 1000.0).round() / 1000.0;
    format!("{}", rounded + 0.0)
}

/// What [`Design::add_imported_tree`] created: the top-level ids (for selection and paste capture),
/// and the leaves that came from strokes with their widths, which the import notice re-operations or
/// outlines.
#[derive(Default)]
pub struct Imported {
    pub roots: Vec<u64>,
    pub strokes: Vec<(u64, f32)>,
}

/// Builds one imported node and its descendants into `out` (depth first, a container directly before
/// its children, so the container invariants hold), allocating ids from `next`. Returns the node's id.
fn build_import_node(node: crate::svg_import::Node, parent: Option<u64>, next: &mut u64, out: &mut Vec<Shape>, added: &mut Imported) -> u64 {
    use crate::svg_import::Node;
    match node {
        Node::Leaf(imp) => {
            let id = *next;
            *next += 1;
            if imp.from_stroke {
                added.strokes.push((id, imp.stroke_in));
            }
            out.push(Shape {
                x_in: imp.x_in,
                y_in: imp.y_in,
                width_in: imp.view_box.0.max(0.01),
                height_in: imp.view_box.1.max(0.01),
                fill: imp.fill,
                operation: imp.operation,
                pen: imp.pen,
                name: imp.name,
                parent,
                ..Shape::new(id, ShapeKind::Imported { contours: imp.contours, view_box: imp.view_box })
            });
            id
        }
        Node::Group { name, children } => {
            let id = *next;
            *next += 1;
            let at = out.len();
            for child in children {
                build_import_node(child, Some(id), next, out, added);
            }
            // Hug the leaf descendants, not the nested frames, so padding does not compound per level.
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for s in out[at..].iter().filter(|s| s.container.is_none()) {
                x0 = x0.min(s.x_in);
                y0 = y0.min(s.y_in);
                x1 = x1.max(s.x_in + s.width_in);
                y1 = y1.max(s.y_in + s.height_in);
            }
            let pad = crate::containers::DEFAULT_PADDING_IN;
            let kind = ShapeKind::Rectangle;
            let fill = kind.default_fill();
            let frame = Shape {
                x_in: x0 - pad,
                y_in: y0 - pad,
                width_in: (x1 - x0) + 2.0 * pad,
                height_in: (y1 - y0) + 2.0 * pad,
                fill,
                operation: Operation::Guide,
                pen: crate::pens::Pen::nearest(fill, Default::default()),
                container: Some(crate::containers::ContainerRole { padding_in: pad, layout: None }),
                parent,
                name,
                ..Shape::new(id, kind)
            };
            out.insert(at, frame);
            id
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path Make sends for `shape`'s body piece.
    fn to_svg_path_d(shape: &Shape) -> String {
        super::to_svg_path_d(&shape.contours())
    }

    fn shape(kind: ShapeKind) -> Shape {
        let mut d = Design::default();
        d.add_shape(kind);
        d.shapes.remove(0)
    }

    fn placed(id: u64, x: f32, y: f32) -> Shape {
        Shape { id, x_in: x, y_in: y, ..shape(ShapeKind::Square) }
    }

    fn ids(d: &Design) -> Vec<u64> {
        d.shapes.iter().map(|s| s.id).collect()
    }

    #[test]
    fn imports_svg_paths_as_scaled_shapes() {
        // Two rects: 96x48 px (1 x 0.5 in) and a 48x48 px (0.5 in) square offset to the right.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="0" y="0" width="96" height="48" fill="#ff0000"/>
            <rect x="120" y="0" width="48" height="48" fill="#0000ff"/>
        </svg>"##;
        let imports = crate::shape_library::import_svg(svg).expect("parses");
        let mut design = Design::default();
        let ids = design.add_imported(imports);
        assert_eq!(ids.len(), 2);
        assert_eq!(design.shapes.len(), 2);

        let first = &design.shapes[0];
        assert!((first.width_in - 1.0).abs() < 1e-3 && (first.height_in - 0.5).abs() < 1e-3, "{} x {}", first.width_in, first.height_in);
        assert_eq!(first.fill, Color32::from_rgb(0xff, 0, 0));
        let d = to_svg_path_d(first);
        assert!(d.starts_with('M') && d.contains("72") && d.contains("36"), "scaled path: {d}");
        // The second rect keeps its x offset of 120 px = 1.25 in.
        assert!((design.shapes[1].x_in - 1.25).abs() < 1e-3, "x {}", design.shapes[1].x_in);
    }

    #[test]
    fn free_shapes_include_the_builtins_with_valid_paths() {
        let free = ShapeKind::free();
        assert!(free.len() >= 40, "only {} free shapes", free.len());
        for kind in &free {
            let mut d = Design::default();
            let (w, h) = kind.default_size_in();
            assert!(w.max(h) > 0.0, "{:?} zero size", kind);
            d.add_shape(kind.clone());
            let path = to_svg_path_d(&d.shapes[0]);
            assert!(path.starts_with('M'), "{:?}: {path}", kind);
        }
        // The ring is a two-contour shape, so Contour is reachable from a built-in.
        let ring = free.iter().find(|k| k.label() == "Ring").expect("ring exists");
        let mut d = Design::default();
        d.add_shape(ring.clone());
        assert_eq!(d.shapes[0].contour_count(), 2);
    }

    #[test]
    fn open_and_closed_paths_emit_the_right_d() {
        let open = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((1.0, 0.0)), Seg::Cubic((1.5, 0.0), (2.0, 0.5), (2.0, 1.0))], closed: false };
        let mut d = Design::default();
        let id = d.add_path(open, Color32::BLACK, Operation::Pen).unwrap();
        let shape = d.shape_mut(id).unwrap().clone();
        assert_eq!(shape.operation, Operation::Pen);
        let path = to_svg_path_d(&shape);
        assert!(path.starts_with('M') && path.contains(" L") && path.contains(" C"), "{path}");
        assert!(!path.ends_with('Z'), "open path must not close: {path}");

        let closed = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((2.0, 0.0)), Seg::Line((2.0, 2.0)), Seg::Line((0.0, 2.0))], closed: true };
        let mut d2 = Design::default();
        let cid = d2.add_path(closed, Color32::BLACK, Operation::BasicCut).unwrap();
        assert!(to_svg_path_d(d2.shape_mut(cid).unwrap()).ends_with('Z'));
    }

    #[test]
    fn text_becomes_closed_contours_and_valid_make_geometry() {
        let mut d = Design::default();
        let id = d.add_text("Hi".into(), crate::text::FontRef::Inter, 1.0, Color32::BLACK, 2.0, 2.0);
        let shape = d.shape_mut(id).unwrap().clone();
        assert_eq!(shape.operation, Operation::Pen);
        assert!(shape.width_in > 0.3 && shape.height_in > 0.3, "{} x {}", shape.width_in, shape.height_in);
        let path = to_svg_path_d(&shape);
        assert!(path.starts_with('M') && path.ends_with('Z') && path.contains(" C"), "{path}");
        // Editing the string re-fits and changes the outline.
        let before = shape.kind.clone();
        if let ShapeKind::Text { content, .. } = &mut d.shape_mut(id).unwrap().kind {
            *content = "Hello".into();
        }
        d.refit_text(id, &before);
        assert_ne!(to_svg_path_d(d.shape_mut(id).unwrap()), path);
    }

    #[test]
    fn a_project_saved_with_three_field_text_loads_with_the_defaults() {
        let saved = r#"{"shapes":[{"id":1,"kind":{"Text":{"content":"Hi","font":"SpaceGrotesk","size_in":1.5}},"x_in":2.0,"y_in":2.0,"width_in":1.2,"height_in":1.1,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"Pen","hidden":false,"contour_hidden":[]}]}"#;
        let loaded: Design = serde_json::from_str(saved).expect("old projects load");
        let mut expected = Design::default();
        expected.add_text("Hi".into(), crate::text::FontRef::SpaceGrotesk, 1.5, Color32::BLACK, 2.0, 2.0);
        assert_eq!(loaded.shapes[0].kind, expected.shapes[0].kind);
    }

    #[test]
    fn editing_resized_text_keeps_its_scale() {
        let mut d = Design::default();
        let id = d.add_text("Hi".into(), crate::text::FontRef::Inter, 1.0, Color32::BLACK, 2.0, 2.0);
        let (w, h) = (d.shape_mut(id).unwrap().width_in, d.shape_mut(id).unwrap().height_in);
        d.shape_mut(id).unwrap().width_in *= 2.0;
        d.shape_mut(id).unwrap().height_in *= 3.0;
        let before = d.shape_mut(id).unwrap().kind.clone();
        if let ShapeKind::Text { content, .. } = &mut d.shape_mut(id).unwrap().kind {
            *content = "Hi\nHi".into();
        }
        d.refit_text(id, &before);
        let s = d.shape_mut(id).unwrap();
        assert!((s.width_in - 2.0 * w).abs() < 1e-4, "same widest line, same doubled width");
        assert!((s.height_in - 6.0 * h).abs() < 1e-4, "two lines at triple height");
    }

    /// Expand on what an edit left in hand.
    fn expand(d: &mut Design, held: &[u64]) -> Vec<u64> {
        d.apply(&crate::edit::Edit::Expand, held).unwrap()
    }

    #[test]
    fn weld_makes_a_live_group_that_expands_into_one_shape() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 1.0, 0.0)], ..Default::default() };
        let group = d.combine(&[1, 2], Combine::Weld);
        assert_eq!(ids(&d), [group[0], 1, 2], "the inputs stay, inside the group");
        assert_eq!(d.shapes[0].kind, ShapeKind::Compound(CompoundOp::Boolean(Combine::Weld)));
        assert!(d.shapes[1..].iter().all(|s| s.parent == Some(group[0])));
        let new = expand(&mut d, &group);
        assert_eq!(d.shapes.len(), 1);
        assert_eq!(new, [d.shapes[0].id]);
        assert!(matches!(d.shapes[0].kind, ShapeKind::Imported { .. }));
        let (w, h) = (d.shapes[0].width_in, d.shapes[0].height_in);
        assert!((w - 3.0).abs() < 1e-2 && (h - 2.0).abs() < 1e-2, "{w} x {h}");
    }

    #[test]
    fn slicing_overlapping_squares_gives_three_pieces_that_tile_the_union() {
        let inputs = vec![placed(1, 0.0, 0.0), Shape { fill: Color32::RED, ..placed(2, 1.0, 1.0) }];
        let union = crate::geometry::even_odd_area(&crate::geometry::boolean(
            &inputs.iter().map(|s| s.world_rings(crate::geometry::FLATTEN_TOL_IN)).collect::<Vec<_>>(),
            crate::geometry::Boolean::Weld,
        ));
        let mut d = Design { shapes: inputs, ..Default::default() };
        let group = d.combine(&[1, 2], Combine::Slice);
        let new = expand(&mut d, &group);
        assert_eq!(new, ids(&d), "the inputs are replaced by the pieces");
        assert_eq!(new.len(), 3);
        let sum: f32 = d.shapes.iter().map(|s| crate::geometry::even_odd_area(&s.world_rings(crate::geometry::FLATTEN_TOL_IN))).sum();
        assert!((sum - union).abs() < 1e-2, "pieces {sum} vs union {union}");
        let fills: Vec<Color32> = d.shapes.iter().map(|s| s.fill).collect();
        assert_eq!(fills, [Color32::BLACK, Color32::RED, Color32::RED], "the overlap takes the front style");
    }

    #[test]
    fn slicing_disjoint_squares_gives_two_pieces() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 5.0, 0.0)], ..Default::default() };
        let group = d.combine(&[1, 2], Combine::Slice);
        assert_eq!(expand(&mut d, &group).len(), 2);
        assert_eq!(d.shapes.len(), 2);
    }

    #[test]
    fn an_attached_set_is_one_unit_and_group_members_are_one_each() {
        let mut d = Design { shapes: (1..=4).map(|i| placed(i, i as f32, 0.0)).collect(), ..Default::default() };
        d.group(&[1, 2]).unwrap();
        d.attach(&[3, 4]).unwrap();
        let units = |d: &Design, ids: &[u64]| d.units_by(ids, UnitKey::Attach).into_iter().map(|u| u.ids).collect::<Vec<_>>();
        assert_eq!(units(&d, &[1, 2]), [vec![1], vec![2]]);
        assert_eq!(units(&d, &[1, 3, 4]), [vec![1], vec![3, 4]]);
        assert!(d.combine(&[1, 2, 3], Combine::Slice).is_empty(), "three units do not slice");
        assert_eq!(d.shapes.len(), 4);
    }

    fn holed_square() -> Shape {
        let outer = Contour { start: (0.0, 0.0), segs: [(4.0, 0.0), (4.0, 4.0), (0.0, 4.0)].iter().map(|&p| Seg::Line(p)).collect(), closed: true };
        let inner = Contour { start: (1.0, 1.0), segs: [(3.0, 1.0), (3.0, 3.0), (1.0, 3.0)].iter().map(|&p| Seg::Line(p)).collect(), closed: true };
        Shape { kind: ShapeKind::Imported { contours: vec![outer, inner], view_box: (4.0, 4.0) }, width_in: 4.0, height_in: 4.0, ..placed(1, 0.0, 0.0) }
    }

    #[test]
    fn hiding_a_contour_changes_the_path_and_area() {
        let mut shape = holed_square();
        assert_eq!(shape.contour_count(), 2);
        let full_d = to_svg_path_d(&shape);
        let full_area = crate::geometry::even_odd_area(&shape.world_rings(0.01));
        assert!((full_area - 12.0).abs() < 1e-2, "with hole {full_area}");

        shape.contours.hidden = vec![false, true];
        let hidden_d = to_svg_path_d(&shape);
        let hidden_area = crate::geometry::even_odd_area(&shape.world_rings(0.01));
        assert_ne!(full_d, hidden_d);
        assert!((hidden_area - 16.0).abs() < 1e-2, "no hole {hidden_area}");
    }

    #[test]
    fn an_expanded_offset_adds_a_larger_shape_behind() {
        let mut d = Design { shapes: vec![placed(1, 1.0, 1.0)], ..Default::default() };
        let effect = crate::effects::Effect::Offset(crate::effects::Offset {
            distance_in: 0.25,
            direction: crate::effects::Direction::Outset,
            join: crate::geometry::Join::Round,
            style: None,
        });
        let live = d.apply(&crate::edit::Edit::AddEffect { effect, reach: crate::effects::Reach::Together }, &[1]).unwrap();
        let new = expand(&mut d, &live)[0];
        assert_eq!(d.shapes.len(), 2);
        assert_eq!(d.shapes[0].id, new, "offset sits behind the original");
        assert!(d.shapes[0].width_in > 2.4 && d.shapes[0].width_in < 2.6, "w {}", d.shapes[0].width_in);
    }

    #[test]
    fn an_expanded_sticker_backs_the_selection_with_a_white_border() {
        let square = Shape { width_in: 1.0, height_in: 1.0, ..placed(1, 1.0, 1.0) };
        let mut d = Design { shapes: vec![placed(9, 5.0, 5.0), square.clone()], ..Default::default() };
        let id = d.expanded_sticker(&[1], 0.125, crate::effects::Corners::Sharp).expect("makes a sticker");
        assert_eq!(ids(&d), vec![9, id], "the sticker takes the square's place");
        let sticker = &d.shapes[1];
        let ShapeKind::Flattened { parts, .. } = &sticker.kind else { panic!("not flattened: {:?}", sticker.kind) };
        assert_eq!((parts[0].fill, parts[0].operation), (Color32::WHITE, Operation::PrintThenCut), "white backing first");
        assert_eq!(parts[1].fill, square.fill);
        assert_eq!((sticker.fill, sticker.pen), (square.fill, square.pen), "the sticker shows its artwork's colour and pen");
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(sticker.width_in, 1.25) && near(sticker.height_in, 1.25), "frame {} x {}", sticker.width_in, sticker.height_in);
        let (min_x, min_y, max_x, max_y) = sticker.world_bounds();
        assert!(near(max_x - min_x, 1.25) && near(max_y - min_y, 1.25), "cut outline {:?}", (min_x, min_y, max_x, max_y));
        assert!(near(min_x, 0.875) && near(min_y, 0.875), "the border is even on every side");
    }

    #[test]
    fn arrange_reorders_the_z_order() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 1.0, 0.0), placed(3, 2.0, 0.0)], ..Default::default() };
        let (a, b, c) = (d.shapes[0].id, d.shapes[1].id, d.shapes[2].id);
        d.arrange(&[a], Arrange::ToFront);
        assert_eq!(ids(&d), vec![b, c, a]);
        d.arrange(&[a], Arrange::ToBack);
        assert_eq!(ids(&d), vec![a, b, c]);
        d.arrange(&[a], Arrange::Forward);
        assert_eq!(ids(&d), vec![b, a, c]);
    }

    #[test]
    fn align_left_shares_one_min_x() {
        let mut d = Design { shapes: vec![placed(1, 1.0, 0.0), placed(2, 3.0, 5.0)], ..Default::default() };
        let all = ids(&d);
        d.align(&all, Align::Left);
        assert!((d.shapes[0].world_bounds().0 - d.shapes[1].world_bounds().0).abs() < 1e-5);
        assert!((d.shapes[0].world_bounds().0 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn distribute_h_evens_the_centres() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 1.3, 0.0), placed(3, 10.0, 0.0)], ..Default::default() };
        let all = ids(&d);
        d.align(&all, Align::DistributeH);
        let cx = |s: &Shape| s.x_in + s.width_in / 2.0;
        let (a, b, c) = (cx(&d.shapes[0]), cx(&d.shapes[1]), cx(&d.shapes[2]));
        assert!(((b - a) - (c - b)).abs() < 1e-4, "gaps {} {}", b - a, c - b);
    }

    #[test]
    fn centre_stacks_every_centre_on_one_point() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 4.0, 1.0), placed(3, 9.0, 7.0)], ..Default::default() };
        d.shapes[2].width_in = 5.0;
        let all = ids(&d);
        d.align(&all, Align::Centre);
        let centre = |s: &Shape| {
            let b = s.world_bounds();
            ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
        };
        let (a, b, c) = (centre(&d.shapes[0]), centre(&d.shapes[1]), centre(&d.shapes[2]));
        for p in [b, c] {
            assert!((p.0 - a.0).abs() < 1e-4 && (p.1 - a.1).abs() < 1e-4, "{a:?} vs {p:?}");
        }
    }

    #[test]
    fn a_group_aligns_as_one_unit_and_keeps_its_internal_offset() {
        let mut d = Design { shapes: vec![placed(1, 3.0, 0.0), placed(2, 5.0, 2.0), placed(3, 1.0, 6.0)], ..Default::default() };
        let (a, b, lone) = (d.shapes[0].id, d.shapes[1].id, d.shapes[2].id);
        d.group(&[a, b]).expect("grouped");
        let at = |d: &Design, id: u64| d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let gap = |d: &Design| (at(d, b).x_in - at(d, a).x_in, at(d, b).y_in - at(d, a).y_in);
        let before = gap(&d);
        assert_eq!(d.units_by(&d.expand(&[a, b, lone]), UnitKey::AnyLink).len(), 2);
        d.align(&[a, b, lone], Align::Left);
        assert_eq!(gap(&d), before, "members move by the same delta");
        assert!((at(&d, a).world_bounds().0 - 1.0).abs() < 1e-5, "the group's left edge meets the lone shape's");
        assert!((at(&d, lone).x_in - 1.0).abs() < 1e-5, "the lone shape was already leftmost");
    }

    #[test]
    fn align_and_distribute_need_enough_units() {
        let mut d = Design { shapes: vec![placed(1, 0.0, 0.0), placed(2, 1.3, 0.0), placed(3, 10.0, 0.0)], ..Default::default() };
        let all = ids(&d);
        d.group(&all[..2]).expect("grouped");
        let before = d.clone();
        d.align(&all, Align::DistributeH);
        assert_eq!(d, before, "two units cannot be distributed");
        d.align(&all[..2], Align::Left);
        assert_eq!(d, before, "one group alone is one unit");
    }

    #[test]
    fn every_free_shape_emits_its_library_path_at_insert_size() {
        for kind in ShapeKind::PRESETS {
            let d = to_svg_path_d(&shape(kind.clone()));
            assert!(d.starts_with('M'), "{kind:?}: {d}");
            assert_eq!(d.ends_with('Z'), kind != ShapeKind::Line, "{kind:?}: {d}");
        }
        assert_eq!(to_svg_path_d(&shape(ShapeKind::Line)), "M0 0 L0 180");
        assert_eq!(to_svg_path_d(&shape(ShapeKind::Square)), "M0 0 L144 0 L144 144 L0 144 Z");
        let native = Shape { width_in: 2.0, height_in: 124.7 / POINTS_PER_INCH, ..shape(ShapeKind::Triangle) };
        assert_eq!(to_svg_path_d(&native), "M72 0 L144 124.7 L0 124.7 Z");
        let circle = to_svg_path_d(&shape(ShapeKind::Circle));
        assert!(circle.starts_with("M72 0 C") && circle.matches(" C").count() >= 4, "{circle}");
    }

    #[test]
    fn insert_sizes_follow_the_default_heights() {
        assert_eq!(ShapeKind::Circle.default_size_in(), (2.0, 2.0));
        let (w, h) = ShapeKind::Rectangle.default_size_in();
        assert!((h - 1.6667).abs() < 1e-4 && (w - 1.6667 * 179.9 / 119.9).abs() < 1e-3, "{w} x {h}");
        assert_eq!(ShapeKind::Line.default_size_in(), (0.0, 2.5));
        assert_eq!(shape(ShapeKind::Line).operation, Operation::Score);
        assert_eq!(shape(ShapeKind::Square).fill, Color32::BLACK);
    }

    #[test]
    fn hit_testing_follows_the_outline_not_the_box() {
        let star = shape(ShapeKind::Star);
        let (x, y) = (star.x_in, star.y_in);
        assert!(star.contains_in(x + 1.0, y + 1.0));
        assert!(!star.contains_in(x + 0.05, y + 0.05));
    }

    #[test]
    fn text_hit_tests_its_whole_box_not_just_the_glyphs() {
        let mut design = Design::default();
        let id = design.add_text("Text".into(), crate::text::FontRef::Inter, 1.0, Color32::BLACK, 2.0, 2.0);
        let mut text = design.shapes.iter().find(|s| s.id == id).unwrap().clone();
        text.rotation_deg = 30.0;
        let (cx, cy) = text.centre_in();
        let (sin, cos) = 30f32.to_radians().sin_cos();
        let misses: Vec<(f32, f32)> = (1..20)
            .flat_map(|i| (1..20).map(move |j| ((i as f32 / 20.0 - 0.5), (j as f32 / 20.0 - 0.5))))
            .map(|(u, v)| (u * text.width_in, v * text.height_in))
            .map(|(lx, ly)| (cx + lx * cos - ly * sin, cy + lx * sin + ly * cos))
            .filter(|&(x, y)| !text.contains_in(x, y))
            .collect();
        assert!(misses.is_empty(), "{} points inside the rotated text box miss it, e.g. {:?}", misses.len(), misses.first());
        assert!(!text.contains_in(cx + text.width_in, cy + text.height_in));
    }

    #[test]
    fn history_folds_a_drag_into_one_entry() {
        let mut design = Design::default();
        let mut history = History::default();
        let before = design.clone();
        design.add_shape(ShapeKind::Square);
        history.observe(before, &design, false);
        for step in 0..5 {
            let before = design.clone();
            design.shapes[0].x_in += 1.0;
            history.observe(before, &design, step < 4);
        }
        history.observe(design.clone(), &design, false);
        history.undo(&mut design);
        assert_eq!(design.shapes[0].x_in, 0.5);
        history.undo(&mut design);
        assert!(design.shapes.is_empty());
        history.redo(&mut design);
        assert_eq!(design.shapes.len(), 1);
    }

    #[test]
    fn undo_mid_gesture_ends_it_so_the_next_change_is_its_own_step() {
        let mut design = Design::default();
        let mut history = History::default();
        let before = design.clone();
        design.add_shape(ShapeKind::Square);
        history.observe(before, &design, false);
        let x0 = design.shapes[0].x_in;
        let before = design.clone();
        design.shapes[0].x_in += 1.0;
        history.observe(before, &design, true);
        history.undo(&mut design);
        assert_eq!(design.shapes[0].x_in, x0, "the drag so far is undone");
        let before = design.clone();
        design.shapes[0].x_in += 2.0;
        history.observe(before, &design, true);
        assert!(!history.can_redo(), "a new change clears Redo");
        history.undo(&mut design);
        assert_eq!(design.shapes[0].x_in, x0, "the change after Undo is a step of its own");
        history.undo(&mut design);
        assert!(design.shapes.is_empty());
    }

    #[test]
    fn history_keeps_at_most_200_steps_dropping_the_oldest() {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Square);
        let mut history = History::default();
        for _ in 0..History::CAP + 50 {
            let before = design.clone();
            design.shapes[0].x_in += 1.0;
            history.observe(before, &design, false);
        }
        assert_eq!(history.depth(), History::CAP);
        let last = design.shapes[0].x_in;
        while history.can_undo() {
            history.undo(&mut design);
        }
        assert_eq!(design.shapes[0].x_in, last - History::CAP as f32, "the 50 oldest steps were dropped");
    }
}

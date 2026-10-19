//! SVG import: turns an SVG file into placed shapes with their operation and pen, and reports what
//! was converted or lost. A stroke-only path becomes a Pen or Cut line (nearest of the 24 pens, pen
//! size nearest the stroke width); a gradient or faded fill keeps its mean or solid colour and is
//! counted; an invisible path is dropped and counted. The converter is pure: it reads bytes and
//! returns geometry plus an [`ImportReport`] the UI reports verbatim.

use egui::Color32;
use usvg::TreeParsing;
use usvg::tiny_skia_path::PathSegment;

use crate::design::{Contour, Operation, Pt, Seg};
use crate::pens::{Pen, PenSize};

/// SVG user units are px at 96 dpi; one inch is 96 px.
const PX_TO_IN: f32 = 1.0 / 96.0;
/// A stroke at or under this width in inches is a laser cut line by Inkscape and laser convention,
/// not a drawn pen stroke.
const HAIRLINE_IN: f32 = 0.01;

/// One path from an imported SVG, placed on the canvas in inches, with its geometry in a local
/// frame starting at the origin and the operation and pen it should carry.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Import {
    pub x_in: f32,
    pub y_in: f32,
    pub view_box: Pt,
    pub contours: Vec<Contour>,
    pub fill: Color32,
    pub operation: Operation,
    pub pen: Pen,
    /// Came from a stroke with no fill, so the import notice may re-operation it in one step.
    pub from_stroke: bool,
    /// The stroke width in inches, kept so Outline strokes can expand it to a filled ring.
    pub stroke_in: f32,
    /// The path's SVG `id`, which names its layer row. `None` when the element had none.
    pub name: Option<String>,
}

/// A node in the imported tree. A `<g>` becomes a [`Node::Group`] (a Kerf container, named by its
/// SVG `id`) holding its children, nested as the file nests; a drawable path becomes a [`Node::Leaf`].
/// A group of one leaf collapses to the leaf, which takes the group's name if it had none.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Group { name: Option<String>, children: Vec<Node> },
    Leaf(Import),
}

fn collect_leaves(nodes: &[Node], out: &mut Vec<Import>) {
    for node in nodes {
        match node {
            Node::Leaf(imp) => out.push(imp.clone()),
            Node::Group { children, .. } => collect_leaves(children, out),
        }
    }
}

fn leaf_count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            Node::Leaf(_) => 1,
            Node::Group { children, .. } => leaf_count(children),
        })
        .sum()
}

/// What an import converted and what it dropped. The UI reports these counts and never invents any.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Drawable paths that came in.
    pub paths: usize,
    /// Stroke-only paths that became Pen or Cut lines.
    pub strokes: usize,
    /// Of those, hairlines (a laser cut line).
    pub hairlines: usize,
    /// Filled-and-stroked paths whose stroke was dropped.
    pub strokes_on_filled: usize,
    /// Gradient or pattern fills reduced to a mean or solid colour.
    pub gradients: usize,
    /// Fills with opacity under 1 that cut at full strength.
    pub faded: usize,
    /// Invisible paths (no fill and no stroke) that were dropped.
    pub invisible: usize,
    /// Dash arrays that were dropped.
    pub dashes: usize,
}

impl ImportReport {
    /// Every stroke that came in is a hairline, so the notice defaults Treat hairlines as to Cut.
    pub fn all_hairline(&self) -> bool {
        self.strokes > 0 && self.strokes == self.hairlines
    }

    /// A one-line account of what came in and what was lost, as the import notice reads it out.
    pub fn summary(&self) -> String {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let mut lost = Vec::new();
        if self.gradients > 0 {
            lost.push(format!("{} gradient{} (mean colour used)", self.gradients, s(self.gradients)));
        }
        if self.strokes_on_filled > 0 {
            lost.push(format!("{} stroke{} on filled shapes", self.strokes_on_filled, s(self.strokes_on_filled)));
        }
        if self.faded > 0 {
            lost.push(format!("{} faded fill{}", self.faded, s(self.faded)));
        }
        if self.dashes > 0 {
            lost.push(format!("{} dash array{}", self.dashes, s(self.dashes)));
        }
        if self.invisible > 0 {
            lost.push(format!("{} invisible path{}", self.invisible, s(self.invisible)));
        }
        let mut msg = format!("Imported {} path{}.", self.paths, s(self.paths));
        if !lost.is_empty() {
            msg.push_str(" Dropped: ");
            msg.push_str(&lost.join(", "));
            msg.push('.');
        }
        msg
    }
}

/// The imported tree plus the report, returned together so the UI reports the counts and the pure
/// logic owns them.
pub struct Converted {
    /// The top-level nodes, nested as the file nests.
    pub roots: Vec<Node>,
    pub report: ImportReport,
}

impl Converted {
    /// Every leaf path, depth first, flattened out of the group tree. For callers that want only the
    /// geometry and no structure (Paste today).
    pub fn leaves(&self) -> Vec<Import> {
        let mut out = Vec::new();
        collect_leaves(&self.roots, &mut out);
        out
    }
}

/// Parses an SVG into a tree of placed paths and groups, converting strokes to operations and
/// reducing gradients, faded fills and invisible paths per the rules in the module docs. Each `<g>`
/// becomes a group named by its SVG `id`, nested as the file nests.
pub fn convert(bytes: &[u8]) -> Result<Converted, String> {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    // usvg keeps the viewBox out of `abs_transform`; without it a file whose viewBox is not in px
    // imports at the wrong scale.
    let view = usvg::utils::view_box_to_transform(tree.view_box.rect, tree.view_box.aspect, tree.size);
    let mut report = ImportReport::default();
    let roots = convert_nodes(tree.root.children(), view, &mut report);
    if leaf_count(&roots) == 0 {
        return Err("no drawable paths found in the SVG".into());
    }
    Ok(Converted { roots, report })
}

/// Converts a run of sibling SVG nodes into Kerf nodes, recursing into groups. Empty groups are
/// dropped; a group of one leaf collapses to the leaf.
fn convert_nodes(nodes: impl Iterator<Item = usvg::Node>, view: usvg::tiny_skia_path::Transform, report: &mut ImportReport) -> Vec<Node> {
    use usvg::NodeExt;
    let mut out = Vec::new();
    for node in nodes {
        enum Kind {
            Group(Option<String>),
            Path(Option<Import>),
            Other,
        }
        let kind = match &*node.borrow() {
            usvg::NodeKind::Group(g) => Kind::Group(non_empty(&g.id)),
            usvg::NodeKind::Path(path) => {
                let transform = node.abs_transform().post_concat(view);
                Kind::Path(convert_path(path, transform, report))
            }
            _ => Kind::Other,
        };
        match kind {
            Kind::Group(name) => {
                let children = convert_nodes(node.children(), view, report);
                if children.is_empty() {
                    continue;
                }
                if children.len() == 1 && matches!(children[0], Node::Leaf(_)) {
                    let Some(Node::Leaf(mut imp)) = children.into_iter().next() else { unreachable!() };
                    imp.name = imp.name.or(name);
                    out.push(Node::Leaf(imp));
                } else {
                    out.push(Node::Group { name, children });
                }
            }
            Kind::Path(Some(imp)) => out.push(Node::Leaf(imp)),
            Kind::Path(None) | Kind::Other => {}
        }
    }
    out
}

/// Converts one SVG path into an [`Import`], or `None` when it is invisible or has no geometry.
/// Updates the report with what it reduced or dropped.
fn convert_path(path: &usvg::Path, transform: usvg::tiny_skia_path::Transform, report: &mut ImportReport) -> Option<Import> {
    let data = path.data.as_ref().clone().transform(transform)?;
    let bounds = data.bounds();
    let (ox, oy) = (bounds.left() * PX_TO_IN, bounds.top() * PX_TO_IN);
    let at = |p: usvg::tiny_skia_path::Point| (p.x * PX_TO_IN - ox, p.y * PX_TO_IN - oy);
    let contours = contours_of(&data, at);
    if contours.is_empty() {
        return None;
    }

    let has_fill = path.fill.is_some();
    let has_stroke = path.stroke.is_some();
    if !has_fill && !has_stroke {
        report.invisible += 1;
        return None;
    }
    if let Some(stroke) = &path.stroke
        && stroke.dasharray.is_some()
    {
        report.dashes += 1;
    }

    let (fill, operation, pen, from_stroke, stroke_in) = if has_fill {
        // A filled shape keeps its fill; any stroke is dropped and counted.
        if has_stroke {
            report.strokes_on_filled += 1;
        }
        let fill = resolve_colour(&path.fill.as_ref().unwrap().paint, report);
        if path.fill.as_ref().unwrap().opacity.get() < 1.0 {
            report.faded += 1;
        }
        (fill, Operation::BasicCut, Pen::nearest(fill, PenSize::default()), false, 0.0)
    } else {
        // A stroke-only path becomes a pen or cut line.
        let stroke = path.stroke.as_ref().unwrap();
        let colour = resolve_colour(&stroke.paint, report);
        if stroke.opacity.get() < 1.0 {
            report.faded += 1;
        }
        let scale = (transform.sx * transform.sy - transform.kx * transform.ky).abs().sqrt();
        let width_in = stroke.width.get() * scale * PX_TO_IN;
        let closed = !contours.is_empty() && contours.iter().all(|c| c.closed);
        let hairline = width_in < HAIRLINE_IN || (is_black(colour) && closed);
        report.strokes += 1;
        if hairline {
            report.hairlines += 1;
        }
        let size = nearest_size(width_in);
        let op = if hairline { Operation::BasicCut } else { Operation::Pen };
        (colour, op, Pen::nearest(colour, size), true, width_in)
    };

    report.paths += 1;
    Some(Import {
        x_in: ox,
        y_in: oy,
        view_box: (bounds.width() * PX_TO_IN, bounds.height() * PX_TO_IN),
        contours,
        fill,
        operation,
        pen,
        from_stroke,
        stroke_in,
        name: non_empty(&path.id),
    })
}

/// A trimmed, non-empty SVG id as a layer name, else `None`.
fn non_empty(id: &str) -> Option<String> {
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

fn contours_of(data: &usvg::tiny_skia_path::Path, at: impl Fn(usvg::tiny_skia_path::Point) -> Pt) -> Vec<Contour> {
    let mut contours: Vec<Contour> = Vec::new();
    let mut last = (0.0, 0.0);
    for seg in data.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                last = at(p);
                contours.push(Contour { start: last, segs: Vec::new(), closed: false });
            }
            PathSegment::LineTo(p) => {
                last = at(p);
                push(&mut contours, Seg::Line(last));
            }
            PathSegment::QuadTo(c, p) => {
                let (q, end) = (at(c), at(p));
                let lerp = |a: Pt, b: Pt| (a.0 + (b.0 - a.0) * 2.0 / 3.0, a.1 + (b.1 - a.1) * 2.0 / 3.0);
                push(&mut contours, Seg::Cubic(lerp(last, q), lerp(end, q), end));
                last = end;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                last = at(p);
                push(&mut contours, Seg::Cubic(at(c1), at(c2), last));
            }
            PathSegment::Close => {
                if let Some(c) = contours.last_mut() {
                    c.closed = true;
                }
            }
        }
    }
    contours
}

fn push(contours: &mut [Contour], seg: Seg) {
    if let Some(c) = contours.last_mut() {
        c.segs.push(seg);
    }
}

/// A paint reduced to one colour: a solid colour as is, a gradient or pattern to its mean colour,
/// counted on the report.
fn resolve_colour(paint: &usvg::Paint, report: &mut ImportReport) -> Color32 {
    match paint {
        usvg::Paint::Color(c) => Color32::from_rgb(c.red, c.green, c.blue),
        usvg::Paint::LinearGradient(g) => {
            report.gradients += 1;
            mean_stops(&g.base.stops)
        }
        usvg::Paint::RadialGradient(g) => {
            report.gradients += 1;
            mean_stops(&g.base.stops)
        }
        usvg::Paint::Pattern(_) => {
            report.gradients += 1;
            Color32::BLACK
        }
    }
}

/// The mean of a gradient's stop colours, each channel averaged evenly.
fn mean_stops(stops: &[usvg::Stop]) -> Color32 {
    if stops.is_empty() {
        return Color32::BLACK;
    }
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for s in stops {
        r += s.color.red as u32;
        g += s.color.green as u32;
        b += s.color.blue as u32;
    }
    let n = stops.len() as u32;
    Color32::from_rgb((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

fn is_black(c: Color32) -> bool {
    c.r() < 16 && c.g() < 16 && c.b() < 16
}

/// The pen size whose nib is nearest the stroke width.
fn nearest_size(width_in: f32) -> PenSize {
    PenSize::ALL
        .into_iter()
        .min_by(|a, b| {
            let da = (a.nib_mm() / 25.4 - width_in).abs();
            let db = (b.nib_mm() / 25.4 - width_in).abs();
            da.total_cmp(&db)
        })
        .unwrap_or_default()
}

/// Expands a stroked path of the given width into a filled ring: the region between the path offset
/// outward and inward by half the width. Caps and joins round. For artwork that relies on thick
/// outlines; the import notice's Outline strokes option drives it.
pub fn outline_stroke(contours: &[Contour], width_in: f32) -> Vec<Contour> {
    use crate::geometry::{self, Boolean, Join};
    let rings: Vec<Vec<Pt>> = contours.iter().map(|c| geometry::flatten(c, 0.002)).collect();
    let half = width_in / 2.0;
    let outer = geometry::offset(&rings, half, Join::Round);
    let inner = geometry::offset(&rings, -half, Join::Round);
    let ring = if inner.is_empty() { outer } else { geometry::boolean(&[outer, inner], Boolean::Subtract) };
    ring.into_iter().filter(|r| r.len() >= 2).map(ring_to_contour).collect()
}

fn ring_to_contour(ring: Vec<Pt>) -> Contour {
    let start = ring[0];
    let segs = ring[1..].iter().map(|&p| Seg::Line(p)).collect();
    Contour { start, segs, closed: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Operation;

    fn svg(body: &str) -> Vec<u8> {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\" viewBox=\"0 0 100 100\">{body}</svg>").into_bytes()
    }

    #[test]
    fn stroke_only_red_04mm_is_a_red_fine_pen() {
        // 0.4 mm at 96 dpi is 1.5118 px.
        let c = convert(&svg(r##"<path d="M10 10 L90 10" fill="none" stroke="#d0202e" stroke-width="1.511811"/>"##)).unwrap();
        assert_eq!(c.leaves().len(), 1);
        let imp = c.leaves()[0].clone();
        assert!(imp.from_stroke);
        assert_eq!(imp.operation, Operation::Pen);
        assert_eq!(imp.pen.size, PenSize::Fine);
        assert_eq!(imp.pen.colour_name(), "Red");
        assert_eq!(c.report.strokes, 1);
        assert_eq!(c.report.hairlines, 0);
        assert!(!c.report.all_hairline());
    }

    #[test]
    fn thin_black_stroke_is_a_hairline_cut() {
        // 0.001 in is 0.096 px, under the hairline threshold.
        let c = convert(&svg(r##"<path d="M10 10 L90 90" fill="none" stroke="#000000" stroke-width="0.096"/>"##)).unwrap();
        let imp = c.leaves()[0].clone();
        assert_eq!(imp.operation, Operation::BasicCut);
        assert_eq!(c.report.strokes, 1);
        assert_eq!(c.report.hairlines, 1);
        assert!(c.report.all_hairline());
    }

    #[test]
    fn closed_black_stroke_is_a_hairline_regardless_of_width() {
        let c = convert(&svg(r##"<rect x="10" y="10" width="40" height="40" fill="none" stroke="#000000" stroke-width="3"/>"##)).unwrap();
        assert_eq!(c.leaves()[0].operation, Operation::BasicCut);
        assert_eq!(c.report.hairlines, 1);
    }

    #[test]
    fn gradient_fill_becomes_its_mean_colour_and_is_counted() {
        let body = r##"<defs><linearGradient id="g"><stop offset="0" stop-color="#000000"/><stop offset="1" stop-color="#ffffff"/></linearGradient></defs><rect x="10" y="10" width="40" height="40" fill="url(#g)"/>"##;
        let c = convert(&svg(body)).unwrap();
        assert_eq!(c.report.gradients, 1);
        let f = c.leaves()[0].fill;
        assert!((f.r() as i32 - 127).abs() <= 1, "mean grey, got {:?}", f);
    }

    #[test]
    fn invisible_path_is_dropped_and_counted() {
        let body = r##"<rect x="0" y="0" width="10" height="10" fill="#000000"/><path d="M0 0 L50 50" fill="none"/>"##;
        let c = convert(&svg(body)).unwrap();
        assert_eq!(c.report.paths, 1);
        assert_eq!(c.report.invisible, 1);
    }

    #[test]
    fn filled_and_stroked_keeps_the_fill_and_counts_the_stroke() {
        let c = convert(&svg(r##"<rect x="10" y="10" width="40" height="40" fill="#ff0000" stroke="#00ff00" stroke-width="2"/>"##)).unwrap();
        let imp = c.leaves()[0].clone();
        assert!(!imp.from_stroke);
        assert_eq!(imp.operation, Operation::BasicCut);
        assert_eq!(imp.fill, Color32::from_rgb(255, 0, 0));
        assert_eq!(c.report.strokes_on_filled, 1);
    }

    #[test]
    fn faded_fill_is_counted_and_cuts_at_full_strength() {
        let c = convert(&svg(r##"<rect x="10" y="10" width="40" height="40" fill="#000000" fill-opacity="0.2"/>"##)).unwrap();
        assert_eq!(c.report.faded, 1);
        assert_eq!(c.leaves()[0].fill, Color32::BLACK);
    }

    #[test]
    fn dash_arrays_are_counted() {
        let c = convert(&svg(r##"<path d="M10 10 L90 10" fill="none" stroke="#d0202e" stroke-width="1.5" stroke-dasharray="4 2"/>"##)).unwrap();
        assert_eq!(c.report.dashes, 1);
    }

    #[test]
    fn outline_stroke_rings_a_square_half_the_width_outside() {
        // A 1 in square (0..1), outlined at 0.1 in, should reach 0.05 in outside on every side.
        let square = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((1.0, 0.0)), Seg::Line((1.0, 1.0)), Seg::Line((0.0, 1.0))], closed: true };
        let ring = outline_stroke(&[square], 0.1);
        assert!(!ring.is_empty());
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in &ring {
            for p in std::iter::once(c.start).chain(c.segs.iter().map(|s| match *s {
                Seg::Line(p) | Seg::Cubic(_, _, p) => p,
            })) {
                min_x = min_x.min(p.0);
                min_y = min_y.min(p.1);
                max_x = max_x.max(p.0);
                max_y = max_y.max(p.1);
            }
        }
        assert!((min_x + 0.05).abs() < 0.01, "left edge {min_x}");
        assert!((min_y + 0.05).abs() < 0.01, "top edge {min_y}");
        assert!((max_x - 1.05).abs() < 0.01, "right edge {max_x}");
        assert!((max_y - 1.05).abs() < 0.01, "bottom edge {max_y}");
    }

    fn names(nodes: &[Node]) -> Vec<Option<&str>> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Group { name, .. } => name.as_deref(),
                Node::Leaf(imp) => imp.name.as_deref(),
            })
            .collect()
    }

    #[test]
    fn nested_groups_become_named_groups_keeping_every_id() {
        let body = concat!(
            r##"<g id="Background"><path id="sky" d="M0 0 L10 0 L10 10 Z" fill="#8888ff"/><path id="hill" d="M0 10 L10 10 L5 5 Z" fill="#88ff88"/></g>"##,
            r##"<g id="Foreground"><g id="tree"><path id="trunk" d="M4 4 L6 4 L6 8 L4 8 Z" fill="#884400"/><path id="leaves" d="M2 2 L8 2 L5 6 Z" fill="#00aa00"/></g></g>"##,
        );
        let c = convert(&svg(body)).unwrap();
        assert_eq!(names(&c.roots), vec![Some("Background"), Some("Foreground")]);
        let Node::Group { children: bg, .. } = &c.roots[0] else { panic!("Background not a group") };
        assert_eq!(names(bg), vec![Some("sky"), Some("hill")]);
        let Node::Group { children: fg, .. } = &c.roots[1] else { panic!("Foreground not a group") };
        assert_eq!(names(fg), vec![Some("tree")], "Foreground holds one nested group");
        let Node::Group { children: tree, .. } = &fg[0] else { panic!("tree not a group") };
        assert_eq!(names(tree), vec![Some("trunk"), Some("leaves")]);
    }

    #[test]
    fn a_group_of_one_path_collapses_to_the_path() {
        // The path keeps its own id.
        let c = convert(&svg(r##"<g id="solo"><path id="only" d="M0 0 L10 0 L10 10 Z" fill="#000000"/></g>"##)).unwrap();
        assert_eq!(names(&c.roots), vec![Some("only")]);
        assert!(matches!(c.roots[0], Node::Leaf(_)));
        // A nameless path takes the group's name.
        let c = convert(&svg(r##"<g id="wrap"><path d="M0 0 L10 0 L10 10 Z" fill="#000000"/></g>"##)).unwrap();
        assert_eq!(names(&c.roots), vec![Some("wrap")]);
    }

    #[test]
    fn conversion_is_deterministic() {
        let body = r##"<rect x="0" y="0" width="10" height="10" fill="#123456"/><path d="M10 10 L90 10" fill="none" stroke="#d0202e" stroke-width="1.5"/>"##;
        let a = convert(&svg(body)).unwrap();
        let b = convert(&svg(body)).unwrap();
        assert_eq!(a.leaves(), b.leaves());
        assert_eq!(a.report, b.report);
    }
}

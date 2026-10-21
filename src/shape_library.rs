//! Shape outlines parsed once from the clean-room SVGs in `assets/ds/shapes/` (72 units per inch).

use std::sync::LazyLock;

use usvg::tiny_skia_path::PathSegment;

use crate::design::{Contour, Pt, Seg, ShapeKind};

pub struct Outline {
    /// Native size in points, from the path bounds. The Line is 0 wide.
    pub size_pt: Pt,
    /// Contours in points, translated so the bounds start at the origin.
    pub contours: Vec<Contour>,
}

pub fn parse(svg: &[u8]) -> Outline {
    let tree = crate::svg_import::parse_tree(svg).expect("bundled shape SVG parses");
    let paths: Vec<_> = tree
        .root
        .descendants()
        .filter_map(|node| match &*node.borrow() {
            usvg::NodeKind::Path(path) => Some(path.data.clone()),
            _ => None,
        })
        .collect();
    let bounds = paths.iter().map(|p| p.bounds()).reduce(|a, b| {
        let (l, t) = (a.left().min(b.left()), a.top().min(b.top()));
        let (r, btm) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
        usvg::tiny_skia_path::Rect::from_ltrb(l, t, r, btm).unwrap()
    });
    let bounds = bounds.expect("shape SVG has a path");
    let at = |p: usvg::tiny_skia_path::Point| (p.x - bounds.left(), p.y - bounds.top());

    let mut contours: Vec<Contour> = Vec::new();
    let mut last = (0.0, 0.0);
    for segment in paths.iter().flat_map(|p| p.segments()) {
        match segment {
            PathSegment::MoveTo(p) => {
                last = at(p);
                contours.push(Contour { start: last, segs: Vec::new(), closed: false });
            }
            PathSegment::LineTo(p) => {
                last = at(p);
                current(&mut contours).segs.push(Seg::Line(last));
            }
            PathSegment::QuadTo(c, p) => {
                let (q, end) = (at(c), at(p));
                let lerp = |a: Pt, b: Pt| (a.0 + (b.0 - a.0) * 2.0 / 3.0, a.1 + (b.1 - a.1) * 2.0 / 3.0);
                current(&mut contours).segs.push(Seg::Cubic(lerp(last, q), lerp(end, q), end));
                last = end;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                last = at(p);
                current(&mut contours).segs.push(Seg::Cubic(at(c1), at(c2), last));
            }
            PathSegment::Close => current(&mut contours).closed = true,
        }
    }
    Outline { size_pt: (bounds.width(), bounds.height()), contours }
}

fn current(contours: &mut [Contour]) -> &mut Contour {
    contours.last_mut().expect("path data starts with a move")
}

macro_rules! shapes {
    ($($file:literal),* $(,)?) => {
        [$(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ds/shapes/", $file)) as &[u8]),*]
    };
}

/// In `ShapeKind::FREE` order.
static FREE_SVGS: [&[u8]; 20] = shapes![
    "01-line.svg",
    "02-triangle-down.svg",
    "03-triangle.svg",
    "04-diamond.svg",
    "05-star.svg",
    "06-star-thin.svg",
    "07-octagon.svg",
    "08-pentagon.svg",
    "09-hexagon.svg",
    "10-heart.svg",
    "11-plus.svg",
    "12-equals.svg",
    "13-circle.svg",
    "14-rectangle.svg",
    "15-square.svg",
    "16-pill.svg",
    "17-rounded-square.svg",
    "18-oval.svg",
    "19-arch-pennant.svg",
    "20-half-circle.svg",
];

pub static FREE: LazyLock<Vec<Outline>> = LazyLock::new(|| FREE_SVGS.iter().map(|svg| parse(svg)).collect());

pub use crate::svg_import::Import;

/// Parses an SVG into one [`Import`] per drawable path. The converter in [`crate::svg_import`] does
/// the work, including strokes, colour and the import report; this keeps the older callers that want
/// only the geometry.
pub fn import_svg(bytes: &[u8]) -> Result<Vec<Import>, String> {
    crate::svg_import::convert(bytes).map(|c| c.leaves())
}

// ---------------------------------------------------------------------------
// Procedurally generated free shapes, drawn with lines and cubic arcs. Unions of circles (cloud,
// flower, scallops) arc between their computed intersection points, so outlines never cross.

/// A built-in shape: a name, its contours in a `size_in`-sized local frame, and that size in inches.
pub struct Builtin {
    pub name: &'static str,
    pub category: Category,
    pub contours: Vec<Contour>,
    pub size_in: Pt,
}

/// Builds one closed contour. Every move joins the previous point with a line unless it is
/// already there, so arcs chain into flanks and edges without bookkeeping.
#[derive(Default)]
struct Pen {
    start: Option<Pt>,
    at: Pt,
    segs: Vec<Seg>,
}

fn near(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
}

impl Pen {
    fn to(mut self, p: Pt) -> Self {
        match self.start {
            None => self.start = Some(p),
            Some(_) if near(self.at, p) => {}
            Some(_) => self.segs.push(Seg::Line(p)),
        }
        self.at = p;
        self
    }

    fn curve(mut self, c1: Pt, c2: Pt, p: Pt) -> Self {
        self.segs.push(Seg::Cubic(c1, c2, p));
        self.at = p;
        self
    }

    /// An elliptical arc in degrees, clockwise on screen when `to > from`, as cubics of at most 90°.
    fn ellipse_arc(mut self, c: Pt, (rx, ry): Pt, from: f32, to: f32) -> Self {
        let at = |deg: f32| {
            let (s, k) = deg.to_radians().sin_cos();
            ((c.0 + rx * k, c.1 + ry * s), (-rx * s, ry * k))
        };
        self = self.to(at(from).0);
        let n = ((to - from).abs() / 90.0).ceil().max(1.0) as usize;
        let step = (to - from) / n as f32;
        let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
        for i in 0..n {
            let ((p0, d0), (p1, d1)) = (at(from + step * i as f32), at(from + step * (i + 1) as f32));
            self = self.curve((p0.0 + k * d0.0, p0.1 + k * d0.1), (p1.0 - k * d1.0, p1.1 - k * d1.1), p1);
        }
        self
    }

    fn arc(self, c: Pt, r: f32, from: f32, to: f32) -> Self {
        self.ellipse_arc(c, (r, r), from, to)
    }

    fn close(mut self) -> Contour {
        let start = self.start.expect("a contour draws something");
        if let Some(Seg::Line(p) | Seg::Cubic(_, _, p)) = self.segs.last_mut()
            && near(*p, start)
        {
            *p = start;
        }
        Contour { start, segs: self.segs, closed: true }
    }
}

fn poly(pts: impl IntoIterator<Item = Pt>) -> Contour {
    pts.into_iter().fold(Pen::default(), Pen::to).close()
}

fn circle(c: Pt, r: f32) -> Contour {
    Pen::default().arc(c, r, 0.0, 360.0).close()
}

fn ellipse(c: Pt, rx: f32, ry: f32, tilt_deg: f32) -> Contour {
    let (s, k) = tilt_deg.to_radians().sin_cos();
    map(&Pen::default().ellipse_arc((0.0, 0.0), (rx, ry), 0.0, 360.0).close(), |(x, y)| (c.0 + x * k - y * s, c.1 + x * s + y * k))
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Contour {
    Pen::default()
        .arc((x + w - r, y + r), r, -90.0, 0.0)
        .arc((x + w - r, y + h - r), r, 0.0, 90.0)
        .arc((x + r, y + h - r), r, 90.0, 180.0)
        .arc((x + r, y + r), r, 180.0, 270.0)
        .close()
}

/// Applies a point map to every point, control points included: exact for affine maps.
fn map(c: &Contour, f: impl Fn(Pt) -> Pt) -> Contour {
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

/// The same outline wound the other way, for a hole under the non-zero fill rule.
fn reversed(c: Contour) -> Contour {
    let mut segs = c.segs;
    if segs.last().is_some_and(|s| !matches!(*s, Seg::Line(p) | Seg::Cubic(_, _, p) if p == c.start)) {
        segs.push(Seg::Line(c.start));
    }
    let ends: Vec<Pt> = std::iter::once(c.start)
        .chain(segs.iter().map(|s| match *s {
            Seg::Line(p) | Seg::Cubic(_, _, p) => p,
        }))
        .collect();
    let segs = segs
        .iter()
        .enumerate()
        .rev()
        .map(|(i, s)| match *s {
            Seg::Line(_) => Seg::Line(ends[i]),
            Seg::Cubic(a, b, _) => Seg::Cubic(b, a, ends[i]),
        })
        .collect();
    Contour { start: c.start, segs, closed: true }
}

fn polar(c: Pt, r: f32, deg: f32) -> Pt {
    let (s, k) = deg.to_radians().sin_cos();
    (c.0 + r * k, c.1 + r * s)
}

fn angle(c: Pt, p: Pt) -> f32 {
    (p.1 - c.1).atan2(p.0 - c.0).to_degrees()
}

fn dist(a: Pt, b: Pt) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn regular(n: usize, r: f32, start_deg: f32) -> Vec<Pt> {
    (0..n).map(|i| polar((0.0, 0.0), r, start_deg + 360.0 * i as f32 / n as f32)).collect()
}

fn star(points: usize, outer: f32, inner: f32) -> Vec<Pt> {
    (0..points * 2).map(|i| polar((0.0, 0.0), if i % 2 == 0 { outer } else { inner }, -90.0 + 180.0 * i as f32 / points as f32)).collect()
}

/// A polygon with every corner, convex or concave, filleted to radius `r`.
fn rounded(pts: &[Pt], r: f32) -> Contour {
    let n = pts.len();
    let unit = |a: Pt, b: Pt| ((b.0 - a.0) / dist(a, b), (b.1 - a.1) / dist(a, b));
    let mut pen = Pen::default();
    for i in 0..n {
        let p = pts[i];
        let (u, v) = (unit(p, pts[(i + n - 1) % n]), unit(p, pts[(i + 1) % n]));
        let half = (u.0 * v.0 + u.1 * v.1).clamp(-1.0, 1.0).acos() / 2.0;
        let bisector = unit((0.0, 0.0), (u.0 + v.0, u.1 + v.1));
        let (along, out) = (r / half.tan(), r / half.sin());
        let c = (p.0 + bisector.0 * out, p.1 + bisector.1 * out);
        let from = angle(c, (p.0 + u.0 * along, p.1 + u.1 * along));
        let sweep = (angle(c, (p.0 + v.0 * along, p.1 + v.1 * along)) - from + 540.0).rem_euclid(360.0) - 180.0;
        pen = pen.arc(c, r, from, from + sweep);
    }
    pen.close()
}

/// The two points where two circles cross. Panics if they do not.
fn meet((c1, r1): (Pt, f32), (c2, r2): (Pt, f32)) -> [Pt; 2] {
    let d = dist(c1, c2);
    assert!(d < r1 + r2 && d > (r1 - r2).abs(), "circles at {c1:?} and {c2:?} do not cross");
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).sqrt();
    let (ux, uy) = ((c2.0 - c1.0) / d, (c2.1 - c1.1) / d);
    let m = (c1.0 + a * ux, c1.1 + a * uy);
    [(m.0 - h * uy, m.1 + h * ux), (m.0 + h * uy, m.1 - h * ux)]
}

/// Arcs clockwise along circle `i` from `ends[i]` to `ends[i + 1]`.
fn bumps(circles: &[(Pt, f32)], ends: &[Pt]) -> Contour {
    let mut pen = Pen::default();
    for (i, &(c, r)) in circles.iter().enumerate() {
        let from = angle(c, ends[i]);
        let to = from + (angle(c, ends[i + 1]) - from).rem_euclid(360.0);
        pen = pen.arc(c, r, from, to);
    }
    pen.close()
}

/// The outline of a closed ring of overlapping circles, listed clockwise around `hub`.
fn lobes(circles: &[(Pt, f32)], hub: Pt) -> Contour {
    let n = circles.len();
    let joint = |i: usize| {
        let [p, q] = meet(circles[i % n], circles[(i + 1) % n]);
        if dist(p, hub) > dist(q, hub) { p } else { q }
    };
    let ends: Vec<Pt> = (0..=n).map(|i| joint(i + n - 1)).collect();
    bumps(circles, &ends)
}

/// A row of overlapping circles, left to right, cut flat at `base`. The end circles should
/// reach the base.
fn chain(circles: &[(Pt, f32)], base: f32) -> Contour {
    let n = circles.len();
    let foot = |(c, r): (Pt, f32), side: f32| (c.0 + side * (r * r - (base - c.1).powi(2)).max(0.0).sqrt(), base);
    let mut ends = vec![foot(circles[0], -1.0)];
    ends.extend(circles.windows(2).map(|w| {
        let [p, q] = meet(w[0], w[1]);
        if p.1 < q.1 { p } else { q }
    }));
    ends.push(foot(circles[n - 1], 1.0));
    bumps(circles, &ends)
}

/// The shape left of a disc after a second, overlapping disc is cut away: a crescent.
fn crescent((c1, r1): (Pt, f32), (r2, d, toward_deg): (f32, f32, f32)) -> Contour {
    let c2 = polar(c1, d, toward_deg);
    let horn1 = ((r1 * r1 + d * d - r2 * r2) / (2.0 * r1 * d)).acos().to_degrees();
    let horn2 = 180.0 - ((r2 * r2 + d * d - r1 * r1) / (2.0 * r2 * d)).acos().to_degrees();
    Pen::default().arc(c1, r1, toward_deg + horn1, toward_deg + 360.0 - horn1).arc(c2, r2, toward_deg - horn2, toward_deg + horn2 - 360.0).close()
}

/// Scales so the long side is two inches and moves the bounds to the origin.
fn fitted(name: &'static str, category: Category, contours: Vec<Contour>) -> Builtin {
    let pts: Vec<Pt> = contours.iter().flat_map(|c| crate::geometry::flatten(c, 1e-4)).collect();
    let min = pts.iter().fold((f32::MAX, f32::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
    let max = pts.iter().fold((f32::MIN, f32::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
    let k = 2.0 / (max.0 - min.0).max(max.1 - min.1);
    let contours = contours.iter().map(|c| map(c, |p| ((p.0 - min.0) * k, (p.1 - min.1) * k))).collect();
    Builtin { name, category, contours, size_in: ((max.0 - min.0) * k, (max.1 - min.1) * k) }
}

fn builtins() -> Vec<Builtin> {
    use Category::*;
    let o = (0.0, 0.0);
    let mut v: Vec<Builtin> = Vec::new();
    let mut add = |name: &'static str, category: Category, contours: Vec<Contour>| v.push(fitted(name, category, contours));

    add("Teardrop", Basic, {
        let (c, r, tip) = ((0.0, 1.35), 0.7, (0.0, 0.0));
        let side = (r / dist(c, tip)).acos().to_degrees();
        vec![Pen::default().to(tip).arc(c, r, -90.0 + side, 270.0 - side).close()]
    });
    add("Crescent", Basic, vec![crescent((o, 1.0), (0.85, 0.5, -35.0))]);
    add("Rounded Rectangle", Basic, vec![rounded_rect(0.0, 0.0, 2.5, 1.6, 0.3)]);
    add("Cloud", Basic, vec![chain(&[((0.5, 1.2), 0.5), ((1.15, 0.7), 0.7), ((1.9, 0.85), 0.55), ((2.4, 1.3), 0.4)], 1.7)]);
    add("Lightning", Basic, vec![poly([(1.15, 0.0), (0.2, 1.15), (0.85, 1.15), (0.45, 2.2), (1.6, 0.85), (0.95, 0.85), (1.5, 0.0)])]);
    add("Flower", Basic, {
        let petals: Vec<(Pt, f32)> = (0..5).map(|i| (polar(o, 0.62, -90.0 + 72.0 * i as f32), 0.45)).collect();
        vec![lobes(&petals, o), reversed(circle(o, 0.24))]
    });
    add("Quarter Circle", Basic, vec![Pen::default().to((0.0, 2.0)).arc((0.0, 2.0), 2.0, -90.0, 0.0).close()]);
    add("Semicircle", Basic, vec![Pen::default().arc((1.0, 1.0), 1.0, 180.0, 360.0).close()]);
    add("Egg", Basic, {
        let (w, top, bottom) = (0.75, 1.15, 0.85);
        let k = 0.5523;
        vec![
            Pen::default()
                .to((0.0, 0.0))
                .curve((k * w, 0.0), (w, top - k * top), (w, top))
                .curve((w, top + k * bottom), (k * w, top + bottom), (0.0, top + bottom))
                .curve((-k * w, top + bottom), (-w, top + k * bottom), (-w, top))
                .curve((-w, top - k * top), (-k * w, 0.0), (0.0, 0.0))
                .close(),
        ]
    });
    add("Leaf", Basic, {
        let blade = Pen::default().to(o).curve((0.45, -0.7), (1.5, -0.6), (2.0, 0.0)).curve((1.5, 0.6), (0.45, 0.7), o).close();
        let vein = Pen::default().to((0.35, 0.0)).curve((0.75, -0.07), (1.25, -0.05), (1.6, 0.0)).curve((1.25, 0.05), (0.75, 0.07), (0.35, 0.0)).close();
        let tilt = |(x, y): Pt| (x * 0.766 + y * 0.643, -x * 0.643 + y * 0.766);
        vec![map(&blade, tilt), map(&reversed(vein), tilt)]
    });
    add(
        "Shield",
        Basic,
        vec![
            Pen::default()
                .to((0.0, 0.0))
                .to((1.0, 0.18))
                .to((2.0, 0.0))
                .to((2.0, 0.95))
                .curve((2.0, 1.6), (1.45, 2.0), (1.0, 2.3))
                .curve((0.55, 2.0), (0.0, 1.6), (0.0, 0.95))
                .close(),
        ],
    );
    add("Paw Print", Basic, {
        let pad = lobes(&[((1.0, 1.55), 0.48), ((1.38, 1.95), 0.38), ((1.0, 2.05), 0.4), ((0.62, 1.95), 0.38)], (1.0, 1.85));
        let toes = [((0.22, 1.0), -25.0), ((0.7, 0.5), -8.0), ((1.3, 0.5), 8.0), ((1.78, 1.0), 25.0)];
        std::iter::once(pad).chain(toes.iter().map(|&(c, tilt)| ellipse(c, 0.2, 0.27, tilt))).collect()
    });
    add("Scalloped Circle", Basic, {
        let scallops: Vec<(Pt, f32)> = (0..12).map(|i| (polar(o, 0.85, 30.0 * i as f32), 0.25)).collect();
        vec![lobes(&scallops, o)]
    });
    add("House", Basic, {
        let body = poly([
            (1.1, 0.0),
            (1.5, 0.36),
            (1.5, 0.12),
            (1.75, 0.12),
            (1.75, 0.59),
            (2.2, 1.0),
            (1.9, 1.0),
            (1.9, 2.0),
            (1.3, 2.0),
            (1.3, 1.45),
            (0.9, 1.45),
            (0.9, 2.0),
            (0.3, 2.0),
            (0.3, 1.0),
            (0.0, 1.0),
        ]);
        vec![body, reversed(poly([(0.5, 1.15), (0.8, 1.15), (0.8, 1.45), (0.5, 1.45)]))]
    });
    add("Sun", Basic, {
        let rays = (0..12).map(|i| {
            let a = 30.0 * i as f32;
            poly([polar(o, 0.66, a - 8.0), polar(o, 1.0, a), polar(o, 0.66, a + 8.0)])
        });
        std::iter::once(circle(o, 0.55)).chain(rays).collect()
    });

    add("Right Triangle", Polygons, vec![poly([(0.0, 0.0), (0.0, 2.0), (2.0, 2.0)])]);
    add("Trapezoid", Polygons, vec![poly([(0.5, 0.0), (1.5, 0.0), (2.0, 1.4), (0.0, 1.4)])]);
    add("Parallelogram", Polygons, vec![poly([(0.5, 0.0), (2.2, 0.0), (1.7, 1.4), (0.0, 1.4)])]);
    add("Heptagon", Polygons, vec![poly(regular(7, 1.0, -90.0))]);
    add("Decagon", Polygons, vec![poly(regular(10, 1.0, -90.0))]);
    add("Kite", Polygons, vec![poly([(1.0, 0.0), (2.0, 0.8), (1.0, 2.8), (0.0, 0.8)])]);

    add("Starburst", StarsAndBursts, vec![poly(star(16, 1.0, 0.8))]);
    add("6-Point Star", StarsAndBursts, vec![poly(star(6, 1.0, 0.5))]);
    add("8-Point Star", StarsAndBursts, vec![poly(star(8, 1.0, 0.52))]);
    add("12-Point Star", StarsAndBursts, vec![poly(star(12, 1.0, 0.6))]);
    add("Gear", StarsAndBursts, {
        let (teeth, root, tip) = (10, 0.8, 1.0);
        let p = 360.0 / teeth as f32;
        let mut pen = Pen::default();
        for i in 0..teeth {
            let a = -90.0 + p * i as f32;
            pen = pen.arc(o, tip, a - p * 0.17, a + p * 0.17).arc(o, root, a + p * 0.27, a + p * 0.73);
        }
        vec![pen.close(), reversed(circle(o, 0.3))]
    });
    add("Rounded Star", StarsAndBursts, vec![rounded(&star(5, 1.0, 0.48), 0.1)]);
    add("Sparkle", StarsAndBursts, {
        let tips = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];
        let pull = |a: Pt, b: Pt| (a.0 * 0.25 + b.0 * 0.2, a.1 * 0.25 + b.1 * 0.2);
        let mut pen = Pen::default().to(tips[0]);
        for i in 0..4 {
            let (a, b) = (tips[i], tips[(i + 1) % 4]);
            pen = pen.curve(pull(a, b), pull(b, a), b);
        }
        vec![pen.close()]
    });
    add("Seal", StarsAndBursts, vec![rounded(&star(20, 1.0, 0.88), 0.05)]);

    add("Chevron", Arrows, vec![poly([(0.0, 0.0), (0.7, 0.0), (1.3, 0.9), (0.7, 1.8), (0.0, 1.8), (0.6, 0.9)])]);
    add("Arrow", Arrows, vec![poly([(0.0, 0.32), (1.4, 0.32), (1.4, 0.0), (2.2, 0.5), (1.4, 1.0), (1.4, 0.68), (0.0, 0.68)])]);
    add("Double Arrow", Arrows, vec![poly([(0.0, 0.9), (0.6, 0.3), (0.6, 0.6), (1.8, 0.6), (1.8, 0.3), (2.4, 0.9), (1.8, 1.5), (1.8, 1.2), (0.6, 1.2), (0.6, 1.5)])]);
    add("Block Arrow", Arrows, vec![poly([(0.0, 0.4), (1.0, 0.4), (1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (1.0, 1.6), (0.0, 1.6)])]);
    add("Notched Arrow", Arrows, vec![poly([(0.0, 0.3), (1.3, 0.3), (1.3, 0.0), (2.2, 0.6), (1.3, 1.2), (1.3, 0.9), (0.0, 0.9), (0.35, 0.6)])]);
    add("U-Turn Arrow", Arrows, {
        let (c, outer, inner, foot, head) = ((1.0, 1.0), 1.0, 0.55, 1.9, 0.28);
        let mid = (outer + inner) / 2.0;
        vec![
            Pen::default()
                .to((c.0 - outer, foot))
                .arc(c, outer, 180.0, 360.0)
                .to((c.0 + outer, 1.5))
                .to((c.0 + outer + head, 1.5))
                .to((c.0 + mid, 2.3))
                .to((c.0 + inner - head, 1.5))
                .to((c.0 + inner, 1.5))
                .arc(c, inner, 360.0, 180.0)
                .to((c.0 - inner, foot))
                .close(),
        ]
    });
    add("Double Chevron", Arrows, {
        let chevron = |x: f32| poly([(x, 0.0), (x + 0.55, 0.0), (x + 1.15, 0.9), (x + 0.55, 1.8), (x, 1.8), (x + 0.6, 0.9)]);
        vec![chevron(0.0), chevron(0.8)]
    });

    add("Cross Outline", FramesAndOutlines, {
        let plus = |s: f32, at: f32| {
            poly(
                [
                    (0.35, 0.0),
                    (0.65, 0.0),
                    (0.65, 0.35),
                    (1.0, 0.35),
                    (1.0, 0.65),
                    (0.65, 0.65),
                    (0.65, 1.0),
                    (0.35, 1.0),
                    (0.35, 0.65),
                    (0.0, 0.65),
                    (0.0, 0.35),
                    (0.35, 0.35),
                ]
                .map(|(x, y)| (at + x * s, at + y * s)),
            )
        };
        vec![plus(2.0, 0.0), reversed(plus(1.2, 0.4))]
    });
    add("Ring", FramesAndOutlines, vec![circle(o, 1.0), reversed(circle(o, 0.55))]);
    add(
        "Square Frame",
        FramesAndOutlines,
        vec![poly([(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]), reversed(poly([(0.45, 0.45), (1.55, 0.45), (1.55, 1.55), (0.45, 1.55)]))],
    );
    add("Pentagon Outline", FramesAndOutlines, vec![poly(regular(5, 1.0, -90.0)), reversed(poly(regular(5, 0.6, -90.0)))]);
    add("Hexagon Outline", FramesAndOutlines, vec![poly(regular(6, 1.0, -90.0)), reversed(poly(regular(6, 0.65, -90.0)))]);
    add("Star Outline", FramesAndOutlines, vec![poly(star(5, 1.0, 0.45)), reversed(poly(star(5, 0.55, 0.25)))]);
    add("Heart Outline", FramesAndOutlines, {
        let heart = Pen::default()
            .to((1.0, 1.8))
            .curve((0.4, 1.35), (0.0, 0.95), (0.0, 0.55))
            .curve((0.0, 0.2), (0.27, 0.0), (0.55, 0.0))
            .curve((0.75, 0.0), (0.92, 0.1), (1.0, 0.28))
            .curve((1.08, 0.1), (1.25, 0.0), (1.45, 0.0))
            .curve((1.73, 0.0), (2.0, 0.2), (2.0, 0.55))
            .curve((2.0, 0.95), (1.6, 1.35), (1.0, 1.8))
            .close();
        let inset = map(&heart, |(x, y)| (1.0 + (x - 1.0) * 0.62, 0.78 + (y - 0.78) * 0.62));
        vec![heart, reversed(inset)]
    });
    add("Rounded Square Frame", FramesAndOutlines, vec![rounded_rect(0.0, 0.0, 2.0, 2.0, 0.45), reversed(rounded_rect(0.35, 0.35, 1.3, 1.3, 0.1))]);
    add("Scalloped Frame", FramesAndOutlines, {
        let (step, cols, rows) = (0.3, 7, 5);
        let (w, h) = (step * cols as f32, step * rows as f32);
        let mut centres: Vec<Pt> = (0..cols).map(|i| (step * i as f32, 0.0)).collect();
        centres.extend((0..rows).map(|i| (w, step * i as f32)));
        centres.extend((0..cols).map(|i| (w - step * i as f32, h)));
        centres.extend((0..rows).map(|i| (0.0, h - step * i as f32)));
        let scallops: Vec<(Pt, f32)> = centres.into_iter().map(|c| (c, 0.2)).collect();
        vec![lobes(&scallops, (w / 2.0, h / 2.0)), reversed(rounded_rect(0.3, 0.3, w - 0.6, h - 0.6, 0.12))]
    });

    add("Speech Bubble", SpeechAndLabels, {
        let (w, h, r) = (2.4, 1.5, 0.3);
        vec![
            Pen::default()
                .arc((w - r, r), r, -90.0, 0.0)
                .arc((w - r, h - r), r, 0.0, 90.0)
                .to((1.0, h))
                .to((0.45, h + 0.5))
                .to((0.6, h))
                .arc((r, h - r), r, 90.0, 180.0)
                .arc((r, r), r, 180.0, 270.0)
                .close(),
        ]
    });
    add("Banner", SpeechAndLabels, vec![poly([(0.0, 0.0), (2.4, 0.0), (2.1, 0.45), (2.4, 0.9), (0.0, 0.9), (0.3, 0.45)])]);
    add("Tag", SpeechAndLabels, vec![rounded(&[(0.55, 0.0), (1.8, 0.0), (1.8, 2.0), (0.55, 2.0), (0.0, 1.0)], 0.08), reversed(circle((0.5, 1.0), 0.16))]);
    add("Round Speech Bubble", SpeechAndLabels, {
        let (c, rad) = ((1.2, 0.85), (1.2, 0.85));
        vec![Pen::default().ellipse_arc(c, rad, 135.0, 470.0).to((0.15, 2.05)).close()]
    });
    add("Thought Bubble", SpeechAndLabels, {
        let hub = (1.3, 0.85);
        let puffs: Vec<(Pt, f32)> = (0..10).map(|i| (polar(o, 1.0, 36.0 * i as f32), 0.38)).map(|(p, r)| ((hub.0 + p.0 * 0.92, hub.1 + p.1 * 0.55), r)).collect();
        vec![lobes(&puffs, hub), circle((0.35, 1.95), 0.15), circle((0.1, 2.25), 0.09)]
    });
    add("Plaque", SpeechAndLabels, {
        let (w, h, r) = (2.4, 1.5, 0.3);
        vec![Pen::default().arc((0.0, 0.0), r, 90.0, 0.0).arc((w, 0.0), r, 180.0, 90.0).arc((w, h), r, 270.0, 180.0).arc((0.0, h), r, 0.0, -90.0).close()]
    });
    add("Ticket", SpeechAndLabels, {
        let (w, h, r, notch) = (2.4, 1.2, 0.1, 0.22);
        vec![
            Pen::default()
                .arc((w - r, r), r, -90.0, 0.0)
                .arc((w, h / 2.0), notch, 270.0, 90.0)
                .arc((w - r, h - r), r, 0.0, 90.0)
                .arc((r, h - r), r, 90.0, 180.0)
                .arc((0.0, h / 2.0), notch, 90.0, -90.0)
                .arc((r, r), r, 180.0, 270.0)
                .close(),
        ]
    });
    add("Ribbon", SpeechAndLabels, {
        vec![poly([
            (0.45, 0.0),
            (2.55, 0.0),
            (2.55, 0.3),
            (3.0, 0.3),
            (2.75, 0.75),
            (3.0, 1.2),
            (2.25, 1.2),
            (2.25, 0.9),
            (0.75, 0.9),
            (0.75, 1.2),
            (0.0, 1.2),
            (0.25, 0.75),
            (0.0, 0.3),
            (0.45, 0.3),
        ])]
    });
    v
}

pub static BUILTIN: LazyLock<Vec<Builtin>> = LazyLock::new(builtins);

/// One of [`BUILTIN`]. Saved by name, so adding or reordering built-ins never changes an old project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltinId(usize);

impl BuiltinId {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..BUILTIN.len()).map(Self)
    }

    pub fn named(name: &str) -> Option<Self> {
        BUILTIN.iter().position(|b| b.name == name).map(Self)
    }

    pub fn get(self) -> &'static Builtin {
        &BUILTIN[self.0]
    }
}

impl serde::Serialize for BuiltinId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.get().name)
    }
}

impl<'de> serde::Deserialize<'de> for BuiltinId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Self::named(&name).ok_or_else(|| serde::de::Error::custom(format!("unknown built-in shape \"{name}\"")))
    }
}

/// Project format 1 saved a built-in as its position in this list. Frozen: upgrading reads it.
pub const FORMAT_1_BUILTINS: [&str; 26] = [
    "Teardrop",
    "Starburst",
    "Crescent",
    "Rounded Rectangle",
    "Speech Bubble",
    "Right Triangle",
    "Trapezoid",
    "Parallelogram",
    "Chevron",
    "Arrow",
    "Double Arrow",
    "Block Arrow",
    "Cross Outline",
    "Ring",
    "Square Frame",
    "6-Point Star",
    "8-Point Star",
    "Gear",
    "Cloud",
    "Lightning",
    "Flower",
    "Banner",
    "Tag",
    "Quarter Circle",
    "Semicircle",
    "Pentagon Outline",
];

/// The Shapes panel's filter groups. Every free shape belongs to exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Basic,
    Polygons,
    StarsAndBursts,
    Arrows,
    FramesAndOutlines,
    SpeechAndLabels,
    Lines,
}

impl Category {
    pub const ALL: [Category; 7] = [Self::Basic, Self::Polygons, Self::StarsAndBursts, Self::Arrows, Self::FramesAndOutlines, Self::SpeechAndLabels, Self::Lines];

    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Polygons => "Polygons",
            Self::StarsAndBursts => "Stars and Bursts",
            Self::Arrows => "Arrows",
            Self::FramesAndOutlines => "Frames and Outlines",
            Self::SpeechAndLabels => "Speech and Labels",
            Self::Lines => "Lines",
        }
    }

    /// The panel group for a free shape; `None` for imported, drawn, text, flattened and compound shapes.
    pub fn of(kind: &ShapeKind) -> Option<Category> {
        use ShapeKind as K;
        Some(match kind {
            K::Line => Self::Lines,
            K::TriangleDown | K::Triangle | K::Diamond | K::Octagon | K::Pentagon | K::Hexagon => Self::Polygons,
            K::Star | K::StarThin => Self::StarsAndBursts,
            K::Heart | K::Plus | K::Equals | K::Circle | K::Rectangle | K::Square | K::Pill | K::RoundedSquare | K::Oval | K::Arch | K::HalfCircle => Self::Basic,
            K::Builtin(b) => b.get().category,
            K::Parametric(p) => {
                use crate::parametric::ParamKind;
                match p.kind() {
                    ParamKind::Line => Self::Lines,
                    ParamKind::Polygon => Self::Polygons,
                    ParamKind::Star => Self::StarsAndBursts,
                    ParamKind::Rectangle | ParamKind::Ellipse | ParamKind::Arc | ParamKind::Ring | ParamKind::Plus => Self::Basic,
                }
            }
            K::Imported { .. } | K::Path { .. } | K::Text { .. } | K::Flattened { .. } | K::Bitmap { .. } | K::Compound(_) | K::Instance { .. } => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders every free shape to `target/shape-sheet.png` for eyeballing: `cargo test shape_sheet -- --ignored`.
    #[test]
    #[ignore]
    fn shape_sheet() {
        let mut shapes: Vec<(String, Vec<Contour>, Pt)> = FREE
            .iter()
            .zip(ShapeKind::free())
            .map(|(o, k)| (k.label().to_string(), o.contours.iter().map(|c| scaled(c, 1.0 / 72.0)).collect(), (o.size_pt.0 / 72.0, o.size_pt.1 / 72.0)))
            .collect();
        shapes.extend(BUILTIN.iter().map(|b| (b.name.to_string(), b.contours.clone(), b.size_in)));
        let (cell, cols) = (160.0_f32, 8);
        let rows = shapes.len().div_ceil(cols);
        let mut pm = tiny_skia::Pixmap::new(cell as u32 * cols as u32, cell as u32 * rows as u32).unwrap();
        pm.fill(tiny_skia::Color::WHITE);
        let mut fill = tiny_skia::Paint::default();
        fill.set_color_rgba8(150, 130, 230, 255);
        let mut ink = tiny_skia::Paint::default();
        ink.set_color_rgba8(0, 0, 0, 255);
        for (i, (name, contours, size)) in shapes.iter().enumerate() {
            let (cx, cy) = ((i % cols) as f32 * cell, (i / cols) as f32 * cell);
            let k = (cell - 30.0) / size.0.max(size.1).max(0.01);
            let (ox, oy) = (cx + (cell - size.0 * k) / 2.0, cy + (cell - size.1 * k) / 2.0);
            let mut pb = tiny_skia::PathBuilder::new();
            for c in contours {
                let t = |p: Pt| (ox + p.0 * k, oy + p.1 * k);
                let s = t(c.start);
                pb.move_to(s.0, s.1);
                for seg in &c.segs {
                    match *seg {
                        Seg::Line(p) => {
                            let p = t(p);
                            pb.line_to(p.0, p.1)
                        }
                        Seg::Cubic(a, b, p) => {
                            let (a, b, p) = (t(a), t(b), t(p));
                            pb.cubic_to(a.0, a.1, b.0, b.1, p.0, p.1)
                        }
                    }
                }
                if c.closed {
                    pb.close();
                }
            }
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &fill, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                pm.stroke_path(&path, &ink, &tiny_skia::Stroke { width: 1.5, ..Default::default() }, tiny_skia::Transform::identity(), None);
            }
            eprintln!("{i:2} {name}");
        }
        pm.save_png(concat!(env!("CARGO_MANIFEST_DIR"), "/target/shape-sheet.png")).unwrap();
    }

    fn scaled(c: &Contour, k: f32) -> Contour {
        let s = |p: Pt| (p.0 * k, p.1 * k);
        let segs = c
            .segs
            .iter()
            .map(|g| match *g {
                Seg::Line(p) => Seg::Line(s(p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(s(a), s(b), s(p)),
            })
            .collect();
        Contour { start: s(c.start), segs, closed: c.closed }
    }

    #[test]
    fn every_free_shape_has_a_category_and_every_category_has_a_shape() {
        let free = ShapeKind::free();
        for kind in &free {
            assert!(Category::of(kind).is_some(), "{} has no category", kind.label());
        }
        for category in Category::ALL {
            assert!(free.iter().any(|k| Category::of(k) == Some(category)), "{} is empty", category.label());
        }
    }

    #[test]
    fn every_built_in_is_closed_simple_wound_and_fits_its_size() {
        use crate::geometry::{contains, flatten, signed_area};
        let cross = |o: Pt, a: Pt, b: Pt| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
        let grazes = |p: Pt, (a, b): (Pt, Pt)| {
            let t = (((p.0 - a.0) * (b.0 - a.0) + (p.1 - a.1) * (b.1 - a.1)) / ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2))).clamp(0.0, 1.0);
            (p.0 - a.0 - t * (b.0 - a.0)).hypot(p.1 - a.1 - t * (b.1 - a.1)) < 1e-5
        };
        let meets = |(a, b): (Pt, Pt), (c, d): (Pt, Pt)| {
            (cross(c, d, a) * cross(c, d, b) < 0.0 && cross(a, b, c) * cross(a, b, d) < 0.0)
                || grazes(a, (c, d))
                || grazes(b, (c, d))
                || grazes(c, (a, b))
                || grazes(d, (a, b))
        };
        let edges = |r: &[Pt]| (0..r.len()).map(|i| (r[i], r[(i + 1) % r.len()])).collect::<Vec<_>>();
        let mut names = std::collections::HashSet::new();
        for b in BUILTIN.iter() {
            assert!(names.insert(b.name), "{} is listed twice", b.name);
            assert!(b.contours.iter().all(|c| c.closed), "{} has an open contour", b.name);
            let rings: Vec<Vec<Pt>> = b.contours.iter().map(|c| flatten(c, 0.002)).collect();
            for (i, ri) in rings.iter().enumerate() {
                let ei = edges(ri);
                for (j, rj) in rings.iter().enumerate().skip(i) {
                    let ej = edges(rj);
                    for (m, &e) in ei.iter().enumerate() {
                        for (n, &f) in ej.iter().enumerate() {
                            let adjacent = i == j && (n <= m + 1 || (m == 0 && n == ej.len() - 1));
                            assert!(adjacent || !meets(e, f), "{}: contour {i} edge {m} meets contour {j} edge {n}", b.name);
                        }
                    }
                }
                let parent = rings
                    .iter()
                    .enumerate()
                    .filter(|&(j, o)| j != i && contains(o, ri[0]))
                    .map(|(_, o)| o)
                    .min_by(|x, y| signed_area(x).abs().total_cmp(&signed_area(y).abs()));
                if let Some(o) = parent {
                    assert!(signed_area(ri).signum() != signed_area(o).signum(), "{}: hole {i} winds the same way as its outline", b.name);
                }
            }
            let pts = rings.concat();
            let min = pts.iter().fold((f32::MAX, f32::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
            let max = pts.iter().fold((f32::MIN, f32::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
            let close = |a: f32, b: f32| (a - b).abs() < 0.005;
            assert!(
                close(min.0, 0.0) && close(min.1, 0.0) && close(max.0, b.size_in.0) && close(max.1, b.size_in.1),
                "{} spans {min:?} to {max:?}, not its {:?}",
                b.name,
                b.size_in
            );
        }
    }

    #[test]
    fn an_svg_sized_in_inches_with_a_viewbox_in_points_imports_at_true_scale() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="2in" height="1in" viewBox="0 0 144 72">
            <rect x="36" y="18" width="72" height="36" fill="#000000"/>
        </svg>"##;
        let [rect] = &import_svg(svg).expect("parses")[..] else { panic!("one path") };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(rect.x_in, 0.5) && close(rect.y_in, 0.25), "placed at {}, {}", rect.x_in, rect.y_in);
        assert!(close(rect.view_box.0, 1.0) && close(rect.view_box.1, 0.5), "sized {:?}", rect.view_box);
    }
}

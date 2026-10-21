use std::sync::{Arc, OnceLock};

use egui::{Color32, Image, ImageSource, Painter, Pos2, Rect, Shape, Stroke, Ui, load::Bytes, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "spec 01 registers the icons the wave 1 specs draw")]
pub enum Icon {
    Chevron,
    Shapes,
    Upload,
    Cursor,
    Pen,
    Pencil,
    Text,
    Filter,
    Cut,
    Draw,
    Flip,
    Arrange,
    Align,
    Effects,
    Combine,
    Contour,
    Print,
    Guides,
    Tick,
    Info,
    Warning,
    Close,
    Minus,
    Plus,
    SubmenuArrow,
    CollapseLeft,
    Trash,
    Slice,
    Weld,
    Flatten,
    Unflatten,
    Unite,
    Subtract,
    Intersect,
    Exclude,
    AlignTop,
    AlignVCentre,
    AlignBottom,
    AlignLeft,
    AlignHCentre,
    AlignRight,
    AlignCentre,
    DistributeV,
    DistributeH,
    Offset,
    Sticker,
    Warp,
    Curve,
    Attach,
    Detach,
    Group,
    Ungroup,
    Duplicate,
    More,
    Format,
    TextAlignLeft,
    TextAlignCentre,
    TextAlignRight,
    LetterSpace,
    LineSpace,
    Lock,
    Unlock,
    Measure,
    /// Initials in a ring, for the Monogram Maker (spec 42).
    Monogram,
    /// A right angle with a rounded elbow, for the Corner tool (spec 55).
    Corner,
    /// Two overlapping circles with a pointer, for the Shape Builder tool (spec 57).
    ShapeBuilder,
    /// A craft knife on a line, for the Knife tool (spec 57).
    Knife,
    /// A dropper with a bead at its tip, for the Eyedropper tool (spec 56).
    Eyedropper,
}

/// The bundled Lucide SVGs (`assets/icons`) paint with `currentColor`. Rendering them white lets egui's tint set the colour.
fn whiten(svg: &[u8]) -> Arc<[u8]> {
    String::from_utf8_lossy(svg).replace("currentColor", "#ffffff").into_bytes().into()
}

macro_rules! svg {
    ($name:literal) => {{
        static WHITE: OnceLock<Arc<[u8]>> = OnceLock::new();
        let bytes = WHITE.get_or_init(|| whiten(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons/", $name))));
        Some(($name, bytes.clone()))
    }};
}

/// Bundled file names for the icon table. `None` where there is no matching glyph.
fn svg(icon: Icon) -> Option<(&'static str, Arc<[u8]>)> {
    match icon {
        Icon::Chevron => svg!("chevron-down.svg"),
        Icon::Shapes => svg!("shapes.svg"),
        Icon::Upload => svg!("upload.svg"),
        Icon::Pen => svg!("pen-tool.svg"),
        Icon::Pencil => svg!("pencil.svg"),
        Icon::Text => svg!("type.svg"),
        Icon::Cut => svg!("scissors.svg"),
        Icon::Draw => svg!("pencil-line.svg"),
        Icon::Flip => svg!("mirror-rectangular.svg"),
        Icon::Arrange => svg!("layers.svg"),
        Icon::Align => svg!("text-align-start.svg"),
        Icon::Effects => svg!("wand-sparkles.svg"),
        Icon::Combine => svg!("combine.svg"),
        Icon::Contour => svg!("square-dashed.svg"),
        Icon::Print => svg!("printer.svg"),
        Icon::Guides => svg!("ruler.svg"),
        Icon::Tick => svg!("check.svg"),
        Icon::Info => svg!("info.svg"),
        Icon::Close => svg!("x.svg"),
        Icon::Minus => svg!("minus.svg"),
        Icon::Plus => svg!("plus.svg"),
        Icon::SubmenuArrow => svg!("chevron-right.svg"),
        Icon::CollapseLeft => svg!("chevron-left.svg"),
        _ => None,
    }
}

pub fn paint_icon(ui: &Ui, icon: Icon, rect: Rect, ink: Color32) {
    match svg(icon) {
        Some((name, bytes)) => paint_svg(ui, name, bytes, rect, ink),
        None => paint_drawn(ui, icon, rect, ink),
    }
}

fn paint_svg(ui: &Ui, name: &str, bytes: Arc<[u8]>, rect: Rect, ink: Color32) {
    let source = ImageSource::Bytes { uri: format!("bytes://icons/{name}").into(), bytes: Bytes::Shared(bytes) };
    Image::new(source).tint(ink).paint_at(ui, rect);
}

/// Paints a bundled icon rotated by `radians` about its centre (for up and down chevrons).
pub fn paint_icon_rotated(ui: &Ui, icon: Icon, rect: Rect, ink: Color32, radians: f32) {
    if let Some((name, bytes)) = svg(icon) {
        let source = ImageSource::Bytes { uri: format!("bytes://icons/{name}").into(), bytes: Bytes::Shared(bytes) };
        Image::new(source).tint(ink).rotate(radians, egui::Vec2::splat(0.5)).paint_at(ui, rect);
    }
}

type P = (f32, f32);

/// Strokes in a unit square mapped onto `rect`, y pointing down.
struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    stroke: Stroke,
}

impl Pen<'_> {
    fn p(&self, (x, y): P) -> Pos2 {
        self.rect.min + vec2(x, y) * self.rect.size()
    }

    fn line(&self, pts: &[P]) {
        self.painter.add(Shape::line(pts.iter().map(|&q| self.p(q)).collect(), self.stroke));
    }

    fn closed(&self, pts: &[P]) {
        self.painter.add(Shape::closed_line(pts.iter().map(|&q| self.p(q)).collect(), self.stroke));
    }

    fn filled(&self, pts: &[P], fill: Color32) {
        self.painter.add(Shape::convex_polygon(pts.iter().map(|&q| self.p(q)).collect(), fill, Stroke::NONE));
    }

    fn rect(&self, (x0, y0): P, (x1, y1): P) {
        self.closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    }

    fn dot(&self, c: P, r: f32) {
        self.painter.circle_filled(self.p(c), r * self.rect.width(), self.stroke.color);
    }

    /// Dashes along a closed path, `dash` long with equal gaps, in unit-square lengths.
    fn dashed(&self, pts: &[P], dash: f32) {
        let ring: Vec<P> = pts.iter().chain(pts.first()).copied().collect();
        let mut on = true;
        let mut left = dash;
        for w in ring.windows(2) {
            let (mut a, b) = (w[0], w[1]);
            let mut len = dist(a, b);
            while len > 0.0 {
                let step = left.min(len);
                let t = step / len;
                let next = lerp(a, b, t);
                if on {
                    self.line(&[a, next]);
                }
                a = next;
                len -= step;
                left -= step;
                if left <= 1e-4 {
                    on = !on;
                    left = dash;
                }
            }
        }
    }

    /// The parts of `ring`'s edges whose midpoints satisfy `keep`, so overlapping outlines can be cut back
    /// to a union, intersection or difference.
    fn outline_where(&self, ring: &[P], keep: impl Fn(P) -> bool) {
        const STEPS: usize = 16;
        let mut run: Vec<P> = Vec::new();
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            for k in 0..STEPS {
                let (s, e) = (lerp(a, b, k as f32 / STEPS as f32), lerp(a, b, (k + 1) as f32 / STEPS as f32));
                if keep(lerp(s, e, 0.5)) {
                    if run.is_empty() {
                        run.push(s);
                    }
                    run.push(e);
                } else if !run.is_empty() {
                    self.line(&std::mem::take(&mut run));
                }
            }
        }
        if !run.is_empty() {
            self.line(&run);
        }
    }

    /// Fills the region where `inside` holds, rasterised on a 48-cell grid so any boolean of shapes fills.
    fn fill_where(&self, inside: impl Fn(P) -> bool) {
        const N: usize = 48;
        let cell = 1.0 / N as f32;
        let mut mesh = egui::Mesh::default();
        for row in 0..N {
            let y = (row as f32 + 0.5) * cell;
            let mut col = 0;
            while col < N {
                if !inside(((col as f32 + 0.5) * cell, y)) {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < N && inside(((col as f32 + 0.5) * cell, y)) {
                    col += 1;
                }
                let r = Rect::from_min_max(self.p((start as f32 * cell, row as f32 * cell)), self.p((col as f32 * cell, (row + 1) as f32 * cell)));
                mesh.add_colored_rect(r, self.stroke.color);
            }
        }
        self.painter.add(mesh);
    }
}

fn lerp(a: P, b: P, t: f32) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn dist(a: P, b: P) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// Points on a circular arc, angles in degrees with y down (so 270 is straight up).
fn arc(c: P, r: f32, from: f32, to: f32) -> Vec<P> {
    let n = ((to - from).abs() / 10.0).ceil().max(2.0) as usize;
    (0..=n).map(|i| at(c, r, from + (to - from) * i as f32 / n as f32)).collect()
}

fn quad(a: P, ctrl: P, b: P) -> Vec<P> {
    (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            lerp(lerp(a, ctrl, t), lerp(ctrl, b, t), t)
        })
        .collect()
}

/// The point `r` from `c` at `deg` degrees, y down.
fn at(c: P, r: f32, deg: f32) -> P {
    let a = deg.to_radians();
    (c.0 + r * a.cos(), c.1 + r * a.sin())
}

fn polygon(c: P, r: f32, sides: usize) -> Vec<P> {
    (0..sides).map(|i| at(c, r, 270.0 + 360.0 * i as f32 / sides as f32)).collect()
}

fn inside(ring: &[P], (x, y): P) -> bool {
    let mut odd = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

/// The Combine glyph pair: a square at the top left overlapped by a star at the bottom right.
fn square_and_star() -> ([P; 4], Vec<P>) {
    let square = [(0.1, 0.1), (0.6, 0.1), (0.6, 0.6), (0.1, 0.6)];
    let star = (0..10).map(|i| at((0.63, 0.64), if i % 2 == 0 { 0.33 } else { 0.15 }, 270.0 + 36.0 * i as f32)).collect();
    (square, star)
}

/// A rectangle as a ring, for `outline_where` and `inside`.
fn ring((x0, y0): P, (x1, y1): P) -> [P; 4] {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

fn paperclip() -> Vec<P> {
    let mut path = vec![(0.64, 0.66), (0.3, 0.66)];
    path.extend(arc((0.3, 0.5), 0.16, 90.0, 270.0));
    path.push((0.72, 0.34));
    path.extend(arc((0.72, 0.47), 0.13, 270.0, 450.0));
    path.push((0.38, 0.6));
    path.extend(arc((0.38, 0.5), 0.1, 90.0, 270.0));
    path.push((0.62, 0.4));
    path
}

/// Glyphs the bundled Lucide set does not contain: original 1.5 px line drawings in the unit square.
fn paint_drawn(ui: &Ui, icon: Icon, rect: Rect, ink: Color32) {
    let pen = Pen { painter: ui.painter(), rect, stroke: Stroke::new(1.5_f32, ink) };
    let ghost = Pen { stroke: Stroke::new(1.0_f32, ink.gamma_multiply(0.4)), ..pen };
    let (square, star) = square_and_star();
    let in_a = |p| inside(&square, p);
    let in_b = |p| inside(&star, p);
    match icon {
        Icon::Filter => {
            for (y, knob) in [(0.28, 0.66), (0.5, 0.34), (0.72, 0.62)] {
                pen.line(&[(0.1, y), (0.9, y)]);
                pen.painter.circle(pen.p((knob, y)), rect.width() * 0.08, Color32::WHITE, pen.stroke);
            }
        }
        Icon::Cursor => {
            pen.filled(&[(0.2, 0.1), (0.2, 0.78), (0.37, 0.62), (0.48, 0.9), (0.6, 0.85), (0.49, 0.57), (0.72, 0.56)], ink);
            pen.closed(&[(0.2, 0.1), (0.2, 0.78), (0.37, 0.62), (0.48, 0.9), (0.6, 0.85), (0.49, 0.57), (0.72, 0.56)]);
        }
        Icon::Trash => {
            pen.line(&[(0.15, 0.25), (0.85, 0.25)]);
            pen.line(&[(0.38, 0.25), (0.4, 0.12), (0.6, 0.12), (0.62, 0.25)]);
            pen.line(&[(0.25, 0.25), (0.31, 0.88), (0.69, 0.88), (0.75, 0.25)]);
            pen.line(&[(0.42, 0.42), (0.43, 0.72)]);
            pen.line(&[(0.58, 0.42), (0.57, 0.72)]);
        }
        Icon::Slice => {
            pen.fill_where(|p| in_a(p) && in_b(p));
            pen.closed(&square);
            pen.closed(&star);
        }
        Icon::Weld => {
            pen.outline_where(&square, |p| !in_b(p));
            pen.outline_where(&star, |p| !in_a(p));
        }
        Icon::Unite => pen.fill_where(|p| in_a(p) || in_b(p)),
        Icon::Subtract => {
            pen.fill_where(|p| in_a(p) && !in_b(p));
            pen.outline_where(&star, |p| !in_a(p));
        }
        Icon::Intersect => {
            pen.fill_where(|p| in_a(p) && in_b(p));
            ghost.closed(&square);
            ghost.closed(&star);
        }
        Icon::Exclude => {
            pen.fill_where(|p| in_a(p) != in_b(p));
            pen.outline_where(&square, in_b);
            pen.outline_where(&star, in_a);
        }
        Icon::Flatten | Icon::Unflatten => {
            pen.closed(&[(0.12, 0.62), (0.5, 0.45), (0.88, 0.62), (0.5, 0.79)]);
            pen.line(&[(0.12, 0.76), (0.5, 0.93), (0.88, 0.76)]);
            let (tail, head, dir) = if matches!(icon, Icon::Flatten) { (0.06, 0.38, -1.0) } else { (0.38, 0.06, 1.0) };
            pen.line(&[(0.5, tail), (0.5, head)]);
            pen.line(&[(0.38, head + 0.12 * dir), (0.5, head), (0.62, head + 0.12 * dir)]);
        }
        Icon::AlignTop => {
            pen.line(&[(0.18, 0.12), (0.82, 0.12)]);
            pen.rect((0.38, 0.24), (0.62, 0.86));
        }
        Icon::AlignBottom => {
            pen.line(&[(0.18, 0.88), (0.82, 0.88)]);
            pen.rect((0.38, 0.14), (0.62, 0.76));
        }
        Icon::AlignLeft => {
            pen.line(&[(0.12, 0.18), (0.12, 0.82)]);
            pen.rect((0.24, 0.38), (0.86, 0.62));
        }
        Icon::AlignRight => {
            pen.line(&[(0.88, 0.18), (0.88, 0.82)]);
            pen.rect((0.14, 0.38), (0.76, 0.62));
        }
        Icon::AlignVCentre => {
            pen.line(&[(0.1, 0.5), (0.36, 0.5)]);
            pen.line(&[(0.64, 0.5), (0.9, 0.5)]);
            pen.rect((0.36, 0.26), (0.64, 0.74));
        }
        Icon::AlignHCentre => {
            pen.line(&[(0.5, 0.1), (0.5, 0.36)]);
            pen.line(&[(0.5, 0.64), (0.5, 0.9)]);
            pen.rect((0.26, 0.36), (0.74, 0.64));
        }
        Icon::AlignCentre => {
            pen.line(&[(0.08, 0.5), (0.32, 0.5)]);
            pen.line(&[(0.68, 0.5), (0.92, 0.5)]);
            pen.line(&[(0.5, 0.08), (0.5, 0.32)]);
            pen.line(&[(0.5, 0.68), (0.5, 0.92)]);
            pen.rect((0.32, 0.32), (0.68, 0.68));
        }
        Icon::DistributeV => {
            pen.line(&[(0.14, 0.1), (0.86, 0.1)]);
            pen.line(&[(0.14, 0.9), (0.86, 0.9)]);
            pen.rect((0.26, 0.33), (0.74, 0.67));
        }
        Icon::DistributeH => {
            pen.line(&[(0.1, 0.14), (0.1, 0.86)]);
            pen.line(&[(0.9, 0.14), (0.9, 0.86)]);
            pen.rect((0.33, 0.26), (0.67, 0.74));
        }
        Icon::Offset => {
            pen.closed(&polygon((0.5, 0.53), 0.2, 5));
            pen.dashed(&polygon((0.5, 0.53), 0.42, 5), 0.08);
        }
        Icon::Sticker => {
            pen.line(&arc((0.5, 0.5), 0.38, 90.0, 360.0));
            pen.line(&quad((0.88, 0.5), (0.5, 0.5), (0.5, 0.88)));
        }
        Icon::Warp => {
            let top = quad((0.1, 0.2), (0.5, 0.44), (0.9, 0.2));
            let bottom = quad((0.1, 0.8), (0.5, 0.56), (0.9, 0.8));
            pen.line(&top);
            pen.line(&bottom);
            pen.line(&[(0.1, 0.2), (0.1, 0.8)]);
            pen.line(&[(0.9, 0.2), (0.9, 0.8)]);
            for i in [4, 8] {
                pen.line(&[top[i], bottom[i]]);
            }
        }
        Icon::Curve => {
            let arch = quad((0.1, 0.78), (0.5, -0.02), (0.9, 0.78));
            pen.line(&arch);
            for (i, lean) in [(3, -0.08), (6, 0.0), (9, 0.08)] {
                let (x, y) = arch[i];
                pen.line(&[(x, y + 0.08), (x + lean, y + 0.28)]);
            }
        }
        Icon::Attach => pen.line(&paperclip()),
        Icon::Detach => {
            pen.line(&paperclip());
            pen.line(&[(0.16, 0.86), (0.84, 0.14)]);
        }
        Icon::Group | Icon::Ungroup => {
            let corners = [(0.16, 0.16), (0.84, 0.16), (0.84, 0.84), (0.16, 0.84)];
            for (x, y) in corners {
                pen.rect((x - 0.08, y - 0.08), (x + 0.08, y + 0.08));
            }
            if matches!(icon, Icon::Group) {
                for i in 0..4 {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    pen.line(&[lerp(a, b, 0.17), lerp(a, b, 0.83)]);
                }
                pen.rect((0.36, 0.36), (0.64, 0.64));
            } else {
                pen.rect((0.32, 0.32), (0.5, 0.5));
                pen.rect((0.56, 0.56), (0.72, 0.72));
            }
        }
        Icon::Duplicate => {
            let front = ring((0.34, 0.34), (0.88, 0.88));
            pen.outline_where(&ring((0.12, 0.12), (0.66, 0.66)), |p| !inside(&front, p));
            pen.closed(&front);
            pen.line(&[(0.61, 0.48), (0.61, 0.74)]);
            pen.line(&[(0.48, 0.61), (0.74, 0.61)]);
        }
        Icon::More => {
            for x in [0.2, 0.5, 0.8] {
                pen.dot((x, 0.5), 0.07);
            }
        }
        Icon::Format => {
            pen.rect((0.12, 0.12), (0.88, 0.88));
            pen.line(&[(0.32, 0.72), (0.5, 0.28), (0.68, 0.72)]);
            pen.line(&[(0.4, 0.56), (0.6, 0.56)]);
        }
        Icon::TextAlignLeft | Icon::TextAlignCentre | Icon::TextAlignRight => {
            let short = match icon {
                Icon::TextAlignLeft => (0.1, 0.6),
                Icon::TextAlignCentre => (0.25, 0.75),
                _ => (0.4, 0.9),
            };
            for (y, (x0, x1)) in [(0.2, (0.1, 0.9)), (0.4, short), (0.6, (0.1, 0.9)), (0.8, short)] {
                pen.line(&[(x0, y), (x1, y)]);
            }
        }
        Icon::LetterSpace => {
            for x in [0.3, 0.7] {
                pen.line(&[(x - 0.13, 0.48), (x, 0.12), (x + 0.13, 0.48)]);
                pen.line(&[(x - 0.08, 0.36), (x + 0.08, 0.36)]);
            }
            pen.line(&[(0.1, 0.75), (0.9, 0.75)]);
            pen.line(&[(0.22, 0.63), (0.1, 0.75), (0.22, 0.87)]);
            pen.line(&[(0.78, 0.63), (0.9, 0.75), (0.78, 0.87)]);
        }
        Icon::LineSpace => {
            pen.line(&[(0.25, 0.1), (0.25, 0.9)]);
            pen.line(&[(0.13, 0.22), (0.25, 0.1), (0.37, 0.22)]);
            pen.line(&[(0.13, 0.78), (0.25, 0.9), (0.37, 0.78)]);
            pen.line(&[(0.5, 0.3), (0.9, 0.3)]);
            pen.line(&[(0.5, 0.7), (0.9, 0.7)]);
        }
        Icon::Lock | Icon::Unlock => {
            pen.rect((0.2, 0.46), (0.8, 0.88));
            pen.dot((0.5, 0.67), 0.06);
            if matches!(icon, Icon::Lock) {
                let mut shackle = vec![(0.3, 0.46)];
                shackle.extend(arc((0.5, 0.32), 0.2, 180.0, 360.0));
                shackle.push((0.7, 0.46));
                pen.line(&shackle);
            } else {
                let mut shackle = vec![(0.3, 0.46)];
                shackle.extend(arc((0.5, 0.24), 0.2, 180.0, 360.0));
                shackle.push((0.7, 0.32));
                pen.line(&shackle);
            }
        }
        Icon::Warning => {
            pen.closed(&[(0.5, 0.1), (0.93, 0.85), (0.07, 0.85)]);
            pen.line(&[(0.5, 0.38), (0.5, 0.6)]);
            pen.dot((0.5, 0.73), 0.04);
        }
        Icon::Measure => {
            pen.closed(&[(0.08, 0.68), (0.68, 0.08), (0.92, 0.32), (0.32, 0.92)]);
            for (i, reach) in [0.12, 0.07, 0.12, 0.07, 0.12].into_iter().enumerate() {
                let t = 0.2 + 0.15 * i as f32;
                let edge = (0.08 + 0.6 * t, 0.68 - 0.6 * t);
                pen.line(&[edge, (edge.0 + reach, edge.1 + reach)]);
            }
        }
        Icon::Monogram => {
            let ring: Vec<P> = (0..32).map(|i| (i as f32 / 32.0 * std::f32::consts::TAU).sin_cos()).map(|(s, c)| (0.5 + 0.42 * c, 0.5 + 0.42 * s)).collect();
            pen.closed(&ring);
            pen.line(&[(0.31, 0.66), (0.31, 0.34), (0.5, 0.56), (0.69, 0.34), (0.69, 0.66)]);
        }
        Icon::Corner => {
            // A right angle whose elbow is rounded: up the left, round the corner, out to the right.
            let mut path = vec![(0.25, 0.82), (0.25, 0.45)];
            path.extend(arc((0.45, 0.45), 0.2, 180.0, 270.0));
            path.push((0.82, 0.25));
            pen.line(&path);
            pen.painter.circle_filled(pen.p((0.25, 0.82)), rect.width() * 0.045, ink);
            pen.painter.circle_filled(pen.p((0.82, 0.25)), rect.width() * 0.045, ink);
        }
        Icon::ShapeBuilder => {
            // Two overlapping circles with a small pointer in their lens.
            let ring = |c: P, r: f32| -> Vec<P> { arc(c, r, 0.0, 360.0) };
            pen.closed(&ring((0.38, 0.5), 0.26));
            pen.closed(&ring((0.62, 0.5), 0.26));
            pen.filled(&[(0.5, 0.42), (0.5, 0.72), (0.57, 0.63), (0.62, 0.74), (0.67, 0.52)], ink);
        }
        Icon::Knife => {
            // A blade pointing down-left, over a dashed cut line.
            pen.closed(&[(0.68, 0.16), (0.78, 0.26), (0.34, 0.7), (0.24, 0.7)]);
            for x in [0.12, 0.3, 0.48, 0.66, 0.84] {
                pen.line(&[(x, 0.86), (x + 0.1, 0.86)]);
            }
        }
        Icon::Eyedropper => {
            // A slanted barrel from top-right down to a tip at bottom-left, with a bead at the tip.
            pen.line(&[(0.78, 0.22), (0.4, 0.6)]);
            pen.line(&[(0.66, 0.1), (0.9, 0.34)]);
            pen.line(&[(0.72, 0.16), (0.84, 0.28)]);
            pen.closed(&[(0.4, 0.6), (0.3, 0.58), (0.18, 0.82), (0.42, 0.7)]);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    const DRAWN: [Icon; 38] = [
        Icon::Slice,
        Icon::Weld,
        Icon::Flatten,
        Icon::Unflatten,
        Icon::Unite,
        Icon::Subtract,
        Icon::Intersect,
        Icon::Exclude,
        Icon::AlignTop,
        Icon::AlignVCentre,
        Icon::AlignBottom,
        Icon::AlignLeft,
        Icon::AlignHCentre,
        Icon::AlignRight,
        Icon::AlignCentre,
        Icon::DistributeV,
        Icon::DistributeH,
        Icon::Offset,
        Icon::Sticker,
        Icon::Warp,
        Icon::Curve,
        Icon::Attach,
        Icon::Detach,
        Icon::Group,
        Icon::Ungroup,
        Icon::Duplicate,
        Icon::More,
        Icon::Format,
        Icon::TextAlignLeft,
        Icon::TextAlignCentre,
        Icon::TextAlignRight,
        Icon::LetterSpace,
        Icon::LineSpace,
        Icon::Lock,
        Icon::Unlock,
        Icon::Measure,
        Icon::Warning,
        Icon::Eyedropper,
    ];

    /// Each icon at 20, 24 and 48 px with its name, in one image that is also the committed baseline.
    #[test]
    fn paints_every_new_icon_to_a_snapshot() {
        const COLS: usize = 5;
        const CELL: egui::Vec2 = egui::vec2(170.0, 84.0);
        let rows = DRAWN.len().div_ceil(COLS);
        let mut harness = Harness::builder().with_size(egui::vec2(CELL.x * COLS as f32, CELL.y * rows as f32)).with_pixels_per_point(1.0).wgpu().build_ui(|ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::WHITE);
            let ink = super::super::tokens::TEXT_PRIMARY;
            for (i, icon) in DRAWN.into_iter().enumerate() {
                let at = ui.max_rect().min + vec2((i % COLS) as f32 * CELL.x + 10.0, (i / COLS) as f32 * CELL.y + 8.0);
                paint_icon(ui, icon, Rect::from_min_size(at + vec2(0.0, 14.0), egui::Vec2::splat(20.0)), ink);
                paint_icon(ui, icon, Rect::from_min_size(at + vec2(30.0, 12.0), egui::Vec2::splat(24.0)), ink);
                paint_icon(ui, icon, Rect::from_min_size(at + vec2(66.0, 0.0), egui::Vec2::splat(48.0)), ink);
                let label = format!("{icon:?}");
                ui.painter().text(at + vec2(0.0, 54.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(12.0), ink);
            }
        });
        harness.run();
        crate::ui::test_harness::assert_image_snapshot(&harness.render().expect("wgpu render"), "01-icons");
    }
}

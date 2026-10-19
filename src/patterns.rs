//! Pattern fill for Print Then Cut (spec 34): where an image or a repeating tile sits inside a shape, and
//! the bundled patterns, drawn by code so there is no artwork licence to track. Pure.

use image::{Rgba, RgbaImage};

use crate::assets::AssetId;
use crate::design::Pt;

/// An image painted inside a Print Then Cut shape instead of its flat colour.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatternFill {
    pub asset: AssetId,
    /// The image's width over its height, captured when applied: evaluation has no pixels to ask.
    pub aspect: f32,
    pub mode: PatternMode,
    /// Tile: the tile's width in inches. Fit, Fill and Stretch: a multiplier on the fitted size.
    pub scale: f32,
    /// About the shape's box centre.
    pub rotation_deg: f32,
    /// In the shape's own inches, before its rotation.
    pub offset_in: Pt,
}

/// The smallest tile width or fitted-size multiplier: below it a tile is too small to print.
pub const MIN_SCALE: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PatternMode {
    /// Repeats in both directions, `scale` inches wide.
    Tile,
    /// The whole image inside the box.
    Fit,
    /// The image covers the box, its overflow cut away.
    Fill,
    /// The image stretched to the box.
    Stretch,
}

impl PatternMode {
    pub const ALL: [PatternMode; 4] = [PatternMode::Tile, PatternMode::Fit, PatternMode::Fill, PatternMode::Stretch];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tile => "Tile",
            Self::Fit => "Fit",
            Self::Fill => "Fill",
            Self::Stretch => "Stretch",
        }
    }
}

impl PatternFill {
    /// `asset` at the default for `mode`: a 1 in tile, or the image fitted once.
    pub fn new(asset: AssetId, aspect: f32, mode: PatternMode) -> Self {
        Self { asset, aspect, mode, scale: 1.0, rotation_deg: 0.0, offset_in: (0.0, 0.0) }
    }
}

/// Where a pattern's image lands in its shape's local frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The image's top-left, top-right and bottom-left corners: the UV transform.
    pub corners: [Pt; 3],
    /// Whether the image repeats past its corners (Tile).
    pub repeat: bool,
}

/// `fill` placed in a shape whose box is `box_in`: centred on the box, sized by its mode, turned about
/// the box centre, then moved by its offset.
pub fn placement(fill: &PatternFill, (bw, bh): Pt) -> Placed {
    let aspect = if fill.aspect > 0.0 { fill.aspect } else { 1.0 };
    let wider = bw / bh.max(f32::EPSILON) > aspect;
    let (w, h) = match fill.mode {
        PatternMode::Tile => (fill.scale, fill.scale / aspect),
        PatternMode::Fit if wider => (bh * aspect * fill.scale, bh * fill.scale),
        PatternMode::Fit => (bw * fill.scale, bw / aspect * fill.scale),
        PatternMode::Fill if wider => (bw * fill.scale, bw / aspect * fill.scale),
        PatternMode::Fill => (bh * aspect * fill.scale, bh * fill.scale),
        PatternMode::Stretch => (bw * fill.scale, bh * fill.scale),
    };
    let centre = (bw / 2.0 + fill.offset_in.0, bh / 2.0 + fill.offset_in.1);
    let (sin, cos) = fill.rotation_deg.to_radians().sin_cos();
    let at = |(x, y): Pt| (centre.0 + x * cos - y * sin, centre.1 + x * sin + y * cos);
    Placed { corners: [at((-w / 2.0, -h / 2.0)), at((w / 2.0, -h / 2.0)), at((-w / 2.0, h / 2.0))], repeat: fill.mode == PatternMode::Tile }
}

/// Where `p` falls in the image whose corners are `corners`: (0, 0) at its top left, (1, 1) at its
/// bottom right. `None` for a collapsed image.
pub fn uv([tl, tr, bl]: [Pt; 3], (x, y): Pt) -> Option<Pt> {
    let (across, down) = ((tr.0 - tl.0, tr.1 - tl.1), (bl.0 - tl.0, bl.1 - tl.1));
    let det = across.0 * down.1 - across.1 * down.0;
    if det.abs() < 1e-12 {
        return None;
    }
    let (dx, dy) = (x - tl.0, y - tl.1);
    Some(((dx * down.1 - dy * down.0) / det, (across.0 * dy - across.1 * dx) / det))
}

/// The patterns Kerf ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bundled {
    Stripes,
    Dots,
    Checks,
    Gingham,
    Chevrons,
    Stars,
}

impl Bundled {
    pub const ALL: [Bundled; 6] = [Bundled::Stripes, Bundled::Dots, Bundled::Checks, Bundled::Gingham, Bundled::Chevrons, Bundled::Stars];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stripes => "Stripes",
            Self::Dots => "Dots",
            Self::Checks => "Checks",
            Self::Gingham => "Gingham",
            Self::Chevrons => "Chevrons",
            Self::Stars => "Stars",
        }
    }
}

/// A bundled pattern's three colours: the ground, the main mark and a second mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colourway {
    pub name: &'static str,
    pub ground: [u8; 3],
    pub ink: [u8; 3],
    pub accent: [u8; 3],
}

pub const COLOURWAYS: [Colourway; 4] = [
    Colourway { name: "Berry", ground: [252, 228, 236], ink: [214, 51, 108], accent: [255, 255, 255] },
    Colourway { name: "Ocean", ground: [224, 242, 254], ink: [3, 105, 161], accent: [125, 211, 252] },
    Colourway { name: "Meadow", ground: [240, 253, 244], ink: [21, 128, 61], accent: [250, 204, 21] },
    Colourway { name: "Ink", ground: [255, 255, 255], ink: [24, 24, 27], accent: [161, 161, 170] },
];

/// A bundled pattern's square tile, in pixels. Every motif's period divides it, so tiles meet seamlessly.
pub const CELL_PX: u32 = 256;
/// Samples per pixel along each axis, for smooth edges.
const SUPERSAMPLE: u32 = 4;

/// `pattern` in `colourway` as one seamless tile. The same arguments always give the same pixels, so
/// the stored asset's id is stable.
pub fn generate(pattern: Bundled, colourway: &Colourway) -> RgbaImage {
    let n = SUPERSAMPLE;
    RgbaImage::from_fn(CELL_PX, CELL_PX, |x, y| {
        let mut sum = [0u32; 3];
        for i in 0..n * n {
            let (sx, sy) = (x as f32 + ((i % n) as f32 + 0.5) / n as f32, y as f32 + ((i / n) as f32 + 0.5) / n as f32);
            let rgb = colour(colourway, mark(pattern, sx, sy));
            sum.iter_mut().zip(rgb).for_each(|(s, c)| *s += u32::from(c));
        }
        let avg = |c: u32| (c / (n * n)) as u8;
        Rgba([avg(sum[0]), avg(sum[1]), avg(sum[2]), 255])
    })
}

/// The whole tile of `pattern` shrunk to `px` square, for the picker.
pub fn thumbnail(pattern: Bundled, colourway: &Colourway, px: u32) -> RgbaImage {
    let step = CELL_PX as f32 / px as f32;
    RgbaImage::from_fn(px, px, |x, y| {
        let [r, g, b] = colour(colourway, mark(pattern, (x as f32 + 0.5) * step, (y as f32 + 0.5) * step));
        Rgba([r, g, b, 255])
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Ground,
    Ink,
    Accent,
}

fn colour(c: &Colourway, mark: Mark) -> [u8; 3] {
    match mark {
        Mark::Ground => c.ground,
        Mark::Ink => c.ink,
        Mark::Accent => c.accent,
    }
}

/// Which colour `pattern` puts at (`x`, `y`) pixels. Periodic in [`CELL_PX`] along both axes.
fn mark(pattern: Bundled, x: f32, y: f32) -> Mark {
    let m = |v: f32, period: f32| v.rem_euclid(period);
    match pattern {
        Bundled::Stripes => match m(x + y, 64.0) {
            t if t < 22.0 => Mark::Ink,
            t if (32.0..42.0).contains(&t) => Mark::Accent,
            _ => Mark::Ground,
        },
        Bundled::Dots => {
            let (lx, ly) = (m(x, 64.0), m(y, 64.0));
            let corner = (lx.min(64.0 - lx)).hypot(ly.min(64.0 - ly));
            if (lx - 32.0).hypot(ly - 32.0) < 14.0 {
                Mark::Ink
            } else if corner < 6.0 {
                Mark::Accent
            } else {
                Mark::Ground
            }
        }
        Bundled::Checks => match (m(x, 64.0) < 32.0) == (m(y, 64.0) < 32.0) {
            true => Mark::Ink,
            false => Mark::Ground,
        },
        Bundled::Gingham => match (m(x, 64.0) < 32.0, m(y, 64.0) < 32.0) {
            (true, true) => Mark::Ink,
            (true, false) | (false, true) => Mark::Accent,
            (false, false) => Mark::Ground,
        },
        Bundled::Chevrons => match m(y + (m(x, 64.0) - 32.0).abs(), 64.0) {
            t if t < 18.0 => Mark::Ink,
            t if (32.0..40.0).contains(&t) => Mark::Accent,
            _ => Mark::Ground,
        },
        Bundled::Stars => {
            let (lx, ly) = (m(x, 128.0), m(y, 128.0));
            let near = |v: f32| if v > 64.0 { v - 128.0 } else { v };
            if in_star((lx - 64.0, ly - 64.0), 36.0) {
                Mark::Ink
            } else if in_star((near(lx), near(ly)), 18.0) {
                Mark::Accent
            } else {
                Mark::Ground
            }
        }
    }
}

/// Whether `p` (relative to the centre) is inside an upright five-pointed star of outer radius `outer`.
fn in_star((x, y): Pt, outer: f32) -> bool {
    const POINT: f32 = std::f32::consts::TAU / 5.0;
    let inner = outer * 0.4;
    let r = x.hypot(y);
    if r > outer {
        return false;
    }
    // Measured from the top point, folded into the half sector between a point and the notch after it.
    let t = (y.atan2(x) + std::f32::consts::FRAC_PI_2).rem_euclid(POINT);
    let u = t.min(POINT - t);
    let (px, py) = (r * u.cos(), r * u.sin());
    let (ax, ay) = (outer, 0.0);
    let (bx, by) = (inner * (POINT / 2.0).cos(), inner * (POINT / 2.0).sin());
    let side = |qx: f32, qy: f32| (bx - ax) * (qy - ay) - (by - ay) * (qx - ax);
    side(px, py) * side(0.0, 0.0) >= 0.0
}

/// A pattern-filled Print Then Cut circle, for tests of everything that prints it.
#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use crate::design::{Design, Operation, ShapeKind};

    pub const RED: [u8; 4] = [255, 0, 0, 255];
    pub const BLUE: [u8; 4] = [0, 0, 255, 255];

    /// A 4 x 4 tile, red on its left half and blue on its right.
    pub fn halves() -> (RgbaImage, AssetId) {
        let img = RgbaImage::from_fn(4, 4, |x, _| Rgba(if x < 2 { RED } else { BLUE }));
        let id = crate::assets::id_of(&img);
        (img, id)
    }

    /// The pixels [`circle`] draws from.
    pub fn pixels() -> crate::assets::Pixels {
        let (img, id) = halves();
        crate::assets::Pixels::from([(id, std::sync::Arc::new(img))])
    }

    /// A 2 in Print Then Cut circle at (1, 1), filled with [`halves`] in `mode` (Tile 0.5 in wide).
    pub fn circle(mode: PatternMode) -> Design {
        let mut design = Design::default();
        let id = design.add_shape(ShapeKind::Circle);
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (1.0, 1.0, 2.0, 2.0);
        s.set_operation(Operation::PrintThenCut);
        s.pattern = Some(PatternFill { scale: if mode == PatternMode::Tile { 0.5 } else { 1.0 }, ..PatternFill::new(halves().1, 1.0, mode) });
        design
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> AssetId {
        crate::assets::id_of(&RgbaImage::new(1, 1))
    }

    fn tile(scale: f32, aspect: f32, rotation_deg: f32, offset_in: Pt) -> PatternFill {
        PatternFill { scale, rotation_deg, offset_in, ..PatternFill::new(asset(), aspect, PatternMode::Tile) }
    }

    fn near(a: Pt, b: Pt) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn a_tile_repeats_every_scale_inches_along_its_turned_axes() {
        let fill = tile(0.5, 2.0, 30.0, (0.1, -0.2));
        let placed = placement(&fill, (2.0, 3.0));
        assert!(placed.repeat, "Tile repeats");
        let (sin, cos) = 30f32.to_radians().sin_cos();
        let across = (0.5 * cos, 0.5 * sin);
        let down = (-0.25 * sin, 0.25 * cos);
        let wrap = |(u, v): Pt| (u.rem_euclid(1.0), v.rem_euclid(1.0));
        for p in [(0.0, 0.0), (0.37, 1.91), (1.8, 2.6), (-0.4, 0.2)] {
            let at = uv(placed.corners, p).unwrap();
            for step in [across, down, (across.0 + down.0, across.1 + down.1)] {
                let moved = uv(placed.corners, (p.0 + step.0, p.1 + step.1)).unwrap();
                assert!(near(wrap(at), wrap(moved)), "{p:?} + {step:?}: {at:?} vs {moved:?}");
                assert!(!near(at, moved), "one tile on is the next repeat, not the same spot");
            }
            let unturned = uv(placed.corners, (p.0 + 0.5, p.1)).unwrap();
            assert!(!near(wrap(at), wrap(unturned)), "half an inch along the unturned x axis is not a repeat");
        }
    }

    #[test]
    fn a_tile_is_centred_on_the_box_centre_moved_by_its_offset() {
        let placed = placement(&tile(0.5, 1.0, 45.0, (0.3, -0.2)), (2.0, 3.0));
        assert!(near(uv(placed.corners, (1.3, 1.3)).unwrap(), (0.5, 0.5)));
        let still = placement(&tile(0.5, 1.0, 0.0, (0.0, 0.0)), (2.0, 3.0));
        assert!(near(still.corners[0], (0.75, 1.25)), "{:?}", still.corners);
    }

    #[test]
    fn fit_fill_and_stretch_size_the_image_to_the_box() {
        let size = |mode, scale| {
            let fill = PatternFill { scale, ..PatternFill::new(asset(), 2.0, mode) };
            let Placed { corners: [tl, tr, bl], repeat } = placement(&fill, (3.0, 3.0));
            assert!(!repeat, "{mode:?} draws the image once");
            ((tr.0 - tl.0).hypot(tr.1 - tl.1), (bl.0 - tl.0).hypot(bl.1 - tl.1))
        };
        assert!(near(size(PatternMode::Fit, 1.0), (3.0, 1.5)), "the whole 2:1 image inside a square");
        assert!(near(size(PatternMode::Fill, 1.0), (6.0, 3.0)), "the square covered");
        assert!(near(size(PatternMode::Stretch, 1.0), (3.0, 3.0)));
        assert!(near(size(PatternMode::Fit, 0.5), (1.5, 0.75)), "scale multiplies the fitted size");
    }

    #[test]
    fn every_bundled_pattern_wraps_seamlessly() {
        let cell = CELL_PX as f32;
        for pattern in Bundled::ALL {
            for y in 0..CELL_PX {
                for x in 0..CELL_PX {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let here = mark(pattern, fx, fy);
                    assert_eq!(mark(pattern, fx + cell, fy), here, "{pattern:?}: the column after the right edge is the left column, at ({x}, {y})");
                    assert_eq!(mark(pattern, fx, fy + cell), here, "{pattern:?}: the row after the bottom edge is the top row, at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn bundled_patterns_are_deterministic_opaque_and_use_their_colourway() {
        let berry = &COLOURWAYS[0];
        for pattern in Bundled::ALL {
            let img = generate(pattern, berry);
            assert_eq!(img.dimensions(), (CELL_PX, CELL_PX));
            assert_eq!(crate::assets::id_of(&img), crate::assets::id_of(&generate(pattern, berry)), "{pattern:?} is the same every time");
            assert!(img.pixels().all(|p| p.0[3] == 255), "{pattern:?} is opaque");
            let has = |rgb: [u8; 3]| img.pixels().any(|p| p.0[..3] == rgb);
            assert!(has(berry.ground) && has(berry.ink), "{pattern:?} shows its ground and its ink");
        }
        let ids: std::collections::HashSet<_> = Bundled::ALL.iter().flat_map(|&p| COLOURWAYS.iter().map(move |c| crate::assets::id_of(&generate(p, c)))).collect();
        assert_eq!(ids.len(), Bundled::ALL.len() * COLOURWAYS.len(), "every pattern and colourway differs");
    }
}

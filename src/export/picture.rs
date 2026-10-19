//! An image on a sheet (spec 37): which asset, where its corners land, and the bleed it is printed with,
//! or a repeating pattern tile (spec 34). The writers get the decoded pixels from the caller, so they stay
//! pure; [`prepare`] turns pixels and a placement into the exact image a writer embeds.

use std::io::Cursor;

use image::RgbaImage;

use crate::assets::{AssetId, Pixels};
use crate::design::{Contour, Pt};

/// Embedded images keep their own resolution up to this many pixels per placed inch.
pub const MAX_DPI: f32 = 300.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub asset: AssetId,
    /// Where the image's top-left, top-right and bottom-left corners land, in sheet inches.
    pub corners: [Pt; 3],
    /// How far past its edges the image extends by repeating its edge pixels: Print Then Cut's bleed
    /// stamp. Zero for the art itself.
    pub bleed_in: f32,
    /// A pattern's one tile, in sheet inches, repeated across the box `corners` spans.
    pub tile: Option<[Pt; 3]>,
}

impl Picture {
    /// `asset` drawn once with its corners at `corners`.
    pub fn once(asset: AssetId, corners: [Pt; 3], bleed_in: f32) -> Picture {
        Picture { asset, corners, bleed_in, tile: None }
    }

    /// `asset` tiled from `tile` across the box around `contours`. `None` when they are empty.
    pub fn tiled(asset: AssetId, tile: [Pt; 3], contours: &[Contour]) -> Option<Picture> {
        let pts: Vec<Pt> = contours.iter().flat_map(|c| crate::geometry::flatten(c, crate::geometry::FLATTEN_TOL_IN)).collect();
        let (x0, y0) = pts.iter().copied().reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))?;
        let (x1, y1) = pts.iter().copied().reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))?;
        Some(Picture { asset, corners: [(x0, y0), (x1, y0), (x0, y1)], bleed_in: 0.0, tile: Some(tile) })
    }

    /// The picture with every point moved by `f`, an affine map.
    pub fn mapped(self, f: impl Fn(Pt) -> Pt) -> Picture {
        Picture { corners: self.corners.map(&f), tile: self.tile.map(|t| t.map(&f)), ..self }
    }
}

/// What a writer draws: the pixels at their embedded resolution and where their corners land.
pub struct Prepared {
    pub image: RgbaImage,
    pub corners: [Pt; 3],
}

/// `picture`'s pixels capped at [`MAX_DPI`] for its placed size, then grown by its bleed; a tiled one is
/// rendered across its box at [`MAX_DPI`]. `None` when the asset is not in `pixels`.
pub fn prepare(picture: &Picture, pixels: &Pixels) -> Option<Prepared> {
    let source = pixels.get(&picture.asset)?;
    if let Some(tile) = picture.tile {
        return tiled(source, picture.corners, tile);
    }
    let [tl, tr, bl] = picture.corners;
    let (across, down) = (sub(tr, tl), sub(bl, tl));
    let capped = |native: u32, inches: f32| native.min((inches * MAX_DPI).ceil().max(1.0) as u32);
    let (w, h) = (capped(source.width(), len(across)), capped(source.height(), len(down)));
    let image = match (w, h) == source.dimensions() {
        true => RgbaImage::clone(source),
        false => image::imageops::resize(&**source, w, h, image::imageops::FilterType::Triangle),
    };
    let pad = |px: u32, inches: f32| if inches > 0.0 { (picture.bleed_in * px as f32 / inches).ceil().max(0.0) as u32 } else { 0 };
    let (px, py) = (pad(w, len(across)), pad(h, len(down)));
    if (px, py) == (0, 0) {
        return Some(Prepared { image, corners: picture.corners });
    }
    let padded = RgbaImage::from_fn(w + 2 * px, h + 2 * py, |x, y| *image.get_pixel(x.saturating_sub(px).min(w - 1), y.saturating_sub(py).min(h - 1)));
    let step = |v: Pt, n: u32, by: u32| scale(v, by as f32 / n as f32);
    let start = sub(sub(tl, step(across, w, px)), step(down, h, py));
    let corners = [start, add(start, step(across, w, w + 2 * px)), add(start, step(down, h, h + 2 * py))];
    Some(Prepared { image: padded, corners })
}

/// `source` repeated from `tile` across the box whose corners are `corners`, one pixel per 1/[`MAX_DPI`] in.
fn tiled(source: &RgbaImage, corners: [Pt; 3], tile: [Pt; 3]) -> Option<Prepared> {
    let [tl, tr, bl] = corners;
    let px = |v: Pt| (len(v) * MAX_DPI).ceil().max(1.0) as u32;
    let (w, h) = (px(sub(tr, tl)), px(sub(bl, tl)));
    let tile_px = tile.map(|p| crate::patterns::uv(corners, p).map(|(u, v)| (u * w as f32, v * h as f32)));
    let [Some(a), Some(b), Some(c)] = tile_px else { return None };
    let pattern = premultiplied(source)?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let paint = tiny_skia::Paint { shader: image_shader(&pattern, [a, b, c], true), ..tiny_skia::Paint::default() };
    pixmap.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?, &paint, tiny_skia::Transform::identity(), None);
    let image = RgbaImage::from_fn(w, h, |x, y| {
        let c = pixmap.pixel(x, y).map_or(tiny_skia::ColorU8::from_rgba(0, 0, 0, 0), |p| p.demultiply());
        image::Rgba([c.red(), c.green(), c.blue(), c.alpha()])
    });
    Some(Prepared { image, corners })
}

/// A shader drawing `img` with its top-left, top-right and bottom-left corners at `corners` (pixels),
/// tiling past them when it repeats and extending its edge pixels when it does not.
pub(crate) fn image_shader(img: &tiny_skia::Pixmap, [tl, tr, bl]: [Pt; 3], repeat: bool) -> tiny_skia::Shader<'_> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let at = tiny_skia::Transform::from_row((tr.0 - tl.0) / w, (tr.1 - tl.1) / w, (bl.0 - tl.0) / h, (bl.1 - tl.1) / h, tl.0, tl.1);
    let spread = if repeat { tiny_skia::SpreadMode::Repeat } else { tiny_skia::SpreadMode::Pad };
    tiny_skia::Pattern::new(img.as_ref(), spread, tiny_skia::FilterQuality::Bilinear, 1.0, at)
}

/// `img` as a tiny-skia pixmap, whose pixels are premultiplied.
pub(crate) fn premultiplied(img: &RgbaImage) -> Option<tiny_skia::Pixmap> {
    let mut out = tiny_skia::Pixmap::new(img.width(), img.height())?;
    for (dst, src) in out.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(out)
}

/// The image as PNG bytes, alpha kept.
pub fn png(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    image.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).expect("PNG encoding to memory cannot fail");
    out
}

/// Standard base64 with padding, for a data URI.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(v: Pt, k: f32) -> Pt {
    (v.0 * k, v.1 * k)
}

fn len(v: Pt) -> f32 {
    v.0.hypot(v.1)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn pixels(img: RgbaImage) -> (Pixels, AssetId) {
        let id = crate::assets::id_of(&img);
        (Pixels::from([(id.clone(), Arc::new(img))]), id)
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (plain, encoded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn a_large_image_is_capped_at_300_dpi_for_its_placed_size() {
        let (pixels, asset) = pixels(RgbaImage::new(3000, 1500));
        let picture = Picture { asset, corners: [(1.0, 1.0), (3.0, 1.0), (1.0, 2.0)], bleed_in: 0.0, tile: None };
        let prepared = prepare(&picture, &pixels).unwrap();
        assert_eq!(prepared.image.dimensions(), (600, 300), "2 x 1 in at 300 dpi");
        assert_eq!(prepared.corners, picture.corners);
    }

    #[test]
    fn a_small_image_keeps_its_own_pixels() {
        let (pixels, asset) = pixels(RgbaImage::new(20, 10));
        let prepared = prepare(&Picture { asset, corners: [(0.0, 0.0), (2.0, 0.0), (0.0, 1.0)], bleed_in: 0.0, tile: None }, &pixels).unwrap();
        assert_eq!(prepared.image.dimensions(), (20, 10));
    }

    #[test]
    fn bleed_repeats_the_edge_pixels_and_grows_the_corners() {
        let img = RgbaImage::from_fn(10, 10, |x, _| if x < 5 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let (pixels, asset) = pixels(img);
        let prepared = prepare(&Picture { asset, corners: [(1.0, 1.0), (2.0, 1.0), (1.0, 2.0)], bleed_in: 0.2, tile: None }, &pixels).unwrap();
        assert_eq!(prepared.image.dimensions(), (14, 14), "two 0.1 in pixels each side");
        assert_eq!(prepared.image.get_pixel(0, 0).0, [255, 0, 0, 255], "the left edge extends");
        assert_eq!(prepared.image.get_pixel(13, 13).0, [0, 0, 255, 255], "the right edge extends");
        let [tl, tr, bl] = prepared.corners;
        let near = |a: Pt, b: Pt| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        assert!(near(tl, (0.8, 0.8)) && near(tr, (2.2, 0.8)) && near(bl, (0.8, 2.2)), "{:?}", prepared.corners);
    }

    #[test]
    fn a_tile_repeats_across_its_box_at_300_dpi() {
        let img = RgbaImage::from_fn(4, 4, |x, _| if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let (pixels, asset) = pixels(img);
        let square = |(x, y): Pt, s: f32| Contour { start: (x, y), segs: [(x + s, y), (x + s, y + s), (x, y + s)].map(crate::design::Seg::Line).to_vec(), closed: true };
        let picture = Picture::tiled(asset, [(1.0, 1.0), (1.1, 1.0), (1.0, 1.1)], &[square((1.0, 1.0), 0.4)]).unwrap();
        assert_eq!(picture.corners, [(1.0, 1.0), (1.4, 1.0), (1.0, 1.4)], "the box around the outline");
        let prepared = prepare(&picture, &pixels).unwrap();
        assert_eq!(prepared.image.dimensions(), (120, 120), "0.4 in at 300 dpi");
        let red = |x: u32| prepared.image.get_pixel(x, 60).0 == [255, 0, 0, 255];
        for (x, want) in [(7, true), (22, false), (37, true), (52, false), (97, true), (112, false)] {
            assert_eq!(red(x), want, "x {x}: each 0.1 in tile is red then blue");
        }
    }

    #[test]
    fn a_missing_asset_prepares_nothing() {
        let picture = Picture { asset: crate::assets::id_of(&RgbaImage::new(1, 1)), corners: [(0.0, 0.0); 3], bleed_in: 0.0, tile: None };
        assert!(prepare(&picture, &Pixels::new()).is_none());
    }
}

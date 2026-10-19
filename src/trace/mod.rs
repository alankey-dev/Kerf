//! Image tracing: a cleaned RGBA buffer in, closed contours in pixel space out. This module is
//! the only caller of `visioncortex`. Outline fitting follows vtracer 0.6.5's binary driver
//! (MIT, visioncortex), reading `Spline`s directly instead of an SVG string; connected
//! components and holes are found here rather than by `BinaryImage::to_clusters`, whose u16
//! labels overflow on a noisy 4096 px mask. Colour separation is Kerf's own (median cut in Lab,
//! refined by k-means), and every layer is traced as one binary mask, so Stacked layers nest
//! and Side by side layers never overlap by construction.

pub mod raster;
pub mod worker;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::Color32;
use image::RgbaImage;
use visioncortex::{BinaryImage, PointF64, Spline};

use crate::design::{Contour, Seg};
use raster::{Lab, LabImage, Mask, delta_e, luminance};

/// A trace stopped between two stages because [`Cancel`] said so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

/// Whether the job running as `generation` should stop: true once a newer generation is the
/// latest. The image worker hands one to every job; submitting a newer job, or cancelling, bumps
/// the latest generation.
#[derive(Clone, Debug)]
pub struct Cancel {
    latest: Arc<AtomicU64>,
    generation: u64,
}

impl Cancel {
    pub fn new(latest: Arc<AtomicU64>, generation: u64) -> Self {
        Self { latest, generation }
    }

    /// A job nothing will ever supersede.
    #[cfg(test)]
    pub fn never() -> Self {
        Self::new(Arc::new(AtomicU64::new(0)), 0)
    }

    pub fn cancelled(&self) -> bool {
        self.latest.load(Ordering::Acquire) != self.generation
    }
}

/// One Posterise colour: its Lab centre and the mean sRGB of the pixels that chose it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swatch {
    pub colour: Color32,
    lab: Lab,
}

/// The Posterise palette, built once from the preview by [`palette`] and reused for the full
/// trace so a swatch index means the same colour at both resolutions. `swatches` are ordered by
/// pixel count descending; `merged` lists swatches the user folded away, whose pixels trace as
/// the nearest unmerged swatch.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub swatches: Vec<Swatch>,
    pub merged: Vec<usize>,
}

impl Palette {
    /// The unmerged swatch indices, in palette order.
    pub fn live(&self) -> Vec<usize> {
        (0..self.swatches.len()).filter(|i| !self.merged.contains(i)).collect()
    }

    /// For each swatch, the live swatch its pixels belong to.
    fn targets(&self) -> Vec<Option<usize>> {
        let live = self.live();
        (0..self.swatches.len())
            .map(|i| if live.contains(&i) { Some(i) } else { live.iter().copied().min_by(|&a, &b| self.distance(i, a).total_cmp(&self.distance(i, b))) })
            .collect()
    }

    fn distance(&self, i: usize, j: usize) -> f32 {
        delta_e(self.swatches[i].lab, self.swatches[j].lab)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Colours {
    /// One layer of every kept pixel darker than `level` (Rec. 601 luminance 0..=255), or
    /// lighter with `invert`. Its fill is black.
    Threshold { level: u8, invert: bool },
    /// One layer per unmerged swatch, in palette order.
    Posterise(Palette),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layering {
    /// Layer i covers its own colour and every layer above it, so each layer contains the next
    /// (layered cardstock). Index 0 is the bottom.
    Stacked,
    /// Each layer covers its own colour only; no two overlap (vinyl). Where a lower (larger)
    /// layer meets a higher one it gives up its one-pixel seam, because visioncortex's
    /// polygon simplifier keeps each outline about half a pixel outside its pixels and two
    /// such outlines on a shared boundary would overlap; the seam leaves them butting.
    SideBySide,
}

/// Panel controls and how each reaches visioncortex (spline mode, splice threshold fixed at
/// 45 degrees, 10 smoothing iterations):
/// - `speckle_px`: 4-connected runs under `speckle_px * speckle_px` pixels (islands and holes)
///   are flipped before tracing, vtracer's `filter_speckle`.
/// - `smoothing` (0..=100): `length_threshold`, the shortest segment the smoother subdivides,
///   over vtracer's range of 3.5 px at 0 to 10 px at 100 (its default 4 px sits at 8).
/// - `corner_deg`: `corner_threshold` in radians; a join sharper than it stays a corner.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceSettings {
    pub colours: Colours,
    pub layering: Layering,
    pub speckle_px: u32,
    pub smoothing: f32,
    pub corner_deg: f32,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self { colours: Colours::Threshold { level: 128, invert: false }, layering: Layering::Stacked, speckle_px: 4, smoothing: 8.0, corner_deg: 60.0 }
    }
}

/// One traced colour: at least one closed contour in pixel space (0..width, 0..height, y down)
/// with `Seg::Cubic` spline segments.
#[derive(Clone, Debug, PartialEq)]
pub struct TracedLayer {
    pub colour: Color32,
    pub contours: Vec<Contour>,
}

/// The `count` (clamped to 2..=16) most representative colours of the kept pixels: median cut
/// in Lab over a stride sample, refined by k-means, deterministic for a given image. Fewer
/// swatches when the image holds fewer distinct colours; none when nothing is kept.
pub fn palette(img: &RgbaImage, count: u8) -> Palette {
    let kept = kept_mask(img);
    Palette { swatches: quantise(img, &LabImage::of(img), &kept, count.clamp(2, 16) as usize), merged: Vec::new() }
}

/// Traces `img` into one layer per colour, dropping layers that own no pixels. Fully
/// transparent pixels belong to no layer.
pub fn trace(img: &RgbaImage, settings: &TraceSettings, cancel: &Cancel) -> Result<Vec<TracedLayer>, Cancelled> {
    let kept = kept_mask(img);
    let (w, h) = (img.width(), img.height());
    let masks: Vec<(Color32, Mask)> = match &settings.colours {
        Colours::Threshold { level, invert } => {
            let dark = Mask::from_fn(w, h, |x, y| {
                let p = img.get_pixel(x, y).0;
                kept.get(x, y) && ((luminance([p[0], p[1], p[2]]) < *level) != *invert)
            });
            vec![(Color32::BLACK, dark)]
        }
        Colours::Posterise(palette) => {
            let live = palette.live();
            if live.is_empty() {
                return Ok(Vec::new());
            }
            let lab = LabImage::of(img);
            check(cancel)?;
            let targets = palette.targets();
            let centres: Vec<Lab> = palette.swatches.iter().map(|s| s.lab).collect();
            let slot_of: Vec<u8> = targets.iter().map(|t| t.and_then(|t| live.iter().position(|&l| l == t)).map_or(u8::MAX, |s| s as u8)).collect();
            let labels: Vec<u8> = assign(&lab, &kept, &centres).into_iter().map(|l| if l == u8::MAX { u8::MAX } else { slot_of[l as usize] }).collect();
            let label = |x: u32, y: u32| labels[(y * w + x) as usize];
            live.iter()
                .enumerate()
                .map(|(slot, &i)| {
                    let slot = slot as u8;
                    let mask = match settings.layering {
                        Layering::Stacked => Mask::from_fn(w, h, |x, y| kept.get(x, y) && label(x, y) >= slot),
                        Layering::SideBySide => Mask::from_fn(w, h, |x, y| {
                            kept.get(x, y) && label(x, y) == slot && !kept.neighbours(x, y).any(|(nx, ny)| kept.get(nx, ny) && label(nx, ny) > slot)
                        }),
                    };
                    (palette.swatches[i].colour, mask)
                })
                .collect()
        }
    };
    let mut layers = Vec::with_capacity(masks.len());
    for (colour, mut mask) in masks {
        check(cancel)?;
        mask.despeckle(min_area(settings));
        check(cancel)?;
        let contours = trace_mask(&mask, settings);
        if !contours.is_empty() {
            layers.push(TracedLayer { colour, contours });
        }
    }
    Ok(layers)
}

/// The outline of every kept (alpha > 0) pixel as closed contours: the Single cut shape and
/// the Print Then Cut clip.
pub fn silhouette(img: &RgbaImage, settings: &TraceSettings, cancel: &Cancel) -> Result<Vec<Contour>, Cancelled> {
    let mut mask = kept_mask(img);
    check(cancel)?;
    mask.despeckle(min_area(settings));
    check(cancel)?;
    Ok(trace_mask(&mask, settings))
}

fn check(cancel: &Cancel) -> Result<(), Cancelled> {
    if cancel.cancelled() { Err(Cancelled) } else { Ok(()) }
}

fn kept_mask(img: &RgbaImage) -> Mask {
    Mask::from_fn(img.width(), img.height(), |x, y| img.get_pixel(x, y).0[3] > 0)
}

fn min_area(settings: &TraceSettings) -> usize {
    (settings.speckle_px as usize).pow(2)
}

const SPLICE_DEG: f64 = 45.0;
const MAX_ITERATIONS: usize = 10;
/// visioncortex's `Cluster::OUTSET_RATIO`, which `to_compound_path` hard-codes.
const OUTSET_RATIO: f64 = 8.0;
const LENGTH_RANGE: (f64, f64) = (3.5, 10.0);

/// Inclusive pixel bounds (x0, y0, x1, y1).
type Bounds = (u32, u32, u32, u32);

/// One closed contour per 4-connected run of set pixels, plus one per hole inside it, each fitted
/// by visioncortex's path walker, smoother and spline fitter on a crop of that run alone.
fn trace_mask(mask: &Mask, settings: &TraceSettings) -> Vec<Contour> {
    let corner = (settings.corner_deg as f64).to_radians();
    let length = LENGTH_RANGE.0 + (settings.smoothing.clamp(0.0, 100.0) as f64 / 100.0) * (LENGTH_RANGE.1 - LENGTH_RANGE.0);
    let fit = |crop: &Mask, offset: (u32, u32), outer: bool| {
        let mut image = BinaryImage::new_w_h(crop.width as usize, crop.height as usize);
        for (i, bit) in crop.bits().iter().enumerate() {
            if *bit {
                image.pixels.set(i, true);
            }
        }
        let mut spline = Spline::from_image(&image, outer, corner, OUTSET_RATIO, length, MAX_ITERATIONS, SPLICE_DEG.to_radians());
        spline.offset(&PointF64 { x: offset.0 as f64, y: offset.1 as f64 });
        to_contour(&spline)
    };
    let mut contours = Vec::new();
    mask.for_each_component(true, |pixels, (x0, y0, x1, y1)| {
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut crop = Mask::new(w, h, false);
        for &(x, y) in pixels {
            crop.set(x - x0, y - y0, true);
        }
        let mut holes: Vec<(Vec<(u32, u32)>, Bounds)> = Vec::new();
        crop.for_each_component(false, |hole, bounds| {
            let touches_border = bounds.0 == 0 || bounds.1 == 0 || bounds.2 == w - 1 || bounds.3 == h - 1;
            if !touches_border {
                holes.push((hole.to_vec(), bounds));
            }
        });
        for (hole, _) in &holes {
            for &(x, y) in hole {
                crop.set(x, y, true);
            }
        }
        contours.extend(fit(&crop, (x0, y0), true));
        for (hole, (hx0, hy0, hx1, hy1)) in &holes {
            let mut hole_crop = Mask::new(hx1 - hx0 + 1, hy1 - hy0 + 1, false);
            for &(x, y) in hole {
                hole_crop.set(x - hx0, y - hy0, true);
            }
            contours.extend(fit(&hole_crop, (x0 + hx0, y0 + hy0), false));
        }
    });
    contours
}

fn to_contour(spline: &Spline) -> Option<Contour> {
    if spline.is_empty() {
        return None;
    }
    let pt = |p: PointF64| (p.x as f32, p.y as f32);
    let segs = spline.points[1..].as_chunks::<3>().0.iter().map(|c| Seg::Cubic(pt(c[0]), pt(c[1]), pt(c[2]))).collect();
    Some(Contour { start: pt(spline.points[0]), segs, closed: true })
}

/// Pixels sampled when building a palette; the whole image is then assigned to it.
const PALETTE_SAMPLE: usize = 200_000;
const KMEANS_ROUNDS: usize = 6;

fn quantise(img: &RgbaImage, lab: &LabImage, kept: &Mask, count: usize) -> Vec<Swatch> {
    let kept_count = kept.bits().iter().filter(|k| **k).count();
    if kept_count == 0 {
        return Vec::new();
    }
    let stride = kept_count.div_ceil(PALETTE_SAMPLE).max(1);
    let sample: Vec<Lab> = lab.px.iter().zip(kept.bits()).filter(|(_, k)| **k).step_by(stride).map(|(p, _)| *p).collect();
    let mut centres = median_cut(&sample, count);
    for _ in 0..KMEANS_ROUNDS {
        let mut sums = vec![(Lab::default(), 0usize); centres.len()];
        for p in &sample {
            let k = nearest(&centres, *p);
            let (s, n) = &mut sums[k];
            s.l += p.l;
            s.a += p.a;
            s.b += p.b;
            *n += 1;
        }
        let next: Vec<Lab> =
            sums.iter().zip(&centres).map(|((s, n), old)| if *n == 0 { *old } else { Lab { l: s.l / *n as f32, a: s.a / *n as f32, b: s.b / *n as f32 } }).collect();
        let moved = next.iter().zip(&centres).any(|(a, b)| delta_e(*a, *b) > 0.01);
        centres = next;
        if !moved {
            break;
        }
    }
    let labels = assign(lab, kept, &centres);
    let mut swatches: Vec<(usize, Swatch)> = centres
        .iter()
        .enumerate()
        .filter_map(|(k, c)| {
            let n = labels.iter().filter(|l| **l == k as u8).count();
            (n > 0).then(|| (n, Swatch { lab: *c, colour: mean_colour(img, &labels, k as u8).unwrap_or(Color32::BLACK) }))
        })
        .collect();
    swatches.sort_by_key(|a| std::cmp::Reverse(a.0));
    swatches.into_iter().map(|(_, s)| s).collect()
}

/// Splits the box with the widest Lab axis at its median until `count` boxes exist or no box
/// has two distinct colours left.
fn median_cut(sample: &[Lab], count: usize) -> Vec<Lab> {
    let mut boxes: Vec<Vec<Lab>> = vec![sample.to_vec()];
    while boxes.len() < count {
        let widest = boxes.iter().enumerate().map(|(i, b)| (i, extent(b))).filter(|(_, (_, range))| *range > 1e-3).max_by(|a, b| a.1.1.total_cmp(&b.1.1));
        let Some((i, (axis, _))) = widest else { break };
        let mut b = boxes.swap_remove(i);
        b.sort_by(|p, q| component(p, axis).total_cmp(&component(q, axis)));
        let tail = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(tail);
    }
    boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let n = b.len() as f32;
            Lab { l: b.iter().map(|p| p.l).sum::<f32>() / n, a: b.iter().map(|p| p.a).sum::<f32>() / n, b: b.iter().map(|p| p.b).sum::<f32>() / n }
        })
        .collect()
}

fn component(p: &Lab, axis: usize) -> f32 {
    [p.l, p.a, p.b][axis]
}

/// The axis with the largest range in the box and that range.
fn extent(b: &[Lab]) -> (usize, f32) {
    (0..3)
        .map(|axis| {
            let (lo, hi) = b.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(component(p, axis)), hi.max(component(p, axis))));
            (axis, hi - lo)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, 0.0))
}

fn nearest(centres: &[Lab], p: Lab) -> usize {
    centres.iter().enumerate().map(|(i, c)| (i, delta_e(*c, p))).min_by(|a, b| a.1.total_cmp(&b.1)).map_or(0, |(i, _)| i)
}

/// Per-pixel index of the nearest centre; `u8::MAX` for removed pixels.
fn assign(lab: &LabImage, kept: &Mask, centres: &[Lab]) -> Vec<u8> {
    lab.px.iter().zip(kept.bits()).map(|(p, k)| if *k && !centres.is_empty() { nearest(centres, *p) as u8 } else { u8::MAX }).collect()
}

fn mean_colour(img: &RgbaImage, labels: &[u8], label: u8) -> Option<Color32> {
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for (p, l) in img.pixels().zip(labels) {
        if *l == label {
            for (s, c) in sum.iter_mut().zip(&p.0[..3]) {
                *s += *c as u64;
            }
            n += 1;
        }
    }
    (n > 0).then(|| Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8))
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{self, Boolean, Ring};
    use crate::trace::fixtures as fx;
    use crate::trace::raster::{Recipe, Step, clean, downscale};

    fn settings(colours: Colours, layering: Layering, speckle_px: u32) -> TraceSettings {
        TraceSettings { colours, layering, speckle_px, ..TraceSettings::default() }
    }

    fn rings(contours: &[Contour]) -> Vec<Ring> {
        contours.iter().map(|c| geometry::flatten(c, 0.05)).collect()
    }

    fn area(contours: &[Contour]) -> f32 {
        geometry::even_odd_area(&rings(contours))
    }

    fn boolean_area(a: &[Contour], b: &[Contour], op: Boolean) -> f32 {
        geometry::even_odd_area(&geometry::boolean(&[rings(a), rings(b)], op))
    }

    fn lab_of(c: Color32) -> Lab {
        raster::lab([c.r(), c.g(), c.b()])
    }

    fn closest_source(c: Color32) -> f32 {
        [fx::BLUE, fx::GREEN, fx::RED].iter().map(|s| delta_e(lab_of(c), raster::lab(*s))).fold(f32::MAX, f32::min)
    }

    fn nearest_source(c: Color32) -> [u8; 3] {
        *[fx::BLUE, fx::GREEN, fx::RED].iter().min_by(|a, b| delta_e(lab_of(c), raster::lab(**a)).total_cmp(&delta_e(lab_of(c), raster::lab(**b)))).unwrap()
    }

    fn point_to_segment((px, py): (f32, f32), (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { ((px - ax) * dx + (py - ay) * dy) / len2 } else { 0.0 }.clamp(0.0, 1.0);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    }

    fn point_to_ring(p: (f32, f32), ring: &[(f32, f32)]) -> f32 {
        (0..ring.len()).map(|i| point_to_segment(p, ring[i], ring[(i + 1) % ring.len()])).fold(f32::MAX, f32::min)
    }

    #[test]
    fn threshold_trace_of_a_two_inch_square_is_one_closed_contour_within_0_02_in() {
        let img = fx::load("square");
        let layers = trace(&img, &settings(Colours::Threshold { level: 128, invert: false }, Layering::Stacked, 4), &Cancel::never()).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].colour, Color32::BLACK);
        assert_eq!(layers[0].contours.len(), 1, "{:?}", layers[0].contours);
        let contour = &layers[0].contours[0];
        assert!(contour.closed);
        assert!(contour.segs.iter().any(|s| matches!(s, Seg::Cubic(..))), "spline segments");
        let (x0, x1, y0, y1) = fx::SQUARE;
        let (x0, x1, y0, y1) = (x0 as f32, x1 as f32, y0 as f32, y1 as f32);
        let square = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        let traced = geometry::flatten(contour, 0.05);
        let tolerance_px = 0.02 * 100.0;
        let traced_to_square = traced.iter().map(|p| point_to_ring(*p, &square)).fold(0.0, f32::max);
        let square_to_traced = (0..800)
            .map(|i| {
                let t = (i % 200) as f32;
                match i / 200 {
                    0 => (x0 + t, y0),
                    1 => (x1, y0 + t),
                    2 => (x1 - t, y1),
                    _ => (x0, y1 - t),
                }
            })
            .map(|p| point_to_ring(p, &traced))
            .fold(0.0, f32::max);
        assert!(traced_to_square <= tolerance_px, "traced outline strays {traced_to_square} px from the square");
        assert!(square_to_traced <= tolerance_px, "square strays {square_to_traced} px from the traced outline");
    }

    #[test]
    fn threshold_invert_traces_the_light_pixels() {
        let img = fx::load("square");
        let layers = trace(&img, &settings(Colours::Threshold { level: 128, invert: true }, Layering::Stacked, 4), &Cancel::never()).unwrap();
        let outer = (img.width() * img.height()) as f32;
        assert!((area(&layers[0].contours) - (outer - 200.0 * 200.0)).abs() / outer < 0.01, "white frame with a square hole");
    }

    #[test]
    fn posterise_three_colours_gives_three_layers_filled_within_lab_3_of_the_source() {
        let img = fx::load("three");
        let layers = trace(&img, &settings(Colours::Posterise(palette(&img, 3)), Layering::SideBySide, 4), &Cancel::never()).unwrap();
        assert_eq!(layers.len(), 3);
        for layer in &layers {
            assert!(closest_source(layer.colour) < 3.0, "{:?} is not a source colour", layer.colour);
            assert!(!layer.contours.is_empty());
        }
        let mut nearest: Vec<[u8; 3]> = layers.iter().map(|l| nearest_source(l.colour)).collect();
        nearest.sort();
        let mut sources = [fx::BLUE, fx::GREEN, fx::RED];
        sources.sort();
        assert_eq!(nearest, sources, "one layer per source colour");
    }

    #[test]
    fn palette_lists_the_source_colours_largest_first_and_merging_folds_a_layer() {
        let img = fx::load("three");
        let swatches = palette(&img, 3).swatches;
        assert_eq!(swatches.len(), 3);
        let nearest: Vec<[u8; 3]> = swatches.iter().map(|s| nearest_source(s.colour)).collect();
        assert_eq!(nearest, [fx::BLUE, fx::GREEN, fx::RED], "largest first");
        assert!(swatches.iter().all(|s| closest_source(s.colour) < 3.0));
        assert_eq!(palette(&img, 1).swatches.len(), 2, "count clamps to at least 2");

        let red = swatches[2];
        let folded =
            trace(&img, &settings(Colours::Posterise(Palette { swatches: swatches.clone(), merged: vec![2] }), Layering::SideBySide, 4), &Cancel::never()).unwrap();
        assert_eq!(
            folded.iter().map(|l| l.colour).collect::<Vec<_>>(),
            vec![swatches[0].colour, swatches[1].colour],
            "red folds into a neighbour, which keeps its colour"
        );
        let unmerged =
            trace(&img, &settings(Colours::Posterise(Palette { swatches: swatches.clone(), merged: vec![] }), Layering::SideBySide, 4), &Cancel::never()).unwrap();
        let red_area = area(&unmerged[2].contours);
        let target = usize::from(delta_e(lab_of(red.colour), lab_of(swatches[0].colour)) > delta_e(lab_of(red.colour), lab_of(swatches[1].colour)));
        let grown = area(&folded[target].contours) - area(&unmerged[target].contours);
        assert!((grown - red_area).abs() / red_area < 0.05, "the red pixels ({red_area} px) went to swatch {target}, which grew by {grown} px");
    }

    #[test]
    fn stacked_layers_nest_each_containing_the_one_above() {
        let img = fx::load("three");
        let layers = trace(&img, &settings(Colours::Posterise(palette(&img, 3)), Layering::Stacked, 4), &Cancel::never()).unwrap();
        assert_eq!(layers.len(), 3);
        let whole = (img.width() * img.height()) as f32;
        assert!((area(&layers[0].contours) - whole).abs() / whole < 0.01, "bottom layer is the whole image");
        for pair in layers.windows(2) {
            let outside = boolean_area(&pair[1].contours, &pair[0].contours, Boolean::Subtract);
            assert!(outside / whole < 0.001, "{outside} px of the upper layer lie outside the lower one");
            assert!(area(&pair[1].contours) < area(&pair[0].contours));
        }
    }

    #[test]
    fn side_by_side_layers_do_not_overlap() {
        let img = fx::load("three");
        let layers = trace(&img, &settings(Colours::Posterise(palette(&img, 3)), Layering::SideBySide, 4), &Cancel::never()).unwrap();
        assert_eq!(layers.len(), 3);
        let whole = (img.width() * img.height()) as f32;
        for i in 0..layers.len() {
            for j in i + 1..layers.len() {
                let overlap = boolean_area(&layers[i].contours, &layers[j].contours, Boolean::Intersect);
                assert!(overlap / whole < 0.001, "layers {i} and {j} overlap by {overlap} px");
            }
        }
        let total: f32 = layers.iter().map(|l| area(&l.contours)).sum();
        assert!(total / whole > 0.97 && total / whole < 1.001, "layers tile the image up to one-pixel seams: {total} of {whole}");
    }

    #[test]
    fn speckle_20_drops_a_3_px_dot_and_keeps_a_40_px_dot() {
        let img = fx::load("speckle");
        let threshold = Colours::Threshold { level: 128, invert: false };
        let kept = trace(&img, &settings(threshold.clone(), Layering::Stacked, 20), &Cancel::never()).unwrap();
        assert_eq!(kept[0].contours.len(), 1);
        let big = std::f32::consts::PI * fx::BIG_DOT.1 * fx::BIG_DOT.1;
        assert!((area(&kept[0].contours) - big).abs() / big < 0.05, "the 40 px dot survives at its size");
        let both = trace(&img, &settings(threshold, Layering::Stacked, 1), &Cancel::never()).unwrap();
        assert_eq!(both[0].contours.len(), 2, "without speckle removal both dots trace");
    }

    #[test]
    fn silhouette_outlines_the_kept_pixels_after_cleaning() {
        let cleaned = clean(&fx::load("disc"), &Recipe { steps: vec![Step::AutoRemove], edges: false });
        let contours = silhouette(&cleaned, &TraceSettings::default(), &Cancel::never()).unwrap();
        assert_eq!(contours.len(), 1, "the enclosed island is kept, so no hole");
        let disc = std::f32::consts::PI * fx::DISC_RADIUS * fx::DISC_RADIUS;
        assert!((area(&contours) - disc).abs() / disc < 0.03);
    }

    #[test]
    fn cancel_returns_cancelled() {
        let img = fx::load("three");
        let cancel = Cancel::new(Arc::new(AtomicU64::new(1)), 0);
        assert_eq!(trace(&img, &settings(Colours::Posterise(palette(&img, 3)), Layering::Stacked, 4), &cancel), Err(Cancelled));
        assert_eq!(trace(&img, &TraceSettings::default(), &cancel), Err(Cancelled));
        assert_eq!(silhouette(&img, &TraceSettings::default(), &cancel), Err(Cancelled));
    }

    #[test]
    fn more_than_u16_max_components_and_holes_trace_without_visioncortex_clustering() {
        let pitch = 5;
        let blocks = 260;
        let side = pitch * blocks;
        let mask = Mask::from_fn(side, side, |x, y| x % pitch < 4 && y % pitch < 4);
        let contours = trace_mask(&mask, &TraceSettings::default());
        assert_eq!(contours.len(), (blocks * blocks) as usize, "one contour per 4 px block");
        let sieve = Mask::from_fn(side, side, |x, y| !(x % pitch < 2 && y % pitch < 2) || x < pitch || y < pitch);
        let contours = trace_mask(&sieve, &TraceSettings { speckle_px: 1, ..TraceSettings::default() });
        assert_eq!(contours.len(), 1 + ((blocks - 1) * (blocks - 1)) as usize, "the plate and one contour per hole");
    }

    #[test]
    fn fully_transparent_image_traces_to_nothing() {
        let img = RgbaImage::new(32, 32);
        let layers = trace(&img, &settings(Colours::Posterise(palette(&img, 3)), Layering::Stacked, 4), &Cancel::never()).unwrap();
        assert!(layers.is_empty());
        assert!(silhouette(&img, &TraceSettings::default(), &Cancel::never()).unwrap().is_empty());
        assert!(palette(&img, 3).swatches.is_empty());
    }

    #[test]
    #[ignore = "timing; run in release: cargo test --release preview_timing -- --ignored --nocapture"]
    fn preview_timing() {
        let full = fx::three_fixture(1600);
        let recipe = Recipe { steps: vec![Step::AutoRemove], edges: true };
        let start = std::time::Instant::now();
        let preview = downscale(&full, 768);
        let cleaned = clean(&preview, &recipe);
        let settings = settings(Colours::Posterise(palette(&cleaned, 3)), Layering::Stacked, 4);
        let layers = trace(&cleaned, &settings, &Cancel::never()).unwrap();
        let elapsed = start.elapsed();
        println!("preview path (downscale + clean + palette + trace, 768 px, {} layers): {:?}", layers.len(), elapsed);
        assert!(elapsed.as_millis() < 150);
    }
}

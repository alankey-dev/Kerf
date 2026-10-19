//! Pure pixel work for the trace panel's Background step: CIE Lab colour distance, flood fills,
//! automatic background detection, a Sobel edge mask, the erase and restore brushes, speckle
//! removal and downscaling. Nothing here knows about contours or visioncortex.

use image::RgbaImage;

/// A pixel in CIE L*a*b* (D65). `l` runs 0..100; CIE76 distance (`delta_e`) is what the
/// panel's Tolerance slider means.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// sRGB to CIE Lab under D65.
pub fn lab([r, g, b]: [u8; 3]) -> Lab {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    Lab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
}

/// CIE76 colour difference.
pub fn delta_e(p: Lab, q: Lab) -> f32 {
    ((p.l - q.l).powi(2) + (p.a - q.a).powi(2) + (p.b - q.b).powi(2)).sqrt()
}

/// Rec. 601 luminance of an sRGB pixel, 0..=255.
pub fn luminance([r, g, b]: [u8; 3]) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8
}

/// One bit per pixel, row major. Which meaning the bit carries (removed, foreground, edge) is up
/// to the function that fills it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    bits: Vec<bool>,
}

impl Mask {
    pub fn new(width: u32, height: u32, fill: bool) -> Self {
        Self { width, height, bits: vec![fill; width as usize * height as usize] }
    }

    pub fn from_fn(width: u32, height: u32, f: impl Fn(u32, u32) -> bool) -> Self {
        let bits = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).map(|(x, y)| f(x, y)).collect();
        Self { width, height, bits }
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    pub fn get(&self, x: u32, y: u32) -> bool {
        self.bits[self.idx(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, v: bool) {
        let i = self.idx(x, y);
        self.bits[i] = v;
    }

    pub fn bits(&self) -> &[bool] {
        &self.bits
    }

    /// Pixels the source left fully transparent.
    pub fn transparent(img: &RgbaImage) -> Self {
        Self { width: img.width(), height: img.height(), bits: img.pixels().map(|p| p.0[3] == 0).collect() }
    }

    fn fill_4(&mut self, seeds: impl IntoIterator<Item = (u32, u32)>, enter: impl Fn(u32, u32) -> bool) {
        let mut stack: Vec<(u32, u32)> = seeds.into_iter().filter(|&(x, y)| enter(x, y)).collect();
        while let Some((x, y)) = stack.pop() {
            if self.get(x, y) {
                continue;
            }
            self.set(x, y, true);
            for (nx, ny) in self.neighbours(x, y) {
                if !self.get(nx, ny) && enter(nx, ny) {
                    stack.push((nx, ny));
                }
            }
        }
    }

    pub fn neighbours(&self, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
        let (w, h) = (self.width, self.height);
        [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].into_iter().filter(move |&(nx, ny)| nx < w && ny < h)
    }

    /// Calls `visit` once per 4-connected run of `value` pixels with the run's pixels and its
    /// bounds (x0, y0, x1, y1, inclusive), in raster order of their first pixel.
    pub fn for_each_component(&self, value: bool, mut visit: impl FnMut(&[(u32, u32)], (u32, u32, u32, u32))) {
        let mut seen = Mask::new(self.width, self.height, false);
        let mut component = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if seen.get(x, y) || self.get(x, y) != value {
                    continue;
                }
                component.clear();
                let mut bounds = (x, y, x, y);
                let mut stack = vec![(x, y)];
                seen.set(x, y, true);
                while let Some((cx, cy)) = stack.pop() {
                    component.push((cx, cy));
                    bounds = (bounds.0.min(cx), bounds.1.min(cy), bounds.2.max(cx), bounds.3.max(cy));
                    for (nx, ny) in self.neighbours(cx, cy) {
                        if !seen.get(nx, ny) && self.get(nx, ny) == value {
                            seen.set(nx, ny, true);
                            stack.push((nx, ny));
                        }
                    }
                }
                visit(&component, bounds);
            }
        }
    }

    /// Flips every 4-connected run of `value` pixels smaller than `min_area` to the other value:
    /// first the islands (true), then the holes (false) the islands left.
    pub fn despeckle(&mut self, min_area: usize) {
        if min_area <= 1 {
            return;
        }
        for value in [true, false] {
            let mut small = Vec::new();
            self.for_each_component(value, |pixels, _| {
                if pixels.len() < min_area {
                    small.extend_from_slice(pixels);
                }
            });
            for (x, y) in small {
                self.set(x, y, !value);
            }
        }
    }
}

/// Every pixel of an image in Lab.
pub struct LabImage {
    pub width: u32,
    pub height: u32,
    pub px: Vec<Lab>,
}

impl LabImage {
    pub fn of(img: &RgbaImage) -> Self {
        Self { width: img.width(), height: img.height(), px: img.pixels().map(|p| lab([p.0[0], p.0[1], p.0[2]])).collect() }
    }

    pub fn get(&self, x: u32, y: u32) -> Lab {
        self.px[y as usize * self.width as usize + x as usize]
    }

    fn l_at(&self, x: i64, y: i64) -> f32 {
        let cx = x.clamp(0, self.width as i64 - 1) as u32;
        let cy = y.clamp(0, self.height as i64 - 1) as u32;
        self.get(cx, cy).l
    }
}

/// Sobel magnitude on L* at or above this marks an edge; a clean step of 10 L* between two
/// neighbouring pixels scores 40.
const EDGE_THRESHOLD: f32 = 40.0;

/// Pixels on a strong luminance edge (Sobel on L*). A flood never enters one.
pub fn edge_mask(lab: &LabImage) -> Mask {
    Mask::from_fn(lab.width, lab.height, |x, y| {
        let (x, y) = (x as i64, y as i64);
        let p = |dx: i64, dy: i64| lab.l_at(x + dx, y + dy);
        let gx = (p(1, -1) + 2.0 * p(1, 0) + p(1, 1)) - (p(-1, -1) + 2.0 * p(-1, 0) + p(-1, 1));
        let gy = (p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)) - (p(-1, -1) + 2.0 * p(0, -1) + p(1, -1));
        (gx * gx + gy * gy).sqrt() >= EDGE_THRESHOLD
    })
}

/// Marks as removed every pixel within `tolerance` (delta E) of the colour under `seed`:
/// 4-connected from the seed when `contiguous`, anywhere in the image otherwise. A contiguous
/// fill stops at `edges`.
pub fn flood_fill(lab: &LabImage, removed: &mut Mask, seed: (u32, u32), tolerance: f32, contiguous: bool, edges: Option<&Mask>) {
    let reference = lab.get(seed.0, seed.1);
    if contiguous {
        flood_from(lab, removed, [seed], reference, tolerance, edges);
    } else {
        for (i, p) in lab.px.iter().enumerate() {
            if delta_e(*p, reference) <= tolerance {
                removed.bits[i] = true;
            }
        }
    }
}

/// A flood never enters an edge pixel. Sobel marks the pixel on each side of a step, so once the
/// flood has settled it takes the one-pixel rim of edge pixels that match the reference colour
/// and touch the flooded area; the rim on the subject's side stays because nothing flooded
/// touches it.
fn flood_from(lab: &LabImage, removed: &mut Mask, seeds: impl IntoIterator<Item = (u32, u32)>, reference: Lab, tolerance: f32, edges: Option<&Mask>) {
    let within = |x: u32, y: u32| delta_e(lab.get(x, y), reference) <= tolerance;
    let Some(edges) = edges else {
        removed.fill_4(seeds, within);
        return;
    };
    removed.fill_4(seeds, |x, y| within(x, y) && !edges.get(x, y));
    let rim: Vec<(u32, u32)> = (0..removed.height)
        .flat_map(|y| (0..removed.width).map(move |x| (x, y)))
        .filter(|&(x, y)| edges.get(x, y) && !removed.get(x, y) && within(x, y) && removed.neighbours(x, y).any(|(nx, ny)| removed.get(nx, ny)))
        .collect();
    for (x, y) in rim {
        removed.set(x, y, true);
    }
}

/// Width of one Lab histogram bin when finding the dominant border colour.
const BORDER_BIN: f32 = 8.0;
/// Border pixels further than this from the dominant colour are foreground touching the frame.
const BORDER_FOREIGN: f32 = 30.0;
const AUTO_TOLERANCE_RANGE: (f32, f32) = (3.0, 40.0);

/// The background colour and tolerance Auto remove would use: the most common border colour
/// (mode of a coarse Lab histogram, then the mean of that bin) and a tolerance read off the
/// spread of the border pixels around it. `None` when no border pixel is opaque.
pub fn border_background(lab: &LabImage, removed: &Mask) -> Option<(Lab, f32)> {
    let border: Vec<Lab> = border_pixels(lab.width, lab.height).filter(|&(x, y)| !removed.get(x, y)).map(|(x, y)| lab.get(x, y)).collect();
    if border.is_empty() {
        return None;
    }
    let bin = |p: &Lab| ((p.l / BORDER_BIN).floor() as i32, (p.a / BORDER_BIN).floor() as i32, (p.b / BORDER_BIN).floor() as i32);
    let mut bins: Vec<((i32, i32, i32), usize)> = Vec::new();
    for p in &border {
        let b = bin(p);
        match bins.iter_mut().find(|(k, _)| *k == b) {
            Some((_, n)) => *n += 1,
            None => bins.push((b, 1)),
        }
    }
    let mode = bins.iter().max_by_key(|(_, n)| *n).map(|(k, _)| *k)?;
    let members: Vec<&Lab> = border.iter().filter(|p| bin(p) == mode).collect();
    let n = members.len() as f32;
    let dominant =
        Lab { l: members.iter().map(|p| p.l).sum::<f32>() / n, a: members.iter().map(|p| p.a).sum::<f32>() / n, b: members.iter().map(|p| p.b).sum::<f32>() / n };
    let mut spread: Vec<f32> = border.iter().map(|p| delta_e(*p, dominant)).filter(|d| *d <= BORDER_FOREIGN).collect();
    spread.sort_by(f32::total_cmp);
    let p90 = spread.get(spread.len() * 9 / 10).copied().unwrap_or(0.0);
    Some((dominant, (p90 * 1.5 + 3.0).clamp(AUTO_TOLERANCE_RANGE.0, AUTO_TOLERANCE_RANGE.1)))
}

fn border_pixels(width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let top = (0..width).map(|x| (x, 0));
    let bottom = (0..width).map(move |x| (x, height - 1));
    let left = (1..height.saturating_sub(1)).map(|y| (0, y));
    let right = (1..height.saturating_sub(1)).map(move |y| (width - 1, y));
    top.chain(bottom).chain(left).chain(right)
}

/// Floods inward from every border pixel near the dominant border colour, at the tolerance
/// `border_background` picks.
pub fn auto_remove(lab: &LabImage, removed: &mut Mask, edges: Option<&Mask>) {
    let Some((dominant, tolerance)) = border_background(lab, removed) else { return };
    let seeds: Vec<(u32, u32)> = border_pixels(lab.width, lab.height).collect();
    flood_from(lab, removed, seeds, dominant, tolerance, edges);
}

/// Stamps a disc of `radius_px` along `path` (pixel coordinates), marking pixels removed
/// (`erase`) or kept again.
pub fn brush(removed: &mut Mask, erase: bool, radius_px: f32, path: &[(f32, f32)]) {
    let step = (radius_px / 2.0).max(0.5);
    let mut stamp = |(cx, cy): (f32, f32)| {
        let r = radius_px.max(0.5);
        let (x0, x1) = (((cx - r).floor().max(0.0)) as u32, ((cx + r).ceil().min(removed.width as f32 - 1.0)) as u32);
        let (y0, y1) = (((cy - r).floor().max(0.0)) as u32, ((cy + r).ceil().min(removed.height as f32 - 1.0)) as u32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    removed.set(x, y, erase);
                }
            }
        }
    };
    match path {
        [] => {}
        [p] => stamp(*p),
        _ => {
            for pair in path.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let n = (len / step).ceil().max(1.0) as usize;
                for i in 0..=n {
                    let t = i as f32 / n as f32;
                    stamp((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                }
            }
        }
    }
}

/// What the user did in the Background step, replayed at any resolution. Points are fractions
/// of width and height (0..1); a brush radius is a fraction of the long side; so the 768 px
/// preview and the full-resolution trace apply the same edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    pub steps: Vec<Step>,
    /// Stop contiguous floods at Sobel edges.
    pub edges: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    RemoveColour { at: (f32, f32), tolerance: f32, contiguous: bool },
    AutoRemove,
    Brush { erase: bool, radius: f32, path: Vec<(f32, f32)> },
}

/// The pixels a recipe removes from `img` (true = removed). Source transparency counts as removed.
pub fn removed_by(img: &RgbaImage, recipe: &Recipe) -> Mask {
    let (w, h) = (img.width(), img.height());
    let mut removed = Mask::transparent(img);
    if w == 0 || h == 0 {
        return removed;
    }
    let lab = LabImage::of(img);
    let edges = recipe.edges.then(|| edge_mask(&lab));
    let to_px = |(fx, fy): (f32, f32)| ((fx * w as f32).floor().clamp(0.0, w as f32 - 1.0) as u32, (fy * h as f32).floor().clamp(0.0, h as f32 - 1.0) as u32);
    let long = w.max(h) as f32;
    for step in &recipe.steps {
        match step {
            Step::RemoveColour { at, tolerance, contiguous } => flood_fill(&lab, &mut removed, to_px(*at), *tolerance, *contiguous, edges.as_ref()),
            Step::AutoRemove => auto_remove(&lab, &mut removed, edges.as_ref()),
            Step::Brush { erase, radius, path } => {
                let path: Vec<(f32, f32)> = path.iter().map(|p| (p.0 * w as f32, p.1 * h as f32)).collect();
                brush(&mut removed, *erase, radius * long, &path);
            }
        }
    }
    removed
}

/// `img` with every pixel the recipe removes set to alpha 0.
pub fn clean(img: &RgbaImage, recipe: &Recipe) -> RgbaImage {
    let removed = removed_by(img, recipe);
    let mut out = img.clone();
    for (p, gone) in out.pixels_mut().zip(removed.bits()) {
        if *gone {
            p.0[3] = 0;
        }
    }
    out
}

/// A copy whose long side is at most `long_side` pixels (4096 for the full trace, 768 for the
/// preview). Smaller images come back unchanged.
pub fn downscale(img: &RgbaImage, long_side: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let long = w.max(h);
    if long <= long_side || long == 0 {
        return img.clone();
    }
    let scale = long_side as f32 / long as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::fixtures::{self as fx, in_disc};

    impl Mask {
        fn count(&self) -> usize {
            self.bits.iter().filter(|b| **b).count()
        }

        fn differing(&self, other: &Mask) -> usize {
            self.bits.iter().zip(&other.bits).filter(|(a, b)| a != b).count()
        }
    }

    fn disc_pixels(img: &RgbaImage) -> Mask {
        Mask::from_fn(img.width(), img.height(), |x, y| in_disc((x, y), fx::DISC_CENTRE, fx::DISC_RADIUS))
    }

    fn island_pixels(img: &RgbaImage) -> Mask {
        Mask::from_fn(img.width(), img.height(), |x, y| in_disc((x, y), fx::DISC_CENTRE, fx::ISLAND_RADIUS))
    }

    fn kept_everywhere(removed: &Mask, region: &Mask) -> bool {
        removed.bits().iter().zip(region.bits()).all(|(r, in_region)| !(*in_region && *r))
    }

    #[test]
    fn lab_endpoints_and_distance() {
        let white = lab([255, 255, 255]);
        let black = lab([0, 0, 0]);
        assert!((white.l - 100.0).abs() < 0.01 && white.a.abs() < 0.01 && white.b.abs() < 0.01);
        assert!(black.l.abs() < 0.01);
        assert!((delta_e(white, black) - 100.0).abs() < 0.01);
        assert_eq!(luminance([255, 255, 255]), 255);
        assert_eq!(luminance([0, 0, 0]), 0);
    }

    #[test]
    fn flood_fill_tolerance_10_removes_background_and_keeps_every_disc_pixel() {
        let img = fx::load("disc");
        let lab = LabImage::of(&img);
        let mut removed = Mask::transparent(&img);
        flood_fill(&lab, &mut removed, (0, 0), 10.0, true, None);
        let disc = disc_pixels(&img);
        let background = Mask::from_fn(img.width(), img.height(), |x, y| !disc.get(x, y));
        assert_eq!(removed, background, "contiguous fill removes exactly the outside");
        assert!(kept_everywhere(&removed, &disc));
        assert!(kept_everywhere(&removed, &island_pixels(&img)));
    }

    #[test]
    fn flood_fill_contiguous_off_also_removes_the_detached_white_island() {
        let img = fx::load("disc");
        let lab = LabImage::of(&img);
        let mut removed = Mask::transparent(&img);
        flood_fill(&lab, &mut removed, (0, 0), 10.0, false, None);
        let island = island_pixels(&img);
        assert!(island.bits().iter().zip(removed.bits()).all(|(i, r)| !*i || *r), "island removed");
        let red = Mask::from_fn(img.width(), img.height(), |x, y| img.get_pixel(x, y).0[..3] == fx::RED);
        assert!(kept_everywhere(&removed, &red));
    }

    #[test]
    fn auto_remove_matches_the_flood_fill_within_one_percent() {
        let img = fx::load("disc");
        let lab = LabImage::of(&img);
        let mut flooded = Mask::transparent(&img);
        flood_fill(&lab, &mut flooded, (0, 0), 10.0, true, None);
        let mut auto = Mask::transparent(&img);
        auto_remove(&lab, &mut auto, None);
        let total = (img.width() * img.height()) as f32;
        assert!(auto.differing(&flooded) as f32 / total < 0.01, "{} pixels differ", auto.differing(&flooded));
        let (dominant, tolerance) = border_background(&lab, &Mask::transparent(&img)).unwrap();
        assert!(delta_e(dominant, super::lab([255, 255, 255])) < 0.5);
        assert!((AUTO_TOLERANCE_RANGE.0..=AUTO_TOLERANCE_RANGE.1).contains(&tolerance));
    }

    #[test]
    fn auto_remove_on_a_fully_transparent_border_removes_nothing() {
        let mut img = fx::load("disc");
        for p in img.pixels_mut() {
            p.0[3] = 0;
        }
        let lab = LabImage::of(&img);
        let before = Mask::transparent(&img);
        let mut removed = before.clone();
        auto_remove(&lab, &mut removed, None);
        assert_eq!(removed, before);
    }

    #[test]
    fn edge_mask_stops_a_flood_that_would_leak_into_a_near_white_subject() {
        let img = fx::load("edge");
        let lab = LabImage::of(&img);
        let (a, b) = fx::EDGE_SQUARE;
        let interior = Mask::from_fn(img.width(), img.height(), |x, y| (a + 2..b - 2).contains(&x) && (a + 2..b - 2).contains(&y));
        let outside = Mask::from_fn(img.width(), img.height(), |x, y| !(a..b).contains(&x) || !(a..b).contains(&y));

        let mut leaked = Mask::transparent(&img);
        flood_fill(&lab, &mut leaked, (0, 0), 25.0, true, None);
        assert!(leaked.get(64, 64), "without the edge mask the flood eats the subject");

        let edges = edge_mask(&lab);
        let mut stopped = Mask::transparent(&img);
        flood_fill(&lab, &mut stopped, (0, 0), 25.0, true, Some(&edges));
        assert!(kept_everywhere(&stopped, &interior), "edge mask keeps the whole interior");
        let outside_removed = stopped.bits().iter().zip(outside.bits()).filter(|(r, o)| **r && **o).count();
        assert!(outside_removed as f32 / outside.count() as f32 > 0.9, "most of the background still goes");
    }

    #[test]
    fn brush_erases_and_restores_a_disc_along_a_path() {
        let mut removed = Mask::new(64, 64, false);
        brush(&mut removed, true, 4.0, &[(10.0, 32.0), (50.0, 32.0)]);
        assert!(removed.get(30, 32) && removed.get(10, 30) && !removed.get(30, 20));
        brush(&mut removed, false, 4.0, &[(30.0, 32.0)]);
        assert!(!removed.get(30, 32) && removed.get(12, 32));
    }

    #[test]
    fn despeckle_drops_small_islands_and_fills_small_holes() {
        let mut mask = Mask::from_fn(40, 40, |x, y| (10..30).contains(&x) && (10..30).contains(&y) || (x == 2 && y == 2));
        mask.set(20, 20, false);
        mask.despeckle(4);
        assert!(!mask.get(2, 2), "island dropped");
        assert!(mask.get(20, 20), "hole filled");
        assert_eq!(mask.count(), 400);
    }

    #[test]
    fn clean_with_normalised_steps_gives_the_same_mask_at_full_and_preview_resolution() {
        let full = fx::load("disc");
        let preview = downscale(&full, 64);
        assert_eq!((preview.width(), preview.height()), (64, 64));
        let recipe = Recipe {
            steps: vec![
                Step::RemoveColour { at: (0.02, 0.02), tolerance: 10.0, contiguous: true },
                Step::Brush { erase: true, radius: 0.08, path: vec![(0.3, 0.5), (0.7, 0.5)] },
                Step::Brush { erase: false, radius: 0.04, path: vec![(0.5, 0.5)] },
            ],
            edges: true,
        };
        let full_removed = removed_by(&full, &recipe);
        let preview_removed = removed_by(&preview, &recipe);
        let full_at_preview = Mask::from_fn(64, 64, |x, y| full_removed.get(x * 2, y * 2));
        let differing = full_at_preview.differing(&preview_removed);
        assert!(differing as f32 / (64.0 * 64.0) < 0.03, "{differing} of 4096 pixels differ");
        let cleaned = clean(&full, &recipe);
        assert_eq!(cleaned.get_pixel(0, 0).0[3], 0);
        assert_eq!(cleaned.get_pixel(64, 64).0[3], 255, "restored centre");
        assert_eq!(cleaned.get_pixel(48, 64).0[3], 0, "erased stroke");
        assert_eq!(cleaned.get_pixel(64, 30).0, full.get_pixel(64, 30).0, "untouched pixels keep their colour");
    }

    #[test]
    fn auto_remove_step_replays_through_clean() {
        let cleaned = clean(&fx::load("disc"), &Recipe { steps: vec![Step::AutoRemove], edges: false });
        assert_eq!(cleaned.get_pixel(0, 0).0[3], 0);
        assert_eq!(cleaned.get_pixel(64, 20).0[3], 255);
    }

    #[test]
    fn downscale_keeps_aspect_and_leaves_small_images_alone() {
        let img = RgbaImage::new(1000, 500);
        let small = downscale(&img, 768);
        assert_eq!((small.width(), small.height()), (768, 384));
        let same = downscale(&small, 4096);
        assert_eq!((same.width(), same.height()), (768, 384));
    }
}

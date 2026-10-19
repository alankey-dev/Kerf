//! The Knife and Scissors cut (spec 57): a drawn line splits closed shapes into area pieces and cuts
//! open paths into separate open paths. All pure, in world inches.
//!
//! A closed shape is split by subtracting a hairline band laid along the line, then grouping the result
//! into connected pieces. The band is thin enough (`BAND_HALF_IN` each side) that each cut edge lands
//! within a thousandth of an inch of the line and the two halves meet along it. An open path is cut at
//! each point where the line crosses it, keeping the path's own vertices.

use crate::design::Pt;
use crate::geometry::{Boolean, Ring, boolean, contains, signed_area};

/// Half the width of the cut band, in inches. The full band is a five-thousandth wide, so a cut edge
/// sits within that of the drawn line.
pub const BAND_HALF_IN: f32 = 0.00025;

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

/// The unit perpendicular to `d`, or `None` when `d` is degenerate.
fn normal(d: Pt) -> Option<Pt> {
    let len = d.0.hypot(d.1);
    (len > 1e-9).then(|| (-d.1 / len, d.0 / len))
}

/// A thin closed band along `line`, as the union of one quad per segment. Each quad is the segment
/// thickened by `half` on both sides and extended by `half` along its length so neighbours overlap.
pub fn band(line: &[Pt], half: f32) -> Vec<Ring> {
    let mut quads: Vec<Vec<Ring>> = Vec::new();
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = sub(b, a);
        let Some(n) = normal(d) else { continue };
        let len = d.0.hypot(d.1);
        let ext = (d.0 / len * half, d.1 / len * half);
        let (a, b) = ((a.0 - ext.0, a.1 - ext.1), (b.0 + ext.0, b.1 + ext.1));
        let off = (n.0 * half, n.1 * half);
        quads.push(vec![vec![(a.0 + off.0, a.1 + off.1), (b.0 + off.0, b.1 + off.1), (b.0 - off.0, b.1 - off.1), (a.0 - off.0, a.1 - off.1)]]);
    }
    if quads.is_empty() {
        return Vec::new();
    }
    boolean(&quads, Boolean::Weld)
}

/// Splits closed `rings` along `line` into connected pieces, each a set of rings (an outer ring with its
/// holes). Fewer than two pieces means the line did not divide the shape, and the caller leaves it be.
pub fn split_closed(rings: &[Ring], line: &[Pt]) -> Vec<Vec<Ring>> {
    let band = band(line, BAND_HALF_IN);
    if band.is_empty() {
        return Vec::new();
    }
    let cut = boolean(&[rings.to_vec(), band], Boolean::Subtract);
    group_pieces(&cut)
}

/// Groups a flat ring set into pieces: each outer ring with the holes it contains.
fn group_pieces(rings: &[Ring]) -> Vec<Vec<Ring>> {
    let holes: Vec<bool> = (0..rings.len()).map(|i| is_hole(i, rings)).collect();
    let mut pieces: Vec<Vec<Ring>> = rings.iter().enumerate().filter(|(i, _)| !holes[*i]).map(|(_, r)| vec![r.clone()]).collect();
    for (h, _) in rings.iter().enumerate().filter(|(i, _)| holes[*i]) {
        let c = centroid(&rings[h]);
        if let Some(piece) = pieces.iter_mut().find(|p| contains(&p[0], c)) {
            piece.push(rings[h].clone());
        }
    }
    pieces
}

fn centroid(ring: &[Pt]) -> Pt {
    let n = ring.len().max(1) as f32;
    (ring.iter().map(|p| p.0).sum::<f32>() / n, ring.iter().map(|p| p.1).sum::<f32>() / n)
}

fn is_hole(i: usize, rings: &[Ring]) -> bool {
    let area_i = signed_area(&rings[i]).abs();
    let c = centroid(&rings[i]);
    let depth = rings.iter().enumerate().filter(|(j, o)| *j != i && signed_area(o).abs() > area_i && contains(o, c)).count();
    depth % 2 == 1
}

/// Cuts open `path` (a polyline) at every point where `line` crosses it, into separate polylines. The
/// path keeps its own vertices; a crossing point ends one piece and starts the next. With no crossing
/// the whole path comes back as the one piece.
pub fn cut_open(path: &[Pt], line: &[Pt]) -> Vec<Vec<Pt>> {
    if path.len() < 2 {
        return vec![path.to_vec()];
    }
    // Crossings as (segment index, t along that segment, point), sorted along the path.
    let mut crossings: Vec<(usize, f32, Pt)> = Vec::new();
    for (i, pw) in path.windows(2).enumerate() {
        for lw in line.windows(2) {
            if let Some((t, pt)) = segment_intersection(pw[0], pw[1], lw[0], lw[1]) {
                crossings.push((i, t, pt));
            }
        }
    }
    crossings.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal));
    crossings.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 1e-6);
    if crossings.is_empty() {
        return vec![path.to_vec()];
    }
    let mut pieces: Vec<Vec<Pt>> = Vec::new();
    let mut current: Vec<Pt> = vec![path[0]];
    let mut next = 0;
    for (i, pw) in path.windows(2).enumerate() {
        while next < crossings.len() && crossings[next].0 == i {
            let pt = crossings[next].2;
            current.push(pt);
            pieces.push(std::mem::replace(&mut current, vec![pt]));
            next += 1;
        }
        current.push(pw[1]);
    }
    pieces.push(current);
    pieces.into_iter().filter(|p| p.len() >= 2).collect()
}

/// Scissors (spec 57): a click at `at` opens a closed polyline at its nearest vertex into one open
/// polyline, or cuts an open polyline in two at its nearest edge point. `closed` says which `path` is.
pub fn scissors(path: &[Pt], closed: bool, at: Pt) -> Vec<Vec<Pt>> {
    if path.len() < 2 {
        return vec![path.to_vec()];
    }
    if closed {
        // Open the loop at the nearest vertex: one open path from that node round and back to it.
        let k = (0..path.len()).min_by(|&i, &j| dist(path[i], at).total_cmp(&dist(path[j], at))).unwrap_or(0);
        let mut out: Vec<Pt> = (0..=path.len()).map(|o| path[(k + o) % path.len()]).collect();
        out.dedup();
        return vec![out];
    }
    // Cut the open path in two at the nearest point on its nearest segment.
    let Some((seg, foot)) = nearest_on_path(path, at) else { return vec![path.to_vec()] };
    let mut first: Vec<Pt> = path[..=seg].to_vec();
    first.push(foot);
    let mut second: Vec<Pt> = vec![foot];
    second.extend_from_slice(&path[seg + 1..]);
    [first, second].into_iter().filter(|p| p.len() >= 2).collect()
}

fn dist(a: Pt, b: Pt) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The segment index and foot point of the closest point on `path` to `at`.
fn nearest_on_path(path: &[Pt], at: Pt) -> Option<(usize, Pt)> {
    path.windows(2)
        .enumerate()
        .map(|(i, w)| {
            let foot = project(at, w[0], w[1]);
            (i, foot, dist(foot, at))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(i, foot, _)| (i, foot))
}

/// The point on segment `ab` nearest `p`.
fn project(p: Pt, a: Pt, b: Pt) -> Pt {
    let ab = sub(b, a);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 < 1e-12 {
        return a;
    }
    let t = ((p.0 - a.0) * ab.0 + (p.1 - a.1) * ab.1) / len2;
    let t = t.clamp(0.0, 1.0);
    (a.0 + t * ab.0, a.1 + t * ab.1)
}

/// The shortest distance from `at` to any edge of `path` (a closed ring or open polyline).
pub fn distance_to(path: &[Pt], closed: bool, at: Pt) -> f32 {
    let mut pts = path.to_vec();
    if closed && pts.len() > 1 {
        pts.push(pts[0]);
    }
    pts.windows(2).map(|w| dist(project(at, w[0], w[1]), at)).fold(f32::INFINITY, f32::min)
}

/// Where segment `a0a1` crosses `b0b1`, as the parameter `t` along `a0a1` and the point, if they cross
/// strictly between the ends.
fn segment_intersection(a0: Pt, a1: Pt, b0: Pt, b1: Pt) -> Option<(f32, Pt)> {
    let r = sub(a1, a0);
    let s = sub(b1, b0);
    let denom = r.0 * s.1 - r.1 * s.0;
    if denom.abs() < 1e-12 {
        return None;
    }
    let qp = sub(b0, a0);
    let t = (qp.0 * s.1 - qp.1 * s.0) / denom;
    let u = (qp.0 * r.1 - qp.1 * r.0) / denom;
    (t > 1e-6 && t < 1.0 - 1e-6 && (-1e-6..=1.0 + 1e-6).contains(&u)).then_some((t, (a0.0 + t * r.0, a0.1 + t * r.1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::even_odd_area;

    fn square(x: f32, y: f32, s: f32) -> Ring {
        vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)]
    }

    #[test]
    fn a_straight_line_splits_a_square_into_two_rectangles_sharing_the_cut() {
        let sq = vec![square(0.0, 0.0, 2.0)];
        let pieces = split_closed(&sq, &[(1.0, -0.5), (1.0, 2.5)]);
        assert_eq!(pieces.len(), 2, "two rectangles");
        for piece in &pieces {
            // Every vertex on the cut edge lies within a thousandth of the line x = 1.
            for &(x, _) in piece.iter().flatten().filter(|&&(x, _)| (x - 1.0).abs() < 0.1) {
                assert!((x - 1.0).abs() < 1e-3, "cut edge at x {x}");
            }
        }
        let sum: f32 = pieces.iter().map(|p| even_odd_area(p)).sum();
        assert!((sum - 4.0).abs() < 1e-2, "areas sum to the square: {sum}");
    }

    #[test]
    fn a_wavy_line_splits_a_square_into_two_pieces_covering_it() {
        let sq = vec![square(0.0, 0.0, 2.0)];
        let wavy: Vec<Pt> = (0..=20).map(|i| i as f32 / 20.0 * 2.0).map(|y| (1.0 + 0.3 * (y * 4.0).sin(), y - 0.1)).collect();
        let mut line = vec![(wavy[0].0, -0.5)];
        line.extend(wavy);
        line.push((1.0, 2.5));
        let pieces = split_closed(&sq, &line);
        assert_eq!(pieces.len(), 2, "two pieces");
        let sum: f32 = pieces.iter().map(|p| even_odd_area(p)).sum();
        assert!((sum - 4.0).abs() < 1e-2, "areas sum to the square: {sum}");
    }

    #[test]
    fn a_line_crossing_nothing_leaves_one_piece() {
        let sq = vec![square(0.0, 0.0, 2.0)];
        assert!(split_closed(&sq, &[(3.0, 0.0), (3.0, 2.0)]).len() < 2, "the shape is not divided");
    }

    #[test]
    fn scissors_opens_a_closed_ring_at_its_nearest_vertex() {
        let ring = square(0.0, 0.0, 2.0);
        let opened = scissors(&ring, true, (0.05, -0.05));
        assert_eq!(opened.len(), 1, "one open path");
        let path = &opened[0];
        assert_eq!(path.first(), path.last(), "it starts and ends at the opened node");
        assert!((path[0].0).abs() < 1e-4 && (path[0].1).abs() < 1e-4, "opened at the (0,0) corner: {:?}", path[0]);
    }

    #[test]
    fn scissors_cuts_an_open_path_in_two_at_the_click() {
        let path = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0)];
        let pieces = scissors(&path, false, (1.0, 0.1));
        assert_eq!(pieces.len(), 2, "two open paths");
        assert_eq!(pieces[0].last(), pieces[1].first(), "they meet at the cut point");
        assert!((pieces[0].last().unwrap().0 - 1.0).abs() < 1e-4, "cut near x = 1");
    }

    #[test]
    fn two_cuts_on_an_open_three_segment_path_give_three_paths_with_the_nodes() {
        // A three-segment open path: four nodes in a zig-zag.
        let path = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)];
        // Two vertical cut lines at x = 0.5 and x = 2.5.
        let pieces = cut_open(&path, &[(0.5, -1.0), (0.5, 2.0)]);
        assert_eq!(pieces.len(), 2, "one cut, two pieces");
        let pieces = {
            let mut all = cut_open(&path, &[(0.5, -1.0), (0.5, 2.0)]);
            all = all.into_iter().flat_map(|p| cut_open(&p, &[(2.5, -1.0), (2.5, 2.0)])).collect();
            all
        };
        assert_eq!(pieces.len(), 3, "two cuts, three pieces");
        // The original interior nodes survive somewhere in the pieces.
        let all: Vec<Pt> = pieces.iter().flatten().copied().collect();
        for node in [(1.0, 1.0), (2.0, 0.0)] {
            assert!(all.iter().any(|p| (p.0 - node.0).abs() < 1e-4 && (p.1 - node.1).abs() < 1e-4), "node {node:?} kept");
        }
    }
}

//! The planar map of several overlapping outlines, the pure piece behind Shape Builder (spec 57).
//!
//! Every closed region bounded by the inputs' edges is one [`Region`], tagged with which inputs cover
//! it. Built from the power set of the inputs: for each non-empty subset `S`, the region covered by
//! exactly `S` is the intersection of the inputs in `S` less the union of the rest. Each connected
//! outer ring of that becomes a region. The tool caps at eight inputs, so the 2^n - 1 subsets stay
//! small. Holes inside a region are themselves other regions of the map, so they are left out of the
//! outer ring; a region with a hole arises only when one input sits wholly inside another, which the
//! gesture tools do not rely on.

use super::{Boolean, Ring, boolean, contains, signed_area};
use crate::design::Pt;

/// One face of the planar map: its outer boundary, and the indices of the inputs that cover it.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub ring: Ring,
    /// The input indices covering this region, ascending.
    pub covered_by: Vec<usize>,
}

impl Region {
    /// A point strictly inside the region, for hit testing the gesture. The ring's centroid, nudged onto
    /// the inside when the centroid falls in a concavity.
    pub fn interior(&self) -> Pt {
        let n = self.ring.len().max(1) as f32;
        let c = (self.ring.iter().map(|p| p.0).sum::<f32>() / n, self.ring.iter().map(|p| p.1).sum::<f32>() / n);
        if contains(&self.ring, c) {
            return c;
        }
        // Fall back to the midpoint of the first edge nudged inwards, which a simple ring always contains.
        let (a, b) = (self.ring[0], self.ring[1 % self.ring.len()]);
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let inward = (-(b.1 - a.1), b.0 - a.0);
        let len = inward.0.hypot(inward.1).max(1e-6);
        let step = 1e-3;
        let sign = if signed_area(&self.ring) >= 0.0 { 1.0 } else { -1.0 };
        (mid.0 + sign * inward.0 / len * step, mid.1 + sign * inward.1 / len * step)
    }
}

/// The planar map of `outlines` (one ring group per input). Every non-empty coverage class that holds
/// area becomes one or more regions, one per connected outer ring.
pub fn planar_map(outlines: &[Vec<Ring>]) -> Vec<Region> {
    let n = outlines.len();
    if n == 0 {
        return Vec::new();
    }
    let mut regions = Vec::new();
    for mask in 1u32..(1u32 << n) {
        let covered_by: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        let rest: Vec<Vec<Ring>> = (0..n).filter(|i| mask & (1 << i) == 0).map(|i| outlines[i].clone()).collect();
        let inside: Vec<Vec<Ring>> = covered_by.iter().map(|&i| outlines[i].clone()).collect();
        let mut rings = boolean(&inside, Boolean::Intersect);
        if !rest.is_empty() {
            let rest_union = boolean(&rest, Boolean::Weld);
            if !rest_union.is_empty() {
                rings = boolean(&[rings, rest_union], Boolean::Subtract);
            }
        }
        for ring in outer_rings(&rings) {
            if ring.len() >= 3 {
                regions.push(Region { ring, covered_by: covered_by.clone() });
            }
        }
    }
    regions
}

/// The outer rings of a boolean result, dropping holes (a hole is a region of its own elsewhere in the
/// map). A ring is a hole when its centroid lies inside an odd number of the strictly larger rings.
fn outer_rings(rings: &[Ring]) -> Vec<Ring> {
    rings.iter().enumerate().filter(|(i, _)| !is_hole(*i, rings)).map(|(_, r)| r.clone()).collect()
}

fn is_hole(i: usize, rings: &[Ring]) -> bool {
    let area_i = signed_area(&rings[i]).abs();
    let centroid = {
        let n = rings[i].len().max(1) as f32;
        (rings[i].iter().map(|p| p.0).sum::<f32>() / n, rings[i].iter().map(|p| p.1).sum::<f32>() / n)
    };
    let depth = rings.iter().enumerate().filter(|(j, o)| *j != i && signed_area(o).abs() > area_i && contains(o, centroid)).count();
    depth % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::even_odd_area;

    /// A circle of radius `r` centred at `(cx, cy)`, as a 64-gon.
    fn circle(cx: f32, cy: f32, r: f32) -> Ring {
        (0..64).map(|i| i as f32 / 64.0 * std::f32::consts::TAU).map(|a| (cx + r * a.cos(), cy + r * a.sin())).collect()
    }

    fn square(x: f32, y: f32, s: f32) -> Ring {
        vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)]
    }

    fn covers(regions: &[Region], set: &[usize]) -> Option<usize> {
        regions.iter().position(|r| r.covered_by == set)
    }

    #[test]
    fn two_overlapping_circles_make_three_regions() {
        let a = vec![circle(0.0, 0.0, 1.0)];
        let b = vec![circle(1.0, 0.0, 1.0)];
        let regions = planar_map(&[a.clone(), b.clone()]);
        assert_eq!(regions.len(), 3, "three faces");
        assert!(covers(&regions, &[0]).is_some() && covers(&regions, &[1]).is_some() && covers(&regions, &[0, 1]).is_some());
        let sum: f32 = regions.iter().map(|r| even_odd_area(std::slice::from_ref(&r.ring))).sum();
        let union = even_odd_area(&boolean(&[a, b], Boolean::Weld));
        assert!((sum - union).abs() < 1e-4, "regions {sum} vs union {union}");
    }

    #[test]
    fn three_circles_in_a_triangle_make_seven_regions() {
        let r = 1.0;
        let a = vec![circle(0.0, 0.0, r)];
        let b = vec![circle(1.0, 0.0, r)];
        let c = vec![circle(0.5, 0.8, r)];
        let regions = planar_map(&[a, b, c]);
        assert_eq!(regions.len(), 7, "the seven-cell Venn diagram");
        assert!(covers(&regions, &[0, 1, 2]).is_some(), "a centre covered by all three");
    }

    #[test]
    fn two_disjoint_squares_make_two_regions() {
        let regions = planar_map(&[vec![square(0.0, 0.0, 1.0)], vec![square(3.0, 0.0, 1.0)]]);
        assert_eq!(regions.len(), 2, "no overlap cell");
        assert!(regions.iter().all(|r| r.covered_by.len() == 1));
    }

    #[test]
    fn uniting_every_region_of_two_circles_matches_unite() {
        let a = vec![circle(0.0, 0.0, 1.0)];
        let b = vec![circle(1.0, 0.0, 1.0)];
        let regions = planar_map(&[a.clone(), b.clone()]);
        let groups: Vec<Vec<Ring>> = regions.iter().map(|r| vec![r.ring.clone()]).collect();
        let united = boolean(&groups, Boolean::Weld);
        let unite = boolean(&[a, b], Boolean::Weld);
        assert!((even_odd_area(&united) - even_odd_area(&unite)).abs() < 0.01, "united {} vs Unite {}", even_odd_area(&united), even_odd_area(&unite));
    }

    #[test]
    fn deleting_the_lens_leaves_two_crescents() {
        let a = vec![circle(0.0, 0.0, 1.0)];
        let b = vec![circle(1.0, 0.0, 1.0)];
        let regions = planar_map(&[a.clone(), b.clone()]);
        let lens = even_odd_area(&boolean(&[a.clone(), b.clone()], Boolean::Intersect));
        let circle_area = even_odd_area(&a);
        let crescent = |set: &[usize]| even_odd_area(std::slice::from_ref(&regions[covers(&regions, set).unwrap()].ring));
        assert!((crescent(&[0]) - (circle_area - lens)).abs() < 1e-3, "crescent 0");
        assert!((crescent(&[1]) - (circle_area - lens)).abs() < 1e-3, "crescent 1");
    }

    #[test]
    fn every_region_interior_point_lies_in_its_ring() {
        let regions = planar_map(&[vec![circle(0.0, 0.0, 1.0)], vec![circle(1.0, 0.0, 1.0)]]);
        for region in &regions {
            assert!(contains(&region.ring, region.interior()), "interior of {:?}", region.covered_by);
        }
    }
}

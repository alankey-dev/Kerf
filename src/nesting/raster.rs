//! Bit grids: a piece's dilated footprint in one rotation, and the board it is placed on.

use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::Piece;
use crate::design::Pt;
use crate::snap::Aabb;

/// How far floor and ceil may round a cell count across float noise.
const SLACK: f32 = 1e-4;

/// The cell grid every footprint and board of one nest shares.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cells {
    pub size: f32,
    /// The dilation radius: half the spacing plus half a cell, so anti-aliased coverage cannot miss a
    /// sliver of the dilated outline.
    pub halo: f32,
}

/// A piece turned by `deg` and dilated by `halo`, as runs of marked cells per row. Its top-left cell
/// sits `halo` above and left of the turned piece's exact bounds.
pub(super) struct Footprint {
    pub deg: f32,
    /// The turned piece's exact bounds, relative to its centre.
    pub extent: Aabb,
    pub width: usize,
    pub height: usize,
    /// Inclusive column ranges of marked cells, per row.
    pub rows: Vec<Vec<(usize, usize)>>,
    pub cells: usize,
}

impl Footprint {
    pub fn of(piece: &Piece, deg: f32, cells: Cells, use_holes: bool) -> Footprint {
        let centre = piece_centre(piece);
        let turn = super::turner(deg, centre);
        let lines: Vec<(Vec<Pt>, bool)> = piece.lines.iter().map(|(pts, closed)| (pts.iter().map(|&p| turn(p)).collect(), *closed)).collect();
        let mut extent = super::turned_bounds(&piece.lines, deg, centre);
        if extent.is_empty() {
            extent = Aabb { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 };
        }
        let span = |len: f32| (((len + 2.0 * cells.halo) / cells.size) - SLACK).ceil().max(1.0) as usize;
        let (width, height) = (span(extent.width()), span(extent.height()));
        let (ox, oy) = (extent.min_x - cells.halo, extent.min_y - cells.halo);
        let to_px = |(x, y): Pt| ((x - ox) / cells.size, (y - oy) / cells.size);

        let mut pixmap = Pixmap::new(width as u32, height as u32).expect("a footprint is at least one cell");
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        paint.anti_alias = true;
        let path = |rings: &mut dyn Iterator<Item = &(Vec<Pt>, bool)>| {
            let mut pb = PathBuilder::new();
            for (pts, closed) in rings {
                let Some((&first, rest)) = pts.split_first() else { continue };
                let (x, y) = to_px(first);
                pb.move_to(x, y);
                if rest.is_empty() {
                    pb.line_to(x, y);
                }
                for &p in rest {
                    let (x, y) = to_px(p);
                    pb.line_to(x, y);
                }
                if *closed {
                    pb.close();
                }
            }
            pb.finish()
        };
        let closed = |l: &&(Vec<Pt>, bool)| l.1 && l.0.len() >= 3;
        if use_holes {
            if let Some(p) = path(&mut lines.iter().filter(closed)) {
                pixmap.fill_path(&p, &paint, FillRule::EvenOdd, Transform::identity(), None);
            }
        } else {
            for ring in lines.iter().filter(closed) {
                if let Some(p) = path(&mut std::iter::once(ring)) {
                    pixmap.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
        let stroke = Stroke { width: 2.0 * cells.halo / cells.size, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        if let Some(p) = path(&mut lines.iter()) {
            pixmap.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }

        let alpha = pixmap.data().as_chunks::<4>().0.iter().map(|px| px[3] > 0).collect::<Vec<_>>();
        let rows: Vec<Vec<(usize, usize)>> = alpha.chunks_exact(width).map(runs).collect();
        let cells = rows.iter().flatten().map(|(a, b)| b - a + 1).sum();
        Footprint { deg, extent, width, height, rows, cells }
    }
}

/// The centre a piece turns about: the centre of its lines' bounds.
pub(super) fn piece_centre(piece: &Piece) -> Pt {
    let b = super::turned_bounds(&piece.lines, 0.0, (0.0, 0.0));
    if b.is_empty() { (0.0, 0.0) } else { b.centre() }
}

fn runs(row: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &on) in row.iter().enumerate() {
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i - 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, row.len() - 1));
    }
    out
}

/// A mat's occupied cells: rows of `u64` words, bit `i` of word `k` being column `64 k + i`. It covers
/// the padded cutting area grown by the halo, so a placed outline may touch the padding but never cross it.
#[derive(Clone)]
pub(super) struct Board {
    pub width: usize,
    pub height: usize,
    words: usize,
    bits: Vec<u64>,
}

impl Board {
    /// A board spanning `(w, h)` inches of padded cutting area.
    pub fn new((w, h): Pt, cells: Cells) -> Board {
        let span = |len: f32| (((len + 2.0 * cells.halo) / cells.size) + SLACK).floor().max(0.0) as usize;
        let (width, height) = (span(w), span(h));
        let words = width.div_ceil(64).max(1);
        Board { width, height, words, bits: vec![0; words * height] }
    }

    pub fn holds(&self, fp: &Footprint) -> bool {
        fp.width <= self.width && fp.height <= self.height
    }

    /// The highest marked column in `lo..=hi` of `row`.
    fn highest(&self, row: usize, lo: usize, hi: usize) -> Option<usize> {
        let words = &self.bits[row * self.words..(row + 1) * self.words];
        let (lw, hw) = (lo / 64, hi / 64);
        for w in (lw..=hw).rev() {
            let mut word = words[w];
            if w == hw && hi % 64 != 63 {
                word &= (1u64 << (hi % 64 + 1)) - 1;
            }
            if w == lw {
                word &= !0u64 << (lo % 64);
            }
            if word != 0 {
                return Some(w * 64 + 63 - word.leading_zeros() as usize);
            }
        }
        None
    }

    /// `None` when `fp` fits with its top-left cell at `(x, y)`, or else the next column worth trying:
    /// every column up to it still covers the marked cell that blocks this one.
    fn blocked(&self, fp: &Footprint, x: usize, y: usize) -> Option<usize> {
        for (r, row) in fp.rows.iter().enumerate() {
            for &(a, b) in row {
                if let Some(c) = self.highest(y + r, x + a, x + b) {
                    return Some(c - a + 1);
                }
            }
        }
        None
    }

    /// The first free cell scanning rows then columns, giving up on rows whose placement would end
    /// below `stop(y)`.
    pub fn first_fit(&self, fp: &Footprint, stop: impl Fn(usize) -> bool) -> Option<(usize, usize)> {
        if !self.holds(fp) {
            return None;
        }
        for y in 0..=self.height - fp.height {
            if stop(y) {
                return None;
            }
            let mut x = 0;
            while x + fp.width <= self.width {
                match self.blocked(fp, x, y) {
                    None => return Some((x, y)),
                    Some(next) => x = next,
                }
            }
        }
        None
    }

    pub fn mark(&mut self, fp: &Footprint, x: usize, y: usize) {
        for (r, row) in fp.rows.iter().enumerate() {
            let words = &mut self.bits[(y + r) * self.words..(y + r + 1) * self.words];
            for &(a, b) in row {
                for c in x + a..=x + b {
                    words[c / 64] |= 1u64 << (c % 64);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELLS: Cells = Cells { size: 0.05, halo: 0.0875 };

    fn square(side: f32) -> Piece {
        let ring = vec![(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)];
        Piece { lines: vec![(ring, true)], grain_locked: false }
    }

    #[test]
    fn a_square_marks_its_dilated_box_and_nothing_far_outside() {
        let fp = Footprint::of(&square(1.0), 0.0, CELLS, true);
        assert_eq!((fp.width, fp.height), (24, 24), "1 in plus two halos, in 0.05 in cells");
        assert_eq!(fp.rows[12], [(0, 23)], "a middle row is solid");
        assert!(fp.cells > 20 * 20 && fp.cells <= 24 * 24);
    }

    #[test]
    fn holes_are_open_only_with_use_holes() {
        let mut frame = square(3.0);
        frame.lines.push((vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)], true));
        let open = Footprint::of(&frame, 0.0, CELLS, true);
        let solid = Footprint::of(&frame, 0.0, CELLS, false);
        let middle = open.height / 2;
        assert_eq!(open.rows[middle].len(), 2, "the hole splits the middle row: {:?}", open.rows[middle]);
        assert_eq!(solid.rows[middle].len(), 1);
    }

    #[test]
    fn highest_finds_the_last_marked_column_across_words() {
        let mut board = Board::new((10.0, 1.0), CELLS);
        let dot = Footprint { deg: 0.0, extent: Aabb { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 }, width: 1, height: 1, rows: vec![vec![(0, 0)]], cells: 1 };
        for c in [3, 63, 64, 130] {
            board.mark(&dot, c, 0);
        }
        assert_eq!(board.highest(0, 0, 200), Some(130));
        assert_eq!(board.highest(0, 0, 129), Some(64));
        assert_eq!(board.highest(0, 4, 63), Some(63));
        assert_eq!(board.highest(0, 65, 129), None);
        assert_eq!(board.highest(0, 0, 2), None);
    }
}

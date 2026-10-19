//! Sorts a design into mats: one mat per material colour, Print Then Cut mats first, attached shapes
//! kept together, and every placement unit of every project copy packed into the cutting area. The
//! Project Preview, Prepare and the bridge payload all read this output, so what the preview shows is
//! what the machine is sent.

use egui::Color32;

use crate::design::{Design, Operation, POINTS_PER_INCH, Pt};
use crate::effects::{Piece, PieceId, Style};
use crate::machine::{MAT_PADDING_PT, MatSize};
use crate::print_then_cut::{self, PaperSize, PrintArea};
use crate::snap::Aabb;

pub mod nested;

/// Space between packed units, in inches.
const UNIT_GAP_IN: f32 = 0.25;

#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
    pub print_then_cut: bool,
    /// The material colour, which fills the mat thumbnail.
    pub colour: Color32,
    pub items: Vec<MatItem>,
    /// Deduped, Print Then Cut first, then plot order.
    pub operations: Vec<Operation>,
    /// One unit too large for the mat, placed on a mat of its own.
    pub overflow: bool,
    /// The paper and printable area of a Print Then Cut page; `None` on cut mats.
    pub page: Option<PrintArea>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatItem {
    /// The evaluated piece this item cuts; `piece.owner` is its shape.
    pub piece: PieceId,
    /// Which project copy this is, counting from 0.
    pub copy: u32,
    /// The top-left of the turned piece's bounds on the cutting area, in inches.
    pub offset_in: Pt,
    /// Nesting turns the piece by this much about the centre of its world bounds, in the sense of
    /// `Matrix::rotate`, before placing it. 0 everywhere else.
    pub rotation_deg: f32,
}

impl Mat {
    /// "Score, Pen, Perforate".
    pub fn label(&self) -> String {
        self.operations.iter().map(|op| op.label()).collect::<Vec<_>>().join(", ")
    }
}

/// Cut operations set a unit's material colour; the rest take the colour of the cut shape they are attached to.
pub fn is_cut(op: Operation) -> bool {
    matches!(op, Operation::BasicCut | Operation::WavyCut | Operation::Perforate | Operation::PrintThenCut)
}

/// `_outputTypeToPlotOrder`, with Print Then Cut ahead of everything.
pub(crate) fn plot_order(op: Operation) -> u8 {
    match op {
        Operation::PrintThenCut => 0,
        Operation::Score => 1,
        Operation::Pen => 2,
        Operation::Deboss => 3,
        Operation::Engrave => 4,
        Operation::Foil(_) => 5,
        Operation::Perforate => 6,
        Operation::WavyCut => 7,
        Operation::BasicCut => 8,
        Operation::Guide => 9,
    }
}

/// What lands on a mat as one unit: the pieces of a cut container with its visible descendants, of an
/// Attach link's visible members, or of a lone shape. See `containers::placement_key`.
pub(crate) struct Unit {
    pieces: Vec<UnitPiece>,
    key: (bool, Color32),
    bounds: Aabb,
}

struct UnitPiece {
    id: PieceId,
    style: Style,
    bounds: Aabb,
}

impl Unit {
    pub(crate) fn size(&self) -> Pt {
        (self.bounds.width(), self.bounds.height())
    }
}

/// What keys a placement unit: a container or Attach link id, or one rigid unit of an instance's parts.
#[derive(Clone, Copy, PartialEq)]
enum Key {
    Id(u64),
    Part(crate::components::PartUnit),
}

fn units(design: &Design) -> Vec<Unit> {
    let mut groups: Vec<(Option<Key>, Vec<UnitPiece>)> = Vec::new();
    for s in design.drawn().filter(|s| !s.hidden) {
        let geometry = design.geometry(s);
        let kept: Vec<&Piece> = geometry.pieces().iter().filter(|p| p.style.operation != Operation::Guide).collect();
        let drawn_key = crate::containers::placement_key(design, s).map(Key::Id);
        let part_units = match (drawn_key, s.instance_of()) {
            (None, Some(_)) => crate::components::part_units(design, s, &kept),
            _ => Vec::new(),
        };
        for (i, p) in kept.into_iter().enumerate() {
            let piece = UnitPiece { id: p.id, style: p.style, bounds: p.bounds };
            // A child's piece inside a Compound keeps its own Attach unit, so a Pen label stays on its cut's mat.
            let owner = || design.shapes.iter().find(|o| o.id == piece.id.owner && o.id != s.id);
            let key = drawn_key
                .or_else(|| part_units.get(i).copied().map(Key::Part))
                .or_else(|| owner().and_then(|o| crate::containers::placement_key(design, o)).map(Key::Id));
            match groups.iter_mut().find(|(k, _)| key.is_some() && *k == key) {
                Some((_, members)) => members.push(piece),
                None => groups.push((key, vec![piece])),
            }
        }
    }
    groups
        .into_iter()
        .map(|(_, pieces)| {
            let anchor = pieces.iter().find(|p| is_cut(p.style.operation)).unwrap_or(&pieces[0]).style;
            // A group with any Print Then Cut layer goes to a print mat, and every print mat is "#ffffff"
            // (`docs/evidence-webapp.md`, "Print Then Cut composition", question 4).
            let key = match pieces.iter().any(|p| p.style.operation == Operation::PrintThenCut) {
                true => (true, Color32::WHITE),
                false => (false, anchor.fill),
            };
            let bounds = pieces.iter().map(|p| p.bounds).reduce(Aabb::union).unwrap();
            Unit { pieces, key, bounds }
        })
        .collect()
}

/// Shelf packing inside the padded cutting area: left to right, then a new row below the tallest unit.
struct Packer {
    min: f32,
    max: Pt,
    cursor: Pt,
    row_height: f32,
}

impl Packer {
    fn new(size: MatSize) -> Self {
        let pad = MAT_PADDING_PT / POINTS_PER_INCH;
        Self { min: pad, max: (size.width_in - pad, size.height_in - pad), cursor: (pad, pad), row_height: 0.0 }
    }

    fn fits_empty(&self, (w, h): Pt) -> bool {
        self.min + w <= self.max.0 && self.min + h <= self.max.1
    }

    /// The top-left for a unit of size `(w, h)`, or `None` when the mat is full.
    fn place(&mut self, (w, h): Pt) -> Option<Pt> {
        if self.cursor.0 > self.min && self.cursor.0 + w > self.max.0 {
            self.cursor = (self.min, self.cursor.1 + self.row_height + UNIT_GAP_IN);
            self.row_height = 0.0;
        }
        if self.cursor.1 + h > self.max.1 {
            return None;
        }
        let at = self.cursor;
        self.cursor.0 += w + UNIT_GAP_IN;
        self.row_height = self.row_height.max(h);
        Some(at)
    }
}

pub(crate) fn add_unit(mat: &mut Mat, unit: &Unit, copy: u32, (x, y): Pt) {
    for p in &unit.pieces {
        let (min_x, min_y) = (p.bounds.min_x, p.bounds.min_y);
        mat.items.push(MatItem { piece: p.id, copy, offset_in: (x + min_x - unit.bounds.min_x, y + min_y - unit.bounds.min_y), rotation_deg: 0.0 });
        if !mat.operations.contains(&p.style.operation) {
            mat.operations.push(p.style.operation);
        }
    }
}

/// The bounds of `piece`'s world lines turned by `deg` about the centre of `piece.bounds`, relative to
/// that centre. At 0 it is the bounds less their centre.
pub fn turned_extent(piece: &Piece, deg: f32) -> Aabb {
    let centre = piece.bounds.centre();
    if deg == 0.0 {
        return piece.bounds.translate(-centre.0, -centre.1);
    }
    crate::nesting::turned_bounds(&piece.lines, deg, centre)
}

pub fn compose(design: &Design, size: MatSize, copies: u32) -> Vec<Mat> {
    compose_on(design, size, copies, print_then_cut::LETTER)
}

/// Each of the `copies` project copies packs unit by unit like any other, spilling onto further mats.
/// Print Then Cut units go onto `paper` pages, ahead of every cut mat. A design whose optimised layout
/// is current takes its cut mats from the layout instead.
pub fn compose_on(design: &Design, size: MatSize, copies: u32, paper: PaperSize) -> Vec<Mat> {
    let units = units(design);
    let nested = nested::current(design, &units, size, copies);
    let mut keys: Vec<(bool, Color32)> = Vec::new();
    for unit in &units {
        if !keys.contains(&unit.key) {
            keys.push(unit.key);
        }
    }
    keys.sort_by_key(|(ptc, _)| !ptc);

    let mut mats = Vec::new();
    for key @ (print_then_cut, colour) in keys {
        if print_then_cut {
            let printed: Vec<&Unit> = units.iter().filter(|u| u.key == key).collect();
            mats.extend(print_then_cut::pages(&printed, &print_then_cut::print_area(paper), copies));
            continue;
        }
        if let Some(layout) = nested {
            mats.extend(nested::mats(design, layout, &units, colour));
            continue;
        }
        let empty = |overflow| Mat { print_then_cut, colour, items: Vec::new(), operations: Vec::new(), overflow, page: None };
        let mut open: Option<(Mat, Packer)> = None;
        let keyed = units.iter().filter(|u| u.key == key);
        for (copy, unit) in (0..copies).flat_map(|copy| keyed.clone().map(move |u| (copy, u))) {
            let mut fresh = Packer::new(size);
            if !fresh.fits_empty(unit.size()) {
                let mut mat = empty(true);
                add_unit(&mut mat, unit, copy, fresh.cursor);
                mats.push(mat);
                continue;
            }
            let at = match open.as_mut().and_then(|(_, packer)| packer.place(unit.size())) {
                Some(at) => at,
                None => {
                    mats.extend(open.take().map(|(mat, _)| mat));
                    let at = fresh.place(unit.size()).expect("fits an empty mat");
                    open = Some((empty(false), fresh));
                    at
                }
            };
            add_unit(&mut open.as_mut().expect("a mat is open").0, unit, copy, at);
        }
        mats.extend(open.map(|(mat, _)| mat));
    }
    for mat in &mut mats {
        mat.operations.sort_by_key(|&op| plot_order(op));
    }
    mats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::machine::MachineFamily;
    use crate::pens::{Pen, PenSize};

    const RED: Color32 = Color32::from_rgb(0xd0, 0x20, 0x20);

    fn twelve() -> MatSize {
        MachineFamily::StudioPro.preferred_mat()
    }

    fn add(design: &mut Design, kind: ShapeKind, op: Operation, fill: Color32, at: Pt) -> u64 {
        let id = design.add_shape(kind);
        let s = design.shape_mut(id).unwrap();
        s.fill = fill;
        s.set_operation(op);
        (s.x_in, s.y_in) = at;
        id
    }

    fn pen_star(design: &mut Design, at: Pt) -> u64 {
        let id = add(design, ShapeKind::Star, Operation::Pen, Color32::BLACK, at);
        design.shape_mut(id).unwrap().set_pen(Pen::named("Wine", PenSize::Fine));
        id
    }

    fn offset(mat: &Mat, id: u64) -> Pt {
        mat.items.iter().find(|i| i.piece.owner == id).unwrap().offset_in
    }

    fn close(a: Pt, b: Pt) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn two_colours_make_two_mats_in_canvas_order() {
        let mut design = Design::default();
        let black = add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
        let red = add(&mut design, ShapeKind::Square, Operation::BasicCut, RED, (4.0, 1.0));
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.iter().map(|m| m.colour).collect::<Vec<_>>(), [Color32::BLACK, RED]);
        assert_eq!(mats[0].items.iter().map(|i| i.piece.owner).collect::<Vec<_>>(), [black]);
        assert_eq!(mats[1].items.iter().map(|i| i.piece.owner).collect::<Vec<_>>(), [red]);
        assert_eq!(mats[1].label(), "Basic Cut");
    }

    #[test]
    fn an_attached_pen_star_joins_its_cut_square() {
        let mut design = Design::default();
        let square = add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
        let star = pen_star(&mut design, (1.5, 1.5));
        design.attach(&[square, star]);
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0].colour, Color32::BLACK);
        assert_eq!(mats[0].label(), "Pen, Basic Cut");
    }

    #[test]
    fn an_unattached_pen_star_gets_its_own_mat() {
        let mut design = Design::default();
        add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
        let star = pen_star(&mut design, (5.0, 1.0));
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.len(), 2);
        assert_eq!(mats[1].items.iter().map(|i| i.piece.owner).collect::<Vec<_>>(), [star]);
        assert_eq!(mats[1].label(), "Pen");
        assert_eq!(mats[1].colour, Pen::named("Wine", PenSize::Fine).rgb());
    }

    #[test]
    fn an_attached_pair_keeps_its_offset_and_an_unattached_pair_packs_top_left() {
        let mut design = Design::default();
        let a = add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (3.0, 5.0));
        let b = add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (6.0, 7.0));
        let loose = compose(&design, twelve(), 1).remove(0);
        assert!(close(offset(&loose, a), (0.25, 0.25)), "{loose:?}");
        assert!(close(offset(&loose, b), (0.25 + 2.0 + 0.25, 0.25)), "{loose:?}");

        design.attach(&[a, b]);
        let attached = compose(&design, twelve(), 1).remove(0);
        assert!(close(offset(&attached, a), (0.25, 0.25)), "{attached:?}");
        assert!(close(offset(&attached, b), (0.25 + 3.0, 0.25 + 2.0)), "{attached:?}");
    }

    #[test]
    fn units_wrap_to_a_new_row_and_then_a_new_mat() {
        let mut design = Design::default();
        for i in 0..30 {
            add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (i as f32, 0.0));
        }
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.iter().map(|m| m.items.len()).collect::<Vec<_>>(), [25, 5]);
        assert!(close(mats[0].items[5].offset_in, (0.25, 0.25 + 2.0 + 0.25)), "the sixth square starts row two");
        assert!(mats.iter().all(|m| !m.overflow));
    }

    #[test]
    fn a_unit_too_large_for_the_mat_overflows_onto_its_own_mat() {
        let mut design = Design::default();
        let big = add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (0.0, 0.0));
        design.shape_mut(big).unwrap().width_in = 13.0;
        add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (0.0, 0.0));
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.iter().map(|m| (m.overflow, m.items.len())).collect::<Vec<_>>(), [(true, 1), (false, 1)]);
    }

    #[test]
    fn a_print_then_cut_mat_comes_first() {
        let mut design = Design::default();
        add(&mut design, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
        add(&mut design, ShapeKind::Circle, Operation::PrintThenCut, RED, (4.0, 1.0));
        let mats = compose(&design, twelve(), 1);
        let evidence = "docs/evidence-webapp.md, \"Print Then Cut composition\", questions 4 and 6: print mats are white and come first";
        assert_eq!(mats.iter().map(|m| (m.print_then_cut, m.colour)).collect::<Vec<_>>(), [(true, Color32::WHITE), (false, Color32::BLACK)], "{evidence}");
        assert_eq!(mats[0].label(), "Print Then Cut");
    }

    #[test]
    fn hidden_and_guide_shapes_stay_off_the_mats() {
        let mut design = Design::default();
        add(&mut design, ShapeKind::Square, Operation::Guide, Color32::BLACK, (1.0, 1.0));
        let hidden = add(&mut design, ShapeKind::Square, Operation::BasicCut, RED, (1.0, 1.0));
        design.shape_mut(hidden).unwrap().hidden = true;
        assert!(compose(&design, twelve(), 1).is_empty());
    }

    #[test]
    fn the_label_sorts_operations_into_plot_order() {
        let mut design = Design::default();
        let ids = [
            add(&mut design, ShapeKind::Square, Operation::Perforate, Color32::WHITE, (0.0, 0.0)),
            add(&mut design, ShapeKind::Square, Operation::Pen, Color32::WHITE, (0.0, 0.0)),
            add(&mut design, ShapeKind::Square, Operation::Score, Color32::WHITE, (0.0, 0.0)),
            add(&mut design, ShapeKind::Square, Operation::Score, Color32::WHITE, (0.0, 0.0)),
        ];
        design.attach(&ids);
        assert_eq!(compose(&design, twelve(), 1)[0].label(), "Score, Pen, Perforate");
    }

    #[test]
    fn pen_single_line_text_attached_to_a_cut_card_joins_the_cards_mat() {
        let mut design = Design::default();
        let card = add(&mut design, ShapeKind::Rectangle, Operation::BasicCut, RED, (1.0, 1.0));
        let font = crate::text::FontRef::Stroke(crate::stroke_font::StrokeFontId("relief-singleline".into()));
        let text = design.add_text("Happy birthday".into(), font, 0.5, Color32::BLACK, 1.5, 1.5);
        design.attach(&[card, text]);
        let mats = compose(&design, twelve(), 1);
        assert_eq!(mats.len(), 1, "one mat");
        assert_eq!(mats[0].colour, RED, "the card's");
        assert_eq!(mats[0].label(), "Pen, Basic Cut");
        let item = mats[0].items.iter().find(|i| i.piece.owner == text).expect("the text is on the card's mat");
        let style = design.piece_style(item.piece).unwrap();
        assert_eq!(style.operation, Operation::Pen);
        assert!(design.geometry(&design.shapes[1]).contours().all(|c| !c.closed), "drawn as open strokes");
    }
}

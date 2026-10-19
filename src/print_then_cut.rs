//! Print Then Cut page composition. Every Print Then Cut unit goes onto white paper pages, whatever its
//! colour, packed in canvas order inside the printable area.
//!
//! Only Letter and A4 are offered. Larger sizes are not in the bundled data. `print_area` takes no machine
//! family, because every Print Then Cut family shares the same margins. Items pack by bounding box, not by
//! outline.

use egui::Color32;

use crate::composition::{Mat, Unit, add_unit};
use crate::design::{POINTS_PER_INCH, Pt};
use crate::units::Units;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperSize {
    pub name: &'static str,
    pub width_in: f32,
    pub height_in: f32,
}

pub const PAPERS: [PaperSize; 2] = [
    // The only bundled Print Then Cut size, 612 x 792 pt.
    PaperSize { name: "Letter", width_in: 612.0 / POINTS_PER_INCH, height_in: 792.0 / POINTS_PER_INCH },
    // The metric default, 595 x 841 pt.
    PaperSize { name: "A4", width_in: 595.0 / POINTS_PER_INCH, height_in: 841.0 / POINTS_PER_INCH },
];

pub const LETTER: PaperSize = PAPERS[0];
pub const A4: PaperSize = PAPERS[1];

/// The page's print margins.
const MARGIN_TOP_LEFT_PT: f32 = 58.25;
const MARGIN_BOTTOM_RIGHT_PT: f32 = 67.75;
/// The size check shrinks the printable rectangle by this much per side.
const SIZE_CHECK_INSET_PT: f32 = 0.1;
/// Every packed item carries this padding, so neighbours sit twice it apart.
const ITEM_PADDING_PT: f32 = 5.0;
/// The bleed distance in pixels on the 300 dpi print image.
const BLEED_PX: f32 = 9.0;
const PRINT_DPI: f32 = 300.0;

impl PaperSize {
    /// Imperial users get the preferred imperial size, metric users get 595 x 841 pt.
    pub fn preferred(units: Units) -> Self {
        match units {
            Units::Imperial => LETTER,
            Units::Metric => A4,
        }
    }

    /// "Letter (8.5 x 11)" in the user's units.
    pub fn label(&self, units: Units) -> String {
        format!("{} ({})", self.name, units.size(self.width_in, self.height_in))
    }
}

/// Where images may go on one page, in inches from the paper's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintArea {
    pub paper: PaperSize,
    pub origin_in: Pt,
    pub max_image_in: Pt,
    /// Added round each image when printing; it does not count against the area (question 3).
    pub bleed_in: f32,
}

pub fn print_area(paper: PaperSize) -> PrintArea {
    let lost = MARGIN_TOP_LEFT_PT + MARGIN_BOTTOM_RIGHT_PT + 2.0 * SIZE_CHECK_INSET_PT;
    let origin = (MARGIN_TOP_LEFT_PT + SIZE_CHECK_INSET_PT) / POINTS_PER_INCH;
    PrintArea {
        paper,
        origin_in: (origin, origin),
        max_image_in: (paper.width_in - lost / POINTS_PER_INCH, paper.height_in - lost / POINTS_PER_INCH),
        bleed_in: BLEED_PX / PRINT_DPI,
    }
}

impl PrintArea {
    fn fits(&self, (w, h): Pt) -> bool {
        w <= self.max_image_in.0 && h <= self.max_image_in.1
    }
}

/// What the Print step's dialog sets. The paper, and Add Bleed on by default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintSettings {
    pub paper: PaperSize,
    pub bleed: bool,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self { paper: LETTER, bleed: true }
    }
}

/// Shelf rows inside the printable area: left to right, then a new row below the tallest image.
struct Shelf {
    min: Pt,
    max: Pt,
    cursor: Pt,
    row_height: f32,
}

impl Shelf {
    fn new(area: &PrintArea) -> Self {
        let min = area.origin_in;
        Self { min, max: (min.0 + area.max_image_in.0, min.1 + area.max_image_in.1), cursor: min, row_height: 0.0 }
    }

    fn place(&mut self, (w, h): Pt) -> Option<Pt> {
        let gap = 2.0 * ITEM_PADDING_PT / POINTS_PER_INCH;
        if self.cursor.0 > self.min.0 && self.cursor.0 + w > self.max.0 {
            self.cursor = (self.min.0, self.cursor.1 + self.row_height + gap);
            self.row_height = 0.0;
        }
        if self.cursor.1 + h > self.max.1 {
            return None;
        }
        let at = self.cursor;
        self.cursor.0 += w + gap;
        self.row_height = self.row_height.max(h);
        Some(at)
    }
}

/// The pages for `units`, every copy after the last, in canvas order. An image that does not fit starts a
/// new page; one larger than the printable area goes alone on a page marked `overflow`, which blocks Make.
pub(crate) fn pages(units: &[&Unit], area: &PrintArea, copies: u32) -> Vec<Mat> {
    let page = |overflow| Mat { print_then_cut: true, colour: Color32::WHITE, items: Vec::new(), operations: Vec::new(), overflow, page: Some(*area) };
    let mut pages = Vec::new();
    let mut open: Option<(Mat, Shelf)> = None;
    for (copy, unit) in (0..copies).flat_map(|copy| units.iter().map(move |u| (copy, u))) {
        if !area.fits(unit.size()) {
            let mut alone = page(true);
            add_unit(&mut alone, unit, copy, area.origin_in);
            pages.push(alone);
            continue;
        }
        let at = match open.as_mut().and_then(|(_, shelf)| shelf.place(unit.size())) {
            Some(at) => at,
            None => {
                pages.extend(open.take().map(|(mat, _)| mat));
                let mut shelf = Shelf::new(area);
                let at = shelf.place(unit.size()).expect("fits an empty page");
                open = Some((page(false), shelf));
                at
            }
        };
        add_unit(&mut open.as_mut().expect("a page is open").0, unit, copy, at);
    }
    pages.extend(open.map(|(mat, _)| mat));
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::compose_on;
    use crate::design::{Design, Operation, ShapeKind};
    use crate::machine::MachineFamily;

    const EVIDENCE: &str = "docs/evidence-webapp.md, \"Print Then Cut composition\"";

    fn ptc(design: &mut Design, size_in: f32, fill: Color32) -> u64 {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.width_in, s.height_in, s.fill) = (size_in, size_in, fill);
        s.set_operation(Operation::PrintThenCut);
        id
    }

    fn compose(design: &Design) -> Vec<Mat> {
        compose_on(design, MachineFamily::StudioPro.preferred_mat(), 1, LETTER)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_paper_table_and_print_areas_match_the_recorded_numbers() {
        let rows: Vec<(&str, f32, f32)> = PAPERS.iter().map(|p| (p.name, p.width_in * 72.0, p.height_in * 72.0)).collect();
        assert_eq!(rows, [("Letter", 612.0, 792.0), ("A4", 595.0, 841.0)], "{EVIDENCE}, question 1");
        let letter = print_area(LETTER);
        assert!(close(letter.origin_in.0 * 72.0, 58.35) && close(letter.origin_in.1 * 72.0, 58.35), "{EVIDENCE}, question 2: {letter:?}");
        assert!(close(letter.max_image_in.0 * 72.0, 485.8) && close(letter.max_image_in.1 * 72.0, 665.8), "{EVIDENCE}, question 2: {letter:?}");
        let a4 = print_area(A4);
        assert!(close(a4.max_image_in.0 * 72.0, 595.0 - 126.2) && close(a4.max_image_in.1 * 72.0, 841.0 - 126.2), "{EVIDENCE}, question 2: {a4:?}");
        assert!(close(letter.bleed_in, 0.03) && close(a4.bleed_in, 0.03), "{EVIDENCE}, question 3");
        assert_eq!((PaperSize::preferred(Units::Imperial), PaperSize::preferred(Units::Metric)), (LETTER, A4), "{EVIDENCE}, question 1");
        assert_eq!(PrintSettings::default(), PrintSettings { paper: LETTER, bleed: true }, "{EVIDENCE}, question 3");
    }

    #[test]
    fn a_red_and_a_blue_print_then_cut_square_share_one_white_page() {
        let mut design = Design::default();
        ptc(&mut design, 2.0, Color32::RED);
        ptc(&mut design, 2.0, Color32::BLUE);
        let mats = compose(&design);
        assert_eq!(mats.len(), 1, "{EVIDENCE}, question 4: print mats use the fixed colour #ffffff");
        assert_eq!((mats[0].colour, mats[0].items.len(), mats[0].page), (Color32::WHITE, 2, Some(print_area(LETTER))));
    }

    #[test]
    fn pages_fill_the_printable_area_in_canvas_order_and_then_spill() {
        let mut design = Design::default();
        let ids: Vec<u64> = (0..14).map(|_| ptc(&mut design, 2.0, Color32::RED)).collect();
        let mats = compose(&design);
        let owners = |m: &Mat| m.items.iter().map(|i| i.piece.owner).collect::<Vec<_>>();
        assert_eq!(mats.iter().map(owners).collect::<Vec<_>>(), [ids[..12].to_vec(), ids[12..].to_vec()], "{EVIDENCE}, questions 5 and 6");
        let area = print_area(LETTER);
        let gap = 10.0 / 72.0;
        let offsets: Vec<Pt> = mats[0].items.iter().map(|i| i.offset_in).collect();
        assert!(close(offsets[0].0, area.origin_in.0) && close(offsets[0].1, area.origin_in.1), "{offsets:?}");
        assert!(close(offsets[1].0, area.origin_in.0 + 2.0 + gap), "items sit 2 x 5 pt padding apart: {offsets:?}");
        assert!(close(offsets[3].0, area.origin_in.0) && close(offsets[3].1, area.origin_in.1 + 2.0 + gap), "three fit a row: {offsets:?}");
        assert!(offsets.iter().all(|&(x, y)| x + 2.0 <= area.origin_in.0 + area.max_image_in.0 && y + 2.0 <= area.origin_in.1 + area.max_image_in.1));
    }

    #[test]
    fn an_image_larger_than_the_printable_area_goes_alone_and_overflows() {
        let mut design = Design::default();
        let small = ptc(&mut design, 2.0, Color32::RED);
        let big = ptc(&mut design, 7.0, Color32::RED);
        let mats = compose(&design);
        let summary: Vec<(bool, Vec<u64>)> = mats.iter().map(|m| (m.overflow, m.items.iter().map(|i| i.piece.owner).collect())).collect();
        assert_eq!(summary, [(true, vec![big]), (false, vec![small])], "{EVIDENCE}, question 7: 7 wide exceeds Letter's 6.75");
    }

    #[test]
    fn print_pages_come_before_cut_mats_and_cut_mats_still_key_by_colour() {
        let mut design = Design::default();
        let cut = |design: &mut Design, fill| {
            let id = design.add_shape(ShapeKind::Square);
            design.shape_mut(id).unwrap().fill = fill;
        };
        cut(&mut design, Color32::BLACK);
        ptc(&mut design, 2.0, Color32::RED);
        cut(&mut design, Color32::GREEN);
        ptc(&mut design, 2.0, Color32::BLUE);
        cut(&mut design, Color32::BLACK);
        let mats = compose(&design);
        let summary: Vec<(bool, Color32, usize)> = mats.iter().map(|m| (m.page.is_some(), m.colour, m.items.len())).collect();
        assert_eq!(summary, [(true, Color32::WHITE, 2), (false, Color32::BLACK, 2), (false, Color32::GREEN, 1)], "{EVIDENCE}, questions 4 and 6");
    }

    #[test]
    fn a_unit_with_any_print_then_cut_piece_goes_to_a_page() {
        let mut design = Design::default();
        let square = design.add_shape(ShapeKind::Square);
        let sticker = ptc(&mut design, 1.0, Color32::RED);
        design.attach(&[square, sticker]);
        let mats = compose(&design);
        assert_eq!(mats.iter().map(|m| (m.page.is_some(), m.items.len())).collect::<Vec<_>>(), [(true, 2)], "{EVIDENCE}, question 4");
    }

    #[test]
    fn copies_follow_the_last_image() {
        let mut design = Design::default();
        let ids = [ptc(&mut design, 2.0, Color32::RED), ptc(&mut design, 2.0, Color32::BLUE)];
        let mats = compose_on(&design, MachineFamily::StudioPro.preferred_mat(), 2, LETTER);
        let order: Vec<(u64, u32)> = mats[0].items.iter().map(|i| (i.piece.owner, i.copy)).collect();
        assert_eq!(order, [(ids[0], 0), (ids[1], 0), (ids[0], 1), (ids[1], 1)]);
    }
}

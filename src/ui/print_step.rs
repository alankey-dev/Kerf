//! The Print step (spec 27): a Print Then Cut page is printed before Set. A full workflow sends the page
//! to a printer with registration marks; Kerf saves the artwork as a PDF and says plainly that it has none.

use egui::emath::GuiRounding as _;
use egui::{Rect, Sense, Shape, Stroke, StrokeKind, Ui, vec2};

use crate::bridge::mat_placements;
use crate::export::{Planned, Style};
use crate::make_flow::{MakeFlow, MatSetup};
use crate::print_sheet;
use crate::print_then_cut::PrintArea;

use super::canvas::Grid;
use super::flow_kit::{self as kit, Notice, body_text, card, heading, primary_button, secondary_button};
use super::tokens::*;
use super::{Kerf, Nav, Outbox};

const NO_MARKS: &str = "This sheet has no registration marks. Your machine cannot line up its cut with it unless the sheet is printed with them.";
const PREVIEW_H: f32 = 360.0;

pub fn show(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) -> Nav {
    let Some(area) = flow.mat().and_then(|m| m.page) else { return Nav::Stay };
    card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        heading(ui, "Print");
        body_text(ui, format!("Print this page on {} paper, then load it to cut.", area.paper.name), true);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_H), Sense::hover());
        paint_page(ui, rect, flow, &area);
        kit::toggle_row(ui, "Add Bleed", &mut flow.print.bleed);
        kit::notice(ui, Notice::Warning, NO_MARKS);
        let mut nav = Nav::Stay;
        kit::controls(ui, |ui| {
            if primary_button(ui, "Next", true).clicked() {
                out.extend(flow.printed());
            }
            if secondary_button(ui, "Save print sheet (PDF)", true).clicked() {
                nav = Nav::SavePrintSheet;
            }
        });
        nav
    })
}

/// The current page fitted into `rect`: white paper, the printable area dashed, each image in its colour.
pub fn paint_page(ui: &Ui, rect: Rect, flow: &MakeFlow, area: &PrintArea) {
    let paper = area.paper;
    let grid = Grid::fit(rect, paper.width_in, paper.height_in);
    let sheet = Rect::from_min_size(grid.origin, vec2(paper.width_in, paper.height_in) * grid.scale).round_to_pixels(ui.pixels_per_point());
    let painter = ui.painter();
    painter.add(egui::Shadow { offset: [0, 4], blur: 24, spread: 0, color: ARTBOARD_SHADOW }.as_shape(sheet, 2.0));
    painter.rect_filled(sheet, 2, BG_ARTBOARD);
    painter.rect_stroke(sheet, 2, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let (x, y) = area.origin_in;
    let (w, h) = area.max_image_in;
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)].map(|p| grid.to_screen(p));
    painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0_f32, TEXT_MUTED), 6.0, 4.0));
    let Some(mat) = flow.mat() else { return };
    for p in mat_placements(&flow.design, mat, &MatSetup { mirror: false, ..flow.setup }) {
        let to_page = super::op_style::in_inches(p.transform).then(grid.matrix());
        super::op_style::paint_placed(painter, &flow.design, p.shape, p.piece().id, to_page, grid.scale, BG_ARTBOARD);
    }
}

impl Kerf {
    /// Writes the current page's print sheet, named for its place among the print pages.
    pub(super) fn save_print_sheet(&mut self) {
        let Some(flow) = &self.doc.flow else { return };
        let Some(sheet) = flow.mat().and_then(|mat| print_sheet::sheet(&flow.design, mat, &flow.setup, flow.print.bleed)) else { return };
        let page = flow.mats[..=flow.current].iter().filter(|m| m.page.is_some()).count();
        tracing::info!(target: "cmd", action = "save print sheet", page, bleed = flow.print.bleed, "print");
        let file = Planned::pdf(print_sheet::file_name(&self.doc.title, page), vec![sheet], Style::Filled);
        self.write_export(&[file], false);
    }
}

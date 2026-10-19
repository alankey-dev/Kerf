use egui::{Frame, Margin, Stroke, Ui};

use crate::bridge::mat_placements;
use crate::machine::LoadType;
use crate::make_flow::MakeFlow;
use crate::print_then_cut::{PAPERS, print_area};
use crate::units::Units;

use super::flow_kit::{self as kit, divider, section_header};
use super::tokens::*;
use super::{Nav, Outbox};

const LOAD_TYPES: [LoadType; 3] = [LoadType::OnMat, LoadType::MatlessRoll, LoadType::CardMat];
const MAX_COPIES: u32 = 20;

pub fn show(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox, units: Units, nest: &mut super::prepare_nest::Nest) -> Nav {
    kit::themed(ui, |ui| {
        let sidebar = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::same(16));
        let panel = egui::SidePanel::left("prepare-sidebar").resizable(false).exact_width(INSPECTOR_W).frame(sidebar).show_separator_line(false);
        let mut nav = panel
            .show_inside(ui, |ui| {
                let full = ui.max_rect().expand(16.0);
                super::anchors::mark(ui, super::anchors::Anchor::PrepareSettings, full);
                ui.painter().vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
                egui::ScrollArea::vertical().show(ui, |ui| sidebar_body(ui, flow, out, units, nest)).inner
            })
            .inner;
        let footer = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::same(16));
        egui::TopBottomPanel::bottom("prepare-footer").frame(footer).show_separator_line(false).show_inside(ui, |ui| {
            let full = ui.max_rect().expand(16.0);
            ui.painter().hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            kit::actions(ui, |ui| {
                let go = kit::primary_button(ui, "Continue", !flow.mats.is_empty() && !too_large(flow));
                super::anchors::mark(ui, super::anchors::Anchor::Continue, go.rect);
                if go.clicked() {
                    out.extend(flow.continue_from_prepare());
                }
                if kit::secondary_button(ui, "Cancel", true).clicked() {
                    nav = Nav::Close;
                }
            });
        });
        egui::CentralPanel::default().frame(Frame::NONE.fill(BG_CANVAS_VOID)).show_inside(ui, |ui| {
            if mat_preview(ui, flow) {
                tracing::info!(target: "cmd", mat = flow.current + 1, "preview cut");
                nav = Nav::CutPreview;
            }
        });
        nav
    })
}

fn sidebar_body(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox, units: Units, nest: &mut super::prepare_nest::Nest) -> Nav {
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut nav = Nav::Stay;
    if too_large(flow) {
        let max = print_area(flow.print.paper).max_image_in;
        kit::notice(ui, kit::Notice::Warning, &format!("Image is too large for {} paper. The max size is {}.", flow.print.paper.name, units.size(max.0, max.1)));
        ui.add_space(8.0);
    }
    let missing = crate::system_fonts::missing_for(&flow.design);
    if !missing.is_empty() {
        kit::notice(ui, kit::Notice::Warning, &uninstalled_words(&missing));
        ui.add_space(4.0);
        let pick = kit::secondary_button(ui, "Pick a font", true);
        let body = "Go back to the canvas with the first of these texts selected, then choose an installed font from the Font menu.";
        let pick = super::tooltip::tip(pick, "Pick a font", body, None);
        if pick.clicked() {
            let first = flow.design.shapes.iter().find(|s| s.kind.system_family().is_some_and(crate::system_fonts::is_missing));
            if let Some(shape) = first {
                tracing::info!(target: "cmd", id = shape.id, "pick a font");
                nav = Nav::PickFont(shape.id);
            }
        }
        ui.add_space(8.0);
    }
    let machine = match (flow.profile(), &flow.machine) {
        (Some(p), _) => Some((p.name.clone(), units.size(p.work_area_in().0, p.work_area_in().1))),
        (None, Some(m)) => Some((m.device_type.clone(), m.connection_type.wire_name().to_owned())),
        (None, None) => None,
    };
    if let Some((name, detail)) = machine {
        section_header(ui, "Machine");
        kit::select_row(ui, true, &name, &detail);
        ui.add_space(4.0);
        if kit::secondary_button(ui, "Change machine", true).clicked() {
            tracing::info!(target: "cmd", "change machine");
            out.extend(flow.change_machine());
        }
        if flow.skipped_print_then_cut > 0 {
            ui.add_space(6.0);
            kit::notice(ui, kit::Notice::Warning, "Print Then Cut needs a registration step this device does not have");
        }
        divider(ui);
    }
    if flow.profile().is_none() {
        section_header(ui, "Material Load Type");
        for load_type in LOAD_TYPES {
            if kit::radio_row(ui, flow.setup.load_type == load_type, load_type.label()).clicked() {
                flow.set_load_type(load_type);
            }
        }
        divider(ui);
    }
    section_header(ui, "Material Size");
    let options: Vec<_> = flow.mat_sizes().into_iter().map(|size| (size, size.name(units))).collect();
    if let Some(size) = kit::dropdown(ui, "Material Size", flow.setup.mat_size, &options) {
        flow.set_mat_size(size);
    }
    if flow.mat().is_some_and(|m| m.page.is_some()) {
        divider(ui);
        section_header(ui, "Paper Size");
        let options: Vec<_> = PAPERS.iter().map(|p| (*p, p.label(units))).collect();
        let options: Vec<_> = options.iter().map(|(p, label)| (*p, label.as_str())).collect();
        if let Some(paper) = kit::dropdown(ui, "Paper Size", flow.print.paper, &options) {
            flow.set_paper(paper);
        }
    }
    divider(ui);
    kit::toggle_row(ui, "Mirror", &mut flow.setup.mirror);
    ui.horizontal(|ui| {
        ui.set_height(32.0);
        kit::body_text(ui, "Project copies", false);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut copies = flow.setup.copies as f32;
            if crate::units::number_field(ui, "Project copies", Some(copies), "", crate::units::FIELD_W).is_some_and(|t| t.set(&mut copies, 1.0..=MAX_COPIES as f32)) {
                flow.set_copies(copies.round() as _);
            }
        });
    });
    divider(ui);
    super::prepare_nest::section(ui, flow, nest, units);
    section_header(ui, "Mats");
    let times = super::cut_preview::mat_times(flow);
    for (n, (mat, seconds)) in flow.mats.iter().zip(times).enumerate() {
        let time = crate::output::estimate::clock(seconds);
        if kit::select_row(ui, flow.current == n, &format!("Load {}", n + 1), &format!("{}  {time}", mat.label())).clicked() {
            flow.current = n;
        }
    }
    nav
}

/// What the notice says about system families that are not installed: they lay out and cut in Inter.
fn uninstalled_words(families: &[String]) -> String {
    match families {
        [only] => format!("{only} is not installed, so its text cuts in Inter."),
        [init @ .., last] => format!("{} and {last} are not installed, so their text cuts in Inter.", init.join(", ")),
        [] => String::new(),
    }
}

/// An image larger than the printable area blocks Make.
fn too_large(flow: &MakeFlow) -> bool {
    flow.mats.iter().any(|m| m.page.is_some() && m.overflow)
}

/// The selected mat, packed as it will be sent, each shape in its own colour, with Preview cut below. A
/// Print Then Cut page shows its paper instead. Returns true when Preview cut is clicked.
fn mat_preview(ui: &mut Ui, flow: &MakeFlow) -> bool {
    let rect = ui.available_rect_before_wrap().shrink(40.0);
    if let Some(area) = flow.mat().and_then(|m| m.page) {
        super::print_step::paint_page(ui, rect, flow, &area);
        return false;
    }
    let size = flow.setup.mat_size;
    let grid = kit::mat_sheet(ui, rect.with_max_y(rect.bottom() - BUTTON_H - 16.0), size.width_in, size.height_in);
    let below = grid.to_screen((size.width_in / 2.0, size.height_in)) + egui::vec2(0.0, 16.0 + BUTTON_H / 2.0);
    let button = egui::Rect::from_center_size(below, egui::vec2(160.0, BUTTON_H));
    let preview = ui.put(button, |ui: &mut Ui| kit::secondary_button(ui, "Preview cut", flow.mat().is_some()));
    let preview = super::tooltip::tip(preview, "Preview cut", "Watch this mat's tool path in machine order, with its time and material use.", None);
    super::anchors::mark(ui, super::anchors::Anchor::PreviewCut, preview.rect);
    let Some(mat) = flow.mat() else { return false };
    for p in mat_placements(&flow.design, mat, &flow.setup) {
        let to_mat = super::op_style::in_inches(p.transform).then(grid.matrix());
        super::op_style::paint_placed(ui.painter(), &flow.design, p.shape, p.piece().id, to_mat, grid.scale, BG_ARTBOARD);
    }
    preview.clicked()
}

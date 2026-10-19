//! Editing a component's master in isolation (spec 31). Edit master (double-click an instance, or the
//! Components panel) opens it: the canvas shows only the master on an artboard the size of the
//! component's frame, under a breadcrumb "Canvas > Component: Name" with Done, above a strip of the
//! canvas's instances drawn live.
//!
//! The model does the swap: each frame `update` opens the master (`Design::enter_master`) after taking
//! the undo baseline and closes it (`leave_master`) once edits have run, before `History::observe`. So
//! history only ever sees the whole design: an edit in isolation is one ordinary undo step, opening and
//! Done are none, and Undo after Done reverts master edits. Isolation closes itself when its component
//! is gone, after an Undo say.

use egui::{Context, Frame, Margin, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::components::canvas_shapes;
use crate::design::{Design, POINTS_PER_INCH, Pt};
use crate::edit::AppCommand;

use super::canvas::Grid;
use super::commands::Command;
use super::document::Document;
use super::flow_kit::{self, Kind};
use super::fonts;
use super::op_style::{Mode, paint_shape};
use super::tokens::*;
use super::tooltip::tip;

const BAR_H: f32 = 44.0;
const STRIP_H: f32 = 104.0;
const PREVIEW: f32 = 64.0;
/// The space a master first fills on screen.
const FIT_PX: Vec2 = vec2(480.0, 360.0);

/// The open master, and the canvas's selection and zoom that Done gives back.
pub struct Isolation {
    component: u64,
    canvas_selection: Vec<u64>,
    canvas_zoom: f32,
    canvas_pan: Vec2,
}

/// Whether the app command keeps the master open. The rest act on the whole project (Export, Make, the
/// preview, versions, leaving the canvas), so they close it first.
fn keeps_isolation(app: &AppCommand) -> bool {
    match app {
        AppCommand::Undo
        | AppCommand::Redo
        | AppCommand::Save
        | AppCommand::Palette
        | AppCommand::ZoomIn
        | AppCommand::ZoomOut
        | AppCommand::ResetZoom
        | AppCommand::ZoomToFit
        | AppCommand::ZoomToSelection
        | AppCommand::ZoomToAll
        | AppCommand::ZoomTo(_)
        | AppCommand::Units(_)
        | AppCommand::Snapping
        | AppCommand::SnapClass(_)
        | AppCommand::SnapSettings
        | AppCommand::PartialGrid
        | AppCommand::GridSettings
        | AppCommand::Upload
        | AppCommand::EditMaster(_)
        | AppCommand::Shortcuts
        | AppCommand::About
        | AppCommand::AiSettings
        | AppCommand::Rename
        | AppCommand::EditMonogram
        | AppCommand::ReplaceColour
        | AppCommand::StepRepeatPanel
        | AppCommand::RotateCopiesPanel => true,
        AppCommand::New
        | AppCommand::Home
        | AppCommand::Export
        | AppCommand::PrepareForDesignSpace
        | AppCommand::Make
        | AppCommand::Preview
        | AppCommand::Open(_)
        | AppCommand::CutPreview
        | AppCommand::VersionHistory
        | AppCommand::LeaveMaster
        | AppCommand::CloseTab(_)
        | AppCommand::NextTab
        | AppCommand::PreviousTab
        | AppCommand::Tab(_)
        | AppCommand::SavePaletteToProject
        | AppCommand::CopyPaletteToSettings
        | AppCommand::Tutorial => false,
    }
}

impl Document {
    /// Swaps the open master into the design for this frame; closes isolation if its component is gone.
    pub(super) fn open_master(&mut self) {
        if self.flow.is_some() || self.trace.is_some() {
            return;
        }
        let Some(open) = self.isolation.as_ref().map(|i| i.component) else { return };
        if !self.design.enter_master(open) {
            self.close_isolation();
        }
    }

    /// The artboard while a master is open: its component's frame.
    pub(super) fn master_frame(&self) -> Option<Pt> {
        self.design.editing.and_then(|c| self.design.component(c)).map(|c| c.size_in)
    }

    pub(super) fn edit_master(&mut self, component: u64) {
        let Some(size) = self.design.component(component).map(|c| c.size_in) else { return };
        self.drop_canvas_sessions();
        let (canvas_selection, canvas_zoom, canvas_pan) = match self.isolation.take() {
            Some(was) => (was.canvas_selection, was.canvas_zoom, was.canvas_pan),
            None => (std::mem::take(&mut self.selection), self.view.zoom, self.view.pan_pt),
        };
        self.selection.clear();
        let fit = (FIT_PX.x / (size.0 * POINTS_PER_INCH)).min(FIT_PX.y / (size.1 * POINTS_PER_INCH));
        self.view.zoom = ((fit * 20.0).floor() / 20.0).clamp(0.25, 4.0);
        self.view.pan_pt = Vec2::ZERO;
        self.isolation = Some(Isolation { component, canvas_selection, canvas_zoom, canvas_pan });
    }

    pub(super) fn close_isolation(&mut self) {
        let Some(was) = self.isolation.take() else { return };
        self.drop_canvas_sessions();
        self.selection = was.canvas_selection;
        self.view.zoom = was.canvas_zoom;
        self.view.pan_pt = was.canvas_pan;
    }

    /// Ends a drag, node edit or text edit begun on the shapes about to be swapped away.
    pub(super) fn drop_canvas_sessions(&mut self) {
        self.view.end_gesture();
        self.view.nodes = None;
        self.view.text_edit = None;
    }

    /// Runs before an app command: closes isolation unless the command keeps it.
    pub(super) fn isolation_before(&mut self, app: &AppCommand) {
        if !keeps_isolation(app) {
            self.close_isolation();
        }
    }

    /// The breadcrumb bar and the instance strip, while a master is open. Returns Done when clicked.
    pub(super) fn isolation_chrome(&mut self, ctx: &Context) -> Option<Command> {
        let open = self.design.editing?;
        let name = self.design.component(open).map_or_else(String::new, |c| c.name.clone());
        let bar = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::symmetric(16, 0));
        let done = egui::TopBottomPanel::top("isolation-bar").exact_height(BAR_H).frame(bar).show_separator_line(false).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.expand2(vec2(16.0, 0.0)).x_range(), full.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            breadcrumb(ui, &name)
        });
        let strip = Frame::NONE.fill(BG_PANEL).inner_margin(Margin::symmetric(16, 10));
        egui::TopBottomPanel::bottom("isolation-strip").exact_height(STRIP_H).frame(strip).show_separator_line(false).show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.expand2(vec2(16.0, 0.0)).x_range(), full.top() - 9.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            preview_strip(ui, &self.design, open);
        });
        done.inner.then_some(Command::App(AppCommand::LeaveMaster))
    }
}

/// "Canvas > Component: Name" and Done. Returns whether Canvas or Done was clicked.
fn breadcrumb(ui: &mut Ui, name: &str) -> bool {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let canvas = ui.add(egui::Label::new(egui::RichText::new("Canvas").font(fonts::regular(13.0)).color(ACCENT_PRIMARY)).selectable(false).sense(Sense::click()));
        let canvas = tip(canvas, "Back to the canvas", "Close the master. Your changes are already on every instance.", None);
        canvas.widget_info(|| WidgetInfo::labeled(WidgetType::Link, true, "Back to canvas"));
        ui.label(egui::RichText::new(">").font(fonts::regular(13.0)).color(TEXT_MUTED));
        let crumb = format!("Component: {name}");
        ui.label(egui::RichText::new(&crumb).font(fonts::semibold(13.0)).color(TEXT_PRIMARY));
        let done = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = flow_kit::button(ui, "Done", Kind::Primary, true);
            tip(resp, "Done", "Close the master and return to the canvas.", None).clicked()
        });
        canvas.clicked() || done.inner
    })
    .inner
}

/// Every instance of `component` on the canvas, each drawn live in a small square.
fn preview_strip(ui: &mut Ui, design: &Design, component: u64) {
    let instances: Vec<_> = canvas_shapes(design).iter().filter(|s| s.instance_of() == Some(component)).collect();
    let caption = match instances.len() {
        0 => "No instances on the canvas".to_owned(),
        1 => "1 instance on the canvas".to_owned(),
        n => format!("{n} instances on the canvas"),
    };
    ui.label(egui::RichText::new(caption).font(fonts::regular(11.0)).color(TEXT_MUTED));
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for (n, instance) in instances.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(PREVIEW), Sense::hover());
                resp.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, format!("Instance preview {}", n + 1)));
                ui.painter().rect(rect, 6, BG_CANVAS_VOID, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
                let area = rect.shrink(8.0);
                let b = design.geometry(instance).bounds();
                if b.is_empty() {
                    continue;
                }
                let scale = (area.width() / b.width().max(1e-3)).min(area.height() / b.height().max(1e-3));
                let mid = pos2((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
                let to_screen = Grid { origin: area.center() - mid.to_vec2() * scale, scale }.matrix();
                paint_shape(&ui.painter().with_clip_rect(rect), design, instance, to_screen, scale, Mode::Design);
            }
        });
    });
}

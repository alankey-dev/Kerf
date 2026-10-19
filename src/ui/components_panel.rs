//! The Components panel (spec 31): a tab beside Layers with a tile per component, its master drawn
//! above its name. Click a tile, or drag it onto the canvas, to place an instance at master size;
//! right-click it to rename, edit, duplicate or delete the component.

use egui::{Align2, Color32, Id, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, vec2};

use crate::components::{Component, instance_count, specimen};
use crate::design::{Design, Pt};
use crate::edit::{AppCommand, Edit, Subject};

use super::canvas::Grid;
use super::commands::Command;
use super::fonts;
use super::modal::{self, Choice};
use super::names::{field, fitted};
use super::notice::Notice;
use super::op_style::{Mode, paint_shape};
use super::popover::text_row;
use super::tokens::*;
use super::tooltip::tip;

const TILE: Vec2 = vec2(102.0, 104.0);
const THUMB_H: f32 = 80.0;
const GAP: f32 = 12.0;

/// What a tile carries while it is dragged: the component to place where it is dropped.
#[derive(Clone, Copy)]
pub struct Dragged(pub u64);

/// A tile's name being typed over.
#[derive(Clone)]
struct Renaming {
    component: u64,
    name: String,
}

/// What the user did to one tile this frame.
enum Picked {
    Place,
    Rename,
    /// The rename field closed: the typed name, or `None` when Escape cancelled it.
    Renamed(Option<String>),
    EditMaster,
    Duplicate,
    Delete,
}

fn tab_id() -> Id {
    Id::new("components-tab")
}

/// Shows the Layers tab, for a rename started by key or menu.
pub fn show_layers(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(tab_id(), false));
}

/// The Layers and Components tabs, where Layers' title was. Returns whether Components is open.
pub fn tabs(ui: &mut Ui) -> bool {
    let id = tab_id();
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        for (label, components) in [("Layers", false), ("Components", true)] {
            let on = open == components;
            let ink = if on { TEXT_PRIMARY } else { TEXT_MUTED };
            let resp = ui.add(egui::Label::new(RichText::new(label).font(fonts::semibold(13.0)).color(ink)).selectable(false).sense(Sense::click()));
            resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, label));
            if components {
                super::anchors::mark(ui, super::anchors::Anchor::ComponentsTab, resp.rect);
            }
            if resp.clicked() {
                open = components;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, open));
    ui.add_space(8.0);
    open
}

/// The panel. Its edits run here through `edit::run`; Edit master comes back as a command for `update`.
pub fn show(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>) -> Option<Command> {
    let renaming_id = Id::new("components-panel-renaming");
    let deleting_id = Id::new("components-panel-deleting");
    let mut renaming: Option<Renaming> = ui.data(|d| d.get_temp(renaming_id)).flatten();
    let mut picked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if design.components.is_empty() {
            let hint = "No components yet. Select shapes and choose Make component to reuse them.";
            ui.label(RichText::new(hint).font(fonts::regular(12.0)).color(TEXT_MUTED));
        }
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        for row in design.components.chunks(2) {
            ui.horizontal(|ui| {
                for c in row {
                    let typing = renaming.as_mut().filter(|r| r.component == c.id).map(|r| &mut r.name);
                    if let Some(p) = tile(ui, design, c, typing) {
                        picked = Some((c.id, p));
                    }
                }
            });
        }
    });
    let mut edit = None;
    let mut command = None;
    match picked {
        Some((c, Picked::Place)) => edit = Some(Edit::PlaceInstance { component: c, at: None }),
        Some((c, Picked::Rename)) => renaming = design.component(c).map(|c| Renaming { component: c.id, name: c.name.clone() }),
        Some((component, Picked::Renamed(name))) => {
            renaming = None;
            edit = name.filter(|n| design.component(component).is_some_and(|c| c.name != *n)).map(|name| Edit::RenameComponent { component, name });
        }
        Some((c, Picked::EditMaster)) => command = Some(Command::App(AppCommand::EditMaster(c))),
        Some((c, Picked::Duplicate)) => edit = Some(Edit::DuplicateComponent(c)),
        Some((c, Picked::Delete)) if instance_count(design, c) == 0 => edit = Some(Edit::DeleteComponent(c)),
        Some((c, Picked::Delete)) => ui.data_mut(|d| d.insert_temp(deleting_id, c)),
        None => {}
    }
    if let Some(c) = ui.data(|d| d.get_temp::<u64>(deleting_id))
        && let Some(delete) = confirm_delete(ui, design, c)
    {
        ui.data_mut(|d| d.remove_temp::<u64>(deleting_id));
        if delete {
            edit = Some(Edit::DeleteComponent(c));
        }
    }
    ui.data_mut(|d| d.insert_temp(renaming_id, renaming));
    if let Some(edit) = edit {
        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
    }
    command
}

/// The question Delete asks when the component has instances: `Some(true)` to delete, `Some(false)` to
/// cancel, `None` while it waits.
fn confirm_delete(ui: &Ui, design: &Design, component: u64) -> Option<bool> {
    let Some(c) = design.component(component) else { return Some(false) };
    let n = instance_count(design, component);
    let detach = if n == 1 { "Detach 1 instance and delete".to_owned() } else { format!("Detach {n} instances and delete") };
    let title = format!("Delete {}?", c.name);
    let detail = "Its instances become plain shapes where they are. Undo brings the component back.";
    let choices = [Choice { label: "Cancel", a11y: "Cancel", primary: false }, Choice { label: &detach, a11y: &detach, primary: true }];
    modal::show(ui.ctx(), "delete-component", &title, detail, &choices, 0).map(|i| i == 1)
}

/// One tile: the master drawn in its frame, the name under it (or `typing`'s field), its menu.
fn tile(ui: &mut Ui, design: &Design, c: &Component, typing: Option<&mut String>) -> Option<Picked> {
    let blocked = design.check(&Edit::PlaceInstance { component: c.id, at: None }, &[]).err();
    let (rect, resp) = ui.allocate_exact_size(TILE, Sense::click_and_drag());
    let body = match blocked {
        Some(why) => why.reason(),
        None => "Click the picture to place an instance, or drag it onto the canvas. Double-click the name to rename it. Right-click for more.".to_owned(),
    };
    let resp = tip(resp, &c.name, &body, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, blocked.is_none(), format!("Component tile {}", c.name)));
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), THUMB_H));
    let fill = if blocked.is_none() && resp.hovered() { ACCENT_HOVER } else { BG_CANVAS_VOID };
    ui.painter().rect(thumb, 8, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    paint_master(&ui.painter().with_clip_rect(thumb), design, c.id, thumb.shrink(10.0));
    let caption = Rect::from_min_max(egui::pos2(rect.left(), thumb.bottom() + 4.0), rect.max);
    let field_slot = Rect::from_min_size(caption.min, vec2(caption.width(), 20.0));
    let mut picked = match typing {
        Some(name) => field(ui, field_slot, name, fonts::regular(12.0), egui::Align::Center, "Component name").map(Picked::Renamed),
        None => {
            let ink = if blocked.is_none() { TEXT_PRIMARY } else { DISABLED_TEXT };
            fitted(ui.painter(), caption.center_top(), Align2::CENTER_TOP, &c.name, fonts::regular(12.0), ink, caption.width());
            let on_name = resp.interact_pointer_pos().is_some_and(|p| caption.contains(p));
            (resp.double_clicked() && on_name).then_some(Picked::Rename)
        }
    };
    if blocked.is_none() {
        resp.dnd_set_drag_payload(Dragged(c.id));
        if resp.dragged() {
            ghost(ui, design, c.id);
        }
        if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| thumb.contains(p)) {
            picked = Some(Picked::Place);
        }
    }
    resp.context_menu(|ui| {
        ui.set_width(180.0);
        for (label, p) in [("Rename", Picked::Rename), ("Edit master", Picked::EditMaster), ("Duplicate component", Picked::Duplicate), ("Delete", Picked::Delete)] {
            if text_row(ui, label).clicked() {
                picked = Some(p);
                ui.close_menu();
            }
        }
    });
    picked
}

/// The component's master in its frame, fitted into `area`.
fn paint_master(painter: &egui::Painter, design: &Design, component: u64, area: Rect) {
    let Some(shape) = specimen(design, component) else { return };
    let b = design.geometry(&shape).bounds();
    if b.is_empty() {
        return;
    }
    let scale = (area.width() / b.width().max(1e-3)).min(area.height() / b.height().max(1e-3));
    let mid = ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
    let to_screen = Grid { origin: area.center() - vec2(mid.0, mid.1) * scale, scale }.matrix();
    paint_shape(painter, design, &shape, to_screen, scale, Mode::Design);
}

/// A small copy of the tile under the pointer while it is dragged.
fn ghost(ui: &Ui, design: &Design, component: u64) {
    let Some(at) = ui.ctx().pointer_latest_pos() else { return };
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("component-drag")));
    let area = Rect::from_center_size(at, Vec2::splat(56.0));
    painter.rect(area, 6, Color32::from_white_alpha(200), Stroke::new(1.0_f32, ACCENT_PRIMARY), StrokeKind::Inside);
    paint_master(&painter, design, component, area.shrink(6.0));
}

/// Places the component a tile was dragged from where it is dropped on the canvas. A blocked place
/// (a component dropped into its own master) says why under the toolbar.
pub(super) fn drop_on(canvas: &Response, design: &mut Design, selection: &mut Vec<u64>, notice: &mut Option<Notice>, to_in: impl Fn(Pos2) -> Pt) {
    let Some(dragged) = canvas.dnd_release_payload::<Dragged>() else { return };
    let Some(at) = canvas.ctx.pointer_latest_pos() else { return };
    let edit = Edit::PlaceInstance { component: dragged.0, at: Some(to_in(at)) };
    if let Err(blocked) = crate::edit::run(design, selection, &edit, Subject::Selection) {
        *notice = Some(Notice::new(blocked.reason(), false));
    }
}

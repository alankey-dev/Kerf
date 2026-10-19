use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};

use crate::containers::AlignTo;
use crate::design::{Arrange, Combine, CompoundOp, Design, Shape};
use crate::edit::{Edit, Lock, Subject};
use crate::links::Link;
use crate::names::Named;

use super::chrome::text;
use super::commands::{Cx, Entry};
use super::effects_list;
use super::fonts;
use super::icons::Icon;
use super::names::{field, row_name};
use super::tokens::*;

mod drag;
mod outline;
#[cfg(test)]
mod scrolling;

use outline::{Kind, Outline};

const ROW_H: f32 = 28.0;
/// The space between rows.
const GAP: f32 = 2.0;
const INDENT: f32 = 16.0;
/// Each action icon's hit box; a name stops `NAME_GAP` short of the first one in its row.
const ACTION: f32 = 18.0;
const NAME_GAP: f32 = 2.0;

#[derive(Debug)]
enum RowAction {
    Select,
    Context(Pos2),
    ToggleCollapsed,
    Drag,
    /// The row's eye, arrows or bin: an edit of the row's objects that leaves the selection be.
    Edit(Edit),
    /// A chip, the disclosure or a line of the effect list of shape `.0`.
    Effect(u64, effects_list::Act),
    /// A double-click on the name: type over it.
    Rename,
}

/// A row's name being typed over, then the name its field committed, which `show` applies after the
/// list whatever else the frame's clicks did.
#[derive(Clone)]
enum Renaming {
    Typing { target: Named, text: String },
    Committed { target: Named, name: String },
}

fn renaming_id() -> egui::Id {
    egui::Id::new("layers-renaming")
}

/// Opens the name of the layer `on` names for typing over, in the Layers tab: F2, Enter or Rename.
pub fn begin_rename(ctx: &egui::Context, design: &Design, on: &[u64]) {
    let Some((target, text)) = design.named(on).and_then(|t| Some((t, design.name_of(t)?))) else { return };
    ctx.data_mut(|d| d.insert_temp(renaming_id(), Renaming::Typing { target, text }));
    super::components_panel::show_layers(ctx);
}

/// `target`'s rename field over `slot` while it is being typed, else `false`. Enter or a click away
/// commits it and Escape drops it.
fn rename_field(ui: &mut Ui, target: Named, slot: Rect, font: egui::FontId) -> bool {
    let Some(Renaming::Typing { mut text, .. }) =
        ui.data(|d| d.get_temp::<Renaming>(renaming_id())).filter(|r| matches!(r, Renaming::Typing { target: t, .. } if *t == target))
    else {
        return false;
    };
    let next = match field(ui, slot, &mut text, font, egui::Align::LEFT, "Layer name") {
        None => Some(Renaming::Typing { target, text }),
        Some(Some(name)) => Some(Renaming::Committed { target, name }),
        Some(None) => None,
    };
    ui.data_mut(|d| match next {
        Some(next) => d.insert_temp(renaming_id(), next),
        None => d.remove::<Renaming>(renaming_id()),
    });
    true
}

/// Applies a committed rename as one `Edit::Rename`, unless the name is unchanged. A rename still being
/// typed whose row is not among `listed` (its group collapsed, or Layers hidden behind Components) commits
/// what was typed, so it never lingers to reopen later over another row.
pub fn finish_rename(ui: &Ui, design: &mut Design, selection: &mut Vec<u64>, listed: &[Named]) {
    let (target, name) = match ui.data(|d| d.get_temp::<Renaming>(renaming_id())) {
        Some(Renaming::Committed { target, name }) => (target, name),
        Some(Renaming::Typing { target, text }) if !listed.contains(&target) => (target, text.trim().to_owned()),
        _ => return,
    };
    ui.data_mut(|d| d.remove::<Renaming>(renaming_id()));
    if design.name_of(target).is_none_or(|current| current == name) {
        return;
    }
    let on: Vec<u64> = match target {
        Named::Shape(id) => vec![id],
        Named::Link(id) => design.shapes.iter().filter(|s| s.link.is_some_and(|l| l.id == id)).map(|s| s.id).collect(),
    };
    _ = crate::edit::run(design, selection, &Edit::Rename(Some(name)), Subject::Ids(&on));
}

/// Where a row's name may run: from `left` to `NAME_GAP` short of the action icon centred at `first_x`.
fn name_slot(line: Rect, left: f32, first_x: f32) -> Rect {
    Rect::from_x_y_ranges(left..=(first_x - ACTION / 2.0 - NAME_GAP).max(left), line.y_range())
}

/// The name in `slot`, or its rename field while `target` is being renamed. A double-click on the name
/// starts renaming it.
fn name_or_field(ui: &mut Ui, resp: &egui::Response, target: Named, slot: Rect, name: &str, font: egui::FontId, ink: egui::Color32) -> Option<RowAction> {
    if rename_field(ui, target, slot, font.clone()) {
        return None;
    }
    row_name(ui, resp, slot, name, font, ink);
    let on_name = resp.interact_pointer_pos().is_some_and(|p| slot.contains(p));
    (resp.double_clicked() && on_name).then_some(RowAction::Rename)
}

/// How a row click changes the selection: plain, Shift or Cmd.
#[derive(Clone, Copy)]
enum Click {
    Only,
    Range,
    Toggle,
}

/// A visible row: a lone shape or member, or a link's parent row. Keys the Shift-click anchor.
#[derive(Clone, Copy, PartialEq)]
enum RowKey {
    Shape(u64),
    Link(u64),
    /// A read-only row for one part of an instance, by its derived id; it selects the instance.
    Part(u64),
}

/// Applies a click on row `at` of `rows`, each the ids that row selects in list order. A range runs from
/// the `anchor` row, which a Shift-click leaves in place. Returns the new anchor.
fn click_rows(rows: &[Vec<u64>], anchor: Option<usize>, at: usize, click: Click, selection: &mut Vec<u64>) -> usize {
    match (click, anchor) {
        (Click::Range, Some(from)) => {
            selection.clear();
            for &id in rows[from.min(at)..=from.max(at)].iter().flatten() {
                if !selection.contains(&id) {
                    selection.push(id);
                }
            }
            return from;
        }
        (Click::Toggle, _) if rows[at].iter().all(|id| selection.contains(id)) => selection.retain(|id| !rows[at].contains(id)),
        (Click::Toggle, _) => selection.extend(rows[at].iter().filter(|id| !selection.contains(id)).collect::<Vec<_>>()),
        (Click::Only | Click::Range, _) => *selection = rows[at].clone(),
    }
    at
}

/// One entry in the list: a lone shape, a container or Compound with its children, or a Group or Attach
/// parent with its members, top first.
#[derive(Debug, PartialEq)]
enum Layer {
    Shape(u64),
    Container { id: u64, children: Vec<Layer> },
    Link { link: Link, members: Vec<u64> },
}

/// The list, top of the z-order first. A link lists where its top-most member sits.
fn layers(design: &Design) -> Vec<Layer> {
    level(design, None)
}

/// The children of `parent` (`None` is the top level) as layers, each container holding its own.
fn level(design: &Design, parent: Option<u64>) -> Vec<Layer> {
    let mut out: Vec<Layer> = Vec::new();
    for s in design.children(parent).collect::<Vec<_>>().into_iter().rev() {
        if s.holds_children() {
            out.push(Layer::Container { id: s.id, children: level(design, Some(s.id)) });
            continue;
        }
        let Some(link) = s.link else {
            out.push(Layer::Shape(s.id));
            continue;
        };
        let seen = out.iter_mut().find_map(|l| match l {
            Layer::Link { link: seen, members } if seen.id == link.id => Some(members),
            _ => None,
        });
        match seen {
            Some(members) => members.push(s.id),
            None => out.push(Layer::Link { link, members: vec![s.id] }),
        }
    }
    out
}

/// The inspector's lower half: shapes top of the z-order first, each a flat row with a colour dot, a
/// name, and hide, reorder and delete controls. A Group or Attach shows as a collapsible parent row with
/// its members indented beneath. The active row is an accent_hover wash, with no borders. Only the rows
/// in view are laid out.
pub fn show(ui: &mut Ui, design: &mut Design, selection: &mut Vec<u64>, units: crate::units::Units, contour_open: &mut bool) {
    let anchor_id = ui.id().with("anchor");
    let seen_id = ui.id().with("seen-selection");
    let seen_rename_id = ui.id().with("seen-rename");
    // A selection made elsewhere, on the canvas say, scrolls its first row into view, and so does a
    // rename begun on a row that is not.
    let reveal = ui.data(|d| d.get_temp::<Vec<u64>>(seen_id)).as_ref() != Some(selection);
    let typing = match ui.data(|d| d.get_temp::<Renaming>(renaming_id())) {
        Some(Renaming::Typing { target, .. }) => Some(target),
        _ => None,
    };
    let reveal_rename = typing.filter(|_| ui.data(|d| d.get_temp::<Option<Named>>(seen_rename_id)).flatten() != typing);
    let mut listed = Vec::new();
    let mut action = None;
    let mut dragging = false;
    egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = GAP;
        super::anchors::mark(ui, super::anchors::Anchor::Layers, ui.max_rect());
        let list = drag::List { top: ui.cursor().top(), backdrop: ui.painter().add(egui::Shape::Noop) };
        if design.shapes.is_empty() {
            ui.label(egui::RichText::new("Nothing on the artboard yet").font(fonts::regular(12.0)).color(TEXT_MUTED));
        }
        let outline = Outline::new(ui, design, ui.cursor().top());
        ui.set_min_height(outline.height());
        let target = reveal_rename.and_then(|t| outline.row_of(t)).or_else(|| outline.first_selected(selection).filter(|_| reveal));
        if let Some(at) = target {
            ui.scroll_to_rect(outline.line(at, ui.max_rect().x_range()), None);
        }
        action = paint(ui, design, selection, &outline, units);
        let start = action.as_ref().and_then(|(act, at)| matches!(act, RowAction::Drag).then_some(*at));
        dragging = drag::update(ui, design, selection, &outline.rows, &outline.slots, &outline.tops, list, start);
        listed = outline.rows;
    });
    let rows = listed;
    if dragging {
        action = None;
    }
    if let Some((act, at)) = action {
        match act {
            RowAction::Select => {
                let click = ui.input(|i| match (i.modifiers.shift, i.modifiers.command) {
                    (true, _) => Click::Range,
                    (false, true) => Click::Toggle,
                    (false, false) => Click::Only,
                });
                let anchor = ui.data(|d| d.get_temp::<RowKey>(anchor_id)).and_then(|k| rows.iter().position(|(key, _)| *key == k));
                let ids: Vec<Vec<u64>> = rows.iter().map(|(_, ids)| ids.clone()).collect();
                let anchor = click_rows(&ids, anchor, at, click, selection);
                ui.data_mut(|d| d.insert_temp(anchor_id, rows[anchor].0));
            }
            RowAction::Context(pos) => {
                ui.data_mut(|d| d.insert_temp(anchor_id, rows[at].0));
                super::context_menu::open_on(ui.ctx(), pos, &rows[at].1, selection);
            }
            RowAction::Edit(edit) => _ = crate::edit::run(design, selection, &edit, Subject::Ids(&rows[at].1)),
            RowAction::Effect(id, effects_list::Act::Toggle) => ui.data_mut(|d| {
                let open = d.get_temp_mut_or(effects_open(id), false);
                *open = !*open;
            }),
            RowAction::Effect(id, act) => {
                if matches!(act, effects_list::Act::Open(..)) {
                    ui.data_mut(|d| d.insert_temp(effects_open(id), true));
                }
                effects_list::run(ui.ctx(), design, selection, id, act, contour_open);
            }
            RowAction::Rename => begin_rename(ui.ctx(), design, &rows[at].1),
            RowAction::ToggleCollapsed | RowAction::Drag => {}
        }
    }
    let listed: Vec<Named> = rows
        .iter()
        .filter_map(|(key, _)| match *key {
            RowKey::Shape(id) => Some(Named::Shape(id)),
            RowKey::Link(id) => Some(Named::Link(id)),
            RowKey::Part(_) => None,
        })
        .collect();
    finish_rename(ui, design, selection, &listed);
    ui.data_mut(|d| {
        d.insert_temp(seen_id, selection.clone());
        d.insert_temp(seen_rename_id, typing);
    });
}

/// Whether shape `id`'s effect list is open under its Layers row.
fn effects_open(id: u64) -> egui::Id {
    egui::Id::new(("layer-effects-open", id))
}

/// Whether the Group or Attach `id`'s members are folded away.
fn collapsed_id(id: u64) -> egui::Id {
    egui::Id::new(("layer-collapsed", id))
}

/// `shape`'s row: its name line, and the chips under it when it lists effects.
fn row_height(shape: &Shape) -> f32 {
    if effects_list::any(shape) { ROW_H + effects_list::CHIPS_H } else { ROW_H }
}

/// Draws the rows of `outline` that reach into view, and returns the last action one of them raised with
/// the index of its row.
fn paint(ui: &mut Ui, design: &Design, selection: &[u64], outline: &Outline, units: crate::units::Units) -> Option<(RowAction, usize)> {
    let rows = outline.visible(ui.clip_rect());
    if rows.is_empty() {
        return None;
    }
    let area = Rect::from_x_y_ranges(ui.max_rect().x_range(), outline.tops[rows.start]..=outline.tops[rows.end - 1] + ROW_H);
    ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
        let mut out = None;
        let mut parts: (Option<usize>, Vec<Shape>) = (None, Vec::new());
        for at in rows {
            let indent = outline.slots[at].depth as f32 * INDENT;
            let act = match outline.kinds[at] {
                Kind::Shape { index, open } => {
                    let shape = &design.shapes[index];
                    let mut act = row(ui, design, shape, selection.contains(&shape.id), indent, open, units);
                    if open && let Some(effect) = effects_list::list(ui, design, shape, units, indent + 8.0) {
                        act = Some(RowAction::Effect(shape.id, effect));
                    }
                    act
                }
                Kind::Link { link, collapsed } => {
                    let members = &outline.rows[at].1;
                    let act = link_row(ui, design, link, members, members.iter().all(|m| selection.contains(m)), collapsed, indent);
                    if matches!(act, Some(RowAction::ToggleCollapsed)) {
                        ui.data_mut(|d| d.insert_temp(collapsed_id(link.id), !collapsed));
                    }
                    act
                }
                Kind::Part { instance, part } => {
                    let owner = &design.shapes[instance];
                    if parts.0 != Some(instance) {
                        parts = (Some(instance), crate::components::expand(design, owner).unwrap_or_default());
                    }
                    part_row(ui, design, &parts.1[part], owner.id, indent)
                }
            };
            if let Some(act) = act {
                out = Some((act, at));
            }
        }
        out
    })
    .inner
}

fn row(ui: &mut Ui, design: &Design, shape: &Shape, selected: bool, indent: f32, effects_open: bool, units: crate::units::Units) -> Option<RowAction> {
    let id = shape.id;
    let name = &design.layer_name(shape);
    let chips = effects_list::any(shape);
    let (full, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_height(shape)), Sense::hover());
    let rect = full.with_min_x(full.min.x + indent);
    // The name line; a row with effects carries its chips on a second line under the name.
    let line = rect.with_max_y(rect.top() + ROW_H);
    let resp = ui.interact(rect, ui.id().with((id, "row")), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, format!("Layer {name}")));
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if shape.hidden { DISABLED_TEXT } else { TEXT_PRIMARY };
    let y = line.center().y;
    let dot = pos2(rect.left() + 14.0, y);
    painter.circle(dot, 5.0, shape.fill, Stroke::new(1.0_f32, BORDER_SUBTLE));
    super::icons::paint_icon(ui, super::toolbar::operation_icon(shape.operation), Rect::from_center_size(pos2(rect.left() + 34.0, y), Vec2::splat(14.0)), TEXT_MUTED);
    let mut name_x = rect.left() + 48.0;
    let mut picked_op = None;
    match shape.compound() {
        Some(CompoundOp::Boolean(current)) => {
            picked_op = op_dropdown(ui, design, Rect::from_min_size(pos2(rect.left() + 42.0, y - 10.0), vec2(26.0, 20.0)), id, current);
            name_x += 24.0;
        }
        Some(CompoundOp::Effects) => {
            effect_group_badge(ui, pos2(rect.left() + 52.0, y), id);
            name_x += 14.0;
        }
        None if shape.container.is_some() => {
            container_badge(ui, pos2(rect.left() + 52.0, y), id);
            name_x += 14.0;
        }
        None if shape.instance_of().is_some() => {
            component_badge(ui, pos2(rect.left() + 52.0, y), id);
            name_x += 14.0;
        }
        None => {
            if let Some(family) = shape.kind.system_family().filter(|f| crate::system_fonts::is_missing(f)) {
                missing_font_badge(ui, pos2(rect.left() + 52.0, y), id, family);
                name_x += 14.0;
            }
        }
    }

    let mut out = select_or_context(&resp);
    let edge = rect.max.x;
    let first_x = if lock_shown(ui, rect, shape.locked) { edge - 100.0 } else { edge - 78.0 };
    if let Some(act) = name_or_field(ui, &resp, Named::Shape(id), name_slot(line, name_x, first_x), name, fonts::regular(13.0), ink) {
        out = Some(act);
    }
    if chips
        && let Some(act) = effects_list::chips(ui, design, shape, units, rect.left() + 44.0, line.bottom() + effects_list::CHIPS_H / 2.0 - 4.0, edge - 4.0, effects_open)
    {
        out = Some(RowAction::Effect(id, act));
    }
    if icon_btn(ui, pos2(edge - 14.0, y), id, Icon::Trash, &format!("Delete {name}")) {
        out = Some(RowAction::Edit(Edit::Delete));
    }
    if chevron_btn(ui, pos2(edge - 36.0, y), id, "down", 0.0, &format!("Move {name} down")) {
        out = Some(RowAction::Edit(Edit::Arrange(Arrange::Backward)));
    }
    if chevron_btn(ui, pos2(edge - 56.0, y), id, "up", std::f32::consts::PI, &format!("Move {name} up")) {
        out = Some(RowAction::Edit(Edit::Arrange(Arrange::Forward)));
    }
    if eye_btn(ui, pos2(edge - 78.0, y), id, shape.hidden, name) {
        out = Some(RowAction::Edit(Edit::SetHidden(!shape.hidden)));
    }
    if let Some(on) = lock_btn(ui, rect, pos2(edge - 100.0, y), id, shape.locked, name) {
        out = Some(RowAction::Edit(Edit::Lock { lock: Lock::Layer, on }));
    }
    if let Some(op) = picked_op {
        out = Some(RowAction::Edit(Edit::BooleanOp(op)));
    }
    out
}

/// One part of instance `instance`, read-only: its colour, operation and name. Clicking it, or dragging
/// it, acts on the instance; parts never move on their own.
fn part_row(ui: &mut Ui, design: &Design, part: &Shape, instance: u64, indent: f32) -> Option<RowAction> {
    let name = design.layer_name(part);
    let (full, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let rect = full.with_min_x(full.min.x + indent);
    let resp = ui.interact(rect, ui.id().with((instance, part.id, "part-row")), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Part {name}")));
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let y = rect.center().y;
    painter.circle(pos2(rect.left() + 14.0, y), 5.0, part.fill, Stroke::new(1.0_f32, BORDER_SUBTLE));
    super::icons::paint_icon(ui, super::toolbar::operation_icon(part.operation), Rect::from_center_size(pos2(rect.left() + 34.0, y), Vec2::splat(14.0)), TEXT_MUTED);
    let ink = if part.hidden { DISABLED_TEXT } else { TEXT_MUTED };
    row_name(ui, &resp, Rect::from_x_y_ranges((rect.left() + 48.0)..=(rect.right() - NAME_GAP), rect.y_range()), &name, fonts::regular(13.0), ink);
    select_or_context(&resp)
}

/// Four small diamonds before an instance's name: the component mark.
fn component_badge(ui: &Ui, centre: Pos2, id: u64) {
    let r = Rect::from_center_size(centre, Vec2::splat(14.0));
    let resp = ui.interact(r, ui.id().with((id, "component-badge")), Sense::hover());
    let resp = super::tooltip::tip(resp, "Component", "An instance of a component. Its parts, listed under it, follow the master.", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Component"));
    for (dx, dy) in [(0.0, -3.5), (3.5, 0.0), (0.0, 3.5), (-3.5, 0.0)] {
        let c = centre + vec2(dx, dy);
        let diamond = [c + vec2(0.0, -2.6), c + vec2(2.6, 0.0), c + vec2(0.0, 2.6), c + vec2(-2.6, 0.0)];
        ui.painter().add(egui::Shape::convex_polygon(diamond.to_vec(), ACCENT_PRIMARY, Stroke::NONE));
    }
}

/// A warning mark before the name of a text whose system family is not installed, so it lays out in Inter.
fn missing_font_badge(ui: &Ui, centre: Pos2, id: u64, family: &str) {
    let r = Rect::from_center_size(centre, Vec2::splat(14.0));
    let resp = ui.interact(r, ui.id().with((id, "missing-font-badge")), Sense::hover());
    let resp = super::tooltip::tip(resp, "Font not installed", &format!("{family} is not installed here, so this text lays out and cuts in Inter."), None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Font not installed"));
    super::icons::paint_icon(ui, Icon::Warning, r, WARNING_TEXT);
}

/// The Offset icon before an effect group's name.
fn effect_group_badge(ui: &Ui, centre: Pos2, id: u64) {
    let r = Rect::from_center_size(centre, Vec2::splat(14.0));
    let resp = ui.interact(r, ui.id().with((id, "effect-group-badge")), Sense::hover());
    let resp = super::tooltip::tip(resp, "Effect group", "Its effects apply to the objects listed under it together.", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Effect group"));
    super::icons::paint_icon(ui, Icon::Offset, r, ACCENT_PRIMARY);
}

/// A boolean group's op before its name: the op's icon and a chevron in a small bordered box that drops
/// a list of the six ops, the current one ticked and any the children cannot take greyed with the
/// reason. Returns the op picked.
fn op_dropdown(ui: &Ui, design: &Design, r: Rect, id: u64, current: Combine) -> Option<Combine> {
    let resp = ui.interact(r, ui.id().with((id, "boolean-op")), Sense::click());
    let resp = super::tooltip::tip(resp, current.label(), "Combines the objects listed under it, which stay editable. Click to change how.", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Operation {}", current.label())));
    let open = ui.memory(|m| m.is_popup_open(resp.id.with("menu")));
    if resp.hovered() || open {
        ui.painter().rect_filled(r, 4.0, ACCENT_HOVER);
    }
    ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0_f32, if open { ACCENT_PRIMARY } else { BORDER_SUBTLE }), egui::StrokeKind::Inside);
    let y = r.center().y;
    let icon = super::commands::info(&super::commands::Command::Edit(Edit::BooleanOp(current))).icon.unwrap_or(Icon::Combine);
    super::icons::paint_icon(ui, icon, Rect::from_center_size(pos2(r.left() + 9.0, y), Vec2::splat(13.0)), ACCENT_PRIMARY);
    let chevron = if resp.hovered() || open { TEXT_PRIMARY } else { TEXT_MUTED };
    super::icons::paint_icon(ui, Icon::Chevron, Rect::from_center_size(pos2(r.right() - 7.0, y), Vec2::splat(9.0)), chevron);
    let cx = Cx::new(design, std::slice::from_ref(&id), AlignTo::Selection);
    super::popover::popover(ui, &resp, "menu", 168.0, |ui| {
        let mut picked = None;
        for op in Combine::ALL {
            if op_choice(ui, op, op == current, &cx.entry(Edit::BooleanOp(op))) {
                picked = Some(op);
            }
        }
        picked
    })
}

/// One op in the dropdown: a tick for the current op, its icon and name. Greyed with the reason on hover
/// while the group cannot take it. Returns whether an enabled row was clicked.
fn op_choice(ui: &mut Ui, op: Combine, current: bool, entry: &Entry) -> bool {
    let enabled = entry.enabled();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, current, op.label()));
    let resp = if enabled { resp } else { super::tooltip::tip(resp, op.label(), &entry.tip_body(), None) };
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let y = rect.center().y;
    if current {
        super::icons::paint_icon(ui, Icon::Tick, Rect::from_center_size(pos2(rect.left() + 14.0, y), Vec2::splat(16.0)), ACCENT_PRIMARY);
    }
    let ink = match (enabled, current) {
        (false, _) => DISABLED_TEXT,
        (true, true) => ACCENT_PRIMARY,
        (true, false) => TEXT_PRIMARY,
    };
    super::icons::paint_icon(ui, entry.info.icon.unwrap_or(Icon::Combine), Rect::from_center_size(pos2(rect.left() + 40.0, y), Vec2::splat(20.0)), ink);
    text(ui.painter(), pos2(rect.left() + 58.0, y), Align2::LEFT_CENTER, op.label(), fonts::regular(14.0), ink);
    enabled && resp.clicked()
}

/// The parent row of a Group or Attach: a collapse chevron, the link's name, and eye and delete buttons
/// that act on every member.
fn link_row(ui: &mut Ui, design: &Design, link: Link, members: &[u64], selected: bool, collapsed: bool, indent: f32) -> Option<RowAction> {
    let name = &design.link_name(link);
    let (full, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let rect = full.with_min_x(full.min.x + indent);
    let resp = ui.interact(rect, ui.id().with((link.id, "link-row")), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, format!("Layer {name}")));
    let hidden = design.shapes.iter().filter(|s| members.contains(&s.id)).all(|s| s.hidden);
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if hidden { DISABLED_TEXT } else { TEXT_PRIMARY };

    let mut out = select_or_context(&resp);
    let y = rect.center().y;
    let locked = design.shapes.iter().filter(|s| members.contains(&s.id)).all(|s| s.locked);
    let first_x = if lock_shown(ui, rect, locked) { rect.max.x - 100.0 } else { rect.max.x - 78.0 };
    if let Some(act) = name_or_field(ui, &resp, Named::Link(link.id), name_slot(rect, rect.left() + 28.0, first_x), name, fonts::semibold(13.0), ink) {
        out = Some(act);
    }
    let (verb, turn) = if collapsed { ("Expand", -std::f32::consts::FRAC_PI_2) } else { ("Collapse", 0.0) };
    if chevron_btn(ui, pos2(rect.left() + 14.0, y), link.id, "collapse", turn, &format!("{verb} {name}")) {
        out = Some(RowAction::ToggleCollapsed);
    }
    if icon_btn(ui, pos2(rect.max.x - 14.0, y), link.id, Icon::Trash, &format!("Delete {name}")) {
        out = Some(RowAction::Edit(Edit::Delete));
    }
    if eye_btn(ui, pos2(rect.max.x - 78.0, y), link.id, hidden, name) {
        out = Some(RowAction::Edit(Edit::SetHidden(!hidden)));
    }
    if let Some(on) = lock_btn(ui, rect, pos2(rect.max.x - 100.0, y), link.id, locked, name) {
        out = Some(RowAction::Edit(Edit::Lock { lock: Lock::Layer, on }));
    }
    out
}

/// A small dashed frame before a container's name, matching its dashed padding frame on the canvas.
fn container_badge(ui: &Ui, centre: Pos2, id: u64) {
    let r = Rect::from_center_size(centre, Vec2::splat(12.0));
    let resp = super::tooltip::tip(ui.interact(r, ui.id().with((id, "container-badge")), Sense::hover()), "Container", "Holds the objects listed under it.", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Container"));
    let b = r.shrink(2.0);
    let corners = [b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom(), b.left_top()];
    ui.painter().extend(egui::Shape::dashed_line(&corners, Stroke::new(1.2_f32, ACCENT_PRIMARY), 2.5, 1.5));
}

fn select_or_context(resp: &egui::Response) -> Option<RowAction> {
    if resp.secondary_clicked() {
        return resp.interact_pointer_pos().map(RowAction::Context);
    }
    if drag::past_threshold(resp) {
        return Some(RowAction::Drag);
    }
    resp.clicked().then_some(RowAction::Select)
}

fn icon_btn(ui: &mut Ui, centre: Pos2, id: u64, icon: Icon, label: &str) -> bool {
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let resp = super::tooltip::tip(ui.interact(r, ui.id().with((id, label)), Sense::click()), label, "", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    super::icons::paint_icon(ui, icon, Rect::from_center_size(centre, Vec2::splat(15.0)), ink);
    resp.clicked()
}

fn chevron_btn(ui: &mut Ui, centre: Pos2, id: u64, tag: &str, radians: f32, label: &str) -> bool {
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let resp = super::tooltip::tip(ui.interact(r, ui.id().with((id, tag)), Sense::click()), label, "", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    super::icons::paint_icon_rotated(ui, Icon::Chevron, Rect::from_center_size(centre, Vec2::splat(15.0)), ink, radians);
    resp.clicked()
}

/// The layer lock, shown while the pointer is over `row` and always while locked. Returns the lock
/// state a click asks for.
fn lock_btn(ui: &mut Ui, row: Rect, centre: Pos2, id: u64, locked: bool, name: &str) -> Option<bool> {
    if !lock_shown(ui, row, locked) {
        return None;
    }
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let (title, body) = if locked {
        ("Unlock layer", "Let the canvas select this layer again.")
    } else {
        ("Lock layer", "Stop the canvas selecting this layer by click, marquee or Select All. Layers can still select it.")
    };
    let resp = super::tooltip::tip(ui.interact(r, ui.id().with((id, "lock")), Sense::click()), title, body, None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} {name}", if locked { "Unlock" } else { "Lock" })));
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    super::icons::paint_icon(ui, if locked { Icon::Lock } else { Icon::Unlock }, Rect::from_center_size(centre, Vec2::splat(14.0)), ink);
    resp.clicked().then_some(!locked)
}

/// Whether `row` shows its lock: while the pointer is over it, and always while locked.
fn lock_shown(ui: &Ui, row: Rect, locked: bool) -> bool {
    locked || ui.rect_contains_pointer(row)
}

fn eye_btn(ui: &mut Ui, centre: Pos2, id: u64, hidden: bool, name: &str) -> bool {
    let r = Rect::from_center_size(centre, Vec2::splat(18.0));
    let resp = ui.interact(r, ui.id().with((id, "eye")), Sense::click());
    let resp = super::tooltip::tip(resp, if hidden { "Show" } else { "Hide" }, "Hidden objects are left out of Make and the Project Preview.", None);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, if hidden { format!("Show {name}") } else { format!("Hide {name}") }));
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    let painter = ui.painter();
    let stroke = Stroke::new(1.3_f32, ink);
    // An almond eye: two arcs meeting at the corners, with a pupil.
    let lid = |sign: f32| -> Vec<Pos2> {
        (0..=10)
            .map(|i| {
                let x = -7.0 + 14.0 * i as f32 / 10.0;
                centre + vec2(x, sign * 4.0 * (1.0 - (x / 7.0).powi(2)))
            })
            .collect()
    };
    painter.add(egui::Shape::line(lid(-1.0), stroke));
    painter.add(egui::Shape::line(lid(1.0), stroke));
    painter.circle(centre, 2.2, Color32::TRANSPARENT, stroke);
    if hidden {
        painter.line_segment([centre + vec2(-8.0, -7.0), centre + vec2(8.0, 7.0)], stroke);
    }
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    #[test]
    fn a_link_lists_once_at_its_top_member_with_members_top_first() {
        let mut d = Design::default();
        for _ in 0..4 {
            d.add_shape(ShapeKind::Square);
        }
        let id = d.attach(&[1, 3]).unwrap();
        let link = d.shapes.iter().find_map(|s| s.link.filter(|l| l.id == id)).unwrap();
        assert_eq!(layers(&d), vec![Layer::Shape(4), Layer::Link { link, members: vec![3, 1] }, Layer::Shape(2)]);
    }

    #[test]
    fn a_container_lists_its_children_under_it_and_a_link_inside_it() {
        let mut d = Design::default();
        for _ in 0..4 {
            d.add_shape(ShapeKind::Square);
        }
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2, 3], Some(c));
        let id = d.group(&[2, 3]).unwrap();
        let link = d.shapes.iter().find_map(|s| s.link.filter(|l| l.id == id)).unwrap();
        let inner = vec![Layer::Link { link, members: vec![3, 2] }];
        assert_eq!(layers(&d), vec![Layer::Shape(4), Layer::Container { id: c, children: inner }]);
    }

    fn rows() -> Vec<Vec<u64>> {
        vec![vec![5], vec![3, 4], vec![3], vec![4], vec![1]]
    }

    #[test]
    fn a_plain_click_selects_only_that_row_and_moves_the_anchor() {
        let mut sel = vec![5, 1];
        assert_eq!(click_rows(&rows(), Some(0), 1, Click::Only, &mut sel), 1);
        assert_eq!(sel, vec![3, 4], "a parent row selects the whole link");
    }

    #[test]
    fn shift_selects_the_range_from_the_anchor_either_way_and_keeps_it() {
        let mut sel = vec![];
        assert_eq!(click_rows(&rows(), Some(4), 0, Click::Range, &mut sel), 4);
        assert_eq!(sel, vec![5, 3, 4, 1], "each id once, list order");
        assert_eq!(click_rows(&rows(), Some(0), 3, Click::Range, &mut sel), 0);
        assert_eq!(sel, vec![5, 3, 4]);
    }

    #[test]
    fn shift_without_an_anchor_is_a_plain_click() {
        let mut sel = vec![1];
        assert_eq!(click_rows(&rows(), None, 2, Click::Range, &mut sel), 2);
        assert_eq!(sel, vec![3]);
    }

    #[test]
    fn cmd_toggles_a_row_in_and_out() {
        let mut sel = vec![5];
        assert_eq!(click_rows(&rows(), Some(0), 4, Click::Toggle, &mut sel), 4);
        assert_eq!(sel, vec![5, 1]);
        click_rows(&rows(), Some(4), 0, Click::Toggle, &mut sel);
        assert_eq!(sel, vec![1]);
        click_rows(&rows(), Some(0), 1, Click::Toggle, &mut sel);
        assert_eq!(sel, vec![1, 3, 4], "a parent row toggles every member");
    }
}

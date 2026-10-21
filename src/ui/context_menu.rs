//! Right-click menus for shapes on the canvas, Layers rows and empty canvas. The rows are a table of
//! edits; each row's label, shortcut and enablement come from `commands`, and a pick runs through
//! `edit::run` like every other surface.

use egui::{Align2, Area, Context, Id, Modifiers, Order, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::containers::AlignTo;
use crate::design::{Align, Arrange, Combine, Design, History};
use crate::edit::{AppCommand, Edit, TextPathEdit};
use crate::effects::{EffectEdit, EntryRef, Row as EffectRow, rows as effect_rows};
use crate::path_edit::{NodeAction, NodeKind, NodePath};
use crate::select::Match;
use crate::transform::Axis;
use crate::units::Units;

use super::chrome::text;
use super::commands::{Command, Cx, Entry};
use super::fonts;
use super::icons::{Icon, paint_icon_rotated};
use super::popover::{clicked_outside, menu_frame};
use super::tokens::*;

const WIDTH: f32 = 220.0;
const SUB_WIDTH: f32 = 200.0;
const ROW_H: f32 = 32.0;

/// What a menu pick asks for: an edit of the selection, an app command, or an action on the selected nodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Picked {
    Edit(Edit),
    App(AppCommand),
    Clipboard(super::clipboard::Action),
    Node(NodeAction),
    /// Edit effect: open the effect's panel beside the menu, at the point it was opened.
    EditEffect(EntryRef, Pos2),
}

pub(super) enum Row {
    Item(Edit),
    /// An app command that reads only the design and the selection, never the history: Rename.
    App(AppCommand),
    Clipboard(super::clipboard::Action),
    /// The second edit when it can run, otherwise the first: Group or Ungroup, Flatten or Unflatten.
    Either(Edit, Edit),
    Sub(&'static str, &'static [Edit]),
    /// One of the effect rows, built from the selection's own effects when the menu opens.
    Effect(EffectAct),
    Separator,
}

/// What an effect row does to the effect it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EffectAct {
    Edit,
    Toggle,
    Remove,
}

/// One pick of a submenu, or of a row built from the selection.
struct Pick {
    label: String,
    enabled: bool,
    picked: Picked,
}

const COMBINE: [Edit; 6] = [
    Edit::Combine(Combine::Slice),
    Edit::Combine(Combine::Weld),
    Edit::Combine(Combine::Unite),
    Edit::Combine(Combine::Subtract),
    Edit::Combine(Combine::Intersect),
    Edit::Combine(Combine::Exclude),
];
const ARRANGE: [Edit; 4] = [Edit::Arrange(Arrange::ToFront), Edit::Arrange(Arrange::Forward), Edit::Arrange(Arrange::Backward), Edit::Arrange(Arrange::ToBack)];
const SELECT_SAME: [Edit; 9] = [
    Edit::SelectSame(Match::Colour),
    Edit::SelectSame(Match::Operation),
    Edit::SelectSame(Match::Pen),
    Edit::SelectSame(Match::Font),
    Edit::SelectSame(Match::Kind),
    Edit::SelectSame(Match::Effects),
    Edit::SelectSame(Match::Everything),
    Edit::SelectSame(Match::Text),
    Edit::SelectSame(Match::Paths),
];
const FLIP: [Edit; 2] = [Edit::Flip(Axis::Horizontal), Edit::Flip(Axis::Vertical)];
/// The target is resolved against the Align To choice when the row is shown.
const fn align(op: Align) -> Edit {
    Edit::Align { op, to: AlignTo::Selection }
}
const ALIGN: [Edit; 9] = [
    align(Align::Left),
    align(Align::HCentre),
    align(Align::Right),
    align(Align::Top),
    align(Align::VCentre),
    align(Align::Bottom),
    align(Align::Centre),
    align(Align::DistributeH),
    align(Align::DistributeV),
];

pub(super) const SHAPE_MENU: &[Row] = &[
    Row::App(AppCommand::Rename),
    Row::Item(Edit::Duplicate),
    Row::Clipboard(super::clipboard::Action::Cut),
    Row::Clipboard(super::clipboard::Action::Copy),
    Row::Clipboard(super::clipboard::Action::CopyAsImage),
    Row::Clipboard(super::clipboard::Action::Paste),
    Row::Item(Edit::Delete),
    Row::Separator,
    Row::Either(Edit::Group, Edit::Ungroup),
    Row::Either(Edit::Attach, Edit::Detach),
    Row::Item(Edit::Contain),
    Row::Item(Edit::Uncontain),
    Row::Item(Edit::MakeComponent),
    Row::Item(Edit::DetachInstance),
    Row::App(AppCommand::EditMonogram),
    Row::Separator,
    Row::Sub("Combine", &COMBINE),
    Row::Either(Edit::Flatten, Edit::Unflatten),
    Row::Item(Edit::Expand),
    Row::Effect(EffectAct::Edit),
    Row::Effect(EffectAct::Toggle),
    Row::Effect(EffectAct::Remove),
    Row::Item(Edit::TextPath(TextPathEdit::Remove)),
    Row::Separator,
    Row::App(AppCommand::StepRepeatPanel),
    Row::App(AppCommand::RotateCopiesPanel),
    Row::Item(Edit::Pivot(None)),
    Row::Sub("Arrange", &ARRANGE),
    Row::Sub("Flip", &FLIP),
    Row::Sub("Align", &ALIGN),
    Row::Separator,
    Row::Item(Edit::SetHidden(true)),
    Row::Sub("Select Same", &SELECT_SAME),
    Row::Item(Edit::SelectInverse),
    Row::Item(Edit::SelectAll),
];

pub(super) const EMPTY_MENU: &[Row] = &[Row::Clipboard(super::clipboard::Action::Paste), Row::Item(Edit::SelectAll), Row::Item(Edit::Deselect)];

const NODE_MENU: &[Option<NodeAction>] = &[
    Some(NodeAction::Convert(NodeKind::Straight)),
    Some(NodeAction::Convert(NodeKind::Cusp)),
    Some(NodeAction::Convert(NodeKind::Smooth)),
    Some(NodeAction::Convert(NodeKind::Symmetric)),
    None,
    Some(NodeAction::AddMidpoints),
    Some(NodeAction::Remove),
    None,
    Some(NodeAction::OpenAt),
    Some(NodeAction::Close),
];

/// The entry a menu row shows for `edit`, with an Align target resolved.
pub(super) fn resolve(cx: &Cx, edit: &Edit) -> Entry {
    match edit {
        Edit::Align { op, .. } => cx.entry(cx.align(*op)),
        e => cx.entry(e.clone()),
    }
}

/// The entry an `Item` or `Either` row shows; `None` for a submenu or separator.
pub(super) fn shown(row: &Row, cx: &Cx) -> Option<Entry> {
    match row {
        Row::Item(e) => Some(resolve(cx, e)),
        Row::Either(a, b) => Some(cx.either(a.clone(), b.clone())),
        Row::App(app) => Some(cx.app(app.clone(), &History::default())),
        Row::Clipboard(action) => Some(cx.clipboard(*action)),
        Row::Sub(..) | Row::Effect(_) | Row::Separator => None,
    }
}

/// What an effect row offers for the selection: the row's own label, and one pick per effect row of the
/// selected shapes (Contour stays in Layers, whose panel it opens). One pick runs from the row itself;
/// several fold into a submenu; none greys the row.
fn effect_picks(cx: &Cx, act: EffectAct) -> (&'static str, Vec<Pick>) {
    let shapes: Vec<_> = cx.design.shapes.iter().filter(|s| cx.on.contains(&s.id)).collect();
    let several = shapes.len() > 1;
    let mut label = match act {
        EffectAct::Edit => "Edit effect",
        EffectAct::Toggle => "Disable effect",
        EffectAct::Remove => "Remove effect",
    };
    let mut picks = Vec::new();
    let mut any_on = false;
    let mut any_off = false;
    for shape in shapes {
        for row in effect_rows(shape, &cx.design.geometry(shape), Units::Imperial) {
            let at = EntryRef { shape: shape.id, row: row.key };
            let name = if several { format!("{}: {}", cx.design.layer_name(shape), row.kind.name()) } else { row.kind.name().to_owned() };
            let (edit, picked, name) = match act {
                EffectAct::Edit if row.key == EffectRow::Contour => continue,
                EffectAct::Edit => (None, Picked::EditEffect(at, Pos2::ZERO), name),
                EffectAct::Toggle => {
                    let (on, off) = if row.enabled { (&mut any_on, "Turn off") } else { (&mut any_off, "Turn on") };
                    *on = true;
                    let edit = Edit::Effect { at, op: EffectEdit::Enable(!row.enabled) };
                    (Some(edit.clone()), Picked::Edit(edit), format!("{off} {name}"))
                }
                EffectAct::Remove => {
                    let edit = Edit::Effect { at, op: EffectEdit::Remove };
                    (Some(edit.clone()), Picked::Edit(edit), name)
                }
            };
            let enabled = edit.is_none_or(|e| cx.design.check(&e, cx.on).is_ok());
            picks.push(Pick { label: name, enabled, picked });
        }
    }
    if act == EffectAct::Toggle {
        label = match (any_on, any_off) {
            (false, true) => "Enable effect",
            (true, true) => "Disable or enable effect",
            _ => label,
        };
    }
    (label, picks)
}

/// The label and picks of a `Sub` row or an effect row, built for this frame.
fn sub_picks(row: &Row, cx: &Cx) -> (&'static str, Vec<Pick>) {
    match row {
        Row::Sub(label, items) => {
            let picks = items.iter().map(|edit| {
                let e = resolve(cx, edit);
                Pick { label: e.label.to_owned(), enabled: e.enabled(), picked: Picked::Edit(e.edit().clone()) }
            });
            (label, picks.collect())
        }
        Row::Effect(act) => effect_picks(cx, *act),
        _ => unreachable!("only a Sub or an Effect row has picks"),
    }
}

/// Which menu is open: the one for the selection, or the one for empty canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Selection,
    Empty,
    Nodes,
    /// An instance's part, by the master shape it stands for: Reset this part.
    Part(u64),
}

#[derive(Clone, Copy)]
struct Open {
    at: Pos2,
    target: Target,
    /// The index in the menu table of the submenu row that is open.
    sub: Option<usize>,
    /// The pass it opened in, so the right-click that opened it does not also close it.
    pass: u64,
}

fn state_id() -> Id {
    Id::new("context-menu")
}

/// Whether a context menu is showing.
pub fn is_open(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<Open>(state_id()).is_some())
}

fn open(ctx: &Context, at: Pos2, target: Target) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(state_id(), Open { at, target, sub: None, pass }));
}

/// A right-click on an object whose left click would select `ids`: selects them first unless they are
/// already all selected, so a menu over a multiple selection acts on all of it.
pub fn open_on(ctx: &Context, at: Pos2, ids: &[u64], selection: &mut Vec<u64>) {
    if !ids.iter().all(|id| selection.contains(id)) {
        *selection = ids.to_vec();
    }
    open(ctx, at, Target::Selection);
}

/// A right-click on the part of an instance that stands for master shape `part`: selects the instance,
/// then offers Reset this part.
pub fn open_part(ctx: &Context, at: Pos2, instance: u64, part: u64, selection: &mut Vec<u64>) {
    *selection = vec![instance];
    open(ctx, at, Target::Part(part));
}

/// A right-click on a node while editing a path's nodes.
pub fn open_nodes(ctx: &Context, at: Pos2) {
    open(ctx, at, Target::Nodes);
}

/// A right-click on the canvas at `at`, `at_in` in inches: a shape's menu, or the empty-canvas menu.
pub fn on_canvas(ctx: &Context, design: &Design, selection: &mut Vec<u64>, at: Pos2, at_in: (f32, f32)) {
    match design.pick_at(at_in.0, at_in.1) {
        Some(id) => open_on(ctx, at, &design.expand(&[id]), selection),
        None => open(ctx, at, Target::Empty),
    }
}

/// macOS reads Ctrl-click as a right-click. Rewrites each such press and its release to the secondary
/// button before egui sees them.
pub fn ctrl_click_is_secondary(ctx: &Context, raw: &mut egui::RawInput) {
    let id = Id::new("ctrl-click");
    let mut held = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    for event in &mut raw.events {
        let egui::Event::PointerButton { button, pressed, modifiers, .. } = event else { continue };
        if *button != egui::PointerButton::Primary {
            continue;
        }
        if *pressed {
            // Off macOS Ctrl is Command, so this never fires there.
            held = modifiers.ctrl && !modifiers.command;
        }
        if held {
            *button = egui::PointerButton::Secondary;
        }
        if !*pressed {
            held = false;
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, held));
}

/// Paints the open menu, if any, and returns the item picked. Escape, a click elsewhere or a pick closes it.
/// `nodes` is the node edit in progress, which the node menu acts on.
pub fn show(ctx: &Context, design: &Design, selection: &[u64], nodes: Option<&super::node_edit::NodeEdit>, align_to: AlignTo) -> Option<Picked> {
    let mut st = ctx.data(|d| d.get_temp::<Open>(state_id()))?;
    let cx = Cx::new(design, selection, align_to);
    let mut picked = None;
    let mut sub_rect = None;
    let mut sub: Vec<Pick> = Vec::new();
    let main = Area::new(state_id()).order(Order::Foreground).fixed_pos(st.at).show(ctx, |ui| {
        menu_frame().show(ui, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            match st.target {
                Target::Nodes => picked = node_rows(ui, design, nodes),
                Target::Part(part) => {
                    let e = cx.entry(Edit::ResetPart(part));
                    if menu_row(ui, e.label, None, e.enabled(), false, false).clicked() && e.enabled() {
                        picked = Some(Picked::Edit(e.edit().clone()));
                    }
                }
                Target::Selection | Target::Empty => {
                    let rows = if st.target == Target::Selection { SHAPE_MENU } else { EMPTY_MENU };
                    for (i, row) in rows.iter().enumerate() {
                        match row {
                            Row::Separator => separator(ui),
                            Row::Sub(..) | Row::Effect(_) => {
                                let (label, picks) = sub_picks(row, &cx);
                                let on = picks.iter().any(|p| p.enabled);
                                // An effect row with one effect acts on it from the row itself.
                                if matches!(row, Row::Effect(_)) && picks.len() <= 1 {
                                    let resp = menu_row(ui, label, None, on, false, false);
                                    if resp.hovered() {
                                        st.sub = None;
                                    }
                                    if on && resp.clicked() {
                                        picked = picks.into_iter().next().map(|p| p.picked);
                                    }
                                    continue;
                                }
                                let resp = menu_row(ui, label, None, on, true, st.sub == Some(i));
                                if on && (resp.hovered() || resp.clicked()) {
                                    st.sub = Some(i);
                                }
                                if st.sub == Some(i) {
                                    sub_rect = Some(resp.rect);
                                    sub = picks;
                                }
                            }
                            Row::Item(_) | Row::Either(..) | Row::App(_) | Row::Clipboard(_) => {
                                let e = shown(row, &cx).expect("an item row");
                                let resp = menu_row(ui, e.label, e.keys(ctx).as_deref(), e.enabled(), false, false);
                                if resp.hovered() {
                                    st.sub = None;
                                }
                                if e.enabled() && resp.clicked() {
                                    picked = Some(match e.command {
                                        Command::Edit(edit) => Picked::Edit(edit),
                                        Command::App(app) => Picked::App(app),
                                        Command::Clipboard(action) => Picked::Clipboard(action),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        })
    });
    let mut keep = vec![main.response.rect];
    if let (false, Some(anchor)) = (sub.is_empty(), sub_rect) {
        let at = anchor.right_top() + vec2(8.0, -16.0);
        let menu = Area::new(state_id().with("sub")).order(Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            menu_frame().show(ui, |ui| {
                ui.set_width(SUB_WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                for pick in sub {
                    if menu_row(ui, &pick.label, None, pick.enabled, false, false).clicked() && pick.enabled {
                        picked = Some(pick.picked);
                    }
                }
            })
        });
        keep.push(menu.response.rect);
    }
    if let Some(Picked::EditEffect(_, at)) = picked.as_mut() {
        *at = st.at;
    }
    let dismissed = ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (st.pass != ctx.cumulative_pass_nr() && clicked_outside(ctx, &keep));
    if picked.is_some() || dismissed {
        ctx.data_mut(|d| d.remove::<Open>(state_id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(state_id(), st));
    }
    picked
}

/// The node menu's rows, each enabled by the path's own check.
fn node_rows(ui: &mut Ui, design: &Design, nodes: Option<&super::node_edit::NodeEdit>) -> Option<Picked> {
    let path = nodes.and_then(|e| design.shapes.iter().find(|s| s.id == e.shape).map(|s| (NodePath::read(s), &e.selected)));
    let mut picked = None;
    for row in NODE_MENU {
        let Some(action) = *row else {
            separator(ui);
            continue;
        };
        let on = path.as_ref().is_some_and(|(path, sel)| path.check(sel, action).is_ok());
        let keys = (action == NodeAction::Remove).then(|| ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(Modifiers::NONE, egui::Key::Delete)));
        if menu_row(ui, super::toolbar::node::label(action), keys.as_deref(), on, false, false).clicked() && on {
            picked = Some(Picked::Node(action));
        }
    }
    picked
}

/// A 32 px row in the `text_row` style: label left, shortcut right in `TEXT_MUTED`, a chevron for a
/// submenu. A disabled row is greyed and never clicks.
fn menu_row(ui: &mut Ui, label: &str, keys: Option<&str>, enabled: bool, submenu: bool, open: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), sense);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && (open || resp.hovered()) {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), ink);
    let right = pos2(rect.right() - 8.0, rect.center().y);
    if let Some(keys) = keys {
        text(ui.painter(), right, Align2::RIGHT_CENTER, keys, fonts::regular(13.0), if enabled { TEXT_MUTED } else { DISABLED_TEXT });
    }
    if submenu {
        let icon = Rect::from_center_size(right - vec2(6.0, 0.0), Vec2::splat(14.0));
        paint_icon_rotated(ui, Icon::Chevron, icon, ink, -std::f32::consts::FRAC_PI_2);
    }
    resp
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range().expand(8.0), rect.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
}

/// Every label `row` can show, for the spec surface test: an effect row's label follows the effect it acts on.
#[cfg(test)]
pub(super) fn labels(row: &Row) -> Vec<&'static str> {
    let of = |e: &Edit| super::commands::info(&Command::Edit(e.clone())).label;
    match row {
        Row::Item(e) => vec![of(e)],
        Row::Either(a, b) => vec![of(a), of(b)],
        Row::App(a) => vec![super::commands::info(&Command::App(a.clone())).label],
        Row::Clipboard(a) => vec![super::commands::info(&Command::Clipboard(*a)).label],
        Row::Sub(label, _) => vec![label],
        Row::Effect(EffectAct::Edit) => vec!["Edit effect"],
        Row::Effect(EffectAct::Toggle) => vec!["Disable effect", "Enable effect"],
        Row::Effect(EffectAct::Remove) => vec!["Remove effect"],
        Row::Separator => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn squares(n: usize) -> Design {
        let mut d = Design::default();
        for _ in 0..n {
            d.add_shape(ShapeKind::Square);
        }
        d
    }

    #[test]
    fn either_rows_show_the_inverse_once_it_applies() {
        let mut d = squares(2);
        let row = Row::Either(Edit::Group, Edit::Ungroup);
        let label = |d: &Design| shown(&row, &Cx::new(d, &[1, 2], AlignTo::default())).unwrap().label;
        assert_eq!(label(&d), "Group");
        d.group(&[1, 2]);
        assert_eq!(label(&d), "Ungroup");
    }

    #[test]
    fn align_rows_follow_the_rule_the_popover_shows() {
        let mut d = squares(2);
        let lone = d.add_shape(ShapeKind::Circle);
        let c = d.contain(&[lone]).unwrap();
        d.reparent(&[1], Some(c));
        let left = Row::Item(align(Align::Left));
        let on = |to| shown(&left, &Cx::new(&d, &[1], to)).unwrap().enabled();
        assert!(on(AlignTo::Container), "one child aligns to its container");
        assert!(!on(AlignTo::Selection), "one object alone does not align to itself");
    }

    fn press(pressed: bool, modifiers: Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos: Pos2::ZERO, button: egui::PointerButton::Primary, pressed, modifiers }
    }

    fn buttons(raw: &egui::RawInput) -> Vec<egui::PointerButton> {
        raw.events.iter().filter_map(|e| if let egui::Event::PointerButton { button, .. } = e { Some(*button) } else { None }).collect()
    }

    #[test]
    fn a_mac_ctrl_click_becomes_a_right_click_even_if_ctrl_lifts_first() {
        use egui::PointerButton::{Primary, Secondary};
        let ctx = Context::default();
        let mut down = egui::RawInput { events: vec![press(true, Modifiers::CTRL)], ..Default::default() };
        ctrl_click_is_secondary(&ctx, &mut down);
        let mut up = egui::RawInput { events: vec![press(false, Modifiers::NONE), press(true, Modifiers::NONE)], ..Default::default() };
        ctrl_click_is_secondary(&ctx, &mut up);
        assert_eq!(buttons(&down), vec![Secondary]);
        assert_eq!(buttons(&up), vec![Secondary, Primary], "the release pairs with its press; the next click is plain");
    }

    #[test]
    fn ctrl_as_command_stays_a_left_click() {
        let ctx = Context::default();
        let command_ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let mut raw = egui::RawInput { events: vec![press(true, command_ctrl), press(false, command_ctrl)], ..Default::default() };
        ctrl_click_is_secondary(&ctx, &mut raw);
        assert_eq!(buttons(&raw), vec![egui::PointerButton::Primary; 2]);
    }
}

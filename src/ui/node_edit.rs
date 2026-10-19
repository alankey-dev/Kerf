//! Editing a drawn path's nodes on the canvas. Double-click a path with Select, or press A, to start.
//! Click a node to select it, Shift-click to add or remove one, drag over empty canvas to select the
//! nodes inside, and Cmd+A to select them all. Drag selected anchors or a curve handle to reshape the
//! path, arrows to nudge. Click a segment to add a node there; Option-click a node or press Delete to
//! remove. Escape, Enter or a click away finishes.

use egui::{CursorIcon, Key, KeyboardShortcut, Modifiers, Painter, Pos2, Rect, Response, Stroke, StrokeKind, Ui, Vec2};

use crate::design::{Design, Pt, Shape};
use crate::path_edit::{self, NodeAction, NodeId, NodePath, Part, SegHit, Selection, Side, Target};

use super::canvas::{Grid, paint_anchor, paint_round_handle, snap_px};
use super::tokens::*;

const PICK_PX: f32 = 8.0;
const NUDGE_IN: f32 = 0.125;
const NUDGE_BIG_IN: f32 = 1.0;

/// Every key node editing answers to, for tooltips and the clash test.
pub const SHORTCUTS: [(&str, KeyboardShortcut); 6] = [
    ("Edit nodes", KeyboardShortcut::new(Modifiers::NONE, Key::A)),
    ("Delete node", KeyboardShortcut::new(Modifiers::NONE, Key::Delete)),
    ("Delete node", KeyboardShortcut::new(Modifiers::NONE, Key::Backspace)),
    ("Select all nodes", KeyboardShortcut::new(Modifiers::COMMAND, Key::A)),
    ("Done", KeyboardShortcut::new(Modifiers::NONE, Key::Escape)),
    ("Done", KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
];

pub struct NodeEdit {
    pub shape: u64,
    pub selected: Selection,
    drag: Option<Drag>,
    /// The segment point under the pointer, where a click would add a node.
    hover: Option<SegHit>,
    /// A Smooth result shown over the path while its slider moves; the design is untouched until Apply.
    pub preview: Option<Shape>,
}

/// A press-drag in progress. Each frame's edit applies to `original` afresh rather than compounding.
enum Drag {
    Anchors { original: Shape, press_in: Pt },
    Handle { original: Shape, id: NodeId, side: Side, from_in: Pt, press_in: Pt },
    Marquee { start_in: Pt, now_in: Pt, kept: Selection },
}

impl NodeEdit {
    pub fn begin(shape: u64) -> Self {
        Self { shape, selected: Selection::new(), drag: None, hover: None, preview: None }
    }
}

/// Starts node editing on `id`. A parametric shape (spec 49) is turned into a `Path` of its outline in
/// the same undo step, after a notice that its parameters are lost, since its nodes replace its fields.
pub fn enter(design: &mut Design, id: u64, nodes: &mut Option<NodeEdit>, notice: &mut Option<super::notice::Notice>) {
    if let Some(s) = design.shape_mut(id)
        && let crate::design::ShapeKind::Parametric(p) = &s.kind
    {
        // A parametric shape hands its corner radii over as live corners (spec 55), so a rounded
        // rectangle or filleted polygon keeps its rounding editable instead of baking arcs.
        let (p, size) = (*p, (s.width_in, s.height_in));
        let (contours, corners) = crate::parametric::to_corner_path(&p, size).unwrap_or_else(|| (crate::parametric::outline(&p, size), Vec::new()));
        s.kind = crate::design::ShapeKind::Path { contours, view_box: size, node_kinds: Vec::new(), corners };
        *notice = Some(super::notice::Notice::new("Editing nodes drops the shape's parameters.", true));
    }
    *nodes = Some(NodeEdit::begin(id));
}

/// Runs a toolbar or context menu action on the selected nodes as one design change.
pub fn apply(design: &mut Design, edit: &mut NodeEdit, action: NodeAction) {
    tracing::info!(target: "cmd", action = ?action, shape = edit.shape, "node");
    let Some(shape) = design.shape_mut(edit.shape) else { return };
    let mut path = NodePath::read(shape);
    if let Ok(next) = path.apply(&edit.selected, action) {
        *shape = path.write(shape);
        edit.selected = next;
    }
    edit.preview = None;
}

/// Starts node editing on the one selected path when A is pressed.
pub fn shortcut(ui: &Ui, design: &mut Design, selection: &[u64], edit: &mut Option<NodeEdit>, notice: &mut Option<super::notice::Notice>) {
    if edit.is_some() || ui.ctx().wants_keyboard_input() {
        return;
    }
    if let [id] = selection
        && design.shapes.iter().any(|s| s.id == *id && path_edit::editable(s))
        && ui.input_mut(|i| i.consume_shortcut(&SHORTCUTS[0].1))
    {
        enter(design, *id, edit, notice);
    }
}

fn pick(path: &NodePath, shape: &Shape, sel: &Selection, grid: &Grid, at: Pos2) -> Option<(Target, Pt)> {
    let hits = path.targets(shape, sel).into_iter().filter(|(_, p)| grid.to_screen(*p).distance(at) <= PICK_PX);
    // Anchors win over a handle lying on top of them.
    hits.min_by_key(|(t, p)| (t.part != Part::Anchor, (grid.to_screen(*p).distance(at) * 100.0) as i32))
}

fn write(design: &mut Design, id: u64, path: &NodePath, original: &Shape) {
    if let Some(s) = design.shape_mut(id) {
        *s = path.write(original);
    }
}

/// Handles this frame's canvas input while a path is being edited. Returns true if it used the input;
/// false hands it to the Select tool, which happens once editing ends.
pub fn input(ui: &Ui, response: &Response, design: &mut Design, selection: &[u64], edit: &mut Option<NodeEdit>, grid: &Grid) -> bool {
    let Some(e) = edit.as_mut() else { return false };
    let Some(shape) = design.shapes.iter().find(|s| s.id == e.shape).filter(|s| selection == [s.id] && path_edit::editable(s)).cloned() else {
        *edit = None;
        return false;
    };
    if path_edit::refusal(&shape).is_some() {
        return false;
    }
    let mut path = NodePath::read(&shape);
    e.selected.retain(|id| path.node(*id).is_some());
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
    let angle_step = design.snap.angle_step_deg;

    e.hover = None;
    if e.drag.is_none()
        && let Some(p) = response.hover_pos()
        && pick(&path, &shape, &e.selected, grid, p).is_none()
    {
        e.hover = path.nearest(shape.to_local(grid.to_inches(p)), PICK_PX / grid.scale);
        if e.hover.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Copy);
        }
    }

    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if let Some((t, _)) = pick(&path, &shape, &e.selected, grid, p) {
            if !e.selected.contains(&t.id) {
                e.selected = Selection::from([t.id]);
            }
            super::context_menu::open_nodes(ui.ctx(), p);
        }
        return true;
    }

    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
    {
        let press_in = grid.to_inches(press);
        e.drag = Some(match pick(&path, &shape, &e.selected, grid, press) {
            Some((Target { id, part: Part::Anchor }, _)) => {
                if !e.selected.contains(&id) {
                    if !shift {
                        e.selected.clear();
                    }
                    e.selected.insert(id);
                }
                Drag::Anchors { original: shape.clone(), press_in }
            }
            Some((Target { id, part: Part::Handle(side) }, from_in)) => Drag::Handle { original: shape.clone(), id, side, from_in, press_in },
            None => Drag::Marquee { start_in: press_in, now_in: press_in, kept: if shift { e.selected.clone() } else { Selection::new() } },
        });
        return true;
    }
    if let Some(drag) = &mut e.drag {
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let now = grid.to_inches(p);
            match drag {
                Drag::Anchors { original, press_in } => {
                    let mut moved = NodePath::read(original);
                    // Shift locks a node move to its own axis or 45 degrees from where the drag began.
                    let raw = (now.0 - press_in.0, now.1 - press_in.1);
                    let delta = if shift { crate::snap::constrain_angle(raw, angle_step) } else { raw };
                    moved.move_anchors(&e.selected, path_edit::local_delta(original, delta));
                    write(design, e.shape, &moved, original);
                }
                Drag::Handle { original, id, side, from_in, press_in } => {
                    let mut moved = NodePath::read(original);
                    let mut target = (from_in.0 + now.0 - press_in.0, from_in.1 + now.1 - press_in.1);
                    // Shift snaps the handle's angle from its anchor to 45 degree steps.
                    if shift && let Some(node) = moved.node(*id) {
                        let anchor = original.to_world(node.pt);
                        let v = crate::snap::constrain_angle((target.0 - anchor.0, target.1 - anchor.1), angle_step);
                        target = (anchor.0 + v.0, anchor.1 + v.1);
                    }
                    moved.move_handle(*id, *side, original.to_local(target));
                    write(design, e.shape, &moved, original);
                }
                Drag::Marquee { start_in, now_in, kept } => {
                    *now_in = now;
                    let r = Rect::from_two_pos(Pos2::from(*start_in), Pos2::from(now));
                    let inside = path.targets(&shape, &Selection::new()).into_iter().filter(|(_, p)| r.contains(Pos2::from(*p))).map(|(t, _)| t.id);
                    e.selected = kept.iter().copied().chain(inside).collect();
                }
            }
        }
        if response.drag_stopped() {
            e.drag = None;
        }
        return true;
    }

    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        match pick(&path, &shape, &e.selected, grid, p) {
            Some((Target { id, part: Part::Anchor }, _)) if alt => {
                if path.apply(&Selection::from([id]), NodeAction::Remove).is_ok() {
                    write(design, e.shape, &path, &shape);
                    e.selected.clear();
                }
            }
            Some((Target { id, part: Part::Anchor }, _)) if shift => {
                if !e.selected.remove(&id) {
                    e.selected.insert(id);
                }
            }
            Some((Target { id, part: Part::Anchor }, _)) => e.selected = Selection::from([id]),
            Some(_) => {}
            None => match e.hover {
                Some(hit) => {
                    let id = path.insert(hit);
                    write(design, e.shape, &path, &shape);
                    e.selected = Selection::from([id]);
                }
                None => {
                    *edit = None;
                    return false;
                }
            },
        }
        return true;
    }

    if ui.ctx().wants_keyboard_input() {
        return true;
    }
    let (done, del, all, arrow, big) = ui.input_mut(|i| {
        let done = i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter);
        let del = i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace);
        let all = i.consume_shortcut(&SHORTCUTS[3].1);
        let arrow = [(Key::ArrowLeft, (-1.0, 0.0)), (Key::ArrowRight, (1.0, 0.0)), (Key::ArrowUp, (0.0, -1.0)), (Key::ArrowDown, (0.0, 1.0))]
            .into_iter()
            .find(|(k, _)| i.key_pressed(*k))
            .map(|(_, d)| d);
        (done, del, all, arrow, i.modifiers.shift)
    });
    if done {
        *edit = None;
    } else if del {
        if path.apply(&e.selected, NodeAction::Remove).is_ok() {
            write(design, e.shape, &path, &shape);
            e.selected.clear();
        }
    } else if all {
        e.selected = path.all();
    } else if let Some((dx, dy)) = arrow {
        let step = if big { NUDGE_BIG_IN } else { NUDGE_IN };
        path.move_anchors(&e.selected, path_edit::local_delta(&shape, (dx * step, dy * step)));
        write(design, e.shape, &path, &shape);
    }
    true
}

/// The path outline in the accent colour, curve handles of the selected nodes and their neighbours,
/// anchors (selected ones filled), the add-node ghost and the marquee.
pub fn paint(painter: &Painter, grid: &Grid, design: &Design, edit: &NodeEdit) {
    let Some(shape) = design.shapes.iter().find(|s| s.id == edit.shape) else { return };
    let outline = |s: &Shape, stroke: Stroke| {
        for (pts, closed) in design.geometry(s).lines() {
            let mut pts: Vec<Pos2> = pts.iter().map(|&p| grid.to_screen(p)).collect();
            if *closed && let Some(&first) = pts.first() {
                pts.push(first);
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
    };
    if let Some(preview) = &edit.preview {
        outline(preview, Stroke::new(2.0_f32, ACCENT_PRIMARY));
        return;
    }
    outline(shape, Stroke::new(1.0_f32, ACCENT_PRIMARY));
    if path_edit::refusal(shape).is_some() {
        return;
    }
    let path = NodePath::read(shape);
    let targets = path.targets(shape, &edit.selected);
    let anchor_at = |id: NodeId| targets.iter().find(|(t, _)| t.id == id && t.part == Part::Anchor).map(|(_, p)| *p);
    for (t, p) in targets.iter().filter(|(t, _)| t.part != Part::Anchor) {
        if let Some(a) = anchor_at(t.id) {
            painter.line_segment([grid.to_screen(a), grid.to_screen(*p)], Stroke::new(1.0_f32, ACCENT_PRIMARY));
            paint_round_handle(painter, grid.to_screen(*p));
        }
    }
    for (t, p) in targets.iter().filter(|(t, _)| t.part == Part::Anchor) {
        let at = grid.to_screen(*p);
        if edit.selected.contains(&t.id) {
            painter.rect_filled(Rect::from_center_size(snap_px(at), Vec2::splat(7.5)), 0.0, ACCENT_PRIMARY);
        } else {
            paint_anchor(painter, at);
        }
        // A dot under a node that carries a live corner (spec 55), so node editing shows it has one.
        if path.node(t.id).is_some_and(|n| n.corner.is_set()) {
            painter.circle_filled(snap_px(at) + Vec2::new(0.0, 9.0), 1.8, ACCENT_PRIMARY);
        }
    }
    if let Some(hit) = edit.hover {
        let at = snap_px(grid.to_screen(shape.to_world(hit.pt)));
        painter.circle(at, 4.0, BG_PANEL, Stroke::new(1.5_f32, ACCENT_PRIMARY));
        let plus = at + Vec2::new(10.0, -10.0);
        let stroke = Stroke::new(1.5_f32, ACCENT_PRIMARY);
        painter.line_segment([plus - Vec2::X * 4.0, plus + Vec2::X * 4.0], stroke);
        painter.line_segment([plus - Vec2::Y * 4.0, plus + Vec2::Y * 4.0], stroke);
    }
    if let Some(Drag::Marquee { start_in, now_in, .. }) = &edit.drag {
        let r = Rect::from_two_pos(grid.to_screen(*start_in), grid.to_screen(*now_in));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, ACCENT_PRIMARY), StrokeKind::Middle);
    }
}

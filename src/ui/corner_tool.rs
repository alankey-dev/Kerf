//! The Corner tool (spec 55): round, chamfer or scoop a path's corners by dragging a node's widget or
//! typing a radius. It reuses node editing's idea of a selection of nodes, but every change it makes goes
//! through `Edit::Corners`, so the whole thing is one command and one undo step per gesture.
//!
//! Selecting a single editable shape begins corner editing on it. Its Straight and Cusp nodes show a
//! small circle widget. Dragging one inward rounds the corner; Option-click cycles Round, Chamfer and
//! Inverted; dragging with several nodes selected rounds them all to the same radius.

use egui::{Pos2, Response, Ui};

use crate::corners::{Corner, CornerOp, CornerStyle};
use crate::design::{Design, Pt, Shape, ShapeKind};
use crate::edit::{Edit, Subject};
use crate::path_edit::{NodeId, NodePath, Selection};

use super::canvas::{Grid, View};

const PICK_PX: f32 = 9.0;
/// The radius a fresh corner edit offers, so Round all and a first drag have something to apply.
pub const DEFAULT_RADIUS_IN: f32 = 0.125;

/// The path the Corner tool is editing, and the nodes it has picked. The working radius and style are
/// what the toolbar shows and what Round all and a drag apply.
pub struct CornerEdit {
    pub shape: u64,
    pub selected: Selection,
    pub radius_in: f32,
    pub style: CornerStyle,
    drag: Option<NodeId>,
    /// A node marquee in progress (start, now) in world inches, as node editing has one.
    marquee: Option<(Pt, Pt)>,
}

impl CornerEdit {
    pub fn begin(shape: u64) -> Self {
        Self { shape, selected: Selection::new(), radius_in: DEFAULT_RADIUS_IN, style: CornerStyle::Round, drag: None, marquee: None }
    }
}

/// The sharp nodes the widgets sit on. A parametric shape is read through a throwaway conversion so its
/// corners show as editable nodes without touching the real design until an edit runs.
pub fn display_path(shape: &Shape) -> NodePath {
    if matches!(shape.kind, ShapeKind::Path { .. }) {
        NodePath::read(shape)
    } else {
        let mut copy = shape.clone();
        crate::path_edit::to_path(&mut copy);
        NodePath::read(&copy)
    }
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn unit(a: Pt) -> Option<Pt> {
    let l = a.0.hypot(a.1);
    (l > 1e-6).then(|| (a.0 / l, a.1 / l))
}

/// The inward bisector of node `id` in the shape's local frame, pointing into the corner's interior.
fn inward_bisector(path: &NodePath, id: NodeId) -> Option<Pt> {
    let nc = path.contours.get(id.contour)?;
    let (i, n) = (id.node, nc.nodes.len());
    let (prev, next) = if nc.closed {
        ((i + n - 1) % n, (i + 1) % n)
    } else if i == 0 || i + 1 == n {
        return None;
    } else {
        (i - 1, i + 1)
    };
    let p = nc.nodes[i].pt;
    let din = unit(sub(nc.nodes[i].inn.unwrap_or(nc.nodes[prev].pt), p))?;
    let dout = unit(sub(nc.nodes[i].out.unwrap_or(nc.nodes[next].pt), p))?;
    unit((din.0 + dout.0, din.1 + dout.1))
}

/// The radius a drag to `pointer` (world inches) asks for: how far the pointer has moved into the corner
/// along its inward bisector, clamped to the effective radius the corner can actually take. Dragging
/// inward grows it, outward shrinks it back to zero.
fn radius_from_drag(path: &NodePath, shape: &Shape, id: NodeId, style: CornerStyle, pointer: Pt) -> f32 {
    let Some(node) = path.node(id) else { return 0.0 };
    let Some(bis) = inward_bisector(path, id) else { return 0.0 };
    let v = sub(shape.to_local(pointer), node.pt);
    let asked = (v.0 * bis.0 + v.1 * bis.1).max(0.0);
    effective(path, id, asked, style)
}

/// The clamped radius a corner of `radius`/`style` at node `id` actually draws (spec 55's reported
/// clamp), by running the pure evaluation for that one node.
fn effective(path: &NodePath, id: NodeId, radius: f32, style: CornerStyle) -> f32 {
    let Some(nc) = path.contours.get(id.contour) else { return radius };
    let mut cs = vec![Corner::default(); nc.nodes.len()];
    cs[id.node] = Corner { radius_in: radius, style };
    crate::corners::effective_radii(&nc.to_contour(), &cs).get(id.node).copied().unwrap_or(radius)
}

/// The eligible node under `at` (screen), nearest wins.
fn pick(path: &NodePath, shape: &Shape, grid: &Grid, at: Pos2) -> Option<NodeId> {
    path.all()
        .into_iter()
        .filter(|id| path.corner_eligible(*id))
        .filter_map(|id| path.node(id).map(|n| (id, grid.to_screen(shape.to_world(n.pt)).distance(at))))
        .filter(|(_, d)| *d <= PICK_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The nodes a widget drag or a toolbar edit touches: the whole eligible selection when the grabbed node
/// is part of it, otherwise just that node.
fn targets(edit: &CornerEdit, path: &NodePath, grabbed: Option<NodeId>) -> Vec<NodeId> {
    match grabbed {
        Some(id) if edit.selected.contains(&id) && edit.selected.len() > 1 => path.eligible(&edit.selected),
        Some(id) => vec![id],
        None => path.eligible(&edit.selected),
    }
}

fn run(design: &mut Design, selection: &mut Vec<u64>, shape: u64, op: CornerOp) {
    _ = crate::edit::run(design, selection, &Edit::Corners(op), Subject::Ids(&[shape]));
}

/// Handles this frame's canvas input for the Corner tool. Returns true when it used the input.
pub fn input(ui: &Ui, response: &Response, design: &mut Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid) -> bool {
    let single = match selection[..] {
        [id] if design.shapes.iter().any(|s| s.id == id && crate::path_edit::editable(s)) => Some(id),
        _ => None,
    };
    if view.corners.as_ref().map(|e| e.shape) != single {
        view.corners = single.map(CornerEdit::begin);
    }
    // No editable single selection: a click selects a shape, or clears the selection.
    let Some(shape) = single.and_then(|id| design.shapes.iter().find(|s| s.id == id)).cloned() else {
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let w = grid.to_inches(p);
            *selection = design.pick_at(w.0, w.1).map_or_else(Vec::new, |id| vec![id]);
            return true;
        }
        return false;
    };

    let path = display_path(&shape);
    let edit = view.corners.as_mut().expect("just set for the single selection");
    edit.selected.retain(|id| path.node(*id).is_some());
    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));

    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
    {
        if let Some(id) = pick(&path, &shape, grid, press) {
            if !edit.selected.contains(&id) {
                if !shift {
                    edit.selected.clear();
                }
                edit.selected.insert(id);
            }
            edit.drag = Some(id);
        } else {
            let at = grid.to_inches(press);
            edit.marquee = Some((at, at));
        }
        return true;
    }

    if let Some((start, _)) = edit.marquee {
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let now = grid.to_inches(p);
            edit.marquee = Some((start, now));
            let rect = egui::Rect::from_two_pos(start.into(), now.into());
            edit.selected = path
                .all()
                .into_iter()
                .filter(|id| path.corner_eligible(*id))
                .filter(|id| path.node(*id).is_some_and(|n| rect.contains(shape.to_world(n.pt).into())))
                .collect();
        }
        if response.drag_stopped() {
            edit.marquee = None;
        }
        return true;
    }

    if let Some(node) = edit.drag {
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let radius = radius_from_drag(&path, &shape, node, edit.style, grid.to_inches(p));
            edit.radius_in = radius;
            let (style, nodes, shape_id) = (edit.style, targets(edit, &path, Some(node)), edit.shape);
            run(design, selection, shape_id, CornerOp::Set { nodes, radius: Some(radius), style: Some(style) });
        }
        if response.drag_stopped()
            && let Some(e) = view.corners.as_mut()
        {
            e.drag = None;
        }
        return true;
    }

    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        match pick(&path, &shape, grid, p) {
            Some(id) if alt => {
                let (nodes, shape_id) = (targets(edit, &path, Some(id)), edit.shape);
                run(design, selection, shape_id, CornerOp::Cycle { nodes });
            }
            Some(id) if shift => {
                if !edit.selected.remove(&id) {
                    edit.selected.insert(id);
                }
            }
            Some(id) => {
                edit.selected = Selection::from([id]);
                if let Some(n) = path.node(id).filter(|n| n.corner.is_set()) {
                    (edit.radius_in, edit.style) = (n.corner.radius_in, n.corner.style);
                }
            }
            None if edit.selected.is_empty() => {
                view.corners = None;
                selection.clear();
            }
            None => edit.selected.clear(),
        }
        return true;
    }
    false
}

/// Paints the Corner tool overlay: the path outline, a circle on every eligible node (filled when
/// selected), and a dot under a node that already carries a corner.
pub fn paint(painter: &egui::Painter, grid: &Grid, design: &Design, edit: &CornerEdit) {
    use super::canvas::snap_px;
    use super::tokens::*;
    let Some(shape) = design.shapes.iter().find(|s| s.id == edit.shape) else { return };
    for (pts, closed) in design.geometry(shape).lines() {
        let mut line: Vec<Pos2> = pts.iter().map(|&p| grid.to_screen(p)).collect();
        if *closed && let Some(&first) = line.first() {
            line.push(first);
        }
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.0_f32, ACCENT_PRIMARY)));
    }
    let path = display_path(shape);
    for id in path.all().into_iter().filter(|id| path.corner_eligible(*id)) {
        let Some(node) = path.node(id) else { continue };
        let at = snap_px(grid.to_screen(shape.to_world(node.pt)));
        let selected = edit.selected.contains(&id);
        if selected {
            painter.circle_filled(at, 4.5, ACCENT_PRIMARY);
        } else {
            painter.circle(at, 4.0, BG_PANEL, egui::Stroke::new(1.5_f32, ACCENT_PRIMARY));
        }
        if node.corner.is_set() {
            painter.circle_filled(at + egui::Vec2::new(0.0, 9.0), 1.8, ACCENT_PRIMARY);
        }
    }
    if let Some((start, now)) = edit.marquee {
        let r = egui::Rect::from_two_pos(grid.to_screen(start), grid.to_screen(now));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0_f32, ACCENT_PRIMARY), egui::StrokeKind::Middle);
    }
}

//! The Path panel (spec 51): the Curve panel becomes this when the selected text follows a path. It
//! sets which side of the path the text sits on, where it sits along the path, how far its start is
//! slid, and offers Remove from path. The canvas handles drive the same `Edit::TextPath` edits.
//!
//! The canvas half lives here too: the faint guide line, the start and centre handles, the overflow
//! plus, and node editing of the text's own path copy (press A), which reflows the letters live. All
//! of it works in the path's own frame, mapped to the world through [`Frame`], which undoes the shift
//! and box scaling [`crate::text_path::place`] applied so the chrome lands exactly under the letters.

use egui::{Color32, Key, KeyboardShortcut, Modifiers, Painter, Pos2, Response, Stroke, Ui, Vec2};

use crate::design::{Design, Pt, Shape, ShapeKind};
use crate::edit::{Edit, TextPathEdit};
use crate::path_edit::{NodeAction, NodeContour, NodeId, NodePath, Selection};
use crate::text_path::{Along, PathFrame, Side, TextPath};
use crate::units::{FIELD_W, Units, length_field};

use super::canvas::{Grid, View, paint_anchor};
use super::tokens::{ACCENT_PRIMARY, BG_PANEL, ERROR_EDGE};

/// How near the pointer must come to a handle or node to grab it, in screen pixels.
const PICK_PX: f32 = 8.0;
/// The guide line and node editing answer to A, the same key node editing a path shape uses.
const EDIT_NODES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::A);

/// The offset field slides the start this far either way along the path, in inches.
const OFFSET_IN: std::ops::RangeInclusive<f32> = -100.0..=100.0;

/// The Path panel on `kind`, a text that follows a path. Returns the applied edit (Side, Align or
/// Remove) and any live edit (Offset dragged in the field), like the Curve panel it replaces.
pub(in crate::ui) fn path_panel(ui: &mut Ui, units: Units, kind: &ShapeKind) -> (Option<Edit>, Option<Edit>) {
    let ShapeKind::Text { path: Some(path), .. } = kind else { return (None, None) };
    let mut applied = None;

    ui.label("Side");
    ui.horizontal(|ui| {
        for (side, label) in [(Side::Above, "Above"), (Side::Below, "Below")] {
            if ui.selectable_label(path.side == side, label).clicked() && path.side != side {
                applied = Some(Edit::TextPath(TextPathEdit::Side(side)));
            }
        }
    });

    ui.label("Align along");
    ui.horizontal(|ui| {
        for (along, label) in [(Along::Start, "Start"), (Along::Centre, "Centre"), (Along::End, "End")] {
            if ui.selectable_label(path.align == along, label).clicked() && path.align != along {
                applied = Some(Edit::TextPath(TextPathEdit::Along(along)));
            }
        }
    });

    ui.label("Offset");
    let mut offset = path.offset_in;
    let live =
        length_field(ui, "Offset", Some(offset), units, FIELD_W).is_some_and(|t| t.set(&mut offset, OFFSET_IN)).then_some(Edit::TextPath(TextPathEdit::Offset(offset)));

    if ui.button("Remove from path").clicked() {
        applied = Some(Edit::TextPath(TextPathEdit::Remove));
    }
    (applied, live)
}

/// The map between a text's path frame and the world. A path point is un-shifted, scaled from the
/// laid-out box into the text's inch box, then placed like any glyph, so the guide line, handles and
/// nodes sit exactly under the letters that follow the path.
#[derive(Clone, Copy)]
struct Frame {
    shift: Pt,
    size: Pt,
    box_in: Pt,
}

impl Frame {
    fn to_world(self, shape: &Shape, p: Pt) -> Pt {
        let q = ((p.0 - self.shift.0) * self.box_in.0 / self.size.0, (p.1 - self.shift.1) * self.box_in.1 / self.size.1);
        shape.to_world(q)
    }

    fn to_path(self, shape: &Shape, world: Pt) -> Pt {
        let q = shape.to_local(world);
        (q.0 * self.size.0 / self.box_in.0 + self.shift.0, q.1 * self.size.1 / self.box_in.1 + self.shift.1)
    }
}

/// The path, its frame, and whether the run overflows the path, for a text that follows one as a
/// single line. `None` for straight, wrapped or multi-line text, which shows no path chrome.
fn placed(shape: &Shape) -> Option<(TextPath, PathFrame, Frame, bool)> {
    let ShapeKind::Text { path: Some(path), .. } = &shape.kind else { return None };
    let layout = shape.kind.text_layout()?;
    let pf = layout.path?;
    let frame = Frame { shift: pf.shift, size: layout.size, box_in: (shape.width_in, shape.height_in) };
    Some((path.clone(), pf, frame, layout.overflow))
}

/// The start and centre handles and the overflow plus of the selected path text, in screen points.
pub(in crate::ui) struct Handles {
    pub shape: u64,
    pub start: Pos2,
    pub centre: Pos2,
    pub overflow: Option<Pos2>,
}

/// Which handle a drag holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum GrabKind {
    Start,
    Centre,
}

/// A handle drag, reading from the path as it was grabbed. Projecting the pointer against the
/// drag-start frame keeps the slide a pure function of the pointer, so a mid-drag refit cannot feed
/// back into the next offset.
pub(in crate::ui) struct Grab {
    kind: GrabKind,
    shape0: Shape,
    pf0: PathFrame,
    frame0: Frame,
    path0: TextPath,
}

/// The handle screen positions for the one selected path text, when the Select tool can show them.
pub(in crate::ui) fn compute_handles(design: &Design, selection: &[u64], view: &View, grid: &Grid) -> Option<Handles> {
    if view.path_nodes.is_some() {
        return None;
    }
    let [id] = *selection else { return None };
    let shape = design.shapes.iter().find(|s| s.id == id)?;
    let (path, pf, frame, overflow) = placed(shape)?;
    let trace = path.trace();
    let at = |s: f32| grid.to_screen(frame.to_world(shape, trace.point_at(s).0));
    Some(Handles { shape: id, start: at(pf.base), centre: at(pf.base + pf.run_width / 2.0), overflow: overflow.then(|| at(trace.total)) })
}

/// Grabs and drags the start or centre handle. The start handle slides the run to the pointer; the
/// centre handle flips the text to whichever side of the path the pointer crosses to. Each drag is one
/// undo step, coalesced like any canvas gesture while the pointer stays down.
pub(in crate::ui) fn handle_input(ui: &Ui, response: &Response, design: &mut Design, view: &mut View, grid: &Grid) -> bool {
    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && let Some(handles) = &view.path_handles
    {
        let kind = if press.distance(handles.start) <= PICK_PX {
            Some(GrabKind::Start)
        } else if press.distance(handles.centre) <= PICK_PX {
            Some(GrabKind::Centre)
        } else {
            None
        };
        let shape = design.shapes.iter().find(|s| s.id == handles.shape).cloned();
        view.path_grab = kind.zip(shape).and_then(|(kind, shape)| {
            let (path0, pf0, frame0, _) = placed(&shape)?;
            Some(Grab { kind, shape0: shape, pf0, frame0, path0 })
        });
        if view.path_grab.is_some() {
            return true;
        }
    }
    let Some(grab) = &view.path_grab else { return false };
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let id = grab.shape0.id;
        let (s, side) = grab.path0.trace().project(grab.frame0.to_path(&grab.shape0, grid.to_inches(p)));
        match grab.kind {
            GrabKind::Start => _ = design.apply_text_path(TextPathEdit::Offset(grab.path0.offset_in + s - grab.pf0.base), &[id]),
            GrabKind::Centre => _ = design.apply_text_path(TextPathEdit::Side(if side < 0.0 { Side::Above } else { Side::Below }), &[id]),
        }
    }
    if response.drag_stopped() {
        view.path_grab = None;
    }
    true
}

/// Draws the faint guide line, the two handles, the overflow plus, and the offset read-out while the
/// start handle is dragged.
pub(in crate::ui) fn paint(ui: &mut Ui, painter: &Painter, grid: &Grid, design: &Design, view: &View, units: Units) {
    let Some(handles) = &view.path_handles else { return };
    let Some(shape) = design.shapes.iter().find(|s| s.id == handles.shape) else { return };
    let Some((path, _, frame, _)) = placed(shape) else { return };
    guide_line(painter, grid, shape, &path, &frame);
    painter.circle_filled(handles.start, 4.0, ACCENT_PRIMARY);
    painter.circle(handles.centre, 4.0, BG_PANEL, Stroke::new(1.5_f32, ACCENT_PRIMARY));
    if let Some(at) = handles.overflow {
        overflow_plus(painter, at);
    }
    if matches!(&view.path_grab, Some(g) if g.kind == GrabKind::Start)
        && let Some(p) = ui.input(|i| i.pointer.latest_pos())
    {
        super::measure_tool::readout(ui, painter, units.len(path.offset_in.abs()), p);
    }
}

fn guide_line(painter: &Painter, grid: &Grid, shape: &Shape, path: &TextPath, frame: &Frame) {
    let faint = ACCENT_PRIMARY.gamma_multiply(0.4);
    for contour in &path.contours {
        let mut pts: Vec<Pos2> = contour.flatten().iter().map(|&p| grid.to_screen(frame.to_world(shape, p))).collect();
        if contour.closed
            && let Some(&first) = pts.first()
        {
            pts.push(first);
        }
        painter.add(egui::Shape::line(pts, Stroke::new(1.0_f32, faint)));
    }
}

fn overflow_plus(painter: &Painter, at: Pos2) {
    painter.rect_filled(egui::Rect::from_center_size(at, Vec2::splat(11.0)), 1.0, ERROR_EDGE);
    let stroke = Stroke::new(1.5_f32, Color32::WHITE);
    painter.line_segment([at - Vec2::X * 3.0, at + Vec2::X * 3.0], stroke);
    painter.line_segment([at - Vec2::Y * 3.0, at + Vec2::Y * 3.0], stroke);
}

/// Node editing of a text's own path copy (spec 51). The nodes are the path's, shown on the guide
/// line; dragging one reshapes the path and the letters reflow. One drag is one undo step.
pub(in crate::ui) struct PathNodeEdit {
    pub shape: u64,
    selected: Selection,
    drag: Option<NodeDrag>,
    /// This frame's node screen positions, in `every_node` order, for painting and tests.
    pub dots: Vec<Pos2>,
}

/// A node drag reads from the path as it was grabbed, so a mid-drag refit cannot make the node chase
/// the pointer.
struct NodeDrag {
    original: NodePath,
    shape0: Shape,
    frame0: Frame,
    press_in: Pt,
}

/// Enters path-node editing when A is pressed with one path text selected.
pub(in crate::ui) fn node_shortcut(ui: &Ui, design: &Design, selection: &[u64], edit: &mut Option<PathNodeEdit>) {
    if edit.is_some() || ui.ctx().wants_keyboard_input() {
        return;
    }
    if let [id] = *selection
        && design.shapes.iter().any(|s| s.id == id && placed(s).is_some())
        && ui.input_mut(|i| i.consume_shortcut(&EDIT_NODES))
    {
        *edit = Some(PathNodeEdit { shape: id, selected: Selection::new(), drag: None, dots: Vec::new() });
    }
}

fn node_path(path: &TextPath) -> NodePath {
    NodePath { contours: path.contours.iter().map(|c| NodeContour::from_contour(c, None)).collect() }
}

fn every_node(np: &NodePath) -> impl Iterator<Item = NodeId> + '_ {
    np.contours.iter().enumerate().flat_map(|(c, nc)| (0..nc.nodes.len()).map(move |n| NodeId { contour: c, node: n }))
}

fn write_path_contours(design: &mut Design, id: u64, np: &NodePath) {
    let before = {
        let Some(s) = design.shape_mut(id) else { return };
        let before = s.kind.clone();
        let ShapeKind::Text { path: Some(p), .. } = &mut s.kind else { return };
        p.contours = np.contours.iter().map(NodeContour::to_contour).collect();
        before
    };
    design.refit_text(id, &before);
}

/// Handles this frame's input while a text's path nodes are being edited. Returns true when it used the
/// input; false ends editing and hands the frame to the Select tool.
pub(in crate::ui) fn node_input(ui: &Ui, response: &Response, design: &mut Design, selection: &[u64], view: &mut View, grid: &Grid) -> bool {
    if view.path_nodes.is_none() {
        return false;
    }
    let Some(shape) = design.shapes.iter().find(|s| selection == [s.id] && s.id == view.path_nodes.as_ref().unwrap().shape).cloned() else {
        view.path_nodes = None;
        return false;
    };
    let Some((path, _, frame, _)) = placed(&shape) else {
        view.path_nodes = None;
        return false;
    };
    let e = view.path_nodes.as_mut().unwrap();
    let np = node_path(&path);
    e.selected.retain(|id| np.node(*id).is_some());
    let screen = |id: NodeId| grid.to_screen(frame.to_world(&shape, np.node(id).unwrap().pt));
    e.dots = every_node(&np).map(&screen).collect();
    let pick = |at: Pos2| every_node(&np).filter(|&id| screen(id).distance(at) <= PICK_PX).min_by(|&a, &b| screen(a).distance(at).total_cmp(&screen(b).distance(at)));

    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
    {
        if let Some(id) = pick(press) {
            if !e.selected.contains(&id) {
                e.selected = Selection::from([id]);
            }
            e.drag = Some(NodeDrag { original: np.clone(), shape0: shape.clone(), frame0: frame, press_in: grid.to_inches(press) });
            return true;
        }
        e.drag = None;
    }
    if let Some(drag) = e.drag.as_ref()
        && response.dragged()
        && let Some(p) = response.interact_pointer_pos()
    {
        let (now, press) = (drag.frame0.to_path(&drag.shape0, grid.to_inches(p)), drag.frame0.to_path(&drag.shape0, drag.press_in));
        let mut moved = drag.original.clone();
        moved.move_anchors(&e.selected, (now.0 - press.0, now.1 - press.1));
        write_path_contours(design, shape.id, &moved);
        return true;
    }
    if response.drag_stopped() {
        e.drag = None;
        return true;
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        match pick(p) {
            Some(id) => e.selected = Selection::from([id]),
            None => {
                view.path_nodes = None;
                return false;
            }
        }
        return true;
    }
    if ui.ctx().wants_keyboard_input() {
        return true;
    }
    let (done, del) = ui.input_mut(|i| (i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter), i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)));
    if done {
        view.path_nodes = None;
    } else if del {
        let mut after = np;
        if after.apply(&e.selected, NodeAction::Remove).is_ok() {
            write_path_contours(design, shape.id, &after);
            e.selected.clear();
        }
    }
    true
}

/// The centre handle's world point and the laid-out baseline of the run's middle glyph, which must
/// coincide: the guide line and handles sit exactly on the letters that follow the path.
#[cfg(test)]
pub(in crate::ui) fn debug_centre_on_baseline(design: &Design, id: u64) -> Option<(Pt, Pt)> {
    let shape = design.shapes.iter().find(|s| s.id == id)?;
    let (path, pf, frame, _) = placed(shape)?;
    let centre = frame.to_world(shape, path.trace().point_at(pf.base + pf.run_width / 2.0).0);
    let layout = shape.kind.text_layout()?;
    let pen = layout
        .glyphs
        .iter()
        .map(|g| shape.to_world((g.pen.0 * shape.width_in / layout.size.0, g.pen.1 * shape.height_in / layout.size.1)))
        .min_by(|a, b| (a.0 - centre.0).hypot(a.1 - centre.1).total_cmp(&(b.0 - centre.0).hypot(b.1 - centre.1)))?;
    Some((centre, pen))
}

/// The topmost non-text shape whose evaluated outline passes within `within_in` inches of `at_in`, for
/// the Text tool's path-text cursor and click. A text already on a path is never a target.
pub(in crate::ui) fn edge_at(design: &Design, at_in: Pt, within_in: f32) -> Option<u64> {
    design.shapes.iter().rev().find(|s| !s.hidden && !matches!(s.kind, ShapeKind::Text { .. }) && near_outline(design, s, at_in, within_in)).map(|s| s.id)
}

fn near_outline(design: &Design, shape: &Shape, at: Pt, within: f32) -> bool {
    for (pts, closed) in design.geometry(shape).lines() {
        for i in 0..pts.len() {
            let a = pts[i];
            let b = match pts.get(i + 1) {
                Some(&b) => b,
                None if *closed => pts[0],
                None => break,
            };
            if seg_dist(at, a, b) <= within {
                return true;
            }
        }
    }
    false
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 1e-12 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - (a.0 + dx * t)).hypot(p.1 - (a.1 + dy * t))
}

/// Puts a new text on `target`'s edge with the Text tool, its run starting at the clicked point. The
/// text is left ready for typing; the caller opens an edit session on the returned id.
pub(in crate::ui) fn place_on_edge(design: &mut Design, target: u64, click_in: Pt) -> u64 {
    let id = design.add_text("Text".into(), crate::text::FontRef::Inter, 1.0, egui::Color32::BLACK, click_in.0, click_in.1);
    design.text_on_path(id, target);
    design.apply_text_path(TextPathEdit::Along(Along::Start), &[id]);
    if let Some(shape) = design.shapes.iter().find(|s| s.id == id).cloned()
        && let Some((path, pf, frame, _)) = placed(&shape)
    {
        let (s, _) = path.trace().project(frame.to_path(&shape, click_in));
        design.apply_text_path(TextPathEdit::Offset(s - pf.base), &[id]);
    }
    id
}

/// Draws the guide line and the path's nodes while they are being edited.
pub(in crate::ui) fn paint_nodes(painter: &Painter, grid: &Grid, design: &Design, edit: &PathNodeEdit) {
    let Some(shape) = design.shapes.iter().find(|s| s.id == edit.shape) else { return };
    let Some((path, _, frame, _)) = placed(shape) else { return };
    guide_line(painter, grid, shape, &path, &frame);
    let np = node_path(&path);
    for id in every_node(&np) {
        let at = grid.to_screen(frame.to_world(shape, np.node(id).unwrap().pt));
        if edit.selected.contains(&id) {
            painter.rect_filled(egui::Rect::from_center_size(at, Vec2::splat(7.5)), 0.0, ACCENT_PRIMARY);
        } else {
            paint_anchor(painter, at);
        }
    }
}

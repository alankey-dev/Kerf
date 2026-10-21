use egui::epaint::TextShape;
use egui::{Align2, Color32, Key, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::containers::AlignTo;
use crate::design::{Design, Operation, POINTS_PER_INCH, Pt, Shape};
use crate::edit::{Edit, Subject};
use crate::grid::GridSettings;
use crate::path_edit::{Node, NodeContour, NodeKind, Side};
use crate::snap::{self, Aabb, Guide, SnapGrid};
use crate::transform::{self, Handle, clamp_pos};
use crate::units::Units;

use super::commands::Command;
use super::fonts;
use super::icons::{self, Icon};
use super::tokens::*;
use super::tooltip::tip;

pub(super) const ZOOM_MIN: f32 = 0.1;
pub(super) const ZOOM_MAX: f32 = 8.0;
const SELECTION: Color32 = ACCENT_PRIMARY;
pub(super) const PILL_W: f32 = 130.0;
pub(super) const PILL_H: f32 = 36.0;
const HANDLE_SIZE_PX: f32 = 6.0;
const HANDLE_STROKE_PX: f32 = 1.5;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Pencil,
    Pen,
    Text,
    Measure,
    /// Round, chamfer or scoop a path's corners (spec 55).
    Corner,
    /// Unite or delete the overlap regions of several shapes by dragging across them (spec 57).
    ShapeBuilder,
    /// Cut shapes along a drawn line, or snip a path at a click (spec 57).
    Knife,
    /// Lift a colour and style from any object, then click to apply it (spec 56).
    Eyedropper,
}

/// `_getNextStepZoom`: 5 % steps below 25 %, 50 % steps from 200 %, 25 % otherwise, snapped to the step.
pub fn next_zoom(zoom: f32, zoom_in: bool) -> f32 {
    let step = match (zoom_in, zoom) {
        (_, z) if z < 0.25 - 1e-4 => 0.05,
        (false, z) if z <= 0.25 + 1e-4 => 0.05,
        (true, z) if z >= 2.0 - 1e-4 => 0.5,
        (false, z) if z > 2.0 + 1e-4 => 0.5,
        _ => 0.25,
    };
    let ratio = zoom / step;
    let next = if zoom_in { (ratio + 1e-3).floor() + 1.0 } else { (ratio - 1e-3).ceil() - 1.0 } * step;
    ((next * 100.0).round() / 100.0).clamp(ZOOM_MIN, ZOOM_MAX)
}

#[derive(Clone, Copy)]
enum Grab {
    Move,
    Resize(Handle),
    /// Cmd-dragging a side handle shears the shape instead of resizing it (spec 52).
    Shear(Handle),
    Rotate,
    /// Dragging the centre crosshair sets the shape's rotation pivot (spec 52).
    Pivot,
    Marquee,
    /// Dragging a parametric shape's parameter handle (spec 49).
    Param(super::param_handles::ParamGrip),
}

/// Live drag, carried across frames so resize and rotate read from the shapes as they were grabbed.
struct Drag {
    grab: Grab,
    start_in: Pt,
    originals: Vec<(u64, Shape)>,
    marquee_additive: bool,
}

/// A path being drawn, carried across frames.
enum DrawInProgress {
    Pencil(Vec<Pt>),
    Pen(Vec<Node>),
}

pub struct View {
    pub zoom: f32,
    /// This frame's grid (spec 46): the design's, or while it has none, the default for the units this
    /// canvas first showed, so switching units later never moves the lines.
    pub grid: GridSettings,
    untouched_grid: Option<GridSettings>,
    /// The active mat's cutting area in inches: the artboard. 12 x 12 until a mat is chosen.
    pub artboard_in: (f32, f32),
    /// Artboard centre relative to the viewport centre, in points.
    pub(super) pan_pt: Vec2,
    /// False until the first frame, which scrolls an oversized artboard to its top-left corner.
    placed: bool,
    drag: Option<Drag>,
    /// What the last snap found this gesture, for the canvas to draw and to light a snapped ruler guide.
    snap_marks: Vec<snap::Mark>,
    /// Each shape's snap points cached across a drag's frames (spec 47).
    snap_cache: snap::cache::PointCache,
    draw: Option<DrawInProgress>,
    pub(super) nodes: Option<super::node_edit::NodeEdit>,
    /// The path whose corners the Corner tool is editing (spec 55), and the nodes it has picked.
    pub(super) corners: Option<super::corner_tool::CornerEdit>,
    /// The Align To choice while the selection sits in a container: align to it, or to the selection.
    pub align_to: AlignTo,
    /// The text whose words are being edited in place.
    pub text_edit: Option<super::text_edit::Session>,
    /// View > Show as made.
    pub mode: super::op_style::Mode,
    /// The material last chosen in a Make flow, named in the hover hint.
    pub material: Option<&'static str>,
    /// The one-line notice under the toolbar.
    pub notice: Option<super::notice::Notice>,
    /// After an SVG import, what came in and what was lost, with the Treat hairlines as choice.
    pub import: Option<super::import_notice::ImportNotice>,
    /// The Measure tool's ruler and this frame's Option-hover distance lines.
    pub measure: super::measure_tool::State,
    /// Last pointer position on the artboard, retained while a canvas context menu is open.
    pub(super) clipboard_target_in: Option<Pt>,
    /// Spec 48's fits, glide and hand.
    pub(super) nav: super::view_nav::Nav,
    /// The freehand lasso in progress (spec 50), if any.
    pub(super) lasso: Option<super::lasso::Lasso>,
    /// The Shape Builder's planar map and gesture (spec 57).
    pub(super) shape_builder: Option<super::shape_builder::State>,
    /// The Knife line being drawn (spec 57).
    pub(super) knife: Option<super::knife_tool::Drag>,
    /// The frame a transform gesture (move, resize, rotate or shear) just started and ended at, so the
    /// app can record it as the step Transform Again repeats (spec 52). Taken once, next frame.
    pub(super) gesture_step: Option<(transform::Frame, transform::Frame)>,
    /// This frame's start and centre handle positions for the one selected path text (spec 51).
    pub(super) path_handles: Option<super::text_path::Handles>,
    /// The path-text handle being dragged, carried across frames.
    pub(super) path_grab: Option<super::text_path::Grab>,
    /// Node editing of a path text's own path copy, entered with A.
    pub(super) path_nodes: Option<super::text_path::PathNodeEdit>,
    /// The eyedropper's lifted style and the decoded pixels under the canvas (spec 56).
    pub(super) eyedropper: super::eyedropper::State,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            grid: GridSettings::for_units(Units::Imperial),
            untouched_grid: None,
            artboard_in: DEFAULT_ARTBOARD_IN,
            pan_pt: Vec2::ZERO,
            placed: false,
            drag: None,
            snap_marks: Vec::new(),
            snap_cache: snap::cache::PointCache::default(),
            draw: None,
            nodes: None,
            corners: None,
            align_to: AlignTo::default(),
            text_edit: None,
            mode: super::op_style::Mode::Design,
            material: None,
            notice: None,
            import: None,
            measure: super::measure_tool::State::default(),
            clipboard_target_in: None,
            nav: super::view_nav::Nav::default(),
            lasso: None,
            shape_builder: None,
            knife: None,
            gesture_step: None,
            path_handles: None,
            path_grab: None,
            path_nodes: None,
            eyedropper: Default::default(),
        }
    }
}

/// Drawn paths default to black (the Pen draw colour), editable afterwards with the Select tool.
const DRAW_COLOUR: Color32 = Color32::BLACK;
const RDP_EPSILON_IN: f32 = 0.02;
const PEN_CLOSE_PX: f32 = 12.0;

pub const DEFAULT_ARTBOARD_IN: (f32, f32) = (12.0, 12.0);
/// Where an oversized artboard first sits: its top-left this far inside the viewport, clear of the rulers.
const INITIAL_MARGIN_PX: f32 = RULER + 16.0 - RULER / 2.0;
pub(super) const SNAP_PX: f32 = 6.0;

impl View {
    /// Drops a move, resize, rotate or marquee in progress, so the held pointer edits nothing more until
    /// it is released.
    pub fn end_gesture(&mut self) {
        self.drag = None;
        self.snap_marks.clear();
    }

    pub fn zoom_pct(&self) -> u32 {
        (self.zoom * 100.0).round() as u32
    }

    fn px_per_in(&self) -> f32 {
        POINTS_PER_INCH * self.zoom
    }

    /// The artboard's centre is a smart-guide target.
    fn artboard_centre_in(&self) -> Pt {
        (self.artboard_in.0 / 2.0, self.artboard_in.1 / 2.0)
    }

    /// The point at the centre of the visible canvas, in inches.
    pub fn centre_in(&self) -> Pt {
        let (cx, cy) = self.artboard_centre_in();
        let off = self.pan_pt * self.zoom / self.px_per_in();
        (cx - off.x, cy - off.y)
    }

    /// However far it is panned, some of the artboard stays in view (`view_nav::clamp_pan`).
    fn clamp_pan(&mut self, viewport_px: Vec2) {
        let art_px = vec2(self.artboard_in.0, self.artboard_in.1) * self.px_per_in();
        if !self.placed {
            self.placed = true;
            let start = |art: f32, vp: f32| if art > vp { (art - vp) / 2.0 + INITIAL_MARGIN_PX } else { 0.0 };
            self.pan_pt = vec2(start(art_px.x, viewport_px.x), start(art_px.y, viewport_px.y)) / self.zoom;
        }
        self.pan_pt = super::view_nav::clamp_pan(self.pan_pt, self.zoom, self.artboard_in, viewport_px);
    }

    /// Shows `target`, in inches, whole and centred with a margin, `lead_px` more above and left, and
    /// zoomed no closer than `max_zoom`.
    pub fn fit_to(&mut self, target: Aabb, lead_px: f32, max_zoom: f32) {
        if let Some(viewport) = self.nav.viewport {
            super::view_nav::go(self, super::view_nav::fit(target, self.artboard_in, viewport, lead_px, max_zoom));
        }
    }

    /// Where the artboard sits on screen inside `viewport`, below the ruler strip.
    pub fn artboard_rect(&self, viewport: Rect) -> Rect {
        let art_px = vec2(self.artboard_in.0, self.artboard_in.1) * self.px_per_in();
        let centre = viewport.center() + vec2(RULER, RULER) / 2.0 + self.pan_pt * self.zoom;
        Rect::from_center_size(centre, art_px)
    }
}

#[cfg(test)]
impl View {
    /// Starts an in-progress Pen path from `(point, optional symmetric handle offset)` anchors.
    pub fn begin_pen_for_test(&mut self, anchors: &[(Pt, Option<Pt>)]) {
        let node = |&(pt, handle): &(Pt, Option<Pt>)| {
            let mut n = Node::new(pt, if handle.is_some() { NodeKind::Symmetric } else { NodeKind::Straight });
            if let Some((dx, dy)) = handle {
                n.move_handle(Side::Out, (pt.0 + dx, pt.1 + dy));
            }
            n
        };
        self.draw = Some(DrawInProgress::Pen(anchors.iter().map(node).collect()));
    }
}

pub struct Grid {
    pub origin: Pos2,
    pub scale: f32,
}

impl Grid {
    pub fn fit(rect: Rect, width_in: f32, height_in: f32) -> Self {
        let scale = (rect.width() / width_in).min(rect.height() / height_in);
        let size = Vec2::new(width_in, height_in) * scale;
        Self { origin: rect.center() - size / 2.0, scale }
    }

    pub fn to_screen(&self, (x, y): (f32, f32)) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.scale
    }

    pub fn to_inches(&self, p: Pos2) -> (f32, f32) {
        let v = (p - self.origin) / self.scale;
        (v.x, v.y)
    }

    /// [`Grid::to_screen`] as a matrix.
    pub fn matrix(&self) -> crate::bridge::Matrix {
        crate::bridge::Matrix::scale(self.scale, self.scale).then(crate::bridge::Matrix::translate(self.origin.x, self.origin.y))
    }
}

/// Pencil, Pen and Text place one object, then hand back to Select. Returns the
/// command the canvas raised this frame (double-click an instance: Edit master), for `update` to run.
pub fn show(
    ui: &mut Ui,
    design: &mut Design,
    selection: &mut Vec<u64>,
    view: &mut View,
    tool: &mut Tool,
    units: &mut Units,
    repeat: &mut Option<super::repeat_panel::RepeatPanel>,
) -> Option<Command> {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let response = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());

    let arrows_pan = selection.is_empty() && *tool == Tool::Select && view.nodes.is_none();
    let hand = super::view_nav::input(ui, &response, view, rect, arrows_pan);
    if response.hovered() && !hand && *tool != Tool::Select {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    view.clamp_pan(rect.size());
    let untouched = *view.untouched_grid.get_or_insert_with(|| GridSettings::for_units(*units));
    view.grid = design.grid_or(untouched);
    let art = view.artboard_rect(rect);
    let grid = Grid { origin: art.min, scale: view.px_per_in() };
    if let Some(pointer) = ui.input(|input| input.pointer.hover_pos()).filter(|pointer| art.contains(*pointer)) {
        view.clipboard_target_in = Some(grid.to_inches(pointer));
    } else if !super::context_menu::is_open(ui.ctx()) {
        view.clipboard_target_in = None;
    }
    super::anchors::board(ui, grid.origin, grid.scale);
    let to_in = |p: Pos2| grid.to_inches(p);
    let shift = ui.input(|i| i.modifiers.shift);
    // Cmd picks one member of a Group or Attach, or the child under the pointer inside a Compound;
    // otherwise a hit selects the whole link or group.
    let member = ui.input(|i| i.modifiers.command);

    selection.retain(|&id| design.shapes.iter().any(|s| s.id == id));
    let mut marquee: Option<(Pt, Pt)> = None;
    let mut dragging_to: Option<Pt> = None;

    if *tool != Tool::Select {
        view.nodes = None;
        view.path_nodes = None;
        view.path_grab = None;
    }
    view.path_handles = super::text_path::compute_handles(design, selection, view, &grid);
    if *tool != Tool::Corner {
        view.corners = None;
    }
    if *tool != Tool::Measure {
        view.measure.measurement = None;
    }
    if *tool != Tool::ShapeBuilder {
        view.shape_builder = None;
    }
    if *tool != Tool::Knife {
        view.knife = None;
    }
    if *tool != Tool::Eyedropper {
        view.eyedropper.lifted = None;
    }
    let mut command = None;
    let mut end_tool = false;
    let select = *tool == Tool::Select && view.nodes.is_none();
    let guided = super::guides::input(ui, &response, design, selection, &grid, art, design.snapping(), select, *units);
    let placed = match *tool {
        _ if hand || guided => false,
        Tool::Select if super::text_path::node_input(ui, &response, design, selection, view, &grid) => false,
        Tool::Select if super::node_edit::input(ui, &response, design, selection, &mut view.nodes, &grid) => false,
        Tool::Select if super::text_path::handle_input(ui, &response, design, view, &grid) => false,
        Tool::Select if super::pattern_panel::canvas_input(ui, &response, design, selection, &grid) => false,
        Tool::Select if super::lasso::input(ui, &response, design, selection, view, &grid, shift) => false,
        Tool::Select => {
            view.draw = None;
            if response.drag_started()
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                begin_drag(design, selection, view, &grid, p, shift, member);
            }
            // Cmd held while moving suppresses dropping into and out of containers.
            let moving = !member && view.drag.as_ref().is_some_and(|d| matches!(d.grab, Grab::Move) && !d.originals.is_empty());
            if response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                if moving {
                    dragging_to = Some(to_in(p));
                }
                if !(moving && super::container_canvas::slide(design, selection, to_in(p))) {
                    marquee = continue_drag(design, selection, view, to_in(p), shift, viewport_in(&grid, rect));
                }
            }
            if response.drag_stopped()
                && moving
                && let Some(p) = ui.input(|i| i.pointer.latest_pos())
            {
                super::container_canvas::drop(design, selection, to_in(p));
            }
            if response.drag_stopped() {
                // Record where a transform gesture ran, so Cmd+Shift+D repeats it (spec 52).
                if let Some(d) = &view.drag
                    && matches!(d.grab, Grab::Move | Grab::Resize(_) | Grab::Rotate | Grab::Shear(_))
                {
                    let originals: Vec<Shape> = d.originals.iter().map(|(_, s)| s.clone()).collect();
                    let now = transform::Frame::of(design, design.shapes.iter().filter(|s| selection.contains(&s.id)));
                    if let (Some(from), Some(to)) = (transform::Frame::of(design, &originals), now)
                        && from != to
                    {
                        view.gesture_step = Some((from, to));
                    }
                }
                view.drag = None;
                view.snap_marks.clear();
            }
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                // The pivot crosshair is read before click_select, which would drop the selection on a
                // click that lands on the frame's corner (spec 52).
                let on_pivot = design.shapes.iter().any(|s| selection.contains(&s.id) && s.pivot.is_some())
                    && matches!(super::selection::grip_at(design, selection, &grid, p), Some(super::selection::Grip::Pivot));
                let reset_pivot = response.double_clicked() && on_pivot;
                if reset_pivot {
                    // Double-clicking a set pivot's crosshair resets it to the centre, ahead of node editing.
                    command = Some(Command::Edit(Edit::Pivot(None)));
                } else if !on_pivot {
                    click_select(design, selection, to_in(p), shift, member);
                }
                if !reset_pivot
                    && response.double_clicked()
                    && let [id] = selection[..]
                    && design.shapes.iter().any(|s| s.id == id && crate::path_edit::editable(s))
                {
                    super::node_edit::enter(design, id, &mut view.nodes, &mut view.notice);
                }
                if !reset_pivot
                    && response.double_clicked()
                    && let [id] = selection[..]
                    && super::text_edit::editable(design, id)
                {
                    view.text_edit = Some(super::text_edit::Session::new(id, super::text_edit::Caret::End));
                }
                if !reset_pivot
                    && response.double_clicked()
                    && let [id] = selection[..]
                    && let Some(c) = design.shapes.iter().find(|s| s.id == id).and_then(Shape::instance_of)
                {
                    command = Some(Command::App(crate::edit::AppCommand::EditMaster(c)));
                }
                if !reset_pivot && response.double_clicked() && design.editable_monogram(selection).is_ok() {
                    command = Some(Command::App(crate::edit::AppCommand::EditMonogram));
                }
            }
            super::node_edit::shortcut(ui, design, selection, &mut view.nodes, &mut view.notice);
            super::text_path::node_shortcut(ui, design, selection, &mut view.path_nodes);
            false
        }
        Tool::Pencil => pencil_tool(&response, design, selection, view, &grid),
        Tool::Pen => pen_tool(ui, &response, design, selection, view, &grid),
        Tool::Corner => {
            view.draw = None;
            super::corner_tool::input(ui, &response, design, selection, view, &grid);
            false
        }
        Tool::Measure => {
            view.draw = None;
            super::measure_tool::input(ui, &response, design, &grid, design.snapping(), view.artboard_in, &mut view.measure.measurement);
            false
        }
        Tool::ShapeBuilder => {
            view.draw = None;
            super::shape_builder::input(ui, &response, design, selection, view, &grid);
            false
        }
        Tool::Knife => {
            view.draw = None;
            super::knife_tool::input(ui, &response, design, selection, view, &grid);
            false
        }
        Tool::Eyedropper => {
            view.draw = None;
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                end_tool = true;
            } else if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let hit_at = to_in(p);
                if let Some(hit_id) = design.pick_at(hit_at.0, hit_at.1)
                    && let Some(hit) = super::eyedropper::sample(design, &view.eyedropper.pixels, hit_at)
                {
                    let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
                    if shift && let Some(lifted) = view.eyedropper.lifted.as_ref() {
                        let edit = if alt { Edit::Colour(lifted.colour) } else { Edit::PasteStyle(lifted.style.clone()) };
                        _ = crate::edit::run(design, selection, &edit, Subject::Ids(&[hit_id]));
                    } else if !selection.is_empty() {
                        let edit = if alt { Edit::Colour(hit.colour) } else { Edit::PasteStyle(hit.style.clone()) };
                        _ = crate::edit::run(design, selection, &edit, Subject::Selection);
                        view.eyedropper.lifted = Some(hit);
                    } else {
                        view.eyedropper.lifted = Some(hit);
                    }
                }
            }
            false
        }
        Tool::Text => {
            view.draw = None;
            let edge_within = SNAP_PX / grid.scale;
            if let Some(hover) = ui.input(|i| i.pointer.hover_pos()).filter(|p| art.contains(*p))
                && super::text_path::edge_at(design, to_in(hover), edge_within).is_some()
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let pt = to_in(p);
                let session = match design.pick_at(pt.0, pt.1).filter(|&id| super::text_edit::editable(design, id)) {
                    Some(id) => super::text_edit::Session::new(id, super::text_edit::Caret::End),
                    None if super::text_path::edge_at(design, pt, edge_within).is_some() => {
                        let target = super::text_path::edge_at(design, pt, edge_within).unwrap();
                        super::text_edit::Session::new(super::text_path::place_on_edge(design, target, pt), super::text_edit::Caret::SelectAll)
                    }
                    None => {
                        let pt = snap_start(pt, grid_snap(design, view));
                        let id = design.add_text("Text".into(), crate::text::FontRef::Inter, 1.0, DRAW_COLOUR, pt.0, pt.1);
                        super::text_edit::Session::new(id, super::text_edit::Caret::SelectAll)
                    }
                };
                *selection = vec![session.id];
                view.text_edit = Some(session);
                true
            } else {
                false
            }
        }
    };
    if placed {
        *tool = Tool::Select;
    }
    if end_tool {
        *tool = Tool::Select;
        view.eyedropper.lifted = None;
    }
    super::components_panel::drop_on(&response, design, selection, &mut view.notice, to_in);
    if *tool == Tool::Select
        && view.nodes.is_none()
        && !guided
        && response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        super::context_menu::on_canvas(ui.ctx(), design, selection, p, to_in(p));
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BG_CANVAS_VOID);
    paint_artboard(&painter, art);
    paint_grid(&painter.with_clip_rect(art.intersect(rect)), view, art);
    paint_shapes(&painter, &grid, design, view.mode);
    super::guides::paint(ui, &painter, &grid, art, design, &snapped_lines(&view.snap_marks), *units);
    super::container_canvas::paint(&painter, &grid, design, selection, dragging_to);
    if let Some(session) = &mut view.text_edit
        && !super::text_edit::show(ui, &grid, design, session)
    {
        view.text_edit = None;
    }
    let pattern = super::pattern_panel::editing(ui.ctx(), design, selection).filter(|_| *tool == Tool::Select);
    if let Some(edit) = &view.path_nodes {
        super::text_path::paint_nodes(&painter, &grid, design, edit);
    } else if let Some(edit) = &view.corners {
        super::corner_tool::paint(&painter, &grid, design, edit);
    } else {
        match (&view.nodes, pattern) {
            (Some(edit), _) => super::node_edit::paint(&painter, &grid, design, edit),
            (None, Some(id)) => super::pattern_panel::paint(&painter, &grid, design, id),
            (None, None) => {
                let grab = view.drag.as_ref().map(|d| d.grab);
                let chrome = super::selection::Chrome {
                    select_tool: *tool == Tool::Select,
                    dragging: grab.is_some() || view.lasso.is_some(),
                    resizing: matches!(grab, Some(Grab::Resize(_))),
                    canvas: rect,
                };
                super::selection::show(ui, &painter, &grid, design, selection, *units, chrome);
                if *tool == Tool::Select {
                    let active = match grab {
                        Some(Grab::Param(g)) => Some(g),
                        _ => None,
                    };
                    super::param_handles::paint(&painter, &grid, design, selection, active, grab.is_some(), *units);
                    super::text_path::paint(ui, &painter, &grid, design, view, *units);
                }
            }
        }
    }
    if let Some((a, b)) = marquee {
        let m = Rect::from_two_pos(grid.to_screen(a), grid.to_screen(b));
        painter.rect_stroke(snap_rect(m), 0.0, Stroke::new(1.0_f32, SELECTION), StrokeKind::Middle);
    }
    super::lasso::paint(&painter, &grid, view);
    let hover_in = ui.input(|i| i.pointer.hover_pos()).filter(|p| art.contains(*p)).map(to_in);
    super::shape_builder::paint(&painter, &grid, view, hover_in);
    super::knife_tool::paint(&painter, &grid, view);
    paint_draw(&painter, &grid, view, ui.input(|i| i.pointer.hover_pos()));
    if *tool == Tool::Eyedropper
        && let Some(pos) = ui.input(|i| i.pointer.hover_pos()).filter(|p| art.contains(*p))
        && let Some(lift) = super::eyedropper::sample(design, &view.eyedropper.pixels, to_in(pos))
    {
        super::eyedropper::paint_cursor(&painter, pos, lift.colour, ui.input(|i| i.modifiers.alt));
    }
    super::snap_draw::draw(&painter, &grid, art, &view.snap_marks);
    if let Some(m) = view.measure.measurement {
        super::measure_tool::show(ui, &painter, &grid, &m, *units);
    }
    if let (Some(r), Some(p)) = (view.drag.as_ref().and_then(|d| drag_readout(design, selection, d)), ui.input(|i| i.pointer.latest_pos())) {
        super::measure_tool::readout(ui, &painter, r.label(*units), p);
    }
    let hovering = *tool == Tool::Select && view.nodes.is_none() && view.drag.is_none();
    view.measure.distances = if hovering { super::measure_tool::distances(ui, &painter, &grid, design, selection, *units) } else { Vec::new() };
    paint_rulers(&painter, view, *units, art);
    super::anchors::mark(ui, super::anchors::Anchor::Rulers, Rect::from_min_max(art.min - Vec2::splat(RULER), pos2(art.right(), art.top())).intersect(rect));
    units_corner(ui, art, units, view, design, selection);
    if let Some(zoom) = zoom_pill(ui, rect, view, design, selection) {
        command = Some(Command::App(zoom));
    }
    super::snap_panel::show(ui, magnet_rect(rect), design, selection);
    super::op_legend::show(ui, rect, design, selection, view.mode);
    if *tool == Tool::Select && view.draw.is_none() && view.nodes.is_none() && view.drag.is_none() && view.text_edit.is_none() && view.measure.distances.is_empty() {
        super::op_legend::hover_hint(&response, &grid, design, selection, view.material, view.mode);
    }
    if let Some(cmd) = super::repeat_panel::show(ui, &grid, rect, design, selection, *units, repeat) {
        command = Some(cmd);
    }
    match super::context_menu::show(ui.ctx(), design, selection, view.nodes.as_ref(), view.align_to) {
        Some(super::context_menu::Picked::Node(action)) => {
            if let Some(edit) = view.nodes.as_mut() {
                super::node_edit::apply(design, edit, action);
            }
        }
        Some(super::context_menu::Picked::Edit(edit)) => {
            let before = selection.clone();
            _ = crate::edit::run(design, selection, &edit, Subject::Selection);
            if matches!(edit, crate::edit::Edit::SelectSame(_)) && ui.input(|i| i.modifiers.shift) {
                super::commands::keep_selected(&before, selection);
            }
        }
        Some(super::context_menu::Picked::EditEffect(at, from)) => super::effects_list::open(ui.ctx(), at, Rect::from_min_size(from, Vec2::ZERO)),
        Some(super::context_menu::Picked::App(app)) => command = Some(Command::App(app)),
        Some(super::context_menu::Picked::Clipboard(action)) => command = Some(Command::Clipboard(action)),
        None => {}
    }
    command
}

fn snap_start(p: Pt, grid: Option<SnapGrid>) -> Pt {
    grid.map_or(p, |g| g.nearest(p))
}

/// The grid a move, resize or new path snaps to while Snap to grid is on.
fn grid_snap(design: &Design, view: &View) -> Option<SnapGrid> {
    design.snap.grid.then(|| view.grid.snap())
}

/// The visible canvas rectangle in world inches, so snapping gathers point targets only from the shapes
/// on screen (spec 47).
fn viewport_in(grid: &Grid, rect: Rect) -> Aabb {
    let a = grid.to_inches(rect.min);
    let b = grid.to_inches(rect.max);
    Aabb { min_x: a.0.min(b.0), min_y: a.1.min(b.1), max_x: a.0.max(b.0), max_y: a.1.max(b.1) }
}

/// Returns true once a stroke is committed.
fn pencil_tool(response: &Response, design: &mut Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid) -> bool {
    if response.drag_started()
        && let Some(p) = response.interact_pointer_pos()
    {
        let start = snap_start(grid.to_inches(p), grid_snap(design, view));
        view.draw = Some(DrawInProgress::Pencil(vec![start]));
    }
    if response.dragged()
        && let (Some(DrawInProgress::Pencil(pts)), Some(p)) = (view.draw.as_mut(), response.interact_pointer_pos())
    {
        pts.push(grid.to_inches(p));
    }
    if response.drag_stopped()
        && let Some(DrawInProgress::Pencil(pts)) = view.draw.take()
        && pts.len() >= 2
    {
        let simplified = crate::geometry::rdp(&pts, RDP_EPSILON_IN);
        let contour = crate::geometry::smooth_open_path(&simplified);
        if let Some(id) = design.add_path(contour, DRAW_COLOUR, Operation::Pen) {
            *selection = vec![id];
            return true;
        }
    }
    false
}

/// Returns true once a path is finished.
fn pen_tool(ui: &Ui, response: &Response, design: &mut Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid) -> bool {
    let (enter, esc, back) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape), i.key_pressed(Key::Backspace)));
    let shift = ui.input(|i| i.modifiers.shift);
    if esc {
        view.draw = None;
        return false;
    }
    if back && let Some(DrawInProgress::Pen(anchors)) = view.draw.as_mut() {
        anchors.pop();
        if anchors.is_empty() {
            view.draw = None;
        }
        return false;
    }
    if (enter || response.double_clicked()) && finish_pen(design, selection, view, false) {
        return true;
    }
    // A click-drag places a Symmetric node, or a Cusp while Option is held; a plain click places a
    // Straight node (or closes on the first anchor).
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        if near_first_anchor(view, grid, p) {
            return finish_pen(design, selection, view, true);
        }
        ensure_pen(view);
        push_anchor(view, pen_point(view, design, grid, p, shift), NodeKind::Symmetric, grid_snap(design, view));
    }
    if response.dragged()
        && let (Some(DrawInProgress::Pen(nodes)), Some(p)) = (view.draw.as_mut(), response.interact_pointer_pos())
        && let Some(last) = nodes.last_mut()
    {
        if ui.input(|i| i.modifiers.alt) {
            last.kind = NodeKind::Cusp;
        }
        last.move_handle(Side::Out, grid.to_inches(p));
    }
    if response.drag_stopped()
        && let Some(DrawInProgress::Pen(nodes)) = view.draw.as_mut()
        && let Some(last) = nodes.last_mut()
        && last.out.is_none_or(|h| (h.0 - last.pt.0).hypot(h.1 - last.pt.1) < 0.03)
    {
        *last = Node::new(last.pt, NodeKind::Straight);
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        if near_first_anchor(view, grid, p) {
            return finish_pen(design, selection, view, true);
        }
        ensure_pen(view);
        push_anchor(view, pen_point(view, design, grid, p, shift), NodeKind::Straight, grid_snap(design, view));
    }
    false
}

fn near_first_anchor(view: &View, grid: &Grid, p: Pos2) -> bool {
    matches!(view.draw.as_ref(), Some(DrawInProgress::Pen(a)) if a.len() >= 2 && grid.to_screen(a[0].pt).distance(p) < PEN_CLOSE_PX)
}

fn ensure_pen(view: &mut View) {
    if !matches!(view.draw, Some(DrawInProgress::Pen(_))) {
        view.draw = Some(DrawInProgress::Pen(Vec::new()));
    }
}

fn push_anchor(view: &mut View, pt: Pt, kind: NodeKind, snap: Option<SnapGrid>) {
    if let Some(DrawInProgress::Pen(nodes)) = view.draw.as_mut() {
        let pt = if nodes.is_empty() { snap_start(pt, snap) } else { pt };
        nodes.push(Node::new(pt, kind));
    }
}

/// Where a new Pen anchor at `p` lands: snapped to the ruler guides, then, while Shift is held, locked
/// to an angle step from the last anchor so a segment holds horizontal, vertical or a diagonal.
fn pen_point(view: &View, design: &Design, grid: &Grid, p: Pos2, shift: bool) -> Pt {
    let pt = super::guides::snapped(design, design.snapping(), grid, p);
    if shift
        && let Some(DrawInProgress::Pen(nodes)) = &view.draw
        && let Some(last) = nodes.last()
    {
        let d = snap::constrain_angle((pt.0 - last.pt.0, pt.1 - last.pt.1), design.snap.angle_step_deg);
        return (last.pt.0 + d.0, last.pt.1 + d.1);
    }
    pt
}

/// Commits the in-progress pen path. Returns true if a path was finished.
fn finish_pen(design: &mut Design, selection: &mut Vec<u64>, view: &mut View, closed: bool) -> bool {
    let Some(DrawInProgress::Pen(nodes)) = view.draw.take() else { return false };
    if nodes.len() < 2 {
        return false;
    }
    if let Some(id) = crate::path_edit::add_drawn(design, &NodeContour { nodes, closed }, DRAW_COLOUR, Operation::Pen) {
        *selection = vec![id];
    }
    true
}

fn begin_drag(design: &Design, selection: &mut Vec<u64>, view: &mut View, grid: &Grid, p: Pos2, shift: bool, member: bool) {
    view.snap_cache.retain(&design.shapes.iter().map(|s| s.id).collect());
    let start_in = grid.to_inches(p);
    // A parameter handle wins over the resize handle it may sit near.
    if let Some(grip) = super::param_handles::grip_at(design, selection, grid, p) {
        let originals = design.shapes.iter().filter(|s| selection.contains(&s.id)).map(|s| (s.id, s.clone())).collect();
        view.drag = Some(Drag { grab: Grab::Param(grip), start_in, originals, marquee_additive: false });
        return;
    }
    if let Some(grip) = super::selection::grip_at(design, selection, grid, p) {
        let side = |h| matches!(h, Handle::Top | Handle::Bottom | Handle::Left | Handle::Right);
        let grab = match grip {
            // Cmd on a side handle shears instead of resizing (spec 52).
            super::selection::Grip::Resize(handle) if member && side(handle) => Grab::Shear(handle),
            super::selection::Grip::Resize(handle) => Grab::Resize(handle),
            super::selection::Grip::Rotate => Grab::Rotate,
            super::selection::Grip::Pivot => Grab::Pivot,
        };
        let originals = design.shapes.iter().filter(|s| selection.contains(&s.id)).map(|s| (s.id, s.clone())).collect();
        view.drag = Some(Drag { grab, start_in, originals, marquee_additive: false });
        return;
    }
    match hit(design, start_in, member) {
        Some(id) => {
            if !selection.contains(&id) {
                if !shift {
                    selection.clear();
                }
                let picked = if member { vec![id] } else { design.expand(&[id]) };
                selection.retain(|s| !picked.contains(s));
                selection.extend(picked);
            }
            let originals: Vec<_> = design.shapes.iter().filter(|s| selection.contains(&s.id) && !s.lock_position).map(|s| (s.id, s.clone())).collect();
            view.drag = (!originals.is_empty()).then_some(Drag { grab: Grab::Move, start_in, originals, marquee_additive: false });
        }
        None => {
            if !shift {
                selection.clear();
            }
            view.drag = Some(Drag { grab: Grab::Move, start_in, originals: Vec::new(), marquee_additive: shift });
            if let Some(d) = view.drag.as_mut() {
                d.grab = Grab::Marquee;
            }
        }
    }
}

fn continue_drag(design: &mut Design, selection: &mut Vec<u64>, view: &mut View, pointer: Pt, shift: bool, viewport: Aabb) -> Option<(Pt, Pt)> {
    let drag = view.drag.take()?;
    view.snap_marks.clear();
    let threshold = SNAP_PX / view.px_per_in();
    let grid = grid_snap(design, view);
    let mat = Aabb { min_x: 0.0, min_y: 0.0, max_x: view.artboard_in.0, max_y: view.artboard_in.1 };
    // Nothing snaps to the moving shapes, their descendants, or the containers that refit around them.
    let moving: Vec<u64> = drag.originals.iter().map(|(id, _)| *id).collect();
    let mut exclude = design.with_descendants(&moving);
    for id in &moving {
        exclude.extend(std::iter::successors(design.parent_of(*id), |&p| design.parent_of(p)));
    }
    let result = match drag.grab {
        Grab::Move => {
            let mut delta = (pointer.0 - drag.start_in.0, pointer.1 - drag.start_in.1);
            if shift {
                // Shift locks the move to the nearer ray and draws it; the magnet steps aside.
                delta = snap::constrain_angle(delta, design.snap.angle_step_deg);
                view.snap_marks = vec![snap::Mark::Ray { from: drag.start_in, to: (drag.start_in.0 + delta.0, drag.start_in.1 + delta.1) }];
            } else if design.snapping() {
                let candidate = snap::cache::move_candidate(design, &drag.originals, delta, &design.snap, &view.snap_cache);
                let targets = snap::cache::targets(design, &exclude, design.shown_guides(), grid, mat, viewport, threshold, &design.snap, &view.snap_cache);
                let snapped = snap::snap(&candidate, &targets, &design.snap);
                delta = (delta.0 + snapped.dx, delta.1 + snapped.dy);
                view.snap_marks = snapped.marks;
            }
            for (id, orig) in &drag.originals {
                if let Some(s) = design.shape_mut(*id) {
                    s.x_in = clamp_pos(orig.x_in + delta.0, orig.width_in, view.artboard_in.0);
                    s.y_in = clamp_pos(orig.y_in + delta.1, orig.height_in, view.artboard_in.1);
                }
            }
            None
        }
        Grab::Resize(handle) => {
            let mut p = pointer;
            if design.snapping() {
                let targets = snap::cache::line_targets(design, &exclude, design.shown_guides(), grid, mat, threshold, &design.snap);
                let candidate = snap::Candidate { points: vec![(pointer, snap::Kind::Edge)], frame: None };
                let snapped = snap::snap(&candidate, &targets, &design.snap);
                p = (pointer.0 + snapped.dx, pointer.1 + snapped.dy);
                view.snap_marks = snapped.marks;
            }
            let originals: Vec<Shape> = drag.originals.iter().map(|(_, s)| s.clone()).collect();
            let keep_aspect = shift != originals.iter().any(|s| s.lock_aspect);
            let mut resized = transform::resize_many(design, &originals, handle, p, keep_aspect);
            if let ([orig], [to]) = (originals.as_slice(), resized.as_mut_slice())
                && let Some(reflowed) = super::toolbar::rewrap(orig, handle, to)
            {
                *to = reflowed;
            }
            super::selection::write_back(design, resized);
            None
        }
        Grab::Rotate => {
            let originals: Vec<Shape> = drag.originals.iter().map(|(_, s)| s.clone()).collect();
            if let Some(f) = transform::Frame::of(design, &originals) {
                let delta = f.rotation_to(pointer, shift) - f.rotation_deg;
                // A lone shape with a pivot turns about it, not the frame centre (spec 52).
                let moved = match originals.as_slice() {
                    [one] if one.pivot.is_some() => transform::rotate_many_about(design, &originals, delta, f.pivot_world(one.pivot)),
                    _ => transform::rotate_many(design, &originals, delta),
                };
                super::selection::write_back(design, moved);
            }
            None
        }
        Grab::Shear(handle) => {
            let originals: Vec<Shape> = drag.originals.iter().map(|(_, s)| s.clone()).collect();
            if let ([one], Some(f)) = (originals.as_slice(), transform::Frame::of(design, &originals))
                && !one.lock_position
                && let Some(s) = design.shape_mut(one.id)
            {
                s.skew_deg = transform::sheared(&f, handle, pointer);
            }
            None
        }
        Grab::Pivot => {
            if let [(id, orig)] = drag.originals.as_slice()
                && let Some(f) = transform::Frame::of(design, std::slice::from_ref(orig))
            {
                let extra: Vec<Pt> = if design.snapping() {
                    snap::cache::targets(design, &exclude, design.shown_guides(), grid, mat, viewport, threshold, &design.snap, &view.snap_cache)
                        .points
                        .iter()
                        .map(|&(p, _)| p)
                        .collect()
                } else {
                    Vec::new()
                };
                let world = super::pivot::snapped(&f, &extra, threshold, pointer);
                if let Some(s) = design.shape_mut(*id) {
                    s.pivot = Some(f.local_pivot(world));
                }
            }
            None
        }
        Grab::Param(grip) => {
            if let Some((_, original)) = drag.originals.first() {
                super::param_handles::drag(design, original, grip, pointer);
            }
            None
        }
        Grab::Marquee => {
            let rect = Rect::from_two_pos(pos2(drag.start_in.0, drag.start_in.1), pos2(pointer.0, pointer.1));
            let hits: Vec<u64> = design
                .drawn()
                .filter(|s| !s.locked && (s.operation != Operation::Guide || design.children(Some(s.id)).next().is_some()))
                .filter(|s| {
                    let b = design.geometry(s).bounds();
                    rect.intersects(Rect::from_min_max(pos2(b.min_x, b.min_y), pos2(b.max_x, b.max_y)))
                })
                .map(|s| s.id)
                .collect();
            let hits = design.expand(&hits);
            if drag.marquee_additive {
                for id in hits {
                    if !selection.contains(&id) {
                        selection.push(id);
                    }
                }
            } else {
                *selection = hits;
            }
            Some((drag.start_in, pointer))
        }
    };
    view.drag = Some(drag);
    result
}

fn drag_readout(design: &Design, selection: &[u64], drag: &Drag) -> Option<crate::measure::Readout> {
    match drag.grab {
        Grab::Move => {
            let (id, was) = drag.originals.first()?;
            let now = design.shapes.iter().find(|s| s.id == *id)?;
            Some(crate::measure::Readout::Moved { dx: now.x_in - was.x_in, dy: now.y_in - was.y_in })
        }
        Grab::Resize(_) => {
            let frame = transform::Frame::of(design, design.shapes.iter().filter(|s| selection.contains(&s.id)))?;
            Some(crate::measure::Readout::Resized { w: frame.width_in, h: frame.height_in })
        }
        Grab::Param(_) => None,
        Grab::Rotate | Grab::Shear(_) | Grab::Pivot | Grab::Marquee => None,
    }
}

/// The shape a click at `p` picks: with Cmd (`member`) the one inside a group, else the top-level shape.
/// Locked layers are never picked.
fn hit(design: &Design, p: Pt, member: bool) -> Option<u64> {
    if member { design.hit_test_deep(p).filter(|&id| design.shapes.iter().any(|s| s.id == id && !s.locked)) } else { design.pick_at(p.0, p.1) }
}

fn click_select(design: &Design, selection: &mut Vec<u64>, pointer: Pt, shift: bool, member: bool) {
    let Some(id) = hit(design, pointer, member) else {
        if !shift {
            selection.clear();
        }
        return;
    };
    let picked = if member { vec![id] } else { design.expand(&[id]) };
    if !shift {
        *selection = picked;
    } else {
        let was_selected = selection.contains(&id);
        selection.retain(|s| !picked.contains(s));
        if !was_selected {
            selection.extend(picked);
        }
    }
}

/// The document sheet: a soft drop shadow, then a white sheet with a 1px hairline.
fn paint_artboard(painter: &Painter, art: Rect) {
    let shadow = egui::Shadow { offset: [0, 4], blur: 24, spread: 0, color: ARTBOARD_SHADOW };
    painter.add(shadow.as_shape(art, 2.0));
    painter.rect_filled(art, 2.0, BG_ARTBOARD);
    painter.rect_stroke(art, 2.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
}

/// The grid lives on the artboard only; `art` is also the clip, so lines stop at its edge.
fn paint_grid(painter: &Painter, view: &View, art: Rect) {
    let clip = painter.clip_rect();
    let xs = view.grid.lines(view.zoom, view.grid.origin_in.0, inches_within(view, clip.x_range(), art.left(), view.artboard_in.0));
    let ys = view.grid.lines(view.zoom, view.grid.origin_in.1, inches_within(view, clip.y_range(), art.top(), view.artboard_in.1));
    let px = view.px_per_in();
    // The artboard's own edge is its border, not a grid line.
    let inside = |m: &&crate::grid::Mark| m.at_in > 1e-4;
    for (major, colour) in [(false, GRID_MINOR), (true, GRID_MAJOR)] {
        let stroke = Stroke::new(1.0_f32, colour);
        for m in xs.iter().filter(inside).filter(|m| (m.sub == 0) == major) {
            painter.vline((art.left() + m.at_in * px).round() + 0.5, art.y_range(), stroke);
        }
        for m in ys.iter().filter(inside).filter(|m| (m.sub == 0) == major) {
            painter.hline(art.x_range(), (art.top() + m.at_in * px).round() + 0.5, stroke);
        }
    }
}

/// The part of `range` (screen points, a pixel of slack before it) that lies on the artboard, in inches
/// from its edge at `edge_px`.
fn inches_within(view: &View, range: egui::Rangef, edge_px: f32, extent_in: f32) -> (f32, f32) {
    let px = view.px_per_in();
    (((range.min - 1.0 - edge_px) / px).max(0.0), ((range.max - edge_px) / px).min(extent_in))
}

/// Rulers hug the artboard's top and left edges from outside, in the void, and move with it. A tick at
/// every grid line, a label every major line that has room for one.
fn paint_rulers(painter: &Painter, view: &View, units: Units, art: Rect) {
    let top = Rect::from_min_max(pos2(art.left(), art.top() - RULER), pos2(art.right(), art.top()));
    let left = Rect::from_min_max(pos2(art.left() - RULER, art.top()), pos2(art.left(), art.bottom()));
    let tick = Stroke::new(1.0_f32, RULER_TICK);
    let drawn = view.grid.ruler(view.zoom);
    let every = crate::grid::label_every(drawn.major_in, view.zoom) as i64;
    let labelled = |m: &crate::grid::Mark| m.sub == 0 && m.major.rem_euclid(every) == 0;
    let font = fonts::regular(12.0);
    let px = view.px_per_in();

    let top_painter = painter.with_clip_rect(top.intersect(painter.clip_rect()));
    let range = inches_within(view, top_painter.clip_rect().x_range(), art.left(), view.artboard_in.0);
    for m in crate::grid::marks(drawn, view.grid.origin_in.0, range) {
        let x = (art.left() + m.at_in * px).round() + 0.5;
        top_painter.vline(x, (top.top() + RULER * crate::grid::tick(m.sub, drawn.per_major))..=top.bottom(), tick);
        if labelled(&m) {
            top_painter.text(pos2(x + 4.0, top.top() + 4.0), Align2::LEFT_TOP, crate::grid::label(units, m.at_in), font.clone(), TEXT_MUTED);
        }
    }
    let left_painter = painter.with_clip_rect(left.intersect(painter.clip_rect()));
    let range = inches_within(view, left_painter.clip_rect().y_range(), art.top(), view.artboard_in.1);
    for m in crate::grid::marks(drawn, view.grid.origin_in.1, range) {
        let y = (art.top() + m.at_in * px).round() + 0.5;
        left_painter.hline((left.left() + RULER * crate::grid::tick(m.sub, drawn.per_major))..=left.right(), y, tick);
        if labelled(&m) {
            let galley = painter.layout_no_wrap(crate::grid::label(units, m.at_in), font.clone(), TEXT_MUTED);
            let pos = pos2(left.left() + 4.0, y + 4.0 + galley.size().x);
            left_painter.add(TextShape::new(pos, galley, TEXT_MUTED).with_angle(-std::f32::consts::FRAC_PI_2));
        }
    }
}

/// Kerf has no canvas-settings panel, so the ruler corner toggles the units.
/// A right-click opens the Grid popover (spec 46).
fn units_corner(ui: &mut Ui, art: Rect, units: &mut Units, view: &View, design: &mut Design, selection: &mut Vec<u64>) {
    let corner = Rect::from_min_size(art.min - Vec2::splat(RULER), Vec2::splat(RULER));
    let body = if view.grid.hidden_at(view.zoom) {
        "Switch between inches and centimetres. Right-click for grid settings. Grid hidden at this zoom."
    } else {
        "Switch between inches and centimetres. Right-click for grid settings."
    };
    let resp = tip(ui.interact(corner, ui.id().with("units"), Sense::click()), "Ruler units", body, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Ruler units"));
    if resp.hovered() {
        ui.painter().text(corner.center(), Align2::CENTER_CENTER, units.suffix(), fonts::regular(10.0), TEXT_MUTED);
    }
    if resp.clicked() {
        *units = match *units {
            Units::Imperial => Units::Metric,
            Units::Metric => Units::Imperial,
        };
    }
    if resp.secondary_clicked() {
        super::grid_panel::open(ui.ctx());
    }
    super::grid_panel::show(ui, corner, design, selection, view.grid, *units);
}

/// `canvas-ui-zoom` with zoom-to-fit off: 130 x 36, 45 px from the canvas edge, 25 px from the bottom.
/// Returns the zoom command its buttons or its Zoom to menu raised.
fn zoom_pill(ui: &mut Ui, area: Rect, view: &mut View, design: &Design, selection: &[u64]) -> Option<crate::edit::AppCommand> {
    use crate::edit::AppCommand;
    let pill = Rect::from_min_size(pos2(area.left() + 45.0, area.bottom() - 25.0 - PILL_H), vec2(PILL_W, PILL_H));
    ui.painter().rect_filled(pill, PILL_H / 2.0, Color32::WHITE);
    super::anchors::mark(ui, super::anchors::Anchor::ZoomControls, pill);
    ui.painter().rect_stroke(pill, PILL_H / 2.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let out = pill_button(ui, Rect::from_center_size(pill.left_center() + vec2(24.0, 0.0), Vec2::splat(28.0)), "Zoom out", Icon::Minus, AppCommand::ZoomOut);
    let into = pill_button(ui, Rect::from_center_size(pill.right_center() - vec2(24.0, 0.0), Vec2::splat(28.0)), "Zoom in", Icon::Plus, AppCommand::ZoomIn);
    let value = Rect::from_center_size(pill.center(), vec2(52.0, 28.0));
    let picked = super::zoom_menu::show(ui, value, view.zoom_pct(), design, selection);
    let pct = format!("{}%", view.zoom_pct());
    ui.painter().text(pill.center(), Align2::CENTER_CENTER, pct, fonts::regular(14.0), TEXT_PRIMARY);
    let stepped = [(out.clicked(), AppCommand::ZoomOut), (into.clicked(), AppCommand::ZoomIn)].into_iter().find(|(clicked, _)| *clicked).map(|(_, c)| c);
    let magnet_r = magnet_rect(area);
    let magnet = magnet_toggle(ui, magnet_r.center(), design);
    let chevron_r = Rect::from_center_size(magnet_r.right_center() + vec2(9.0, 0.0), vec2(14.0, PILL_H));
    // The chevron shows once the pointer is over the magnet, so it never clutters the resting canvas.
    let reveal = ui.rect_contains_pointer(magnet_r.union(chevron_r)) || super::snap_panel::is_open(ui.ctx());
    let chevron = snap_chevron(ui, chevron_r, reveal);
    let snap = magnet.then_some(AppCommand::Snapping).or(chevron.then_some(AppCommand::SnapSettings));
    stepped.or(picked).or(snap)
}

/// The magnet button's rectangle, right of the zoom pill in `area`. Shared by the button and the
/// Snapping popover it anchors.
fn magnet_rect(area: Rect) -> Rect {
    let pill = Rect::from_min_size(pos2(area.left() + 45.0, area.bottom() - 25.0 - PILL_H), vec2(PILL_W, PILL_H));
    Rect::from_center_size(pill.right_center() + vec2(22.0, 0.0), Vec2::splat(PILL_H))
}

/// A small chevron beside the magnet that opens the Snapping popover (spec 47). It paints only while
/// `reveal` holds, but always answers clicks, so a test and the palette can reach it.
fn snap_chevron(ui: &mut Ui, rect: Rect, reveal: bool) -> bool {
    let resp =
        tip(ui.interact(rect, ui.id().with("snap-chevron"), Sense::click()), "Snapping settings", "Choose what the magnet snaps to and the angle Shift locks to.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Snapping settings"));
    if !reveal {
        return resp.clicked();
    }
    let ink = if resp.hovered() { ACCENT_PRIMARY } else { Color32::from_gray(0x9a) };
    let c = rect.center();
    ui.painter().add(egui::Shape::line(vec![c + vec2(-3.0, -2.0), c + vec2(0.0, 2.0), c + vec2(3.0, -2.0)], Stroke::new(1.5_f32, ink)));
    resp.clicked()
}

/// A magnet button that toggles snapping to the grid and to objects together; it reads filled while
/// either is on. Returns true when clicked, for the caller to apply.
fn magnet_toggle(ui: &mut Ui, centre: Pos2, design: &Design) -> bool {
    let rect = Rect::from_center_size(centre, Vec2::splat(PILL_H));
    ui.painter().rect_filled(rect, PILL_H / 2.0, Color32::WHITE);
    ui.painter().rect_stroke(rect, PILL_H / 2.0, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let resp = ui.interact(rect, ui.id().with("snap"), Sense::click());
    super::anchors::mark(ui, super::anchors::Anchor::Snapping, rect);
    let resp = tip(resp, "Snapping", "Snap to the grid, other objects' edges and centres, and the mat centre.", None);
    let on = design.snapping();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, "Snapping"));
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), 14.0, ACCENT_HOVER);
    }
    let ink = if on { ACCENT_PRIMARY } else { Color32::from_gray(0x9a) };
    paint_magnet(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.0)), ink);
    resp.clicked()
}

/// A horseshoe magnet in outline, tipped 45° so its poles point down and left, with a band across each
/// pole. Drawn on a 24-unit grid centred on (12, 12).
fn paint_magnet(painter: &Painter, rect: Rect, ink: Color32) {
    let (sin, cos) = 45.0_f32.to_radians().sin_cos();
    let p = |x: f32, y: f32| {
        let (dx, dy) = (x - 12.0, y - 12.0);
        rect.center() + vec2(dx * cos - dy * sin, dx * sin + dy * cos) * rect.width() / 24.0
    };
    let arc = |r: f32, from: f32, to: f32| (0..=12).map(move |i| from + (to - from) * i as f32 / 12.0).map(move |a: f32| (12.0 + r * a.cos(), 10.0 + r * a.sin()));
    let (outer, inner, foot) = (8.0, 3.5, 20.5);
    let mut outline = vec![p(12.0 - outer, foot)];
    outline.extend(arc(outer, std::f32::consts::PI, std::f32::consts::TAU).map(|(x, y)| p(x, y)));
    outline.push(p(12.0 + outer, foot));
    outline.push(p(12.0 + inner, foot));
    outline.extend(arc(inner, std::f32::consts::TAU, std::f32::consts::PI).map(|(x, y)| p(x, y)));
    outline.push(p(12.0 - inner, foot));
    let stroke = Stroke::new(1.5_f32, ink);
    painter.add(egui::Shape::closed_line(outline, stroke));
    painter.add(egui::Shape::line_segment([p(12.0 - outer, 16.5), p(12.0 - inner, 16.5)], stroke));
    painter.add(egui::Shape::line_segment([p(12.0 + inner, 16.5), p(12.0 + outer, 16.5)], stroke));
}

fn pill_button(ui: &mut Ui, rect: Rect, label: &str, icon: Icon, app: crate::edit::AppCommand) -> Response {
    let info = super::commands::info(&Command::App(app));
    let keys = info.shortcut.map(|k| ui.ctx().format_shortcut(&k));
    let resp = tip(ui.interact(rect, ui.id().with(label), Sense::click()), label, info.tip, keys.as_deref());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), 14.0, ACCENT_HOVER);
    }
    icons::paint_icon(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), TEXT_PRIMARY);
    resp
}

fn paint_shapes(painter: &Painter, grid: &Grid, design: &Design, mode: super::op_style::Mode) {
    for shape in design.drawn().filter(|s| !s.hidden) {
        super::op_style::paint_shape(painter, design, shape, grid.matrix(), grid.scale, mode);
    }
}

/// Renders the path being drawn: the pencil trail, or the pen path with anchors, handles and a
/// rubber-band preview from the last anchor to the cursor.
fn paint_draw(painter: &Painter, grid: &Grid, view: &View, hover: Option<Pos2>) {
    match &view.draw {
        Some(DrawInProgress::Pencil(pts)) => {
            let screen: Vec<Pos2> = pts.iter().map(|&p| grid.to_screen(p)).collect();
            painter.add(egui::Shape::line(screen, Stroke::new(2.0_f32, DRAW_COLOUR)));
        }
        Some(DrawInProgress::Pen(anchors)) if !anchors.is_empty() => {
            let contour = NodeContour { nodes: anchors.clone(), closed: false }.to_contour();
            let pts: Vec<Pos2> = contour.flatten().into_iter().map(|p| grid.to_screen(p)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, DRAW_COLOUR)));
            // Rubber-band preview from the last anchor to the cursor.
            let closing = hover.filter(|&h| near_first_anchor(view, grid, h));
            if let Some(h) = hover {
                let last = anchors.last().unwrap();
                let to = if closing.is_some() { grid.to_screen(anchors[0].pt) } else { h };
                painter.line_segment([grid.to_screen(last.pt), to], Stroke::new(1.0_f32, SELECTION.gamma_multiply(0.7)));
            }
            for a in anchors {
                let c = grid.to_screen(a.pt);
                for h in [a.inn, a.out].into_iter().flatten() {
                    let h = grid.to_screen(h);
                    painter.line_segment([c, h], Stroke::new(1.0_f32, SELECTION));
                    paint_round_handle(painter, h);
                }
                paint_anchor(painter, c);
            }
            if closing.is_some() {
                let first = grid.to_screen(anchors[0].pt);
                painter.circle_stroke(first, HANDLE_SIZE_PX + 1.0, Stroke::new(HANDLE_STROKE_PX, ACCENT_PRIMARY));
            }
        }
        _ => {}
    }
}

/// The snap lines among the marks, for the ruler-guide painter to light a guide being snapped to.
fn snapped_lines(marks: &[snap::Mark]) -> Vec<Guide> {
    marks.iter().filter_map(|m| if let snap::Mark::Line(g) = m { Some(*g) } else { None }).collect()
}

/// Rounds to the pixel centre so a 1px line is crisp.
pub(super) fn snap_px(p: Pos2) -> Pos2 {
    pos2(p.x.floor() + 0.5, p.y.floor() + 0.5)
}

fn snap_rect(r: Rect) -> Rect {
    Rect::from_min_max(snap_px(r.min), snap_px(r.max))
}

/// A transform handle or path anchor: a 6 x 6 bg_panel square with a 1.5px accent stroke. Painted in
/// screen pixels, so it keeps its size at every zoom.
pub(super) fn paint_anchor(painter: &Painter, centre: Pos2) {
    let stroke = Stroke::new(HANDLE_STROKE_PX, ACCENT_PRIMARY);
    painter.rect(Rect::from_center_size(snap_px(centre), Vec2::splat(HANDLE_SIZE_PX)), 0.0, BG_PANEL, stroke, StrokeKind::Middle);
}

/// The rotate handle and Pen control handles: the same fill and stroke as an anchor, round to tell them apart.
pub(super) fn paint_round_handle(painter: &Painter, centre: Pos2) {
    painter.circle(snap_px(centre), HANDLE_SIZE_PX / 2.0 + 0.5, BG_PANEL, Stroke::new(HANDLE_STROKE_PX, ACCENT_PRIMARY));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(from: f32, zoom_in: bool, n: usize) -> Vec<u32> {
        std::iter::successors(Some(from), |&z| Some(next_zoom(z, zoom_in))).skip(1).take(n).map(|z| (z * 100.0).round() as u32).collect()
    }

    #[test]
    fn zoom_steps_match_design_space() {
        assert_eq!(walk(1.0, true, 10), [125, 150, 175, 200, 250, 300, 350, 400, 450, 500]);
        assert_eq!(walk(1.0, false, 7), [75, 50, 25, 20, 15, 10, 10]);
        assert_eq!(walk(7.5, true, 2), [800, 800]);
    }

    #[test]
    fn the_major_pitch_doubles_as_zoom_halves() {
        assert_eq!([1.0, 0.75, 0.25, 0.2, 0.1].map(crate::grid::multiplier), [1.0, 2.0, 4.0, 8.0, 16.0]);
    }

    #[test]
    fn the_artboard_is_a_centred_12_inch_sheet_that_follows_the_mat() {
        let viewport = Rect::from_min_size(pos2(48.0, 40.0), vec2(1300.0, 960.0));
        let mut view = View::default();
        view.clamp_pan(viewport.size());
        let art = view.artboard_rect(viewport);
        assert_eq!(art.size(), vec2(864.0, 864.0), "12 in at 100 %");
        assert!((art.center().x - (viewport.center().x + RULER / 2.0)).abs() < 0.01);
        view.artboard_in = (4.5, 6.5);
        assert_eq!(view.artboard_rect(viewport).size(), vec2(324.0, 468.0));
        view.zoom = 0.5;
        assert_eq!(view.artboard_rect(viewport).size(), vec2(162.0, 234.0));
    }

    #[test]
    fn panning_keeps_part_of_the_artboard_in_view() {
        let vp = vec2(1000.0, 800.0);
        let mut view = View { zoom: 4.0, ..View::default() };
        view.placed = true;
        view.pan_pt = vec2(1.0e6, -1.0e6);
        view.clamp_pan(vp);
        let art = view.artboard_rect(Rect::from_min_size(Pos2::ZERO, vp));
        assert_eq!((art.left(), art.bottom()), (vp.x - super::super::view_nav::KEEP_VISIBLE_PX, super::super::view_nav::KEEP_VISIBLE_PX));
    }

    #[test]
    fn a_three_click_pen_path_builds_an_open_curve() {
        // Two corner clicks then a smooth (click-drag) anchor.
        let mut smooth = Node::new((2.0, 1.0), NodeKind::Symmetric);
        smooth.move_handle(Side::Out, (2.5, 1.0));
        let nodes = vec![Node::new((0.0, 0.0), NodeKind::Straight), Node::new((1.0, 0.0), NodeKind::Straight), smooth];
        let open = NodeContour { nodes: nodes.clone(), closed: false }.to_contour();
        assert_eq!(open.segs.len(), 2);
        assert!(!open.closed);
        assert!(matches!(open.segs[0], crate::design::Seg::Line(_)));
        assert!(matches!(open.segs[1], crate::design::Seg::Cubic(..)));
        let mut d = crate::design::Design::default();
        let id = d.add_path(open, Color32::BLACK, Operation::Pen).unwrap();
        let path = crate::design::to_svg_path_d(&d.shape_mut(id).unwrap().contours());
        assert!(path.starts_with('M') && path.contains(" L") && path.contains(" C") && !path.ends_with('Z'), "{path}");

        // Closing adds a segment back to the first anchor and a Z.
        let closed = NodeContour { nodes, closed: true }.to_contour();
        assert_eq!(closed.segs.len(), 3);
        assert!(closed.closed);
    }

    #[test]
    fn a_marquee_skips_a_locked_layer_and_a_drag_skips_a_position_locked_shape() {
        let mut design = Design::default();
        let ids: Vec<u64> = (0..3).map(|_| design.add_shape(crate::design::ShapeKind::Square)).collect();
        design.shape_mut(ids[1]).unwrap().locked = true;
        design.set_snapping(false);
        let mut view = View { artboard_in: (12.0, 12.0), ..View::default() };
        let mut selection = Vec::new();
        view.drag = Some(Drag { grab: Grab::Marquee, start_in: (-1.0, -1.0), originals: Vec::new(), marquee_additive: false });
        continue_drag(&mut design, &mut selection, &mut view, (24.0, 24.0), false, Aabb { min_x: 0.0, min_y: 0.0, max_x: 12.0, max_y: 12.0 });
        assert_eq!(selection, [ids[0], ids[2]]);

        design.shape_mut(ids[0]).unwrap().lock_position = true;
        let grid = Grid { origin: Pos2::ZERO, scale: 72.0 };
        let (x, y) = design.shapes[0].centre_in();
        begin_drag(&design, &mut selection, &mut view, &grid, grid.to_screen((x, y)), false, false);
        let held: Vec<u64> = view.drag.iter().flat_map(|d| d.originals.iter().map(|(id, _)| *id)).collect();
        assert_eq!(held, [ids[2]], "the locked square is held but not moved");
        selection = vec![ids[0]];
        begin_drag(&design, &mut selection, &mut view, &grid, grid.to_screen((x, y)), false, false);
        assert!(view.drag.is_none(), "a drag on the locked square alone moves nothing");
        assert!(super::super::selection::grip_at(&design, &selection, &grid, grid.to_screen((x + 0.5, y + 0.5))).is_none(), "its handles are gone");
    }

    #[test]
    fn dragging_far_off_the_mat_clamps_to_one_board_of_void() {
        let mut design = Design::default();
        let id = design.add_shape(crate::design::ShapeKind::Square);
        let orig = design.shape_mut(id).unwrap().clone();
        design.set_snapping(false);
        let mut view = View { artboard_in: (12.0, 12.0), ..View::default() };
        let mut selection = vec![id];
        view.drag = Some(Drag { grab: Grab::Move, start_in: (orig.x_in, orig.y_in), originals: vec![(id, orig.clone())], marquee_additive: false });
        continue_drag(&mut design, &mut selection, &mut view, (-3.0, orig.y_in), false, Aabb { min_x: 0.0, min_y: 0.0, max_x: 12.0, max_y: 12.0 });
        assert_eq!(design.shape_mut(id).unwrap().x_in, -3.0, "a small drag parks the shape in the void");
        view.drag = Some(Drag { grab: Grab::Move, start_in: (orig.x_in, orig.y_in), originals: vec![(id, orig.clone())], marquee_additive: false });
        continue_drag(&mut design, &mut selection, &mut view, (-100.0, orig.y_in), false, Aabb { min_x: 0.0, min_y: 0.0, max_x: 12.0, max_y: 12.0 });
        assert_eq!(design.shape_mut(id).unwrap().x_in, -12.0, "a wild drag stops one board out");
    }
}

//! Dragging Layers rows to reorder and relink. This side turns the pointer into a `DropTarget` and draws the
//! feedback; `crate::layers_dnd` decides what a drop does.

use egui::{Align2, Color32, CursorIcon, Id, LayerId, Order, Pos2, Rect, Stroke, Ui, pos2, vec2};

use super::{INDENT, ROW_H, RowKey};
use crate::design::Design;
use crate::layers_dnd::{DropPlan, DropTarget, apply_drop, plan_drop};
use crate::ui::chrome::text;
use crate::ui::fonts;
use crate::ui::tokens::*;

/// How far a pressed row must move before it drags rather than clicks.
const START_DISTANCE: f32 = 4.0;
/// The band at the list's top and bottom edges that scrolls it, and the fastest scroll in points per second.
const SCROLL_BAND: f32 = 28.0;
const SCROLL_SPEED: f32 = 600.0;

pub(super) fn past_threshold(resp: &egui::Response) -> bool {
    resp.is_pointer_button_down_on()
        && resp.ctx.input(|i| i.pointer.primary_down() && i.pointer.press_origin().zip(i.pointer.latest_pos()).is_some_and(|(a, b)| a.distance(b) > START_DISTANCE))
}

#[derive(Clone)]
enum Gesture {
    Dragging {
        ids: Vec<u64>,
        label: String,
        dot: Option<Color32>,
    },
    /// Escape was pressed; the drag stays dead until the button comes up.
    Cancelled,
}

/// A visible row as the drop hit-test sees it: the id a `DropTarget` names it by (a shape id, or a link
/// id for a Group or Attach row), its nesting depth, and whether it takes drops inside it (a link or a
/// container). A parent is open when the next row is deeper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Slot {
    pub node: u64,
    pub depth: usize,
    pub parent: bool,
}

/// What the pointer is over: the drop it would make and the mark that shows it.
#[derive(Debug, PartialEq)]
struct Hover {
    target: DropTarget,
    mark: Mark,
}

#[derive(Debug, PartialEq)]
enum Mark {
    /// The insertion line in the gap above row `before`, indented to `depth`.
    Line { before: usize, depth: usize },
    /// The parent row at this index is highlighted.
    Into(usize),
}

/// Where the list's rows start, and the painter slot reserved beneath them for the drop-inside highlight.
pub(super) struct List {
    pub top: f32,
    pub backdrop: egui::layers::ShapeIdx,
}

/// Runs the drag for this frame: starts it from row `start`, draws the feedback, scrolls, and drops or
/// cancels. Returns true while a drag owns the pointer, so its release is not also a click.
/// `tops` is where each row starts; an effect list open under a row pushes the rows after it down.
#[expect(clippy::too_many_arguments, reason = "one frame of the list, read where the rows were painted")]
pub(super) fn update(
    ui: &mut Ui,
    design: &mut Design,
    selection: &mut Vec<u64>,
    rows: &[(RowKey, Vec<u64>)],
    slots: &[Slot],
    tops: &[f32],
    list: List,
    start: Option<usize>,
) -> bool {
    let id = ui.id().with("layers-drag");
    let mut gesture = ui.data(|d| d.get_temp::<Gesture>(id));
    if let (None, Some(at)) = (&gesture, start) {
        gesture = Some(begin(design, selection, &rows[at]));
    }
    let Some(gesture) = gesture else {
        return false;
    };
    let (released, down, pointer) = ui.input(|i| (i.pointer.primary_released(), i.pointer.primary_down(), i.pointer.latest_pos()));
    let Gesture::Dragging { ids, label, dot } = &gesture else {
        if !down {
            ui.data_mut(|d| d.remove::<Gesture>(id));
        }
        return true;
    };
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ui.data_mut(|d| d.insert_temp(id, Gesture::Cancelled));
        return true;
    }
    let pitch = ROW_H + ui.spacing().item_spacing.y;
    let view = ui.clip_rect();
    let over = pointer.filter(|p| view.contains(*p)).map(|p| {
        let hover = hover(slots, list.top, pitch, evenly(tops, list.top, pitch, p.y));
        let plan = plan_drop(design, ids, hover.target);
        (hover, plan)
    });
    if released || !down {
        if let Some((_, plan)) = over.filter(|(_, plan)| released && *plan != DropPlan::Invalid) {
            apply_drop(design, &plan);
            if ids.iter().any(|id| !selection.contains(id)) {
                *selection = ids.clone();
            }
        }
        ui.data_mut(|d| d.remove::<Gesture>(id));
        return true;
    }
    match &over {
        Some((_, DropPlan::Invalid)) => ui.ctx().set_cursor_icon(CursorIcon::NotAllowed),
        _ => ui.ctx().set_cursor_icon(CursorIcon::Grabbing),
    }
    if let Some((hover, plan)) = &over
        && *plan != DropPlan::Invalid
    {
        paint_mark(ui, &list, tops, pitch, &hover.mark);
    }
    if let Some(p) = pointer {
        auto_scroll(ui, Rect::from_x_y_ranges(view.x_range(), list.top..=ui.min_rect().bottom()), p);
        paint_ghost(ui, p, label, *dot, ids.len());
    }
    ui.data_mut(|d| d.insert_temp(id, gesture.clone()));
    true
}

/// Dragging a selected row drags the whole selection; an unselected row drags alone and is selected when
/// it drops. Selecting it at once would open Properties and shift the list under the pointer.
fn begin(design: &Design, selection: &[u64], (key, row): &(RowKey, Vec<u64>)) -> Gesture {
    let ids = if row.iter().all(|id| selection.contains(id)) { selection.to_vec() } else { row.clone() };
    let shape = |id: u64| design.shapes.iter().find(|s| s.id == id);
    let (label, dot) = match *key {
        RowKey::Shape(id) => (shape(id).map(|s| design.layer_name(s)).unwrap_or_default(), shape(id).map(|s| s.fill)),
        RowKey::Part(_) => (row.first().and_then(|&i| shape(i)).map(|s| design.layer_name(s)).unwrap_or_default(), row.first().and_then(|&i| shape(i)).map(|s| s.fill)),
        RowKey::Link(_) => (row.first().and_then(|&m| shape(m)?.link).map(|l| design.link_name(l)).unwrap_or_default(), None),
    };
    Gesture::Dragging { ids, label, dot }
}

/// The first of `rows` at `depth`, unless a shallower row, which ends that level, comes first.
fn sibling<'a>(mut rows: impl Iterator<Item = &'a Slot>, depth: usize) -> Option<u64> {
    rows.find(|s| s.depth <= depth).filter(|s| s.depth == depth).map(|s| s.node)
}

/// The drop under the pointer at height `y`, for rows starting at `top` every `pitch` points. A row's top
/// half drops above it and its bottom half below it, at that row's level. A parent row splits in thirds:
/// above it, into it, and below it, which for an open parent is the top of its children.
fn hover(slots: &[Slot], top: f32, pitch: f32, y: f32) -> Hover {
    let up = |i: usize, depth: usize| sibling(slots[..i].iter().rev(), depth);
    let down = |i: usize, depth: usize| sibling(slots[i + 1..].iter(), depth);
    let line =
        |before: usize, depth: usize, above: Option<u64>, below: Option<u64>| Hover { target: DropTarget::Between { above, below }, mark: Mark::Line { before, depth } };

    let at = (y - top) / pitch;
    if at >= slots.len() as f32 {
        return line(slots.len(), 0, up(slots.len(), 0), None);
    }
    let i = at.max(0.0) as usize;
    let f = if at < 0.0 { 0.0 } else { at.fract() };
    let Slot { node, depth, parent } = slots[i];
    let first_child = slots.get(i + 1).filter(|next| next.depth > depth).map(|next| next.node);
    let upper = if parent { 1.0 / 3.0 } else { 0.5 };
    match first_child {
        _ if f < upper => line(i, depth, up(i, depth), Some(node)),
        _ if parent && f < 2.0 / 3.0 => Hover { target: DropTarget::Into(node), mark: Mark::Into(i) },
        Some(child) => line(i + 1, depth + 1, None, Some(child)),
        None => line(i + 1, depth, Some(node), down(i, depth)),
    }
}

/// `y` moved onto rows laid every `pitch` from `top`, so the space an open effect list takes under a row
/// counts as the bottom of that row.
fn evenly(tops: &[f32], top: f32, pitch: f32, y: f32) -> f32 {
    let Some(i) = tops.iter().rposition(|&t| t <= y) else { return y - tops.first().map_or(0.0, |t| t - top) };
    top + (i as f32 + ((y - tops[i]) / pitch).min(0.99)) * pitch + if i + 1 == tops.len() && y > tops[i] + pitch { pitch } else { 0.0 }
}

/// Where row `i` starts on screen; one past the last row is just below it.
fn row_top(tops: &[f32], top: f32, pitch: f32, i: usize) -> f32 {
    tops.get(i).copied().unwrap_or_else(|| tops.last().map_or(top, |t| t + pitch))
}

fn paint_mark(ui: &Ui, list: &List, tops: &[f32], pitch: f32, mark: &Mark) {
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    match *mark {
        Mark::Line { before, depth } => {
            let y = row_top(tops, list.top, pitch, before) - (pitch - ROW_H) / 2.0;
            let x = left + depth as f32 * INDENT + 4.0;
            let stroke = Stroke::new(2.0_f32, ACCENT_PRIMARY);
            ui.painter().circle(pos2(x, y), 3.0, BG_PANEL, stroke);
            ui.painter().hline(x + 3.0..=right - 2.0, y, stroke);
        }
        Mark::Into(i) => {
            let y = row_top(tops, list.top, pitch, i);
            let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW_H));
            ui.painter().set(list.backdrop, egui::epaint::RectShape::filled(row, 4.0, ACCENT_HOVER));
            ui.painter().rect_stroke(row, 4.0, Stroke::new(1.0_f32, ACCENT_PRIMARY), egui::StrokeKind::Inside);
        }
    }
}

/// Near the list's top or bottom edge the list scrolls towards it, faster the closer the pointer is, while
/// `content` reaches past that edge.
fn auto_scroll(ui: &mut Ui, content: Rect, p: Pos2) {
    let view = ui.clip_rect();
    if !view.x_range().contains(p.x) {
        return;
    }
    let depth = |edge: f32, more: bool| if more { (1.0 - (p.y - edge).abs() / SCROLL_BAND).clamp(0.0, 1.0) } else { 0.0 };
    let dir = depth(view.top(), content.top() < view.top()) - depth(view.bottom(), content.bottom() > view.bottom());
    if dir != 0.0 {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        ui.scroll_with_delta(vec2(0.0, dir * SCROLL_SPEED * dt));
        ui.ctx().request_repaint();
    }
}

/// A translucent copy of the grabbed row just below the pointer, kept on screen, with a count badge for more than one object.
fn paint_ghost(ui: &Ui, p: Pos2, label: &str, dot: Option<Color32>, count: usize) {
    let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::new("layers-drag-ghost")));
    let width = ui.max_rect().width() * 0.75;
    let x = (p.x + 10.0).min(ui.ctx().screen_rect().right() - width - 4.0);
    let rect = Rect::from_min_size(pos2(x, p.y + 6.0), vec2(width, ROW_H));
    let fade = |c: Color32| c.gamma_multiply(0.92);
    painter.rect(rect, 4.0, fade(BG_PANEL), Stroke::new(1.0_f32, fade(ACCENT_PRIMARY)), egui::StrokeKind::Inside);
    let mut x = rect.left() + 12.0;
    if let Some(fill) = dot {
        painter.circle(pos2(x + 2.0, rect.center().y), 5.0, fade(fill), Stroke::new(1.0_f32, BORDER_SUBTLE));
        x += 16.0;
    }
    let room = if count > 1 { rect.right() - 28.0 } else { rect.right() - 8.0 } - x;
    super::super::names::fitted(&painter, pos2(x, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(13.0), fade(TEXT_PRIMARY), room);
    if count > 1 {
        let centre = pos2(rect.right() - 14.0, rect.center().y);
        painter.circle_filled(centre, 9.0, ACCENT_PRIMARY);
        text(&painter, centre, Align2::CENTER_CENTER, &count.to_string(), fonts::semibold(11.0), ON_PRIMARY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: f32 = 100.0;
    const PITCH: f32 = 30.0;

    fn leaf(node: u64, depth: usize) -> Slot {
        Slot { node, depth, parent: false }
    }

    fn parent(node: u64, depth: usize) -> Slot {
        Slot { node, depth, parent: true }
    }

    /// Listed top first: loose 9, an open Group 20 of members 3 and 2, then loose 1.
    fn rows() -> Vec<Slot> {
        vec![leaf(9, 0), parent(20, 0), leaf(3, 1), leaf(2, 1), leaf(1, 0)]
    }

    fn at(row: f32) -> Hover {
        hover(&rows(), TOP, PITCH, TOP + row * PITCH)
    }

    fn between(above: Option<u64>, below: Option<u64>) -> DropTarget {
        DropTarget::Between { above, below }
    }

    #[test]
    fn a_loose_rows_halves_drop_above_and_below_it() {
        assert_eq!(at(0.2), Hover { target: between(None, Some(9)), mark: Mark::Line { before: 0, depth: 0 } });
        assert_eq!(at(0.8), Hover { target: between(Some(9), Some(20)), mark: Mark::Line { before: 1, depth: 0 } });
        assert_eq!(at(4.2).target, between(Some(20), Some(1)), "a top-level neighbour above a link is the link");
    }

    #[test]
    fn a_parent_rows_thirds_drop_above_into_and_at_the_top_of_its_members() {
        assert_eq!(at(1.2).target, between(Some(9), Some(20)));
        assert_eq!(at(1.5), Hover { target: DropTarget::Into(20), mark: Mark::Into(1) });
        assert_eq!(at(1.8), Hover { target: between(None, Some(3)), mark: Mark::Line { before: 2, depth: 1 } });
    }

    #[test]
    fn below_the_last_member_stays_inside_the_link() {
        assert_eq!(at(3.8), Hover { target: between(Some(2), None), mark: Mark::Line { before: 4, depth: 1 } });
        assert_eq!(at(2.8).target, between(Some(3), Some(2)));
    }

    #[test]
    fn a_closed_parents_bottom_third_drops_after_the_link() {
        let slots = [parent(20, 0), leaf(1, 0)];
        assert_eq!(hover(&slots, TOP, PITCH, TOP + 0.9 * PITCH).target, between(Some(20), Some(1)));
    }

    #[test]
    fn nested_levels_pair_rows_with_their_own_siblings() {
        // Container 5 holds container 6 (holding 7) and then 8; loose 1 is below at the top level.
        let slots = [parent(5, 0), parent(6, 1), leaf(7, 2), leaf(8, 1), leaf(1, 0)];
        let at = |row: f32| hover(&slots, TOP, PITCH, TOP + row * PITCH);
        assert_eq!(at(1.5), Hover { target: DropTarget::Into(6), mark: Mark::Into(1) }, "an inner container takes drops");
        assert_eq!(at(2.8), Hover { target: between(Some(7), None), mark: Mark::Line { before: 3, depth: 2 } }, "after the last of its children");
        assert_eq!(at(3.2).target, between(Some(6), Some(8)), "the inner container is 8's sibling");
        assert_eq!(at(3.8), Hover { target: between(Some(8), None), mark: Mark::Line { before: 4, depth: 1 } });
        assert_eq!(at(4.2).target, between(Some(5), Some(1)), "back at the top level");
        assert_eq!(hover(&[parent(5, 0), leaf(1, 0)], TOP, PITCH, TOP + 0.9 * PITCH).target, between(Some(5), Some(1)), "an empty container");
    }

    #[test]
    fn an_open_effect_list_counts_as_the_bottom_of_its_row() {
        let tops = [TOP, TOP + PITCH, TOP + 4.0 * PITCH];
        let at = |row: f32| (evenly(&tops, TOP, PITCH, TOP + row * PITCH) - TOP) / PITCH;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(at(0.4), 0.4), "rows before the list are unmoved");
        assert!(near(at(3.0), 1.99), "the list under row 1 is that row's bottom");
        assert!(near(at(4.25), 2.25), "the row after it");
        assert!(near(at(6.0), 3.99), "past the end");
    }

    #[test]
    fn past_either_end_drops_at_that_end() {
        assert_eq!(at(-1.0).target, between(None, Some(9)));
        assert_eq!(at(7.0), Hover { target: between(Some(1), None), mark: Mark::Line { before: 5, depth: 0 } });
    }
}

//! Names in Layers rows and Components tiles (spec 40): fitted to the room the row leaves them, ending in
//! an ellipsis when cut short, shown whole in a tooltip then, and typed over in place to rename.

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::text_edit::TextEditState;
use egui::{Align, Align2, Color32, FontId, Key, Modifiers, Painter, Pos2, Rect, Response, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, vec2};

use super::tooltip::tip;

/// The rename field's height.
const FIELD_H: f32 = 20.0;

/// `name` on one line at `pos` by `anchor`, no wider than `max_width`, ending in "…" when it does not fit.
/// Returns where it was painted and whether it was cut short.
pub(super) fn fitted(painter: &Painter, pos: Pos2, anchor: Align2, name: &str, font: FontId, ink: Color32, max_width: f32) -> (Rect, bool) {
    // egui lays out at least one glyph whatever the width, which would spill over the next control.
    if max_width < font.size {
        return (anchor.anchor_size(pos, Vec2::ZERO), !name.is_empty());
    }
    let mut job = LayoutJob::single_section(name.to_owned(), TextFormat::simple(font, ink));
    job.wrap = TextWrapping { max_width: max_width.max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let cut = galley.elided;
    let rect = anchor.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, ink);
    (rect, cut)
}

/// `name` at the left of `slot`, centred on its height and fitted to its width. While the pointer rests
/// on a name cut short, the row `row` shows it whole.
pub(super) fn row_name(ui: &Ui, row: &Response, slot: Rect, name: &str, font: FontId, ink: Color32) {
    let (rect, cut) = fitted(ui.painter(), slot.left_center(), Align2::LEFT_CENTER, name, font, ink, slot.width());
    if cut && ui.rect_contains_pointer(rect) {
        let mut on_name = row.clone();
        on_name.rect = rect;
        tip(on_name, name, "", None);
    }
}

/// The rename field over `slot`, focused with its text selected as it opens, so typing replaces the name.
/// `Some(Some(text))` once Enter or a click away commits it, `Some(None)` on Escape. It takes the Enter or
/// Escape that closes it, so nothing behind it acts on the same key.
pub(super) fn field(ui: &mut Ui, slot: Rect, text: &mut String, font: FontId, align: Align, label: &str) -> Option<Option<String>> {
    let edit = egui::TextEdit::singleline(text).font(font).horizontal_align(align).desired_width(slot.width()).margin(vec2(4.0, 2.0));
    // A child over the slot, so the field leaves the list's layout as it was.
    let mut over = ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(slot.center(), vec2(slot.width(), FIELD_H))));
    let resp = over.add(edit);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
    if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
        if let Some(mut state) = TextEditState::load(ui.ctx(), resp.id) {
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()))));
            state.store(ui.ctx(), resp.id);
        }
    }
    if !resp.lost_focus() {
        return None;
    }
    let cancelled = ui.input_mut(|i| {
        i.consume_key(Modifiers::NONE, Key::Enter);
        i.consume_key(Modifiers::NONE, Key::Escape)
    });
    Some((!cancelled).then(|| text.trim().to_owned()))
}

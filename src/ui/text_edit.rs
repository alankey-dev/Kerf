//! Editing a text's words in place: a multi-line editor over the text box, in the UI face nearest the
//! text's font at the zoomed size. Each keystroke rewrites the shape, so the canvas outlines follow
//! the typing. Escape or a click outside ends the session, and the app keeps the whole session as one
//! undo step.

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Align, Color32, Rect, Ui, WidgetInfo, WidgetType};

use crate::design::{Design, ShapeKind};
use crate::text::TextAlign;
use crate::ui::canvas::Grid;
use crate::ui::fonts;
use crate::ui::tokens::TEXT_PRIMARY;

/// White at 90 % behind the words being edited.
const BACKDROP: Color32 = Color32::from_rgba_premultiplied(230, 230, 230, 230);

pub struct Session {
    pub id: u64,
    /// Where the caret goes when the editor first takes focus; `None` once it has.
    entry: Option<Caret>,
}

#[derive(Clone, Copy)]
pub enum Caret {
    End,
    SelectAll,
}

impl Session {
    pub fn new(id: u64, caret: Caret) -> Self {
        Self { id, entry: Some(caret) }
    }
}

/// Whether `id` is a text, whose words edit in place.
pub fn editable(design: &Design, id: u64) -> bool {
    design.shapes.iter().any(|s| s.id == id && matches!(s.kind, ShapeKind::Text { .. }))
}

/// Shows the editor over text `session.id`. Returns false when the session has ended; a text left
/// empty is removed then.
pub fn show(ui: &Ui, grid: &Grid, design: &mut Design, session: &mut Session) -> bool {
    let Some(shape) = design.shapes.iter().find(|s| s.id == session.id) else { return false };
    let ShapeKind::Text { content, font, size_in, align, wrap_in, .. } = &shape.kind else { return false };
    let corners = [(0.0, 0.0), (shape.width_in, 0.0), (0.0, shape.height_in), (shape.width_in, shape.height_in)];
    let rect = Rect::from_points(&corners.map(|p| grid.to_screen(shape.to_world(p))));
    let natural_h = shape.kind.default_size_in().1;
    let px = size_in * shape.height_in / natural_h * grid.scale;
    let face = fonts::typeface(ui.ctx(), font, px.max(4.0));
    let halign = match align {
        TextAlign::Left => Align::LEFT,
        TextAlign::Centre => Align::Center,
        TextAlign::Right => Align::RIGHT,
    };
    let mut buffer = content.clone();
    let edit_id = ui.id().with(("text-edit", session.id));
    // Unwrapped text grows as it is typed, so its lines never wrap at the box's last width.
    let wrap_px = if wrap_in.is_some() { rect.width() } else { f32::INFINITY };
    let mut layouter = |ui: &Ui, text: &str, _: f32| {
        let mut job = LayoutJob::simple(text.to_owned(), face.clone(), TEXT_PRIMARY, wrap_px);
        job.halign = halign;
        ui.fonts(|f| f.layout_job(job))
    };

    let area = egui::Area::new(edit_id.with("area")).fixed_pos(rect.min).order(egui::Order::Foreground).constrain(false);
    let output = area
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE.fill(BACKDROP).show(ui, |ui| {
                egui::TextEdit::multiline(&mut buffer)
                    .id(edit_id)
                    .frame(false)
                    .margin(egui::Margin::ZERO)
                    .layouter(&mut layouter)
                    .horizontal_align(halign)
                    .desired_width(0.0)
                    .desired_rows(1)
                    .min_size(rect.size())
                    .show(ui)
            })
        })
        .inner
        .inner;
    let resp = &output.response;
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Text content"));

    let entering = session.entry.is_some();
    if let Some(caret) = session.entry.take() {
        resp.request_focus();
        let end = CCursor::new(buffer.chars().count());
        let start = match caret {
            Caret::End => end,
            Caret::SelectAll => CCursor::new(0),
        };
        let mut state = output.state;
        state.cursor.set_char_range(Some(CCursorRange::two(start, end)));
        state.store(ui.ctx(), edit_id);
    }
    if resp.changed()
        && let Some(s) = design.shape_mut(session.id)
    {
        let before = s.kind.clone();
        if let ShapeKind::Text { content, .. } = &mut s.kind {
            content.clone_from(&buffer);
        }
        design.refit_text(session.id, &before);
    }

    let ended = !entering && (resp.lost_focus() || resp.clicked_elsewhere() || ui.input(|i| i.key_pressed(egui::Key::Escape)));
    if ended && buffer.trim().is_empty() {
        design.remove_many(&[session.id]);
    }
    !ended
}

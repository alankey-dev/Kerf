//! The Make flow's widgets, painted from `tokens` so every flow screen wears the canvas identity.

use egui::emath::GuiRounding as _;
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, Visuals, WidgetInfo, WidgetType,
    pos2, vec2,
};

use super::canvas::Grid;
use super::chrome::{text, text_width};
use super::fonts;
use super::icons::{Icon, paint_icon, paint_icon_rotated};
use super::popover::{popover, text_row};
use super::tokens::*;

pub use super::inspector::section_title as section_header;

const ROW_H: f32 = 32.0;
const CARD_RADIUS: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    /// The canvas Preview look: a white pill with an accent ring.
    Accent,
    Secondary,
}

/// The canvas header pill: `BUTTON_H` tall, fully rounded, medium 14, at least `BUTTON_MIN_W` wide.
pub fn button(ui: &mut Ui, label: &str, kind: Kind, enabled: bool) -> Response {
    let enabled = enabled && ui.is_enabled();
    let font = fonts::medium(14.0);
    let w = (text_width(ui, label, font.clone()) + 48.0).max(BUTTON_MIN_W);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, BUTTON_H), sense);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let hovered = enabled && resp.hovered();
    let (fill, ring, ink) = match (kind, enabled, hovered) {
        (Kind::Primary, true, false) => (ACCENT_PRIMARY, None, ON_PRIMARY),
        (Kind::Primary, true, true) => (PRIMARY_HOVER, None, ON_PRIMARY),
        (Kind::Primary, false, _) => (BG_CANVAS_VOID, None, DISABLED_TEXT),
        (Kind::Accent, true, false) => (BG_PANEL, Some(ACCENT_PRIMARY), ACCENT_PRIMARY),
        (Kind::Accent, true, true) => (ACCENT_PRIMARY, None, ON_PRIMARY),
        (Kind::Secondary, true, false) => (BG_PANEL, Some(BORDER_SUBTLE), TEXT_PRIMARY),
        (Kind::Secondary, true, true) => (ACCENT_HOVER, Some(BORDER_SUBTLE), TEXT_PRIMARY),
        (Kind::Accent | Kind::Secondary, false, _) => (Color32::TRANSPARENT, Some(BORDER_SUBTLE), DISABLED_TEXT),
    };
    let radius = (BUTTON_H / 2.0) as u8;
    ui.painter().rect_filled(rect, radius, fill);
    if let Some(ring) = ring {
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0_f32, ring), StrokeKind::Inside);
    }
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, font, ink);
    resp
}

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Kind::Primary, enabled)
}

pub fn secondary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    button(ui, label, Kind::Secondary, enabled)
}

/// A bordered track of segments; the selected one takes the accent tint. Returns a newly picked value.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: T, options: &[(T, &str)]) -> Option<T> {
    let enabled = ui.is_enabled();
    let font = fonts::medium(13.0);
    let widths: Vec<f32> = options.iter().map(|(_, label)| text_width(ui, label, font.clone()) + 28.0).collect();
    let (track, base) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 4.0, BUTTON_H), Sense::hover());
    ui.painter().rect(track, 6, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let mut x = track.left() + 2.0;
    let mut picked = None;
    for (i, (&(value, label), w)) in options.iter().zip(widths).enumerate() {
        let seg = Rect::from_min_size(pos2(x, track.top() + 2.0), vec2(w, track.height() - 4.0));
        x += w;
        let resp = ui.interact(seg, base.id.with(i), sense);
        let selected = value == current;
        resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, selected, label));
        let hovered = enabled && resp.hovered();
        if selected || hovered {
            ui.painter().rect_filled(seg, 4, ACCENT_HOVER);
        }
        let ink = match (enabled, selected) {
            (false, _) => DISABLED_TEXT,
            (true, true) => ACCENT_PRIMARY,
            (true, false) => TEXT_PRIMARY,
        };
        text(ui.painter(), seg.center(), Align2::CENTER_CENTER, label, font.clone(), ink);
        if resp.clicked() && !selected {
            picked = Some(value);
        }
    }
    picked
}

/// A full-width row that senses clicks only while the ui is enabled.
fn row(ui: &mut Ui, height: f32) -> (Rect, Response, bool) {
    let enabled = ui.is_enabled();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    (rect, resp, enabled)
}

pub fn radio_row(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let (rect, resp, enabled) = row(ui, ROW_H);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, selected, label));
    let painter = ui.painter();
    if enabled && resp.hovered() {
        painter.rect_filled(rect, 4, ACCENT_HOVER);
    }
    let dot = pos2(rect.left() + 16.0, rect.center().y);
    if selected {
        painter.circle_stroke(dot, 7.0, Stroke::new(1.5_f32, ACCENT_PRIMARY));
        painter.circle_filled(dot, 3.5, ACCENT_PRIMARY);
    } else {
        painter.circle(dot, 7.0, BG_PANEL, Stroke::new(1.0_f32, TEXT_MUTED));
    }
    text(painter, pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(13.0), TEXT_PRIMARY);
    resp
}

/// The label on the left and a small accent switch on the right. Flips `on` when clicked.
pub fn toggle_row(ui: &mut Ui, label: &str, on: &mut bool) -> Response {
    let (rect, mut resp, enabled) = row(ui, ROW_H);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, value, label));
    let painter = ui.painter();
    text(painter, pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(13.0), TEXT_PRIMARY);
    let track = Rect::from_min_size(pos2(rect.right() - 32.0, rect.center().y - 9.0), vec2(32.0, 18.0));
    let how_on = ui.ctx().animate_bool(resp.id, value);
    painter.rect_filled(track, 9, if value { ACCENT_PRIMARY } else { MAT_GREY });
    let knob_x = egui::lerp(track.left() + 9.0..=track.right() - 9.0, how_on);
    painter.circle_filled(pos2(knob_x, track.center().y), 7.0, BG_PANEL);
    resp
}

/// The toolbar's operation dropdown look; the menu is a popover of text rows.
pub fn dropdown<T: Copy + PartialEq>(ui: &mut Ui, name: &str, current: T, options: &[(T, &str)]) -> Option<T> {
    let current_label = options.iter().find(|(v, _)| *v == current).map_or("", |(_, l)| *l);
    let (rect, resp, enabled) = row(ui, ROW_H);
    resp.widget_info(|| WidgetInfo { current_text_value: Some(current_label.into()), ..WidgetInfo::labeled(WidgetType::ComboBox, enabled, name) });
    let open = ui.memory(|m| m.is_popup_open(resp.id.with("menu")));
    let painter = ui.painter();
    painter.rect_filled(rect, 4, if enabled && resp.hovered() { ACCENT_HOVER } else { BG_PANEL });
    let edge = if open { ACCENT_PRIMARY } else { BORDER_SUBTLE };
    painter.rect_stroke(rect, 4, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    text(painter, pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, current_label, fonts::regular(13.0), TEXT_PRIMARY);
    paint_icon(ui, Icon::Chevron, Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(20.0)), TEXT_PRIMARY);
    popover(ui, &resp, "menu", rect.width() - 16.0, |ui| options.iter().find_map(|&(value, label)| text_row(ui, label).clicked().then_some(value)))
        .filter(|&v| v != current)
}

/// A Layers row: 32 px, accent tint when selected or hovered. `detail` follows `label` in muted ink and
/// joins it, two spaces apart, in the accessible name.
pub fn select_row(ui: &mut Ui, selected: bool, label: &str, detail: &str) -> Response {
    let (rect, resp, enabled) = row(ui, ROW_H);
    let name = if detail.is_empty() { label.to_owned() } else { format!("{label}  {detail}") };
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, selected, &name));
    let painter = ui.painter();
    if selected || (enabled && resp.hovered()) {
        painter.rect_filled(rect, 4, ACCENT_HOVER);
    }
    let ink = if selected { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    let font = if selected { fonts::medium(13.0) } else { fonts::regular(13.0) };
    let end = text(painter, pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, font, ink).right();
    if !detail.is_empty() {
        text(painter, pos2(end + 8.0, rect.center().y), Align2::LEFT_CENTER, detail, fonts::regular(13.0), TEXT_MUTED);
    }
    resp
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, BORDER_SUBTLE));
    ui.add_space(8.0);
}

/// A chevron and title that open and close `body`; the open state lives in the ui's memory. A disabled
/// disclosure shows shut, so a finished step folds away and the current one stays in view.
pub fn disclosure(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    let (rect, resp, enabled) = row(ui, ROW_H);
    let id = resp.id.with("open");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let open = open && enabled;
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::CollapsingHeader, enabled, title));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4, ACCENT_HOVER);
    }
    let turn = if open { 0.0 } else { -std::f32::consts::FRAC_PI_2 };
    paint_icon_rotated(ui, Icon::Chevron, Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(16.0)), TEXT_PRIMARY, turn);
    text(ui.painter(), pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, title, fonts::medium(13.0), TEXT_PRIMARY);
    if open {
        ui.add_space(4.0);
        Frame::NONE.inner_margin(Margin { left: 28, right: 0, top: 0, bottom: 4 }).show(ui, body);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepState {
    Done,
    Current,
    Upcoming,
}

impl StepState {
    pub fn of(number: u8, current: u8) -> Self {
        match number.cmp(&current) {
            std::cmp::Ordering::Less => Self::Done,
            std::cmp::Ordering::Equal => Self::Current,
            std::cmp::Ordering::Greater => Self::Upcoming,
        }
    }
}

/// The white card every flow step sits in.
pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let frame = Frame::NONE.fill(BG_PANEL).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).corner_radius(CARD_RADIUS).inner_margin(Margin::same(20));
    frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            body(ui)
        })
        .inner
}

/// A numbered step. Only the current step's body takes input; the others stay visible, dimmed.
pub fn step_card(ui: &mut Ui, number: u8, title: &str, state: StepState, body: impl FnOnce(&mut Ui)) {
    card(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let centre = pos2(rect.left() + 12.0, rect.center().y);
        let painter = ui.painter();
        let digit = number.to_string();
        match state {
            StepState::Current => {
                painter.circle_filled(centre, 12.0, ACCENT_PRIMARY);
                text(painter, centre, Align2::CENTER_CENTER, &digit, fonts::semibold(13.0), ON_PRIMARY);
            }
            StepState::Done => {
                painter.circle(centre, 11.5, ACCENT_HOVER, Stroke::new(1.0_f32, ACCENT_PRIMARY));
                paint_icon(ui, Icon::Tick, Rect::from_center_size(centre, Vec2::splat(16.0)), ACCENT_PRIMARY);
            }
            StepState::Upcoming => {
                painter.circle_stroke(centre, 11.5, Stroke::new(1.0_f32, TEXT_MUTED));
                text(painter, centre, Align2::CENTER_CENTER, &digit, fonts::semibold(13.0), TEXT_MUTED);
            }
        }
        let ink = if state == StepState::Upcoming { TEXT_MUTED } else { TEXT_PRIMARY };
        let title_pos = pos2(rect.left() + 36.0, rect.center().y);
        let title_rect = text(ui.painter(), title_pos, Align2::LEFT_CENTER, title, fonts::semibold(14.0), ink);
        ui.interact(title_rect, ui.id().with(("step", number)), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, title));
        ui.add_space(4.0);
        Frame::NONE.inner_margin(Margin { left: 36, right: 0, top: 0, bottom: 0 }).show(ui, |ui| {
            ui.add_enabled_ui(state == StepState::Current, body);
        });
    });
}

/// A heading inside a card: semibold 16.
pub fn heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(fonts::semibold(16.0)).color(TEXT_PRIMARY));
}

/// Body copy: regular 13, primary or muted.
pub fn body_text(ui: &mut Ui, words: impl Into<String>, muted: bool) -> Response {
    ui.label(RichText::new(words).font(fonts::regular(13.0)).color(if muted { TEXT_MUTED } else { TEXT_PRIMARY }))
}

/// A checklist line: an accent check disc when done, a muted ring when not.
pub fn check_item(ui: &mut Ui, done: bool, words: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        if done {
            ui.painter().circle_filled(rect.center(), 9.0, ACCENT_PRIMARY);
            paint_icon(ui, Icon::Tick, rect.shrink(3.0), ON_PRIMARY);
        } else {
            ui.painter().circle_stroke(rect.center(), 8.5, Stroke::new(1.0_f32, TEXT_MUTED));
        }
        body_text(ui, words, false);
    });
}

#[derive(Clone, Copy)]
pub enum Notice {
    Warning,
}

pub fn notice(ui: &mut Ui, kind: Notice, words: &str) {
    let (fill, ink) = match kind {
        Notice::Warning => (WARNING_BG, WARNING_TEXT),
    };
    Frame::NONE.fill(fill).corner_radius(6).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(words).font(fonts::medium(13.0)).color(ink));
    });
}

/// An 8 px accent bar on a hairline track, with the percentage on the right.
pub fn progress(ui: &mut Ui, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let percent = format!("{}%", (fraction * 100.0).round());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, &percent));
    let track = Rect::from_min_max(pos2(rect.left(), rect.center().y - 4.0), pos2(rect.right() - 52.0, rect.center().y + 4.0));
    let painter = ui.painter();
    painter.rect_filled(track, 4, BORDER_SUBTLE);
    if fraction > 0.0 {
        painter.rect_filled(track.with_max_x(track.left() + track.width() * fraction), 4, ACCENT_PRIMARY);
    }
    text(painter, pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, &percent, fonts::medium(13.0), TEXT_PRIMARY);
}

/// The inspector's text field look: a 1px hairline box around a frameless single line.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str) -> Response {
    let field = Frame::NONE.fill(BG_PANEL).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).corner_radius(4).inner_margin(Margin::symmetric(8, 6));
    field
        .show(ui, |ui| {
            let hint = RichText::new(hint).font(fonts::regular(13.0)).color(TEXT_MUTED);
            ui.add(egui::TextEdit::singleline(value).frame(false).hint_text(hint).desired_width(f32::INFINITY).font(fonts::regular(13.0)).text_color(TEXT_PRIMARY))
        })
        .inner
}

/// A floating card for the machine modal: white, radius 8, hairline, soft shadow.
pub fn card_frame() -> Frame {
    Frame::NONE
        .fill(BG_PANEL)
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .stroke(Stroke::new(1.0_f32, BORDER_SUBTLE))
        .inner_margin(Margin::same(24))
        .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: SHADOW })
}

/// A panel title: semibold 14, as the Project Preview heads its panel.
pub fn panel_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
    ui.add_space(8.0);
}

/// The canvas artboard recipe over `rect`: a shadowed white sheet with a quarter-inch and inch grid.
pub fn mat_sheet(ui: &Ui, rect: Rect, width_in: f32, height_in: f32) -> Grid {
    let grid = Grid::fit(rect, width_in, height_in);
    let sheet = Rect::from_min_size(grid.origin, vec2(width_in, height_in) * grid.scale).round_to_pixels(ui.pixels_per_point());
    let painter = ui.painter();
    let shadow = egui::Shadow { offset: [0, 4], blur: 24, spread: 0, color: ARTBOARD_SHADOW };
    painter.add(shadow.as_shape(sheet, 2.0));
    painter.rect_filled(sheet, 2, BG_ARTBOARD);
    for (step, colour) in [(0.25, GRID_MINOR), (1.0, GRID_MAJOR)] {
        let stroke = Stroke::new(1.0_f32, colour);
        let lines = |extent: f32| (1..).map(move |i| i as f32 * step).take_while(move |&at| at < extent - 1e-3);
        for x in lines(width_in) {
            painter.vline(grid.to_screen((x, 0.0)).x.round() + 0.5, sheet.y_range(), stroke);
        }
        for y in lines(height_in) {
            painter.hline(sheet.x_range(), grid.to_screen((0.0, y)).y.round() + 0.5, stroke);
        }
    }
    painter.rect_stroke(sheet, 2, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    grid
}

/// Right-aligns `add` on its own row, for footers and modal actions.
pub fn actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), BUTTON_H), Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

/// A row of controls, 8 px apart and at least a button tall, so labels centre on the buttons.
pub fn controls(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(BUTTON_H);
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

/// Paints a hover tint behind `body` when the pointer is over it, like a Layers row.
pub fn hover_block<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::NONE.inner_margin(Margin::symmetric(10, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui)
    });
    if ui.rect_contains_pointer(inner.response.rect) {
        ui.painter().set(bg, Shape::rect_filled(inner.response.rect, 4, ACCENT_HOVER));
    }
    inner.inner
}

/// egui's own widgets in the flow take the canvas palette, never its default blue.
pub fn visuals() -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.window_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    v.extreme_bg_color = BG_PANEL;
    v.faint_bg_color = BG_CANVAS_VOID;
    v.code_bg_color = BG_CANVAS_VOID;
    v.hyperlink_color = ACCENT_PRIMARY;
    v.selection.bg_fill = ACCENT_HOVER;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT_PRIMARY);
    v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT_PRIMARY);
    let w = &mut v.widgets;
    for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = CornerRadius::same(4);
        state.fg_stroke.color = TEXT_PRIMARY;
        state.expansion = 0.0;
    }
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    w.inactive.bg_fill = BG_PANEL;
    w.inactive.weak_bg_fill = BG_PANEL;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    for state in [&mut w.hovered, &mut w.active, &mut w.open] {
        state.bg_fill = ACCENT_HOVER;
        state.weak_bg_fill = ACCENT_HOVER;
        state.bg_stroke = Stroke::new(1.0_f32, ACCENT_PRIMARY);
    }
    v
}

pub fn themed<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        *ui.visuals_mut() = visuals();
        add(ui)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_done_current_or_upcoming_by_its_number() {
        let states: Vec<StepState> = (1..=3).map(|n| StepState::of(n, 2)).collect();
        assert_eq!(states, [StepState::Done, StepState::Current, StepState::Upcoming]);
    }

    #[test]
    fn the_flow_visuals_select_in_the_accent_tint_not_egui_blue() {
        let v = visuals();
        assert_eq!(v.selection.bg_fill, ACCENT_HOVER);
        assert_ne!(v.selection.bg_fill, Visuals::light().selection.bg_fill);
        assert_eq!(v.widgets.inactive.bg_stroke.color, BORDER_SUBTLE);
    }
}

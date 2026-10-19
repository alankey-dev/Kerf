//! The text group at the left of the edit toolbar: font, size in points, style, alignment and Format.
//! It shows only when every selected shape is text, and each change applies to all of them.

use egui::{Align2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Shape, ShapeKind};
use crate::text::{FontRef, FontStyle, TextAlign, VAlign};
use crate::text_ops::{POINTS_PER_IN, Split, TextChange, can_split};
use crate::transform::Handle;
use crate::ui::chrome::text;
use crate::ui::fonts;
use crate::ui::icons::{Icon, paint_icon};
use crate::ui::popover::{disabled_row, popover, text_row};
use crate::ui::tokens::*;
use crate::ui::tooltip::tip;

use super::{divider, font_menu, format, tool};

const FIELD_H: f32 = 32.0;

/// The values every selected text agrees on; `None` in a field where they differ.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextCommon {
    pub font: Option<FontRef>,
    pub size_pt: Option<f32>,
    pub style: Option<FontStyle>,
    pub align: Option<TextAlign>,
    pub letter_space: Option<f32>,
    pub line_space: Option<f32>,
    pub wrap: Option<bool>,
    pub v_align: Option<VAlign>,
    /// Whether any selected text splits into letters, and into lines.
    pub letters: bool,
    pub lines: bool,
}

impl TextCommon {
    /// `None` unless `shapes` is non-empty and every one is text.
    pub fn of(shapes: &[&Shape]) -> Option<Self> {
        let all: Vec<Self> = shapes.iter().map(|s| Self::one(s)).collect::<Option<_>>()?;
        let (first, rest) = all.split_first()?;
        Some(rest.iter().fold(first.clone(), |acc, t| Self {
            font: agree(acc.font, t.font.clone()),
            size_pt: agree(acc.size_pt, t.size_pt),
            style: agree(acc.style, t.style),
            align: agree(acc.align, t.align),
            letter_space: agree(acc.letter_space, t.letter_space),
            line_space: agree(acc.line_space, t.line_space),
            wrap: agree(acc.wrap, t.wrap),
            v_align: agree(acc.v_align, t.v_align),
            letters: acc.letters || t.letters,
            lines: acc.lines || t.lines,
        }))
    }

    fn one(shape: &Shape) -> Option<Self> {
        let ShapeKind::Text { font, size_in, style, letter_space, line_space, align, wrap_in, v_align, .. } = &shape.kind else { return None };
        Some(Self {
            font: Some(font.clone()),
            size_pt: Some(size_in * POINTS_PER_IN),
            style: Some(*style),
            align: Some(*align),
            letter_space: Some(*letter_space),
            line_space: Some(*line_space),
            wrap: Some(wrap_in.is_some()),
            v_align: Some(*v_align),
            letters: can_split(shape, Split::Letters),
            lines: can_split(shape, Split::Lines),
        })
    }
}

fn agree<T: PartialEq>(a: Option<T>, b: Option<T>) -> Option<T> {
    if a == b { a } else { None }
}

/// Dragging a side handle of wrapped text moves the wrap width and reflows the words, keeping the
/// glyphs' scale. `None` for any other shape or handle, which resize as usual.
pub fn rewrap(orig: &Shape, handle: Handle, to: &Shape) -> Option<Shape> {
    if !matches!(handle, Handle::Left | Handle::Right) || !matches!(orig.kind, ShapeKind::Text { wrap_in: Some(_), .. }) {
        return None;
    }
    let (natural_w, natural_h) = orig.kind.default_size_in();
    let scale = (orig.width_in / natural_w, orig.height_in / natural_h);
    let mut out = orig.clone();
    if let ShapeKind::Text { wrap_in, .. } = &mut out.kind {
        *wrap_in = Some(to.width_in / scale.0);
    }
    let (w, h) = out.kind.default_size_in();
    (out.x_in, out.width_in, out.height_in) = (to.x_in, w * scale.0, h * scale.1);
    Some(out)
}

/// Both bundled fonts carry every weight, and italics are sheared, so every style is offered. A
/// system family offers the four styles that resolve to its closest face or a synthetic one. A writing
/// font has one weight, sheared for italic.
fn styles(font: &FontRef) -> &'static [FontStyle] {
    const ALL: [FontStyle; 5] = [FontStyle::Regular, FontStyle::Medium, FontStyle::Bold, FontStyle::Italic, FontStyle::BoldItalic];
    const SYSTEM: [FontStyle; 4] = [FontStyle::Regular, FontStyle::Bold, FontStyle::Italic, FontStyle::BoldItalic];
    const WRITING: [FontStyle; 2] = [FontStyle::Regular, FontStyle::Italic];
    match font {
        FontRef::Inter | FontRef::SpaceGrotesk => &ALL,
        FontRef::System(_) => &SYSTEM,
        FontRef::Stroke(_) => &WRITING,
    }
}

const ONE_WEIGHT: &str = "Writing fonts have one weight; the pen size sets the line weight.";

fn style_label(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Regular => "Regular",
        FontStyle::Medium => "Medium",
        FontStyle::Bold => "Bold",
        FontStyle::Italic => "Italic",
        FontStyle::BoldItalic => "Bold Italic",
    }
}

fn align_icon(align: TextAlign) -> Icon {
    match align {
        TextAlign::Left => Icon::TextAlignLeft,
        TextAlign::Centre => Icon::TextAlignCentre,
        TextAlign::Right => Icon::TextAlignRight,
    }
}

fn next_align(align: Option<TextAlign>) -> TextAlign {
    match align {
        Some(TextAlign::Left) => TextAlign::Centre,
        Some(TextAlign::Centre) => TextAlign::Right,
        Some(TextAlign::Right) | None => TextAlign::Left,
    }
}

/// Font, Size, Style, Alignment and Format, each followed by a toolbar divider.
pub fn group(ui: &mut Ui, common: &TextCommon) -> Option<TextChange> {
    let font = font_dropdown(ui, common.font.as_ref());
    divider(ui);
    let size = crate::units::number_field(ui, "Font size", common.size_pt, "pt", 64.0).and_then(|t| t.against(common.size_pt)).map(TextChange::SizePt);
    divider(ui);
    let style = style_dropdown(ui, common);
    divider(ui);
    let align = align_button(ui, common.align);
    divider(ui);
    let anchor = tool(ui, Icon::Format, "Format", true, "Letter and line spacing, wrapping and text box alignment.");
    crate::ui::anchors::mark(ui, crate::ui::anchors::Anchor::TextFormat, anchor.rect);
    let format = format::menu(ui, &anchor, common);
    divider(ui);
    font.or(size).or(style).or(align).or(format)
}

fn font_dropdown(ui: &mut Ui, current: Option<&FontRef>) -> Option<TextChange> {
    let label = current.map(FontRef::label).unwrap_or_default();
    let face = current.map_or_else(|| fonts::regular(14.0), |f| fonts::typeface(ui.ctx(), f, 14.0));
    let anchor = dropdown(ui, "Font", &label, None, 136.0, face);
    crate::ui::anchors::mark(ui, crate::ui::anchors::Anchor::FontMenu, anchor.rect);
    if anchor.clicked() {
        font_menu::reset(ui.ctx());
    }
    let fonts = crate::system_fonts::current();
    popover(ui, &anchor, "fonts", 260.0, |ui| font_menu::show(ui, current, fonts.families(), &fonts.writing())).map(TextChange::Font)
}

fn style_dropdown(ui: &mut Ui, common: &TextCommon) -> Option<TextChange> {
    let label = common.style.map_or("", style_label);
    let anchor = dropdown(ui, "Font style", label, Some("A"), 128.0, fonts::regular(14.0));
    let writing = common.font.as_ref().is_some_and(|f| matches!(f, FontRef::Stroke(_)));
    let offered = common.font.as_ref().map_or(styles(&FontRef::Inter), styles);
    popover(ui, &anchor, "styles", 160.0, |ui| {
        let mut picked = None;
        for &st in offered {
            if text_row(ui, style_label(st)).clicked() {
                picked = Some(TextChange::Style(st));
            }
            if writing && st == FontStyle::Regular {
                disabled_row(ui, style_label(FontStyle::Bold), ONE_WEIGHT);
            }
        }
        picked
    })
}

fn align_button(ui: &mut Ui, current: Option<TextAlign>) -> Option<TextChange> {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, FIELD_H), Sense::click());
    let resp = tip(resp, "Alignment", "Cycle the lines between left, centre and right.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Text alignment"));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    if let Some(align) = current {
        paint_icon(ui, align_icon(align), Rect::from_center_size(rect.center(), Vec2::splat(20.0)), TEXT_PRIMARY);
    }
    resp.clicked().then(|| TextChange::Align(next_align(current)))
}

/// A bordered dropdown trigger showing `value` in `font`, with an optional leading glyph.
fn dropdown(ui: &mut Ui, label: &str, value: &str, glyph: Option<&str>, width: f32, font: egui::FontId) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::click());
    let resp = tip(resp, label, "", None);
    resp.widget_info(|| WidgetInfo { current_text_value: Some(value.to_owned()), ..WidgetInfo::labeled(WidgetType::ComboBox, true, label) });
    let fill = if resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    ui.painter().rect(rect, 8.0, fill, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    if let Some(glyph) = glyph {
        text(ui.painter(), pos2(x + 10.0, rect.center().y), Align2::CENTER_CENTER, glyph, fonts::semibold(18.0), TEXT_PRIMARY);
        x += 26.0;
    }
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 28.0, rect.bottom()));
    ui.painter().with_clip_rect(clip).text(pos2(x, rect.center().y), Align2::LEFT_CENTER, value, font, TEXT_PRIMARY);
    paint_icon(ui, Icon::Chevron, Rect::from_min_size(pos2(rect.right() - 26.0, rect.center().y - 10.0), Vec2::splat(20.0)), TEXT_PRIMARY);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::Design;
    use crate::edit::{Edit, Subject};
    use egui::Color32;

    fn apply(d: &mut Design, ids: &mut Vec<u64>, change: TextChange) {
        crate::edit::run(d, ids, &Edit::Text(change), Subject::Selection).unwrap();
    }

    fn text_design(contents: &[&str]) -> (Design, Vec<u64>) {
        let mut d = Design::default();
        let ids = contents.iter().map(|c| d.add_text((*c).into(), FontRef::Inter, 1.0, Color32::BLACK, 1.0, 1.0)).collect();
        (d, ids)
    }

    fn shapes<'a>(d: &'a Design, ids: &[u64]) -> Vec<&'a Shape> {
        ids.iter().map(|id| d.shapes.iter().find(|s| s.id == *id).unwrap()).collect()
    }

    #[test]
    fn common_reports_shared_values_and_blanks_differences() {
        let (mut d, ids) = text_design(&["One", "Two"]);
        apply(&mut d, &mut ids[1..].to_vec(), TextChange::SizePt(36.0));
        let common = TextCommon::of(&shapes(&d, &ids)).unwrap();
        assert_eq!(common.font, Some(FontRef::Inter));
        assert_eq!(common.size_pt, None, "sizes differ");
        assert_eq!(common.wrap, Some(false));
    }

    #[test]
    fn common_is_none_unless_every_shape_is_text() {
        let (mut d, mut ids) = text_design(&["One"]);
        ids.push(d.add_shape(ShapeKind::Square));
        assert_eq!(TextCommon::of(&shapes(&d, &ids)), None);
        assert_eq!(TextCommon::of(&[]), None);
    }

    #[test]
    fn size_in_points_sets_the_em_in_inches_and_clamps() {
        let (mut d, ids) = text_design(&["Hi"]);
        apply(&mut d, &mut ids.clone(), TextChange::SizePt(36.0));
        assert_eq!(TextCommon::of(&shapes(&d, &ids)).unwrap().size_pt, Some(36.0));
        apply(&mut d, &mut ids.clone(), TextChange::SizePt(5000.0));
        assert_eq!(TextCommon::of(&shapes(&d, &ids)).unwrap().size_pt, Some(1440.0));
    }

    #[test]
    fn a_change_applies_to_every_selected_text_and_refits_its_box() {
        let (mut d, ids) = text_design(&["Wide", "Wider"]);
        let before: Vec<f32> = d.shapes.iter().map(|s| s.width_in).collect();
        apply(&mut d, &mut ids.clone(), TextChange::LetterSpace(200.0));
        for (s, w) in d.shapes.iter().zip(before) {
            assert!(s.width_in > w + 0.1, "{} grew from {w} to {}", s.id, s.width_in);
        }
    }

    #[test]
    fn turning_wrap_on_keeps_the_lines_and_box() {
        let (mut d, ids) = text_design(&["Hello there world"]);
        let (w, h) = (d.shapes[0].width_in, d.shapes[0].height_in);
        apply(&mut d, &mut ids.clone(), TextChange::Wrap(true));
        assert!((d.shapes[0].width_in - w).abs() < 1e-4 && (d.shapes[0].height_in - h).abs() < 1e-4);
        assert_eq!(TextCommon::of(&shapes(&d, &ids)).unwrap().wrap, Some(true));
    }

    #[test]
    fn alignment_cycles_left_centre_right() {
        assert_eq!(next_align(Some(TextAlign::Left)), TextAlign::Centre);
        assert_eq!(next_align(Some(TextAlign::Centre)), TextAlign::Right);
        assert_eq!(next_align(Some(TextAlign::Right)), TextAlign::Left);
        assert_eq!(next_align(None), TextAlign::Left, "a mixed selection starts at Left");
    }

    #[test]
    fn rewrap_narrows_the_wrap_and_adds_a_line_without_scaling_glyphs() {
        let (mut d, ids) = text_design(&["Hello there world"]);
        d.shapes[0].width_in *= 2.0;
        d.shapes[0].height_in *= 2.0;
        apply(&mut d, &mut ids.clone(), TextChange::Wrap(true));
        let orig = d.shapes[0].clone();
        let to = Shape { width_in: orig.width_in * 0.6, ..orig.clone() };
        let out = rewrap(&orig, Handle::Right, &to).unwrap();
        assert!((out.width_in - to.width_in).abs() < 1e-4, "the box follows the handle");
        assert!(out.height_in > orig.height_in * 1.9, "a second line at the same scale: {} vs {}", out.height_in, orig.height_in);
        assert!(rewrap(&orig, Handle::Bottom, &to).is_none(), "top and bottom handles still stretch");
    }

    #[test]
    fn rewrap_ignores_unwrapped_text() {
        let (d, _) = text_design(&["Hello"]);
        let s = &d.shapes[0];
        let to = Shape { width_in: 0.5, ..s.clone() };
        assert!(rewrap(s, Handle::Right, &to).is_none());
    }
}

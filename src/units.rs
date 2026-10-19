//! The one place a length becomes text. Designs store inches; `Units` converts them to and from what
//! the user sees and types, and its field widgets are the only place the app takes a typed number.

use egui::text::{CCursor, CCursorRange};
use egui::{Align, Align2, Context, Id, Key, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::expr::{ExprError, Quantity, Typed};
use crate::ui::fonts;
use crate::ui::tokens::{ACCENT_HOVER, ACCENT_PRIMARY, BG_PANEL, BORDER_SUBTLE, ERROR_BG, ERROR_EDGE, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Units {
    #[default]
    Imperial,
    Metric,
}

impl Units {
    pub const CM_PER_IN: f32 = 2.54;

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Imperial => "in",
            Self::Metric => "cm",
        }
    }

    #[allow(clippy::wrong_self_convention, reason = "reads as a conversion from inches into these units")]
    pub fn from_in(self, inches: f32) -> f32 {
        match self {
            Self::Imperial => inches,
            Self::Metric => inches * Self::CM_PER_IN,
        }
    }

    pub fn to_in(self, shown: f32) -> f32 {
        match self {
            Self::Imperial => shown,
            Self::Metric => shown / Self::CM_PER_IN,
        }
    }

    /// Two decimals, trailing zeros trimmed: "5.08 cm", "2 in".
    pub fn len(self, inches: f32) -> String {
        format!("{} {}", number(self.from_in(inches)), self.suffix())
    }

    pub fn size(self, w_in: f32, h_in: f32) -> String {
        format!("{} x {}", self.len(w_in), self.len(h_in))
    }

    /// An area in square units, to three significant figures: "0.0016 in²", "1.5 cm²".
    pub fn area(self, sq_in: f32) -> String {
        let shown = sq_in * self.from_in(1.0).powi(2);
        let decimals = if shown > 0.0 { (2 - shown.log10().floor() as i32).clamp(0, 6) as usize } else { 0 };
        let s = format!("{shown:.decimals$}");
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
        format!("{s} {}²", self.suffix())
    }

    /// The unit an exported file states its lengths in: millimetres for metric, as CAD and laser
    /// software expect, rather than the centimetres the screen shows.
    pub fn file_unit(self) -> &'static str {
        match self {
            Self::Imperial => "in",
            Self::Metric => "mm",
        }
    }

    pub fn file_from_in(self, inches: f32) -> f32 {
        match self {
            Self::Imperial => inches,
            Self::Metric => inches * Self::CM_PER_IN * 10.0,
        }
    }
}

fn number(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

pub const FIELD_H: f32 = 28.0;
/// A field's width in a popover or a form row.
pub const FIELD_W: f32 = 96.0;

/// The one way the app takes a typed length: a box showing `inches` in `units` that accepts an
/// expression (`2in + 5mm`, `*2`). Returns the entry once, on Enter or blur; a `Typed::To` is in
/// inches. `None` shows blank, for a selection that disagrees.
pub fn length_field(ui: &mut Ui, label: &str, inches: Option<f32>, units: Units, width: f32) -> Option<Typed> {
    field(ui, label, inches.map(|v| units.from_in(v)), Quantity::Length(units), units.suffix(), width)
}

/// [`length_field`] for an angle, count or percentage: the same expressions, without units.
pub fn number_field(ui: &mut Ui, label: &str, value: Option<f32>, suffix: &str, width: f32) -> Option<Typed> {
    field(ui, label, value, Quantity::Plain, suffix, width)
}

/// What the user has typed since focusing a field, and why it did not parse on their last Enter.
#[derive(Clone, Default)]
struct Draft {
    text: String,
    edited: bool,
    error: Option<ExprError>,
}

fn field(ui: &mut Ui, label: &str, shown: Option<f32>, q: Quantity, suffix: &str, width: f32) -> Option<Typed> {
    let id = ui.id().with(("field", label));
    let focused = ui.memory(|m| m.has_focus(id));
    let shown_text = shown.map(field_number).unwrap_or_default();
    let mut draft = ui.data(|d| d.get_temp::<Draft>(id)).filter(|_| focused).unwrap_or_else(|| Draft { text: shown_text.clone(), edited: false, error: None });

    let (rect, hover) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::hover());
    let (fill, edge) = match (&draft.error, focused) {
        (Some(_), _) => (ERROR_BG, ERROR_EDGE),
        (None, true) => (BG_PANEL, ACCENT_PRIMARY),
        (None, false) if ui.is_enabled() && hover.hovered() => (ACCENT_HOVER, BORDER_SUBTLE),
        (None, false) => (BG_PANEL, BORDER_SUBTLE),
    };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
    let unit = ui.painter().text(rect.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, suffix, fonts::regular(11.0), TEXT_MUTED);
    if !suffix.is_empty() {
        hover.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), suffix));
    }
    let inner = Rect::from_min_max(rect.min + vec2(6.0, 1.0), pos2(unit.left() - 4.0, rect.max.y - 1.0));
    let edit = egui::TextEdit::singleline(&mut draft.text)
        .id(id)
        .frame(false)
        .hint_text(RichText::new("-").color(TEXT_MUTED))
        .font(fonts::regular(13.0))
        .text_color(TEXT_PRIMARY)
        .vertical_align(Align::Center);
    // A child, so the editor does not pull the row's cursor back from the end of the box.
    let resp = ui.new_child(UiBuilder::new().max_rect(rect)).put(inner, edit);
    resp.widget_info(|| WidgetInfo { value: shown.map(f64::from), ..WidgetInfo::labeled(WidgetType::TextEdit, ui.is_enabled(), label) });
    if resp.gained_focus() {
        select_all(ui.ctx(), id, draft.text.chars().count());
    }
    if resp.changed() {
        draft.edited = true;
        draft.error = None;
    }

    let mut out = None;
    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if resp.lost_focus() && !escape && draft.edited && !draft.text.trim().is_empty() {
        let typed = if q == Quantity::Plain { without_mark(&draft.text, suffix) } else { &draft.text };
        match crate::expr::parse(typed, q) {
            Ok(typed) => out = Some(typed),
            Err(e) if enter => {
                draft.error = Some(e);
                resp.request_focus();
            }
            Err(_) => {}
        }
    }
    if let Some(e) = &draft.error {
        crate::ui::tooltip::card(&resp, &e.to_string(), "Fix it, or press Escape to keep the old value.", None);
    }
    let editing = resp.has_focus();
    ui.data_mut(|d| {
        if editing {
            d.insert_temp(id, draft);
        } else {
            d.remove_temp::<Draft>(id);
        }
    });
    out
}

/// `text` without a trailing copy of the field's own `suffix` (`12pt`, `500 ms`), which a plain
/// field shows but its parser does not know.
fn without_mark<'a>(text: &'a str, suffix: &str) -> &'a str {
    let trimmed = text.trim_end();
    trimmed.strip_suffix(suffix).filter(|_| !suffix.is_empty()).unwrap_or(text)
}

/// Up to three decimals, trailing zeros trimmed.
fn field_number(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn select_all(ctx: &Context, id: Id, len: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
        state.store(ctx, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_convert_both_ways_and_format_trimmed() {
        assert_eq!(Units::Imperial.len(2.0), "2 in");
        assert_eq!(Units::Metric.len(2.0), "5.08 cm");
        assert_eq!(Units::Metric.len(0.5), "1.27 cm");
        assert_eq!(Units::Imperial.len(0.25), "0.25 in");
        assert_eq!(Units::Imperial.len(-0.001), "0 in");
        assert_eq!(Units::Metric.size(2.0, 1.0), "5.08 cm x 2.54 cm");
        assert!((Units::Metric.to_in(5.08) - 2.0).abs() < 1e-6);
        assert_eq!(Units::Imperial.to_in(Units::Imperial.from_in(3.5)), 3.5);
    }

    #[test]
    fn areas_show_three_significant_figures_in_square_units() {
        assert_eq!(Units::Imperial.area(0.0016), "0.0016 in²");
        assert_eq!(Units::Imperial.area(0.25), "0.25 in²");
        assert_eq!(Units::Imperial.area(12.345), "12.3 in²");
        assert_eq!(Units::Metric.area(1.0), "6.45 cm²");
        assert_eq!(Units::Metric.area(0.0), "0 cm²");
    }

    /// Lines in `source` that format a unit by hand instead of going through `Units`.
    fn hand_formatted(source: &str) -> Vec<(usize, &str)> {
        source.lines().enumerate().filter(|(_, line)| formats_a_unit(line)).map(|(n, line)| (n + 1, line.trim())).collect()
    }

    fn formats_a_unit(line: &str) -> bool {
        const LITERALS: [&str; 4] = ["\"in\"", "\"cm\"", "suffix(\" in\")", "suffix(\" cm\")"];
        LITERALS.iter().any(|l| line.contains(l)) || placeholder_then_unit(line)
    }

    /// `\{[^}]*\} ?(in|cm)\b`, without a regex dependency.
    fn placeholder_then_unit(line: &str) -> bool {
        line.match_indices('{').any(|(open, _)| {
            let Some(close) = line[open..].find('}') else { return false };
            let rest = &line[open + close + 1..];
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            ["in", "cm"].iter().any(|u| rest.strip_prefix(u).is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')))
        })
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn the_scan_catches_hand_formatted_units_and_spares_ordinary_words() {
        for bad in ["format!(\"{} in\", x)", "format!(\"{w:.1}cm\")", "let s = \"cm\";", ".suffix(\" in\")"] {
            assert!(formats_a_unit(bad), "{bad}");
        }
        for fine in ["ui.button(\"Zoom in\")", "format!(\"{} inches\", x)", "format!(\"{a} into {b}\")", "\"Centimetres\""] {
            assert!(!formats_a_unit(fine), "{fine}");
        }
    }

    /// `find`'s hits in every source file except those ending in one of `skip`, as `file:line: text`.
    fn offenders(skip: &[&str], find: fn(&str) -> Vec<(usize, &str)>) -> Vec<String> {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        files
            .iter()
            .filter(|p| !skip.iter().any(|s| p.ends_with(s)))
            .flat_map(|p| {
                let text = std::fs::read_to_string(p).unwrap();
                let rel = p.strip_prefix(&src).unwrap().display().to_string();
                find(&text).into_iter().map(move |(n, line)| format!("{rel}:{n}: {line}")).collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn only_units_machine_and_expr_format_a_unit_by_hand() {
        let offenders = offenders(&["src/units.rs", "src/machine.rs", "src/expr.rs"], hand_formatted);
        assert!(offenders.is_empty(), "format lengths through units::Units:\n{}", offenders.join("\n"));
    }

    /// Lines in `source` that build a number field of their own: any `DragValue`, or a `TextEdit`
    /// with length vocabulary within a few lines of it.
    fn raw_number_fields(source: &str) -> Vec<(usize, &str)> {
        const NEAR: usize = 4;
        let lines: Vec<&str> = source.lines().collect();
        let near_a_length = |n: usize| lines[n.saturating_sub(NEAR)..(n + NEAR + 1).min(lines.len())].iter().any(|l| mentions_a_length(l));
        let builds = |n: usize, line: &str| line.contains("DragValue::") || line.contains("TextEdit::") && near_a_length(n);
        lines.iter().enumerate().filter(|&(n, line)| builds(n, line)).map(|(n, line)| (n + 1, line.trim())).collect()
    }

    fn mentions_a_length(line: &str) -> bool {
        let word = |w: &str| w.ends_with("_in") || w.ends_with("_mm") || matches!(w, "inches" | "units" | "Units" | "from_in" | "to_in");
        line.contains("parse::<") || line.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(word)
    }

    #[test]
    fn the_field_scan_catches_raw_length_inputs_and_spares_text_inputs() {
        for bad in [
            "let r = ui.add(egui::DragValue::new(&mut s.width_in).range(0.0..=10.0));",
            "ui.add(DragValue::new(&mut copies));",
            "ui.add(egui::DragValue::from_get_set(|v| v.unwrap_or(0.0)));",
            "let edit = TextEdit::singleline(&mut text);\nlet n = text.parse::<u32>();",
            "let edit = egui::TextEdit::singleline(&mut text);\nif let Ok(v) = text.parse::<f32>() {\n    s.width_in = units.to_in(v);\n}",
        ] {
            assert!(!raw_number_fields(bad).is_empty(), "{bad}");
        }
        for fine in [
            "let edit = egui::TextEdit::singleline(&mut self.query).hint_text(\"Search\");",
            "ui.add(egui::TextEdit::singleline(name).hint_text(NAME_HINT).desired_width(f32::INFINITY));",
            "resp.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label));",
        ] {
            assert!(raw_number_fields(fine).is_empty(), "{fine}");
        }
    }

    #[test]
    fn a_plain_field_ignores_its_own_suffix() {
        assert_eq!(without_mark("12pt", "pt"), "12");
        assert_eq!(without_mark("500 ms ", "ms"), "500 ");
        assert_eq!(without_mark("12", ""), "12");
    }

    #[test]
    fn only_units_builds_a_number_field() {
        let offenders = offenders(&["src/units.rs"], raw_number_fields);
        assert!(offenders.is_empty(), "take typed numbers through units::length_field or number_field:\n{}", offenders.join("\n"));
    }
}

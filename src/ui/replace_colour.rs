//! Replace colour (spec 56): a modal that lists the design's fill colours with counts and swaps one for
//! another across every shape that has it, in one undo step. It is Select Same Colour then a swatch click
//! (spec 50) packaged for "make all the dark blue navy". Pens and patterns are left alone.

use egui::{Align2, Color32, Context, Id, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::{Design, Operation};
use crate::edit::{Blocked, Edit, Subject};
use crate::palettes;

use super::Kerf;
use super::flow_kit::{self as kit, heading};
use super::tokens::*;

/// The ids of every shape Replace colour would recolour: a plain fill (not a pen layer, not a pattern)
/// equal to `old`.
pub fn shapes_with_fill(design: &Design, old: Color32) -> Vec<u64> {
    design.shapes.iter().filter(|s| s.operation != Operation::Pen && s.pattern.is_none() && s.fill == old).map(|s| s.id).collect()
}

/// Recolours every shape whose fill is `old` to `new`, as one edit, leaving the selection be. Pens and
/// patterns keep their look.
pub fn replace(design: &mut Design, selection: &mut Vec<u64>, old: Color32, new: Color32) -> Result<(), Blocked> {
    let ids = shapes_with_fill(design, old);
    crate::edit::run(design, selection, &Edit::Colour(new), Subject::Ids(&ids))
}

/// One distinct fill colour the design uses (pens excluded), as the modal lists it.
fn fill_colours(design: &Design) -> Vec<(Color32, usize)> {
    palettes::project_colours(design).into_iter().filter(|u| !u.pen).map(|u| (u.colour, u.count)).collect()
}

pub struct Dialog {
    /// The colour being replaced.
    from: Color32,
    /// The replacement, as typed text; parsed with [`palettes::parse_colour`].
    to: String,
    picked_from: bool,
}

impl Dialog {
    /// Opens on the design's most used fill, with the replacement field empty.
    pub fn new(design: &Design) -> Self {
        let from = fill_colours(design).first().map_or(Color32::BLACK, |&(c, _)| c);
        Self { from, to: String::new(), picked_from: false }
    }
}

pub enum Outcome {
    Cancel,
    Apply { from: Color32, to: Color32 },
}

const WIDTH: f32 = 320.0;

pub fn show(ctx: &Context, dialog: &mut Dialog, design: &Design) -> Option<Outcome> {
    let colours = fill_colours(design);
    let mut outcome = None;
    let modal = egui::Modal::new(Id::new("replace-colour")).frame(kit::card_frame()).show(ctx, |ui| {
        kit::themed(ui, |ui| {
            ui.set_width(WIDTH);
            heading(ui, "Replace colour");
            ui.add_space(12.0);
            kit::body_text(ui, "Replace one colour everywhere it is used, across every mat.", true);
            ui.add_space(12.0);
            for &(colour, count) in &colours {
                if from_row(ui, colour, count, dialog.from == colour) {
                    dialog.from = colour;
                    dialog.picked_from = true;
                }
            }
            ui.add_space(12.0);
            kit::body_text(ui, "New colour", false);
            ui.add_space(4.0);
            kit::text_field(ui, &mut dialog.to, "#RRGGBB or a colour name").widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "New colour"));
            let parsed = palettes::parse_colour(&dialog.to).ok();
            if !dialog.to.trim().is_empty() && parsed.is_none() {
                ui.add_space(4.0);
                kit::body_text(ui, "Type a hex colour such as #1C2E6B, or a colour name.", true);
            }
            ui.add_space(16.0);
            let (mut apply, mut cancel) = (false, false);
            let ready = parsed.is_some_and(|to| to != dialog.from) && !shapes_with_fill(design, dialog.from).is_empty();
            kit::actions(ui, |ui| {
                apply = kit::primary_button(ui, "Replace", ready).clicked();
                cancel = kit::secondary_button(ui, "Cancel", true).clicked();
            });
            if apply && let Some(to) = parsed {
                outcome = Some(Outcome::Apply { from: dialog.from, to });
            } else if cancel {
                outcome = Some(Outcome::Cancel);
            }
        });
    });
    if modal.should_close() && outcome.is_none() {
        outcome = Some(Outcome::Cancel);
    }
    outcome
}

/// A selectable row: a colour chip, its name and shape count, with an accent tint and ring while chosen.
fn from_row(ui: &mut Ui, colour: Color32, count: usize, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let shapes = if count == 1 { "shape" } else { "shapes" };
    let label = format!("{}, {count} {shapes}", palettes::name_of(colour));
    let resp = super::tooltip::tip(resp, &label, "Replace this colour.", None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, selected, &label));
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    if selected {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0_f32, ACCENT_PRIMARY), StrokeKind::Inside);
    }
    let chip = Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), Vec2::splat(16.0));
    ui.painter().rect(chip, 3.0, colour, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    super::chrome::text(ui.painter(), pos2(chip.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, &label, super::fonts::regular(13.0), TEXT_PRIMARY);
    resp.clicked()
}

impl Kerf {
    pub(super) fn replace_colour_dialog(&mut self, ctx: &Context) {
        let Some(dialog) = self.doc.replace_colour.as_mut() else { return };
        match show(ctx, dialog, &self.doc.design) {
            Some(Outcome::Cancel) => self.doc.replace_colour = None,
            Some(Outcome::Apply { from, to }) => {
                self.doc.replace_colour = None;
                if let Err(blocked) = replace(&mut self.doc.design, &mut self.doc.selection, from, to) {
                    self.report(blocked.reason());
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::patterns::{PatternFill, PatternMode};
    use crate::pens::{Pen, PenSize};

    const RED: Color32 = Color32::from_rgb(0xD0, 0x20, 0x2E);
    const BLUE: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xBF);
    const NAVY: Color32 = Color32::from_rgb(0x1C, 0x2E, 0x6B);

    #[test]
    fn replace_maps_fills_and_leaves_pens_and_patterns_in_one_edit() {
        let mut d = Design::default();
        let mut reds = Vec::new();
        for _ in 0..3 {
            let id = d.add_shape(ShapeKind::Square);
            d.shape_mut(id).unwrap().fill = RED;
            reds.push(id);
        }
        let blue = d.add_shape(ShapeKind::Square);
        d.shape_mut(blue).unwrap().fill = BLUE;
        let pen = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(pen).unwrap();
        s.set_operation(Operation::Pen);
        s.set_pen(Pen::named("Red", PenSize::Fine));
        let patterned = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(patterned).unwrap();
        s.fill = RED;
        s.pattern = Some(PatternFill::new(crate::assets::id_of(&image::RgbaImage::new(1, 1)), 1.0, PatternMode::Tile));

        assert_eq!(shapes_with_fill(&d, RED), reds, "only the plain red fills, not the pen or the pattern");

        let mut selection = vec![blue];
        replace(&mut d, &mut selection, RED, NAVY).unwrap();

        for id in &reds {
            assert_eq!(d.shape_mut(*id).unwrap().fill, NAVY, "every red fill is navy");
        }
        assert_eq!(d.shape_mut(blue).unwrap().fill, BLUE, "the blue fill is untouched");
        assert_eq!(d.shape_mut(pen).unwrap().pen.colour_name(), "Red", "the red pen keeps its pen");
        assert!(d.shape_mut(patterned).unwrap().pattern.is_some(), "the pattern-filled shape keeps its pattern");
        assert_eq!(selection, vec![blue], "Replace colour does not change the selection");
    }

    #[test]
    fn replacing_an_unused_colour_is_refused() {
        let mut d = Design::default();
        d.add_shape(ShapeKind::Square);
        let mut selection = Vec::new();
        assert_eq!(replace(&mut d, &mut selection, RED, NAVY), Err(Blocked::NothingSelected));
    }
}

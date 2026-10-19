//! Step and repeat and Rotate copies (spec 52): a popover near the top-right of the canvas with a live
//! preview of the copies in faint accent outlines around the selection while its fields are edited. Apply
//! makes the copies as one undo step through `Edit::StepRepeat` or `Edit::RotateCopies`; Cancel or Escape
//! makes nothing.

use egui::{Area, Id, Key, Order, Painter, Ui, pos2, vec2};

use crate::design::{Design, Shape};
use crate::edit::Edit;
use crate::repeat::{RotateCopies, StepRepeat};
use crate::transform::Frame;
use crate::units::{FIELD_W, Units, length_field, number_field};

use super::canvas::Grid;
use super::commands::Command;
use super::flow_kit as kit;
use super::tokens::*;

const WIDTH: f32 = 236.0;
const LABEL_W: f32 = 64.0;

/// The open popover: Step and repeat, or Rotate copies. It carries the pure params it edits, so Apply
/// hands them straight to the matching edit.
pub enum RepeatPanel {
    Step(StepRepeat),
    Rotate(RotateCopies),
}

impl RepeatPanel {
    /// Step and repeat's defaults: three copies, an inch to the right, no turn or shrink.
    pub fn step() -> Self {
        Self::Step(StepRepeat { copies: 3, offset_in: (1.0, 0.0), rotation_deg: 0.0, scale: 1.0, grid: None })
    }

    /// Rotate copies' defaults: eight copies around a full circle.
    pub fn rotate() -> Self {
        Self::Rotate(RotateCopies { copies: 8, total_deg: 360.0 })
    }

    fn title(&self) -> &'static str {
        match self {
            Self::Step(_) => "Step and repeat",
            Self::Rotate(_) => "Rotate copies",
        }
    }
}

/// Shows the popover and paints its preview. Returns the Apply command, if any. Clears `*panel` on Apply,
/// Cancel or Escape.
pub fn show(ui: &mut Ui, grid: &Grid, canvas: egui::Rect, design: &Design, selection: &[u64], units: Units, panel: &mut Option<RepeatPanel>) -> Option<Command> {
    let open = panel.as_mut()?;
    if selection.is_empty() {
        *panel = None;
        return None;
    }
    paint_preview(&ui.painter_at(canvas), grid, design, selection, open);

    // Near the top-right of the canvas, clear of the inspector, so the faint preview stays in view.
    let anchor = pos2(canvas.right() - WIDTH - 80.0, canvas.top() + 24.0);
    let mut outcome = Outcome::Stay;
    Area::new(Id::new("repeat-popover")).order(Order::Foreground).fixed_pos(anchor).constrain(true).show(ui.ctx(), |ui| {
        kit::card_frame().show(ui, |ui| {
            kit::themed(ui, |ui| {
                ui.set_width(WIDTH);
                kit::heading(ui, open.title());
                ui.add_space(8.0);
                match open {
                    RepeatPanel::Step(s) => step_fields(ui, s, units),
                    RepeatPanel::Rotate(r) => rotate_fields(ui, r),
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if kit::secondary_button(ui, "Cancel", true).clicked() {
                        outcome = Outcome::Cancel;
                    }
                    if kit::primary_button(ui, "Apply", true).clicked() {
                        outcome = Outcome::Apply;
                    }
                });
            });
        });
    });

    if ui.input(|i| i.key_pressed(Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    match outcome {
        Outcome::Stay => None,
        Outcome::Cancel => {
            *panel = None;
            None
        }
        Outcome::Apply => {
            let edit = match panel.take() {
                Some(RepeatPanel::Step(s)) => Edit::StepRepeat(s),
                Some(RepeatPanel::Rotate(r)) => Edit::RotateCopies(r),
                None => return None,
            };
            Some(Command::Edit(edit))
        }
    }
}

enum Outcome {
    Stay,
    Apply,
    Cancel,
}

fn step_fields(ui: &mut Ui, s: &mut StepRepeat, units: Units) {
    count_row(ui, "Copies", &mut s.copies);
    offset_row(ui, "Offset X", "Offset Y", &mut s.offset_in, units);
    row(ui, "Rotation", |ui| _ = number_field(ui, "Rotation step", Some(s.rotation_deg), "°", FIELD_W).is_some_and(|t| t.set(&mut s.rotation_deg, -360.0..=360.0)));
    let mut pct = s.scale * 100.0;
    row(ui, "Scale", |ui| {
        if number_field(ui, "Scale step", Some(pct), "%", FIELD_W).is_some_and(|t| t.set(&mut pct, 1.0..=400.0)) {
            s.scale = pct / 100.0;
        }
    });
    let mut on = s.grid.is_some();
    if kit::toggle_row(ui, "Grid", &mut on).changed() {
        s.grid = on.then_some(crate::repeat::Grid { columns: 3, offset_in: (0.0, 1.0) });
    }
    if let Some(g) = &mut s.grid {
        count_row(ui, "Columns", &mut g.columns);
        offset_row(ui, "Row X", "Row Y", &mut g.offset_in, units);
    }
}

fn rotate_fields(ui: &mut Ui, r: &mut RotateCopies) {
    count_row(ui, "Copies", &mut r.copies);
    row(ui, "Angle", |ui| _ = number_field(ui, "Total angle", Some(r.total_deg), "°", FIELD_W).is_some_and(|t| t.set(&mut r.total_deg, -360.0..=360.0)));
}

fn count_row(ui: &mut Ui, label: &str, value: &mut u32) {
    let mut n = *value as f32;
    row(ui, label, |ui| {
        if number_field(ui, label, Some(n), "", FIELD_W).is_some_and(|t| t.set(&mut n, 1.0..=200.0)) {
            *value = n.round() as u32;
        }
    });
}

fn offset_row(ui: &mut Ui, x_label: &str, y_label: &str, offset: &mut crate::design::Pt, units: Units) {
    row(ui, x_label, |ui| {
        let half = (ui.available_width() - 6.0) / 2.0;
        _ = length_field(ui, x_label, Some(offset.0), units, half).is_some_and(|t| t.set(&mut offset.0, -60.0..=60.0));
        _ = length_field(ui, y_label, Some(offset.1), units, half).is_some_and(|t| t.set(&mut offset.1, -60.0..=60.0));
    });
}

fn row<R>(ui: &mut Ui, caption: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_W, 28.0), egui::Sense::hover());
        ui.put(rect, egui::Label::new(egui::RichText::new(caption).font(super::fonts::regular(12.0)).color(TEXT_MUTED)));
        body(ui)
    })
    .inner
}

/// The copies the open popover would make, as the engine lays them out for the current fields.
fn preview_sets(design: &Design, subtree: &[Shape], panel: &RepeatPanel) -> Vec<Vec<Shape>> {
    match panel {
        RepeatPanel::Step(s) => crate::repeat::step_and_repeat(design, subtree, *s),
        RepeatPanel::Rotate(r) => {
            let pivot = Frame::of(design, subtree).map_or((0.0, 0.0), |f| f.pivot_world(single_pivot(design, subtree)));
            crate::repeat::rotate_copies(design, subtree, *r, pivot)
        }
    }
}

/// A lone subject's pivot, so Rotate copies previews around the same point it will turn about.
fn single_pivot(design: &Design, subtree: &[Shape]) -> Option<crate::design::Pt> {
    match subtree {
        [one] => design.shapes.iter().find(|s| s.id == one.id).and_then(|s| s.pivot),
        _ => None,
    }
}

fn paint_preview(painter: &Painter, grid: &Grid, design: &Design, selection: &[u64], panel: &RepeatPanel) {
    let subtree = design.subtree(selection);
    let faint = egui::Stroke::new(1.0_f32, ACCENT_PRIMARY.gamma_multiply(0.5));
    for set in preview_sets(design, &subtree, panel) {
        for shape in set {
            for (pts, closed) in design.geometry(&shape).lines() {
                let screen: Vec<egui::Pos2> = pts.iter().map(|&p| grid.to_screen(p)).collect();
                if *closed {
                    painter.add(egui::Shape::closed_line(screen, faint));
                } else {
                    painter.add(egui::Shape::line(screen, faint));
                }
            }
        }
    }
}

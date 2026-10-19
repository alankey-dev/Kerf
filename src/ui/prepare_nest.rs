//! Prepare's Optimise layout section (spec 30): the button and its settings, progress while the
//! worker nests, the summary card with Undo optimise, and the notice when the design has changed since.

use std::time::Duration;

use egui::{Context, Key, KeyboardShortcut, Modifiers, Response, Sense, Ui, Vec2, WidgetInfo, WidgetType};

use crate::composition::nested::{self, LayoutStatus};
use crate::composition::{Mat, compose_on};
use crate::design::Design;
use crate::edit::{Edit, Subject};
use crate::make_flow::MakeFlow;
use crate::nesting::worker::Worker;
use crate::nesting::{FreeStep, NestSettings, NestedLayout, Rotation};
use crate::units::Units;

use super::flow_kit as kit;
use super::icons::{Icon, paint_icon};
use super::tokens::*;

/// How long the improvement loop may run after the greedy pass.
const BUDGET: Duration = Duration::from_millis(1500);
const MAX_SPACING_IN: f32 = 1.0;

/// What Prepare's nesting section keeps between frames. Lives on `Kerf`, since the flow holds a clone
/// of the design and the edit has to land on the canvas design and its history.
pub struct Nest {
    pub settings: NestSettings,
    pub budget: Duration,
    running: Option<Worker>,
    summary: Option<Summary>,
    request: Option<Request>,
}

enum Request {
    Apply(NestedLayout),
    Undo,
}

struct Summary {
    before_in: f32,
    after_in: f32,
    before_mats: usize,
    after_mats: usize,
    /// The history depth the optimise left, so Undo optimise only undoes the optimise.
    depth: Option<usize>,
}

impl Default for Nest {
    fn default() -> Self {
        Nest { settings: NestSettings::default(), budget: BUDGET, running: None, summary: None, request: None }
    }
}

impl Nest {
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// Blocks until a running job's thread has sent its layout; the next frame collects it.
    #[cfg(test)]
    pub fn wait_finished(&mut self) {
        if let Some(worker) = self.running.as_mut() {
            worker.wait_finished();
        }
    }
}

/// The section above the Mats list.
pub fn section(ui: &mut Ui, flow: &MakeFlow, nest: &mut Nest, units: Units) {
    let (size, copies) = (flow.setup.mat_size, flow.setup.copies);
    if let Some(layout) = nest.running.as_ref().and_then(Worker::try_result) {
        nest.running = None;
        nest.summary = Some(summarise(flow, &layout));
        nest.request = Some(Request::Apply(layout));
    }
    let status = nested::status(&flow.design, size, copies);
    if let Some(worker) = &nest.running {
        kit::body_text(ui, "Optimising layout", false);
        kit::progress(ui, worker.progress());
        ui.add_space(4.0);
        if kit::secondary_button(ui, "Cancel optimise", true).clicked() {
            worker.cancel();
            nest.running = None;
        }
        kit::divider(ui);
        return;
    }
    let cuts = flow.mats.iter().any(|m| !m.print_then_cut);
    if !cuts && status != LayoutStatus::Stale {
        return;
    }
    let label = if status == LayoutStatus::Stale { "Optimise again" } else { "Optimise layout" };
    if status == LayoutStatus::Stale {
        kit::notice(ui, kit::Notice::Warning, "The design changed since the layout was optimised");
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        if kit::secondary_button(ui, label, cuts).clicked() {
            tracing::info!(target: "cmd", settings = ?nest.settings, "optimise layout");
            nest.summary = None;
            nest.running = Some(Worker::start(nested::job(&flow.design, size, copies, nest.settings), nest.budget));
        }
        let gear = settings_button(ui);
        super::popover::popover(ui, &gear, "nest-settings", 260.0, |ui| {
            settings(ui, &mut nest.settings, units);
            None::<()>
        });
    });
    if let Some(summary) = nest.summary.as_ref().filter(|_| status == LayoutStatus::Current) {
        ui.add_space(8.0);
        if summary_card(ui, summary, units) {
            nest.request = Some(Request::Undo);
        }
    }
    kit::divider(ui);
}

fn settings_button(ui: &mut Ui) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BUTTON_H), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Layout settings"));
    if resp.hovered() {
        ui.painter().rect_filled(rect, BUTTON_H / 2.0, ACCENT_HOVER);
    }
    paint_icon(ui, Icon::More, egui::Rect::from_center_size(rect.center(), Vec2::splat(20.0)), TEXT_PRIMARY);
    super::tooltip::tip(resp, "Layout settings", "Rotation, spacing and holes for Optimise layout.", None)
}

fn settings(ui: &mut Ui, settings: &mut NestSettings, units: Units) {
    super::popover::titled(ui, "Layout settings");
    ui.spacing_mut().item_spacing.y = 6.0;
    kit::body_text(ui, "Rotation", true);
    #[derive(Clone, Copy, PartialEq)]
    enum Pick {
        None,
        Quarter,
        Free,
    }
    let current = match settings.rotation {
        Rotation::None => Pick::None,
        Rotation::Quarter => Pick::Quarter,
        Rotation::Free(_) => Pick::Free,
    };
    match kit::segmented(ui, current, &[(Pick::None, "None"), (Pick::Quarter, "90 degrees"), (Pick::Free, "Free")]) {
        Some(Pick::None) => settings.rotation = Rotation::None,
        Some(Pick::Quarter) => settings.rotation = Rotation::Quarter,
        Some(Pick::Free) => settings.rotation = Rotation::Free(FreeStep::Fifteen),
        None => {}
    }
    if let Rotation::Free(step) = settings.rotation {
        let labels = FreeStep::ALL.map(|s| (s, format!("{} degrees", s.degrees())));
        let options: Vec<(FreeStep, &str)> = labels.iter().map(|(s, l)| (*s, l.as_str())).collect();
        if let Some(step) = kit::segmented(ui, step, &options) {
            settings.rotation = Rotation::Free(step);
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        kit::body_text(ui, "Spacing", true);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            _ = crate::units::length_field(ui, "Spacing", Some(settings.spacing_in), units, crate::units::FIELD_W)
                .is_some_and(|t| t.set(&mut settings.spacing_in, 0.0..=MAX_SPACING_IN));
        });
    });
    kit::toggle_row(ui, "Use holes", &mut settings.use_holes);
}

/// "Material: 24 in to 15.5 in, 35 % less", the mats when their count changed, and Undo optimise.
/// Returns true when Undo optimise is clicked.
fn summary_card(ui: &mut Ui, s: &Summary, units: Units) -> bool {
    kit::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        kit::body_text(ui, material_line(s, units), false);
        if s.before_mats != s.after_mats {
            kit::body_text(ui, format!("Mats: {} to {}", s.before_mats, s.after_mats), false);
        }
        ui.add_space(4.0);
        kit::secondary_button(ui, "Undo optimise", true).clicked()
    })
}

fn material_line(s: &Summary, units: Units) -> String {
    let saved = if s.before_in > 0.0 { ((1.0 - s.after_in / s.before_in) * 100.0).round() } else { 0.0 };
    let lengths = format!("Material: {} to {}", units.len(s.before_in), units.len(s.after_in));
    if saved > 0.0 { format!("{lengths}, {saved} % less") } else { lengths }
}

/// Today's packing of the flow's design against `layout`'s, on the flow's mats.
fn summarise(flow: &MakeFlow, layout: &NestedLayout) -> Summary {
    let with = |nesting: Option<NestedLayout>| {
        let design = Design { nesting, ..flow.design.clone() };
        let mats: Vec<Mat> = compose_on(&design, flow.setup.mat_size, flow.setup.copies, flow.print.paper).into_iter().filter(|m| !m.print_then_cut).collect();
        (nested::material_length(&design, &mats, flow.setup.mat_size), mats.len())
    };
    let ((before_in, before_mats), (after_in, after_mats)) = (with(None), with(Some(layout.clone())));
    Summary { before_in, after_in, before_mats, after_mats, depth: None }
}

/// The inspector's grain lock: a small lock at the right end of `row`, taking no layout space. `locked`
/// is `None` with nothing selected. Returns the lock state a click asks for.
pub(super) fn grain_toggle(ui: &mut Ui, row: egui::Rect, locked: Option<bool>) -> Option<bool> {
    let locked = locked?;
    let side = row.height().max(14.0);
    let font = super::fonts::regular(11.0);
    let word = super::chrome::text_width(ui, "Grain", font.clone());
    let right = ui.max_rect().right();
    let rect = egui::Rect::from_min_max(egui::pos2(right - side - 4.0 - word, row.top()), egui::pos2(right, row.top() + side));
    let resp = ui.interact(rect, ui.id().with("grain-lock"), Sense::click());
    let (label, body) = match locked {
        true => ("Unlock grain", "Optimise layout may only turn the selection upside down. Click to let it turn any way."),
        false => ("Lock grain", "Keeps wood grain, leather and directional glitter running one way. Optimise layout may then only turn the selection upside down."),
    };
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, locked, label));
    let ink = match (locked, resp.hovered()) {
        (true, _) => ACCENT_PRIMARY,
        (false, true) => TEXT_PRIMARY,
        (false, false) => TEXT_MUTED,
    };
    super::chrome::text(ui.painter(), egui::pos2(rect.left(), rect.center().y), egui::Align2::LEFT_CENTER, "Grain", font, ink);
    let icon = egui::Rect::from_min_size(egui::pos2(right - side, rect.top()), Vec2::splat(side));
    paint_icon(ui, if locked { Icon::Lock } else { Icon::Unlock }, icon.shrink(1.0), ink);
    super::tooltip::tip(resp, label, body, None).clicked().then_some(!locked)
}

impl super::Kerf {
    /// Lands what the nesting section asked for on the canvas design, as one undo step, and hands the
    /// flow the result. Cmd+Z undoes an optimise while its summary shows. Leaving Make drops a run.
    pub(super) fn nest_requests(&mut self, ctx: &Context) {
        let Some(flow) = self.doc.flow.as_mut() else {
            self.doc.nest.running = None;
            self.doc.nest.summary = None;
            self.doc.nest.request = None;
            return;
        };
        let undoable = self.doc.nest.summary.as_ref().is_some_and(|s| s.depth == Some(self.doc.history.depth()));
        if undoable && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z))) {
            self.doc.nest.request = Some(Request::Undo);
        }
        match self.doc.nest.request.take() {
            Some(Request::Apply(layout)) => {
                let before = self.doc.design.clone();
                _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &Edit::Nesting(Some(layout)), Subject::Selection);
                self.doc.history.observe(before, &self.doc.design, false);
                if let Some(s) = &mut self.doc.nest.summary {
                    s.depth = Some(self.doc.history.depth());
                }
                flow.replace_design(self.doc.design.clone());
            }
            Some(Request::Undo) if undoable => {
                tracing::info!(target: "cmd", "undo optimise");
                self.doc.history.undo(&mut self.doc.design);
                self.doc.nest.summary = None;
                flow.replace_design(self.doc.design.clone());
            }
            Some(Request::Undo) | None => {}
        }
        if self.doc.nest.running() {
            ctx.request_repaint();
        }
    }
}

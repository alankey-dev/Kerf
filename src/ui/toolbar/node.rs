//! The node toolbar, which stands in for the edit toolbar while a path's nodes are edited: convert the
//! selected nodes between kinds, add and delete nodes, open or close the path, smooth it, and Done.

use egui::{Align2, Color32, Context, Frame, Painter, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::Design;
use crate::path_edit::{NodeAction, NodeKind, NodePath};

use super::super::chrome::{text, text_width};
use super::super::fonts;
use super::super::node_edit::NodeEdit;
use super::super::popover::{popover, titled};
use super::super::tokens::*;
use super::super::tooltip::tip;

const SMOOTH_W: f32 = 240.0;
const DEFAULT_STRENGTH: u8 = 50;

/// A toolbar control, its tooltip body and shortcut.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Action(NodeAction),
    Smoothing,
    Done,
}

const GROUPS: [&[Control]; 4] = [
    &[
        Control::Action(NodeAction::Convert(NodeKind::Straight)),
        Control::Action(NodeAction::Convert(NodeKind::Cusp)),
        Control::Action(NodeAction::Convert(NodeKind::Smooth)),
        Control::Action(NodeAction::Convert(NodeKind::Symmetric)),
    ],
    &[Control::Action(NodeAction::AddMidpoints), Control::Action(NodeAction::Remove)],
    &[Control::Action(NodeAction::OpenAt), Control::Action(NodeAction::Close)],
    &[Control::Smoothing],
];

/// The menu and toolbar name of a node action.
pub fn label(action: NodeAction) -> &'static str {
    match action {
        NodeAction::Convert(kind) => kind.label(),
        NodeAction::AddMidpoints => "Add Node",
        NodeAction::Remove => "Delete Node",
        NodeAction::OpenAt => "Open Path",
        NodeAction::Close => "Close Path",
        NodeAction::Smooth(_) => "Smoothing",
    }
}

impl Control {
    fn label(self) -> &'static str {
        match self {
            Self::Action(a) => label(a),
            Self::Smoothing => "Smoothing",
            Self::Done => "Done",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Action(NodeAction::Convert(NodeKind::Straight)) => "Make the selected nodes sharp corners with straight lines either side.",
            Self::Action(NodeAction::Convert(NodeKind::Cusp)) => "Give the selected nodes curve handles that move independently.",
            Self::Action(NodeAction::Convert(NodeKind::Smooth)) => "Keep the selected nodes' handles in a line, each its own length.",
            Self::Action(NodeAction::Convert(NodeKind::Symmetric)) => "Keep the selected nodes' handles in a line and the same length.",
            Self::Action(NodeAction::AddMidpoints) => "Add a node halfway between each pair of selected neighbours. Click a segment to add one anywhere.",
            Self::Action(NodeAction::Remove) => "Remove the selected nodes, refitting the curve between their neighbours. Option-click a node to remove it.",
            Self::Action(NodeAction::OpenAt) => "Cut a closed path open at the selected node.",
            Self::Action(NodeAction::Close) => "Join the two ends of an open path with a straight line.",
            Self::Action(NodeAction::Smooth(_)) | Self::Smoothing => "Smooth the selected nodes, or the whole path when none are selected.",
            Self::Done => "Finish editing nodes.",
        }
    }

    /// Kinds, Smoothing and Done read best with their names; the rest are icons with a tooltip, so the
    /// strip fits a laptop-width canvas.
    fn labelled(self) -> bool {
        matches!(self, Self::Action(NodeAction::Convert(_)) | Self::Smoothing | Self::Done)
    }

    fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Action(NodeAction::Remove) => Some("Delete"),
            Self::Done => Some("Esc"),
            _ => None,
        }
    }
}

/// The node toolbar for the edit in progress. Done, or the shape going away, clears `nodes`.
pub fn show(ctx: &Context, design: &mut Design, nodes: &mut Option<NodeEdit>) {
    let Some(edit) = nodes.as_mut() else { return };
    let Some(shape) = design.shapes.iter().find(|s| s.id == edit.shape) else {
        *nodes = None;
        return;
    };
    let path = NodePath::read(shape);
    let current = path.kind_of(&edit.selected);
    let (notice, refused) = (crate::path_edit::notice(shape), crate::path_edit::refusal(shape).is_some());
    let mut picked = None;
    let mut done = false;
    let mut expand = false;
    let mut preview = None;

    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: 8, right: 4, top: 4, bottom: 4 });
    let panel = egui::TopBottomPanel::top("edit-toolbar").exact_height(TOOLBAR_H).frame(frame).show_separator_line(false);
    panel.show(ctx, |ui| {
        let bar = ui.max_rect().expand2(vec2(8.0, 4.0));
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (g, group) in GROUPS.iter().enumerate().filter(|_| !refused) {
                if g > 0 {
                    super::divider(ui);
                }
                for &control in group.iter() {
                    match control {
                        Control::Action(action) => {
                            let blocked = path.check(&edit.selected, action).err();
                            let on = matches!(action, NodeAction::Convert(k) if current == Some(k));
                            if button(ui, control, blocked.map(|b| b.reason()), on).clicked() {
                                picked = Some(action);
                            }
                        }
                        Control::Smoothing => {
                            let anchor = button(ui, control, None, false);
                            let id = anchor.id.with("strength");
                            let applied = popover(ui, &anchor, "smoothing", SMOOTH_W, |ui| {
                                let mut strength = ui.data(|d| d.get_temp(id)).unwrap_or(DEFAULT_STRENGTH);
                                let applied = smoothing(ui, &mut strength, edit.selected.is_empty());
                                ui.data_mut(|d| d.insert_temp(id, strength));
                                let action = NodeAction::Smooth(strength);
                                let mut shown = path.clone();
                                if shown.apply(&edit.selected, action).is_ok() {
                                    preview = Some(shown.write(shape));
                                }
                                applied.then_some(action)
                            });
                            picked = picked.or(applied);
                        }
                        Control::Done => {}
                    }
                }
            }
            if let Some(notice) = notice {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(notice).font(fonts::regular(13.0)).color(TEXT_MUTED));
            }
            if refused {
                ui.add_space(8.0);
                expand = ui.button("Expand").clicked();
            }
            ui.add_space(ui.available_width() - width(ui, Control::Done) - 4.0);
            done = button(ui, Control::Done, None, false).clicked();
            crate::ui::anchors::mark(ui, crate::ui::anchors::Anchor::NodeToolbar, ui.min_rect());
        });
    });

    edit.preview = preview;
    if let Some(action) = picked {
        super::super::node_edit::apply(design, edit, action);
    }
    if expand {
        let id = edit.shape;
        _ = crate::edit::run(design, &mut vec![id], &crate::edit::Edit::Expand, crate::edit::Subject::Ids(&[id]));
        *nodes = None;
    }
    if done {
        *nodes = None;
    }
}

/// The Smoothing popover body. Returns true when Apply is clicked.
fn smoothing(ui: &mut Ui, strength: &mut u8, whole_path: bool) -> bool {
    titled(ui, if whole_path { "Smooth path" } else { "Smooth nodes" });
    let mut pct = f32::from(*strength);
    ui.horizontal(|ui| {
        ui.label("Strength");
        let slider = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).show_value(false));
        slider.widget_info(|| WidgetInfo { value: Some(pct.into()), ..WidgetInfo::labeled(WidgetType::Slider, true, "Smoothing strength") });
        _ = crate::units::number_field(ui, "Strength", Some(pct), "%", crate::units::FIELD_W).is_some_and(|t| t.set(&mut pct, 0.0..=100.0));
    });
    *strength = pct.round().clamp(0.0, 100.0) as u8;
    ui.add_space(8.0);
    ui.button("Apply").clicked()
}

fn width(ui: &Ui, control: Control) -> f32 {
    match control.labelled() {
        true => 8.0 + 20.0 + 4.0 + text_width(ui, control.label(), fonts::regular(14.0)) + 8.0,
        false => 8.0 + 20.0 + 8.0,
    }
}

/// The edit toolbar's `discrete-icon-button`, lit with `ACCENT_HOVER` when `on`. A blocked control is
/// greyed and its tooltip says why.
fn button(ui: &mut Ui, control: Control, blocked: Option<String>, on: bool) -> Response {
    let label = control.label();
    let enabled = blocked.is_none();
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    let (rect, resp) = ui.allocate_exact_size(vec2(width(ui, control), 32.0), if enabled { Sense::click() } else { Sense::hover() });
    let body = blocked.unwrap_or_else(|| control.tip().to_owned());
    let resp = tip(resp, label, &body, control.shortcut());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    if on || (enabled && resp.hovered()) {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    let glyph_rect = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 10.0), Vec2::splat(20.0));
    glyph(ui.painter(), control, glyph_rect, if on { ACCENT_PRIMARY } else { ink });
    if control.labelled() {
        text(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), if on { ACCENT_PRIMARY } else { ink });
    }
    resp
}

/// Line drawings in the unit square: a node with the handles its kind gives it, and so on.
fn glyph(painter: &Painter, control: Control, rect: Rect, ink: Color32) {
    let p = |x: f32, y: f32| rect.min + vec2(x, y) * rect.width();
    let stroke = Stroke::new(1.5_f32, ink);
    let thin = Stroke::new(1.0_f32, ink);
    let anchor = |x: f32, y: f32, filled: bool| {
        let r = Rect::from_center_size(p(x, y), Vec2::splat(rect.width() * 0.22));
        painter.rect(r, 0.0, if filled { ink } else { BG_PANEL }, thin, egui::StrokeKind::Middle);
    };
    let handle = |from: (f32, f32), to: (f32, f32)| {
        painter.line_segment([p(from.0, from.1), p(to.0, to.1)], thin);
        painter.circle(p(to.0, to.1), rect.width() * 0.07, BG_PANEL, thin);
    };
    let curve = |pts: &[(f32, f32)]| {
        let shape = egui::epaint::CubicBezierShape::from_points_stroke(
            [p(pts[0].0, pts[0].1), p(pts[1].0, pts[1].1), p(pts[2].0, pts[2].1), p(pts[3].0, pts[3].1)],
            false,
            Color32::TRANSPARENT,
            stroke,
        );
        painter.add(shape);
    };
    match control {
        Control::Action(NodeAction::Convert(NodeKind::Straight)) => {
            painter.line_segment([p(0.08, 0.85), p(0.5, 0.3)], stroke);
            painter.line_segment([p(0.5, 0.3), p(0.92, 0.85)], stroke);
            anchor(0.5, 0.3, true);
        }
        Control::Action(NodeAction::Convert(NodeKind::Cusp)) => {
            curve(&[(0.05, 0.85), (0.15, 0.5), (0.3, 0.35), (0.5, 0.45)]);
            curve(&[(0.5, 0.45), (0.65, 0.3), (0.9, 0.5), (0.95, 0.85)]);
            handle((0.5, 0.45), (0.22, 0.12));
            handle((0.5, 0.45), (0.85, 0.15));
            anchor(0.5, 0.45, true);
        }
        Control::Action(NodeAction::Convert(kind @ (NodeKind::Smooth | NodeKind::Symmetric))) => {
            curve(&[(0.05, 0.85), (0.15, 0.4), (0.3, 0.45), (0.5, 0.45)]);
            curve(&[(0.5, 0.45), (0.7, 0.45), (0.85, 0.4), (0.95, 0.85)]);
            let left = if kind == NodeKind::Symmetric { 0.15 } else { 0.28 };
            handle((0.5, 0.45), (left, 0.45));
            handle((0.5, 0.45), (0.85, 0.45));
            anchor(0.5, 0.45, true);
        }
        Control::Action(NodeAction::AddMidpoints | NodeAction::Remove) => {
            painter.line_segment([p(0.05, 0.6), p(0.95, 0.6)], stroke);
            anchor(0.35, 0.6, false);
            let c = p(0.75, 0.25);
            let h = rect.width() * 0.16;
            painter.line_segment([c - vec2(h, 0.0), c + vec2(h, 0.0)], stroke);
            if control == Control::Action(NodeAction::AddMidpoints) {
                painter.line_segment([c - vec2(0.0, h), c + vec2(0.0, h)], stroke);
            }
        }
        Control::Action(NodeAction::OpenAt) => {
            painter.add(egui::Shape::line(vec![p(0.42, 0.12), p(0.12, 0.12), p(0.12, 0.88), p(0.88, 0.88), p(0.88, 0.42)], stroke));
            anchor(0.42, 0.12, true);
            anchor(0.88, 0.42, true);
        }
        Control::Action(NodeAction::Close) => {
            painter.add(egui::Shape::closed_line(vec![p(0.12, 0.12), p(0.88, 0.12), p(0.88, 0.88), p(0.12, 0.88)], stroke));
            anchor(0.88, 0.12, true);
        }
        Control::Action(NodeAction::Smooth(_)) | Control::Smoothing => {
            let wiggle: Vec<_> = (0..=8).map(|i| p(0.08 + 0.105 * i as f32, if i % 2 == 0 { 0.62 } else { 0.48 })).collect();
            painter.add(egui::Shape::line(wiggle, thin));
            curve(&[(0.08, 0.38), (0.35, 0.2), (0.65, 0.2), (0.92, 0.38)]);
        }
        Control::Done => {
            painter.add(egui::Shape::line(vec![p(0.18, 0.52), p(0.42, 0.75), p(0.84, 0.28)], stroke));
        }
    }
}

//! The recovery dialog after an unclean exit: each project left with unsaved changes, with its
//! thumbnail, name and time, to restore or discard.

use std::sync::Arc;

use egui::{Align2, Area, Color32, Context, Frame, Id, Image, Order, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, load::Bytes, pos2, vec2};

use super::autosave::Clock;
use super::chrome::text;
use super::tokens::*;
use super::{fonts, modal};
use crate::store::session::Recoverable;

const WIDTH: f32 = 420.0;
const ROW_H: f32 = 64.0;
const THUMB: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    RestoreAll,
    RestoreSelected,
    Discard,
}

struct Row {
    unsaved: Recoverable,
    chosen: bool,
    thumb: Option<Arc<[u8]>>,
}

pub(super) struct Dialog {
    rows: Vec<Row>,
}

impl Dialog {
    /// `None` when there is nothing to recover. Thumbnails draw images from the store under `projects`.
    pub(super) fn new(list: Vec<Recoverable>, projects: &std::path::Path) -> Option<Self> {
        let assets = crate::assets::dir(projects);
        let thumb = |design: &crate::design::Design| crate::project::render_thumbnail(design, 220, &crate::assets::thumbnail_pixels(&assets, design)).map(Arc::from);
        let rows = list.into_iter().map(|unsaved| Row { thumb: thumb(&unsaved.project.design), chosen: true, unsaved }).collect::<Vec<_>>();
        (!rows.is_empty()).then_some(Self { rows })
    }

    /// The ids waiting on an answer, which are open in all but name.
    pub(super) fn ids(&self) -> impl Iterator<Item = &str> {
        self.rows.iter().map(|r| r.unsaved.project.id.as_str())
    }

    /// The entries to restore, in their session order, and the ones to discard.
    pub(super) fn resolve(self, answer: Answer) -> (Vec<Recoverable>, Vec<Recoverable>) {
        let (chosen, rest): (Vec<Row>, Vec<Row>) = self.rows.into_iter().partition(|r| match answer {
            Answer::RestoreAll => true,
            Answer::RestoreSelected => r.chosen,
            Answer::Discard => false,
        });
        (chosen.into_iter().map(|r| r.unsaved).collect(), rest.into_iter().map(|r| r.unsaved).collect())
    }

    pub(super) fn show(&mut self, ctx: &Context, clock: &Clock) -> Option<Answer> {
        let screen = ctx.screen_rect();
        Area::new(Id::new("recovery-scrim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(28));
        });
        let frame = Frame::NONE.fill(BG_PANEL).corner_radius(8).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).inner_margin(egui::Margin::same(20)).shadow(egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: SHADOW,
        });
        let mut answer = None;
        Area::new(Id::new("recovery")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(WIDTH);
                ui.label(egui::RichText::new("Kerf didn't quit properly").font(fonts::semibold(15.0)).color(TEXT_PRIMARY));
                ui.add_space(6.0);
                let detail = "These projects have changes that were not saved. Restore the ones you want to keep.";
                ui.label(egui::RichText::new(detail).font(fonts::regular(13.0)).color(TEXT_MUTED));
                ui.add_space(12.0);
                for row in &mut self.rows {
                    row.show(ui, clock);
                }
                ui.add_space(16.0);
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                let buttons = [
                    (Answer::Discard, "Discard", 80.0, false),
                    (Answer::RestoreSelected, "Restore selected", 132.0, false),
                    (Answer::RestoreAll, "Restore all", 104.0, true),
                ];
                let mut right = bar.right();
                for (choice, label, w, primary) in buttons.into_iter().rev() {
                    let rect = Rect::from_min_size(pos2(right - w, bar.top()), vec2(w, 28.0));
                    if modal::button(ui, rect, label, label, primary) {
                        answer = Some(choice);
                    }
                    right -= w + 8.0;
                }
            });
        });
        answer
    }
}

impl Row {
    fn show(&mut self, ui: &mut Ui, clock: &Clock) {
        let name = &self.unsaved.project.name;
        let when = format!("Unsaved changes from {}", clock.time(self.unsaved.modified * 1000));
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, self.chosen, format!("Restore {name}, {}", when.to_lowercase())));
        if resp.clicked() {
            self.chosen = !self.chosen;
        }
        let painter = ui.painter();
        painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
        let check = Rect::from_center_size(pos2(rect.left() + 9.0, rect.center().y), Vec2::splat(16.0));
        let (fill, edge) = if self.chosen { (ACCENT_PRIMARY, ACCENT_PRIMARY) } else { (BG_PANEL, TEXT_MUTED) };
        painter.rect(check, 3, fill, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
        if self.chosen {
            let tick = [check.left_center() + vec2(3.5, 0.5), check.center_bottom() + vec2(-1.0, -4.0), check.right_top() + vec2(-3.5, 4.0)];
            painter.line(tick.to_vec(), Stroke::new(2.0_f32, ON_PRIMARY));
        }
        let thumb = Rect::from_min_size(pos2(check.right() + 12.0, rect.top() + (ROW_H - THUMB) / 2.0), Vec2::splat(THUMB));
        painter.rect(thumb, 4, BG_CANVAS_VOID, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
        if let Some(png) = &self.thumb {
            let uri = format!("bytes://recovery/{}-{}.png", self.unsaved.project.id, self.unsaved.modified);
            Image::new((uri, Bytes::Shared(png.clone()))).max_size(Vec2::splat(THUMB - 4.0)).paint_at(ui, thumb.shrink(2.0));
        }
        let x = thumb.right() + 12.0;
        let painter = ui.painter();
        text(painter, pos2(x, rect.top() + 14.0), Align2::LEFT_TOP, name, fonts::medium(14.0), TEXT_PRIMARY);
        text(painter, pos2(x, rect.top() + 34.0), Align2::LEFT_TOP, &when, fonts::regular(12.0), TEXT_MUTED);
    }
}

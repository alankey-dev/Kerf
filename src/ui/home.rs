use egui::{Align2, Color32, Context, CornerRadius, Frame, Image, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, load::Bytes, pos2, vec2};

use std::sync::Arc;

use crate::project::{Entry, Project, Thumbnail};
use crate::store::unsaved::Unsaved;

use super::chrome::text;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::modal::{self, Choice};
use super::tokens::*;

const CARD: Vec2 = vec2(220.0, 226.0);
const THUMB_H: f32 = 140.0;
const GAP: f32 = 20.0;

#[derive(Debug)]
pub enum HomeEvent {
    New,
    Edit(String),
    Make(String),
    /// A closed never-saved design: open it, offering its autosave back.
    OpenUnsaved(String),
    /// Confirmed: remove this unsaved design's autosave.
    DeleteUnsaved(String),
    /// Confirmed: remove this project's files.
    Delete(String),
    /// Confirmed: remove every project.
    ClearAll,
}

/// A deletion waiting on the user's yes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    One {
        id: String,
        name: String,
    },
    /// An unsaved design or an unreadable autosave: there is no project file to remove.
    Unsaved {
        id: String,
        name: String,
    },
    All,
}

impl Confirm {
    fn title(&self) -> String {
        match self {
            Self::One { name, .. } | Self::Unsaved { name, .. } => format!("Delete {name}?"),
            Self::All => "Delete all projects?".into(),
        }
    }
}

/// The Home tab: a New Project card, one card per closed never-saved design marked "Unsaved", then
/// one per project file, each with a trash button. A file that would not load gets a "Couldn't open"
/// card. Deleting asks first through `confirm`, which the caller keeps between frames. `now` (unix
/// seconds) dates the cards.
pub fn show(ctx: &Context, entries: &[Entry], unsaved: &[Unsaved], confirm: &mut Option<Confirm>, now: u64) -> Option<HomeEvent> {
    let frame = Frame::NONE.fill(BG_CANVAS_VOID).inner_margin(egui::Margin::same(32));
    let mut event = egui::CentralPanel::default()
        .frame(frame)
        .show(ctx, |ui| {
            let top = ui.max_rect().left_top();
            text(ui.painter(), top + vec2(0.0, 12.0), Align2::LEFT_TOP, "Your Projects", fonts::display(22.0), TEXT_PRIMARY);
            if !entries.is_empty() {
                let clear = Rect::from_min_size(pos2(ui.max_rect().right() - 90.0, top.y + 8.0), vec2(90.0, 28.0));
                if modal::button(ui, clear, "Clear all", "Clear all projects", false) {
                    *confirm = Some(Confirm::All);
                }
            }
            ui.add_space(56.0);
            let mut event = None;
            let grid = egui::ScrollArea::vertical().show(ui, |ui| {
                let cols = ((ui.available_width() + GAP) / (CARD.x + GAP)).floor().max(1.0) as usize;
                ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                let mut i = 0;
                let total = 1 + unsaved.len() + entries.len();
                while i < total {
                    ui.horizontal(|ui| {
                        for _ in 0..cols {
                            let card = match i {
                                0 => new_card(ui).then_some(HomeEvent::New),
                                n if n <= unsaved.len() => match &unsaved[n - 1] {
                                    Unsaved::Ready { project, thumbnail } => unsaved_card(ui, project, thumbnail.as_ref(), confirm, now),
                                    Unsaved::Unreadable { id, reason } => {
                                        unreadable_card(ui, id, &format!("autosave/{id}.json"), reason, |id, name| Confirm::Unsaved { id, name }, confirm)
                                    }
                                },
                                n => match &entries[n - 1 - unsaved.len()] {
                                    Entry::Ready { project, thumbnail } => project_card(ui, project, thumbnail.as_ref(), confirm, now),
                                    Entry::Unreadable { id, reason } => {
                                        unreadable_card(ui, id, &format!("{id}.json"), reason, |id, name| Confirm::One { id, name }, confirm)
                                    }
                                },
                            };
                            event = event.take().or(card);
                            i += 1;
                            if i >= total {
                                break;
                            }
                        }
                    });
                }
            });
            let bottom = (grid.inner_rect.top() + grid.content_size.y).min(grid.inner_rect.bottom());
            super::anchors::mark(ui, super::anchors::Anchor::HomeProjects, Rect::from_min_max(top, pos2(ui.max_rect().right(), bottom)));
            event
        })
        .inner;
    if let Some(pending) = confirm.clone()
        && let Some(answer) = confirm_dialog(ctx, &pending)
    {
        *confirm = None;
        if answer == DELETE {
            event = Some(match pending {
                Confirm::One { id, .. } => HomeEvent::Delete(id),
                Confirm::Unsaved { id, .. } => HomeEvent::DeleteUnsaved(id),
                Confirm::All => HomeEvent::ClearAll,
            });
        }
    }
    event
}

const DELETE: usize = 1;

fn confirm_dialog(ctx: &Context, pending: &Confirm) -> Option<usize> {
    let detail = match pending {
        Confirm::One { .. } => "This removes the project and its thumbnail. It cannot be undone.",
        Confirm::Unsaved { .. } => "This removes the unsaved changes. It cannot be undone.",
        Confirm::All => "This removes every saved project and thumbnail. It cannot be undone.",
    };
    let choices = [Choice { label: "Cancel", a11y: "Cancel delete", primary: false }, Choice { label: "Delete", a11y: "Confirm delete", primary: true }];
    modal::show(ctx, "home-confirm", &pending.title(), detail, &choices, 0)
}

fn new_card(ui: &mut Ui) -> bool {
    let (rect, resp) = ui.allocate_exact_size(CARD, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "New Project"));
    super::anchors::mark(ui, super::anchors::Anchor::HomeNew, rect);
    let fill = if resp.hovered() { ACCENT_HOVER } else { BG_PANEL };
    ui.painter().rect(rect, 10, fill, Stroke::new(1.5_f32, ACCENT_PRIMARY), StrokeKind::Inside);
    let c = rect.center();
    ui.painter().circle(c - vec2(0.0, 14.0), 22.0, Color32::WHITE, Stroke::new(2.0_f32, ACCENT_PRIMARY));
    ui.painter().line_segment([c - vec2(10.0, 14.0), c + vec2(10.0, -14.0)], Stroke::new(2.0_f32, ACCENT_PRIMARY));
    ui.painter().line_segment([c - vec2(0.0, 24.0), c - vec2(0.0, 4.0)], Stroke::new(2.0_f32, ACCENT_PRIMARY));
    text(ui.painter(), c + vec2(0.0, 28.0), Align2::CENTER_CENTER, "New Project", fonts::medium(15.0), ACCENT_PRIMARY);
    resp.clicked()
}

/// The card body every project card shares: a frame with a thumbnail area on top. Returns the card's
/// rect and its thumbnail area.
fn card_frame(ui: &mut Ui) -> (Rect, Rect) {
    let (rect, resp) = ui.allocate_exact_size(CARD, Sense::hover());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect.expand(1.0).translate(vec2(0.0, 2.0)), 11.0, Color32::from_black_alpha(26));
    }
    painter.rect(rect, 10, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), THUMB_H));
    painter.rect_filled(thumb, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, BG_CANVAS_VOID);
    (rect, thumb)
}

fn button_row(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.left() + 12.0, rect.bottom() - 30.0), pos2(rect.right() - 12.0, rect.bottom() - 9.0))
}

fn project_card(ui: &mut Ui, p: &Project, thumbnail: Option<&Thumbnail>, confirm: &mut Option<Confirm>, now: u64) -> Option<HomeEvent> {
    let (rect, thumb) = card_frame(ui);
    if let Some(t) = thumbnail {
        let uri = format!("bytes://thumb/{}-{:x}.png", p.id, t.key);
        Image::new((uri, Bytes::Shared(t.png.clone()))).max_size(thumb.size() - Vec2::splat(16.0)).paint_at(ui, thumb.shrink(8.0));
    }
    let painter = ui.painter();
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 14.0), Align2::LEFT_TOP, &p.name, fonts::medium(15.0), TEXT_PRIMARY);
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 36.0), Align2::LEFT_TOP, &relative_time(p.modified, now), fonts::regular(12.0), TEXT_MUTED);

    let mut event = None;
    let row = button_row(rect);
    if modal::button(ui, Rect::from_min_size(row.min, vec2(70.0, row.height())), "Edit", "Edit", true) {
        event = Some(HomeEvent::Edit(p.id.clone()));
    }
    if modal::button(ui, Rect::from_min_size(pos2(row.left() + 78.0, row.top()), vec2(70.0, row.height())), "Make", "Make", false) {
        event = Some(HomeEvent::Make(p.id.clone()));
    }
    if trash_button(ui, Rect::from_center_size(pos2(rect.right() - 20.0, row.center().y), Vec2::splat(24.0)), &p.name, "Delete project") {
        *confirm = Some(Confirm::One { id: p.id.clone(), name: p.name.clone() });
    }
    event
}

/// A closed never-saved design: its autosave's picture, name and age under an "Unsaved" mark, with
/// Open, which offers Restore or Discard, and Delete.
fn unsaved_card(ui: &mut Ui, p: &Project, thumbnail: Option<&Arc<[u8]>>, confirm: &mut Option<Confirm>, now: u64) -> Option<HomeEvent> {
    let (rect, thumb) = card_frame(ui);
    if let Some(png) = thumbnail {
        let uri = format!("bytes://unsaved/{}-{}.png", p.id, p.modified);
        Image::new((uri, Bytes::Shared(png.clone()))).max_size(thumb.size() - Vec2::splat(16.0)).paint_at(ui, thumb.shrink(8.0));
    }
    let badge = Rect::from_min_size(thumb.left_top() + vec2(8.0, 8.0), vec2(64.0, 20.0));
    let painter = ui.painter();
    painter.rect(badge, 10, WARNING_BG, Stroke::new(1.0_f32, WARNING_TEXT), StrokeKind::Inside);
    text(painter, badge.center(), Align2::CENTER_CENTER, "Unsaved", fonts::medium(12.0), WARNING_TEXT);
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 14.0), Align2::LEFT_TOP, &p.name, fonts::medium(15.0), TEXT_PRIMARY);
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 36.0), Align2::LEFT_TOP, &relative_time(p.modified, now), fonts::regular(12.0), TEXT_MUTED);

    let mut event = None;
    let row = button_row(rect);
    if modal::button(ui, Rect::from_min_size(row.min, vec2(70.0, row.height())), "Open", &format!("Open unsaved {}", p.name), true) {
        event = Some(HomeEvent::OpenUnsaved(p.id.clone()));
    }
    if trash_button(ui, Rect::from_center_size(pos2(rect.right() - 20.0, row.center().y), Vec2::splat(24.0)), &p.name, "Delete unsaved work") {
        *confirm = Some(Confirm::Unsaved { id: p.id.clone(), name: p.name.clone() });
    }
    event
}

/// A file that would not load: its name and why, with only Delete on offer. `ask` builds the
/// confirmation for the file's kind.
fn unreadable_card(ui: &mut Ui, id: &str, file: &str, reason: &str, ask: impl Fn(String, String) -> Confirm, confirm: &mut Option<Confirm>) -> Option<HomeEvent> {
    let (rect, thumb) = card_frame(ui);
    let resp = ui.interact(rect, ui.id().with(("unreadable", file)), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("Couldn't open {file}: {reason}")));
    let painter = ui.painter();
    text(painter, thumb.center(), Align2::CENTER_CENTER, "Couldn't open", fonts::medium(15.0), WARNING_TEXT);
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 14.0), Align2::LEFT_TOP, file, fonts::medium(13.0), TEXT_PRIMARY);
    let short: String = reason.chars().take(32).collect();
    text(painter, rect.left_top() + vec2(12.0, THUMB_H + 36.0), Align2::LEFT_TOP, &short, fonts::regular(12.0), TEXT_MUTED);
    let row = button_row(rect);
    if trash_button(ui, Rect::from_center_size(pos2(rect.right() - 20.0, row.center().y), Vec2::splat(24.0)), file, "Delete project") {
        *confirm = Some(ask(id.to_owned(), file.to_owned()));
    }
    None
}

/// A flat trash icon button; accent_hover wash on hover. `tip` names what it deletes.
fn trash_button(ui: &mut Ui, rect: Rect, name: &str, tip: &str) -> bool {
    let resp = ui.interact(rect, ui.id().with(("home-trash", name, rect.left() as i32)), Sense::click());
    let resp = super::tooltip::tip(resp, tip, "", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Delete {name}")));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, ACCENT_HOVER);
    }
    let ink = if resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED };
    paint_icon(ui, Icon::Trash, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), ink);
    resp.clicked()
}

fn relative_time(modified: u64, now: u64) -> String {
    let secs = now.saturating_sub(modified);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} hr ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

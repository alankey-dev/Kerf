//! File > Version history: a right panel listing the open project's versions newest first, grouped
//! by day. Selecting one previews it on the canvas; Restore makes it the design; Name pins it.

use std::path::Path;
use std::sync::Arc;

use egui::{Align2, Context, Image, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, load::Bytes, pos2, vec2};

use super::autosave::Clock;
use super::chrome::text;
use super::tokens::*;
use super::{flow_kit, fonts, modal};
use crate::design::Design;
use crate::store::{self, Version, VersionId, VersionKind};

const WIDTH: f32 = 300.0;
const ROW_H: f32 = 64.0;
const THUMB: f32 = 48.0;
const NAME_HINT: &str = "Name this version";

struct Row {
    version: Version,
    thumb: Option<(Arc<[u8]>, u64)>,
}

pub(super) struct Panel {
    id: String,
    /// Newest first.
    rows: Vec<Row>,
    selected: Option<VersionId>,
    /// The selected version's design, shown read-only on the canvas.
    pub(super) preview: Option<Design>,
    /// The row being named and the name typed so far.
    naming: Option<(VersionId, String)>,
    /// Set when naming starts, so the field takes focus the frame it first shows.
    focus_name: bool,
}

#[derive(Debug)]
pub(super) enum Event {
    Select(VersionId),
    Restore(VersionId),
    Name(VersionId, String),
    Close,
}

fn kind_label(kind: VersionKind) -> &'static str {
    match kind {
        VersionKind::Manual => "Saved",
        VersionKind::Auto => "Autosave",
        VersionKind::BeforeRestore => "Before restore",
    }
}

impl Panel {
    pub(super) fn load(dir: &Path, id: &str) -> std::io::Result<Self> {
        let index = store::read_index(dir, id)?;
        let thumb = |v: VersionId| {
            let png = std::fs::read(store::version_png(dir, id, v)).ok()?;
            Some((Arc::from(png), v.0))
        };
        let rows = index.versions.into_iter().rev().map(|version| Row { thumb: thumb(version.id), version }).collect();
        Ok(Self { id: id.to_owned(), rows, selected: None, preview: None, naming: None, focus_name: false })
    }

    pub(super) fn select(&mut self, version: VersionId, design: Design) {
        self.selected = Some(version);
        self.preview = Some(design);
        self.naming = None;
    }

    pub(super) fn name(&mut self, version: VersionId, name: &str) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.version.id == version) {
            row.version.name = Some(name.to_owned());
            row.version.pinned = true;
        }
        self.naming = None;
    }

    pub(super) fn show(&mut self, ctx: &Context, clock: &Clock) -> Option<Event> {
        let frame = egui::Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin::same(16));
        let panel = egui::SidePanel::right("versions").exact_width(WIDTH).resizable(false).frame(frame).show_separator_line(false);
        panel
            .show(ctx, |ui| {
                let full = ui.max_rect().expand(16.0);
                ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
                flow_kit::themed(ui, |ui| self.body(ui, clock))
            })
            .inner
    }

    fn body(&mut self, ui: &mut Ui, clock: &Clock) -> Option<Event> {
        let mut event = None;
        let header = ui.max_rect();
        flow_kit::panel_title(ui, "Version history");
        let close = Rect::from_min_size(pos2(header.right() - 64.0, header.top() - 4.0), vec2(64.0, 26.0));
        if modal::button(ui, close, "Close", "Close version history", false) {
            event = Some(Event::Close);
        }
        if self.rows.is_empty() {
            flow_kit::body_text(ui, "Versions appear here once the project is saved.", true);
            return event;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut day = None;
            for i in 0..self.rows.len() {
                let at = self.rows[i].version.id.0;
                let heading = clock.day(at);
                if day.as_ref() != Some(&heading) {
                    ui.add_space(if day.is_some() { 12.0 } else { 4.0 });
                    ui.label(egui::RichText::new(&heading).font(fonts::semibold(12.0)).color(TEXT_MUTED));
                    ui.add_space(4.0);
                    day = Some(heading);
                }
                event = event.take().or(self.row(ui, i, clock));
            }
        });
        event
    }

    fn row(&mut self, ui: &mut Ui, i: usize, clock: &Clock) -> Option<Event> {
        let row = &self.rows[i];
        let v = row.version.id;
        let selected = self.selected == Some(v);
        let time = clock.time(v.0);
        let kind = kind_label(row.version.kind);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, format!("Version {time} {kind}")));
        let painter = ui.painter();
        let (fill, stroke) = match (selected, resp.hovered()) {
            (true, _) => (ACCENT_HOVER, ACCENT_PRIMARY),
            (false, true) => (BG_CANVAS_VOID, BORDER_SUBTLE),
            (false, false) => (BG_PANEL, BG_PANEL),
        };
        painter.rect(rect, 6, fill, Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
        let thumb = Rect::from_min_size(rect.left_top() + vec2(8.0, (ROW_H - THUMB) / 2.0), Vec2::splat(THUMB));
        painter.rect(thumb, 4, BG_PANEL, Stroke::new(1.0_f32, BORDER_SUBTLE), StrokeKind::Inside);
        if let Some((png, key)) = &row.thumb {
            let uri = format!("bytes://version/{}-{key:x}.png", self.id);
            Image::new((uri, Bytes::Shared(png.clone()))).max_size(Vec2::splat(THUMB - 4.0)).paint_at(ui, thumb.shrink(2.0));
        }
        let x = thumb.right() + 12.0;
        let painter = ui.painter();
        text(painter, pos2(x, rect.top() + 12.0), Align2::LEFT_TOP, &time, fonts::medium(13.0), TEXT_PRIMARY);
        text(painter, pos2(x + 48.0, rect.top() + 13.0), Align2::LEFT_TOP, kind, fonts::regular(12.0), TEXT_MUTED);
        if let Some(name) = &row.version.name {
            text(painter, pos2(x, rect.top() + 34.0), Align2::LEFT_TOP, name, fonts::regular(12.0), ACCENT_PRIMARY);
        }
        let mut event = resp.clicked().then_some(Event::Select(v));
        if selected {
            event = event.or(self.actions(ui, v));
        }
        event
    }

    fn actions(&mut self, ui: &mut Ui, v: VersionId) -> Option<Event> {
        ui.add_space(6.0);
        let mut event = None;
        if let Some((_, name)) = self.naming.as_mut().filter(|(at, _)| *at == v) {
            let edit = ui.add(egui::TextEdit::singleline(name).hint_text(NAME_HINT).desired_width(f32::INFINITY));
            edit.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, NAME_HINT));
            if std::mem::take(&mut self.focus_name) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() {
                event = Some(Event::Name(v, name.trim().to_owned()));
            }
            ui.add_space(6.0);
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
        if modal::button(ui, Rect::from_min_size(bar.min, vec2(96.0, 28.0)), "Restore", "Restore version", true) {
            event = Some(Event::Restore(v));
        }
        if modal::button(ui, Rect::from_min_size(bar.min + vec2(104.0, 0.0), vec2(80.0, 28.0)), "Name", "Name version", false) {
            let current = self.rows.iter().find(|r| r.version.id == v).and_then(|r| r.version.name.clone()).unwrap_or_default();
            self.naming = Some((v, current));
            self.focus_name = true;
        }
        ui.add_space(8.0);
        event
    }
}

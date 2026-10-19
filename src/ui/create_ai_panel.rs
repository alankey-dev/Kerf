//! The Create AI panel (spec 43), beside the rail: a prompt, a style and a level of detail; four tiles
//! from the chosen model; Insert, More like this and Regenerate; and a History tab of past designs,
//! which insert offline. The footer names the service and model and opens the Create AI settings
//! window ([`settings`]), where the user picks the service (Anthropic, OpenAI, OpenRouter or a local
//! server), its key and its model. A service that needs a key and has none says what is sent and to
//! whom, and offers the window.

mod settings;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use egui::{Align, Align2, Color32, Context, Frame, Layout, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use super::chrome::text;
use super::flow_kit;
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;
use super::tooltip::tip;
use crate::ai::anthropic::AnthropicProvider;
use crate::ai::history::{self, Entry};
use crate::ai::keys::Keys;
use crate::ai::openai::OpenAiProvider;
use crate::ai::{AiError, CANDIDATES, Candidate, Detail, GenerateRequest, Job, Listing, PROMPT_MAX_CHARS, Provider, Service, Style};
use crate::design::{Shape, ShapeKind};
use crate::geometry::{FLATTEN_TOL_IN, Ring};
use crate::settings::AiSettings;

const GUTTER: f32 = 4.0;
const GAP: f32 = 10.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Create,
    History,
}

pub enum Event {
    /// Add `piece` (at the origin, 4 in wide) to the canvas as one shape named `title`.
    Insert {
        title: String,
        piece: Box<Shape>,
    },
    /// The service, a model name or the local address changed; save the settings.
    SettingsChanged,
    Close,
}

/// A design ready to draw in a tile: its outline as rings in a `size`-inch frame.
struct Tile {
    candidate: Candidate,
    rings: Vec<Ring>,
    size: (f32, f32),
}

impl Tile {
    fn of(candidate: Candidate) -> Self {
        let (rings, size) = match candidate.piece.as_ref().map(|p| (&p.kind, (p.width_in, p.height_in))) {
            Some((ShapeKind::Imported { contours, .. }, size)) => {
                (contours.iter().map(|c| crate::geometry::flatten(c, FLATTEN_TOL_IN)).filter(|r| r.len() >= 3).collect(), size)
            }
            _ => (Vec::new(), (1.0, 1.0)),
        };
        Self { candidate, rings, size }
    }

    fn ready(&self) -> bool {
        self.candidate.piece.is_some()
    }
}

/// The app keeps one for the session, so a prompt and its results survive closing the panel.
pub struct CreateAi {
    pub open: bool,
    tab: Tab,
    prompt: String,
    style: Style,
    detail: Detail,
    key_field: String,
    key_note: Option<String>,
    /// The service, models and local address, saved in `settings.json`.
    pub ai: AiSettings,
    keys: Keys,
    /// Answers in place of the real services: set in tests, so no test reaches the network.
    provider: Option<Arc<dyn Provider>>,
    /// The running job, its request and the service it went to.
    job: Option<(Job, GenerateRequest, Service)>,
    /// The last request that finished, for Regenerate and the history entries.
    last: Option<GenerateRequest>,
    tiles: Vec<Tile>,
    selected: Option<usize>,
    /// The last failure and the service that failed.
    error: Option<(AiError, Service)>,
    /// Loaded when the History tab first shows, and again after each generation.
    history: Option<Vec<(Entry, Tile)>>,
    /// The Create AI settings window is showing.
    pub settings_open: bool,
    /// The settings window shows the key being typed.
    show_key: bool,
    /// A model list being fetched for the settings window, and the service asked.
    listing: Option<(Listing, Service)>,
    /// The last model list, or why there is none.
    models: Option<settings::Models>,
}

impl CreateAi {
    /// The live panel: the keychain, the services' environment variables, and the real services.
    #[cfg(not(test))]
    pub fn new(ai: AiSettings) -> Self {
        Self::with(ai, Keys::system(), None)
    }

    /// Tests: an in-memory key store, no environment keys, and a fake provider, so no test reads the
    /// keychain or reaches the network.
    #[cfg(test)]
    pub fn new(ai: AiSettings) -> Self {
        let keys = Keys::new(Box::new(crate::ai::keys::MemoryKeys::default()), |_| None);
        Self::with(ai, keys, Some(Arc::new(crate::ai::FakeProvider::default())))
    }

    /// Answers from a fresh fake the test can script.
    #[cfg(test)]
    pub fn use_fake(&mut self) -> Arc<crate::ai::FakeProvider> {
        let fake = Arc::new(crate::ai::FakeProvider::default());
        self.provider = Some(fake.clone());
        fake
    }

    /// Saves `key` for the chosen service, as the Save key button does.
    #[cfg(test)]
    pub fn save_key_for_test(&mut self, key: &str) {
        self.keys.save(self.ai.service, key).unwrap();
    }

    fn with(ai: AiSettings, keys: Keys, provider: Option<Arc<dyn Provider>>) -> Self {
        Self {
            open: false,
            tab: Tab::Create,
            prompt: String::new(),
            style: Style::default(),
            detail: Detail::default(),
            key_field: String::new(),
            key_note: None,
            ai,
            keys,
            provider,
            job: None,
            last: None,
            tiles: Vec::new(),
            selected: None,
            error: None,
            history: None,
            settings_open: false,
            show_key: false,
            listing: None,
            models: None,
        }
    }

    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    #[cfg(test)]
    pub fn ready_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.ready()).count()
    }

    #[cfg(test)]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    #[cfg(test)]
    pub fn has_key(&mut self) -> bool {
        self.keys.current(self.ai.service).is_some()
    }

    /// Picks up a finished generation. Called every frame, open or not; while a job runs it keeps
    /// the frames coming so the answer shows as soon as it lands.
    pub fn poll(&mut self, ctx: &Context, projects_dir: Option<&Path>) {
        let Some((job, ..)) = &self.job else { return };
        let Some(result) = job.try_result() else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let Some((_, request, service)) = self.job.take() else { return };
        match result {
            Ok(drawn) => {
                self.tiles = drawn.into_iter().map(Tile::of).collect();
                self.selected = self.tiles.iter().position(Tile::ready);
                if let Some(dir) = projects_dir {
                    self.remember(dir, &request);
                }
            }
            Err(e) => {
                tracing::info!(target: "ai", error = ?e, ?service, "generation failed");
                self.error = Some((e, service));
            }
        }
        self.last = Some(request);
        ctx.request_repaint();
    }

    /// Saves every cut-ready design to the history.
    fn remember(&mut self, projects_dir: &Path, request: &GenerateRequest) {
        let created = jiff::Timestamp::now().as_millisecond();
        for (i, tile) in self.tiles.iter().enumerate().filter(|(_, t)| t.ready()) {
            let c = &tile.candidate;
            let entry = Entry {
                id: format!("{created}-{i}"),
                prompt: request.prompt.clone(),
                style: request.style,
                detail: request.detail,
                created,
                svg: c.svg.clone(),
                title: c.title.clone(),
            };
            if let Err(e) = history::save(projects_dir, &entry) {
                tracing::error!(target: "error", error = %e, "saving a Create AI design failed");
            }
        }
        self.history = None;
    }

    fn start(&mut self, request: GenerateRequest) {
        let service = self.ai.service;
        let Some(provider) = self.provider.clone().or_else(|| self.live(service)) else { return };
        self.tiles.clear();
        self.selected = None;
        self.error = None;
        tracing::info!(target: "ai", ?service, model = %self.ai.model(service), "service");
        self.job = Some((Job::start(provider, request.clone()), request, service));
    }

    /// The real client for `service`, when it has what it needs.
    fn live(&mut self, service: Service) -> Option<Arc<dyn Provider>> {
        let model = self.ai.model(service).to_owned();
        let key = self.keys.current(service).map(|(k, _)| k);
        Some(match (service, key) {
            (Service::Anthropic, Some(key)) => Arc::new(AnthropicProvider::new(key, model)),
            (Service::OpenAi, Some(key)) => Arc::new(OpenAiProvider::openai(key, model)),
            (Service::OpenRouter, Some(key)) => Arc::new(OpenAiProvider::openrouter(key, model)),
            (Service::Local, _) => Arc::new(OpenAiProvider::local(&self.ai.local_address, model)),
            (_, None) => return None,
        })
    }

    /// Drops the running job; an answer that still arrives goes nowhere.
    fn cancel(&mut self) {
        tracing::info!(target: "ai", "cancelled");
        self.job = None;
    }

    pub fn show(&mut self, ctx: &Context, projects_dir: Option<&Path>) -> Option<Event> {
        let frame = Frame::NONE.fill(Color32::WHITE).inner_margin(egui::Margin { left: 15, right: 16, top: 0, bottom: 12 });
        let panel = egui::SidePanel::left("create-ai-panel").exact_width(SHAPES_PANEL_W + GUTTER).resizable(false).frame(frame).show_separator_line(false);
        panel
            .show(ctx, |ui| {
                let full = ui.max_rect();
                ui.painter().vline(full.right() + 16.0 - GUTTER / 2.0, full.y_range().expand(12.0), Stroke::new(1.0_f32, BORDER_SUBTLE));
                ui.set_width(SHAPES_PANEL_W - 31.0);
                let mut event = self.header(ui);
                egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    let service = self.ai.service;
                    let body = match (self.tab, self.keys.current(service)) {
                        (Tab::History, _) => self.history_tab(ui, projects_dir),
                        (Tab::Create, None) if service.needs_key() => {
                            self.setup(ui, service);
                            None
                        }
                        (Tab::Create, _) => self.create_tab(ui),
                    };
                    event = event.take().or(body);
                });
                event
            })
            .inner
    }

    fn header(&mut self, ui: &mut Ui) -> Option<Event> {
        let mut event = None;
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 40.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Create AI").font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
            ui.add_space(4.0);
            if let Some(tab) = flow_kit::segmented(ui, self.tab, &[(Tab::Create, "Create"), (Tab::History, "History")]) {
                self.tab = tab;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if super::chrome::icon_button(ui, Icon::Close, "Close Create AI", true).clicked() {
                    event = Some(Event::Close);
                }
            });
        });
        ui.add_space(4.0);
        event
    }

    /// No key yet for `service`: what is sent and to whom, and the way into the settings window.
    fn setup(&mut self, ui: &mut Ui, service: Service) {
        let name = service.name();
        flow_kit::body_text(
            ui,
            format!("Create AI sends only your prompt and chosen style to {name}, using your own API key; your designs and files never leave this computer."),
            false,
        );
        flow_kit::body_text(ui, "Choose Claude, OpenAI, OpenRouter or a model on this computer, and add your key, in Create AI settings.", true);
        ui.add_space(4.0);
        if flow_kit::primary_button(ui, "Set up Create AI", true).clicked() {
            self.open_settings();
        }
    }

    fn create_tab(&mut self, ui: &mut Ui) -> Option<Event> {
        let idle = self.job.is_none();
        self.prompt_box(ui, idle);
        ui.add_enabled_ui(idle, |ui| {
            section(ui, "Style");
            self.style_chips(ui);
            section(ui, "Detail");
            let options = Detail::ALL.map(|d| (d, d.label()));
            if let Some(detail) = flow_kit::segmented(ui, self.detail, &options) {
                self.detail = detail;
            }
        });
        ui.add_space(4.0);
        let mut event = None;
        if idle {
            let named = !self.ai.model(self.ai.service).is_empty();
            let ready = named && !self.prompt.trim().is_empty();
            if flow_kit::primary_button(ui, "Generate", ready).clicked() {
                self.start(GenerateRequest::new(&self.prompt, self.style, self.detail));
            }
            if !named {
                flow_kit::body_text(ui, "Choose a model in Create AI settings.", true);
            }
        }
        if let Some((e, service)) = &self.error {
            flow_kit::notice(ui, flow_kit::Notice::Warning, &e.message(*service));
        }
        if !idle {
            placeholders(ui);
            flow_kit::body_text(ui, "Drawing four designs. This can take a minute or two.", true);
            if flow_kit::secondary_button(ui, "Cancel", true).clicked() {
                self.cancel();
            }
        } else if !self.tiles.is_empty() {
            event = self.results(ui);
        }
        self.footer(ui);
        event
    }

    fn prompt_box(&mut self, ui: &mut Ui, enabled: bool) {
        section(ui, "What should Kerf draw?");
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("create-ai-prompt")));
        let stroke = Stroke::new(1.0_f32, if focused { ACCENT_PRIMARY } else { BORDER_SUBTLE });
        Frame::NONE.fill(BG_PANEL).stroke(stroke).corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            let hint = RichText::new("Describe a design, such as a dinosaur with a unicorn horn riding a skateboard").font(fonts::regular(13.0)).color(TEXT_MUTED);
            let edit = egui::TextEdit::multiline(&mut self.prompt)
                .id(egui::Id::new("create-ai-prompt"))
                .char_limit(PROMPT_MAX_CHARS)
                .desired_rows(3)
                .frame(false)
                .hint_text(hint)
                .desired_width(f32::INFINITY)
                .font(fonts::regular(13.0))
                .text_color(TEXT_PRIMARY);
            ui.add_enabled(enabled, edit).widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, enabled, "Prompt"));
        });
        let count = format!("{}/{PROMPT_MAX_CHARS}", self.prompt.chars().count());
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| ui.label(RichText::new(count).font(fonts::regular(12.0)).color(TEXT_MUTED)));
    }

    fn style_chips(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for style in Style::ALL {
                if chip(ui, style.label(), style.brief(), self.style == style).clicked() {
                    self.style = style;
                }
            }
        });
    }

    fn results(&mut self, ui: &mut Ui) -> Option<Event> {
        let w = (ui.available_width() - GAP) / 2.0;
        let mut clicked = None;
        for (row, pair) in self.tiles.chunks(2).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (col, tile) in pair.iter().enumerate() {
                    let i = row * 2 + col;
                    if result_tile(ui, tile, &format!("Design {}", i + 1), w, self.selected == Some(i)).clicked() && tile.ready() {
                        clicked = Some(i);
                    }
                }
            });
        }
        if clicked.is_some() {
            self.selected = clicked;
        }
        let chosen = self.selected.and_then(|i| self.tiles.get(i)).filter(|t| t.ready()).map(|t| t.candidate.clone());
        let mut event = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            if flow_kit::primary_button(ui, "Insert", chosen.is_some()).clicked()
                && let Some(c) = &chosen
            {
                let prompt = self.last.as_ref().map_or("", |r| r.prompt.as_str());
                event = c.piece.clone().map(|piece| Event::Insert { title: crate::ai::layer_name(&c.title, prompt), piece: Box::new(piece) });
            }
            if flow_kit::secondary_button(ui, "More like this", chosen.is_some()).clicked()
                && let (Some(c), Some(last)) = (&chosen, &self.last)
            {
                let request =
                    GenerateRequest { variations_of: Some(Candidate { piece: None, ..c.clone() }), ..GenerateRequest::new(&last.prompt, last.style, last.detail) };
                self.start(request);
            }
            if flow_kit::secondary_button(ui, "Regenerate", self.last.is_some()).clicked()
                && let Some(last) = &self.last
            {
                self.start(GenerateRequest::new(&last.prompt, last.style, last.detail));
            }
        });
        event
    }

    /// Who draws, with which model, and the way into the settings window.
    fn footer(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        flow_kit::divider(ui);
        let service = self.ai.service;
        let model = self.ai.model(service);
        let using = if model.is_empty() { format!("{}, no model chosen", service.label()) } else { format!("{}, {model}", service.label()) };
        flow_kit::body_text(ui, using, true);
        let settings = egui::Button::new(RichText::new("Create AI settings").font(fonts::medium(13.0)).color(ACCENT_PRIMARY)).frame(false);
        if ui.add(settings).clicked() {
            self.open_settings();
        }
    }

    fn history_tab(&mut self, ui: &mut Ui, projects_dir: Option<&Path>) -> Option<Event> {
        let entries = self.history.get_or_insert_with(|| {
            let loaded = projects_dir.map(history::load).unwrap_or_default();
            loaded.into_iter().map(|e| (e.clone(), Tile::of(Candidate::drawn(e.title.clone(), e.svg.clone()).validated(e.style, e.detail)))).collect()
        });
        if entries.is_empty() {
            flow_kit::body_text(ui, "Designs you generate are kept here, so you can insert them again, even offline.", true);
            return None;
        }
        let mut event = None;
        for (entry, tile) in entries.iter() {
            let name = crate::ai::layer_name(&entry.title, &entry.prompt);
            if history_row(ui, entry, tile, &name).clicked()
                && let Some(piece) = tile.candidate.piece.clone()
            {
                event = Some(Event::Insert { title: name, piece: Box::new(piece) });
            }
        }
        event
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(fonts::medium(13.0)).color(TEXT_PRIMARY));
}

/// A pill: accent ring and tint when chosen, hairline otherwise.
fn chip(ui: &mut Ui, label: &str, body: &str, on: bool) -> Response {
    let enabled = ui.is_enabled();
    let font = fonts::medium(13.0);
    let w = super::chrome::text_width(ui, label, font.clone()) + 24.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, BUTTON_H), if enabled { Sense::click() } else { Sense::hover() });
    let resp = tip(resp, label, body, None);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, on, label));
    let (fill, ring, ink) = match (on, enabled && resp.hovered()) {
        (true, _) => (ACCENT_HOVER, ACCENT_PRIMARY, ACCENT_PRIMARY),
        (false, true) => (ACCENT_HOVER, BORDER_SUBTLE, TEXT_PRIMARY),
        (false, false) => (BG_PANEL, BORDER_SUBTLE, if enabled { TEXT_PRIMARY } else { DISABLED_TEXT }),
    };
    let radius = (BUTTON_H / 2.0) as u8;
    ui.painter().rect(rect, radius, fill, Stroke::new(1.0_f32, ring), StrokeKind::Inside);
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, font, ink);
    resp
}

/// Four grey tiles that breathe while the model draws.
fn placeholders(ui: &mut Ui) {
    let w = (ui.available_width() - GAP) / 2.0;
    let t = ui.input(|i| i.time) as f32;
    for row in 0..CANDIDATES / 2 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for col in 0..2 {
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(w), Sense::hover());
                resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, "Drawing"));
                let phase = ((t * 2.0 + (row * 2 + col) as f32 * 0.6).sin() + 1.0) / 2.0;
                ui.painter().rect_filled(rect, 8, BG_CANVAS_VOID.lerp_to_gamma(BORDER_SUBTLE, phase));
            }
        });
    }
}

fn result_tile(ui: &mut Ui, tile: &Tile, label: &str, w: f32, selected: bool) -> Response {
    let ready = tile.ready();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(w), if ready { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, ready, selected, label));
    let painter = ui.painter();
    painter.rect_filled(rect, 8, if ready && resp.hovered() && !selected { ACCENT_HOVER } else { BG_CANVAS_VOID });
    if selected {
        painter.rect_stroke(rect, 8, Stroke::new(2.0_f32, ACCENT_PRIMARY), StrokeKind::Inside);
    }
    if ready {
        paint_rings(ui, rect.shrink(w * 0.12), tile);
        return resp;
    }
    let inner = rect.shrink(10.0);
    paint_icon(ui, Icon::Warning, Rect::from_min_size(inner.min, Vec2::splat(18.0)), WARNING_TEXT);
    let words = format!("This one didn't come out cut-ready: {}", tile.candidate.problem.as_deref().unwrap_or("no reason given."));
    let galley = ui.painter().layout(words, fonts::regular(11.0), TEXT_MUTED, inner.width());
    ui.painter().galley(pos2(inner.left(), inner.top() + 24.0), galley, TEXT_MUTED);
    resp
}

/// A past design: its thumbnail, title and prompt. Clicking inserts it.
fn history_row(ui: &mut Ui, entry: &Entry, tile: &Tile, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::click());
    let label = format!("Insert {name}");
    let resp = tip(resp, &label, &entry.prompt, None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, tile.ready(), &label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8, ACCENT_HOVER);
    }
    let thumb = Rect::from_min_size(rect.min + vec2(4.0, 4.0), Vec2::splat(64.0));
    ui.painter().rect_filled(thumb, 6, BG_CANVAS_VOID);
    paint_rings(ui, thumb.shrink(8.0), tile);
    let x = thumb.right() + 12.0;
    text(ui.painter(), pos2(x, rect.top() + 22.0), Align2::LEFT_CENTER, name, fonts::medium(13.0), TEXT_PRIMARY);
    let detail = format!("{}, {}", entry.style.label(), entry.detail.label());
    text(ui.painter(), pos2(x, rect.top() + 42.0), Align2::LEFT_CENTER, &detail, fonts::regular(12.0), TEXT_MUTED);
    resp
}

/// The tile's outline, holes and all, as a flat dark silhouette centred in `area`.
fn paint_rings(ui: &Ui, area: Rect, tile: &Tile) {
    let (w, h) = tile.size;
    let scale = (area.width() / w.max(0.01)).min(area.height() / h.max(0.01));
    let origin = area.center() - vec2(w, h) * scale / 2.0;
    let rings: Vec<Ring> = tile.rings.iter().map(|r| r.iter().map(|&(x, y)| (origin.x + x * scale, origin.y + y * scale)).collect()).collect();
    let (points, tris) = crate::geometry::triangulate(&rings);
    let mut mesh = egui::Mesh::default();
    for p in &points {
        mesh.colored_vertex(pos2(p.0, p.1), TEXT_PRIMARY);
    }
    for [a, b, c] in tris {
        mesh.add_triangle(a as u32, b as u32, c as u32);
    }
    ui.painter().add(mesh);
}

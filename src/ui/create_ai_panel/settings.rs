//! The Create AI settings window: who draws (Anthropic, OpenAI, OpenRouter or a local server), the
//! API key for that service, a local server's address, and the model, picked from the list the service
//! itself reports. Everything a user who has only installed the app needs, with no file to edit.
//!
//! Keys go straight to the keychain; the rest is saved to `settings.json` as it changes.

use egui::{Align, Align2, Area, Color32, Context, Id, Layout, Order, RichText, Sense, Ui, WidgetInfo, WidgetType, vec2};

use super::{CreateAi, Event};
use crate::ai::keys::Source;
use crate::ai::{Listing, Service};
use crate::ui::fonts;
use crate::ui::tokens::*;
use crate::ui::{chrome, flow_kit};

const WIDTH: f32 = 440.0;
/// The model list's height before it scrolls.
const LIST_H: f32 = 150.0;

/// What the provider said when asked for its models.
pub(super) type Models = (Service, Result<Vec<String>, String>);

impl CreateAi {
    pub fn open_settings(&mut self) {
        tracing::info!(target: "ai", "settings opened");
        self.settings_open = true;
        (self.key_note, self.show_key) = (None, false);
    }

    fn close_settings(&mut self) {
        self.settings_open = false;
        self.key_field.clear();
        self.listing = None;
    }

    /// The window, while it is open, over a scrim that keeps clicks off the canvas.
    pub fn settings_window(&mut self, ctx: &Context) -> Option<Event> {
        if !self.settings_open {
            return None;
        }
        self.poll_listing(ctx);
        let screen = ctx.screen_rect();
        Area::new(Id::new("create-ai-settings-scrim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(28));
        });
        let mut event = None;
        // The card's margins and some air around it; on a short screen the body scrolls.
        let tallest = (screen.height() - 96.0).max(200.0);
        Area::new(Id::new("create-ai-settings")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            flow_kit::card_frame().show(ui, |ui| {
                ui.set_width(WIDTH);
                egui::ScrollArea::vertical().id_salt("create-ai-settings-body").max_height(tallest).show(ui, |ui| {
                    flow_kit::themed(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        event = self.settings_body(ui);
                    });
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_settings();
        }
        event
    }

    fn settings_body(&mut self, ui: &mut Ui) -> Option<Event> {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Create AI settings").font(fonts::semibold(16.0)).color(TEXT_PRIMARY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chrome::icon_button(ui, crate::ui::icons::Icon::Close, "Close Create AI settings", true).clicked() {
                    self.close_settings();
                }
            });
        });
        flow_kit::body_text(ui, "Choose who draws your designs. Kerf sends only your prompt, style and detail; your designs and files never leave this computer.", true);
        let mut event = self.provider_choice(ui);
        let service = self.ai.service;
        flow_kit::divider(ui);
        if service.needs_key() {
            self.key_section(ui, service);
        } else {
            event = self.address_section(ui).or(event);
        }
        flow_kit::divider(ui);
        event = self.model_section(ui, service).or(event);
        ui.add_space(4.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if flow_kit::primary_button(ui, "Done", true).clicked() {
                self.close_settings();
            }
        });
        event
    }

    fn provider_choice(&mut self, ui: &mut Ui) -> Option<Event> {
        heading(ui, "Provider");
        let mut picked = None;
        for service in Service::ALL {
            if flow_kit::radio_row(ui, self.ai.service == service, service.label()).clicked() && self.ai.service != service {
                picked = Some(service);
            }
        }
        flow_kit::body_text(ui, describe(self.ai.service), true);
        let service = picked?;
        tracing::info!(target: "ai", ?service, "service picked");
        self.ai.service = service;
        (self.error, self.key_note, self.show_key, self.listing) = (None, None, false, None);
        self.key_field.clear();
        Some(Event::SettingsChanged)
    }

    fn key_section(&mut self, ui: &mut Ui, service: Service) {
        heading(ui, "API key");
        let status = match (self.keys.current(service).map(|(_, source)| source), service.env_var()) {
            (Some(Source::Environment), Some(var)) => format!("Using {var} from the environment. It takes precedence over a saved key."),
            (Some(_), _) => "A key is saved in your system keychain.".to_owned(),
            (None, _) => "No key yet. Paste yours here; Kerf keeps it in your system keychain, never in its settings or your projects.".to_owned(),
        };
        let saved = matches!(self.keys.current(service), Some((_, Source::Saved)));
        flow_kit::body_text(ui, status, true);
        let field = egui::Frame::NONE.fill(BG_PANEL).stroke(egui::Stroke::new(1.0_f32, BORDER_SUBTLE)).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 6));
        let resp = field
            .show(ui, |ui| {
                let hint = RichText::new(if saved { "Paste a new key to replace it" } else { service.key_hint() }).font(fonts::regular(13.0)).color(TEXT_MUTED);
                let edit = egui::TextEdit::singleline(&mut self.key_field).password(!self.show_key).frame(false).hint_text(hint).desired_width(f32::INFINITY);
                ui.add(edit.font(fonts::regular(13.0)))
            })
            .inner;
        let name = service.name();
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, format!("{name} API key")));
        let submitted = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if flow_kit::primary_button(ui, "Save key", !self.key_field.trim().is_empty()).clicked() || (submitted && !self.key_field.trim().is_empty()) {
                let key = std::mem::take(&mut self.key_field);
                self.key_note = self.keys.save(service, &key).err();
                self.show_key = false;
            }
            if flow_kit::secondary_button(ui, "Remove key", saved).clicked() {
                self.key_note = self.keys.forget(service).err();
            }
            let toggle = if self.show_key { "Hide key" } else { "Show key" };
            if link(ui, toggle).clicked() {
                self.show_key = !self.show_key;
            }
        });
        if let Some(note) = &self.key_note {
            flow_kit::notice(ui, flow_kit::Notice::Warning, note);
        }
        if let Some((url, page)) = service.key_page() {
            ui.hyperlink_to(RichText::new(format!("Get a key from {page}")).font(fonts::medium(13.0)).color(ACCENT_PRIMARY), url);
        }
    }

    fn address_section(&mut self, ui: &mut Ui) -> Option<Event> {
        heading(ui, "Server address");
        let resp = flow_kit::text_field(ui, &mut self.ai.local_address, crate::settings::DEFAULT_LOCAL_ADDRESS);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Local server address"));
        flow_kit::body_text(ui, "Ollama listens on http://localhost:11434/v1 and LM Studio on http://localhost:1234/v1. No key is needed.", true);
        resp.changed().then_some(Event::SettingsChanged)
    }

    fn model_section(&mut self, ui: &mut Ui, service: Service) -> Option<Event> {
        heading(ui, "Model");
        let default = crate::settings::default_ai_model(service);
        let hint = if default.is_empty() { "Load the list, or type a model name" } else { default };
        let resp = flow_kit::text_field(ui, self.ai.model_mut(service), hint);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Model name"));
        let mut event = resp.changed().then_some(Event::SettingsChanged);
        let loading = self.listing.as_ref().is_some_and(|(_, s)| *s == service);
        ui.horizontal(|ui| {
            if flow_kit::secondary_button(ui, if loading { "Loading models" } else { "Load models" }, !loading).clicked() {
                self.load_models(service);
            }
            if !default.is_empty() && self.ai.model(service) != default && link(ui, "Use the default").clicked() {
                *self.ai.model_mut(service) = default.into();
                event = Some(Event::SettingsChanged);
            }
        });
        match self.models.as_ref().filter(|(s, _)| *s == service).map(|(_, list)| list) {
            Some(Err(why)) => flow_kit::notice(ui, flow_kit::Notice::Warning, why),
            Some(Ok(list)) if list.is_empty() => _ = flow_kit::body_text(ui, format!("{} lists no models.", service.name()), true),
            Some(Ok(list)) => {
                let typed = self.ai.model(service).to_lowercase();
                // The field filters the list as the user types; a name that matches exactly, or matches
                // nothing, shows it all.
                let exact = list.iter().any(|m| m.to_lowercase() == typed);
                let matching: Vec<&String> = list.iter().filter(|m| m.to_lowercase().contains(&typed)).collect();
                let shown: Vec<&String> = if exact || matching.is_empty() { list.iter().collect() } else { matching };
                flow_kit::body_text(ui, format!("{} of {} models. Click one to use it.", shown.len(), list.len()), true);
                let mut chosen = None;
                egui::ScrollArea::vertical().id_salt("create-ai-models").max_height(LIST_H).show(ui, |ui| {
                    for model in shown {
                        let detail = if model == default { "default" } else { "" };
                        if flow_kit::select_row(ui, model.to_lowercase() == typed, model, detail).clicked() {
                            chosen = Some(model.clone());
                        }
                    }
                });
                if let Some(model) = chosen {
                    *self.ai.model_mut(service) = model;
                    event = Some(Event::SettingsChanged);
                }
            }
            None if !loading => _ = flow_kit::body_text(ui, format!("Load models asks {} which models it offers.", service.name()), true),
            None => {}
        }
        event
    }

    /// Asks the service for its models on a thread; the answer shows when it lands.
    fn load_models(&mut self, service: Service) {
        let Some(provider) = self.provider.clone().or_else(|| self.live(service)) else {
            self.models = Some((service, Err("Save an API key first.".into())));
            return;
        };
        tracing::info!(target: "ai", ?service, "models asked");
        self.models = None;
        self.listing = Some((Listing::start(provider), service));
    }

    fn poll_listing(&mut self, ctx: &Context) {
        let Some((listing, service)) = &self.listing else { return };
        let Some(result) = listing.try_result() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let service = *service;
        tracing::info!(target: "ai", ?service, ok = result.is_ok(), "models listed");
        self.models = Some((service, result.map_err(|e| e.message(service))));
        self.listing = None;
    }

    #[cfg(test)]
    pub fn listing(&self) -> bool {
        self.listing.is_some()
    }
}

/// One line on what a service is and what it needs.
fn describe(service: Service) -> &'static str {
    match service {
        Service::Anthropic => "Claude models, with your Anthropic API key.",
        Service::OpenAi => "OpenAI's models, with your OpenAI API key.",
        Service::OpenRouter => "Models from many vendors through one OpenRouter API key.",
        Service::Local => "A model running on your own computer, through Ollama, LM Studio or another OpenAI-compatible server.",
    }
}

fn heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(fonts::medium(13.0)).color(TEXT_PRIMARY));
}

/// A frameless accent text button.
fn link(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label).font(fonts::medium(13.0)).color(ACCENT_PRIMARY)).frame(false))
}

mod anchors;
mod autosave;
pub(crate) mod bench_mode;
mod canvas;
mod chrome;
mod clipboard;
mod colour_picker;
mod commands;
mod component_isolation;
mod components_panel;
mod container_canvas;
mod container_panel;
mod context_menu;
mod contour_panel;
mod corner_tool;
mod create_ai_panel;
mod cut_preview;
mod device_setup;
mod document;
mod effects_list;
mod export_dialog;
mod eyedropper;
mod flow_kit;
mod font_preview;
pub(crate) mod fonts;
mod grid_panel;
mod guides;
mod home;
mod icons;
mod import_notice;
mod inspector;
mod instance_overrides;
mod knife_tool;
mod lasso;
mod layers_actions;
mod layers_panel;
mod machine_modal;
mod measure_tool;
mod modal;
mod monogram_panel;
mod names;
mod node_edit;
mod notice;
mod op_legend;
pub(crate) mod op_style;
mod palette;
mod param_handles;
mod pattern_panel;
mod persistence;
mod pivot;
mod popover;
mod prepare;
mod prepare_nest;
mod print_step;
mod project_preview;
mod recovery;
mod repeat_panel;
mod replace_colour;
mod selection;
mod set_load_go;
mod shape_builder;
mod shape_panel;
mod shapes_panel;
mod snap_draw;
mod snap_panel;
mod tab_bar;
mod tabs;
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod tests_core;
#[cfg(test)]
mod tests_font_missing;
#[cfg(test)]
mod tests_shortcut_labels;
#[cfg(test)]
mod tests_spec05;
#[cfg(test)]
mod tests_spec06;
#[cfg(test)]
mod tests_spec07;
#[cfg(test)]
mod tests_spec09;
#[cfg(test)]
mod tests_spec10;
#[cfg(test)]
mod tests_spec11;
#[cfg(test)]
mod tests_spec12;
#[cfg(test)]
mod tests_spec13;
#[cfg(test)]
mod tests_spec14;
#[cfg(test)]
mod tests_spec15;
#[cfg(test)]
mod tests_spec16;
#[cfg(test)]
mod tests_spec17;
#[cfg(test)]
mod tests_spec18;
#[cfg(test)]
mod tests_spec19;
#[cfg(test)]
mod tests_spec20;
#[cfg(test)]
mod tests_spec21;
#[cfg(test)]
mod tests_spec21_groups;
#[cfg(test)]
mod tests_spec22;
#[cfg(test)]
mod tests_spec23;
#[cfg(test)]
mod tests_spec24;
#[cfg(test)]
mod tests_spec25;
#[cfg(test)]
mod tests_spec26;
#[cfg(test)]
mod tests_spec27;
#[cfg(test)]
mod tests_spec28;
#[cfg(test)]
mod tests_spec29;
#[cfg(test)]
mod tests_spec30;
#[cfg(test)]
mod tests_spec31;
#[cfg(test)]
mod tests_spec32;
#[cfg(test)]
mod tests_spec33;
#[cfg(test)]
mod tests_spec34;
#[cfg(test)]
mod tests_spec35;
#[cfg(test)]
mod tests_spec37;
#[cfg(test)]
mod tests_spec38;
#[cfg(test)]
mod tests_spec39;
#[cfg(test)]
mod tests_spec40;
#[cfg(test)]
mod tests_spec42;
#[cfg(test)]
mod tests_spec43;
#[cfg(test)]
mod tests_spec44;
#[cfg(test)]
mod tests_spec45;
#[cfg(test)]
mod tests_spec46;
#[cfg(test)]
mod tests_spec47;
#[cfg(test)]
mod tests_spec48;
#[cfg(test)]
mod tests_spec49;
#[cfg(test)]
mod tests_spec50;
#[cfg(test)]
mod tests_spec51;
#[cfg(test)]
mod tests_spec51b;
#[cfg(test)]
mod tests_spec52;
#[cfg(test)]
mod tests_spec52b;
#[cfg(test)]
mod tests_spec53;
#[cfg(test)]
mod tests_spec53b;
#[cfg(test)]
mod tests_spec54;
#[cfg(test)]
mod tests_spec55;
#[cfg(test)]
mod tests_spec56;
#[cfg(test)]
mod tests_spec56b;
#[cfg(test)]
mod tests_spec57;
#[cfg(test)]
mod tests_spec58;
#[cfg(test)]
mod tests_svg_hostile;
mod text_edit;
mod text_path;
pub(crate) mod tokens;
mod toolbar;
pub(crate) mod tooltip;
mod trace_panel;
mod tutorial;
mod tutorial_overlay;
mod upload;
mod versions_panel;
mod view_nav;
mod zoom_menu;

use std::path::PathBuf;
use std::time::Duration;

use egui::{Context, RichText};

use crate::bridge::{BridgeMessage, DeviceInfo};
#[cfg(test)]
use crate::design::Design;
use crate::device::Transport;
use crate::edit::{AppCommand, Edit, Insert, Subject};
#[cfg(test)]
use crate::links::LinkKind;
use crate::machine::MachineFamily;
use crate::make_flow::{MakeFlow, MakeStep, MatSetup};
use crate::print_then_cut::PaperSize;
use crate::settings::Settings;
use crate::units::Units;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Stay,
    Close,
    SavePrintSheet,
    CutPreview,
    /// Back to the canvas with this text selected, so its font can be picked.
    PickFont(u64),
}

type Outbox = Vec<crate::make_flow::Effect>;

pub struct Kerf {
    /// The active document.
    doc: document::Document,
    /// The other open documents.
    shelf: tabs::Shelf,
    machine: Option<DeviceInfo>,
    transport: Box<dyn Transport>,
    last_sent: Option<Logged>,
    last_received: Option<Logged>,
    show_payload: bool,
    /// Every length on every screen shows in these units. Saved to `settings.json` when it changes.
    units: Units,
    tab: chrome::Tab,
    /// Where projects live. Injected so tests never touch the real app-data directory.
    projects_dir: Option<PathBuf>,
    home_projects: Vec<crate::project::Entry>,
    /// Closed never-saved designs, listed beside the projects when Home's list is built.
    home_unsaved: Vec<crate::store::unsaved::Unsaved>,
    home_confirm: Option<home::Confirm>,
    /// The error banner's message, until dismissed.
    error: Option<String>,
    /// The system fonts text lays out in. Injected so tests run against a fixed set.
    fonts: crate::system_fonts::FontCatalogue,
    /// Where exports go without asking. Injected so tests never open a file dialog.
    export_dir: Option<PathBuf>,
    /// Open devices from `settings.json`, and the one the next Make starts on.
    devices: Vec<crate::output::DeviceProfile>,
    last_device: Option<u64>,
    /// The open device's thread, from its connection test until the flow closes.
    runner: Option<crate::output::runner::Runner>,
    /// Serial ports and dry-run paths. Tests inject a fake one, so no test opens a real port.
    host: Box<dyn crate::output::link::Host>,
    /// The setup screen's port list, scanned when it opens and on Refresh.
    ports: Option<Vec<crate::output::link::PortInfo>>,
    palette: palette::Palette,
    /// Autosave and crash recovery for every document (spec 33).
    saving: autosave::Saving,
    /// Decodes and stores uploaded images off the UI thread (spec 28).
    images: crate::trace::worker::Worker,
    /// The canvas's image textures, installed in the egui context for as long as the app lives.
    _textures: crate::assets::Installed,
    /// The file Upload takes without asking. Injected so tests never open a file dialog.
    upload_file: Option<PathBuf>,
    /// The id of the document the image worker's latest job is for; its answer goes there.
    image_owner: Option<String>,
    /// An uploaded SVG being read and parsed off the UI thread.
    svg_import: Option<upload::SvgJob>,
    /// The tab whose running Make kept this tab's Make from starting, while the question shows.
    make_elsewhere: Option<String>,
    /// The interactive tutorial, while it runs (spec 39).
    tutorial: Option<tutorial::Tour>,
    /// Help > About Kerf is showing.
    about: bool,
    /// The command the `--bench` script plays this frame (spec 38), set from outside the frame and run as
    /// a menu pick is. Surfaces never set it: what they raise in a frame runs in that frame.
    scripted: Option<commands::Command>,
    /// The Create AI panel and its running generation (spec 43).
    create_ai: create_ai_panel::CreateAi,
    /// The last colours applied, for the picker's Recent row (spec 56). Saved to `settings.json`.
    recent_colours: crate::settings::RecentColours,
    /// The user's saved swatches (spec 56). Saved to `settings.json`.
    swatches: Vec<crate::settings::Swatch>,
}

/// Rendered once per message so the payload panel never re-serialises on every frame.
struct Logged {
    title: String,
    json: String,
}

impl Logged {
    fn of(msg: &BridgeMessage) -> Self {
        Self { title: format!("{} ({})", msg.status.wire_name(), msg.status.code()), json: msg.to_pretty_json() }
    }
}

impl Kerf {
    pub fn new(ctx: &Context, transport: Box<dyn Transport>, projects_dir: Option<PathBuf>, fonts: crate::system_fonts::FontCatalogue) -> Self {
        install_theme(ctx);
        fonts.make_current();
        if let Some(dir) = &projects_dir {
            fonts.load_user_fonts(dir);
        }
        let settings = projects_dir.as_deref().map(Settings::load).unwrap_or_default();
        let saving = autosave::Saving::start(projects_dir.as_deref(), &fonts);
        let _textures = crate::assets::Textures::install(ctx, projects_dir.as_deref().map(crate::assets::dir));
        let images = upload::worker(ctx, projects_dir.as_deref());
        let mut app = Self {
            doc: document::Document::blank("Untitled-1".into()),
            shelf: Default::default(),
            machine: None,
            transport,
            last_sent: None,
            last_received: None,
            show_payload: true,
            units: settings.units,
            tab: chrome::Tab::Home,
            projects_dir,
            home_projects: Vec::new(),
            home_unsaved: Vec::new(),
            home_confirm: None,
            error: None,
            fonts,
            export_dir: None,
            devices: settings.devices,
            last_device: settings.last_device,
            runner: None,
            host: Box::new(crate::output::link::SystemHost),
            ports: None,
            palette: palette::Palette::new(settings.palette_recent),
            saving,
            images,
            _textures,
            upload_file: None,
            image_owner: None,
            svg_import: None,
            make_elsewhere: None,
            tutorial: None,
            about: false,
            scripted: None,
            create_ai: create_ai_panel::CreateAi::new(settings.ai),
            recent_colours: settings.recent_colours,
            swatches: settings.swatches,
        };
        app.restore_session();
        app.claim_session();
        app.show_home();
        app
    }

    /// Every launch opens here, the restored tabs waiting in the strip.
    fn show_home(&mut self) {
        self.refresh_home();
        self.tab = chrome::Tab::Home;
    }

    /// The active tab's canvas, as `--open` and `--bench` start.
    pub fn show_canvas(&mut self) {
        self.tab = chrome::Tab::Canvas;
    }

    /// Opens the Make flow on the current design, on the last device used. One machine means one Make:
    /// while another tab's runs, this asks what to do instead.
    fn start_make(&mut self) {
        if self.doc.flow.is_some() {
            return;
        }
        let other = self.docs().find(|d| d.flow.is_some()).map(|d| d.autosave.id.clone());
        if other.is_some() {
            self.make_elsewhere = other;
            return;
        }
        self.doc.flow = Some(self.make_flow());
    }

    fn send_all(&mut self, out: Outbox) {
        use crate::make_flow::Effect;
        for effect in out {
            match effect {
                Effect::Bridge(msg) => {
                    crate::debug_log::bridge("send", &msg);
                    self.last_sent = Some(Logged::of(&msg));
                    if self.transport.send(msg).is_err() {
                        return self.lose_machine();
                    }
                }
                Effect::Device(command) => {
                    tracing::info!(target: "flow", command = command_name(&command), "device");
                    if self.runner.as_ref().is_none_or(|r| r.send(command).is_err()) {
                        return self.lose_device("The device's connection closed.");
                    }
                }
                Effect::Open(profile) => {
                    tracing::info!(target: "flow", port = ?profile.port, protocol = ?profile.protocol, "open device");
                    self.runner = Some(crate::output::runner::Runner::spawn(self.host.open(&profile), profile));
                }
                Effect::DryRun { job, mat } => self.dry_run(&job, mat),
                Effect::UseDevice(profile) => self.change_settings(|s| s.last_device = profile.map(|p| s.upsert(p))),
                Effect::DeleteProfile(id) => self.change_settings(|s| s.remove(id)),
            }
        }
    }

    fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.settings();
        change(&mut settings);
        (self.devices, self.last_device) = (settings.devices, settings.last_device);
        self.save_settings();
    }

    fn settings(&self) -> Settings {
        Settings {
            units: self.units,
            devices: self.devices.clone(),
            last_device: self.last_device,
            palette_recent: self.palette.recents.clone(),
            ai: self.create_ai.ai.clone(),
            recent_colours: self.recent_colours.clone(),
            swatches: self.swatches.clone(),
        }
    }

    fn save_settings(&mut self) {
        if let Some(dir) = &self.projects_dir
            && let Err(e) = self.settings().save(dir)
        {
            tracing::error!(target: "error", error = %e, "saving settings failed");
            self.report(format!("Couldn't save settings: {e}"));
        }
    }

    /// Writes the job through a `FileLink` with the runner's own streaming, so the file holds exactly the
    /// bytes the port would get.
    fn dry_run(&mut self, job: &crate::output::Job, mat: usize) {
        let name = format!("{} - Mat {mat}.{}", self.doc.title, job.protocol.extension());
        let Some(path) = self.host.save_path(&name) else { return };
        tracing::info!(target: "flow", path = %path.display(), lines = job.lines.len(), "save to file");
        let mut link = crate::output::link::FileLink::new(path.clone());
        let written = crate::output::runner::run_to_end(&mut link, job).and_then(|()| link.finish().map_err(|e| e.to_string()));
        if let Err(e) = written {
            tracing::error!(target: "error", path = %path.display(), error = %e, "dry run failed");
            self.report(format!("Couldn't save {}: {e}", path.display()));
        }
    }

    /// The simulated machine went away. An open device never hears from it, so its flow carries on.
    fn lose_machine(&mut self) {
        if let Some(flow) = self.flow_owner_mut().and_then(|d| d.flow.as_mut()).filter(|f| f.profile().is_none()) {
            flow.lose_machine();
        }
    }

    fn lose_device(&mut self, why: &str) {
        self.runner = None;
        if let Some(flow) = self.flow_owner_mut().and_then(|d| d.flow.as_mut()).filter(|f| f.profile().is_some()) {
            flow.failure = Some(why.into());
            flow.lose_machine();
        }
    }

    fn poll_device(&mut self) {
        loop {
            let Some(runner) = &self.runner else { return };
            let event = match runner.try_recv() {
                Ok(Some(event)) => event,
                Ok(None) => return,
                Err(_) => return self.lose_device("The device's connection closed."),
            };
            tracing::info!(target: "flow", event = ?event, "device event");
            if let Some(flow) = self.flow_owner_mut().and_then(|d| d.flow.as_mut()) {
                flow.on_device_event(event);
            }
        }
    }

    /// Cancels a running job before the flow (or the app) goes, then lets the runner go.
    fn close_device(&mut self) {
        if let Some(runner) = &self.runner
            && self.docs().filter_map(|d| d.flow.as_ref()).any(|f| f.job_running())
        {
            tracing::info!(target: "flow", "cancel on close");
            _ = runner.send(crate::output::DeviceCommand::Cancel);
        }
        self.runner = None;
    }

    fn poll_machine(&mut self) {
        loop {
            let msg = match self.transport.try_recv() {
                Ok(Some(msg)) => msg,
                Ok(None) => return,
                Err(_) => return self.lose_machine(),
            };
            crate::debug_log::bridge("recv", &msg);
            self.last_received = Some(Logged::of(&msg));
            let Some(flow) = self.flow_owner_mut().and_then(|d| d.flow.as_mut()) else { continue };
            let out = flow.on_message(&msg);
            let machine = flow.machine.clone();
            self.machine = machine.or(self.machine.take());
            self.send_all(out);
        }
    }

    fn flow_top_bar(&mut self, ctx: &Context) {
        let step = match self.doc.flow.as_ref().map(|f| f.step) {
            Some(MakeStep::Prepare | MakeStep::ConnectMachine(_)) => "Prepare",
            _ => "Make",
        };
        let open = self.doc.flow.as_ref().and_then(|f| f.profile()).map(|p| format!("{} ({})", p.name, p.protocol.label()));
        let machine = open.or_else(|| self.machine.as_ref().map(|m| format!("{} ({})", m.device_type, m.connection_type.wire_name())));
        chrome::flow_top_bar(ctx, chrome::FlowBar { step, payload: &mut self.show_payload, machine });
    }

    fn canvas_chrome(&mut self, ctx: &Context) -> Option<commands::Command> {
        let status = self.doc.save_status(&self.saving.clock);
        let cx = commands::Cx::new(&self.doc.design, &self.doc.selection, self.doc.view.align_to);
        let bar = chrome::TopBar { title: &mut self.doc.title, view: &mut self.doc.view, units: &mut self.units, cx, history: &self.doc.history, status: &status };
        let mut action = chrome::top_bar(ctx, bar);
        self.collect_upload(ctx);
        match chrome::rail(ctx, self.doc.shapes_open, self.doc.monogram.is_some(), self.create_ai.open, self.doc.tool) {
            Some(chrome::RailEvent::Shapes) => {
                self.doc.shapes_open = !self.doc.shapes_open;
                self.doc.monogram = None;
                self.create_ai.open = false;
            }
            Some(chrome::RailEvent::Monogram) => {
                self.doc.monogram = self.doc.monogram.take().is_none().then(monogram_panel::MonogramPanel::new);
                self.create_ai.open = false;
                self.doc.shapes_open = false;
            }
            Some(chrome::RailEvent::CreateAi) => {
                self.create_ai.open = !self.create_ai.open;
                self.doc.monogram = None;
                self.doc.shapes_open = false;
            }
            Some(chrome::RailEvent::Upload) => action = action.or(Some(commands::Command::App(AppCommand::Upload))),
            Some(chrome::RailEvent::Tool(t)) => self.doc.tool = t,
            None => {}
        }
        if let Some(t) = tool_keys(ctx) {
            self.doc.tool = t;
        }
        match self.doc.shapes_open.then(|| shapes_panel::show(ctx, &mut self.doc.shapes_filter)).flatten() {
            Some(shapes_panel::PanelEvent::Insert(kind)) => {
                _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &Edit::Insert(Insert::Shape(kind)), Subject::Selection);
            }
            Some(shapes_panel::PanelEvent::Collapse) => self.doc.shapes_open = false,
            None => {}
        }
        self.monogram_panel(ctx);
        self.create_ai_panel(ctx);
        let versions_open = self.versions_frame(ctx);
        let mut inspected = None;
        if !versions_open {
            // Scoped so the picker's borrow of the swatch lists ends before we touch `self` again.
            let (asked, command, palette_changed) = {
                let mut palette = colour_picker::Palette::new(&mut self.recent_colours, &mut self.swatches);
                let (asked, command) = inspector::show(
                    ctx,
                    &mut self.doc.design,
                    &mut self.doc.selection,
                    &mut self.doc.contour_open,
                    self.units,
                    (self.doc.mat.width_in, self.doc.mat.height_in),
                    &mut palette,
                );
                (asked, command, palette.changed)
            };
            if let Some((asked, target)) = asked {
                self.pattern_asked(ctx, asked, target);
            }
            inspected = command;
            if palette_changed {
                self.save_settings();
            }
        }
        if self.doc.preview_open {
            let family = self.machine.as_ref().map_or(MachineFamily::StudioPro, |m| m.machine_family);
            self.doc.preview_open = project_preview::overlay(ctx, &self.doc.design, &MatSetup::preferred(family), self.units);
        }
        if self.doc.contour_open {
            if let Some(&id) = self.doc.selection.first() {
                self.doc.contour_open = contour_panel::show(ctx, &mut self.doc.design, id);
            } else {
                self.doc.contour_open = false;
            }
        }
        if !versions_open {
            toolbar::show(ctx, &mut self.doc.design, &mut self.doc.selection, &mut self.doc.contour_open, self.units, &mut self.doc.view);
        }
        let undo = notice::show(ctx, &mut self.doc.view.notice, &self.doc.design).then_some(commands::Command::App(AppCommand::Undo));
        if let Some(action) = import_notice::show(ctx, &mut self.doc.view.import)
            && let Some(notice) = self.doc.view.import.as_mut()
        {
            let ids = notice.stroke_ids();
            let edit = match action {
                import_notice::NoticeAction::Operation(op) => Edit::Operation(op),
                import_notice::NoticeAction::Outline => Edit::OutlineStrokes(notice.strokes.clone()),
            };
            // Once outlined the shapes are filled rings, not strokes, so retire the stroke row rather
            // than let a second click re-outline them or the hairline choice re-operation them.
            if matches!(action, import_notice::NoticeAction::Outline) {
                notice.strokes.clear();
            }
            if !ids.is_empty() {
                _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &edit, Subject::Ids(&ids));
            }
        }
        self.export_dialog(ctx);
        self.replace_colour_dialog(ctx);
        let edits_live = self.doc.tool == canvas::Tool::Select && self.doc.view.nodes.is_none() && !versions_open;
        action.or(inspected).or(undo).or_else(|| self.transform_again(ctx, edits_live)).or_else(|| commands::pressed(ctx, edits_live, self.doc.view.artboard_in))
    }

    /// The Create AI panel, when open. An Insert lands at the centre of the view as one undo step.
    fn create_ai_panel(&mut self, ctx: &Context) {
        self.create_ai.poll(ctx, self.projects_dir.as_deref());
        let panel = if self.create_ai.open { self.create_ai.show(ctx, self.projects_dir.as_deref()) } else { None };
        match panel.or(self.create_ai.settings_window(ctx)) {
            Some(create_ai_panel::Event::Insert { title, piece }) => {
                let ((cx, cy), board) = (self.doc.view.centre_in(), self.doc.view.artboard_in);
                let (w, h) = (piece.width_in, piece.height_in);
                let x_in = crate::transform::clamp_pos(cx - w / 2.0, w, board.0);
                let y_in = crate::transform::clamp_pos(cy - h / 2.0, h, board.1);
                let outline = crate::edit::Outline(Box::new(crate::design::Shape { x_in, y_in, ..*piece }));
                tracing::info!(target: "ai", title = %title, "insert");
                _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &Edit::Insert(Insert::Generated { title, outline }), Subject::Selection);
            }
            Some(create_ai_panel::Event::SettingsChanged) => self.save_settings(),
            Some(create_ai_panel::Event::Close) => self.create_ai.open = false,
            None => {}
        }
    }

    /// Runs an edit through `edit::run`, mapping Cmd+D to Power Duplicate while the last copies stay
    /// selected, and remembering each duplicate's step so Cmd+D repeats it and Transform Again applies it
    /// (spec 52).
    fn run_edit(&mut self, edit: Edit) {
        let frame_of = |design: &crate::design::Design, ids: &[u64]| {
            let shapes: Vec<crate::design::Shape> = design.shapes.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
            crate::transform::Frame::of(design, &shapes)
        };
        let doc = &mut self.doc;
        // Text on path over a lone text waits for a click on the shape to follow instead of editing now.
        if text_path::begin_pick(&doc.design, &doc.selection, &mut doc.view, &edit) {
            doc.tool = canvas::Tool::Select;
            return;
        }
        // Cmd+D is Power Duplicate only while the copies of a prior Duplicate stay selected; a step a
        // bare gesture recorded leaves it a plain Duplicate (spec 52).
        let edit = match (&edit, &doc.duplicate_memory) {
            (Edit::Duplicate, Some(m)) if m.from_duplicate && m.ids == doc.selection => Edit::TransformAgain { from: m.from, copy: true },
            _ => edit,
        };
        let sources = doc.selection.clone();
        let from = frame_of(&doc.design, &sources);
        let was_duplicate = matches!(edit, Edit::Duplicate | Edit::TransformAgain { copy: true, .. });
        let in_place_again = matches!(edit, Edit::TransformAgain { copy: false, .. });
        if let Some(notice) = notice::run_edit(&mut doc.design, &mut doc.selection, &edit) {
            doc.view.notice = Some(notice);
        }
        // Each duplicate and each Transform Again advances the remembered step, so the next press keeps an
        // even pitch (spec 52). Transform Again in place keeps the Duplicate flag of the step it repeated.
        if (was_duplicate || in_place_again)
            && let (Some(from), Some(to)) = (from, frame_of(&doc.design, &doc.selection))
        {
            let from_duplicate = was_duplicate || doc.duplicate_memory.as_ref().is_some_and(|m| m.from_duplicate);
            doc.duplicate_memory = Some(crate::repeat::DuplicateMemory { from, to, ids: doc.selection.clone(), from_duplicate });
        }
    }

    /// Cmd+Shift+D applies the last duplicate's step to the selection without a copy (spec 52). It is not
    /// in the shortcut table because it carries the remembered frame, which no const can hold.
    fn transform_again(&mut self, ctx: &Context, edits_live: bool) -> Option<commands::Command> {
        if !edits_live || ctx.wants_keyboard_input() {
            return None;
        }
        let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), egui::Key::D);
        ctx.input_mut(|i| i.consume_shortcut(&shortcut))
            .then(|| self.doc.duplicate_memory.clone())
            .flatten()
            .map(|m| commands::Command::Edit(Edit::TransformAgain { from: m.from, copy: false }))
    }

    /// Runs a command from the top bar or the keyboard. Edits go through `edit::run` like every other
    /// surface's; a blocked one does nothing.
    fn run(&mut self, command: commands::Command, ctx: &Context) {
        ctx.request_repaint();
        let app = match command {
            commands::Command::Edit(edit) => {
                self.run_edit(edit);
                return;
            }
            commands::Command::Clipboard(action) => {
                self.run_clipboard(action, ctx);
                return;
            }
            commands::Command::App(app) => app,
        };
        let blocked = app.check(&self.doc.design, &self.doc.history, &self.doc.selection).err();
        tracing::info!(target: "cmd", action = ?app, sel = %crate::debug_log::ids(&self.doc.selection), blocked = ?blocked, "app");
        if blocked.is_some() {
            return;
        }
        self.doc.isolation_before(&app);
        match app {
            AppCommand::New => self.new_tab(),
            AppCommand::Home => self.show_home(),
            AppCommand::Save if self.doc.transient => {}
            AppCommand::Save => _ = self.save_to_disk(),
            AppCommand::Export => self.doc.export = Some(export_dialog::Dialog::new(self.units)),
            AppCommand::PrepareForDesignSpace => self.prepare_for_design_space(),
            // Undo and Redo end a drag in progress, so the pointer cannot keep editing the restored design.
            AppCommand::Undo => {
                self.doc.view.end_gesture();
                self.doc.history.undo(&mut self.doc.design);
            }
            AppCommand::Redo => {
                self.doc.view.end_gesture();
                self.doc.history.redo(&mut self.doc.design);
            }
            AppCommand::Make => self.start_make(),
            AppCommand::Preview => self.doc.preview_open = true,
            AppCommand::Open(id) => _ = self.open_tab(&id),
            AppCommand::Palette => {
                let dir = self.projects_dir.clone();
                self.palette.toggle(|| dir.as_deref().map(recent_projects).unwrap_or_default());
            }
            AppCommand::ZoomIn
            | AppCommand::ZoomOut
            | AppCommand::ResetZoom
            | AppCommand::ZoomToFit
            | AppCommand::ZoomToSelection
            | AppCommand::ZoomToAll
            | AppCommand::ZoomTo(_) => view_nav::run(&mut self.doc.view, &self.doc.design, &self.doc.selection, &app),
            AppCommand::Units(units) => {
                self.units = units;
                self.save_settings();
            }
            AppCommand::Snapping => self.doc.design.set_snapping(!self.doc.design.snapping()),
            AppCommand::SnapClass(class) => {
                let mut options = self.doc.design.snap;
                options.set(class, !options.enabled(class));
                _ = crate::edit::run(&mut self.doc.design, &mut self.doc.selection, &crate::edit::Edit::Snap(options), crate::edit::Subject::Selection);
            }
            AppCommand::SnapSettings => snap_panel::open(ctx),
            AppCommand::PartialGrid => grid_panel::toggle_partial(&mut self.doc.design, &mut self.doc.selection, self.doc.view.grid),
            AppCommand::GridSettings => grid_panel::open(ctx),
            AppCommand::CutPreview => self.doc.cut_preview = Some(cut_preview::CutPreview::new(&self.make_flow())),
            AppCommand::VersionHistory => self.open_versions(),
            AppCommand::Upload => self.upload(ctx),
            AppCommand::EditMaster(component) => self.doc.edit_master(component),
            AppCommand::LeaveMaster => {}
            AppCommand::CloseTab(i) => self.close_tab(i.unwrap_or(self.active_index())),
            // The Tab key also moved egui's focus this frame; the new tab starts with none.
            AppCommand::NextTab | AppCommand::PreviousTab => {
                self.step_tab(if app == AppCommand::NextTab { 1 } else { -1 });
                ctx.memory_mut(|m| m.stop_text_input());
            }
            AppCommand::Tab(i) => self.switch_to(i),
            AppCommand::Tutorial => self.start_tutorial(ctx),
            AppCommand::Shortcuts => {
                let dir = self.projects_dir.clone();
                self.palette.open_shortcuts(|| dir.as_deref().map(recent_projects).unwrap_or_default());
            }
            AppCommand::About => self.about = true,
            AppCommand::AiSettings => self.create_ai.open_settings(),
            AppCommand::Rename => layers_panel::begin_rename(ctx, &self.doc.design, &self.doc.selection),
            AppCommand::EditMonogram => {
                let target = self.doc.design.editable_monogram(&self.doc.selection).ok();
                let spec = target.and_then(|id| self.doc.design.shapes.iter().find(|s| s.id == id)).and_then(|s| s.monogram.as_deref().cloned());
                if let (Some(id), Some(spec)) = (target, spec) {
                    self.doc.monogram = Some(monogram_panel::MonogramPanel::editing(id, spec));
                    self.doc.shapes_open = false;
                    self.create_ai.open = false;
                }
            }
            AppCommand::ReplaceColour => self.doc.replace_colour = Some(replace_colour::Dialog::new(&self.doc.design)),
            AppCommand::SavePaletteToProject => self.doc.design.swatches = self.swatches.clone(),
            AppCommand::CopyPaletteToSettings => {
                self.swatches = self.doc.design.swatches.clone();
                self.save_settings();
            }
            AppCommand::StepRepeatPanel => self.doc.repeat_panel = Some(repeat_panel::RepeatPanel::step()),
            AppCommand::RotateCopiesPanel => self.doc.repeat_panel = Some(repeat_panel::RepeatPanel::rotate()),
        }
    }

    /// The palette over this frame, if open, and the command picked from it. A pick is saved to the
    /// recent-use history straight away.
    fn palette_overlay(&mut self, ctx: &Context, on_canvas: bool) -> Option<commands::Command> {
        if !self.palette.is_open() {
            return None;
        }
        let families = self.fonts.families().unwrap_or_default();
        let writing = self.fonts.writing();
        let sources = palette::Sources {
            cx: commands::Cx::new(&self.doc.design, &self.doc.selection, self.doc.view.align_to),
            history: &self.doc.history,
            on_canvas,
            system_fonts: families,
            writing_fonts: &writing,
            projects: self.palette.projects(),
            board_in: self.doc.view.artboard_in,
        };
        let mut rows = palette::rows(&sources);
        if self.palette.keyed() {
            rows.retain(|r| r.shortcut.is_some());
        }
        let picked = self.palette.show(ctx, &rows)?;
        self.save_settings();
        Some(picked)
    }

    /// A Make flow over the design on the last device, its mats composed as Make composes them.
    fn make_flow(&mut self) -> MakeFlow {
        self.fonts.settle_for(&self.doc.design);
        let last = self.settings().last_profile().cloned();
        let device = last.map_or(crate::output::OutputDevice::Simulator, crate::output::OutputDevice::Open);
        MakeFlow::new(self.doc.design.clone(), self.machine.clone()).on(device).with_paper(PaperSize::preferred(self.units))
    }

    fn payload_panel(&mut self, ctx: &Context) {
        if !self.show_payload {
            return;
        }
        let frame = egui::Frame::NONE.fill(tokens::BG_PANEL).inner_margin(egui::Margin::same(16));
        let panel = egui::SidePanel::right("payload").exact_width(340.0).resizable(false).frame(frame).show_separator_line(false);
        panel.show(ctx, |ui| {
            let full = ui.max_rect().expand(16.0);
            ui.painter().vline(full.left() + 0.5, full.y_range(), egui::Stroke::new(1.0_f32, tokens::BORDER_SUBTLE));
            flow_kit::themed(ui, |ui| {
                flow_kit::panel_title(ui, "Payload");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    logged_section(ui, "Last sent", &self.last_sent);
                    logged_section(ui, "Last received", &self.last_received);
                });
            });
        });
    }

    /// Reloads the decoded pixels the eyedropper reads from images and patterns when the set of assets
    /// under the canvas has changed (spec 56). Cheap, and skipped while the set is unchanged.
    fn refresh_eyedropper_pixels(&mut self) {
        use std::collections::HashSet;
        let mut ids: HashSet<crate::assets::AssetId> = HashSet::new();
        for shape in &self.doc.design.shapes {
            if let crate::design::ShapeKind::Bitmap { asset, .. } = &shape.kind {
                ids.insert(asset.clone());
            }
            if let Some(pattern) = &shape.pattern {
                ids.insert(pattern.asset.clone());
            }
        }
        let current: HashSet<crate::assets::AssetId> = self.doc.view.eyedropper.pixels.keys().cloned().collect();
        if ids != current {
            let dir = self.projects_dir.as_deref().map(crate::assets::dir);
            self.doc.view.eyedropper.pixels = crate::assets::pixels(dir.as_deref(), ids.iter());
        }
    }

    /// The canvas or the Make flow's screen. Returns the command the canvas raised this frame.
    fn central(&mut self, ctx: &Context) -> Option<commands::Command> {
        if self.doc.flow.is_none() && self.doc.trace.is_some() {
            return None;
        }
        let mut out = Outbox::new();
        let panel = match self.doc.flow {
            None => egui::CentralPanel::default().frame(egui::Frame::NONE),
            Some(_) => egui::CentralPanel::default().frame(egui::Frame::NONE.fill(tokens::BG_CANVAS_VOID)),
        };
        self.doc.view.artboard_in = self.doc.master_frame().unwrap_or((self.doc.mat.width_in, self.doc.mat.height_in));
        if self.doc.tool == canvas::Tool::Eyedropper {
            self.refresh_eyedropper_pixels();
        }
        let (nav, raised) = panel
            .show(ctx, |ui| match self.doc.flow.as_mut() {
                None => match self.doc.autosave.preview().cloned() {
                    // A version preview is read-only: the canvas edits a scratch copy.
                    Some(mut scratch) => {
                        (Nav::Stay, canvas::show(ui, &mut scratch, &mut Vec::new(), &mut self.doc.view, &mut canvas::Tool::Select, &mut self.units, &mut None))
                    }
                    None => (
                        Nav::Stay,
                        canvas::show(
                            ui,
                            &mut self.doc.design,
                            &mut self.doc.selection,
                            &mut self.doc.view,
                            &mut self.doc.tool,
                            &mut self.units,
                            &mut self.doc.repeat_panel,
                        ),
                    ),
                },
                Some(flow) => (flow_screen(ui, flow, &mut out, self.units, &mut self.doc.nest), None),
            })
            .inner;
        if let Some(flow) = self.doc.flow.as_mut() {
            let devices = machine_modal::Devices { saved: &self.devices, host: &*self.host, ports: &mut self.ports, units: self.units };
            machine_modal::show(ctx, flow, devices, &mut out);
        }
        self.send_all(out);
        if nav == Nav::SavePrintSheet {
            self.save_print_sheet();
        }
        let leaving = matches!(nav, Nav::Close | Nav::PickFont(_));
        if leaving || self.doc.flow.as_ref().is_some_and(|f| f.profile().is_none()) {
            self.close_device();
        }
        if nav == Nav::CutPreview
            && let Some(flow) = &self.doc.flow
        {
            self.doc.cut_preview = Some(cut_preview::CutPreview::new(flow));
            ctx.request_repaint();
        }
        if leaving {
            self.close_flow();
        }
        if let Nav::PickFont(id) = nav {
            self.doc.selection = vec![id];
        }
        self.nest_requests(ctx);
        raised
    }

    /// Leaves the Make flow for the canvas, keeping the mat and material it chose.
    fn close_flow(&mut self) {
        self.close_device();
        if let Some(flow) = self.doc.flow.take() {
            self.doc.mat = flow.setup.mat_size;
            self.doc.view.material = flow.material.map(|m| m.name);
        }
    }

    /// Help > About Kerf.
    fn about_dialog(&mut self, ctx: &Context) {
        if !self.about {
            return;
        }
        let detail = format!("Version {}. A design and cutting app for hobby cutting machines.", env!("CARGO_PKG_VERSION"));
        let ok = [modal::Choice { label: "OK", a11y: "Close About", primary: true }];
        if modal::show(ctx, "about", "Kerf", &detail, &ok, 0).is_some() {
            self.about = false;
        }
    }
}

fn flow_screen(ui: &mut egui::Ui, flow: &mut MakeFlow, out: &mut Outbox, units: Units, nest: &mut prepare_nest::Nest) -> Nav {
    match flow.step {
        MakeStep::Prepare | MakeStep::ConnectMachine(_) => prepare::show(ui, flow, out, units, nest),
        _ => set_load_go::show(ui, flow, out),
    }
}

fn logged_section(ui: &mut egui::Ui, label: &str, logged: &Option<Logged>) {
    let header = logged.as_ref().map_or(label.to_owned(), |l| format!("{label}: {}", l.title));
    flow_kit::section_header(ui, &header);
    match logged {
        Some(l) => {
            egui::Frame::NONE.fill(tokens::BG_CANVAS_VOID).corner_radius(6).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&l.json).monospace().size(11.0).color(tokens::TEXT_PRIMARY))
            });
        }
        None => {
            flow_kit::body_text(ui, "Nothing yet", true);
        }
    }
    ui.add_space(16.0);
}

/// The log names a command without the job's lines.
fn command_name(command: &crate::output::DeviceCommand) -> &'static str {
    use crate::output::DeviceCommand as C;
    match command {
        C::Test => "test",
        C::Start(_) => "start",
        C::Pause => "pause",
        C::Resume => "resume",
        C::Cancel => "cancel",
    }
}

/// The most recently saved projects as (id, name), for the palette.
fn recent_projects(dir: &std::path::Path) -> Vec<(String, String)> {
    let ready = crate::project::list(dir).into_iter().filter_map(|e| match e {
        crate::project::Entry::Ready { project, .. } => Some((project.id, project.name)),
        crate::project::Entry::Unreadable { .. } => None,
    });
    ready.take(palette::RECENT_PROJECTS).collect()
}

fn tool_keys(ctx: &Context) -> Option<canvas::Tool> {
    if ctx.wants_keyboard_input() {
        return None;
    }
    // Match a key press only when it carries no modifiers, read from the event itself. A synthesised
    // Cmd+C delivers the C press with the command flag off `i.modifiers`, so the global modifier state
    // cannot stand in for the event's own; checking it here keeps Cmd+C as Copy, not the Corner tool.
    ctx.input(|i| {
        let plain = |key: egui::Key| i.events.iter().any(|e| matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.is_none()));
        // Shift alone, read from the event, so Shape Builder takes Illustrator's Shift+M.
        let shifted = |key: egui::Key| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.shift && !modifiers.command && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd))
        };
        if plain(egui::Key::V) {
            Some(canvas::Tool::Select)
        } else if plain(egui::Key::P) {
            Some(canvas::Tool::Pen)
        } else if plain(egui::Key::N) {
            Some(canvas::Tool::Pencil)
        } else if plain(egui::Key::T) {
            Some(canvas::Tool::Text)
        } else if plain(egui::Key::M) {
            Some(canvas::Tool::Measure)
        } else if plain(egui::Key::C) {
            Some(canvas::Tool::Corner)
        } else if plain(egui::Key::K) {
            Some(canvas::Tool::Knife)
        } else if plain(egui::Key::I) {
            Some(canvas::Tool::Eyedropper)
        } else if shifted(egui::Key::M) {
            Some(canvas::Tool::ShapeBuilder)
        } else {
            None
        }
    })
}

fn install_theme(ctx: &Context) {
    use egui::TextStyle;
    fonts::install(ctx);
    egui_extras::install_image_loaders(ctx);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.window_fill = egui::Color32::WHITE;
    style.visuals.panel_fill = egui::Color32::WHITE;
    // Set per-state text colours rather than override_text_color, which would force dark text even on
    // selected purple backgrounds. Selected widgets then take the white selection stroke.
    for w in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        w.fg_stroke.color = tokens::TEXT_PRIMARY;
    }
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, tokens::ACCENT_PRIMARY);
    // Hover and selection read as a soft violet tint with violet text, not a hard grey or a strong
    // fill with white, so menus and toggles feel calm and premium.
    style.visuals.widgets.hovered.weak_bg_fill = tokens::ACCENT_HOVER;
    style.visuals.widgets.active.weak_bg_fill = tokens::ACCENT_HOVER;
    style.visuals.widgets.open.weak_bg_fill = tokens::ACCENT_HOVER;
    style.visuals.selection.bg_fill = tokens::ACCENT_HOVER;
    style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, tokens::ACCENT_PRIMARY);
    style.visuals.hyperlink_color = tokens::ACCENT_PRIMARY;
    style.text_styles = [
        (TextStyle::Small, fonts::regular(12.0)),
        (TextStyle::Body, fonts::regular(14.0)),
        (TextStyle::Button, fonts::regular(14.0)),
        (TextStyle::Heading, fonts::display(16.0)),
        (TextStyle::Monospace, egui::FontId::monospace(13.0)),
    ]
    .into();
    ctx.set_style(style);
}

impl eframe::App for Kerf {
    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        context_menu::ctrl_click_is_secondary(ctx, raw_input);
        crate::debug_log::input(raw_input);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.close_device();
        self.quit();
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        crate::debug_log::frame_start();
        self.fonts.make_current();
        self.poll_machine();
        self.poll_device();
        self.persistence_dialogs(ctx);
        self.about_dialog(ctx);
        self.tutorial_acts(ctx);
        commands::frame_start(ctx);
        let cycled = commands::cycle_pressed(ctx);
        if self.tab == chrome::Tab::Home {
            let strip = self.tab_strip(ctx, true).or(cycled);
            let picked = self.palette_overlay(ctx, false);
            self.home_screen(ctx);
            let keyed = commands::pressed(ctx, false, self.doc.view.artboard_in).filter(|c| !c.needs_canvas());
            if let Some(command) = strip.or(picked).or(keyed) {
                self.run(command, ctx);
            }
            if self.docs().any(|d| d.flow.is_some()) {
                ctx.request_repaint_after(POLL_INTERVAL);
            }
            self.autosave_frame(ctx);
            self.tutorial_frame(ctx);
            crate::debug_log::frame_end(ctx, &self.doc.selection, self.doc.flow.as_ref().map(|f| (f.step, f.current, f.mats.len())));
            return;
        }
        let before = self.doc.flow.is_none().then(|| self.doc.design.clone());
        // The remembered duplicate step belongs to its copies; selecting anything else drops it (spec 52).
        if self.doc.duplicate_memory.as_ref().is_some_and(|m| m.ids != self.doc.selection) {
            self.doc.duplicate_memory = None;
        }
        self.doc.open_master();
        self.clipboard_input(ctx);
        let units = self.units;
        // Shown first so it takes Escape before the canvas does; while it is open no command runs under it.
        let modal = self.doc.cut_preview.is_some();
        if let Some(preview) = &mut self.doc.cut_preview
            && !preview.show(ctx, self.units)
        {
            self.doc.cut_preview = None;
        }
        let picked = if self.doc.flow.is_none() && self.doc.trace.is_none() { self.palette_overlay(ctx, true) } else { None };
        let strip = self.tab_strip(ctx, false).or(cycled);
        let action = match self.doc.flow {
            None if self.doc.trace.is_some() => {
                self.trace_screen(ctx);
                None
            }
            None => self.canvas_chrome(ctx),
            Some(_) => {
                self.flow_top_bar(ctx);
                if let Some(flow) = &self.doc.flow {
                    project_preview::panel(ctx, flow, self.units);
                }
                self.payload_panel(ctx);
                None
            }
        };
        let isolation = self.doc.isolation_chrome(ctx);
        let mut action = strip.or(picked).or(action).or(isolation);
        let raised = self.central(ctx);
        // A transform gesture that just ended becomes the step Transform Again repeats (spec 52). It is
        // not a Duplicate, so Cmd+D stays a plain Duplicate.
        if let Some((from, to)) = self.doc.view.gesture_step.take() {
            self.doc.duplicate_memory = Some(crate::repeat::DuplicateMemory { from, to, ids: self.doc.selection.clone(), from_duplicate: false });
        }
        action = action.or(raised).or(self.scripted.take());
        if modal {
            action = None;
        }
        // Commands apply before `observe` so each is one undo step, except those `after_observe` names.
        // Edits, and Rename, which names a layer of what Layers lists, run on the open master; app commands
        // on the whole design, once it is closed.
        if let Some(command) = action.take_if(|c| matches!(c, commands::Command::Edit(_) | commands::Command::App(AppCommand::Rename) | commands::Command::Clipboard(_)))
        {
            self.run(command, ctx);
        }
        self.doc.design.leave_master();
        if let Some(command) = action.take_if(|c| !matches!(c, commands::Command::App(app) if app.after_observe())) {
            self.run(command, ctx);
        }
        if let Some(before) = before {
            if before != self.doc.design {
                // Panels painted earlier this frame (the header's Make, the toolbar) still show the old design.
                ctx.request_repaint();
            }
            // An in-place text edit holds its gesture open, so the whole session is one undo step.
            let pointer_down = ctx.input(|i| i.pointer.any_down()) || self.doc.view.text_edit.is_some();
            self.doc.design.settle(Some(&before));
            let (diff, depth) = (crate::debug_log::DesignDiff::between(&before, &self.doc.design), self.doc.history.depth());
            self.doc.history.observe(before, &self.doc.design, pointer_down);
            if let Some(d) = diff {
                d.log("edit", Some(self.doc.history.depth() > depth))
            }
        }
        if let Some(action) = action {
            let pre = tracing::enabled!(target: "design", tracing::Level::DEBUG).then(|| (self.doc.design.clone(), format!("{action:?}")));
            self.run(action, ctx);
            if let Some((d, label)) = pre.and_then(|(pre, label)| crate::debug_log::DesignDiff::between(&pre, &self.doc.design).map(|d| (d, label))) {
                d.log(&label, None)
            }
        }
        if self.units != units {
            self.save_settings();
        }
        if self.docs().any(|d| d.flow.is_some()) {
            ctx.request_repaint_after(POLL_INTERVAL);
        }
        self.autosave_frame(ctx);
        self.tutorial_frame(ctx);
        crate::debug_log::frame_end(ctx, &self.doc.selection, self.doc.flow.as_ref().map(|f| (f.step, f.current, f.mats.len())));
    }
}

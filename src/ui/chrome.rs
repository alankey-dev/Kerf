use egui::{Align, Align2, Color32, Context, FontId, Frame, Layout, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::History;
use crate::edit::{AppCommand, Edit};
use crate::units::Units;

use super::anchors::{Anchor, mark};
use super::commands::{Command, Cx, Entry};
use super::flow_kit::{self, Kind};
use super::fonts;
use super::icons::{Icon, paint_icon, paint_icon_rotated};
use super::tokens::*;
use super::tooltip::tip;

pub fn text(painter: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, ink: Color32) -> Rect {
    painter.text(pos, anchor, text, font, ink)
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, TEXT_PRIMARY).size().x
}

/// Which screen the app shows. Home is reached via File > Home.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Canvas,
}

pub const FILE_MENU: [AppCommand; 6] =
    [AppCommand::New, AppCommand::Home, AppCommand::Save, AppCommand::VersionHistory, AppCommand::Export, AppCommand::PrepareForDesignSpace];

/// The Edit menu, in groups split by a separator.
pub const EDIT_MENU: [&[Command]; 3] = [
    &[
        Command::App(AppCommand::Undo),
        Command::App(AppCommand::Redo),
        Command::Edit(Edit::Duplicate),
        Command::Clipboard(super::clipboard::Action::Cut),
        Command::Clipboard(super::clipboard::Action::Copy),
        Command::Clipboard(super::clipboard::Action::CopyAsImage),
        Command::Clipboard(super::clipboard::Action::Paste),
        Command::Clipboard(super::clipboard::Action::PasteInPlace),
        Command::Clipboard(super::clipboard::Action::PasteStyle),
        Command::Edit(Edit::Delete),
    ],
    &[
        Command::Edit(Edit::Group),
        Command::Edit(Edit::Ungroup),
        Command::Edit(Edit::Attach),
        Command::Edit(Edit::Detach),
        Command::Edit(Edit::Contain),
        Command::Edit(Edit::Uncontain),
        Command::Edit(Edit::MakeComponent),
        Command::Edit(Edit::DetachInstance),
    ],
    &[Command::Edit(super::commands::TRANSFORM_AGAIN), Command::App(AppCommand::StepRepeatPanel), Command::App(AppCommand::RotateCopiesPanel)],
];

/// What the top bar needs to render and edit: the document title, the view settings, and what its
/// commands resolve against.
pub struct TopBar<'a> {
    pub title: &'a mut String,
    pub view: &'a mut super::canvas::View,
    pub units: &'a mut Units,
    pub cx: Cx<'a>,
    pub history: &'a History,
    /// "Saved", "Edited" or "Autosaved 10:42", beside the title.
    pub status: &'a str,
}

impl TopBar<'_> {
    pub fn entry(&self, command: &Command) -> Entry {
        self.cx.command(command, self.history)
    }
}

/// The 40 px top bar: brand mark, File/Edit/View menus, an inline-editable title, and the right actions.
pub fn top_bar(ctx: &Context, st: TopBar) -> Option<Command> {
    bar_shell(ctx, |ui| {
        let full = ui.max_rect();
        let mut cmd = None;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            wordmark(ui);
            ui.add_space(6.0);
            cmd = cmd.take().or(file_menu(ui, &st));
            cmd = cmd.take().or(edit_menu(ui, &st));
            let view_cmd = view_menu(ui, &st.cx, st.history, st.view, st.units);
            cmd = cmd.take().or(view_cmd);
            cmd = cmd.take().or(help_menu(ui, &st));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (app, kind) in [(AppCommand::Make, Kind::Primary), (AppCommand::Preview, Kind::Accent)] {
                    let e = st.entry(&Command::App(app.clone()));
                    let resp = flow_kit::button(ui, e.label, kind, e.enabled());
                    if app == AppCommand::Make {
                        mark(ui, Anchor::Make, resp.rect);
                    }
                    if resp.clicked() {
                        cmd = Some(e.command);
                    }
                }
                let export = st.entry(&Command::App(AppCommand::Export));
                let resp = bar_button(ui, export.info.short.unwrap_or(export.label), false);
                mark(ui, Anchor::Export, resp.rect);
                if resp.clicked() {
                    cmd = Some(export.command);
                }
            });
        });
        // The title sits centred, editable in place.
        let title_rect = Rect::from_center_size(pos2(full.center().x, full.center().y), vec2(220.0, 22.0));
        let edit = egui::TextEdit::singleline(st.title).frame(false).horizontal_align(Align::Center).font(fonts::medium(13.0));
        ui.put(title_rect, edit).on_hover_text("Rename");
        let title_w = text_width(ui, st.title, fonts::medium(13.0)).min(title_rect.width());
        let at = pos2(full.center().x + title_w / 2.0 + 10.0, full.center().y);
        let status = text(ui.painter(), at, Align2::LEFT_CENTER, st.status, fonts::regular(12.0), TEXT_MUTED);
        mark(ui, Anchor::SaveStatus, status.expand2(vec2(6.0, 4.0)));
        ui.interact(status, ui.id().with("save-status"), Sense::hover()).widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, st.status));
        cmd
    })
}

/// What the Make flow's top bar shows: where the user is, the connected machine and the payload toggle.
pub struct FlowBar<'a> {
    pub step: &'a str,
    pub payload: &'a mut bool,
    pub machine: Option<String>,
}

/// The canvas top bar during the Make flow: the wordmark and a breadcrumb from the canvas. No menus,
/// since New, Home and Undo would edit the design under an open flow.
pub fn flow_top_bar(ctx: &Context, st: FlowBar) {
    bar_shell(ctx, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            wordmark(ui);
            ui.add_space(12.0);
            breadcrumb(ui, st.step);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if bar_button(ui, "Payload", *st.payload).clicked() {
                    *st.payload = !*st.payload;
                }
                if let Some(machine) = &st.machine {
                    ui.label(egui::RichText::new(machine).font(fonts::regular(13.0)).color(TEXT_MUTED));
                }
            });
        });
    });
}

fn breadcrumb(ui: &mut Ui, step: &str) {
    let muted = fonts::regular(13.0);
    let current = fonts::medium(13.0);
    let w = text_width(ui, "Canvas", muted.clone()) + 24.0 + text_width(ui, step, current.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, TOP_BAR_H), Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, format!("Canvas > {step}")));
    let y = rect.center().y;
    let end = text(ui.painter(), pos2(rect.left(), y), Align2::LEFT_CENTER, "Canvas", muted, TEXT_MUTED).right();
    let chevron = Rect::from_center_size(pos2(end + 12.0, y), Vec2::splat(14.0));
    paint_icon_rotated(ui, Icon::Chevron, chevron, TEXT_MUTED, -std::f32::consts::FRAC_PI_2);
    text(ui.painter(), pos2(end + 24.0, y), Align2::LEFT_CENTER, step, current, TEXT_PRIMARY);
}

/// The 40 px white bar with a bottom hairline, clear of the macOS traffic lights.
fn bar_shell<R>(ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> R {
    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: TRAFFIC_LIGHT_PAD as i8, right: 10, top: 0, bottom: 0 });
    egui::TopBottomPanel::top("top-bar")
        .exact_height(TOP_BAR_H)
        .frame(frame)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let full = ui.max_rect();
            ui.painter().hline(full.x_range().expand(10.0), full.bottom() - 0.5, Stroke::new(1.0_f32, BORDER_SUBTLE));
            ui.style_mut().spacing.button_padding = vec2(8.0, 4.0);
            flat_buttons(ui);
            add(ui)
        })
        .inner
}

/// Menu titles read as flat text: no resting fill or stroke, an accent_hover wash on hover and while open.
fn flat_buttons(ui: &mut Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    for w in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        w.bg_stroke = Stroke::NONE;
        w.weak_bg_fill = Color32::TRANSPARENT;
    }
    widgets.hovered.weak_bg_fill = ACCENT_HOVER;
    widgets.active.weak_bg_fill = ACCENT_HOVER;
    widgets.open.weak_bg_fill = ACCENT_HOVER;
}

fn file_menu(ui: &mut Ui, st: &TopBar) -> Option<Command> {
    let mut cmd = None;
    let title = ui.menu_button("File", |ui| {
        for app in FILE_MENU {
            cmd = cmd.take().or(menu_item(ui, st.entry(&Command::App(app))));
        }
    });
    mark(ui, Anchor::MenuFile, title.response.rect);
    cmd
}

/// The Help menu: the interactive tutorial, the shortcut list and the version.
pub const HELP_MENU: [AppCommand; 3] = [AppCommand::Tutorial, AppCommand::Shortcuts, AppCommand::About];

fn help_menu(ui: &mut Ui, st: &TopBar) -> Option<Command> {
    let mut cmd = None;
    let title = ui.menu_button("Help", |ui| {
        for app in HELP_MENU {
            cmd = cmd.take().or(menu_item(ui, st.entry(&Command::App(app))));
        }
    });
    mark(ui, Anchor::MenuHelp, title.response.rect);
    cmd
}

fn edit_menu(ui: &mut Ui, st: &TopBar) -> Option<Command> {
    let mut cmd = None;
    let title = ui.menu_button("Edit", |ui| {
        for (i, group) in EDIT_MENU.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            for command in group.iter() {
                cmd = cmd.take().or(menu_item(ui, st.entry(command)));
            }
        }
        ui.separator();
        cmd = cmd.take().or(menu_item(ui, st.entry(&Command::Edit(Edit::SelectAll))));
        cmd = cmd.take().or(menu_item(ui, st.entry(&Command::Edit(Edit::SelectInverse))));
        cmd = cmd.take().or(select_same_menu(ui, st));
        ui.separator();
        cmd = cmd.take().or(menu_item(ui, st.entry(&Command::App(AppCommand::ReplaceColour))));
        cmd = cmd.take().or(menu_item(ui, st.entry(&Command::App(AppCommand::SavePaletteToProject))));
        cmd = cmd.take().or(menu_item(ui, st.entry(&Command::App(AppCommand::CopyPaletteToSettings))));
    });
    mark(ui, Anchor::MenuEdit, title.response.rect);
    cmd
}

/// The Select Same submenu (spec 50): one row per match, each greyed with its reason when the selection
/// does not share that value. The whole submenu is greyed when no row can run, as with nothing selected.
fn select_same_menu(ui: &mut Ui, st: &TopBar) -> Option<Command> {
    use crate::select::Match;
    let entries: Vec<_> = Match::MENU.iter().map(|&m| st.entry(&Command::Edit(Edit::SelectSame(m)))).collect();
    let any = entries.iter().any(Entry::enabled);
    let mut cmd = None;
    ui.add_enabled_ui(any, |ui| {
        ui.menu_button("Select Same", |ui| {
            for e in entries {
                cmd = cmd.take().or(menu_item(ui, e));
            }
        });
    });
    cmd
}

/// A menu button with its shortcut on the right, greyed while the command cannot run.
fn menu_item(ui: &mut Ui, e: Entry) -> Option<Command> {
    let keys = e.keys(ui.ctx()).unwrap_or_default();
    let resp = ui.add_enabled(e.enabled(), egui::Button::new(e.label).shortcut_text(keys));
    let resp = if e.enabled() { resp } else { resp.on_disabled_hover_text(e.tip_body()) };
    resp.clicked().then(|| {
        ui.close_menu();
        e.command
    })
}

/// The View menu's command rows, in groups split by a separator.
pub const VIEW_MENU: [&[AppCommand]; 3] = [
    &[AppCommand::Palette, AppCommand::CutPreview],
    &[AppCommand::ZoomIn, AppCommand::ZoomOut, AppCommand::ResetZoom],
    &[AppCommand::ZoomToFit, AppCommand::ZoomToSelection, AppCommand::ZoomToAll],
];

fn view_menu(ui: &mut Ui, cx: &Cx, history: &History, view: &mut super::canvas::View, units: &mut Units) -> Option<Command> {
    let mut cmd = None;
    ui.menu_button("View", |ui| {
        for group in VIEW_MENU {
            for app in group {
                cmd = cmd.take().or(menu_item(ui, cx.app(app.clone(), history)));
            }
            ui.separator();
        }
        cmd = cmd.take().or(super::guides::menu_rows(ui, cx.design));
        ui.separator();
        ui.label(egui::RichText::new("Units").color(TEXT_MUTED));
        ui.horizontal(|ui| {
            ui.selectable_value(units, Units::Imperial, "Inches");
            ui.selectable_value(units, Units::Metric, "Centimetres");
        });
        ui.label(egui::RichText::new("Grid").color(TEXT_MUTED));
        let grid = view.grid;
        if let Some(visible) = super::grid_panel::visibility_row(ui, grid.visible) {
            cmd = Some(Command::Edit(Edit::Grid(crate::grid::GridSettings { visible, ..grid })));
        }
        cmd = cmd.take().or(menu_item(ui, cx.app(AppCommand::GridSettings, history)));
        ui.separator();
        let mut snapping = cx.design.snapping();
        if ui.checkbox(&mut snapping, "Magnetic snap").changed() {
            cmd = Some(Command::App(AppCommand::Snapping));
        }
        cmd = cmd.take().or(menu_item(ui, cx.app(AppCommand::SnapSettings, history)));
        super::op_legend::show_as_made_toggle(ui, &mut view.mode);
    });
    cmd
}

/// The Kerf wordmark in a rounded violet chip.
fn wordmark(ui: &mut Ui) {
    let font = fonts::semibold(13.0);
    let w = text_width(ui, "Kerf", font.clone()) + 16.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, TOP_BAR_H), Sense::hover());
    let chip = Rect::from_center_size(rect.center(), vec2(w, 22.0));
    ui.painter().rect_filled(chip, 5.0, ACCENT_PRIMARY);
    text(ui.painter(), chip.center(), Align2::CENTER_CENTER, "Kerf", font, ON_PRIMARY);
}

/// A flat text button for the top bar's right actions; `active` holds the accent tint, as a rail item does.
fn bar_button(ui: &mut Ui, label: &str, active: bool) -> Response {
    let font = fonts::medium(13.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(text_width(ui, label, font.clone()) + 20.0, 28.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    if active || resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, ACCENT_HOVER);
    }
    let ink = if active { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, font, ink);
    resp
}

pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), sense);
    let resp = tip(resp, label, "", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, ACCENT_HOVER);
    }
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    paint_icon(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), ink);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RailEvent {
    Shapes,
    Monogram,
    Upload,
    CreateAi,
    Tool(super::canvas::Tool),
}

/// The 48 px tool rail: a flat, borderless stack of icon buttons with a 1px right border.
pub fn rail(ctx: &Context, shapes_open: bool, monogram_open: bool, create_ai_open: bool, tool: super::canvas::Tool) -> Option<RailEvent> {
    use super::canvas::Tool;
    let frame = Frame::NONE.fill(BG_PANEL).inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 });
    let panel = egui::SidePanel::left("rail").exact_width(RAIL_W).resizable(false).frame(frame).show_separator_line(false);
    panel
        .show(ctx, |ui| {
            ui.painter().vline(ui.max_rect().right() - 0.5, ui.max_rect().y_range().expand(6.0), Stroke::new(1.0_f32, BORDER_SUBTLE));
            ui.spacing_mut().item_spacing.y = 2.0;
            let top = ui.cursor().top();
            let mut event = None;
            let measure = format!("Drag between two points to measure them. Shift keeps to 45 degrees, {} copies.", ctx.format_shortcut(&super::measure_tool::COPY));
            for (icon, label, t, body, key) in [
                (Icon::Cursor, "Select", Tool::Select, "Select, move, resize and rotate objects.", "V"),
                (Icon::Pen, "Pen", Tool::Pen, "Click to place points, drag for curves. Enter or a click on the first point finishes.", "P"),
                (Icon::Pencil, "Pencil", Tool::Pencil, "Draw freehand. The stroke is smoothed into a curve.", "N"),
                (Icon::Text, "Text", Tool::Text, "Click the canvas to add a line of text.", "T"),
                (Icon::Measure, "Measure", Tool::Measure, measure.as_str(), "M"),
                (Icon::Corner, "Corner", Tool::Corner, "Round, chamfer or scoop a path's corners. Drag a node's widget or type a radius.", "C"),
                (
                    Icon::ShapeBuilder,
                    "Shape Builder",
                    Tool::ShapeBuilder,
                    "Drag across overlapping shapes to unite them, Option-drag to delete. Select up to 8 shapes.",
                    "\u{21e7}M",
                ),
                (Icon::Knife, "Knife", Tool::Knife, "Drag to cut shapes along a line, Shift keeps it straight. Click a path to snip it.", "K"),
                (
                    Icon::Eyedropper,
                    "Eyedropper",
                    Tool::Eyedropper,
                    "Sample a colour and style from any object, then click to apply it. Hold Option to lift colour only.",
                    "I",
                ),
            ] {
                if rail_item(ui, icon, label, tool == t, body, Some(key)).clicked() {
                    event = Some(RailEvent::Tool(t));
                }
            }
            let shapes = rail_item(ui, Icon::Shapes, "Shapes", shapes_open, "Add a basic shape to the canvas.", None);
            mark(ui, Anchor::RailShapes, shapes.rect);
            if shapes.clicked() {
                event = Some(RailEvent::Shapes);
            }
            if rail_item(ui, Icon::Monogram, "Monogram", monogram_open, "Build initials inside a frame.", None).clicked() {
                event = Some(RailEvent::Monogram);
            }
            let upload = rail_item(ui, Icon::Upload, "Upload", false, super::commands::info(&Command::App(AppCommand::Upload)).tip, None);
            mark(ui, Anchor::RailUpload, upload.rect);
            if upload.clicked() {
                event = Some(RailEvent::Upload);
            }
            if rail_item(ui, Icon::Effects, "Create AI", create_ai_open, "Describe a design and Kerf draws it for cutting.", None).clicked() {
                event = Some(RailEvent::CreateAi);
            }
            mark(ui, Anchor::Rail, Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top()));
            event
        })
        .inner
}

/// A flat 44 px icon button. Active: accent_hover background with an accent icon. Hover: accent_hover.
fn rail_item(ui: &mut Ui, icon: Icon, label: &str, active: bool, body: &str, shortcut: Option<&str>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(RAIL_W, 44.0), Sense::click());
    let resp = tip(resp, label, body, shortcut);
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, label));
    let (bg, ink) = match (active, resp.hovered()) {
        (true, _) => (ACCENT_HOVER, ACCENT_PRIMARY),
        (false, true) => (ACCENT_HOVER, TEXT_PRIMARY),
        (false, false) => (Color32::TRANSPARENT, TEXT_PRIMARY),
    };
    ui.painter().rect_filled(rect.shrink2(vec2(6.0, 2.0)), 6.0, bg);
    paint_icon(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), ink);
    resp
}

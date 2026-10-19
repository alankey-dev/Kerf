//! The command palette overlay (spec 32). Cmd+K opens a search over every command, shape, font and
//! recent project. Rows come from the command table and resolve through `Cx`, so a row is enabled
//! exactly when the same command is enabled everywhere else, and a pick runs through the app's one
//! command path.

use egui::{
    Align2, Area, Color32, Context, Frame, Id, Key, KeyboardShortcut, Modifiers, Order, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::design::{History, ShapeKind};
use crate::edit::{Blocked, Edit, Insert};
use crate::palette::{self, Candidate, Recents, Section};
use crate::text::FontRef;
use crate::text_ops::TextChange;

use super::chrome::text;
use super::commands::{self, Command, Cx};
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::tokens::*;

/// The most rows the list renders for a query.
pub const MAX_ROWS: usize = 50;
pub const RECENT_PROJECTS: usize = 10;
const WIDTH: f32 = 560.0;
const LIST_H: f32 = 420.0;
const ROW_H: f32 = 32.0;
const ROW_WITH_REASON_H: f32 = 48.0;
const HEADER_H: f32 = 24.0;
/// How long Enter on a disabled row highlights its reason.
const FLASH_S: f64 = 1.5;
pub const SEARCH_LABEL: &str = "Command palette search";

/// One thing the palette can run, resolved for this frame.
#[derive(Clone, Debug)]
pub struct Row {
    pub command: Command,
    /// Names the row across sessions in the recent-use history.
    pub key: String,
    pub label: String,
    pub section: Section,
    pub icon: Option<Icon>,
    pub shortcut: Option<KeyboardShortcut>,
    pub blocked: Option<Blocked>,
    pub aliases: &'static [&'static str],
}

/// What the rows are built from.
pub struct Sources<'a> {
    pub cx: Cx<'a>,
    pub history: &'a History,
    /// Off on Home, where commands that need the canvas are left out.
    pub on_canvas: bool,
    pub system_fonts: &'a [String],
    pub writing_fonts: &'a [crate::system_fonts::WritingFont],
    /// Recently opened projects as (id, name), newest first.
    pub projects: &'a [(String, String)],
    pub board_in: (f32, f32),
}

/// Every row the palette can show: the command registry, then shapes, fonts and recent projects.
pub fn rows(src: &Sources) -> Vec<Row> {
    let row = |command: &Command, section: Section, label: Option<String>, key: String| {
        let e = src.cx.command(command, src.history);
        let label = label.unwrap_or_else(|| if e.label == e.info.label { commands::title(&e.command).into_owned() } else { e.label.to_owned() });
        Row {
            key: format!("{}:{key}", section.label()),
            label,
            section,
            icon: e.info.icon,
            shortcut: e.info.shortcut,
            blocked: e.blocked,
            aliases: commands::aliases(&e.command),
            command: e.command,
        }
    };
    let registry = commands::registry(src.board_in).into_iter().map(|c| row(&c, Section::Commands, None, commands::title(&c).into_owned()));
    let shapes = ShapeKind::free().into_iter().map(|kind| {
        let label = kind.label().to_owned();
        row(&Command::Edit(Edit::Insert(Insert::Shape(kind))), Section::Shapes, None, label)
    });
    let mut families: Vec<FontRef> = FontRef::BUNDLED.into();
    families.extend(src.writing_fonts.iter().map(|w| FontRef::Stroke(w.id.clone())));
    families.extend(src.system_fonts.iter().map(|f| FontRef::from(f.clone())).filter(|f| !FontRef::BUNDLED.contains(f)));
    let fonts = families.into_iter().map(|font| {
        let set = Edit::Text(TextChange::Font(font.clone()));
        let label = font.label().into_owned();
        let either = src.cx.either(Edit::Insert(Insert::Text(font)), set);
        row(&either.command, Section::Fonts, Some(label.clone()), label)
    });
    let projects = src.projects.iter().map(|(id, name)| row(&Command::App(crate::edit::AppCommand::Open(id.clone())), Section::Projects, Some(name.clone()), id.clone()));
    registry.chain(shapes).chain(fonts).chain(projects).filter(|r| src.on_canvas || !r.command.needs_canvas()).collect()
}

/// The palette between frames: whether it is open, the query, the highlighted row and the history of
/// runs that ranks rows and fills Recent.
#[derive(Default)]
pub struct Palette {
    open: bool,
    query: String,
    cursor: usize,
    projects: Vec<(String, String)>,
    /// A disabled row's key and the time its reason stops flashing.
    flash: Option<(String, f64)>,
    /// The row Cmd+Enter just ran, which the highlight follows when its use reranks the list.
    follow: Option<String>,
    focus: bool,
    /// Lists only commands with a shortcut, as Help > Keyboard shortcuts opens it.
    keyed: bool,
    pub recents: Recents,
}

impl Palette {
    pub fn new(recents: Recents) -> Self {
        Self { recents, ..Self::default() }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn projects(&self) -> &[(String, String)] {
        &self.projects
    }

    /// Opens with an empty query over `projects`, or closes.
    pub fn toggle(&mut self, projects: impl FnOnce() -> Vec<(String, String)>) {
        if self.open {
            self.open = false;
        } else {
            *self = Self { open: true, focus: true, projects: projects(), recents: std::mem::take(&mut self.recents), ..Self::default() };
        }
    }

    /// Opens listing only the commands that have a shortcut.
    pub fn open_shortcuts(&mut self, projects: impl FnOnce() -> Vec<(String, String)>) {
        self.open = false;
        self.toggle(projects);
        self.keyed = true;
    }

    pub fn keyed(&self) -> bool {
        self.keyed
    }

    /// The rows the list shows, in order, each with the section it is listed under: Recent then
    /// Suggested for an empty query, else the best matches.
    pub fn listing(&self, rows: &[Row], now: u64) -> Vec<(Section, usize)> {
        if self.query.trim().is_empty() {
            let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
            let suggested: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.blocked.is_none() && commands::suggested(&r.command)).map(|(i, _)| i).collect();
            palette::browse(&keys, &self.recents, &suggested)
        } else {
            let candidates: Vec<Candidate> = rows.iter().map(|r| Candidate { key: &r.key, label: &r.label, aliases: r.aliases }).collect();
            palette::rank(&self.query, &candidates, &self.recents, now).into_iter().take(MAX_ROWS).map(|i| (rows[i].section, i)).collect()
        }
    }

    /// Shows the palette while open. Returns the command picked this frame, already recorded as used.
    pub fn show(&mut self, ctx: &Context, rows: &[Row]) -> Option<Command> {
        if !self.open {
            return None;
        }
        let now = crate::project::now();
        let time = ctx.input(|i| i.time);
        let shown = self.listing(rows, now);
        if let Some(key) = self.follow.take()
            && let Some(n) = shown.iter().position(|&(_, i)| rows[i].key == key)
        {
            self.cursor = n;
        }
        let keys = ctx.input_mut(|i| Keys {
            close: i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K)),
            down: i.consume_key(Modifiers::NONE, Key::ArrowDown),
            up: i.consume_key(Modifiers::NONE, Key::ArrowUp),
            page_down: i.consume_key(Modifiers::NONE, Key::PageDown),
            page_up: i.consume_key(Modifiers::NONE, Key::PageUp),
            run_keep: i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)),
            run: i.consume_key(Modifiers::NONE, Key::Enter),
        });
        if keys.close {
            self.open = false;
            return None;
        }
        let last = shown.len().saturating_sub(1);
        let before = self.cursor;
        self.cursor = match () {
            _ if keys.down => (self.cursor + 1).min(last),
            _ if keys.up => self.cursor.saturating_sub(1),
            _ if keys.page_down => section_after(&shown, self.cursor),
            _ if keys.page_up => section_before(&shown, self.cursor),
            _ => self.cursor.min(last),
        };
        let moved = self.cursor != before;
        let mut picked = (keys.run || keys.run_keep).then_some((self.cursor, keys.run_keep)).filter(|_| !shown.is_empty());

        let screen = ctx.screen_rect();
        let scrim = Area::new(Id::new("palette-scrim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| ui.allocate_rect(screen, Sense::click()));
        if scrim.inner.clicked() {
            self.open = false;
            return None;
        }
        let frame = Frame::NONE.fill(BG_PANEL).corner_radius(10).stroke(Stroke::new(1.0_f32, BORDER_SUBTLE)).inner_margin(egui::Margin::same(8)).shadow(egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: SHADOW,
        });
        let flashing = self.flash.as_ref().filter(|(_, until)| *until > time).map(|(key, _)| key.clone());
        Area::new(Id::new("palette")).order(Order::Tooltip).anchor(Align2::CENTER_TOP, vec2(0.0, TOP_BAR_H + 48.0)).show(ctx, |ui| {
            let shown_at = frame.show(ui, |ui| {
                ui.set_width(WIDTH);
                if self.search(ui) {
                    self.cursor = 0;
                }
                ui.add_space(6.0);
                let full = ui.available_rect_before_wrap();
                ui.painter().hline(full.x_range().expand(8.0), full.top(), Stroke::new(1.0_f32, BORDER_SUBTLE));
                ui.add_space(6.0);
                if shown.is_empty() {
                    let hint = if self.query.trim().is_empty() { "Type to find a command, shape, font or project." } else { "Nothing matches." };
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(hint).font(fonts::regular(13.0)).color(TEXT_MUTED));
                    ui.add_space(8.0);
                    return;
                }
                let browsing = self.query.trim().is_empty();
                let headers = if browsing { 1 + shown.windows(2).filter(|w| w[0].0 != w[1].0).count() } else { 0 };
                let content: f32 = shown.iter().map(|&(_, i)| row_height(&rows[i])).sum::<f32>() + headers as f32 * HEADER_H;
                let height = content.min(LIST_H);
                egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (n, &(section, i)) in shown.iter().enumerate() {
                        if browsing && (n == 0 || shown[n - 1].0 != section) {
                            header(ui, section);
                        }
                        let row = &rows[i];
                        let resp = row_ui(ui, row, n == self.cursor, flashing.as_deref() == Some(row.key.as_str()), ctx);
                        if n == self.cursor && moved {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            picked = Some((n, false));
                        }
                    }
                });
            });
            super::anchors::mark(ui, super::anchors::Anchor::Palette, shown_at.response.rect);
        });

        let (n, keep) = picked?;
        let row = &rows[shown[n].1];
        self.cursor = n;
        if row.blocked.is_some() {
            self.flash = Some((row.key.clone(), time + FLASH_S));
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(FLASH_S));
            return None;
        }
        self.recents.record(&row.key, now);
        self.open = keep;
        self.follow = keep.then(|| row.key.clone());
        Some(row.command.clone())
    }

    /// The search field, focused when the palette opens. True when the query changed.
    fn search(&mut self, ui: &mut Ui) -> bool {
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(Id::new("palette-search"))
            .hint_text(if self.keyed { "Search keyboard shortcuts" } else { "Search commands, shapes, fonts and projects" })
            .font(fonts::regular(15.0))
            .frame(false)
            .margin(vec2(8.0, 8.0))
            .desired_width(f32::INFINITY);
        let resp = ui.add(edit);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, SEARCH_LABEL));
        if std::mem::take(&mut self.focus) {
            resp.request_focus();
        }
        resp.changed()
    }
}

struct Keys {
    close: bool,
    down: bool,
    up: bool,
    page_down: bool,
    page_up: bool,
    run_keep: bool,
    run: bool,
}

/// The first row of the next section, or the last row.
fn section_after(shown: &[(Section, usize)], at: usize) -> usize {
    let here = shown.get(at).map(|s| s.0);
    (at + 1..shown.len()).find(|&n| Some(shown[n].0) != here).unwrap_or(shown.len().saturating_sub(1))
}

/// The first row of this section, or of the one before when already there.
fn section_before(shown: &[(Section, usize)], at: usize) -> usize {
    let start = |mut n: usize| {
        while n > 0 && shown[n - 1].0 == shown[n].0 {
            n -= 1;
        }
        n
    };
    match start(at) {
        n if n < at => n,
        0 => 0,
        n => start(n - 1),
    }
}

fn header(ui: &mut Ui, section: Section) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    text(ui.painter(), pos2(rect.left() + 8.0, rect.center().y + 2.0), Align2::LEFT_CENTER, section.label(), fonts::medium(11.0), TEXT_MUTED);
}

/// A row: icon, label, its section and shortcut on the right, and for a disabled row the reason it
/// cannot run underneath.
fn row_height(row: &Row) -> f32 {
    if row.blocked.is_some() { ROW_WITH_REASON_H } else { ROW_H }
}

fn row_ui(ui: &mut Ui, row: &Row, selected: bool, flashing: bool, ctx: &Context) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_height(row)), Sense::click());
    let enabled = row.blocked.is_none();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, &row.label));
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(rect, 6.0, ACCENT_HOVER);
    }
    let line = if enabled { rect.center().y } else { rect.top() + 16.0 };
    let ink = if enabled { TEXT_PRIMARY } else { DISABLED_TEXT };
    if let Some(icon) = row.icon {
        paint_icon(ui, icon, Rect::from_center_size(pos2(rect.left() + 18.0, line), Vec2::splat(18.0)), ink);
    }
    let painter = ui.painter();
    let mut right = rect.right() - 10.0;
    if let Some(keys) = row.shortcut {
        right = text(painter, pos2(right, line), Align2::RIGHT_CENTER, &ctx.format_shortcut(&keys), fonts::regular(12.0), TEXT_MUTED).left() - 12.0;
    }
    let section = text(painter, pos2(right, line), Align2::RIGHT_CENTER, row.section.label(), fonts::regular(12.0), TEXT_MUTED);
    let label_clip = Rect::from_min_max(rect.min, pos2(section.left() - 8.0, rect.bottom()));
    painter.with_clip_rect(label_clip).text(pos2(rect.left() + 36.0, line), Align2::LEFT_CENTER, &row.label, fonts::regular(14.0), ink);
    if let Some(blocked) = row.blocked {
        let colour: Color32 = if flashing { WARNING_TEXT } else { TEXT_MUTED };
        if flashing {
            painter.rect_filled(Rect::from_min_max(pos2(rect.left() + 30.0, rect.top() + 26.0), pos2(rect.right() - 6.0, rect.bottom() - 4.0)), 4.0, WARNING_BG);
        }
        text(painter, pos2(rect.left() + 36.0, rect.top() + 36.0), Align2::LEFT_CENTER, &blocked.reason(), fonts::regular(12.0), colour);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(sections: &[Section]) -> Vec<(Section, usize)> {
        sections.iter().copied().zip(0..).collect()
    }

    use crate::containers::AlignTo;
    use crate::design::Design;
    use crate::edit::AppCommand;

    /// Squares 1 and 2, a container 3 made of a circle, and a text 4.
    fn mixed() -> Design {
        let mut d = Design::default();
        d.add_shape(ShapeKind::Square);
        d.add_shape(ShapeKind::Square);
        let c = d.add_shape(ShapeKind::Circle);
        d.contain(&[c]);
        d.add_text("Hi".into(), FontRef::Inter, 1.0, Color32::BLACK, 1.0, 6.0);
        d
    }

    const BOARD: (f32, f32) = (12.0, 12.0);
    const FAMILIES: [&str; 2] = ["Futura", "Inter"];

    fn rows_for(d: &Design, on: &[u64], on_canvas: bool) -> Vec<Row> {
        let families: Vec<String> = FAMILIES.map(String::from).into();
        let projects = [("p1".to_owned(), "Birthday card".to_owned())];
        let history = History::default();
        let src = Sources {
            cx: Cx::new(d, on, AlignTo::Container),
            history: &history,
            on_canvas,
            system_fonts: &families,
            writing_fonts: &[],
            projects: &projects,
            board_in: BOARD,
        };
        rows(&src)
    }

    #[test]
    fn every_registered_command_shape_font_and_project_is_listed() {
        let d = mixed();
        let rows = rows_for(&d, &[1], true);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        for command in commands::registry(BOARD) {
            let title = commands::title(&command);
            assert!(rows.iter().any(|r| r.key == format!("Command:{title}")), "{command:?} is missing");
        }
        for kind in ShapeKind::free() {
            assert!(labels.contains(&kind.label()), "{} is missing", kind.label());
        }
        for font in ["Inter", "Space Grotesk", "Futura", "Birthday card"] {
            assert_eq!(labels.iter().filter(|l| **l == font).count(), 1, "{font} is listed once");
        }
        let mut keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), rows.len(), "every row has its own key");
    }

    #[test]
    fn every_row_is_enabled_exactly_when_the_model_says() {
        let d = mixed();
        let history = History::default();
        for on in [&[][..], &[1], &[1, 2], &[1, 3], &[3], &[4], &[1, 2, 3, 4]] {
            for row in rows_for(&d, on, true) {
                let want = match &row.command {
                    Command::Edit(edit) => d.check(edit, on).err(),
                    Command::App(app) => app.check(&d, &history, on).err(),
                    Command::Clipboard(action) => {
                        let needs_selection = matches!(
                            action,
                            crate::ui::clipboard::Action::Cut
                                | crate::ui::clipboard::Action::Copy
                                | crate::ui::clipboard::Action::CopyAsImage
                                | crate::ui::clipboard::Action::PasteStyle
                        );
                        (needs_selection && on.is_empty()).then_some(Blocked::NothingSelected)
                    }
                };
                assert_eq!(row.blocked, want, "{} on {on:?}", row.label);
            }
        }
    }

    #[test]
    fn colour_and_color_both_find_every_set_colour_row() {
        let d = mixed();
        let rows = rows_for(&d, &[1], true);
        let candidates: Vec<Candidate> = rows.iter().map(|r| Candidate { key: &r.key, label: &r.label, aliases: r.aliases }).collect();
        let set_colour: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].label.starts_with("Set colour")).collect();
        assert!(set_colour.len() >= 8, "the registry lists a Set colour row per pen colour: {}", set_colour.len());
        for query in ["colour", "color"] {
            let found = palette::rank(query, &candidates, &Recents::default(), 0);
            for &row in &set_colour {
                assert!(found.contains(&row), "{query:?} finds {:?}", rows[row].label);
            }
            let top: Vec<&str> = found.iter().take(set_colour.len()).map(|&i| rows[i].label.as_str()).collect();
            assert!(top.iter().all(|l| l.starts_with("Set colour")), "the colour rows rank first for {query:?}: {top:?}");
        }
    }

    #[test]
    fn a_font_sets_selected_text_or_places_new_text() {
        let d = mixed();
        let font = |on: &[u64]| rows_for(&d, on, true).into_iter().find(|r| r.label == "Futura").unwrap().command;
        let futura = FontRef::System("Futura".into());
        assert_eq!(font(&[4]), Command::Edit(Edit::Text(TextChange::Font(futura.clone()))));
        assert_eq!(font(&[1]), Command::Edit(Edit::Insert(Insert::Text(futura))));
    }

    #[test]
    fn home_hides_what_needs_the_canvas() {
        let rows = rows_for(&mixed(), &[], false);
        assert!(rows.iter().all(|r| !r.command.needs_canvas()));
        for want in [AppCommand::New, AppCommand::Open("p1".into()), AppCommand::Units(crate::units::Units::Metric)] {
            assert!(rows.iter().any(|r| r.command == Command::App(want.clone())), "{want:?} is listed on Home");
        }
    }

    #[test]
    fn page_keys_jump_between_sections() {
        use Section::{Recent as R, Suggested as S};
        let list = shown(&[R, R, R, S, S]);
        assert_eq!(section_after(&list, 0), 3);
        assert_eq!(section_after(&list, 3), 4, "the last section ends at the last row");
        assert_eq!(section_before(&list, 4), 3, "to the start of this section");
        assert_eq!(section_before(&list, 3), 0, "then to the one before");
        assert_eq!(section_before(&list, 0), 0);
    }
}

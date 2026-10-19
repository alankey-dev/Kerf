//! The interactive tutorial (spec 39). [`STEPS`] is the tour as data; [`Tour`] walks it. The tour runs
//! in a transient "Tutorial" tab holding a sample card, so it never touches the user's work. Everything
//! a step does to the app (opening a panel, selecting the heart, "Show me") is an [`Act`], performed at
//! the start of the next frame through the same paths the UI uses: `Kerf::run` for commands, so edits go
//! through `edit::run` and are undo steps.

use egui::{Context, Key, KeyboardShortcut, Modifiers, Rect};

use super::anchors::{self, Anchor};
use super::canvas::Tool;
use super::chrome;
use super::commands::Command;
use super::document::Document;
use super::tutorial_overlay::{self as overlay, Card, Pressed, Spot};
use super::{Kerf, node_edit};
use crate::design::{Combine, CompoundOp, Design, ShapeKind};
use crate::edit::{AppCommand, Edit, Insert};
use crate::effects::{Direction, Effect, Offset, Reach};
use crate::geometry::Join;
use crate::text::FontRef;

/// Which screen a step shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    /// The Tutorial tab's canvas.
    Canvas,
    /// The Tutorial tab's Make flow, on Prepare.
    Make,
}

/// A piece of the sample card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Card,
    Heart,
    Text,
    Star,
}

/// What a step's card points at.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    /// Nothing: the card sits in the middle of the window.
    Centre,
    /// These widgets, together.
    Widgets(&'static [Anchor]),
    /// A piece of the sample, where the canvas draws it.
    Part(Part),
}

/// Whether a "do it" step is done, given what the app looked like when the step began.
pub type Done = fn(&Kerf, &Seen) -> bool;

/// One thing a step does to the app.
#[derive(Clone, Copy)]
pub enum Act {
    Shapes(bool),
    Select(&'static [Part]),
    Tool(Tool),
    /// Opens node editing on a part, or closes it.
    Nodes(Option<Part>),
    Palette(bool),
    /// A command, run as a menu pick runs it.
    Run(fn() -> Command),
    /// These acts, unless the check already holds.
    Ensure(Done, &'static [Act]),
    /// Shows the step's screen; every step starts with it.
    Screen(Screen),
}

/// How a step moves on: the Next button, or the user doing what it asks.
#[derive(Clone, Copy)]
pub enum Advance {
    Next,
    When { done: Done, show_me: &'static [Act] },
}

pub struct Step {
    pub section: &'static str,
    pub on: Screen,
    pub target: Target,
    pub title: &'static str,
    pub body: &'static str,
    pub keys: Option<KeyboardShortcut>,
    /// Puts the app in the state the step talks about, and again whenever its target goes missing.
    pub prepare: &'static [Act],
    pub advance: Advance,
}

/// What the app looked like when the step began.
#[derive(Clone, Copy, Default)]
pub struct Seen {
    shapes: usize,
}

const fn step(section: &'static str, on: Screen, target: Target, title: &'static str, body: &'static str) -> Step {
    Step { section, on, target, title, body, keys: None, prepare: &[], advance: Advance::Next }
}

const fn at(anchors: &'static [Anchor]) -> Target {
    Target::Widgets(anchors)
}

const CMD: Modifiers = Modifiers::COMMAND;

fn insert_circle() -> Command {
    Command::Edit(Edit::Insert(Insert::Shape(ShapeKind::Circle)))
}

fn outline() -> Command {
    Command::Edit(Edit::AddEffect {
        effect: Effect::Offset(Offset { distance_in: 0.15, direction: Direction::Outset, join: Join::Round, style: None }),
        reach: Reach::Together,
    })
}

fn subtract() -> Command {
    Command::Edit(Edit::Combine(Combine::Subtract))
}

fn make_component() -> Command {
    Command::Edit(Edit::MakeComponent)
}

fn shapes_open(app: &Kerf, _: &Seen) -> bool {
    app.doc.shapes_open
}

fn shape_added(app: &Kerf, seen: &Seen) -> bool {
    app.doc.design.shapes.len() > seen.shapes
}

fn heart_selected(app: &Kerf, _: &Seen) -> bool {
    app.part(Part::Heart).is_some_and(|id| app.doc.selection == [id])
}

fn text_outlined(app: &Kerf, _: &Seen) -> bool {
    app.part(Part::Text).and_then(|id| app.doc.design.shapes.iter().find(|s| s.id == id)).is_some_and(|s| !s.effects.is_empty())
}

fn combined(app: &Kerf, _: &Seen) -> bool {
    app.doc.design.shapes.iter().any(|s| matches!(s.kind, ShapeKind::Compound(CompoundOp::Boolean(_))))
}

fn editing_nodes(app: &Kerf, _: &Seen) -> bool {
    app.doc.view.nodes.is_some()
}

fn has_component(app: &Kerf, _: &Seen) -> bool {
    !app.doc.design.components.is_empty()
}

fn palette_open(app: &Kerf, _: &Seen) -> bool {
    app.palette.is_open()
}

fn making(app: &Kerf, _: &Seen) -> bool {
    app.doc.flow.is_some()
}

const HOME: &str = "Home";
const BASICS: &str = "Canvas basics";
const EDITING: &str = "Selecting and editing";
const OPERATIONS: &str = "Operations";
const TEXT: &str = "Text";
const ARRANGE: &str = "Arrange";
const EFFECTS: &str = "Effects";
const COMBINE: &str = "Combine";
const NODES: &str = "Nodes";
const COMPONENTS: &str = "Components";
const IMAGES: &str = "Images and patterns";
const PALETTE: &str = "Command palette";
const MAKE: &str = "Make";
const EXPORT: &str = "Export";
const SAVING: &str = "Saving";

/// The tour, in order.
pub static STEPS: &[Step] = &[
    step(
        HOME,
        Screen::Home,
        Target::Centre,
        "Welcome to Kerf",
        "This tour takes about five minutes. It uses a sample card in its own tab, so your projects stay untouched.",
    ),
    step(
        HOME,
        Screen::Home,
        at(&[Anchor::HomeProjects]),
        "Your projects",
        "Saved projects appear here as cards. Edit opens one on the canvas, and Make sends it to a machine.",
    ),
    Step {
        keys: Some(KeyboardShortcut::new(CMD, Key::T)),
        ..step(HOME, Screen::Home, at(&[Anchor::HomeNew]), "Start something new", "New Project opens an empty canvas in a new tab.")
    },
    step(
        HOME,
        Screen::Home,
        at(&[Anchor::TabStrip]),
        "Tabs",
        "Each open project gets a tab with its own undo history. The Tutorial tab holds the sample card and closes when the tour ends.",
    ),
    step(
        BASICS,
        Screen::Canvas,
        at(&[Anchor::Rail]),
        "Tools",
        "Select (V) moves and resizes. Pen (P) and Pencil (N) draw, Text (T) adds text and Measure (M) measures distances.",
    ),
    Step {
        prepare: &[Act::Shapes(false)],
        advance: Advance::When { done: shapes_open, show_me: &[Act::Shapes(true)] },
        ..step(BASICS, Screen::Canvas, at(&[Anchor::RailShapes]), "Open Shapes", "Click Shapes to open the shape library.")
    },
    Step {
        prepare: &[Act::Shapes(true)],
        advance: Advance::When { done: shape_added, show_me: &[Act::Run(insert_circle)] },
        ..step(BASICS, Screen::Canvas, at(&[Anchor::ShapesPanel]), "Add a shape", "Click any shape to add it to the canvas.")
    },
    Step {
        prepare: &[Act::Shapes(true)],
        ..step(
            BASICS,
            Screen::Canvas,
            at(&[Anchor::ShapesSearch]),
            "Search and filter",
            "Type a name to find a shape. Filter narrows the list to one category, such as Stars or Frames.",
        )
    },
    Step {
        prepare: &[Act::Shapes(false)],
        ..step(
            BASICS,
            Screen::Canvas,
            at(&[Anchor::RailUpload]),
            "Upload",
            "Upload adds SVG, PNG, JPG and WebP files. A picture opens the trace panel, and an SVG font becomes a writing font.",
        )
    },
    step(
        BASICS,
        Screen::Canvas,
        at(&[Anchor::ZoomControls, Anchor::Snapping]),
        "Zoom and snap",
        "Zoom in and out here, and click the percentage for a zoom level or a fit. With the magnet on, objects snap to each other as you drag.",
    ),
    step(BASICS, Screen::Canvas, at(&[Anchor::Rulers]), "Rulers", "The rulers measure in inches or centimetres. Click the corner where they meet to switch."),
    Step {
        prepare: &[Act::Shapes(false), Act::Nodes(None), Act::Tool(Tool::Select), Act::Select(&[])],
        advance: Advance::When { done: heart_selected, show_me: &[Act::Tool(Tool::Select), Act::Select(&[Part::Heart])] },
        ..step(EDITING, Screen::Canvas, Target::Part(Part::Heart), "Select the heart", "Click the heart to select it.")
    },
    Step {
        prepare: &[Act::Select(&[Part::Heart])],
        ..step(
            EDITING,
            Screen::Canvas,
            at(&[Anchor::SelectionFrame, Anchor::ActionBar]),
            "Move, resize, rotate",
            "Drag a corner to resize and the round handle to rotate. The bar below holds quick actions, and right-click shows the rest.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Heart])],
        ..step(
            EDITING,
            Screen::Canvas,
            at(&[Anchor::Properties]),
            "Exact values",
            "Fields do sums, so *2 doubles a size and 2in + 5mm mixes units. The padlocks stop a value changing.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Heart])],
        ..step(
            OPERATIONS,
            Screen::Canvas,
            at(&[Anchor::Operation]),
            "Operation",
            "Choose Cut, Pen, Score or Print Then Cut to set how the machine makes this object. Guide keeps it on screen but never makes it.",
        )
    },
    step(
        OPERATIONS,
        Screen::Canvas,
        at(&[Anchor::OpLegend]),
        "Legend",
        "Each operation draws in its own style, listed here. Click one to select its objects, or turn on View > Show as made to preview the result.",
    ),
    Step {
        prepare: &[Act::Select(&[Part::Text])],
        ..step(TEXT, Screen::Canvas, at(&[Anchor::FontMenu]), "Fonts", "Pick any font on your computer, or a single-line writing font for pens.")
    },
    Step {
        prepare: &[Act::Select(&[Part::Text])],
        ..step(
            TEXT,
            Screen::Canvas,
            at(&[Anchor::TextFormat]),
            "Format",
            "Format sets size, spacing and alignment, and can split text into letters. Effects > Curve bends a line into an arc.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Text])],
        ..step(TEXT, Screen::Canvas, Target::Part(Part::Text), "Edit in place", "Double-click text to change the words on the canvas.")
    },
    Step {
        prepare: &[Act::Select(&[Part::Heart, Part::Star])],
        ..step(
            ARRANGE,
            Screen::Canvas,
            at(&[Anchor::Align]),
            "Align and distribute",
            "With two or more objects selected, Align lines them up and Distribute spaces them evenly.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Heart, Part::Star])],
        keys: Some(KeyboardShortcut::new(CMD, Key::G)),
        ..step(
            ARRANGE,
            Screen::Canvas,
            at(&[Anchor::MenuEdit]),
            "Group, attach and contain",
            "Group moves objects as one, and Attach also keeps them in place on the mat. A container lays objects out in a row, column or grid.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Text])],
        advance: Advance::When { done: text_outlined, show_me: &[Act::Select(&[Part::Text]), Act::Run(outline)] },
        ..step(
            EFFECTS,
            Screen::Canvas,
            at(&[Anchor::Effects]),
            "Outline the text",
            "Effects has Offset, Sticker and Warp. Choose Offset to draw an outline around the text.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Text]), Act::Ensure(text_outlined, &[Act::Run(outline)])],
        ..step(
            EFFECTS,
            Screen::Canvas,
            at(&[Anchor::LayerEffects]),
            "Live effects",
            "Effects stay editable. Click a chip in Layers to change one, or Expand to turn it into plain paths.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Card, Part::Heart])],
        advance: Advance::When { done: combined, show_me: &[Act::Select(&[Part::Card, Part::Heart]), Act::Run(subtract)] },
        ..step(
            COMBINE,
            Screen::Canvas,
            at(&[Anchor::Combine]),
            "Cut a window",
            "The card and heart are selected. Choose Combine > Subtract to cut the heart out of the card.",
        )
    },
    Step {
        prepare: &[Act::Ensure(combined, &[Act::Select(&[Part::Card, Part::Heart]), Act::Run(subtract)])],
        ..step(
            COMBINE,
            Screen::Canvas,
            at(&[Anchor::Layers]),
            "Still editable",
            "The cut stays live. Open it in Layers to move the heart, or switch Subtract to Weld or Slice.",
        )
    },
    Step {
        prepare: &[Act::Nodes(None), Act::Tool(Tool::Select), Act::Select(&[Part::Star])],
        advance: Advance::When { done: editing_nodes, show_me: &[Act::Nodes(Some(Part::Star))] },
        ..step(NODES, Screen::Canvas, Target::Part(Part::Star), "Edit points", "Double-click the star to edit its points. Any shape works.")
    },
    Step {
        prepare: &[Act::Nodes(Some(Part::Star))],
        ..step(
            NODES,
            Screen::Canvas,
            at(&[Anchor::NodeToolbar]),
            "Point tools",
            "Click a point to make it sharp or smooth. These tools also add and delete points, or smooth the whole path.",
        )
    },
    Step {
        prepare: &[Act::Nodes(None), Act::Select(&[Part::Star])],
        advance: Advance::When { done: has_component, show_me: &[Act::Nodes(None), Act::Select(&[Part::Star]), Act::Run(make_component)] },
        ..step(
            COMPONENTS,
            Screen::Canvas,
            at(&[Anchor::MenuEdit]),
            "Make a component",
            "Choose Edit > Make component to reuse the star. Every copy follows the original.",
        )
    },
    Step {
        prepare: &[Act::Nodes(None)],
        ..step(
            COMPONENTS,
            Screen::Canvas,
            at(&[Anchor::ComponentsTab]),
            "Components",
            "Open this tab and drag a component onto the canvas for another copy. Double-click a copy to edit them all; Parts in the inspector changes just one.",
        )
    },
    Step {
        prepare: &[Act::Select(&[Part::Text])],
        ..step(IMAGES, Screen::Canvas, at(&[Anchor::Colour]), "Patterns", "On Print Then Cut objects, the colour swatch can fill a shape with a pattern image.")
    },
    Step {
        prepare: &[Act::Palette(false)],
        keys: Some(KeyboardShortcut::new(CMD, Key::K)),
        advance: Advance::When { done: palette_open, show_me: &[Act::Palette(true)] },
        ..step(PALETTE, Screen::Canvas, Target::Centre, "Command palette", "Press the shortcut below to find any command by name.")
    },
    Step {
        prepare: &[Act::Palette(true)],
        ..step(PALETTE, Screen::Canvas, at(&[Anchor::Palette]), "Search everything", "Type to find commands, shapes, fonts and projects. Enter runs the highlighted one.")
    },
    Step {
        prepare: &[Act::Palette(false)],
        advance: Advance::When { done: making, show_me: &[Act::Screen(Screen::Make)] },
        ..step(MAKE, Screen::Canvas, at(&[Anchor::Make]), "Make it", "When the design is ready, Make lays it out on mats for your machine.")
    },
    step(
        MAKE,
        Screen::Make,
        at(&[Anchor::PrepareSettings]),
        "Prepare",
        "Each colour and operation gets its own mat, listed under Mats. Set the material here, and Optimise layout packs shapes closer to use less of it.",
    ),
    step(MAKE, Screen::Make, at(&[Anchor::PreviewCut]), "Cut preview", "Watch the tool path in the order the machine follows, with time and material estimates."),
    step(
        MAKE,
        Screen::Make,
        at(&[Anchor::Continue]),
        "Connect and cut",
        "Continue asks for a machine, the built-in simulator or an open cutter. Then you set the material, load the mat and press Go.",
    ),
    step(
        EXPORT,
        Screen::Canvas,
        at(&[Anchor::Export]),
        "Export",
        "Export writes SVG, DXF or PDF files. File > Prepare for Design Space writes one SVG per mat for Cricut's app.",
    ),
    step(
        SAVING,
        Screen::Canvas,
        at(&[Anchor::SaveStatus]),
        "Autosave",
        "Kerf saves your work as you go, and this shows when it last did. The Tutorial tab is never saved.",
    ),
    step(
        SAVING,
        Screen::Canvas,
        at(&[Anchor::MenuFile]),
        "Versions and recovery",
        "File > Version history lets you preview and restore earlier saves. If Kerf quits unexpectedly, it offers your unsaved work at the next launch.",
    ),
    step(SAVING, Screen::Canvas, Target::Centre, "You're all set", "You can rerun this from Help > Interactive tutorial."),
];

/// The sample card's pieces, by id.
#[derive(Clone, Copy)]
struct Parts {
    card: u64,
    heart: u64,
    text: u64,
    star: u64,
}

impl Parts {
    fn id(&self, part: Part) -> u64 {
        match part {
            Part::Card => self.card,
            Part::Heart => self.heart,
            Part::Text => self.text,
            Part::Star => self.star,
        }
    }
}

/// A card with a heart, a line of text and a star drawn as a path, so it has points to edit.
fn sample() -> (Design, Parts) {
    let mut design = Design::default();
    let mut place = |kind: ShapeKind, (x, y, w, h): (f32, f32, f32, f32), fill: egui::Color32| {
        let id = design.add_shape(kind);
        let shape = design.shapes.last_mut().expect("just added");
        (shape.x_in, shape.y_in, shape.width_in, shape.height_in, shape.fill) = (x, y, w, h, fill);
        shape.pen = crate::pens::Pen::nearest(fill, Default::default());
        id
    };
    let card = place(ShapeKind::Rectangle, (1.0, 1.0, 7.0, 5.0), egui::Color32::from_rgb(0xf6, 0xd6, 0xde));
    let heart = place(ShapeKind::Heart, (1.8, 2.0, 2.4, 2.2), egui::Color32::from_rgb(0xd9, 0x3a, 0x55));
    let star = place(ShapeKind::Star, (5.6, 3.9, 1.6, 1.6), egui::Color32::from_rgb(0xf2, 0xb3, 0x1b));
    let text = design.add_text("Hello".into(), FontRef::SpaceGrotesk, 0.9, egui::Color32::from_rgb(0x21, 0x23, 0x29), 4.7, 2.2);
    (design, Parts { card, heart, text, star })
}

/// Where the tour stands, between frames.
pub struct Tour {
    pub step: usize,
    /// The Tutorial tab's id.
    doc: String,
    parts: Parts,
    /// The screen and tab the tour started from, shown again when it ends.
    back_to: (chrome::Tab, String),
    started: f64,
    entered: f64,
    /// Where the spotlight stood on the step before, to move from.
    from: Option<Rect>,
    /// Where it stands now.
    hole: Option<Rect>,
    /// Acts to perform at the start of the next frame.
    pending: Vec<Act>,
    /// The step's prepare has run and its first frame drawn; `seen` and `armed` hold from then on.
    settled: bool,
    seen: Seen,
    /// The step's "do it" was not yet done when it began, so doing it moves on.
    armed: bool,
    /// Show me was pressed on this step.
    tried: bool,
    /// The target went missing and prepare ran again to bring it back.
    healed: bool,
    /// The user closed a panel or mode with Escape on this step, so prepare does not reopen it.
    dismissed: bool,
    /// Escape belongs to the tour this frame: no text field, popup or palette had it as the frame began.
    esc_ok: bool,
}

impl Tour {
    fn current(&self) -> &'static Step {
        &STEPS[self.step]
    }

    /// The spotlight's cut-out this frame, or `None` for a centred card.
    #[cfg(test)]
    pub fn hole(&self) -> Option<Rect> {
        self.hole
    }
}

/// Why the tour ended.
#[derive(Clone, Copy, Debug)]
enum End {
    Finished,
    Skipped,
    Closed,
    /// Its tab was closed under it.
    TabClosed,
}

impl Kerf {
    /// Help > Interactive tutorial: a Tutorial tab with the sample card, and the tour from its first step.
    /// While a tour runs, this shows it again.
    pub(super) fn start_tutorial(&mut self, ctx: &Context) {
        if let Some(i) = self.tutorial.as_ref().and_then(|t| self.tab_of(&t.doc)) {
            self.switch_to(i);
            return;
        }
        let back_to = (self.tab, self.doc.autosave.id.clone());
        let (design, parts) = sample();
        let mut doc = Document::blank("Tutorial".into());
        doc.design = design;
        doc.mark_saved();
        doc.transient = true;
        let id = doc.autosave.id.clone();
        self.push_tab(doc);
        let now = ctx.input(|i| i.time);
        tracing::info!(target: "tutorial", steps = STEPS.len(), "start");
        let mut tour = Tour {
            step: 0,
            doc: id,
            parts,
            back_to,
            started: now,
            entered: now,
            from: None,
            hole: None,
            pending: Vec::new(),
            settled: false,
            seen: Seen::default(),
            armed: false,
            tried: false,
            healed: false,
            dismissed: false,
            esc_ok: false,
        };
        tour.pending = entry_acts(&STEPS[0]);
        self.tutorial = Some(tour);
        ctx.request_repaint();
    }

    /// The id of a piece of the sample, while the tour runs.
    pub(super) fn part(&self, part: Part) -> Option<u64> {
        self.tutorial.as_ref().map(|t| t.parts.id(part))
    }

    /// Runs at the start of every frame: performs what the tour queued last frame, and notes whether
    /// Escape is the tour's this frame.
    pub(super) fn tutorial_acts(&mut self, ctx: &Context) {
        let Some(tour) = &mut self.tutorial else { return };
        let elsewhere = ctx.wants_keyboard_input() || ctx.memory(|m| m.any_popup_open()) || super::context_menu::is_open(ctx);
        let modes = self.palette.is_open() || self.about || self.make_elsewhere.is_some() || self.doc.autosave.offer.is_some();
        let canvas = self.doc.view.nodes.is_some() || self.doc.view.measure.measurement.is_some() || self.doc.view.text_edit.is_some();
        tour.esc_ok = !(elsewhere || modes || canvas);
        // Escape that closes a panel or mode is the user leaving it; the tour does not open it again.
        if !tour.esc_ok && ctx.input(|i| i.key_pressed(Key::Escape)) {
            tour.dismissed = true;
        }
        let acts = std::mem::take(&mut tour.pending);
        for act in acts {
            self.perform(act, ctx);
        }
    }

    /// Runs last in every frame: moves the tour on when its step is done, then draws the spotlight and
    /// card and acts on their buttons.
    pub(super) fn tutorial_frame(&mut self, ctx: &Context) {
        let Some(tour) = &self.tutorial else { return };
        if self.tab_of(&tour.doc).is_none() {
            return self.end_tutorial(End::TabClosed, ctx);
        }
        if tour.esc_ok && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            return self.end_tutorial(End::Closed, ctx);
        }
        let (index, settled, armed) = (tour.step, tour.settled, tour.armed);
        let seen = if settled { tour.seen } else { Seen { shapes: self.doc.design.shapes.len() } };
        let step = &STEPS[index];
        let done = match step.advance {
            Advance::When { done, .. } => Some(self.tutorial_is_active() && done(self, &seen)),
            Advance::Next => None,
        };
        if settled && armed && done == Some(true) {
            self.go_to(index + 1, ctx);
        } else {
            let target = self.target_rect(ctx, step.target);
            let now = ctx.input(|i| i.time);
            let tour = self.tutorial.as_mut().expect("checked above");
            if !settled {
                (tour.settled, tour.seen, tour.armed, tour.entered) = (true, seen, done == Some(false), now);
            }
            match target {
                Some(rect) => (tour.hole, tour.healed) = (Some(overlay::hole_around(rect)), false),
                None if !matches!(step.target, Target::Centre) && !tour.healed && !tour.dismissed => {
                    (tour.hole, tour.healed) = (None, true);
                    tour.pending = entry_acts(step);
                    ctx.request_repaint();
                }
                None => tour.hole = None,
            }
        }
        let moved_to = self.tutorial.as_ref().map_or(index, |t| t.step);
        let (index, done) = if moved_to == index { (index, done) } else { (moved_to, None) };
        match self.tutorial_card(ctx, done) {
            Some(Pressed::Next) if index + 1 == STEPS.len() => self.end_tutorial(End::Finished, ctx),
            Some(Pressed::Next) => self.go_to(index + 1, ctx),
            Some(Pressed::Back) => self.go_to(index.saturating_sub(1), ctx),
            Some(Pressed::ShowMe) => self.show_me(ctx),
            Some(Pressed::Skip) => self.end_tutorial(End::Skipped, ctx),
            Some(Pressed::Close) => self.end_tutorial(End::Closed, ctx),
            None => {}
        }
    }

    /// Draws this frame's spotlight and card, easing in over `FADE_S` after a step change.
    fn tutorial_card(&self, ctx: &Context, done: Option<bool>) -> Option<Pressed> {
        let tour = self.tutorial.as_ref()?;
        let now = ctx.input(|i| i.time);
        let step = tour.current();
        let ease = |since: f64| {
            let t = ((now - since) as f32 / overlay::FADE_S).clamp(0.0, 1.0);
            1.0 - (1.0 - t) * (1.0 - t)
        };
        let fade = ease(tour.entered);
        if fade < 1.0 {
            ctx.request_repaint();
        }
        let hole = match (tour.from, tour.hole) {
            (Some(from), Some(to)) => Some(from.lerp_towards(&to, fade)),
            (_, hole) => hole,
        };
        let waiting = matches!(step.advance, Advance::When { .. }) && tour.armed && done != Some(true) && !tour.tried;
        let last = tour.step + 1 == STEPS.len();
        let card = Card {
            section: step.section,
            title: step.title,
            body: step.body,
            keys: step.keys.map(|k| ctx.format_shortcut(&k)),
            step: tour.step + 1,
            of: STEPS.len(),
            back: tour.step > 0,
            show_me: matches!(step.advance, Advance::When { .. }) && done != Some(true),
            next: (!waiting).then_some(match tour.step {
                0 => "Start",
                _ if last => "Finish",
                _ => "Next",
            }),
            skip: !last,
        };
        overlay::show(ctx, Spot { hole, dim: ease(tour.started), fade }, &card)
    }

    /// Where the step's target is this frame, if it is on screen.
    fn target_rect(&self, ctx: &Context, target: Target) -> Option<Rect> {
        match target {
            Target::Centre => None,
            Target::Widgets(anchors) => anchors.iter().filter_map(|&a| anchors::rect(ctx, a)).reduce(|a, b| a.union(b)),
            Target::Part(part) => {
                let id = self.part(part).filter(|_| self.tutorial_is_active())?;
                let s = self.doc.design.shapes.iter().find(|s| s.id == id)?;
                anchors::on_board(ctx, (s.x_in, s.y_in), (s.x_in + s.width_in, s.y_in + s.height_in))
            }
        }
    }

    fn go_to(&mut self, step: usize, ctx: &Context) {
        let Some(tour) = &mut self.tutorial else { return };
        tracing::info!(target: "tutorial", from = tour.step + 1, to = step + 1, title = STEPS[step].title, "step");
        tour.from = tour.hole;
        tour.entered = ctx.input(|i| i.time);
        tour.step = step;
        tour.settled = false;
        tour.tried = false;
        tour.healed = false;
        tour.dismissed = false;
        tour.pending = entry_acts(&STEPS[step]);
        ctx.request_repaint();
    }

    fn show_me(&mut self, ctx: &Context) {
        let Some(tour) = &mut self.tutorial else { return };
        let Advance::When { show_me, .. } = tour.current().advance else { return };
        tracing::info!(target: "tutorial", step = tour.step + 1, "show me");
        tour.tried = true;
        tour.pending = std::iter::once(Act::Screen(tour.current().on)).chain(show_me.iter().copied()).collect();
        ctx.request_repaint();
    }

    /// Closes the Tutorial tab without saving it and returns to the screen and tab the tour began on.
    fn end_tutorial(&mut self, why: End, ctx: &Context) {
        let Some(tour) = self.tutorial.take() else { return };
        tracing::info!(target: "tutorial", ?why, step = tour.step + 1, of = STEPS.len(), "end");
        if self.palette.is_open() {
            self.palette.toggle(Vec::new);
        }
        if let Some(i) = self.tab_of(&tour.doc) {
            self.drop_tab(i);
        }
        let (screen, id) = tour.back_to;
        if let Some(i) = self.tab_of(&id) {
            self.switch_to(i);
        }
        self.tab = screen;
        if screen == chrome::Tab::Home {
            self.run(Command::App(AppCommand::Home), ctx);
        }
        ctx.request_repaint();
    }

    fn perform(&mut self, act: Act, ctx: &Context) {
        match act {
            Act::Shapes(open) => self.doc.shapes_open = open,
            Act::Select(parts) => {
                let ids: Vec<u64> = parts.iter().filter_map(|&p| self.part(p)).filter(|id| self.doc.design.shapes.iter().any(|s| s.id == *id)).collect();
                self.doc.selection = ids;
            }
            Act::Tool(tool) => self.doc.tool = tool,
            Act::Nodes(None) => self.doc.view.nodes = None,
            Act::Nodes(Some(part)) => {
                let editable = |id: &u64| self.doc.design.shapes.iter().any(|s| s.id == *id && crate::path_edit::editable(s));
                if let Some(id) = self.part(part).filter(editable) {
                    self.doc.selection = vec![id];
                    self.doc.view.nodes = Some(node_edit::NodeEdit::begin(id));
                }
            }
            Act::Palette(open) if open != self.palette.is_open() => self.run(Command::App(AppCommand::Palette), ctx),
            Act::Palette(_) => {}
            Act::Run(command) => self.run_observed(command(), ctx),
            Act::Ensure(done, acts) => {
                let seen = self.tutorial.as_ref().map(|t| t.seen).unwrap_or_default();
                if !done(self, &seen) {
                    for &act in acts {
                        self.perform(act, ctx);
                    }
                }
            }
            Act::Screen(screen) => self.show_screen(screen, ctx),
        }
    }

    /// Runs `command` as a menu pick would, with an edit recorded as one undo step of its own.
    fn run_observed(&mut self, command: Command, ctx: &Context) {
        let Command::Edit(_) = command else { return self.run(command, ctx) };
        let before = self.doc.design.clone();
        self.run(command, ctx);
        self.doc.design.settle(Some(&before));
        self.doc.history.observe(before, &self.doc.design, false);
    }

    /// Brings up the screen a step is on: Home, or the Tutorial tab's canvas or Make flow.
    fn show_screen(&mut self, screen: Screen, ctx: &Context) {
        if screen != Screen::Canvas && self.palette.is_open() {
            self.run(Command::App(AppCommand::Palette), ctx);
        }
        if screen != Screen::Make && self.tutorial_is_active() && self.doc.flow.is_some() {
            self.close_flow();
        }
        match screen {
            Screen::Home if self.tab != chrome::Tab::Home => self.run(Command::App(AppCommand::Home), ctx),
            Screen::Home => {}
            Screen::Canvas | Screen::Make => {
                let i = self.tutorial.as_ref().and_then(|t| self.tab_of(&t.doc));
                if let Some(i) = i.filter(|_| self.tab == chrome::Tab::Home || !self.tutorial_is_active()) {
                    self.switch_to(i);
                }
                // One machine means one Make: while another tab's runs, the Make steps only describe it.
                if screen == Screen::Make && !self.docs().any(|d| d.flow.is_some()) {
                    self.run(Command::App(AppCommand::Make), ctx);
                }
            }
        }
    }

    fn tutorial_is_active(&self) -> bool {
        self.tutorial.as_ref().is_some_and(|t| t.doc == self.doc.autosave.id)
    }
}

/// What entering `step` does: show its screen, then its prepare.
fn entry_acts(step: &Step) -> Vec<Act> {
    std::iter::once(Act::Screen(step.on)).chain(step.prepare.iter().copied()).collect()
}

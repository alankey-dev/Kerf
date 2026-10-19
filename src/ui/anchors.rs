//! Where named widgets were drawn this frame. A widget records its rect with [`mark`]; the tutorial
//! looks its target up with [`rect`] once every panel has drawn. A mark from an earlier frame does not
//! count, so a closed panel has no rect.

use std::collections::HashMap;

use egui::{Context, Id, Pos2, Rect, Ui, Vec2};

/// A widget the tutorial can point at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    // Home and the tab strip.
    HomeProjects,
    HomeNew,
    TabStrip,
    // The top bar.
    MenuFile,
    MenuEdit,
    MenuHelp,
    SaveStatus,
    Export,
    Make,
    // The rail and the Shapes panel.
    Rail,
    RailShapes,
    RailUpload,
    ShapesPanel,
    ShapesSearch,
    // The canvas.
    Rulers,
    ZoomControls,
    Snapping,
    OpLegend,
    SelectionFrame,
    ActionBar,
    // The edit toolbar.
    FontMenu,
    TextFormat,
    Align,
    Effects,
    Combine,
    NodeToolbar,
    // The inspector.
    Properties,
    Operation,
    Colour,
    Layers,
    LayerEffects,
    ComponentsTab,
    // The Make flow.
    PrepareSettings,
    PreviewCut,
    Continue,
    // Overlays.
    Palette,
}

#[derive(Clone, Default)]
struct Marks {
    pass: u64,
    rects: HashMap<Anchor, Rect>,
    board: Option<Board>,
}

/// Where the canvas drew the design this frame: inches map to `origin + inches * scale`, inside `clip`.
#[derive(Clone, Copy)]
struct Board {
    origin: Pos2,
    scale: f32,
    clip: Rect,
}

fn id() -> Id {
    Id::new("anchors")
}

/// Records that `anchor` is drawn at `rect` this frame, cut to what `ui` shows of it. Marking one anchor
/// several times marks the area around them all.
pub fn mark(ui: &Ui, anchor: Anchor, rect: Rect) {
    let shown = rect.intersect(ui.clip_rect());
    if shown.is_positive() {
        record(ui.ctx(), anchor, shown);
    }
}

fn record(ctx: &Context, anchor: Anchor, rect: Rect) {
    this_frame(ctx, |marks| _ = marks.rects.entry(anchor).and_modify(|r| *r = r.union(rect)).or_insert(rect));
}

fn this_frame(ctx: &Context, change: impl FnOnce(&mut Marks)) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(id());
        if marks.pass != pass {
            *marks = Marks { pass, ..Marks::default() };
        }
        change(marks);
    });
}

fn read<R>(ctx: &Context, get: impl FnOnce(&Marks) -> Option<R>) -> Option<R> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Marks>(id()).filter(|m| m.pass == pass).and_then(|m| get(&m)))
}

/// Where `anchor` was drawn this frame, if it was.
pub fn rect(ctx: &Context, anchor: Anchor) -> Option<Rect> {
    read(ctx, |m| m.rects.get(&anchor).copied())
}

/// Records how the canvas maps inches to the screen this frame: `origin + inches * scale`.
pub fn board(ui: &Ui, origin: Pos2, scale: f32) {
    let clip = ui.clip_rect();
    this_frame(ui.ctx(), |marks| marks.board = Some(Board { origin, scale, clip }));
}

/// Where the canvas drew the area from `min` to `max` (in inches) this frame, cut to what it shows.
pub fn on_board(ctx: &Context, min: (f32, f32), max: (f32, f32)) -> Option<Rect> {
    let b = read(ctx, |m| m.board)?;
    let at = |(x, y): (f32, f32)| b.origin + Vec2::new(x, y) * b.scale;
    Some(Rect::from_two_pos(at(min), at(max)).intersect(b.clip)).filter(|r| r.is_positive())
}

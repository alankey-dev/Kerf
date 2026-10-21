//! The one command model (spec 23). Every design edit a menu, button, key or popover can make is an
//! [`Edit`]. [`Design::check`] holds each edit's enable rule, so every surface agrees on what is
//! enabled and why not, and [`Design::apply`] runs an edit only when its check passes. [`run`] is the one
//! dispatch function the UI calls. Commands that need the app rather than the design are [`AppCommand`].
//!
//! An edit acts on a list of ids, its subject, and returns the ids the user holds afterwards: the copies
//! after Duplicate, the new shape after Combine, nothing after Delete. The model never touches a
//! selection; `run` turns the held ids into the next selection.

use egui::Color32;

use crate::containers::AlignTo;
use crate::design::{Align, Arrange, Combine, CompoundOp, Contour, Design, History, Operation, Pt, Seg, Shape, ShapeKind, UnitKey};
use crate::effects::{Effect, EffectEdit, EntryRef, Reach, Row};
use crate::expr::Typed;
use crate::links::LinkKind;
use crate::pens::PenPick;
use crate::select::Match;
use crate::text::FontRef;
use crate::text_ops::TextChange;
use crate::transform::{self, Axis, Field, clamp_pos};
use crate::warp::Warp;

/// How far Duplicate offsets the copies, down and right.
const DUPLICATE_OFFSET_IN: f32 = 0.25;
/// Skew is clamped to this many degrees either way, so the shear never folds the shape flat.
const MAX_SKEW_DEG: f32 = crate::transform::MAX_SKEW_DEG;
/// How near a Scissors click must land to a path's edge for it to cut, in inches.
const SCISSORS_REACH_IN: f32 = 0.15;
/// The size of text `Insert` places, as the Text tool places it.
const INSERTED_TEXT_IN: f32 = 1.0;

/// What `Edit::Insert` adds to the canvas.
#[derive(Clone, Debug, PartialEq)]
pub enum Insert {
    Shape(ShapeKind),
    /// A line of text in this font.
    Text(FontRef),
    /// A monogram group (spec 42), centred at `at` or in the next free slot.
    Monogram {
        spec: Box<crate::monogram::MonogramSpec>,
        at: Option<crate::design::Pt>,
    },
    /// A Create AI design (spec 43): `outline` is the placed, sized shape, named `title`.
    Generated {
        title: String,
        outline: Outline,
    },
}

/// A generated shape; its `Debug` is a summary for the `cmd` log line, not the geometry.
#[derive(Clone, PartialEq)]
pub struct Outline(pub Box<Shape>);

impl std::fmt::Debug for Outline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let contours = match &self.0.kind {
            ShapeKind::Imported { contours, .. } | ShapeKind::Path { contours, .. } => contours.len(),
            _ => 0,
        };
        write!(f, "{contours} contours")
    }
}

/// The three per-shape locks (spec 34).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    /// `Shape::lock_aspect`: W and H keep their proportions.
    Aspect,
    /// `Shape::lock_position`: no move, resize, rotate or flip.
    Position,
    /// `Shape::locked`: the canvas cannot select it.
    Layer,
}

impl Lock {
    fn flag(self, s: &mut Shape) -> &mut bool {
        match self {
            Self::Aspect => &mut s.lock_aspect,
            Self::Position => &mut s.lock_position,
            Self::Layer => &mut s.locked,
        }
    }

    pub fn of(self, s: &Shape) -> bool {
        match self {
            Self::Aspect => s.lock_aspect,
            Self::Position => s.lock_position,
            Self::Layer => s.locked,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Aspect => "aspect",
            Self::Position => "position",
            Self::Layer => "layer",
        }
    }
}

/// Every change a user makes to a design through a command surface.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Delete,
    Duplicate,
    SelectAll,
    /// Selects every visible, unlocked shape that shares the selection's value for `Match` (spec 50).
    SelectSame(Match),
    /// Selects every selectable shape that is not in the selection.
    SelectInverse,
    /// Moves the selection to the next shape in Layers order, wrapping (Tab).
    SelectNext,
    /// Moves the selection to the previous shape in Layers order, wrapping (Shift+Tab).
    SelectPrevious,
    Deselect,
    Group,
    Ungroup,
    Attach,
    Detach,
    /// One object becomes a container; two or more are wrapped in a new one.
    Contain,
    Uncontain,
    Arrange(Arrange),
    /// Mirrors the subject across its frame.
    Flip(Axis),
    /// `to` is the resolved target: `Design::align_target` of the user's Align To choice.
    Align {
        op: Align,
        to: AlignTo,
    },
    /// Wraps the subject in a live boolean group.
    Combine(Combine),
    /// Sets the operation of every boolean group in the subject.
    BooleanOp(Combine),
    Flatten,
    Unflatten,
    /// Adds an Offset or Sticker live to each subject shape, or, `Together` over several objects, wraps
    /// them in an effect group that carries one welded around them.
    AddEffect {
        effect: Effect,
        reach: Reach,
    },
    /// Edits one row of one shape's effect list: its eye, its removal, its place in the stack, or its
    /// values. The subject is that shape.
    Effect {
        at: EntryRef,
        op: EffectEdit,
    },
    /// Bakes every effect of the subject into plain shapes, and each Compound one level.
    Expand,
    /// Sets the warp on every subject shape that can take one, or removes it with `None`.
    Warp(Option<Warp>),
    /// Sets the signed curve diameter of every subject text; 0 straightens it.
    Curve(f32),
    /// Puts text on a path, straightens it, or slides and flips it along one (spec 51).
    TextPath(TextPathEdit),
    Text(TextChange),
    /// Changes one parameter of every subject parametric shape (spec 49): a count, radius, angle,
    /// fraction or toggle. Each commit is one undo step.
    Params(crate::parametric::ParamOp),
    /// Rounds, chamfers or scoops corners of the subject paths (spec 55). The subject is turned into a
    /// path first, carrying a parametric shape's radii over as live corners. Each commit is one undo step.
    Corners(crate::corners::CornerOp),
    /// Turns every subject text into a path of its outline, open strokes staying open.
    ConvertToPaths,
    SetHidden(bool),
    /// Moves the subject, clamped to one `board_in` of void around the mat.
    Nudge {
        dx_in: f32,
        dy_in: f32,
        board_in: (f32, f32),
    },
    /// A typed position, size or rotation, applied to each shape's own value (`*2` doubles each
    /// width); X and Y clamp as `Nudge` does, and W or H keeps the proportions of a shape whose aspect
    /// is locked.
    Field {
        field: Field,
        value: Typed,
        board_in: (f32, f32),
    },
    /// Applies the last transform again (spec 52): `from` is the pose the last duplicate or gesture
    /// started at, from the document's `DuplicateMemory`, and the subject steps one more such transform
    /// beyond where it sits now. With `copy`, it duplicates the subject first (Power Duplicate); without,
    /// it moves the subject in place (Transform Again). Each press is one undo step.
    TransformAgain {
        from: transform::Frame,
        copy: bool,
    },
    /// Step and repeat (spec 52): copies of the subject offset in a row or grid, optionally turned and
    /// scaled each step, grouped with the source in one undo step.
    StepRepeat(crate::repeat::StepRepeat),
    /// Rotate copies (spec 52): a ring of copies of the subject about a pivot, as a new Group.
    RotateCopies(crate::repeat::RotateCopies),
    /// Sets or clears the subject's pivot, the point rotation turns about, in its local frame (spec 52).
    Pivot(Option<crate::design::Pt>),
    /// Sets the subject's skew in degrees along its own X or Y axis, -89 to 89 (spec 52).
    Skew {
        axis: Axis,
        value: Typed,
    },
    Operation(Operation),
    /// Expands each imported stroke (by id and width in inches) to a filled ring, for artwork that
    /// relies on thick outlines (spec 53). Driven by the import notice's Outline strokes option.
    OutlineStrokes(Vec<(u64, f32)>),
    Colour(Color32),
    Pen(PenPick),
    /// Adds a shape or text in the next free slot on the mat; it needs no subject.
    Insert(Insert),
    /// Stores or clears the optimised mat layout (spec 30); it needs no subject.
    Nesting(Option<crate::nesting::NestedLayout>),
    /// Lets nesting turn the subject only 0 or 180 degrees, or frees it again.
    GrainLock(bool),
    /// Turns one lock on or off for the subject, each whole object: a link with all its members, a
    /// container with everything in it.
    Lock {
        lock: Lock,
        on: bool,
    },
    /// New shapes from outside the design: an uploaded image, a trace. Ids are reassigned on apply.
    Place(Placement),
    /// Adds a validated clipboard package, assigning fresh ids and preserving its closed hierarchy.
    Paste {
        package: Box<crate::clipboard::ValidatedPackage>,
        placement: crate::clipboard::Placement,
        drop_at: crate::design::Pt,
    },
    /// Applies a clipboard style to the subject without changing its geometry.
    PasteStyle(crate::clipboard::PasteStyle),
    /// Fills every subject shape with an image or repeating pattern, or with `None` prints its colour again.
    Pattern(Option<crate::patterns::PatternFill>),
    /// The subject moves into a new component's master and one instance of it takes its place (spec 31).
    MakeComponent,
    /// Each subject instance becomes plain shapes.
    DetachInstance,
    /// Places an instance of a component, centred at `at` or in the next free slot; it needs no subject.
    PlaceInstance {
        component: u64,
        at: Option<crate::design::Pt>,
    },
    RenameComponent {
        component: u64,
        name: String,
    },
    DuplicateComponent(u64),
    /// Detaches every instance of the component, then deletes it.
    DeleteComponent(u64),
    /// Changes one part (a master shape id) of the subject, one instance.
    Override {
        part: u64,
        change: crate::components::PartChange,
    },
    ResetOverrides,
    /// Clears the overrides of one part of each subject instance.
    ResetPart(u64),
    /// Names the subject, one layer or one whole Group or Attach (spec 40). A blank name or `None` gives
    /// it back its label.
    Rename(Option<String>),
    /// Lays monogram `id` out again from `spec` in place (spec 42); it needs no subject.
    Monogram {
        id: u64,
        spec: Box<crate::monogram::MonogramSpec>,
    },
    /// Sets the canvas grid's pitch, subdivisions, origin and visibility (spec 46); it needs no subject.
    Grid(crate::grid::GridSettings),
    /// Adds, moves, locks or removes ruler guides (spec 45); it needs no subject.
    Guide(crate::guides::GuideEdit),
    /// Sets what the magnet snaps to and the Shift angle step (spec 47); it needs no subject.
    Snap(crate::snap::SnapOptions),
    /// Shape Builder (spec 57): rebuilds the planar map of the subject's evaluated outlines and either
    /// unites the regions the gesture crossed into one shape or deletes them, splitting the rest region
    /// by region. `picks` are interior points of the crossed regions, in world inches. Destructive.
    BuildShape {
        picks: Vec<crate::design::Pt>,
        delete: bool,
    },
    /// Knife (spec 57): cuts the subject along `line` (world inches). A closed shape splits into area
    /// pieces, an open path into separate open paths; effects are expanded first. Destructive.
    Knife {
        line: Vec<crate::design::Pt>,
    },
}

/// What [`Edit::TextPath`] does to a text's path (spec 51).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextPathEdit {
    /// Puts the one selected text on the one selected path shape.
    Attach,
    /// Straightens the subject texts, keeping their size and font.
    Remove,
    /// Flips the subject path texts to the other side of their path.
    Side(crate::text_path::Side),
    /// Sets where the run sits along the path.
    Along(crate::text_path::Along),
    /// Slides the run's start along the path, in inches.
    Offset(f32),
}

/// The shapes an [`Edit::Place`] adds; with `group`, two or more land in one Group.
#[derive(Clone, PartialEq)]
pub struct Placement {
    pub shapes: Vec<Shape>,
    pub group: bool,
}

/// A summary for the `cmd` log line, not the geometry.
impl std::fmt::Debug for Placement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.shapes.len();
        write!(f, "{n} shape{}{}", if n == 1 { "" } else { "s" }, if self.group { ", grouped" } else { "" })
    }
}

/// Commands that act on the app rather than the design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppCommand {
    New,
    Home,
    Save,
    Export,
    PrepareForDesignSpace,
    Undo,
    Redo,
    Make,
    Preview,
    /// Opens the saved project with this id.
    Open(String),
    /// Opens or closes the command palette.
    Palette,
    ZoomIn,
    ZoomOut,
    /// Actual size, 100 %.
    ResetZoom,
    /// Fits the artboard with a margin (spec 48).
    ZoomToFit,
    /// Fits the selection's frame, no closer than 800 %.
    ZoomToSelection,
    /// Fits every shape, or the artboard when there are none.
    ZoomToAll,
    /// Zooms to this many per cent about the view centre.
    ZoomTo(u32),
    Units(crate::units::Units),
    Snapping,
    /// Turns one snap target class on or off from the Snapping popover or the palette (spec 47).
    SnapClass(crate::snap::SnapClass),
    /// Opens the Snapping popover beside the magnet (spec 47).
    SnapSettings,
    PartialGrid,
    /// Opens the Grid popover at the ruler corner (spec 46).
    GridSettings,
    CutPreview,
    /// Opens the open project's version history.
    VersionHistory,
    /// Pick an SVG, PNG, JPG or WebP file and add it to the canvas.
    Upload,
    /// Opens this component's master on its own on the canvas (spec 31).
    EditMaster(u64),
    /// Closes the open master and returns to the canvas.
    LeaveMaster,
    /// Closes this tab, or the active one when `None`. Its autosave is kept.
    CloseTab(Option<usize>),
    NextTab,
    PreviousTab,
    /// Shows the tab at this index, counting from 0.
    Tab(usize),
    /// Starts the interactive tutorial in a tab of its own (spec 39).
    Tutorial,
    /// Opens the command palette listing only commands with a shortcut.
    Shortcuts,
    About,
    /// Opens the Create AI settings window: provider, API key and model (spec 43).
    AiSettings,
    /// Opens the selected layer's name for typing over in Layers (spec 40).
    Rename,
    /// Opens the selected monogram in the Monogram panel (spec 42).
    EditMonogram,
    /// Opens Replace colour: swap one of the design's colours for another everywhere (spec 56).
    ReplaceColour,
    /// Opens the Step and repeat popover over the selection (spec 52).
    StepRepeatPanel,
    /// Opens the Rotate copies popover over the selection (spec 52).
    RotateCopiesPanel,
    /// Copies the user's saved swatches into this project's own palette (spec 56).
    SavePaletteToProject,
    /// Copies this project's palette into the user's saved swatches (spec 56).
    CopyPaletteToSettings,
}

/// Why a command cannot run; `reason` fills the disabled control's tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    NothingSelected,
    NoShapes,
    TooFewToLink,
    HasContainer,
    NoLink(LinkKind),
    CannotContain,
    NoContainer,
    SliceNeedsTwo,
    TooFewToCombine,
    NotBooleanGroup,
    TooFewToFlatten,
    NotFlattened,
    TooFewToAlign {
        need: usize,
    },
    TooFewToDistribute {
        need: usize,
    },
    CannotWarp,
    ImageCannotWarp,
    GroupCannotWarp,
    ImageSticker,
    NotText,
    /// The subject has no shape whose parameter this edit changes.
    NotParametric,
    /// Text on path needs one text, and either the shape for it to follow selected too or picked next.
    NeedTextAndPath,
    /// A path edit needs a text that is already on a path.
    NotOnPath,
    /// The subject has no editable outline to round corners on.
    NotPath,
    SingleLineText,
    SplitEffects,
    SplitContainer,
    LiveEffects,
    StickerOnContainer,
    NoEntry,
    PinnedRow,
    WrongEffect,
    NoEffects,
    WarpAfterSticker,
    AlreadyHidden,
    AlreadyVisible,
    NothingToUndo,
    NothingToRedo,
    GrainLocked,
    GrainUnlocked,
    NothingToPlace,
    /// Every subject shape is position-locked, so a move, resize, rotate, flip or align has nothing to move.
    PositionLocked,
    AlreadyLocked(Lock),
    NotLocked(Lock),
    /// Select All has nothing it may pick: every layer is locked.
    AllLocked,
    /// A Select Same alike row whose value differs across the selection (spec 50).
    Mixed(Match),
    /// Select All text or Select All paths with no shape of that kind to pick.
    NoneToSelect(Match),
    /// A pattern fills only Print Then Cut shapes that carry no artwork of their own.
    PatternNeedsPrint,
    NoPattern,
    NoInstance,
    NoComponent,
    ComponentCycle,
    NoPart,
    NoOverrides,
    EmptyName,
    /// A rename names one layer, or one whole Group or Attach.
    NotOneLayer,
    NotMonogram,
    /// The monogram's frame or letters were changed directly, so its settings no longer describe it.
    MonogramEdited,
    NoLetters,
    /// That ruler guide has been removed.
    NoGuide,
    NoGuides,
    /// The guide, or every guide, is locked.
    GuideLocked,
    InvalidClipboard,
    /// Zoom to selection with nothing selected.
    NothingToZoomTo,
}

impl Blocked {
    pub fn reason(self) -> String {
        match self {
            Self::NothingSelected => "Select one or more objects first.".into(),
            Self::NoShapes => "Add an object to the canvas first.".into(),
            Self::TooFewToLink => "Select two or more objects.".into(),
            Self::HasContainer => "A container cannot be grouped or attached.".into(),
            Self::NoLink(LinkKind::Group) => "Select a group to ungroup.".into(),
            Self::NoLink(LinkKind::Attach) => "Select an attached set to detach.".into(),
            Self::CannotContain => "The selection is already a container.".into(),
            Self::NoContainer => "Select a container to remove.".into(),
            Self::SliceNeedsTwo => "Select exactly two objects to slice.".into(),
            Self::TooFewToCombine => "Select two or more objects.".into(),
            Self::NotBooleanGroup => "Select a combined group to change its operation.".into(),
            Self::TooFewToFlatten => "Select two or more objects, or a group, to flatten.".into(),
            Self::NotFlattened => "Select one flattened image to unflatten.".into(),
            Self::TooFewToAlign { need } => format!("Select {need} or more objects to align."),
            Self::TooFewToDistribute { need } => format!("Select {need} or more objects to distribute."),
            Self::CannotWarp => "Unflatten this shape to warp its parts.".into(),
            Self::ImageCannotWarp => "Images can't be warped.".into(),
            Self::GroupCannotWarp => "Expand the group to warp what it makes.".into(),
            Self::ImageSticker => "Images can't be made into stickers.".into(),
            Self::NotText => "Select text first.".into(),
            Self::NotParametric => "Select a shape with parameters first.".into(),
            Self::NeedTextAndPath => "Select a text to put on a path, then click the shape for it to follow. Or select both at once.".into(),
            Self::NotOnPath => "Select text that is on a path.".into(),
            Self::NotPath => "Select a path or shape to round its corners.".into(),
            Self::SingleLineText => "Single-line text has no area to combine.".into(),
            Self::SplitEffects => "Expand or remove effects to split.".into(),
            Self::SplitContainer => "Move its contents out to split.".into(),
            Self::LiveEffects => "Expand effects first: an Offset or Sticker cannot be flattened while it is live.".into(),
            Self::StickerOnContainer => "A container cannot be made into a sticker. Select what it holds instead.".into(),
            Self::NoEntry => "That effect has been removed.".into(),
            Self::PinnedRow => "Curve and Contour always come before the other effects.".into(),
            Self::WrongEffect => "That entry is a different effect.".into(),
            Self::NoEffects => "Select an object with effects to expand.".into(),
            Self::WarpAfterSticker => "A Warp cannot bend a sticker's print; keep it above the Sticker.".into(),
            Self::AlreadyHidden => "The selection is already hidden.".into(),
            Self::AlreadyVisible => "The selection is already showing.".into(),
            Self::NothingToUndo => "Nothing to undo.".into(),
            Self::NothingToRedo => "Nothing to redo.".into(),
            Self::GrainLocked => "The selection's grain is already locked.".into(),
            Self::GrainUnlocked => "The selection's grain is not locked.".into(),
            Self::NothingToPlace => "There is nothing to add.".into(),
            Self::PositionLocked => "The selection's position is locked. Unlock it to move, resize, rotate or flip it.".into(),
            Self::AlreadyLocked(lock) => format!("The selection's {} is already locked.", lock.name()),
            Self::NotLocked(lock) => format!("The selection's {} is not locked.", lock.name()),
            Self::AllLocked => "Every layer is locked. Select locked layers from Layers.".into(),
            Self::Mixed(m) => m.mixed_reason().into(),
            Self::NoneToSelect(m) => m.mixed_reason().into(),
            Self::PatternNeedsPrint => {
                "Patterns fill Print Then Cut shapes. Set the operation to Print Then Cut first; images and flattened art keep their own picture.".into()
            }
            Self::NoPattern => "The selection has no pattern.".into(),
            Self::NoInstance => "Select an instance of a component first.".into(),
            Self::NoComponent => "That component no longer exists.".into(),
            Self::ComponentCycle => "A component can't contain itself.".into(),
            Self::NoPart => "That part is no longer in the component.".into(),
            Self::NoOverrides => "The instance has no overrides to reset.".into(),
            Self::EmptyName => "Give the component a name.".into(),
            Self::NotOneLayer => "Select one layer, or one whole group, to rename it.".into(),
            Self::NotMonogram => "Select a monogram to edit it.".into(),
            Self::MonogramEdited => "This group was edited by hand.".into(),
            Self::NoLetters => "Type at least one letter.".into(),
            Self::NoGuide => "That guide has been removed.".into(),
            Self::NoGuides => "There are no guides to clear.".into(),
            Self::GuideLocked => "The guide is locked. Unlock it to move or delete it.".into(),
            Self::InvalidClipboard => "This clipboard item can't be pasted.".into(),
            Self::NothingToZoomTo => "Select something to zoom to.".into(),
        }
    }
}

/// What an edit acts on: the selection, or other ids such as a Layers row's, leaving the selection be.
#[derive(Clone, Copy, Debug)]
pub enum Subject<'a> {
    Selection,
    Ids(&'a [u64]),
}

/// The one dispatch function. On the selection, the held ids become the selection. On other ids the
/// selection only loses what no longer exists.
pub fn run(design: &mut Design, selection: &mut Vec<u64>, edit: &Edit, subject: Subject) -> Result<(), Blocked> {
    let on = match subject {
        Subject::Selection => selection.clone(),
        Subject::Ids(ids) => ids.to_vec(),
    };
    let result = design.apply(edit, &on);
    tracing::info!(target: "cmd", action = ?edit, on = %crate::debug_log::ids(&on), sel = %crate::debug_log::ids(selection), blocked = ?result.as_ref().err(), "edit");
    match subject {
        Subject::Selection => *selection = result?,
        Subject::Ids(_) => {
            result?;
            selection.retain(|id| design.shapes.iter().any(|s| s.id == *id));
        }
    }
    Ok(())
}

fn ok_if(cond: bool, blocked: Blocked) -> Result<(), Blocked> {
    if cond { Ok(()) } else { Err(blocked) }
}

/// Text in a writing font: open strokes with no area to combine, offset or back.
fn single_line(kind: &ShapeKind) -> bool {
    matches!(kind, ShapeKind::Text { font: crate::text::FontRef::Stroke(_), .. })
}

/// The scale from a shape's local view-box units to its drawn box, or 1 when the box is degenerate.
fn safe_scale(box_in: f32, view_box: f32) -> f32 {
    if view_box > 1e-6 { box_in / view_box } else { 1.0 }
}

impl Design {
    /// Whether `edit` would do anything to `on`, and if not, why.
    pub fn check(&self, edit: &Edit, on: &[u64]) -> Result<(), Blocked> {
        let chosen = || self.shapes.iter().filter(|s| on.contains(&s.id));
        let warp_blocked = || chosen().next().map_or(Blocked::CannotWarp, Shape::warp_blocked);
        match edit {
            Edit::SelectAll if self.shapes.is_empty() => return Err(Blocked::NoShapes),
            Edit::SelectAll => return ok_if(self.shapes.iter().any(|s| !s.locked), Blocked::AllLocked),
            Edit::SelectSame(m) => return self.check_select_same(*m, on),
            Edit::SelectInverse | Edit::SelectNext | Edit::SelectPrevious => return self.check_selectable(),
            Edit::Insert(Insert::Monogram { spec, .. }) => return ok_if(!spec.initials().is_empty(), Blocked::NoLetters),
            Edit::Monogram { id, spec } => return self.check_monogram(*id, spec),
            Edit::Guide(g) => return self.check_guide(g),
            Edit::Insert(Insert::Generated { outline, .. }) => return ok_if(!crate::effects::evaluate(&outline.0).bounds().is_empty(), Blocked::NothingToPlace),
            Edit::Insert(_) | Edit::Nesting(_) | Edit::Grid(_) | Edit::Snap(_) => return Ok(()),
            Edit::Place(p) => return ok_if(!p.shapes.is_empty(), Blocked::NothingToPlace),
            Edit::Paste { .. } => return Ok(()),
            Edit::PlaceInstance { .. } | Edit::RenameComponent { .. } | Edit::DuplicateComponent(_) | Edit::DeleteComponent(_) => return self.check_component(edit),
            _ if chosen().next().is_none() => return Err(Blocked::NothingSelected),
            Edit::Combine(_) | Edit::BuildShape { .. } | Edit::AddEffect { effect: Effect::Offset(_) | Effect::Sticker(_), .. }
                if chosen().any(|s| single_line(&s.kind)) =>
            {
                return Err(Blocked::SingleLineText);
            }
            _ => {}
        }
        match edit {
            Edit::SelectAll
            | Edit::SelectSame(_)
            | Edit::SelectInverse
            | Edit::SelectNext
            | Edit::SelectPrevious
            | Edit::Insert(_)
            | Edit::Nesting(_)
            | Edit::Grid(_)
            | Edit::Place(_)
            | Edit::Paste { .. }
            | Edit::PlaceInstance { .. }
            | Edit::RenameComponent { .. }
            | Edit::DuplicateComponent(_)
            | Edit::DeleteComponent(_)
            | Edit::Monogram { .. }
            | Edit::Guide(_)
            | Edit::Snap(_) => unreachable!("checked above"),
            Edit::MakeComponent | Edit::DetachInstance | Edit::Override { .. } | Edit::ResetOverrides | Edit::ResetPart(_) => self.check_instances(edit, on),
            Edit::Group | Edit::Attach => {
                ok_if(!self.has_container(on), Blocked::HasContainer)?;
                ok_if(self.units_by(on, UnitKey::AnyLink).len() >= 2, Blocked::TooFewToLink)
            }
            Edit::Ungroup => {
                let groups = !self.members_of_kind(on, LinkKind::Group).is_empty() || !self.monograms_in(on).is_empty();
                ok_if(groups, Blocked::NoLink(LinkKind::Group))
            }
            Edit::Detach => ok_if(!self.members_of_kind(on, LinkKind::Attach).is_empty(), Blocked::NoLink(LinkKind::Attach)),
            Edit::Contain => ok_if(self.can_contain(on), Blocked::CannotContain),
            Edit::Uncontain => ok_if(self.has_container(on), Blocked::NoContainer),
            Edit::Combine(Combine::Slice) => ok_if(self.units_by(on, UnitKey::Attach).len() == 2, Blocked::SliceNeedsTwo),
            Edit::Combine(_) => ok_if(self.units_by(on, UnitKey::Attach).len() >= 2, Blocked::TooFewToCombine),
            Edit::BooleanOp(op) => {
                let groups: Vec<u64> = chosen().filter(|s| matches!(s.compound(), Some(CompoundOp::Boolean(_)))).map(|s| s.id).collect();
                ok_if(!groups.is_empty(), Blocked::NotBooleanGroup)?;
                let two_units = |g: u64| self.units_by(&self.children(Some(g)).map(|s| s.id).collect::<Vec<_>>(), UnitKey::Attach).len() == 2;
                ok_if(*op != Combine::Slice || groups.into_iter().all(two_units), Blocked::SliceNeedsTwo)
            }
            Edit::Flatten => {
                ok_if(chosen().count() >= 2, Blocked::TooFewToFlatten)?;
                ok_if(!self.live_inputs(on), Blocked::LiveEffects)
            }
            Edit::AddEffect { effect: Effect::Warp(_), .. } => ok_if(chosen().any(|s| s.can_warp()), warp_blocked()),
            Edit::AddEffect { effect: Effect::Sticker(_), .. } if chosen().any(|s| matches!(s.kind, ShapeKind::Bitmap { .. })) => Err(Blocked::ImageSticker),
            Edit::AddEffect { reach, .. } if self.wraps_together(on, *reach) => Ok(()),
            Edit::AddEffect { effect: Effect::Sticker(_), .. } => ok_if(!chosen().any(|s| s.container.is_some()), Blocked::StickerOnContainer),
            Edit::AddEffect { effect: Effect::Offset(_), .. } => Ok(()),
            Edit::Effect { at, op } => {
                let shape = self.shapes.iter().find(|s| s.id == at.shape).filter(|s| crate::effects::has_row(s, at.row)).ok_or(Blocked::NoEntry)?;
                let mut after = shape.clone();
                after.change_effects(|fx| match (at.row, *op) {
                    (Row::Entry(id), EffectEdit::MoveTo(i)) => fx.move_to(id, i),
                    (Row::Entry(id), EffectEdit::Enable(on)) => fx.set_enabled(id, on),
                    _ => {}
                });
                if after.effects.warp_after_sticker() && !shape.effects.warp_after_sticker() {
                    return Err(Blocked::WarpAfterSticker);
                }
                match (at.row, op) {
                    (Row::Curve | Row::Contour, EffectEdit::MoveTo(_) | EffectEdit::Replace(_)) => Err(Blocked::PinnedRow),
                    (Row::Entry(id), EffectEdit::Replace(effect)) => ok_if(shape.effects.get(id).is_some_and(|e| e.effect.same_kind(effect)), Blocked::WrongEffect),
                    _ => Ok(()),
                }
            }
            Edit::Expand => ok_if(chosen().any(Shape::expands), Blocked::NoEffects),
            Edit::Unflatten => ok_if(self.flattened(on).is_some(), Blocked::NotFlattened),
            Edit::Align { .. } if chosen().all(|s| s.lock_position) => Err(Blocked::PositionLocked),
            Edit::Align { op, to } => {
                let need = to.min_units(*op);
                let blocked = match op {
                    Align::DistributeH | Align::DistributeV => Blocked::TooFewToDistribute { need },
                    _ => Blocked::TooFewToAlign { need },
                };
                ok_if(self.align_units_to(on, *to) >= need, blocked)
            }
            Edit::Warp(Some(_)) => ok_if(chosen().any(|s| s.can_warp()), warp_blocked()),
            Edit::Text(TextChange::Split(_)) => {
                let mut texts = chosen().filter(|s| matches!(s.kind, ShapeKind::Text { .. })).peekable();
                texts.peek().ok_or(Blocked::NotText)?;
                let mut refusals = texts.map(crate::text_ops::split_refusal);
                match refusals.find(Option::is_none) {
                    Some(_) => Ok(()),
                    None => Err(chosen().find_map(crate::text_ops::split_refusal).unwrap_or(Blocked::NotText)),
                }
            }
            Edit::Curve(_) | Edit::Text(_) | Edit::ConvertToPaths => ok_if(chosen().any(|s| matches!(s.kind, ShapeKind::Text { .. })), Blocked::NotText),
            Edit::Params(op) => ok_if(chosen().any(|s| matches!(&s.kind, ShapeKind::Parametric(p) if p.accepts(*op))), Blocked::NotParametric),
            Edit::TextPath(TextPathEdit::Attach) => ok_if(self.text_and_path(on).is_some() || self.lone_text(on).is_some(), Blocked::NeedTextAndPath),
            Edit::TextPath(_) => ok_if(chosen().any(|s| matches!(&s.kind, ShapeKind::Text { path: Some(_), .. })), Blocked::NotOnPath),
            Edit::Corners(_) => ok_if(chosen().any(crate::path_edit::editable), Blocked::NotPath),
            Edit::SetHidden(true) => ok_if(chosen().any(|s| !s.hidden), Blocked::AlreadyHidden),
            Edit::SetHidden(false) => ok_if(chosen().any(|s| s.hidden), Blocked::AlreadyVisible),
            Edit::GrainLock(true) => ok_if(chosen().any(|s| !s.grain_lock), Blocked::GrainLocked),
            Edit::GrainLock(false) => ok_if(chosen().any(|s| s.grain_lock), Blocked::GrainUnlocked),
            Edit::Lock { lock, on: wanted } => {
                let reach = self.lock_reach(on);
                let blocked = if *wanted { Blocked::AlreadyLocked(*lock) } else { Blocked::NotLocked(*lock) };
                ok_if(self.shapes.iter().any(|s| reach.contains(&s.id) && lock.of(s) != *wanted), blocked)
            }
            Edit::Flip(_) | Edit::Nudge { .. } | Edit::Field { .. } | Edit::Skew { .. } => ok_if(chosen().any(|s| !s.lock_position), Blocked::PositionLocked),
            Edit::TransformAgain { copy, .. } => ok_if(*copy || chosen().any(|s| !s.lock_position), Blocked::PositionLocked),
            Edit::Pattern(Some(_)) => ok_if(chosen().all(Shape::takes_pattern), Blocked::PatternNeedsPrint),
            Edit::Pattern(None) => ok_if(chosen().any(|s| s.pattern.is_some()), Blocked::NoPattern),
            Edit::Rename(_) => ok_if(self.named(on).is_some(), Blocked::NotOneLayer),
            Edit::BuildShape { .. } => ok_if(chosen().count() >= 2, Blocked::TooFewToCombine),
            Edit::Knife { .. } => Ok(()),
            Edit::Delete
            | Edit::Duplicate
            | Edit::Deselect
            | Edit::Arrange(_)
            | Edit::Warp(None)
            | Edit::Operation(_)
            | Edit::OutlineStrokes(_)
            | Edit::Colour(_)
            | Edit::Pen(_)
            | Edit::StepRepeat(_)
            | Edit::RotateCopies(_)
            | Edit::Pivot(_)
            | Edit::PasteStyle(_) => Ok(()),
        }
    }

    /// Runs `edit` on `on` once `check` passes. Returns the ids the user holds afterwards.
    pub fn apply(&mut self, edit: &Edit, on: &[u64]) -> Result<Vec<u64>, Blocked> {
        self.check(edit, on)?;
        let same = on.to_vec();
        let or_same = |made: Vec<u64>| if made.is_empty() { on.to_vec() } else { made };
        Ok(match edit {
            Edit::Delete => {
                self.remove_many(on);
                Vec::new()
            }
            Edit::Duplicate => self.duplicate(on, DUPLICATE_OFFSET_IN),
            Edit::SelectAll => crate::select::select_all(self, on),
            Edit::SelectSame(m) => crate::select::same(self, on, *m),
            Edit::SelectInverse => crate::select::inverse(self, on),
            Edit::SelectNext => crate::select::walk(self, on, true),
            Edit::SelectPrevious => crate::select::walk(self, on, false),
            Edit::Deselect => Vec::new(),
            Edit::Group => {
                self.group(on);
                same
            }
            Edit::Attach => {
                self.attach(on);
                same
            }
            Edit::Ungroup => {
                self.unlink(&self.members_of_kind(on, LinkKind::Group));
                let monograms = self.monograms_in(on);
                let mut held: Vec<u64> = on.iter().copied().filter(|id| !monograms.contains(id)).collect();
                for id in monograms {
                    held.extend(self.expand_compound(id));
                }
                held
            }
            Edit::Detach => {
                self.unlink(&self.members_of_kind(on, LinkKind::Attach));
                same
            }
            Edit::Contain => self.contain(on).map_or(same, |id| vec![id]),
            Edit::Uncontain => {
                self.uncontain(on);
                same
            }
            Edit::Arrange(op) => {
                self.arrange(on, *op);
                same
            }
            Edit::Flip(axis) => {
                let chosen: Vec<_> = self.shapes.iter().filter(|s| on.contains(&s.id) && !s.lock_position).cloned().collect();
                for flipped in transform::flip_many(self, &chosen, *axis) {
                    if let Some(s) = self.shape_mut(flipped.id) {
                        *s = flipped;
                    }
                }
                same
            }
            Edit::Align { op, to } => {
                self.align_to(on, *op, *to);
                same
            }
            Edit::Combine(op) => or_same(self.combine(on, *op)),
            Edit::BooleanOp(op) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && matches!(s.compound(), Some(CompoundOp::Boolean(_)))) {
                    s.kind = ShapeKind::Compound(CompoundOp::Boolean(*op));
                }
                same
            }
            Edit::Flatten => or_same(self.flatten(on).into_iter().collect()),
            Edit::Unflatten => or_same(self.flattened(on).map_or_else(Vec::new, |id| self.unflatten(id))),
            Edit::AddEffect { effect, reach } => self.add_effect(on, *effect, *reach),
            Edit::Effect { at, op } => {
                self.edit_effect(*at, *op);
                same
            }
            Edit::Expand => or_same(self.bake(on)),
            Edit::Warp(warp) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.set_warp(*warp);
                }
                same
            }
            Edit::Curve(curve_in) => {
                for &id in on {
                    self.set_text_curve(id, *curve_in);
                }
                same
            }
            Edit::TextPath(op) => self.apply_text_path(*op, on),
            Edit::Text(change) => on.iter().flat_map(|&id| self.change_text(id, change)).collect(),
            Edit::Params(op) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    if let ShapeKind::Parametric(p) = &mut s.kind {
                        p.apply(*op);
                    }
                }
                same
            }
            Edit::Corners(op) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && crate::path_edit::editable(s)) {
                    crate::path_edit::to_path(s);
                    let mut path = crate::path_edit::NodePath::read(s);
                    path.set_corners(op);
                    *s = path.write(s);
                }
                same
            }
            Edit::ConvertToPaths => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && matches!(s.kind, ShapeKind::Text { .. })) {
                    let contours = s.kind.outline(s.width_in, s.height_in);
                    s.kind = ShapeKind::Path { contours, view_box: (s.width_in, s.height_in), node_kinds: Vec::new(), corners: Vec::new() };
                }
                same
            }
            Edit::SetHidden(hidden) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.hidden = *hidden;
                }
                if *hidden { Vec::new() } else { same }
            }
            Edit::Nudge { dx_in, dy_in, board_in } => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && !s.lock_position) {
                    s.x_in = clamp_pos(s.x_in + dx_in, s.width_in, board_in.0);
                    s.y_in = clamp_pos(s.y_in + dy_in, s.height_in, board_in.1);
                }
                same
            }
            Edit::Field { field, value, board_in } => {
                let targets: Vec<u64> = self.shapes.iter().filter(|s| on.contains(&s.id) && !s.lock_position).map(|s| s.id).collect();
                // A typed Rotation on a lone pivoted shape turns it about that pivot, not its centre (spec 52).
                if *field == Field::Rotation
                    && let [id] = targets.as_slice()
                    && let Some(one) = self.shapes.iter().find(|s| s.id == *id).filter(|s| s.pivot.is_some()).cloned()
                {
                    let to = value.apply(one.rotation_deg);
                    let pivot = transform::Frame::of(self, std::slice::from_ref(&one)).unwrap().pivot_world(one.pivot);
                    for moved in transform::rotate_many_about(self, std::slice::from_ref(&one), to - one.rotation_deg, pivot) {
                        if let Some(s) = self.shape_mut(moved.id) {
                            *s = moved;
                        }
                    }
                } else {
                    for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && !s.lock_position) {
                        let to = value.apply(transform::field_value(s, *field));
                        transform::set_field(s, *field, to, *board_in);
                    }
                }
                same
            }
            Edit::TransformAgain { from, copy } => {
                let ids = self.with_descendants(on);
                let chosen: Vec<Shape> = self.shapes.iter().filter(|s| ids.contains(&s.id)).cloned().collect();
                let Some(now) = transform::Frame::of(self, &chosen) else { return Ok(same) };
                let moved = transform::remap_to(&chosen, &now, &now.stepped_from(from));
                if *copy {
                    self.place_copies(vec![moved]).into_iter().flatten().collect()
                } else {
                    for m in moved {
                        if let Some(s) = self.shape_mut(m.id).filter(|s| !s.lock_position) {
                            *s = m;
                        }
                    }
                    same
                }
            }
            Edit::StepRepeat(params) => {
                let subtree = self.subtree(on);
                let sets = crate::repeat::step_and_repeat(self, &subtree, *params);
                or_same(self.repeat_copies(on, &subtree, sets))
            }
            Edit::RotateCopies(params) => {
                let subtree = self.subtree(on);
                let pivot = transform::Frame::of(self, &subtree).map_or((0.0, 0.0), |f| {
                    let single = on.iter().find_map(|id| self.shapes.iter().find(|s| s.id == *id).filter(|s| s.pivot.is_some()));
                    f.pivot_world(single.and_then(|s| s.pivot).filter(|_| on.len() == 1))
                });
                let sets = crate::repeat::rotate_copies(self, &subtree, *params, pivot);
                or_same(self.repeat_copies(on, &subtree, sets))
            }
            Edit::Pivot(pivot) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.pivot = *pivot;
                }
                same
            }
            Edit::Skew { axis, value } => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id) && !s.lock_position) {
                    let current = if *axis == Axis::Horizontal { s.skew_deg.0 } else { s.skew_deg.1 };
                    let set = value.apply(current).clamp(-MAX_SKEW_DEG, MAX_SKEW_DEG);
                    if *axis == Axis::Horizontal {
                        s.skew_deg.0 = set;
                    } else {
                        s.skew_deg.1 = set;
                    }
                }
                same
            }
            Edit::Operation(op) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.set_operation(*op);
                }
                same
            }
            Edit::OutlineStrokes(widths) => {
                for &(id, width_in) in widths.iter().filter(|(id, _)| on.contains(id)) {
                    let Some(s) = self.shape_mut(id) else { continue };
                    let ShapeKind::Imported { contours, view_box } = &s.kind else { continue };
                    // The contours live in `view_box` units; the drawn shape scales them to its box, so
                    // offset by the stroke width in those units, not inches, and leave the box and pose
                    // untouched so a resized or rotated stroke outlines in place without jumping.
                    let (sx, sy) = (safe_scale(s.width_in, view_box.0), safe_scale(s.height_in, view_box.1));
                    let local_width = width_in / ((sx + sy) / 2.0).max(1e-6);
                    let rings = crate::svg_import::outline_stroke(contours, local_width);
                    if rings.is_empty() {
                        continue;
                    }
                    s.kind = ShapeKind::Imported { contours: rings, view_box: *view_box };
                    s.set_operation(Operation::BasicCut);
                }
                same
            }
            Edit::Colour(fill) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.fill = *fill;
                }
                same
            }
            Edit::Pen(pick) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.pick_pen(*pick);
                }
                same
            }
            Edit::Insert(Insert::Shape(kind)) => {
                let size = kind.default_size_in();
                let live = crate::parametric::insert_as(kind.clone(), size);
                let named = live != *kind && live.label() != kind.label();
                let id = self.add_shape_at(live, size);
                // A live preset keeps the tile's name (Square, Pentagon, Thin Star) only where its
                // parametric family would read differently, so it still shows what the user picked.
                if named && let Some(s) = self.shape_mut(id) {
                    s.name = Some(kind.label().to_owned());
                }
                vec![id]
            }
            Edit::Insert(Insert::Monogram { spec, at }) => vec![self.insert_monogram(spec, *at)],
            Edit::Monogram { id, spec } => {
                self.update_monogram(*id, spec);
                vec![*id]
            }
            Edit::Insert(Insert::Text(font)) => {
                let (x_in, y_in) = self.next_free_slot(INSERTED_TEXT_IN);
                vec![self.add_text("Text".into(), font.clone(), INSERTED_TEXT_IN, Color32::BLACK, x_in, y_in)]
            }
            Edit::Insert(Insert::Generated { title, outline }) => {
                self.place(&Placement { shapes: vec![Shape { name: Some(title.clone()), ..(*outline.0).clone() }], group: false })
            }
            Edit::Nesting(layout) => {
                self.nesting = layout.clone();
                same
            }
            Edit::Grid(grid) => {
                self.grid = Some(grid.sane());
                same
            }
            Edit::Snap(options) => {
                self.snap = *options;
                same
            }
            Edit::BuildShape { picks, delete } => or_same(self.build_shape(on, picks, *delete)),
            Edit::Knife { line } => or_same(self.knife_cut(on, line)),
            Edit::GrainLock(locked) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.grain_lock = *locked;
                }
                same
            }
            Edit::Lock { lock, on: wanted } => {
                let reach = self.lock_reach(on);
                for s in self.shapes.iter_mut().filter(|s| reach.contains(&s.id)) {
                    *lock.flag(s) = *wanted;
                }
                same
            }
            Edit::Place(p) => self.place(p),
            Edit::Guide(g) => {
                self.apply_guide(g);
                same
            }
            Edit::Paste { package, placement, drop_at } => match package.place(self, *placement, *drop_at) {
                Ok(result) => {
                    if !result.repairs.is_empty() {
                        tracing::info!(target: "cmd", repairs = %result.repairs, "clipboard paste repaired");
                    }
                    result.roots
                }
                Err(crate::clipboard::ClipboardError::ComponentCycle) => return Err(Blocked::ComponentCycle),
                Err(_) => return Err(Blocked::InvalidClipboard),
            },
            Edit::PasteStyle(style) => {
                for shape in self.shapes.iter_mut().filter(|shape| on.contains(&shape.id)) {
                    shape.set_operation(style.operation);
                    shape.pen = style.pen;
                    shape.fill = style.fill;
                    shape.pattern = style.pattern.clone();
                }
                same
            }
            Edit::Rename(name) => {
                if let Some(target) = self.named(on) {
                    self.rename(target, name.as_deref());
                }
                same
            }
            Edit::Pattern(fill) => {
                for s in self.shapes.iter_mut().filter(|s| on.contains(&s.id)) {
                    s.pattern = fill.clone();
                }
                same
            }
            Edit::MakeComponent
            | Edit::DetachInstance
            | Edit::PlaceInstance { .. }
            | Edit::RenameComponent { .. }
            | Edit::DuplicateComponent(_)
            | Edit::DeleteComponent(_)
            | Edit::Override { .. }
            | Edit::ResetOverrides
            | Edit::ResetPart(_) => self.apply_component(edit, on),
        })
    }

    /// Shape Builder (spec 57): builds the planar map of the subject's evaluated outlines, then either
    /// unites the regions the gesture crossed into one shape in the front-most input's style, or deletes
    /// them, and splits the rest region by region. Destructive, as Expand is. Returns the new ids.
    fn build_shape(&mut self, on: &[u64], picks: &[Pt], delete: bool) -> Vec<u64> {
        let Some(front) = self.shapes.iter().rev().find(|s| on.contains(&s.id)).cloned() else { return on.to_vec() };
        let inputs: Vec<Shape> = self.drawn().filter(|s| on.contains(&s.id)).cloned().collect();
        let outlines: Vec<Vec<crate::geometry::Ring>> = inputs.iter().map(|s| self.geometry(s).rings(crate::geometry::FLATTEN_TOL_IN)).collect();
        let regions = crate::geometry::planar_map(&outlines);
        if regions.is_empty() {
            return on.to_vec();
        }
        let crossed = |r: &crate::geometry::Region| picks.iter().any(|&p| crate::geometry::contains(&r.ring, p));
        let mut fresh = self.next_id()..;
        let make =
            |id: u64, rings: Vec<crate::geometry::Ring>| crate::design::shape_from_rings(id, rings, front.fill, front.operation).map(|s| Shape { pen: front.pen, ..s });
        let mut out: Vec<Shape> = Vec::new();
        if !delete {
            let merged: Vec<Vec<crate::geometry::Ring>> = regions.iter().filter(|r| crossed(r)).map(|r| vec![r.ring.clone()]).collect();
            if !merged.is_empty() {
                let welded = crate::geometry::boolean(&merged, crate::geometry::Boolean::Weld);
                out.extend(make(fresh.next().unwrap_or_default(), welded));
            }
        }
        for region in regions.iter().filter(|r| !crossed(r)) {
            out.extend(make(fresh.next().unwrap_or_default(), vec![region.ring.clone()]));
        }
        self.replace_with(on, out)
    }

    /// Knife (spec 57): cuts each subject shape along `line`. A shape with closed rings splits into its
    /// connected pieces; an open path is cut at each crossing. The pieces come from the evaluated
    /// outline, so a live effect is expanded and the pieces carry none; they keep the source's colour,
    /// operation, pen and name, the later ones suffixed " 2", " 3". A shape the line does not divide is
    /// left as it was. Returns the ids selected afterwards.
    fn knife_cut(&mut self, on: &[u64], line: &[Pt]) -> Vec<u64> {
        let targets: Vec<u64> = self.drawn().filter(|s| on.contains(&s.id)).map(|s| s.id).collect();
        let mut selected: Vec<u64> = Vec::new();
        for id in targets {
            let Some(source) = self.shapes.iter().find(|s| s.id == id).cloned() else { continue };
            let base = self.layer_name(&source);
            let geom = self.geometry(&source);
            let closed_rings = geom.rings(crate::geometry::FLATTEN_TOL_IN);
            let open: Vec<Vec<Pt>> = geom.pieces().iter().flat_map(|p| geom.world_contours(p)).filter(|c| !c.closed).map(|c| c.flatten()).collect();
            drop(geom);
            let mut fresh = self.next_id()..;
            let mut pieces: Vec<Shape> = Vec::new();
            let mut cut_happened = false;
            let open_shape = |id, sub: &[Pt], source: &Shape| {
                let contour = Contour { start: sub[0], segs: sub[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: false };
                crate::design::shape_from_contours(id, vec![contour], source.fill, source.operation).map(|s| Shape { pen: source.pen, ..s })
            };
            if let [at] = line {
                // Scissors: open or cut the contour nearest the click.
                let closed_polys: Vec<Vec<Pt>> = closed_rings.clone();
                let nearest_closed = closed_polys.iter().map(|p| crate::knife::distance_to(p, true, *at)).enumerate().min_by(|a, b| a.1.total_cmp(&b.1));
                let nearest_open = open.iter().map(|p| crate::knife::distance_to(p, false, *at)).enumerate().min_by(|a, b| a.1.total_cmp(&b.1));
                let take_closed = match (&nearest_closed, &nearest_open) {
                    (Some((_, dc)), Some((_, doo))) => dc <= doo,
                    (Some(_), None) => true,
                    _ => false,
                };
                if take_closed {
                    if let Some((i, _)) = nearest_closed.filter(|&(_, d)| d <= SCISSORS_REACH_IN) {
                        for poly in crate::knife::scissors(&closed_polys[i], true, *at) {
                            pieces.extend(open_shape(fresh.next().unwrap_or_default(), &poly, &source));
                        }
                        cut_happened = true;
                    }
                } else if let Some((i, _)) = nearest_open.filter(|&(_, d)| d <= SCISSORS_REACH_IN) {
                    let cut = crate::knife::scissors(&open[i], false, *at);
                    cut_happened = cut.len() > 1;
                    for poly in cut {
                        pieces.extend(open_shape(fresh.next().unwrap_or_default(), &poly, &source));
                    }
                }
            } else {
                let closed_pieces = if closed_rings.is_empty() { Vec::new() } else { crate::knife::split_closed(&closed_rings, line) };
                if closed_pieces.len() >= 2 {
                    cut_happened = true;
                    for rings in closed_pieces {
                        pieces.extend(
                            crate::design::shape_from_rings(fresh.next().unwrap_or_default(), rings, source.fill, source.operation)
                                .map(|s| Shape { pen: source.pen, ..s }),
                        );
                    }
                }
                for poly in &open {
                    let cut = crate::knife::cut_open(poly, line);
                    cut_happened |= cut.len() > 1;
                    for sub in cut {
                        pieces.extend(open_shape(fresh.next().unwrap_or_default(), &sub, &source));
                    }
                }
            }
            if !cut_happened {
                selected.push(id);
                continue;
            }
            // Keep the source's displayed name on the first piece, and suffix the rest, so a cut Heart
            // becomes Heart and Heart 2 even though each piece is now a plain path.
            for (i, piece) in pieces.iter_mut().enumerate() {
                piece.name = Some(if i == 0 { base.clone() } else { format!("{base} {}", i + 1) });
            }
            selected.extend(self.replace_with(&[id], pieces));
        }
        selected
    }

    /// What a lock on `on` covers: each whole object, a link with all its members and a container or
    /// group with everything in it.
    fn lock_reach(&self, on: &[u64]) -> Vec<u64> {
        self.with_descendants(&self.expand(on))
    }

    /// Whether anything `on` would feed Flatten (its subtrees included, Compounds expanded) has a live
    /// Offset or Sticker, which would be welded into the result and lost.
    fn live_inputs(&self, on: &[u64]) -> bool {
        self.subtree_parts(on).iter().any(|s| s.effects.has_live_pieces())
    }

    /// Adds `p`'s shapes on top under fresh ids, unlinked and at the top level. Returns the new ids.
    fn place(&mut self, p: &Placement) -> Vec<u64> {
        let ids: Vec<u64> = (self.next_id()..).take(p.shapes.len()).collect();
        for (&id, s) in ids.iter().zip(&p.shapes) {
            self.shapes.push(Shape { id, link: None, container: None, parent: None, ..s.clone() });
        }
        if p.group && ids.len() >= 2 {
            self.group(&ids);
        }
        ids
    }

    /// Select Same's enable rule: the two kind rows need a shape of that kind, the alike rows need a
    /// selection that shares one value (spec 50).
    fn check_select_same(&self, m: Match, on: &[u64]) -> Result<(), Blocked> {
        match m {
            Match::Text | Match::Paths => ok_if(!crate::select::same(self, on, m).is_empty(), Blocked::NoneToSelect(m)),
            _ => {
                ok_if(self.shapes.iter().any(|s| on.contains(&s.id)), Blocked::NothingSelected)?;
                ok_if(!crate::select::mixed(self, on, m), Blocked::Mixed(m))
            }
        }
    }

    /// Select Inverse, Tab and Shift+Tab need at least one selectable shape to walk to.
    fn check_selectable(&self) -> Result<(), Blocked> {
        let blocked = if self.shapes.is_empty() { Blocked::NoShapes } else { Blocked::AllLocked };
        ok_if(crate::select::selectable(self).next().is_some(), blocked)
    }

    /// The selected shapes with their descendants, in `with_descendants` order, so a copy set lines up
    /// with it position for position.
    pub(crate) fn subtree(&self, on: &[u64]) -> Vec<Shape> {
        self.with_descendants(on).iter().filter_map(|id| self.shapes.iter().find(|s| s.id == *id)).cloned().collect()
    }

    /// Adds `sets` of copies, groups the source with the copies' roots (the copies of `on`) and returns
    /// the group's members, so Step and repeat and Rotate copies each make one undo step (spec 52).
    fn repeat_copies(&mut self, on: &[u64], subtree: &[Shape], sets: Vec<Vec<Shape>>) -> Vec<u64> {
        if sets.is_empty() {
            return Vec::new();
        }
        let root_pos: Vec<usize> = subtree.iter().enumerate().filter(|(_, s)| on.contains(&s.id)).map(|(i, _)| i).collect();
        let placed = self.place_copies(sets);
        let mut roots = on.to_vec();
        for set in &placed {
            roots.extend(root_pos.iter().filter_map(|&i| set.get(i).copied()));
        }
        self.group(&roots);
        roots
    }

    /// The id of `on` when it is exactly one flattened shape.
    fn flattened(&self, on: &[u64]) -> Option<u64> {
        let [id] = on else { return None };
        self.shapes.iter().find(|s| s.id == *id && matches!(s.kind, ShapeKind::Flattened { .. })).map(|s| s.id)
    }
}

impl AppCommand {
    /// `on` is what the command would act on, the selection.
    pub fn check(&self, design: &Design, history: &History, on: &[u64]) -> Result<(), Blocked> {
        match self {
            Self::Rename => design.check(&Edit::Rename(None), on),
            Self::EditMonogram => design.editable_monogram(on).map(drop),
            Self::Undo => ok_if(history.can_undo(), Blocked::NothingToUndo),
            Self::Redo => ok_if(history.can_redo(), Blocked::NothingToRedo),
            Self::Make | Self::CutPreview => ok_if(!design.shapes.is_empty(), Blocked::NoShapes),
            Self::Preview => ok_if(history.can_undo() || !design.shapes.is_empty(), Blocked::NoShapes),
            Self::Export | Self::PrepareForDesignSpace => ok_if(crate::export::has_output(design), Blocked::NoShapes),
            Self::EditMaster(c) => ok_if(design.component(*c).is_some(), Blocked::NoComponent),
            Self::ZoomToSelection => ok_if(!on.is_empty(), Blocked::NothingToZoomTo),
            Self::ReplaceColour => ok_if(design.shapes.iter().any(|s| s.operation != Operation::Pen && s.pattern.is_none()), Blocked::NoShapes),
            Self::SavePaletteToProject => Ok(()),
            Self::CopyPaletteToSettings => ok_if(!design.swatches.is_empty(), Blocked::NoShapes),
            Self::StepRepeatPanel | Self::RotateCopiesPanel => ok_if(!on.is_empty(), Blocked::NothingSelected),
            Self::New
            | Self::Home
            | Self::Save
            | Self::Open(_)
            | Self::Palette
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ResetZoom
            | Self::ZoomToFit
            | Self::ZoomToAll
            | Self::ZoomTo(_)
            | Self::Units(_)
            | Self::Snapping
            | Self::SnapClass(_)
            | Self::SnapSettings
            | Self::PartialGrid
            | Self::GridSettings
            | Self::VersionHistory
            | Self::Upload
            | Self::LeaveMaster
            | Self::CloseTab(_)
            | Self::NextTab
            | Self::PreviousTab
            | Self::Tab(_)
            | Self::Tutorial
            | Self::Shortcuts
            | Self::About
            | Self::AiSettings => Ok(()),
        }
    }

    /// Whether the command runs after `History::observe` rather than before it. Undo and Redo rewrite
    /// history themselves, and the rest change which document is active, so none of them is an undo step.
    pub fn after_observe(&self) -> bool {
        match self {
            Self::Undo | Self::Redo | Self::New | Self::Open(_) | Self::CloseTab(_) | Self::NextTab | Self::PreviousTab | Self::Tab(_) | Self::Tutorial => true,
            Self::Home
            | Self::Save
            | Self::Export
            | Self::PrepareForDesignSpace
            | Self::Make
            | Self::Preview
            | Self::Palette
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ResetZoom
            | Self::ZoomToFit
            | Self::ZoomToSelection
            | Self::ZoomToAll
            | Self::ZoomTo(_)
            | Self::Units(_)
            | Self::Snapping
            | Self::SnapClass(_)
            | Self::SnapSettings
            | Self::PartialGrid
            | Self::GridSettings
            | Self::CutPreview
            | Self::VersionHistory
            | Self::Upload
            | Self::EditMaster(_)
            | Self::LeaveMaster
            | Self::Shortcuts
            | Self::About
            | Self::AiSettings
            | Self::Rename
            | Self::EditMonogram
            | Self::ReplaceColour
            | Self::SavePaletteToProject
            | Self::CopyPaletteToSettings
            | Self::StepRepeatPanel
            | Self::RotateCopiesPanel => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::FontRef;
    use crate::text_ops::Split;
    use crate::warp::WarpStyle;

    /// Squares 1 to 4 and a circle 5, 3 in apart.
    fn design() -> Design {
        let mut d = Design::default();
        for kind in [ShapeKind::Square, ShapeKind::Square, ShapeKind::Square, ShapeKind::Square, ShapeKind::Circle] {
            let id = d.add_shape(kind);
            let s = d.shape_mut(id).unwrap();
            (s.x_in, s.y_in) = (id as f32 * 3.0, 1.0);
        }
        d
    }

    const WARP: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };
    const SEL: AlignTo = AlignTo::Selection;
    const OFFSET: Effect =
        Effect::Offset(crate::effects::Offset { distance_in: 0.25, direction: crate::effects::Direction::Outset, join: crate::geometry::Join::Round, style: None });
    const STICKER: Effect = Effect::Sticker(crate::effects::Sticker { border_in: 0.125, corners: crate::effects::Corners::Round });
    const LAYER_UNLOCK: Edit = Edit::Lock { lock: Lock::Layer, on: false };
    const CURVE_EYE: Edit = Edit::Effect { at: EntryRef { shape: 1, row: Row::Curve }, op: EffectEdit::Enable(false) };

    /// Every edit, one of each payload shape.
    fn every_edit() -> Vec<Edit> {
        let board_in = (12.0, 12.0);
        let mut clip_design = Design::default();
        clip_design.add_shape(ShapeKind::Circle);
        let package = crate::clipboard::Package::capture(
            &clip_design,
            &[1],
            crate::clipboard::Source { document_id: "test".into(), mat_in: board_in },
            &crate::assets::Pixels::new(),
        )
        .unwrap()
        .validate()
        .unwrap();
        vec![
            Edit::Delete,
            Edit::Duplicate,
            Edit::SelectAll,
            Edit::Deselect,
            Edit::Group,
            Edit::Ungroup,
            Edit::Attach,
            Edit::Detach,
            Edit::Contain,
            Edit::Uncontain,
            Edit::Arrange(Arrange::ToFront),
            Edit::Flip(Axis::Horizontal),
            Edit::Align { op: Align::Left, to: SEL },
            Edit::Align { op: Align::DistributeH, to: SEL },
            Edit::Combine(Combine::Slice),
            Edit::Combine(Combine::Weld),
            Edit::BooleanOp(Combine::Subtract),
            Edit::Flatten,
            Edit::Unflatten,
            Edit::AddEffect { effect: OFFSET, reach: Reach::Together },
            Edit::AddEffect { effect: STICKER, reach: Reach::Together },
            Edit::Effect { at: EntryRef { shape: 1, row: Row::Curve }, op: EffectEdit::Enable(false) },
            Edit::Expand,
            Edit::Warp(Some(WARP)),
            Edit::Warp(None),
            Edit::Curve(4.0),
            Edit::Text(TextChange::SizePt(36.0)),
            Edit::SetHidden(true),
            Edit::SetHidden(false),
            Edit::Nudge { dx_in: 0.125, dy_in: 0.0, board_in },
            Edit::Field { field: Field::X, value: Typed::To(2.0), board_in },
            Edit::Operation(Operation::Pen),
            Edit::Colour(Color32::RED),
            Edit::Pen(PenPick::Colour(1)),
            Edit::Insert(Insert::Shape(ShapeKind::Heart)),
            Edit::Insert(Insert::Text(FontRef::SpaceGrotesk)),
            Edit::Nesting(None),
            Edit::GrainLock(true),
            Edit::GrainLock(false),
            Edit::Lock { lock: Lock::Position, on: true },
            Edit::Lock { lock: Lock::Layer, on: false },
            Edit::Place(Placement { shapes: vec![Shape::new(0, ShapeKind::Circle)], group: false }),
            Edit::Paste { package: Box::new(package), placement: crate::clipboard::Placement::InPlace, drop_at: (0.0, 0.0) },
            Edit::PasteStyle(crate::clipboard::PasteStyle { operation: Operation::BasicCut, fill: Color32::BLACK, pen: Default::default(), pattern: None }),
        ]
    }

    /// Asserts every edit's check on `on`: blocked as `want` lists, enabled otherwise.
    fn expect(d: &Design, on: &[u64], want: &[(Edit, Blocked)]) {
        for edit in every_edit() {
            let wanted = want.iter().find(|(e, _)| *e == edit).map(|&(_, b)| b);
            assert_eq!(d.check(&edit, on).err(), wanted, "{edit:?} on {on:?}");
        }
    }

    /// What blocks for any lone object that is not text, a link or a container.
    fn lone(extra: &[(Edit, Blocked)]) -> Vec<(Edit, Blocked)> {
        let mut out = vec![
            (Edit::Group, Blocked::TooFewToLink),
            (Edit::Attach, Blocked::TooFewToLink),
            (Edit::Ungroup, Blocked::NoLink(LinkKind::Group)),
            (Edit::Detach, Blocked::NoLink(LinkKind::Attach)),
            (Edit::Uncontain, Blocked::NoContainer),
            (Edit::Align { op: Align::Left, to: SEL }, Blocked::TooFewToAlign { need: 2 }),
            (Edit::Align { op: Align::DistributeH, to: SEL }, Blocked::TooFewToDistribute { need: 3 }),
            (Edit::Combine(Combine::Slice), Blocked::SliceNeedsTwo),
            (Edit::Combine(Combine::Weld), Blocked::TooFewToCombine),
            (Edit::BooleanOp(Combine::Subtract), Blocked::NotBooleanGroup),
            (Edit::Flatten, Blocked::TooFewToFlatten),
            (Edit::Curve(4.0), Blocked::NotText),
            (Edit::Text(TextChange::SizePt(36.0)), Blocked::NotText),
            (Edit::SetHidden(false), Blocked::AlreadyVisible),
            (CURVE_EYE, Blocked::NoEntry),
            (Edit::Expand, Blocked::NoEffects),
            (Edit::GrainLock(false), Blocked::GrainUnlocked),
            (LAYER_UNLOCK, Blocked::NotLocked(Lock::Layer)),
        ];
        out.extend_from_slice(extra);
        out
    }

    #[test]
    fn a_pattern_fills_only_print_then_cut_shapes_and_clears_in_one_step() {
        let mut d = design();
        let fill = crate::patterns::PatternFill::new(crate::assets::id_of(&image::RgbaImage::new(1, 1)), 1.0, crate::patterns::PatternMode::Tile);
        let set = Edit::Pattern(Some(fill.clone()));
        assert_eq!(d.check(&set, &[1]), Err(Blocked::PatternNeedsPrint), "a cut shape takes no pattern");
        d.shape_mut(1).unwrap().set_operation(Operation::PrintThenCut);
        assert_eq!(d.check(&set, &[1, 2]), Err(Blocked::PatternNeedsPrint), "every subject must print");
        assert_eq!(d.check(&Edit::Pattern(None), &[1]), Err(Blocked::NoPattern));
        let mut sel = vec![1];
        run(&mut d, &mut sel, &set, Subject::Selection).unwrap();
        assert_eq!(d.shapes[0].pattern, Some(fill));
        run(&mut d, &mut sel, &Edit::Pattern(None), Subject::Selection).unwrap();
        assert_eq!(d.shapes[0].pattern, None);

        let image = d.add_shape(ShapeKind::Bitmap { asset: crate::assets::id_of(&image::RgbaImage::new(1, 1)), px: (1, 1), clip: Vec::new() });
        d.shape_mut(image).unwrap().set_operation(Operation::PrintThenCut);
        assert_eq!(d.check(&set, &[image]), Err(Blocked::PatternNeedsPrint), "an image keeps its own picture");
    }

    #[test]
    fn nothing_selected_blocks_everything_but_select_all_insert_and_nesting() {
        let d = design();
        for edit in every_edit() {
            let want = if matches!(edit, Edit::SelectAll | Edit::Insert(_) | Edit::Nesting(_) | Edit::Place(_) | Edit::Paste { .. }) {
                None
            } else {
                Some(Blocked::NothingSelected)
            };
            assert_eq!(d.check(&edit, &[]).err(), want, "{edit:?}");
        }
        assert_eq!(Design::default().check(&Edit::SelectAll, &[]), Err(Blocked::NoShapes));
        assert_eq!(Design::default().check(&Edit::Place(Placement { shapes: Vec::new(), group: true }), &[]), Err(Blocked::NothingToPlace));
    }

    #[test]
    fn one_shape() {
        expect(&design(), &[1], &lone(&[(Edit::Unflatten, Blocked::NotFlattened)]));
    }

    #[test]
    fn two_shapes() {
        expect(
            &design(),
            &[1, 2],
            &[
                (Edit::Ungroup, Blocked::NoLink(LinkKind::Group)),
                (Edit::Detach, Blocked::NoLink(LinkKind::Attach)),
                (Edit::Uncontain, Blocked::NoContainer),
                (Edit::Align { op: Align::DistributeH, to: SEL }, Blocked::TooFewToDistribute { need: 3 }),
                (Edit::Unflatten, Blocked::NotFlattened),
                (Edit::BooleanOp(Combine::Subtract), Blocked::NotBooleanGroup),
                (Edit::Curve(4.0), Blocked::NotText),
                (Edit::Text(TextChange::SizePt(36.0)), Blocked::NotText),
                (Edit::SetHidden(false), Blocked::AlreadyVisible),
                (CURVE_EYE, Blocked::NoEntry),
                (Edit::Expand, Blocked::NoEffects),
                (CURVE_EYE, Blocked::NoEntry),
                (Edit::Expand, Blocked::NoEffects),
                (Edit::GrainLock(false), Blocked::GrainUnlocked),
                (LAYER_UNLOCK, Blocked::NotLocked(Lock::Layer)),
            ],
        );
    }

    #[test]
    fn a_link_counts_once_for_linking_and_aligning_but_only_attach_counts_once_for_combine() {
        let mut d = design();
        d.group(&[1, 2]);
        expect(
            &d,
            &[1, 2],
            &[
                (Edit::Group, Blocked::TooFewToLink),
                (Edit::Attach, Blocked::TooFewToLink),
                (Edit::Detach, Blocked::NoLink(LinkKind::Attach)),
                (Edit::Uncontain, Blocked::NoContainer),
                (Edit::Align { op: Align::Left, to: SEL }, Blocked::TooFewToAlign { need: 2 }),
                (Edit::Align { op: Align::DistributeH, to: SEL }, Blocked::TooFewToDistribute { need: 3 }),
                (Edit::Unflatten, Blocked::NotFlattened),
                (Edit::BooleanOp(Combine::Subtract), Blocked::NotBooleanGroup),
                (Edit::Curve(4.0), Blocked::NotText),
                (Edit::Text(TextChange::SizePt(36.0)), Blocked::NotText),
                (Edit::SetHidden(false), Blocked::AlreadyVisible),
                (CURVE_EYE, Blocked::NoEntry),
                (Edit::Expand, Blocked::NoEffects),
                (CURVE_EYE, Blocked::NoEntry),
                (Edit::Expand, Blocked::NoEffects),
                (Edit::GrainLock(false), Blocked::GrainUnlocked),
                (LAYER_UNLOCK, Blocked::NotLocked(Lock::Layer)),
            ],
        );
        d.unlink(&[1, 2]);
        d.attach(&[1, 2]);
        for (edit, want) in
            [(Edit::Combine(Combine::Weld), Err(Blocked::TooFewToCombine)), (Edit::Detach, Ok(())), (Edit::Ungroup, Err(Blocked::NoLink(LinkKind::Group)))]
        {
            assert_eq!(d.check(&edit, &[1, 2]), want, "{edit:?} on an attached pair");
        }
        assert_eq!(d.check(&Edit::Group, &[1, 2, 3]), Ok(()), "a link and a lone shape are two objects");
    }

    #[test]
    fn a_container_and_a_shape_cannot_be_grouped_or_attached() {
        let mut d = design();
        let c = d.contain(&[1]).unwrap();
        for edit in [Edit::Group, Edit::Attach] {
            let blocked = d.check(&edit, &[c, 2]).unwrap_err();
            assert_eq!(blocked, Blocked::HasContainer, "{edit:?}");
            assert_eq!(blocked.reason(), "A container cannot be grouped or attached.");
            let before = d.clone();
            assert_eq!(d.apply(&edit, &[c, 2]), Err(Blocked::HasContainer));
            assert_eq!(d, before, "a blocked edit changes nothing");
        }
        assert_eq!(d.check(&Edit::Uncontain, &[c, 2]), Ok(()));
        assert_eq!(d.check(&Edit::Contain, &[c]), Err(Blocked::CannotContain), "already a container");
        assert_eq!(d.check(&Edit::Contain, &[c, 2]), Ok(()), "two objects wrap");
    }

    #[test]
    fn align_to_a_container_needs_one_child() {
        let mut d = design();
        let c = d.contain(&[5]).unwrap();
        d.reparent(&[1], Some(c));
        assert_eq!(d.check(&Edit::Align { op: Align::Left, to: AlignTo::Container }, &[1]), Ok(()));
        assert_eq!(d.check(&Edit::Align { op: Align::DistributeH, to: AlignTo::Container }, &[1]), Err(Blocked::TooFewToDistribute { need: 2 }));
        assert_eq!(d.align_target(&[1], AlignTo::Container), AlignTo::Container);
        assert_eq!(d.align_target(&[1], AlignTo::Selection), AlignTo::Selection, "the choice is remembered");
        assert_eq!(d.align_target(&[2, 3], AlignTo::Container), AlignTo::Selection, "outside a container Align uses the selection");
    }

    #[test]
    fn a_flattened_shape_unflattens_but_does_not_warp() {
        let mut d = design();
        let flat = d.flatten(&[1, 2]).unwrap();
        expect(&d, &[flat], &lone(&[(Edit::Warp(Some(WARP)), Blocked::CannotWarp)]));
        assert_eq!(d.check(&Edit::Warp(Some(WARP)), &[flat, 3]), Ok(()), "the square beside it can warp");
    }

    #[test]
    fn an_image_takes_an_offset_but_no_warp_or_sticker() {
        let mut d = design();
        let image = d.add_shape(ShapeKind::Bitmap { asset: crate::assets::id_of(&image::RgbaImage::new(2, 2)), px: (2, 2), clip: Vec::new() });
        let warp = Edit::AddEffect { effect: Effect::Warp(WARP), reach: Reach::EachShape };
        assert_eq!(d.check(&warp, &[image]), Err(Blocked::ImageCannotWarp));
        assert_eq!(d.check(&Edit::Warp(Some(WARP)), &[image]), Err(Blocked::ImageCannotWarp));
        assert_eq!(Blocked::ImageCannotWarp.reason(), "Images can't be warped.");
        assert_eq!(d.check(&Edit::AddEffect { effect: STICKER, reach: Reach::EachShape }, &[image]), Err(Blocked::ImageSticker));
        assert_eq!(
            d.check(&Edit::AddEffect { effect: STICKER, reach: Reach::Together }, &[image, 1]),
            Err(Blocked::ImageSticker),
            "a sticker would paint over the picture"
        );
        assert_eq!(d.check(&Edit::AddEffect { effect: OFFSET, reach: Reach::EachShape }, &[image]), Ok(()));
    }

    #[test]
    fn text_takes_text_edits_and_hidden_shapes_show() {
        let mut d = design();
        let t = d.add_text("Hi there".into(), FontRef::Inter, 1.0, Color32::BLACK, 1.0, 8.0);
        let mut want = lone(&[(Edit::Unflatten, Blocked::NotFlattened)]);
        want.retain(|(e, _)| !matches!(e, Edit::Curve(_) | Edit::Text(_)));
        expect(&d, &[t], &want);
        for edit in [Edit::Curve(4.0), Edit::Text(TextChange::SizePt(36.0)), Edit::Text(TextChange::Split(Split::Letters))] {
            assert_eq!(d.check(&edit, &[t, 1]), Ok(()), "{edit:?} with a square too");
        }
        d.shape_mut(1).unwrap().hidden = true;
        assert_eq!(d.check(&Edit::SetHidden(true), &[1]), Err(Blocked::AlreadyHidden));
        assert_eq!(d.check(&Edit::SetHidden(false), &[1]), Ok(()));
    }

    #[test]
    fn flip_mirrors_every_shape_across_the_selection() {
        let mut d = design();
        let xs = |d: &Design| d.shapes.iter().take(3).map(|s| s.x_in).collect::<Vec<_>>();
        let before = xs(&d);
        let mut sel = vec![1, 2, 3];
        run(&mut d, &mut sel, &Edit::Flip(Axis::Horizontal), Subject::Selection).unwrap();
        let after = xs(&d);
        assert!((after[0] - before[2]).abs() < 1e-4 && (after[2] - before[0]).abs() < 1e-4, "{before:?} became {after:?}");
        assert!(d.shapes.iter().take(3).all(|s| s.flip_x));
        assert_eq!(sel, [1, 2, 3]);
    }

    #[test]
    fn the_selection_becomes_what_the_edit_leaves_in_hand() {
        let mut d = design();
        let mut sel = vec![1, 2];
        run(&mut d, &mut sel, &Edit::Duplicate, Subject::Selection).unwrap();
        assert_eq!(sel, [6, 7], "the copies");
        run(&mut d, &mut sel, &Edit::SetHidden(true), Subject::Selection).unwrap();
        assert!(sel.is_empty(), "hidden shapes leave the selection");
        sel = vec![3, 4];
        run(&mut d, &mut sel, &Edit::Combine(Combine::Unite), Subject::Selection).unwrap();
        assert_eq!(sel.len(), 1, "the united shape");
        assert_eq!(run(&mut d, &mut sel, &Edit::Group, Subject::Selection), Err(Blocked::TooFewToLink));
        run(&mut d, &mut sel, &Edit::Delete, Subject::Selection).unwrap();
        assert!(sel.is_empty());
    }

    #[test]
    fn a_row_edit_leaves_the_selection_but_drops_what_it_deleted() {
        let mut d = design();
        let mut sel = vec![1, 2];
        run(&mut d, &mut sel, &Edit::SetHidden(true), Subject::Ids(&[2])).unwrap();
        assert_eq!(sel, [1, 2], "the eye keeps the selection");
        run(&mut d, &mut sel, &Edit::Delete, Subject::Ids(&[2])).unwrap();
        assert_eq!(sel, [1]);
    }

    #[test]
    fn nudge_and_typed_positions_clamp_to_one_board_of_void() {
        let mut d = design();
        let board_in = (12.0, 12.0);
        d.apply(&Edit::Nudge { dx_in: -100.0, dy_in: 0.0, board_in }, &[1]).unwrap();
        assert_eq!(d.shapes[0].x_in, -12.0);
        d.apply(&Edit::Field { field: Field::X, value: Typed::To(1000.0), board_in }, &[1]).unwrap();
        assert_eq!(d.shapes[0].x_in, 24.0 - d.shapes[0].width_in);
    }

    #[test]
    fn a_scaling_field_edit_scales_each_shape_by_its_own_width() {
        let mut d = design();
        d.shape_mut(1).unwrap().width_in = 1.0;
        d.shape_mut(2).unwrap().width_in = 2.5;
        let edit = Edit::Field { field: Field::W, value: Typed::Times(2.0), board_in: (12.0, 12.0) };
        d.apply(&edit, &[1, 2]).unwrap();
        assert_eq!((d.shapes[0].width_in, d.shapes[1].width_in), (2.0, 5.0));
    }

    #[test]
    fn a_position_locked_shape_ignores_move_resize_rotate_flip_and_nudge() {
        let mut d = design();
        d.apply(&Edit::Lock { lock: Lock::Position, on: true }, &[1]).unwrap();
        let board_in = (12.0, 12.0);
        let moves = [
            Edit::Nudge { dx_in: 0.125, dy_in: 0.0, board_in },
            Edit::Field { field: Field::X, value: Typed::To(5.0), board_in },
            Edit::Field { field: Field::W, value: Typed::Times(2.0), board_in },
            Edit::Field { field: Field::Rotation, value: Typed::To(45.0), board_in },
            Edit::Flip(Axis::Horizontal),
        ];
        let locked = d.shapes[0].clone();
        for edit in &moves {
            assert_eq!(d.check(edit, &[1]), Err(Blocked::PositionLocked), "{edit:?}");
            let free = d.shapes[1].clone();
            d.apply(edit, &[1, 2]).unwrap();
            assert_eq!(d.shapes[0], locked, "{edit:?} leaves the locked square be");
            assert_ne!(d.shapes[1], free, "{edit:?} still changes the free one");
        }
        assert_eq!(d.check(&Edit::Align { op: Align::Left, to: SEL }, &[1]), Err(Blocked::PositionLocked));
        d.shape_mut(2).unwrap().x_in = 7.0;
        d.apply(&Edit::Align { op: Align::Left, to: SEL }, &[1, 2]).unwrap();
        assert_eq!(d.shapes[0], locked, "Align measures against the locked square");
        let left = |s: &Shape| d.geometry(s).bounds().min_x;
        assert!((left(&d.shapes[1]) - left(&locked)).abs() < 1e-4, "and lines the free one up with it");
        d.apply(&Edit::Operation(Operation::Perforate), &[1]).unwrap();
        d.apply(&Edit::Colour(Color32::RED), &[1]).unwrap();
        assert_eq!((d.shapes[0].fill, d.shapes[0].operation), (Color32::RED, Operation::Perforate), "it can still be recoloured");
        assert_eq!(Blocked::PositionLocked.reason(), "The selection's position is locked. Unlock it to move, resize, rotate or flip it.");
    }

    #[test]
    fn a_locked_layer_is_not_picked_and_select_all_skips_it() {
        let mut d = design();
        d.apply(&Edit::Lock { lock: Lock::Layer, on: true }, &[2]).unwrap();
        let (cx, cy) = d.shapes[1].centre_in();
        assert_eq!((d.hit_test(cx, cy), d.pick_at(cx, cy)), (Some(2), None), "drawn there but not picked");
        (d.shapes[0].x_in, d.shapes[0].y_in) = (d.shapes[1].x_in, d.shapes[1].y_in);
        assert_eq!(d.pick_at(cx, cy), Some(1), "the click falls through to the square beneath");
        let mut sel = Vec::new();
        run(&mut d, &mut sel, &Edit::SelectAll, Subject::Selection).unwrap();
        assert_eq!(sel, [1, 3, 4, 5]);
        d.apply(&Edit::Lock { lock: Lock::Layer, on: true }, &[1, 3, 4, 5]).unwrap();
        assert_eq!(d.check(&Edit::SelectAll, &[]), Err(Blocked::AllLocked));
    }

    #[test]
    fn a_lock_covers_each_whole_object_and_each_toggle_is_one_undo_step() {
        let mut d = design();
        d.group(&[1, 2]);
        let c = d.contain(&[3]).unwrap();
        d.reparent(&[4], Some(c));
        let mut history = History::default();
        let mut sel = Vec::new();
        let locked = |d: &Design| d.shapes.iter().filter(|s| s.locked).map(|s| s.id).collect::<Vec<_>>();
        let mut lock = |d: &mut Design, lock: Lock, on: bool, ids: &[u64]| {
            let before = d.clone();
            run(d, &mut sel, &Edit::Lock { lock, on }, Subject::Ids(ids)).unwrap();
            history.observe(before, d, false);
        };
        lock(&mut d, Lock::Layer, true, &[1]);
        assert_eq!(locked(&d), [1, 2], "a member row locks its whole group");
        lock(&mut d, Lock::Layer, true, &[c]);
        assert_eq!(locked(&d), [1, 2, 3, 4], "a container row locks what it holds");
        assert_eq!(d.check(&Edit::Lock { lock: Lock::Layer, on: true }, &[2]), Err(Blocked::AlreadyLocked(Lock::Layer)));
        lock(&mut d, Lock::Aspect, true, &[5]);
        assert!(d.shapes[4].lock_aspect && !d.shapes[4].lock_position && !d.shapes[4].locked, "one lock at a time");
        history.undo(&mut d);
        assert!(!d.shapes[4].lock_aspect);
        history.undo(&mut d);
        assert_eq!(locked(&d), [1, 2]);
        history.undo(&mut d);
        assert!(locked(&d).is_empty());
    }

    #[test]
    fn a_lock_on_a_group_covers_its_children_and_a_locked_group_stays_put() {
        let mut d = design();
        (d.shapes[1].x_in, d.shapes[2].x_in) = (3.5, 4.0);
        let g = d.apply(&Edit::Combine(Combine::Weld), &[2, 3]).unwrap()[0];
        assert!(d.shape_mut(g).unwrap().compound().is_some());
        d.apply(&Edit::Lock { lock: Lock::Position, on: true }, &[g]).unwrap();
        d.apply(&Edit::Lock { lock: Lock::Layer, on: true }, &[g]).unwrap();
        let tree = d.with_descendants(&[g]);
        assert_eq!(tree.len(), 3);
        assert!(d.shapes.iter().filter(|s| tree.contains(&s.id)).all(|s| s.lock_position && s.locked), "the group and both children");

        let poses = |d: &Design| d.shapes.iter().filter(|s| tree.contains(&s.id)).map(|s| (s.x_in, s.y_in)).collect::<Vec<_>>();
        let before = d.clone();
        assert_eq!(d.check(&Edit::Nudge { dx_in: 1.0, dy_in: 0.0, board_in: (12.0, 12.0) }, &[g]), Err(Blocked::PositionLocked));
        d.apply(&Edit::Nudge { dx_in: 1.0, dy_in: 1.0, board_in: (12.0, 12.0) }, &[g, 5]).unwrap();
        d.settle(Some(&before));
        assert_eq!(poses(&d), poses(&before), "neither the group nor its children move");

        let (cx, cy) = d.shapes.iter().find(|s| s.id == 2).unwrap().centre_in();
        assert_eq!((d.hit_test(cx, cy), d.pick_at(cx, cy)), (Some(g), Some(1)), "a click passes through the locked group to the square beneath");
    }

    #[test]
    fn insert_adds_one_object_in_a_free_slot_and_holds_it() {
        let mut d = design();
        let mut sel = vec![1, 2];
        run(&mut d, &mut sel, &Edit::Insert(Insert::Shape(ShapeKind::Heart)), Subject::Selection).unwrap();
        let heart = d.shapes.last().unwrap().clone();
        assert_eq!((sel.as_slice(), &heart.kind), (&[heart.id][..], &ShapeKind::Heart));
        run(&mut d, &mut sel, &Edit::Insert(Insert::Text(FontRef::SpaceGrotesk)), Subject::Selection).unwrap();
        let text = d.shapes.last().unwrap();
        assert_eq!(sel, [text.id]);
        assert!(matches!(&text.kind, ShapeKind::Text { font: FontRef::SpaceGrotesk, content, .. } if content == "Text"));
        assert_ne!((text.x_in, text.y_in), (heart.x_in, heart.y_in), "the text takes its own slot");
    }

    #[test]
    fn grain_lock_sets_the_subject_and_nesting_needs_no_subject() {
        let mut d = design();
        let mut sel = vec![1, 2];
        run(&mut d, &mut sel, &Edit::GrainLock(true), Subject::Selection).unwrap();
        assert_eq!(d.shapes.iter().map(|s| s.grain_lock).collect::<Vec<_>>(), [true, true, false, false, false]);
        assert_eq!(d.check(&Edit::GrainLock(true), &[1, 2]), Err(Blocked::GrainLocked));
        assert_eq!(d.check(&Edit::GrainLock(true), &[1, 3]), Ok(()), "one unlocked shape is enough");
        let mut none = Vec::new();
        let layout = crate::nesting::NestedLayout { fingerprint: 1, settings: Default::default(), mats: Vec::new() };
        run(&mut d, &mut none, &Edit::Nesting(Some(layout.clone())), Subject::Selection).unwrap();
        assert_eq!(d.nesting, Some(layout));
        assert!(none.is_empty(), "the selection is left alone");
    }

    #[test]
    fn app_commands_follow_the_history_and_the_canvas() {
        let mut d = Design::default();
        let h = History::default();
        assert_eq!(AppCommand::Undo.check(&d, &h, &[]), Err(Blocked::NothingToUndo));
        assert_eq!(AppCommand::Redo.check(&d, &h, &[]), Err(Blocked::NothingToRedo));
        assert_eq!(AppCommand::Make.check(&d, &h, &[]), Err(Blocked::NoShapes));
        assert_eq!(AppCommand::Save.check(&d, &h, &[]), Ok(()));
        d.add_shape(ShapeKind::Square);
        assert_eq!(AppCommand::Make.check(&d, &h, &[]), Ok(()));
    }

    #[test]
    fn rename_names_one_layer_or_one_whole_link_and_starting_it_follows_the_same_rule() {
        let mut d = design();
        let h = History::default();
        d.group(&[1, 2]);
        assert_eq!(AppCommand::Rename.check(&d, &h, &[]), Err(Blocked::NothingSelected));
        assert_eq!(AppCommand::Rename.check(&d, &h, &[1, 3]), Err(Blocked::NotOneLayer));
        assert_eq!(AppCommand::Rename.check(&d, &h, &[3]), Ok(()));
        assert_eq!(AppCommand::Rename.check(&d, &h, &[1, 2]), Ok(()));
        d.apply(&Edit::Rename(Some("Badge".into())), &[1, 2]).unwrap();
        d.apply(&Edit::Rename(Some("Speech Bubble".into())), &[3]).unwrap();
        assert_eq!(d.name_of(d.named(&[2, 1]).unwrap()).unwrap(), "Badge");
        assert_eq!(d.layer_name(&d.shapes[2]), "Speech Bubble");
        assert_eq!(d.apply(&Edit::Rename(None), &[1, 3]), Err(Blocked::NotOneLayer));
        d.apply(&Edit::Rename(None), &[3]).unwrap();
        assert_eq!(d.layer_name(&d.shapes[2]), "Square");
    }

    fn writing_text(d: &mut Design) -> u64 {
        let font = FontRef::Stroke(crate::stroke_font::StrokeFontId("hershey-simplex".into()));
        d.add_text("Hi".into(), font, 1.0, Color32::BLACK, 1.0, 6.0)
    }

    #[test]
    fn single_line_text_blocks_combine_offset_and_sticker_but_not_attach() {
        let mut d = Design::default();
        let square = d.add_shape(ShapeKind::Square);
        let text = writing_text(&mut d);
        let both = [square, text];
        let offset = Edit::AddEffect { effect: OFFSET, reach: Reach::Together };
        let sticker = Edit::AddEffect { effect: STICKER, reach: Reach::Together };
        for op in [Combine::Slice, Combine::Weld, Combine::Unite, Combine::Subtract, Combine::Intersect, Combine::Exclude] {
            assert_eq!(d.check(&Edit::Combine(op), &both), Err(Blocked::SingleLineText), "{op:?}");
        }
        assert_eq!(d.check(&offset, &[text]), Err(Blocked::SingleLineText));
        assert_eq!(d.check(&sticker, &both), Err(Blocked::SingleLineText));
        assert_eq!(Blocked::SingleLineText.reason(), "Single-line text has no area to combine.");
        assert_eq!(d.check(&Edit::Attach, &both), Ok(()));
        assert_eq!(d.check(&offset, &[square]), Ok(()), "outline shapes still offset");
    }

    #[test]
    fn convert_to_paths_keeps_the_box_style_and_open_strokes() {
        let mut d = Design::default();
        let text = writing_text(&mut d);
        let square = d.add_shape(ShapeKind::Square);
        assert_eq!(d.check(&Edit::ConvertToPaths, &[square]), Err(Blocked::NotText));
        let before = d.shapes[0].clone();
        let outline = before.kind.outline(before.width_in, before.height_in);
        let mut sel = vec![text, square];
        run(&mut d, &mut sel, &Edit::ConvertToPaths, Subject::Selection).unwrap();
        let after = &d.shapes[0];
        let ShapeKind::Path { contours, view_box, .. } = &after.kind else { panic!("not a path: {:?}", after.kind) };
        assert_eq!(contours, &outline, "the same contours");
        assert!(contours.iter().all(|c| !c.closed), "writing strokes stay open");
        assert_eq!(*view_box, (before.width_in, before.height_in));
        assert_eq!(
            (after.x_in, after.y_in, after.width_in, after.height_in, after.operation, after.fill),
            (before.x_in, before.y_in, before.width_in, before.height_in, before.operation, before.fill)
        );
        assert_eq!(d.shapes[1].kind, ShapeKind::Square, "only text converts");
    }

    #[test]
    fn outline_strokes_outlines_a_resized_stroke_in_place() {
        // An imported stroke resized to twice its view box after import.
        let mut d = Design::default();
        let square = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((1.0, 0.0)), Seg::Line((1.0, 1.0)), Seg::Line((0.0, 1.0))], closed: true };
        let id = d.add_imported(vec![crate::shape_library::Import {
            x_in: 3.0,
            y_in: 4.0,
            view_box: (1.0, 1.0),
            contours: vec![square],
            fill: Color32::RED,
            operation: Operation::Pen,
            from_stroke: true,
            stroke_in: 0.1,
            ..Default::default()
        }])[0];
        d.shape_mut(id).unwrap().width_in = 2.0;
        d.shape_mut(id).unwrap().height_in = 2.0;

        let mut sel = vec![id];
        run(&mut d, &mut sel, &Edit::OutlineStrokes(vec![(id, 0.1)]), Subject::Selection).unwrap();

        let s = d.shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!((s.x_in, s.y_in, s.width_in, s.height_in), (3.0, 4.0, 2.0, 2.0), "the box and position do not move");
        assert_eq!(s.operation, Operation::BasicCut, "a filled ring cuts");
        let ShapeKind::Imported { contours, view_box } = &s.kind else { panic!("not imported") };
        assert_eq!(*view_box, (1.0, 1.0), "the local units are kept");
        assert!(contours.len() >= 2, "an outer and an inner loop");
    }

    /// One 1 in square at the origin, selected.
    fn one_square() -> (Design, Vec<u64>) {
        let mut d = Design::default();
        let id = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in) = (0.0, 0.0);
        (d, vec![id])
    }

    #[test]
    fn step_and_repeat_groups_the_source_with_its_copies_in_one_step() {
        let (mut d, mut sel) = one_square();
        let params = crate::repeat::StepRepeat { copies: 3, offset_in: (1.0, 0.0), rotation_deg: 0.0, scale: 1.0, grid: None };
        run(&mut d, &mut sel, &Edit::StepRepeat(params), Subject::Selection).unwrap();
        assert_eq!(d.shapes.len(), 4, "source and three copies");
        let mut xs: Vec<f32> = d.shapes.iter().map(|s| s.x_in).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs, [0.0, 1.0, 2.0, 3.0]);
        let link = d.shapes[0].link;
        assert!(link.is_some() && d.shapes.iter().all(|s| s.link == link), "all one group");
        assert_eq!(sel.len(), 4);
    }

    #[test]
    fn rotate_copies_makes_a_ring_as_one_group() {
        let (mut d, mut sel) = one_square();
        run(&mut d, &mut sel, &Edit::RotateCopies(crate::repeat::RotateCopies { copies: 8, total_deg: 360.0 }), Subject::Selection).unwrap();
        assert_eq!(d.shapes.len(), 8, "eight petals in the ring");
        let link = d.shapes[0].link;
        assert!(link.is_some() && d.shapes.iter().all(|s| s.link == link));
    }

    #[test]
    fn power_duplicate_repeats_the_offset_the_first_move_set() {
        let (mut d, mut sel) = one_square();
        // First duplicate: the copy lands at the default offset.
        run(&mut d, &mut sel, &Edit::Duplicate, Subject::Selection).unwrap();
        let copy = sel[0];
        // The frame the source sat at, as the document's memory would keep it.
        let source_frame = transform::Frame::of(&d, &[d.shapes.iter().find(|s| s.id == 1).unwrap().clone()]).unwrap();
        // Move the copy so it sits one inch right of the source.
        d.shape_mut(copy).unwrap().x_in = 1.0;
        run(&mut d, &mut sel, &Edit::TransformAgain { from: source_frame, copy: true }, Subject::Selection).unwrap();
        let mut xs: Vec<f32> = d.shapes.iter().map(|s| s.x_in).collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs, [0.0, 1.0, 2.0], "source, moved copy, stepped copy at an even pitch");
    }

    #[test]
    fn skew_sets_the_axis_and_clamps_past_eighty_nine_degrees() {
        let (mut d, mut sel) = one_square();
        run(&mut d, &mut sel, &Edit::Skew { axis: Axis::Horizontal, value: Typed::To(200.0) }, Subject::Selection).unwrap();
        assert_eq!(d.shapes[0].skew_deg, (MAX_SKEW_DEG, 0.0), "clamped to 89");
        run(&mut d, &mut sel, &Edit::Skew { axis: Axis::Vertical, value: Typed::To(-20.0) }, Subject::Selection).unwrap();
        assert_eq!(d.shapes[0].skew_deg, (MAX_SKEW_DEG, -20.0));
    }

    #[test]
    fn a_typed_rotation_turns_a_pivoted_shape_about_its_pivot() {
        let (mut d, mut sel) = one_square();
        d.shape_mut(1).unwrap().pivot = Some((0.0, 0.0)); // the top-left corner
        let before = d.shapes[0].centre_in();
        run(&mut d, &mut sel, &Edit::Field { field: Field::Rotation, value: Typed::To(90.0), board_in: (12.0, 12.0) }, Subject::Selection).unwrap();
        let after = d.shapes[0].centre_in();
        assert!((after.0 - before.0).abs() > 0.1 || (after.1 - before.1).abs() > 0.1, "the centre swung round the corner, {before:?} -> {after:?}");
        assert!((d.shapes[0].rotation_deg - 90.0).abs() < 1e-3);
    }

    #[test]
    fn place_adds_shapes_under_fresh_ids_and_holds_them() {
        let mut d = design();
        let stray = Shape { id: 1, link: d.shapes[0].link, ..Shape::new(1, ShapeKind::Heart) };
        let mut sel = vec![2];
        let place = Edit::Place(Placement { shapes: vec![stray.clone(), stray], group: true });
        assert_eq!(format!("{place:?}"), "Place(2 shapes, grouped)", "the log line summarises");
        run(&mut d, &mut sel, &place, Subject::Selection).unwrap();
        assert_eq!(sel, [6, 7], "new ids, not the stray ones");
        assert_eq!(d.shapes.len(), 7);
        assert!(d.shapes[5].link.is_some_and(|l| l.kind == LinkKind::Group) && d.shapes[5].link == d.shapes[6].link, "placed in one group");
        let mut sel = Vec::new();
        run(&mut d, &mut sel, &Edit::Place(Placement { shapes: vec![Shape::new(0, ShapeKind::Star)], group: true }), Subject::Selection).unwrap();
        assert_eq!(d.shapes.last().unwrap().link, None, "one shape is never grouped");
    }
}

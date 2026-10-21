//! The command table every surface reads (spec 23): each command's label, icon, tooltip body and
//! shortcut, the keyboard handling derived from the same shortcuts, and `Cx`, which resolves a command
//! against this frame's design into an `Entry` a surface paints. Enable rules live in the model
//! (`Design::check`, `AppCommand::check`); nothing here decides what is enabled.

use std::borrow::Cow;

use egui::{Color32, Context, Key, KeyboardShortcut, Modifiers};

use crate::containers::AlignTo;
use crate::design::{Align, Arrange, Combine, Design, History};
use crate::edit::{AppCommand, Blocked, Edit, Insert, Lock, TextPathEdit};
use crate::effects::{Corners, Direction, Effect, EffectEdit, Offset, Reach, Sticker};
use crate::geometry::Join;
use crate::guides::GuideEdit;
use crate::text_ops::TextChange;
use crate::transform::Axis;
use crate::units::Units;
use crate::warp::{Warp, WarpStyle};

use super::icons::Icon;

/// Anything a menu, button or key can fire.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Edit(Edit),
    App(AppCommand),
    Clipboard(super::clipboard::Action),
}

impl Command {
    /// Whether it only makes sense with the canvas open; the palette hides these on Home.
    pub fn needs_canvas(&self) -> bool {
        match self {
            Self::Edit(_) => true,
            Self::Clipboard(_) => true,
            Self::App(app) => !matches!(
                app,
                AppCommand::New
                    | AppCommand::Open(_)
                    | AppCommand::Palette
                    | AppCommand::Units(_)
                    | AppCommand::CloseTab(_)
                    | AppCommand::NextTab
                    | AppCommand::PreviousTab
                    | AppCommand::Tab(_)
                    | AppCommand::Tutorial
                    | AppCommand::Shortcuts
                    | AppCommand::About
            ),
        }
    }
}

/// One row of the table. `short` is a tile's caption where the tile sits under a heading that says the
/// rest ("Top" under Align).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandInfo {
    pub label: &'static str,
    pub short: Option<&'static str>,
    pub icon: Option<Icon>,
    pub tip: &'static str,
    pub shortcut: Option<KeyboardShortcut>,
}

const fn keys(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);
const CMD_ALT_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT).plus(Modifiers::SHIFT);
const SHIFT: Modifiers = Modifiers::SHIFT;
const ALT: Modifiers = Modifiers::ALT;
const ALT_SHIFT: Modifiers = Modifiers::ALT.plus(Modifiers::SHIFT);
const CTRL: Modifiers = Modifiers::CTRL;
const CTRL_SHIFT: Modifiers = Modifiers::CTRL.plus(Modifiers::SHIFT);

const fn row(label: &'static str, icon: Option<Icon>, tip: &'static str, shortcut: Option<KeyboardShortcut>) -> CommandInfo {
    CommandInfo { label, short: None, icon, tip, shortcut }
}

const fn tile(label: &'static str, short: &'static str, icon: Icon, tip: &'static str) -> CommandInfo {
    CommandInfo { label, short: Some(short), icon: Some(icon), tip, shortcut: None }
}

/// The palette row and popover label for one snap target class, so "snap nodes" finds its switch.
fn snap_class_row(class: crate::snap::SnapClass) -> CommandInfo {
    use crate::snap::SnapClass::*;
    let (label, tip) = match class {
        Grid => ("Snap grid", "Snap to the grid lines."),
        ShapeEdges => ("Snap shape edges", "Snap to other objects' edges and centres."),
        Mat => ("Snap mat", "Snap to the mat's edges, centre and corners."),
        Guides => ("Snap guides", "Snap to the ruler guides."),
        Nodes => ("Snap nodes", "Snap to other objects' nodes and segment midpoints."),
        Intersections => ("Snap intersections", "Snap to where two objects' edges cross."),
        Spacing => ("Snap equal spacing", "Snap to an equal gap between two neighbours."),
    };
    row(label, None, tip, None)
}

/// The table. One arm per command, so a new variant does not compile until it has a row.
pub fn info(command: &Command) -> CommandInfo {
    match command {
        Command::Edit(edit) => edit_info(edit),
        Command::App(app) => app_info(app),
        Command::Clipboard(action) => clipboard_info(*action),
    }
}

fn clipboard_info(action: super::clipboard::Action) -> CommandInfo {
    use super::clipboard::Action;
    match action {
        Action::Cut => row("Cut", None, "Copy the selection to the clipboard and remove it.", keys(CMD, Key::X)),
        Action::Copy => row("Copy", None, "Copy the selection to the clipboard.", keys(CMD, Key::C)),
        Action::CopyAsImage => row("Copy as PNG", None, "Copy the selection to the clipboard as a picture, for pasting into a chat or a listing.", None),
        Action::Paste => row("Paste", None, "Add the clipboard contents to the canvas.", keys(CMD, Key::V)),
        Action::PasteInPlace => row("Paste in Place", None, "Paste the clipboard contents at their original position.", keys(CMD_SHIFT, Key::V)),
        Action::PasteStyle => row("Paste Style", None, "Apply the first copied shape's operation, colour, pen and pattern.", keys(CMD_ALT, Key::V)),
    }
}

fn edit_info(edit: &Edit) -> CommandInfo {
    match edit {
        Edit::Delete => row("Delete", Some(Icon::Trash), "Remove the selection.", keys(Modifiers::NONE, Key::Delete)),
        Edit::Duplicate => row("Duplicate", Some(Icon::Duplicate), "Copy the selection.", keys(CMD, Key::D)),
        Edit::SelectAll => row("Select All", None, "Select every object on the canvas except locked layers.", keys(CMD, Key::A)),
        Edit::SelectSame(m) => row(m.label(), None, "Select every visible, unlocked object that matches the selection.", None),
        Edit::SelectInverse => row("Select Inverse", None, "Select everything that is not selected.", keys(CMD_SHIFT, Key::I)),
        Edit::SelectNext => row("Select Next", None, "Select the next object in Layers order.", keys(Modifiers::NONE, Key::Tab)),
        Edit::SelectPrevious => row("Select Previous", None, "Select the previous object in Layers order.", keys(SHIFT, Key::Tab)),
        Edit::Deselect => row("Deselect", None, "Clear the selection.", None),
        Edit::Group => row("Group", Some(Icon::Group), "Select and move the selection as one.", keys(CMD, Key::G)),
        Edit::Ungroup => row("Ungroup", Some(Icon::Ungroup), "Split the group into its shapes.", keys(CMD_SHIFT, Key::G)),
        Edit::Attach => row("Attach", Some(Icon::Attach), "Keeps objects in place so your design cuts exactly the way it is arranged on the canvas.", keys(ALT, Key::A)),
        Edit::Detach => row("Detach", Some(Icon::Detach), "Lets attached objects move and cut on their own again.", keys(ALT_SHIFT, Key::A)),
        Edit::Contain => row(CONTAIN_ONE, None, "Hold objects in a container that aligns and lays them out.", keys(CMD_ALT, Key::G)),
        Edit::Uncontain => row("Remove container", None, "Turn the container back into a plain shape, keeping its objects where they are.", None),
        Edit::Arrange(Arrange::ToFront) => row("Move to Front", Some(Icon::Arrange), "Put the selection above everything beside it.", None),
        Edit::Arrange(Arrange::Forward) => row("Move Forward", Some(Icon::Arrange), "Move the selection up one place.", None),
        Edit::Arrange(Arrange::Backward) => row("Move Backward", Some(Icon::Arrange), "Move the selection down one place.", None),
        Edit::Arrange(Arrange::ToBack) => row("Move to Back", Some(Icon::Arrange), "Put the selection below everything beside it.", None),
        Edit::Flip(Axis::Horizontal) => row("Flip Horizontal", Some(Icon::Flip), "Mirror the selection left to right.", None),
        Edit::Flip(Axis::Vertical) => row("Flip Vertical", Some(Icon::Flip), "Mirror the selection top to bottom.", None),
        Edit::Align { op, .. } => align_info(*op),
        Edit::Combine(op) => combine_info(*op),
        Edit::BooleanOp(op) => boolean_op_info(*op),
        Edit::Flatten => row("Flatten", Some(Icon::Flatten), "Merge into one Print Then Cut image whose outline is cut.", None),
        Edit::Unflatten => row("Unflatten", Some(Icon::Unflatten), "Split a flattened image back into its original objects.", None),
        Edit::AddEffect { effect: Effect::Offset(_), .. } => row("Offset", Some(Icon::Offset), "Add an outline around the selection, editable later from Layers.", None),
        Edit::AddEffect { effect: Effect::Sticker(_), .. } => {
            row("Sticker", Some(Icon::Sticker), "Make the selection a Print Then Cut sticker on a white backing.", None)
        }
        Edit::AddEffect { effect: Effect::Warp(_), .. } => row("Warp", Some(Icon::Warp), "Bend the selection inside an envelope.", None),
        Edit::Effect { op: EffectEdit::Enable(true), .. } => row("Enable effect", None, "Turn the effect back on.", None),
        Edit::Effect { op: EffectEdit::Enable(false), .. } => row("Disable effect", None, "Turn the effect off without losing its settings.", None),
        Edit::Effect { op: EffectEdit::Remove, .. } => row("Remove effect", Some(Icon::Trash), "Take the effect off, restoring what it changed.", None),
        Edit::Effect { op: EffectEdit::MoveTo(_), .. } => row("Reorder effect", None, "Apply the effect earlier or later in the stack.", None),
        Edit::Effect { op: EffectEdit::Replace(_), .. } => row("Edit effect", None, "Change the effect's settings.", None),
        Edit::Expand => row("Expand", None, "Bake the selection's effects into plain shapes.", None),
        Edit::Warp(Some(_)) => row("Warp", Some(Icon::Warp), "Bend the selection inside an envelope.", None),
        Edit::Warp(None) => row("Remove Warp", Some(Icon::Warp), "Restore the original outline and size.", None),
        Edit::Curve(_) => row("Curve", Some(Icon::Curve), "Bend a line of text around a circle.", None),
        Edit::TextPath(TextPathEdit::Attach) => row("Text on path", Some(Icon::Curve), "Run the selected text along the selected shape's edge.", keys(CMD_ALT, Key::T)),
        Edit::TextPath(TextPathEdit::Remove) => row("Remove from path", Some(Icon::Curve), "Straighten the text, keeping its size and font.", None),
        Edit::TextPath(TextPathEdit::Side(_)) => row("Path side", None, "Flip the text to the other side of its path.", None),
        Edit::TextPath(TextPathEdit::Along(_)) => row("Path align", None, "Set where the text sits along its path.", None),
        Edit::TextPath(TextPathEdit::Offset(_)) => row("Path offset", None, "Slide the text along its path.", None),
        Edit::Text(_) => row("Format text", Some(Icon::Format), "Change the font, size, style or spacing of the selected text.", None),
        Edit::Corners(crate::corners::CornerOp::RoundAll { .. }) => {
            row("Round all corners", Some(Icon::Corner), "Round every corner of the selected shapes to the radius.", None)
        }
        Edit::Corners(_) => row("Corner", Some(Icon::Corner), "Round, chamfer or scoop the selected path corners.", None),
        Edit::ConvertToPaths => row("Convert to Paths", Some(Icon::Pen), "Turn text into paths you can edit node by node. Writing strokes stay open.", None),
        Edit::SetHidden(true) => row("Hide", None, "Hidden objects are left out of Make and the Project Preview.", None),
        Edit::SetHidden(false) => row("Show", None, "Hidden objects are left out of Make and the Project Preview.", None),
        Edit::Nudge { dx_in, dy_in, .. } => {
            let label = match (dx_in.total_cmp(&0.0), dy_in.total_cmp(&0.0)) {
                (std::cmp::Ordering::Less, _) => "Nudge Left",
                (std::cmp::Ordering::Greater, _) => "Nudge Right",
                (_, std::cmp::Ordering::Less) => "Nudge Up",
                _ => "Nudge Down",
            };
            row(label, None, "Move the selection an eighth of an inch, or an inch with Shift.", None)
        }
        Edit::Field { .. } => row("Set position, size or rotation", None, "Type a value for the selection.", None),
        Edit::Params(_) => row("Shape parameters", None, "Change a shape's points, corner radius, angle or other field.", None),
        Edit::TransformAgain { copy: false, .. } => {
            row("Transform Again", None, "Apply the last move, rotation, resize or skew to the selection again.", keys(CMD_SHIFT, Key::D))
        }
        Edit::TransformAgain { copy: true, .. } => row("Power Duplicate", Some(Icon::Duplicate), "Duplicate the selection, repeating the last duplicate's step.", None),
        Edit::StepRepeat(_) => row("Step and repeat", Some(Icon::Duplicate), "Make a row or grid of copies, grouped with the original.", None),
        Edit::RotateCopies(_) => row("Rotate copies", None, "Make a ring of copies about a pivot, as a group.", None),
        Edit::Pivot(Some(_)) => row("Set pivot", None, "Set the point the selection rotates about.", None),
        Edit::Pivot(None) => row("Reset pivot", None, "Rotate the selection about its centre again.", None),
        Edit::Skew { .. } => row("Skew", None, "Lean the selection along its X or Y axis.", None),
        Edit::Operation(_) => row("Operation", None, "How the machine makes the selection: cut, draw, print or guide.", None),
        Edit::OutlineStrokes(_) => row("Outline strokes", None, "Expand each imported stroke to a filled ring.", None),
        Edit::Colour(_) => row("Colour", None, "The selection's colour, which also picks its mat.", None),
        Edit::Pen(_) => row("Pen", None, "The pen a Pen layer draws with.", None),
        Edit::Insert(Insert::Shape(_)) => row("Insert shape", Some(Icon::Shapes), "Add this shape to the canvas.", None),
        Edit::Insert(Insert::Text(_)) => row("Add text", Some(Icon::Text), "Add a line of text in this font.", None),
        Edit::Insert(Insert::Monogram { .. }) => row("Insert monogram", Some(Icon::Monogram), "Add the monogram at the centre of the view.", None),
        Edit::Insert(Insert::Generated { .. }) => row("Insert design", Some(Icon::Effects), "Add the generated design to the canvas.", None),
        Edit::Nesting(_) => row("Optimise layout", None, "Pack every mat's pieces as tightly as their outlines allow.", None),
        Edit::Grid(_) => row("Set grid", None, "Change the canvas grid's spacing, subdivisions, origin or lines.", None),
        Edit::Snap(_) => row("Set snapping", None, "Change what the magnet snaps to and the Shift angle step.", None),
        Edit::GrainLock(true) => row("Lock grain", Some(Icon::Lock), "Optimise layout may only turn the selection upside down, keeping its grain.", None),
        Edit::GrainLock(false) => row("Unlock grain", Some(Icon::Unlock), "Let Optimise layout turn the selection any way.", None),
        Edit::Lock { lock: Lock::Aspect, on: true } => row("Lock aspect ratio", None, "Keep the selection's width and height in proportion.", None),
        Edit::Lock { lock: Lock::Aspect, on: false } => row("Unlock aspect ratio", None, "Let the selection's width and height change on their own.", None),
        Edit::Lock { lock: Lock::Position, on: true } => {
            row("Lock position", Some(Icon::Lock), "Stop the selection being moved, resized, rotated or flipped. It can still be recoloured.", None)
        }
        Edit::Lock { lock: Lock::Position, on: false } => {
            row("Unlock position", Some(Icon::Unlock), "Let the selection be moved, resized, rotated and flipped again.", None)
        }
        Edit::Lock { lock: Lock::Layer, on: true } => row("Lock layer", Some(Icon::Lock), "Stop the canvas selecting the selection. Layers can still select it.", None),
        Edit::Lock { lock: Lock::Layer, on: false } => row("Unlock layer", Some(Icon::Unlock), "Let the canvas select the selection again.", None),
        Edit::Place(_) => row("Place", None, "Add new objects, such as an uploaded image, to the canvas.", None),
        Edit::Paste { .. } => row("Paste", None, "Add the clipboard contents to the canvas.", None),
        Edit::PasteStyle(_) => row("Paste Style", None, "Apply the copied operation, colour, pen and pattern.", None),
        Edit::Pattern(Some(_)) => row("Pattern", None, "Fill the selected Print Then Cut shapes with an image or a repeating pattern.", None),
        Edit::Pattern(None) => row("Remove pattern", None, "Print the selection in its flat colour again.", None),
        Edit::MakeComponent => {
            row("Make component", None, "Turn the selection into a reusable component. Editing its master updates every instance.", keys(CMD_ALT, Key::K))
        }
        Edit::DetachInstance => row("Detach instance", None, "Turn the instance into plain shapes, no longer linked to its component.", keys(CMD_ALT_SHIFT, Key::K)),
        Edit::PlaceInstance { .. } => row("Place component", None, "Add an instance of this component to the canvas.", None),
        Edit::RenameComponent { .. } => row("Rename component", None, "Change the component's name.", None),
        Edit::DuplicateComponent(_) => row("Duplicate component", None, "Make a new component from a copy of this one's master.", None),
        Edit::DeleteComponent(_) => row("Delete component", Some(Icon::Trash), "Detach every instance of the component, then delete it.", None),
        Edit::Override { .. } => row("Override part", None, "Change one part of this instance only.", None),
        Edit::ResetOverrides => row("Reset overrides", None, "Make every part of the instance match its master again.", None),
        Edit::ResetPart(_) => row("Reset this part", None, "Make this part match its master again.", None),
        Edit::Rename(_) => row("Rename layer", None, "Give the layer a name of your own. A blank name gives it back its label.", None),
        Edit::Monogram { .. } => row("Update monogram", Some(Icon::Monogram), "Lay the monogram out again with these settings, in place.", None),
        Edit::Guide(g) => guide_info(*g),
        Edit::BuildShape { delete: false, .. } => row("Build shape", Some(Icon::Combine), "Unite the regions you drag across into one shape.", None),
        Edit::BuildShape { delete: true, .. } => row("Delete region", Some(Icon::Trash), "Delete the regions you drag across.", None),
        Edit::Knife { .. } => row("Knife", Some(Icon::Slice), "Cut the shapes the line crosses into pieces.", None),
    }
}

fn guide_info(edit: GuideEdit) -> CommandInfo {
    match edit {
        GuideEdit::Add { .. } => row("Add guide", None, "Place a ruler guide on the artboard.", None),
        GuideEdit::Move { .. } => row("Move guide", None, "Set the guide's position.", None),
        GuideEdit::SetLocked { locked: true, .. } => row("Lock guide", Some(Icon::Lock), "Stop the guide being dragged. Clicks pass through it.", None),
        GuideEdit::SetLocked { locked: false, .. } => row("Unlock guide", Some(Icon::Unlock), "Let the guide be dragged again.", None),
        GuideEdit::Delete(_) => row("Delete guide", Some(Icon::Trash), "Remove the guide.", None),
        GuideEdit::LockAll => row("Lock guides", None, "Stop every guide being dragged, so clicks reach the shapes beneath.", keys(CMD_ALT, Key::Semicolon)),
        GuideEdit::Clear => row("Clear guides", None, "Remove every ruler guide.", None),
        GuideEdit::Show => row("Show guides", None, "Show or hide the ruler guides. Hidden guides do not snap.", keys(CMD, Key::Semicolon)),
    }
}

fn align_info(op: Align) -> CommandInfo {
    match op {
        Align::Top => tile("Align Top", "Top", Icon::AlignTop, "Line up top edges."),
        Align::Left => tile("Align Left", "Left", Icon::AlignLeft, "Line up left edges."),
        Align::VCentre => tile("Align Vertical", "Vertical", Icon::AlignVCentre, "Line up middles on one horizontal line."),
        Align::HCentre => tile("Align Horizontal", "Horizontal", Icon::AlignHCentre, "Line up centres on one vertical line."),
        Align::Bottom => tile("Align Bottom", "Bottom", Icon::AlignBottom, "Line up bottom edges."),
        Align::Right => tile("Align Right", "Right", Icon::AlignRight, "Line up right edges."),
        Align::Centre => tile("Align Centre", "Centre", Icon::AlignCentre, "Stack centres on one point."),
        Align::DistributeV => tile("Distribute Vertical", "Vertical", Icon::DistributeV, "Space evenly from top to bottom."),
        Align::DistributeH => tile("Distribute Horizontal", "Horizontal", Icon::DistributeH, "Space evenly from left to right."),
    }
}

fn combine_info(op: Combine) -> CommandInfo {
    match op {
        Combine::Slice => row("Slice", Some(Icon::Slice), "Cut two overlapping objects into their separate and shared pieces.", None),
        Combine::Weld => row("Weld", Some(Icon::Weld), "Join into one shape, removing overlapping cut lines.", None),
        Combine::Unite => row("Unite", Some(Icon::Unite), "Combine into one shape, keeping every cut line.", None),
        Combine::Subtract => row("Subtract", Some(Icon::Subtract), "Cut the front objects out of the back one.", None),
        Combine::Intersect => row("Intersect", Some(Icon::Intersect), "Keep only the area where every object overlaps.", None),
        Combine::Exclude => row("Exclude", Some(Icon::Exclude), "Remove the areas where objects overlap.", None),
    }
}

fn boolean_op_info(op: Combine) -> CommandInfo {
    let tip = "Change how the combined group joins its objects, keeping them editable.";
    match op {
        Combine::Slice => row("Change to Slice", Some(Icon::Slice), tip, None),
        Combine::Weld => row("Change to Weld", Some(Icon::Weld), tip, None),
        Combine::Unite => row("Change to Unite", Some(Icon::Unite), tip, None),
        Combine::Subtract => row("Change to Subtract", Some(Icon::Subtract), tip, None),
        Combine::Intersect => row("Change to Intersect", Some(Icon::Intersect), tip, None),
        Combine::Exclude => row("Change to Exclude", Some(Icon::Exclude), tip, None),
    }
}

fn app_info(app: &AppCommand) -> CommandInfo {
    match app {
        AppCommand::New => row("New", None, "Start an empty canvas in a new tab.", keys(CMD, Key::T)),
        AppCommand::Home => row("Home", None, "Open your saved projects.", None),
        AppCommand::Save => row("Save", None, "Save the canvas as a project.", None),
        AppCommand::Export => CommandInfo { short: Some("Export"), ..row("Export...", None, "Write the design as SVG, DXF or PDF files.", keys(CMD, Key::E)) },
        AppCommand::PrepareForDesignSpace => row("Prepare for Design Space", None, "Write one SVG per mat, and a guide to uploading them, for Design Space.", None),
        AppCommand::Undo => row("Undo", None, "Undo the last change.", keys(CMD, Key::Z)),
        AppCommand::Redo => row("Redo", None, "Redo the change just undone.", keys(CMD_SHIFT, Key::Z)),
        AppCommand::Make => row("Make", None, "Send the design to a machine.", None),
        AppCommand::Preview => row("Preview", None, "See the mats the design makes.", None),
        AppCommand::Open(_) => row("Open project", None, "Open this saved project.", None),
        AppCommand::Palette => row("Command palette", None, "Find and run any command by name.", keys(CMD, Key::K)),
        AppCommand::ZoomIn => row("Zoom In", None, "Magnify the canvas, about the pointer when it is over the canvas.", keys(CMD, Key::Equals)),
        AppCommand::ZoomOut => row("Zoom Out", None, "See more of the canvas, about the pointer when it is over the canvas.", keys(CMD, Key::Minus)),
        AppCommand::ResetZoom => row("Actual size", None, "Show the canvas at 100 %.", keys(CMD, Key::Num1)),
        AppCommand::ZoomToFit => row("Zoom to fit", None, "Show the whole mat.", keys(CMD, Key::Num0)),
        AppCommand::ZoomToSelection => row("Zoom to selection", None, "Fill the view with the selection.", keys(CMD, Key::Num2)),
        AppCommand::ZoomToAll => row("Zoom to all", None, "Show every object on the canvas.", keys(CMD_SHIFT, Key::Num0)),
        AppCommand::ZoomTo(_) => row("Zoom to", None, "Show the canvas at this zoom level.", None),
        AppCommand::Units(Units::Imperial) => row("Use inches", None, "Show and type every length in inches.", None),
        AppCommand::Units(Units::Metric) => row("Use centimetres", None, "Show and type every length in centimetres.", None),
        AppCommand::Snapping => row("Magnetic snap", None, "Turn snapping to other objects on or off.", None),
        AppCommand::SnapClass(class) => snap_class_row(*class),
        AppCommand::SnapSettings => row("Snapping...", None, "Choose what the magnet snaps to and the angle Shift locks to.", None),
        AppCommand::PartialGrid => row("Partial grid", None, "Switch the canvas grid between full and partial.", None),
        AppCommand::GridSettings => row("Grid settings...", None, "Set the grid's spacing, subdivisions, origin and snapping.", keys(CMD, Key::Quote)),
        AppCommand::CutPreview => row("Cut preview", None, "Watch each mat's tool path in machine order, with its time and material use.", None),
        AppCommand::VersionHistory => row("Version history", None, "Browse, preview and restore earlier versions of this project.", None),
        AppCommand::Upload => row("Upload", Some(Icon::Upload), "Import an SVG, PNG, JPG or WebP file, or add an SVG font as a writing font.", None),
        AppCommand::EditMaster(_) => row("Edit master", None, "Open the component's master on its own. Every instance follows your changes.", None),
        AppCommand::LeaveMaster => row("Done", None, "Close the master and return to the canvas.", None),
        AppCommand::CloseTab(_) => row("Close tab", None, "Close the tab. Its unsaved changes are kept and offered when you open it again.", keys(CMD, Key::W)),
        AppCommand::NextTab => row("Next tab", None, "Show the tab to the right.", keys(CTRL, Key::Tab)),
        AppCommand::PreviousTab => row("Previous tab", None, "Show the tab to the left.", keys(CTRL_SHIFT, Key::Tab)),
        AppCommand::Tutorial => row("Interactive tutorial", None, "A guided tour of Kerf on a sample card. Your projects stay untouched.", None),
        AppCommand::Shortcuts => row("Keyboard shortcuts", None, "List every command that has a shortcut.", None),
        AppCommand::About => row("About Kerf", None, "The version of Kerf you are running.", None),
        AppCommand::AiSettings => row("Create AI settings", Some(Icon::Effects), "Choose who draws Create AI designs, your API key and the model.", None),
        AppCommand::Tab(i) => row("Go to tab", None, "Show the tab at this place in the tab bar.", TAB_KEYS.get(*i).and_then(|&k| keys(TAB_MODIFIERS, k))),
        AppCommand::Rename => row("Rename", None, "Type a new name for the selected layer in Layers.", keys(Modifiers::NONE, Key::F2)),
        AppCommand::EditMonogram => {
            row("Edit monogram", Some(Icon::Monogram), "Open the monogram in the Monogram panel to change its letters, style, frame or fonts.", None)
        }
        AppCommand::ReplaceColour => row("Replace colour...", None, "Swap one of the design's colours for another, across every shape that has it.", None),
        AppCommand::SavePaletteToProject => row("Save palette to project", None, "Copy your saved swatches into this project's own palette, kept with the file.", None),
        AppCommand::CopyPaletteToSettings => row("Copy palette to settings", None, "Copy this project's palette into your saved swatches, for every project.", None),
        AppCommand::StepRepeatPanel => {
            row("Step and repeat...", Some(Icon::Duplicate), "Make a row or grid of copies, with a live preview, grouped with the original.", None)
        }
        AppCommand::RotateCopiesPanel => row("Rotate copies...", None, "Make a ring of copies about the pivot, with a live preview, as a group.", None),
    }
}

const CONTAIN_ONE: &str = "Make container";
const CONTAIN_MANY: &str = "Wrap in container";

/// Cmd+Option+1 to Cmd+Option+9 go to the first nine tabs; Cmd+0, Cmd+1 and Cmd+2 are the zoom keys.
pub const TAB_MODIFIERS: Modifiers = CMD_ALT;
const TAB_KEYS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

/// Every command with a shortcut, in the order keys are matched: more modifiers first, so Cmd+Shift+G
/// is not read as Cmd+G. Backspace is Delete's second key.
pub const KEYED: [(Command, KeyboardShortcut); 49] = [
    (Command::App(AppCommand::PreviousTab), KeyboardShortcut::new(CTRL_SHIFT, Key::Tab)),
    (Command::App(AppCommand::NextTab), KeyboardShortcut::new(CTRL, Key::Tab)),
    (Command::Edit(Edit::SelectPrevious), KeyboardShortcut::new(SHIFT, Key::Tab)),
    (Command::Edit(Edit::SelectNext), KeyboardShortcut::new(Modifiers::NONE, Key::Tab)),
    (Command::Edit(Edit::SelectInverse), KeyboardShortcut::new(CMD_SHIFT, Key::I)),
    (Command::Edit(Edit::TextPath(TextPathEdit::Attach)), KeyboardShortcut::new(CMD_ALT, Key::T)),
    (Command::App(AppCommand::New), KeyboardShortcut::new(CMD, Key::T)),
    (Command::App(AppCommand::CloseTab(None)), KeyboardShortcut::new(CMD, Key::W)),
    (Command::App(AppCommand::Tab(0)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[0])),
    (Command::App(AppCommand::Tab(1)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[1])),
    (Command::App(AppCommand::Tab(2)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[2])),
    (Command::App(AppCommand::Tab(3)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[3])),
    (Command::App(AppCommand::Tab(4)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[4])),
    (Command::App(AppCommand::Tab(5)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[5])),
    (Command::App(AppCommand::Tab(6)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[6])),
    (Command::App(AppCommand::Tab(7)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[7])),
    (Command::App(AppCommand::Tab(8)), KeyboardShortcut::new(TAB_MODIFIERS, TAB_KEYS[8])),
    (Command::Edit(Edit::DetachInstance), KeyboardShortcut::new(CMD_ALT_SHIFT, Key::K)),
    (Command::Edit(Edit::MakeComponent), KeyboardShortcut::new(CMD_ALT, Key::K)),
    (Command::Clipboard(super::clipboard::Action::PasteStyle), KeyboardShortcut::new(CMD_ALT, Key::V)),
    (Command::App(AppCommand::Palette), KeyboardShortcut::new(CMD, Key::K)),
    (Command::App(AppCommand::Redo), KeyboardShortcut::new(CMD_SHIFT, Key::Z)),
    (Command::Clipboard(super::clipboard::Action::PasteInPlace), KeyboardShortcut::new(CMD_SHIFT, Key::V)),
    (Command::App(AppCommand::Undo), KeyboardShortcut::new(CMD, Key::Z)),
    (Command::App(AppCommand::Export), KeyboardShortcut::new(CMD, Key::E)),
    (Command::Clipboard(super::clipboard::Action::Cut), KeyboardShortcut::new(CMD, Key::X)),
    (Command::Clipboard(super::clipboard::Action::Copy), KeyboardShortcut::new(CMD, Key::C)),
    (Command::Clipboard(super::clipboard::Action::Paste), KeyboardShortcut::new(CMD, Key::V)),
    (Command::Edit(Edit::Contain), KeyboardShortcut::new(CMD_ALT, Key::G)),
    (Command::Edit(Edit::Guide(GuideEdit::LockAll)), KeyboardShortcut::new(CMD_ALT, Key::Semicolon)),
    (Command::Edit(Edit::Guide(GuideEdit::Show)), KeyboardShortcut::new(CMD, Key::Semicolon)),
    (Command::Edit(Edit::Ungroup), KeyboardShortcut::new(CMD_SHIFT, Key::G)),
    (Command::Edit(Edit::Group), KeyboardShortcut::new(CMD, Key::G)),
    (Command::Edit(Edit::Detach), KeyboardShortcut::new(ALT_SHIFT, Key::A)),
    (Command::Edit(Edit::Attach), KeyboardShortcut::new(ALT, Key::A)),
    (Command::Edit(Edit::SelectAll), KeyboardShortcut::new(CMD, Key::A)),
    (Command::Edit(Edit::Duplicate), KeyboardShortcut::new(CMD, Key::D)),
    (Command::App(AppCommand::GridSettings), KeyboardShortcut::new(CMD, Key::Quote)),
    (Command::Edit(Edit::Delete), KeyboardShortcut::new(Modifiers::NONE, Key::Delete)),
    (Command::Edit(Edit::Delete), KeyboardShortcut::new(Modifiers::NONE, Key::Backspace)),
    (Command::App(AppCommand::Rename), KeyboardShortcut::new(Modifiers::NONE, Key::F2)),
    (Command::App(AppCommand::Rename), KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
    (Command::App(AppCommand::ZoomToAll), KeyboardShortcut::new(CMD_SHIFT, Key::Num0)),
    (Command::App(AppCommand::ZoomToFit), KeyboardShortcut::new(CMD, Key::Num0)),
    (Command::App(AppCommand::ResetZoom), KeyboardShortcut::new(CMD, Key::Num1)),
    (Command::App(AppCommand::ZoomToSelection), KeyboardShortcut::new(CMD, Key::Num2)),
    (Command::App(AppCommand::ZoomIn), KeyboardShortcut::new(CMD, Key::Equals)),
    (Command::App(AppCommand::ZoomIn), KeyboardShortcut::new(CMD, Key::Plus)),
    (Command::App(AppCommand::ZoomOut), KeyboardShortcut::new(CMD, Key::Minus)),
];

/// Every command the palette lists under Commands, with the payload it runs with. Commands whose payload
/// the user picks (a shape, a font, a project) come from their own sources; see [`source`].
pub fn registry(board_in: (f32, f32)) -> Vec<Command> {
    use AppCommand as A;
    let app = [
        A::New,
        A::Home,
        A::Save,
        A::Upload,
        A::Export,
        A::PrepareForDesignSpace,
        A::Undo,
        A::Redo,
        A::Make,
        A::Preview,
        A::CutPreview,
        A::VersionHistory,
        A::ZoomIn,
        A::ZoomOut,
        A::ResetZoom,
        A::ZoomToFit,
        A::ZoomToSelection,
        A::ZoomToAll,
    ]
    .into_iter()
    .chain(super::view_nav::ZOOM_TO_PCT.map(A::ZoomTo))
    .chain([
        A::Units(Units::Imperial),
        A::Units(Units::Metric),
        A::Snapping,
        A::SnapSettings,
        A::PartialGrid,
        A::GridSettings,
        A::Tutorial,
        A::Shortcuts,
        A::About,
        A::AiSettings,
        A::Rename,
        A::EditMonogram,
        A::ReplaceColour,
        A::SavePaletteToProject,
        A::CopyPaletteToSettings,
        A::StepRepeatPanel,
        A::RotateCopiesPanel,
    ])
    .chain(crate::snap::SnapClass::ALL.map(A::SnapClass));
    let to = AlignTo::Selection;
    let aligns = [Align::Left, Align::HCentre, Align::Right, Align::Top, Align::VCentre, Align::Bottom, Align::Centre, Align::DistributeH, Align::DistributeV]
        .map(|op| Edit::Align { op, to });
    let ops = [Combine::Slice, Combine::Weld, Combine::Unite, Combine::Subtract, Combine::Intersect, Combine::Exclude];
    let combines = ops.map(Edit::Combine);
    let boolean_ops = ops.map(Edit::BooleanOp);
    let nudges = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)].map(|(dx, dy)| Edit::Nudge { dx_in: dx * NUDGE_IN, dy_in: dy * NUDGE_IN, board_in });
    let colours = crate::pens::PEN_COLOURS.map(|(_, rgb)| Edit::Colour(rgb));
    let edits = [
        Edit::Delete,
        Edit::Duplicate,
        Edit::SelectAll,
        Edit::SelectInverse,
        Edit::Deselect,
        TRANSFORM_AGAIN,
        Edit::Group,
        Edit::Ungroup,
        Edit::Attach,
        Edit::Detach,
        Edit::Contain,
        Edit::Uncontain,
        Edit::Arrange(Arrange::ToFront),
        Edit::Arrange(Arrange::Forward),
        Edit::Arrange(Arrange::Backward),
        Edit::Arrange(Arrange::ToBack),
        Edit::Flip(Axis::Horizontal),
        Edit::Flip(Axis::Vertical),
    ]
    .into_iter()
    .chain(crate::select::Match::MENU.map(Edit::SelectSame))
    .chain(combines)
    .chain(boolean_ops)
    .chain(aligns)
    .chain([
        Edit::Flatten,
        Edit::Unflatten,
        Edit::AddEffect { effect: Effect::Offset(Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None }), reach: Reach::Together },
        Edit::AddEffect { effect: Effect::Sticker(Sticker { border_in: 0.125, corners: Corners::Round }), reach: Reach::Together },
        Edit::Expand,
        Edit::ConvertToPaths,
        Edit::TextPath(TextPathEdit::Attach),
        Edit::TextPath(TextPathEdit::Remove),
        Edit::Warp(Some(Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false })),
        Edit::Warp(None),
        Edit::SetHidden(true),
        Edit::SetHidden(false),
        Edit::GrainLock(true),
        Edit::GrainLock(false),
        Edit::Pattern(None),
        Edit::MakeComponent,
        Edit::DetachInstance,
        Edit::Guide(GuideEdit::Show),
        Edit::Guide(GuideEdit::LockAll),
        Edit::Guide(GuideEdit::Clear),
    ])
    .chain([Lock::Aspect, Lock::Position, Lock::Layer].into_iter().flat_map(|lock| [true, false].map(|on| Edit::Lock { lock, on })))
    .chain(nudges)
    .chain(colours);
    let tabs = [A::CloseTab(None), A::NextTab, A::PreviousTab];
    let clipboard = [
        super::clipboard::Action::Cut,
        super::clipboard::Action::Copy,
        super::clipboard::Action::CopyAsImage,
        super::clipboard::Action::Paste,
        super::clipboard::Action::PasteInPlace,
        super::clipboard::Action::PasteStyle,
    ]
    .map(Command::Clipboard);
    app.into_iter().map(Command::App).chain(edits.map(Command::Edit)).chain(tabs.map(Command::App)).chain(clipboard).collect()
}

/// Where the palette finds a command. One arm per variant, so a new command has to choose; the
/// registry test holds the registry to it.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Registry,
    Shapes,
    Fonts,
    Projects,
    /// Needs a value typed or dragged in place (a field, a curve's diameter) or picked from a list its
    /// own surface shows, or is the palette itself.
    Unlisted,
}

#[cfg(test)]
pub fn source(command: &Command) -> Source {
    match command {
        Command::Edit(edit) => match edit {
            Edit::Insert(Insert::Shape(_)) => Source::Shapes,
            Edit::Insert(Insert::Text(_)) | Edit::Text(TextChange::Font(_)) => Source::Fonts,
            Edit::Text(_)
            | Edit::Curve(_)
            | Edit::TextPath(TextPathEdit::Side(_) | TextPathEdit::Along(_) | TextPathEdit::Offset(_))
            | Edit::Field { .. }
            | Edit::Params(_)
            | Edit::Corners(_)
            | Edit::Operation(_)
            | Edit::Pen(_)
            | Edit::Effect { .. }
            | Edit::Nesting(_)
            | Edit::Grid(_)
            | Edit::Snap(_)
            | Edit::Place(_)
            | Edit::Paste { .. }
            | Edit::PasteStyle(_)
            | Edit::Insert(Insert::Generated { .. })
            | Edit::Pattern(Some(_))
            | Edit::SelectNext
            | Edit::TransformAgain { copy: true, .. }
            | Edit::StepRepeat(_)
            | Edit::RotateCopies(_)
            | Edit::Pivot(_)
            | Edit::Skew { .. }
            | Edit::BuildShape { .. }
            | Edit::Knife { .. }
            | Edit::OutlineStrokes(_)
            | Edit::SelectPrevious => Source::Unlisted,
            Edit::PlaceInstance { .. }
            | Edit::RenameComponent { .. }
            | Edit::DuplicateComponent(_)
            | Edit::DeleteComponent(_)
            | Edit::Override { .. }
            | Edit::ResetOverrides
            | Edit::ResetPart(_)
            | Edit::Rename(_)
            | Edit::Insert(Insert::Monogram { .. })
            | Edit::Monogram { .. }
            | Edit::Guide(GuideEdit::Add { .. } | GuideEdit::Move { .. } | GuideEdit::SetLocked { .. } | GuideEdit::Delete(_)) => Source::Unlisted,
            Edit::Guide(GuideEdit::LockAll | GuideEdit::Clear | GuideEdit::Show) => Source::Registry,
            Edit::Delete
            | Edit::Duplicate
            | Edit::SelectAll
            | Edit::SelectInverse
            | Edit::Deselect
            | Edit::Group
            | Edit::Ungroup
            | Edit::Attach
            | Edit::Detach
            | Edit::Contain
            | Edit::Uncontain
            | Edit::Arrange(_)
            | Edit::Flip(_)
            | Edit::Align { .. }
            | Edit::Combine(_)
            | Edit::BooleanOp(_)
            | Edit::Flatten
            | Edit::Unflatten
            | Edit::AddEffect { .. }
            | Edit::Expand
            | Edit::ConvertToPaths
            | Edit::TextPath(TextPathEdit::Attach | TextPathEdit::Remove)
            | Edit::Warp(_)
            | Edit::SetHidden(_)
            | Edit::GrainLock(_)
            | Edit::Lock { .. }
            | Edit::Pattern(None)
            | Edit::MakeComponent
            | Edit::DetachInstance
            | Edit::Nudge { .. }
            | Edit::SelectSame(_)
            | Edit::TransformAgain { copy: false, .. }
            | Edit::Colour(_) => Source::Registry,
        },
        Command::App(app) => match app {
            AppCommand::Open(_) => Source::Projects,
            AppCommand::Palette | AppCommand::EditMaster(_) | AppCommand::LeaveMaster | AppCommand::Tab(_) => Source::Unlisted,
            AppCommand::New
            | AppCommand::Home
            | AppCommand::Save
            | AppCommand::Upload
            | AppCommand::Export
            | AppCommand::PrepareForDesignSpace
            | AppCommand::Undo
            | AppCommand::Redo
            | AppCommand::Make
            | AppCommand::Preview
            | AppCommand::CutPreview
            | AppCommand::VersionHistory
            | AppCommand::ZoomIn
            | AppCommand::ZoomOut
            | AppCommand::ResetZoom
            | AppCommand::ZoomToFit
            | AppCommand::ZoomToSelection
            | AppCommand::ZoomToAll
            | AppCommand::ZoomTo(_)
            | AppCommand::Units(_)
            | AppCommand::Snapping
            | AppCommand::SnapClass(_)
            | AppCommand::SnapSettings
            | AppCommand::PartialGrid
            | AppCommand::GridSettings
            | AppCommand::CloseTab(_)
            | AppCommand::Rename
            | AppCommand::EditMonogram
            | AppCommand::NextTab
            | AppCommand::PreviousTab
            | AppCommand::Tutorial
            | AppCommand::Shortcuts
            | AppCommand::About
            | AppCommand::AiSettings
            | AppCommand::ReplaceColour
            | AppCommand::SavePaletteToProject
            | AppCommand::CopyPaletteToSettings
            | AppCommand::StepRepeatPanel
            | AppCommand::RotateCopiesPanel => Source::Registry,
        },
        Command::Clipboard(_) => Source::Registry,
    }
}

/// Other words the palette finds a command by.
pub fn aliases(command: &Command) -> &'static [&'static str] {
    match command {
        Command::Edit(edit) => match edit {
            Edit::Delete => &["remove", "erase", "trash"],
            Edit::Duplicate => &["copy", "clone"],
            Edit::Combine(Combine::Weld) => &["join", "merge", "union"],
            Edit::Combine(Combine::Unite) => &["weld", "union", "merge"],
            Edit::Combine(Combine::Subtract) => &["minus", "difference", "punch"],
            Edit::Combine(Combine::Exclude) => &["xor"],
            Edit::Align { op: Align::Centre | Align::HCentre | Align::VCentre, .. } => &["center"],
            Edit::Flip(_) => &["mirror"],
            Edit::SetHidden(_) => &["visibility"],
            Edit::AddEffect { effect: Effect::Offset(_), .. } => &["outline", "contour"],
            Edit::Expand => &["bake", "outline", "flatten effects"],
            Edit::Warp(_) => &["bend", "distort"],
            Edit::Colour(_) => &["color", "fill"],
            Edit::Insert(Insert::Text(_)) | Edit::Text(TextChange::Font(_)) => &["typeface"],
            Edit::Guide(_) => &["ruler guides", "lines"],
            Edit::SelectSame(_) => &["match", "similar", "alike"],
            Edit::TransformAgain { copy: false, .. } => &["repeat", "again", "last transform"],
            _ => &[],
        },
        Command::App(app) => match app {
            AppCommand::Export => &["svg", "dxf", "pdf", "save as"],
            AppCommand::PrepareForDesignSpace => &["cricut", "upload", "export"],
            AppCommand::Make => &["cut", "machine"],
            AppCommand::Preview => &["mats"],
            AppCommand::ZoomIn | AppCommand::ZoomOut | AppCommand::ResetZoom | AppCommand::ZoomTo(_) => &["magnify", "zoom"],
            AppCommand::ZoomToFit | AppCommand::ZoomToAll => &["fit", "whole mat", "view all"],
            AppCommand::ZoomToSelection => &["fit selection", "magnify"],
            AppCommand::Units(Units::Imperial) => &["units", "imperial"],
            AppCommand::Units(Units::Metric) => &["units", "metric"],
            AppCommand::Snapping => &["snap", "magnet"],
            AppCommand::SnapSettings => &["snap", "snapping", "magnet"],
            AppCommand::SnapClass(_) => &["snap"],
            AppCommand::GridSettings => &["grid", "spacing", "subdivisions", "origin", "snap"],
            AppCommand::Tutorial => &["interactive tutorial", "tour", "help", "learn"],
            AppCommand::Shortcuts => &["keys", "hotkeys", "help"],
            AppCommand::About => &["version", "help"],
            AppCommand::AiSettings => &["openai", "anthropic", "claude", "openrouter", "ollama", "api key", "model"],
            AppCommand::EditMonogram => &["initials", "monogram maker"],
            AppCommand::StepRepeatPanel => &["array", "grid", "copies", "row"],
            AppCommand::RotateCopiesPanel => &["ring", "radial", "circular", "copies"],
            _ => &[],
        },
        Command::Clipboard(action) => match action {
            super::clipboard::Action::Cut => &["remove", "delete"],
            super::clipboard::Action::Copy => &["clipboard", "clone"],
            super::clipboard::Action::CopyAsImage => &["picture", "png", "image"],
            super::clipboard::Action::Paste => &["clipboard", "insert"],
            super::clipboard::Action::PasteInPlace => &["original position"],
            super::clipboard::Action::PasteStyle => &["format", "appearance"],
        },
    }
}

/// Whether an empty palette suggests the command while it can run: the commands a selection unlocks.
pub fn suggested(command: &Command) -> bool {
    matches!(
        command,
        Command::Edit(
            Edit::Group
                | Edit::Ungroup
                | Edit::Attach
                | Edit::Detach
                | Edit::Contain
                | Edit::Uncontain
                | Edit::Combine(_)
                | Edit::Align { .. }
                | Edit::Flatten
                | Edit::Unflatten
                | Edit::Warp(None)
                | Edit::SetHidden(false)
        )
    )
}

/// The name a palette row shows: the table's label, or the payload's name for a command that carries
/// one (a shape, a font, a colour).
pub fn title(command: &Command) -> Cow<'static, str> {
    match command {
        Command::Edit(Edit::Insert(Insert::Shape(kind))) => kind.label().into(),
        Command::Edit(Edit::Insert(Insert::Text(font)) | Edit::Text(TextChange::Font(font))) => font.label().into_owned().into(),
        Command::Edit(Edit::Colour(rgb)) => format!("Set colour to {}", colour_name(*rgb)).into(),
        Command::Edit(Edit::SelectSame(m)) => format!("Select Same: {}", m.label()).into(),
        Command::App(AppCommand::Tutorial) => "Start tutorial".into(),
        Command::App(AppCommand::ZoomTo(pct)) => format!("Zoom to {pct}%").into(),
        _ => info(command).label.into(),
    }
}

fn colour_name(rgb: Color32) -> String {
    crate::pens::PEN_COLOURS.iter().find(|(_, c)| *c == rgb).map_or_else(|| rgb.to_hex(), |(name, _)| (*name).to_owned())
}

/// Transform Again as a surface carries it. The step it repeats is the document's `DuplicateMemory`, which
/// no const can hold, so `Ui::run_edit` swaps this placeholder frame for the remembered one.
pub const TRANSFORM_AGAIN: Edit = Edit::TransformAgain {
    from: crate::transform::Frame { centre: (0.0, 0.0), width_in: 0.0, height_in: 0.0, rotation_deg: 0.0, skew_deg: (0.0, 0.0), flip: (false, false) },
    copy: false,
};

/// Select Same with Shift held adds its matches to the selection it started from (spec 50): puts the
/// ids of `before` that the edit dropped back in `selection`.
pub fn keep_selected(before: &[u64], selection: &mut Vec<u64>) {
    let added: Vec<u64> = selection.iter().copied().filter(|id| !before.contains(id)).collect();
    *selection = before.iter().copied().chain(added).collect();
}

/// The arrow keys nudge an eighth of an inch, or an inch with Shift.
const NUDGE_IN: f32 = 0.125;
const NUDGE_BIG_IN: f32 = 1.0;

/// Whether `command` fires while another tool or node editing owns the keyboard: the app commands, except
/// Rename, whose Enter finishes a pen path or a node edit there.
pub fn stays_live(command: &Command) -> bool {
    matches!(command, Command::App(a) if *a != AppCommand::Rename)
}

fn held_id() -> egui::Id {
    egui::Id::new("keyboard-held-at-frame-start")
}

/// Notes whether a field held the keyboard as this frame began, read before any widget runs. A field
/// gives the keyboard up on the Enter that commits it, and that Enter must not also start a rename.
pub fn frame_start(ctx: &Context) {
    let held = ctx.wants_keyboard_input();
    ctx.data_mut(|d| d.insert_temp(held_id(), held));
}

/// The command this frame's keys fire, if any. While `edits_live` is false (another tool or node editing
/// owns the keyboard) only the commands that `stays_live` fire. `board_in` bounds a nudge.
pub fn pressed(ctx: &Context, edits_live: bool, board_in: (f32, f32)) -> Option<Command> {
    if ctx.wants_keyboard_input() {
        return None;
    }
    let field_closed = ctx.data(|d| d.get_temp::<bool>(held_id())).unwrap_or(false);
    let live = |c: &Command, k: &KeyboardShortcut| (edits_live || stays_live(c)) && !(field_closed && k.logical_key == Key::Enter);
    let keyed = ctx.input_mut(|i| KEYED.iter().filter(|(c, k)| live(c, k)).find(|(_, k)| i.consume_shortcut(k)).map(|(c, _)| c.clone()));
    keyed.or_else(|| {
        let arrows = [(Key::ArrowLeft, (-1.0, 0.0)), (Key::ArrowRight, (1.0, 0.0)), (Key::ArrowUp, (0.0, -1.0)), (Key::ArrowDown, (0.0, 1.0))];
        let (dir, big) = ctx.input(|i| (arrows.iter().find(|(k, _)| i.key_pressed(*k)).map(|&(_, d)| d), i.modifiers.shift));
        let step = if big { NUDGE_BIG_IN } else { NUDGE_IN };
        dir.filter(|_| edits_live).map(|(dx, dy): (f32, f32)| Command::Edit(Edit::Nudge { dx_in: dx * step, dy_in: dy * step, board_in }))
    })
}

/// Ctrl+Tab and Ctrl+Shift+Tab, read before any widget runs. egui moves keyboard focus on Tab whatever
/// the modifiers, so by the time [`pressed`] runs a text field would hold the keyboard.
pub fn cycle_pressed(ctx: &Context) -> Option<Command> {
    if ctx.wants_keyboard_input() {
        return None;
    }
    ctx.input_mut(|i| KEYED.iter().filter(|(_, k)| k.logical_key == Key::Tab).find(|(_, k)| i.consume_shortcut(k)).map(|(c, _)| c.clone()))
}

/// A command resolved against this frame: its table row, its live label and why it cannot run.
#[derive(Clone, Debug)]
pub struct Entry {
    pub command: Command,
    pub label: &'static str,
    pub info: CommandInfo,
    pub blocked: Option<Blocked>,
}

impl Entry {
    pub fn enabled(&self) -> bool {
        self.blocked.is_none()
    }

    /// The edit this entry runs. Only for entries built from an `Edit`.
    pub fn edit(&self) -> &Edit {
        match &self.command {
            Command::Edit(e) => e,
            Command::App(_) => panic!("an app command has no edit"),
            Command::Clipboard(_) => panic!("a clipboard command has no edit"),
        }
    }

    /// The tooltip body: why it cannot run while disabled, else what it does.
    pub fn tip_body(&self) -> String {
        self.blocked.map_or_else(|| self.info.tip.to_owned(), Blocked::reason)
    }

    /// The shortcut as this platform prints it.
    pub fn keys(&self, ctx: &Context) -> Option<String> {
        self.info.shortcut.map(|k| ctx.format_shortcut(&k))
    }
}

/// What a surface resolves commands against: the design, the ids the commands act on and the Align To
/// choice.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub design: &'a Design,
    pub on: &'a [u64],
    pub align_to: AlignTo,
}

impl<'a> Cx<'a> {
    pub fn new(design: &'a Design, on: &'a [u64], align_to: AlignTo) -> Self {
        Self { design, on, align_to }
    }

    pub fn entry(&self, edit: Edit) -> Entry {
        let blocked = self.design.check(&edit, self.on).err();
        let label = match edit {
            Edit::Contain if self.design.contain_wraps(self.on) => CONTAIN_MANY,
            _ => edit_info(&edit).label,
        };
        let command = Command::Edit(edit);
        Entry { info: info(&command), command, label, blocked }
    }

    /// `b` when it can run, else `a`: Group or Ungroup, Attach or Detach, Flatten or Unflatten.
    pub fn either(&self, a: Edit, b: Edit) -> Entry {
        let b = self.entry(b);
        if b.enabled() { b } else { self.entry(a) }
    }

    /// Align `op` against the target the Align To choice resolves to for these ids.
    pub fn align(&self, op: Align) -> Edit {
        Edit::Align { op, to: self.design.align_target(self.on, self.align_to) }
    }

    pub fn app(&self, app: AppCommand, history: &History) -> Entry {
        let blocked = app.check(self.design, history, self.on).err();
        let command = Command::App(app);
        let info = info(&command);
        Entry { blocked, label: info.label, info, command }
    }

    pub fn clipboard(&self, action: super::clipboard::Action) -> Entry {
        let blocked = matches!(
            action,
            super::clipboard::Action::Cut | super::clipboard::Action::Copy | super::clipboard::Action::CopyAsImage | super::clipboard::Action::PasteStyle
        )
        .then_some(Blocked::NothingSelected)
        .filter(|_| self.on.is_empty());
        let command = Command::Clipboard(action);
        let info = info(&command);
        Entry { blocked, label: info.label, info, command }
    }

    /// Any command resolved for this frame, Align against the Align To choice.
    pub fn command(&self, command: &Command, history: &History) -> Entry {
        match command {
            Command::Edit(Edit::Align { op, .. }) => self.entry(self.align(*op)),
            Command::Edit(edit) => self.entry(edit.clone()),
            Command::App(app) => self.app(app.clone(), history),
            Command::Clipboard(action) => self.clipboard(*action),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyed_command_shows_the_shortcut_it_answers_to() {
        for (command, _) in &KEYED {
            let shown = info(command).shortcut.expect("a keyed command shows its shortcut");
            assert!(KEYED.contains(&(command.clone(), shown)), "{command:?} shows {shown:?} but does not answer it");
        }
    }

    /// Builds one sample per listed pattern and, from the same patterns, a `match` over the whole enum,
    /// so a variant added to `Edit`, `AppCommand` or `Action` without a line here does not compile.
    /// `skipped` lists variants whose payload a test cannot build; they stay out of the sample.
    macro_rules! samples {
        ($ty:ty; $($pat:pat => $sample:expr,)+ ; skipped: $($skip:pat),* $(,)?) => {{
            fn exhaustive(v: &$ty) {
                match v {
                    $($pat => {})+
                    $($skip => {})*
                }
            }
            vec![$({
                let sample: $ty = $sample;
                exhaustive(&sample);
                assert!(matches!(&sample, $pat), "{} sampled as {sample:?}", stringify!($pat));
                sample
            }),+]
        }};
    }

    /// One command of every variant of `Edit`, `AppCommand` and `Action`, bar the few `skipped` below.
    fn every_variant() -> Vec<Command> {
        use crate::design::{Operation, ShapeKind};
        use crate::transform::Field;
        let board_in = (12.0, 12.0);
        let edits = samples! { Edit;
            Edit::Delete => Edit::Delete,
            Edit::Duplicate => Edit::Duplicate,
            Edit::SelectAll => Edit::SelectAll,
            Edit::SelectSame(_) => Edit::SelectSame(crate::select::Match::Colour),
            Edit::SelectInverse => Edit::SelectInverse,
            Edit::SelectNext => Edit::SelectNext,
            Edit::SelectPrevious => Edit::SelectPrevious,
            Edit::Deselect => Edit::Deselect,
            Edit::Group => Edit::Group,
            Edit::Ungroup => Edit::Ungroup,
            Edit::Attach => Edit::Attach,
            Edit::Detach => Edit::Detach,
            Edit::Contain => Edit::Contain,
            Edit::Uncontain => Edit::Uncontain,
            Edit::Arrange(_) => Edit::Arrange(Arrange::ToFront),
            Edit::Flip(_) => Edit::Flip(Axis::Horizontal),
            Edit::Align { .. } => Edit::Align { op: Align::Left, to: AlignTo::Selection },
            Edit::Combine(_) => Edit::Combine(Combine::Weld),
            Edit::BooleanOp(_) => Edit::BooleanOp(Combine::Subtract),
            Edit::Flatten => Edit::Flatten,
            Edit::Unflatten => Edit::Unflatten,
            Edit::AddEffect { .. } => Edit::AddEffect {
                effect: Effect::Offset(Offset {
                    distance_in: 0.1,
                    direction: Direction::Outset,
                    join: Join::Miter { limit: crate::geometry::DEFAULT_MITER_LIMIT },
                    style: None,
                }),
                reach: Reach::EachShape,
            },
            Edit::Effect { .. } => Edit::Effect { at: crate::effects::EntryRef { shape: 1, row: crate::effects::Row::Curve }, op: EffectEdit::Remove },
            Edit::Expand => Edit::Expand,
            Edit::Warp(_) => Edit::Warp(None),
            Edit::Curve(_) => Edit::Curve(4.0),
            Edit::TextPath(_) => Edit::TextPath(TextPathEdit::Attach),
            Edit::Text(_) => Edit::Text(TextChange::SizePt(12.0)),
            Edit::Params(_) => Edit::Params(crate::parametric::ParamOp::Corner { at: 0, value: crate::expr::Typed::To(0.1) }),
            Edit::Corners(_) => Edit::Corners(crate::corners::CornerOp::RoundAll { radius_in: 0.1 }),
            Edit::ConvertToPaths => Edit::ConvertToPaths,
            Edit::SetHidden(_) => Edit::SetHidden(true),
            Edit::Nudge { .. } => Edit::Nudge { dx_in: 1.0, dy_in: 0.0, board_in },
            Edit::Field { .. } => Edit::Field { field: Field::X, value: crate::expr::Typed::To(1.0), board_in },
            Edit::TransformAgain { .. } => TRANSFORM_AGAIN,
            Edit::StepRepeat(_) => Edit::StepRepeat(crate::repeat::StepRepeat { copies: 2, offset_in: (1.0, 0.0), rotation_deg: 0.0, scale: 1.0, grid: None }),
            Edit::RotateCopies(_) => Edit::RotateCopies(crate::repeat::RotateCopies { copies: 4, total_deg: 360.0 }),
            Edit::Pivot(_) => Edit::Pivot(None),
            Edit::Skew { .. } => Edit::Skew { axis: Axis::Horizontal, value: crate::expr::Typed::To(10.0) },
            Edit::Operation(_) => Edit::Operation(Operation::Pen),
            Edit::OutlineStrokes(_) => Edit::OutlineStrokes(Vec::new()),
            Edit::Colour(_) => Edit::Colour(Color32::RED),
            Edit::Pen(_) => Edit::Pen(crate::pens::PenPick::Colour(1)),
            Edit::Insert(_) => Edit::Insert(Insert::Shape(ShapeKind::Heart)),
            Edit::Nesting(_) => Edit::Nesting(None),
            Edit::GrainLock(_) => Edit::GrainLock(true),
            Edit::Lock { .. } => Edit::Lock { lock: crate::edit::Lock::Layer, on: true },
            Edit::PasteStyle(_) => Edit::PasteStyle(crate::clipboard::PasteStyle { operation: Operation::BasicCut, fill: Color32::BLACK, pen: Default::default(), pattern: None }),
            Edit::Pattern(_) => Edit::Pattern(None),
            Edit::MakeComponent => Edit::MakeComponent,
            Edit::DetachInstance => Edit::DetachInstance,
            Edit::PlaceInstance { .. } => Edit::PlaceInstance { component: 1, at: None },
            Edit::RenameComponent { .. } => Edit::RenameComponent { component: 1, name: "c".into() },
            Edit::DuplicateComponent(_) => Edit::DuplicateComponent(1),
            Edit::DeleteComponent(_) => Edit::DeleteComponent(1),
            Edit::Override { .. } => Edit::Override { part: 1, change: crate::components::PartChange::Hidden(true) },
            Edit::ResetOverrides => Edit::ResetOverrides,
            Edit::ResetPart(_) => Edit::ResetPart(1),
            Edit::Rename(_) => Edit::Rename(None),
            Edit::Grid(_) => Edit::Grid(crate::grid::GridSettings::for_units(Units::Imperial)),
            Edit::Guide(_) => Edit::Guide(GuideEdit::Show),
            Edit::Snap(_) => Edit::Snap(crate::snap::SnapOptions::default()),
            Edit::BuildShape { .. } => Edit::BuildShape { picks: Vec::new(), delete: false },
            Edit::Knife { .. } => Edit::Knife { line: Vec::new() },
            ; skipped: Edit::Place(_), Edit::Paste { .. }, Edit::Monogram { .. }
        };
        let apps = samples! { AppCommand;
            AppCommand::New => AppCommand::New,
            AppCommand::Home => AppCommand::Home,
            AppCommand::Save => AppCommand::Save,
            AppCommand::Export => AppCommand::Export,
            AppCommand::PrepareForDesignSpace => AppCommand::PrepareForDesignSpace,
            AppCommand::Undo => AppCommand::Undo,
            AppCommand::Redo => AppCommand::Redo,
            AppCommand::Make => AppCommand::Make,
            AppCommand::Preview => AppCommand::Preview,
            AppCommand::Open(_) => AppCommand::Open("p".into()),
            AppCommand::Palette => AppCommand::Palette,
            AppCommand::ZoomIn => AppCommand::ZoomIn,
            AppCommand::ZoomOut => AppCommand::ZoomOut,
            AppCommand::ResetZoom => AppCommand::ResetZoom,
            AppCommand::ZoomToFit => AppCommand::ZoomToFit,
            AppCommand::ZoomToSelection => AppCommand::ZoomToSelection,
            AppCommand::ZoomToAll => AppCommand::ZoomToAll,
            AppCommand::ZoomTo(_) => AppCommand::ZoomTo(200),
            AppCommand::Units(_) => AppCommand::Units(Units::Metric),
            AppCommand::Snapping => AppCommand::Snapping,
            AppCommand::SnapClass(_) => AppCommand::SnapClass(crate::snap::SnapClass::Nodes),
            AppCommand::SnapSettings => AppCommand::SnapSettings,
            AppCommand::PartialGrid => AppCommand::PartialGrid,
            AppCommand::GridSettings => AppCommand::GridSettings,
            AppCommand::CutPreview => AppCommand::CutPreview,
            AppCommand::VersionHistory => AppCommand::VersionHistory,
            AppCommand::Upload => AppCommand::Upload,
            AppCommand::EditMaster(_) => AppCommand::EditMaster(1),
            AppCommand::LeaveMaster => AppCommand::LeaveMaster,
            AppCommand::CloseTab(_) => AppCommand::CloseTab(None),
            AppCommand::NextTab => AppCommand::NextTab,
            AppCommand::PreviousTab => AppCommand::PreviousTab,
            AppCommand::Tab(_) => AppCommand::Tab(0),
            AppCommand::Tutorial => AppCommand::Tutorial,
            AppCommand::Shortcuts => AppCommand::Shortcuts,
            AppCommand::About => AppCommand::About,
            AppCommand::AiSettings => AppCommand::AiSettings,
            AppCommand::Rename => AppCommand::Rename,
            AppCommand::EditMonogram => AppCommand::EditMonogram,
            AppCommand::ReplaceColour => AppCommand::ReplaceColour,
            AppCommand::StepRepeatPanel => AppCommand::StepRepeatPanel,
            AppCommand::RotateCopiesPanel => AppCommand::RotateCopiesPanel,
            AppCommand::SavePaletteToProject => AppCommand::SavePaletteToProject,
            AppCommand::CopyPaletteToSettings => AppCommand::CopyPaletteToSettings,
            ; skipped:
        };
        let clipboard = samples! { crate::ui::clipboard::Action;
            crate::ui::clipboard::Action::Cut => crate::ui::clipboard::Action::Cut,
            crate::ui::clipboard::Action::Copy => crate::ui::clipboard::Action::Copy,
            crate::ui::clipboard::Action::CopyAsImage => crate::ui::clipboard::Action::CopyAsImage,
            crate::ui::clipboard::Action::Paste => crate::ui::clipboard::Action::Paste,
            crate::ui::clipboard::Action::PasteInPlace => crate::ui::clipboard::Action::PasteInPlace,
            crate::ui::clipboard::Action::PasteStyle => crate::ui::clipboard::Action::PasteStyle,
            ; skipped:
        };
        edits.into_iter().map(Command::Edit).chain(apps.into_iter().map(Command::App)).chain(clipboard.into_iter().map(Command::Clipboard)).collect()
    }

    type Kind = (Option<std::mem::Discriminant<Edit>>, Option<std::mem::Discriminant<AppCommand>>, Option<std::mem::Discriminant<crate::ui::clipboard::Action>>);

    fn kind(c: &Command) -> Kind {
        match c {
            Command::Edit(e) => (Some(std::mem::discriminant(e)), None, None),
            Command::App(a) => (None, Some(std::mem::discriminant(a)), None),
            Command::Clipboard(a) => (None, None, Some(std::mem::discriminant(a))),
        }
    }

    #[test]
    fn the_registry_holds_every_variant_its_source_says_it_lists() {
        let registry = registry((12.0, 12.0));
        for command in &registry {
            assert_eq!(source(command), Source::Registry, "{command:?}");
        }
        for command in every_variant().iter().filter(|c| source(c) == Source::Registry) {
            assert!(registry.iter().any(|r| kind(r) == kind(command)), "{command:?} is listed but missing from the registry");
        }
    }

    #[test]
    fn registry_rows_have_distinct_titles() {
        let titles: Vec<_> = registry((12.0, 12.0)).iter().map(title).collect();
        for (i, t) in titles.iter().enumerate() {
            assert!(!titles[i + 1..].contains(t), "{t} appears twice");
        }
    }

    #[test]
    fn more_modifiers_are_matched_first() {
        for (i, (_, a)) in KEYED.iter().enumerate() {
            for (_, b) in &KEYED[i + 1..] {
                let shadowed = a.logical_key == b.logical_key && b.modifiers.matches_logically(a.modifiers) && a.modifiers != b.modifiers;
                assert!(!shadowed, "{b:?} comes after {a:?}, which it would also fire");
            }
        }
    }
}

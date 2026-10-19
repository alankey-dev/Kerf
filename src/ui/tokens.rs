//! The single source of truth for Kerf's palette and metrics.
//!
//! Kerf is a modern monolithic editor with a violet accent on flat white panels separated by 1px
//! low-contrast borders, and the document floating as a sheet over a light void. White text on
//! ACCENT_PRIMARY is 5.6:1, TEXT_PRIMARY on white is 15.7:1, both passing AA; disabled labels use a
//! 3.8:1 grey.

use egui::Color32;

// The spec palette: the whole UI draws from these names and no others.
pub const BG_PANEL: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const BG_CANVAS_VOID: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf5);
pub const BG_ARTBOARD: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0xe9, 0xec, 0xef);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0x21, 0x23, 0x29);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x6c, 0x75, 0x7d);
pub const ACCENT_PRIMARY: Color32 = Color32::from_rgb(0x70, 0x48, 0xe8);
/// Hover and active tint behind controls, and selected-row background.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xf3, 0xf0, 0xff);
/// Pressed or active accent, a shade darker than ACCENT_PRIMARY.
pub const PRIMARY_HOVER: Color32 = Color32::from_rgb(0x5f, 0x3d, 0xc4);
/// Foreground for text and icons on any ACCENT_PRIMARY background.
pub const ON_PRIMARY: Color32 = Color32::WHITE;
/// Dim but legible (3.8:1 on white) for disabled interactive labels.
pub const DISABLED_TEXT: Color32 = Color32::from_rgb(0x86, 0x80, 0x89);
/// Artboard drop shadow, rgba(0,0,0,0.05).
pub const ARTBOARD_SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 13);
/// Menu and popover drop shadow.
pub const SHADOW: Color32 = Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x1f);
/// The tutorial's dim layer over everything but its target: black at 60 %.
pub const TUTORIAL_DIM: Color32 = Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x99);

/// Warning notices: 5.9:1 amber ink on a pale amber wash.
pub const WARNING_TEXT: Color32 = Color32::from_rgb(0x8a, 0x53, 0x00);
pub const WARNING_BG: Color32 = Color32::from_rgb(0xff, 0xf6, 0xe0);
/// An entry the app cannot use, such as a field expression that does not parse.
pub const ERROR_EDGE: Color32 = Color32::from_rgb(0xd6, 0x33, 0x3a);
pub const ERROR_BG: Color32 = Color32::from_rgb(0xfd, 0xec, 0xed);

pub const MAT_GREY: Color32 = Color32::from_rgb(0xad, 0xb5, 0xbd);
pub const RULER_TICK: Color32 = Color32::from_rgb(0xce, 0xd4, 0xda);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(0xde, 0xe2, 0xe6);
pub const GRID_MINOR: Color32 = Color32::from_rgb(0xed, 0xf0, 0xf2);

// Layout.
pub const TOP_BAR_H: f32 = 40.0;
/// Room for the macOS traffic lights on the left of the top bar.
pub const TRAFFIC_LIGHT_PAD: f32 = 78.0;
pub const RAIL_W: f32 = 48.0;
pub const INSPECTOR_W: f32 = 240.0;
pub const RULER: f32 = 24.0;
pub const SHAPES_PANEL_W: f32 = 336.0;
pub const BUTTON_H: f32 = 28.0;
pub const BUTTON_MIN_W: f32 = 72.0;
// Retained for the Make flow screens, which keep their own chrome.
pub const PREVIEW_PANEL_W: f32 = 312.0;
pub const TOOLBAR_H: f32 = 40.0;

//! Open fonts for this build: Inter for body text, Inter Display for headings.
//! Both are the SIL OFL release from rsms/inter (see `assets/fonts/OFL.txt`).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId};

macro_rules! font {
    ($file:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/", $file))
    };
}

const MEDIUM: &str = "inter-medium";
const SEMIBOLD: &str = "inter-semibold";
const DISPLAY: &str = "inter-display";
const SPACE_GROTESK: &str = "space-grotesk";

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 5] = [
        ("inter-regular", font!("Inter-Regular.ttf")),
        (MEDIUM, font!("Inter-Medium.ttf")),
        (SEMIBOLD, font!("Inter-SemiBold.ttf")),
        (DISPLAY, font!("InterDisplay-SemiBold.ttf")),
        (SPACE_GROTESK, font!("SpaceGrotesk.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "inter-regular".to_owned());
    for name in [MEDIUM, SEMIBOLD, DISPLAY, SPACE_GROTESK] {
        let mut chain = vec![name.to_owned()];
        chain.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    ctx.set_fonts(fonts);
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// A text font's own face, for previews such as the toolbar's font menu. A system family uses the UI
/// font until its face has loaded. A writing font has no face egui can draw, so it uses the UI font.
pub fn typeface(ctx: &egui::Context, font: &crate::text::FontRef, size: f32) -> FontId {
    match font {
        crate::text::FontRef::Inter | crate::text::FontRef::Stroke(_) => regular(size),
        crate::text::FontRef::SpaceGrotesk => FontId::new(size, FontFamily::Name(SPACE_GROTESK.into())),
        crate::text::FontRef::System(family) => super::font_preview::family(ctx, family).map_or_else(|| regular(size), |f| FontId::new(size, f)),
    }
}

use serde::Serialize;

proto_enum! {
    /// Tool types.
    pub enum Tool {
        RollingBlade = 1 => "ROLLING_BLADE_TT",
        DragKnife = 2 => "DRAG_KNIFE_TT",
        TangentialBlade = 3 => "TANGENTIAL_BLADE_TT",
        Pen = 4 => "PEN_DT_TT",
        Stylus = 5 => "STYLUS_TT",
        ScoringWheel = 6 => "SCORING_WHEEL_TT",
        PerforationWheel = 9 => "PERFORATION_WHEEL_TT",
        Debossing = 10 => "DEBOSSING_TOOL_TT",
        Wave = 11 => "WAVE_TOOL_TT",
        Engrave = 12 => "ENGRAVE_TOOL_TT",
        MidasFine = 13 => "MIDAS_FINE_TOOL_TT",
        MidasRegular = 14 => "MIDAS_REGULAR_TOOL_TT",
        MidasBold = 15 => "MIDAS_BOLD_TOOL_TT",
    }
}

impl Tool {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::RollingBlade => "Rotary Blade",
            Self::DragKnife => "Fine-Point Blade",
            Self::TangentialBlade => "Knife Blade",
            Self::Pen => "Pen",
            Self::Stylus => "Scoring Stylus",
            Self::ScoringWheel => "Scoring Wheel",
            Self::PerforationWheel => "Perforation Blade",
            Self::Debossing => "Debossing Tip",
            Self::Wave => "Wavy Blade",
            Self::Engrave => "Engraving Tip",
            Self::MidasFine => "Foil Transfer Tool, Fine Tip",
            Self::MidasRegular => "Foil Transfer Tool, Medium Tip",
            Self::MidasBold => "Foil Transfer Tool, Bold Tip",
        }
    }
}

#[allow(dead_code, reason = "the Pen Holder belongs to machines this app does not simulate")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Clamp {
    A,
    B,
    PenHolder,
}

impl Clamp {
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "Clamp A",
            Self::B => "Clamp B",
            Self::PenHolder => "Pen Holder",
        }
    }
}

/// Renderer enum `ig`: LESS=-1, DEFAULT=0, MORE=1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PressureAdjust {
    Less = -1,
    #[default]
    Default = 0,
    More = 1,
}

impl PressureAdjust {
    pub const ALL: [PressureAdjust; 3] = [Self::Default, Self::More, Self::Less];

    pub fn label(self) -> &'static str {
        match self {
            Self::Less => "Less",
            Self::Default => "Default",
            Self::More => "More",
        }
    }

    /// MORE adds `delta[2]`, LESS adds `delta[0]`, the default leaves the pressure unchanged.
    pub fn apply(self, pressure: i32, delta: [i32; 3]) -> i32 {
        match self {
            Self::More => pressure + delta[2],
            Self::Less => pressure + delta[0],
            Self::Default => pressure,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Material {
    pub name: &'static str,
    pub pressure: i32,
    pub multi_pass: u32,
    pub cut_speed: u32,
    pub delta_adjustment: [i32; 3],
    pub tool: Tool,
    pub clamp: Clamp,
}

impl Material {
    pub fn adjusted_pressure(&self, adjust: PressureAdjust) -> i32 {
        adjust.apply(self.pressure, self.delta_adjustment)
    }
}

const fn fine_point(name: &'static str, pressure: i32, multi_pass: u32, cut_speed: u32, delta: i32) -> Material {
    Material { name, pressure, multi_pass, cut_speed, delta_adjustment: [-delta, 0, delta], tool: Tool::DragKnife, clamp: Clamp::B }
}

const fn knife(name: &'static str, pressure: i32, multi_pass: u32, delta: i32) -> Material {
    Material { name, pressure, multi_pass, cut_speed: 3, delta_adjustment: [-delta, 0, delta], tool: Tool::TangentialBlade, clamp: Clamp::B }
}

/// Plausible stand-ins. Kerf ships no measured material data; real values would come from the
/// connected machine's own material list.
pub static MATERIALS: [Material; 9] = [
    fine_point("Cardstock", 300, 2, 7, 20),
    fine_point("Vinyl", 140, 1, 10, 10),
    fine_point("Iron-On", 155, 1, 10, 10),
    fine_point("Light Cardstock", 230, 1, 8, 15),
    fine_point("Poster Board", 330, 2, 6, 25),
    knife("Chipboard", 1500, 9, 100),
    fine_point("Roll Vinyl", 140, 1, 10, 10),
    fine_point("Roll Paper", 180, 1, 10, 10),
    knife("Basswood 1/16", 1200, 8, 100),
];

pub fn search(query: &str) -> impl Iterator<Item = &'static Material> + '_ {
    let q = query.trim().to_lowercase();
    MATERIALS.iter().filter(move |m| m.name.to_lowercase().contains(&q))
}

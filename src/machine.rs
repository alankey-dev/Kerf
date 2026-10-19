use serde::Serialize;

use crate::units::Units;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum MachineFamily {
    StudioPro,
    Studio,
    Cutter,
    Mini,
    Wide,
}

proto_enum! {
    /// Wire values for the connection type.
    pub enum ConnectionType {
        Bluetooth = 1 => "BLUETOOTH_CT",
        Usb = 2 => "USB_CT",
        Hid = 3 => "HID_CT",
        Ble = 4 => "BLE_CT",
        IpTcp = 5 => "IP_TCP_CT",
    }
}

proto_enum! {
    pub enum MatMode {
        NormalMat = 0 => "MAT_MODE_NORMAL_MAT",
        MatLess = 1 => "MAT_MODE_MAT_LESS",
        CardMat = 2 => "MAT_MODE_CARD_MAT",
    }
}

#[allow(dead_code, reason = "full load type table; the Prepare screen offers a matless roll for Without Mat")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadType {
    OnMat = 0,
    Matless = 1,
    MatlessRoll = 2,
    CardMat = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatSize {
    pub imperial_name: &'static str,
    /// The metric name where it differs; paper sizes repeat the imperial name.
    pub metric_name: &'static str,
    pub width_in: f32,
    pub height_in: f32,
    pub preferred: bool,
    pub matless: bool,
}

impl MatSize {
    pub fn name(&self, units: Units) -> &'static str {
        match units {
            Units::Imperial => self.imperial_name,
            Units::Metric => self.metric_name,
        }
    }
}

pub const MAT_PADDING_PT: f32 = 18.0;
pub const MAT_MARGIN_LR_PT: f32 = 50.0;
pub const MAT_MARGIN_TB_PT: f32 = 100.0;

const fn mat(names: (&'static str, &'static str), width_in: f32, height_in: f32) -> MatSize {
    MatSize { imperial_name: names.0, metric_name: names.1, width_in, height_in, preferred: false, matless: false }
}

const fn preferred(names: (&'static str, &'static str), width_in: f32, height_in: f32) -> MatSize {
    MatSize { preferred: true, ..mat(names, width_in, height_in) }
}

const fn roll(names: (&'static str, &'static str), width_in: f32, height_in: f32) -> MatSize {
    MatSize { matless: true, ..mat(names, width_in, height_in) }
}

/// A paper size reads the same in either unit.
const fn paper(name: &'static str) -> (&'static str, &'static str) {
    (name, name)
}

/// Standard mats for the Studio and Cutter families. The 12 in mats carry rounded metric names; the
/// others convert exactly, with 12 in as 30.5 cm.
const STANDARD_MATS: [MatSize; 14] = [
    mat(("4 in x 6 in", "10.16 cm x 15.24 cm"), 4.0, 6.0),
    mat(paper("A5"), 5.83, 8.27),
    mat(("6 in x 6 in", "15.24 cm x 15.24 cm"), 6.0, 6.0),
    mat(("8.25 in x 10 in", "20.96 cm x 25.4 cm"), 8.25, 10.0),
    mat(paper("A4"), 8.27, 11.69),
    mat(paper("Letter"), 8.5, 11.0),
    mat(paper("Legal"), 8.5, 14.0),
    mat(("9 in x 12 in", "22.86 cm x 30.5 cm"), 9.0, 12.0),
    mat(("11 in x 11 in", "27.94 cm x 27.94 cm"), 11.0, 11.0),
    mat(paper("Tabloid"), 11.0, 17.0),
    mat(paper("A3"), 11.69, 16.54),
    preferred(("12 in x 12 in", "30.5 cm x 30.5 cm"), 12.0, 12.0),
    mat(("12 in x 19 in", "30.5 cm x 48.26 cm"), 12.0, 19.0),
    mat(("12 in x 24 in", "30.5 cm x 61 cm"), 12.0, 24.0),
];

const MINI_MATS: [MatSize; 3] = [
    preferred(("4.5 in x 6.5 in", "11.43 cm x 16.51 cm"), 4.5, 6.5),
    mat(("4.5 in x 12 in", "11.43 cm x 30.5 cm"), 4.5, 12.0),
    roll(("Roll material 5.45 in", "Roll material 13.84 cm"), 5.45, 12.0),
];

const WIDE_MATS: [MatSize; 4] = [
    preferred(("24 in x 12 in", "61 cm x 30.5 cm"), 24.0, 12.0),
    mat(("24 in x 28 in", "61 cm x 71.12 cm"), 24.0, 28.0),
    roll(("Roll material 13 in", "Roll material 33.02 cm"), 13.0, 12.0),
    roll(("Roll material 25 in", "Roll material 63.5 cm"), 25.0, 12.0),
];

impl MachineFamily {
    pub const ALL: [MachineFamily; 5] = [Self::StudioPro, Self::Studio, Self::Cutter, Self::Mini, Self::Wide];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::StudioPro => "Kerf Studio Pro",
            Self::Studio => "Kerf Studio",
            Self::Cutter => "Kerf Cutter",
            Self::Mini => "Kerf Mini",
            Self::Wide => "Kerf Wide",
        }
    }

    pub fn codename(self) -> Option<&'static str> {
        match self {
            Self::StudioPro => Some("Chisel"),
            Self::Cutter => Some("Plane"),
            Self::Mini => Some("Auger"),
            Self::Wide => Some("Lathe"),
            Self::Studio => None,
        }
    }

    pub fn mat_sizes(self) -> &'static [MatSize] {
        match self {
            Self::StudioPro | Self::Studio | Self::Cutter => &STANDARD_MATS,
            Self::Mini => &MINI_MATS,
            Self::Wide => &WIDE_MATS,
        }
    }

    pub fn preferred_mat(self) -> MatSize {
        let sizes = self.mat_sizes();
        sizes.iter().copied().find(|m| m.preferred).unwrap_or(sizes[0])
    }
}

impl LoadType {
    pub fn label(self) -> &'static str {
        match self {
            Self::OnMat => "On Mat",
            Self::Matless | Self::MatlessRoll => "Without Mat",
            Self::CardMat => "On Card Mat",
        }
    }

    pub fn mat_mode(self) -> MatMode {
        match self {
            Self::OnMat => MatMode::NormalMat,
            Self::Matless | Self::MatlessRoll => MatMode::MatLess,
            Self::CardMat => MatMode::CardMat,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mat_names_follow_the_units() {
        let mat = MachineFamily::StudioPro.preferred_mat();
        assert_eq!(mat.name(Units::Imperial), "12 in x 12 in");
        assert_eq!(mat.name(Units::Metric), "30.5 cm x 30.5 cm");
        let a4 = MachineFamily::StudioPro.mat_sizes().iter().find(|m| m.imperial_name == "A4").unwrap();
        assert_eq!(a4.name(Units::Metric), "A4");
    }
}

//! The bridge message model and the three Make payloads sent to the machine.
//!
//! Kerf sends plaintext path data with `path_encrypted: false` rather than pre-encrypted path strings.

use serde::Serialize;

use crate::composition::{Mat, MatItem};
use std::rc::Rc;

use crate::design::{self, Design, Operation, POINTS_PER_INCH, Shape};
use crate::effects::{Geometry, Piece};
use crate::machine::{ConnectionType, MAT_MARGIN_LR_PT, MAT_MARGIN_TB_PT, MAT_PADDING_PT, MachineFamily, MatMode};
use crate::make_flow::MatSetup;
use crate::material::{Material, PressureAdjust, Tool};

proto_enum! {
    /// Interaction status, the cut subset.
    pub enum BridgeStatus {
        Error = 0 => "riError",
        Complete = 1 => "riComplete",
        StartSuccess = 2 => "riStartSuccess",
        NoDeviceConnected = 101 => "riNoDeviceConnected",
        SingleDeviceConnected = 103 => "riSingleDeviceConnected",
        DeviceOpenSuccess = 105 => "riDeviceOpenSuccess",
        SelectDevice = 109 => "riSelectDevice",
        WaitOnMatLoad = 110 => "riWaitOnMatLoad",
        MatLoaded = 111 => "riMatLoaded",
        MatUnloaded = 112 => "riMatUnloaded",
        WaitOnMatUnload = 113 => "riWaitOnMatUnload",
        WaitOnGo = 115 => "riWaitOnGo",
        GoPressed = 117 => "riGoPressed",
        PausePressed = 118 => "riPausePressed",
        DevicePaused = 122 => "riDevicePaused",
        DeviceResumed = 123 => "riDeviceResumed",
        WaitingOnMaterialSelected = 133 => "riWaitingOnMaterialSelected",
        MaterialSelected = 134 => "riMaterialSelected",
        SendToolArray = 135 => "riSendToolArray",
        ToolInfoReceived = 136 => "riToolInfoReceived",
        DetectingTool = 137 => "riDetectingTool",
        MatCut = 700 => "riMATCUT",
        MatCutNeedPathData = 701 => "riMATCUTNeedPathData",
        MatCutSetPathData = 703 => "riMATCUTSetPathData",
        MatCutNeedAccessoryChange = 706 => "riMATCUTNeedAccessoryChange",
        MatCutSetProgress = 710 => "riMATCUTSetProgress",
        MatCutAbortCut = 717 => "riMATCUTAbortCut",
        MatCutSimulateLoadButtonPressed = 719 => "riMATCUTSimulateLoadButtonPressed",
        MatCutSimulateGoButtonPressed = 720 => "riMATCUTSimulateGoButtonPressed",
        MatCutSimulatePauseButtonPressed = 721 => "riMATCUTSimulatePauseButtonPressed",
        MatCutGettingDevicePressureSettings = 726 => "riMATCUTGettingDevicePressureSettings",
        MatCutCompleteSuccess = 727 => "riMATCUTCompleteSuccess",
    }
}

proto_enum! {
    /// `NativeModel.PBArtType`, without the die-cut, kiss-cut, stitch and broad-deboss variants.
    pub enum ArtType {
        None = 0 => "NONE_ART_TYPE",
        Cut = 1 => "CUT_ART_TYPE",
        Draw = 2 => "DRAW_ART_TYPE",
        Score = 4 => "SCORE_ART_TYPE",
        Deboss = 5 => "DEBOSS_ART_TYPE",
        Wave = 6 => "WAVE_ART_TYPE",
        Engrave = 7 => "ENGRAVE_ART_TYPE",
        Perforate = 8 => "PERFORATE_ART_TYPE",
        MidasFine = 10 => "MIDAS_FINE_ART_TYPE",
        MidasRegular = 11 => "MIDAS_REGULAR_ART_TYPE",
        MidasBold = 12 => "MIDAS_BOLD_ART_TYPE",
    }
}

proto_enum! {
    pub enum PathToPointsAlgorithm {
        DefaultRules = 0 => "PTP_DEFAULT_RULES",
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<InteractionHandle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<BridgeStatus>,
    pub status: BridgeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub device_list: Vec<DeviceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_info: Option<BridgeSelectedTools>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mat_path_data: Option<MatPathData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_selected_payload: Option<MaterialSelected>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionHandle {
    pub current_interaction: BridgeStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub device_type: String,
    pub serial: String,
    pub connection_type: ConnectionType,
    pub bluetooth_name: String,
    pub allows_virtual_buttons: bool,
    /// Local only. The family is resolved from the device type and never sent on the wire.
    #[serde(skip)]
    pub machine_family: MachineFamily,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MarginPadding {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineMargins {
    pub use_this_info: bool,
    pub mat_margin: MarginPadding,
    pub mat_padding: MarginPadding,
    pub matless_margin: MarginPadding,
    pub matless_padding: MarginPadding,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialSelected {
    pub selected: bool,
    pub mat_height: f32,
    pub mat_width: f32,
    pub matless: bool,
    pub skip_mat_measure: bool,
    pub mat_mode: MatMode,
    pub machine_margins: MachineMargins,
    pub prescribed_mat_height_inches: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeApi {
    pub pressure: i32,
    pub original_pressure: i32,
    pub multi_pass: u32,
    pub cut_speed: f32,
    pub cut_accel: f32,
    pub move_to_speed: f32,
    pub z_speed: f32,
    pub delta_adjustment: Vec<i32>,
}

/// `NativeModel.PBTool`, whose field names are already camelCase in the proto.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub display_name: String,
    pub name: String,
    pub tool_type_enum: Tool,
    pub precision_mode: ModeApi,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matless_mode: Option<ModeApi>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub tool: Tool,
    pub line: ArtType,
    pub tool_from_api: ToolSpec,
    pub path_to_points_algorithm: PathToPointsAlgorithm,
    pub custom_offset: f64,
    pub use_custom_offset: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSelectedTools {
    pub first_pen: String,
    pub tools: Vec<ToolInfo>,
    pub is_matless: bool,
    pub use_walk_away_cutting: bool,
    pub mat_mode: MatMode,
    pub perform_auto_cutoff: bool,
    pub machine_margins: MachineMargins,
}

/// SVG affine `matrix(a b c d e f)`: `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Matrix {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// One `imageData[]` entry; in the proto it is itself a `PBMatPathData`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    pub path_data: String,
    pub transform: Matrix,
    pub actual_path_type: ArtType,
    pub art_tool: Tool,
    pub path_color: String,
    pub contour_active_flags: Vec<u32>,
    pub image_id: String,
    pub path_id: String,
    pub fiducial_id: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatPathData {
    pub material_size: Size,
    pub image_data: Vec<ImageData>,
    pub path_encrypted: bool,
    pub fiducial_id: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonToolCut {
    pub pct: f64,
    pub total_num_passes: u32,
    pub current_pass: u32,
}

/// The renderer flattens this into `CUT_PROGRESS{currentOperation, currentPass, totalPasses, percent, ...}`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub cut: CommonToolCut,
    pub current_operation: ArtType,
    pub current_percentage: f64,
    pub estimated_seconds_left: f64,
    pub elapsed_seconds: f64,
}

impl BridgeMessage {
    pub fn new(status: BridgeStatus) -> Self {
        Self {
            handle: None,
            interaction: None,
            status,
            device: None,
            device_list: Vec::new(),
            tool_info: None,
            mat_path_data: None,
            material_selected_payload: None,
            progress: None,
        }
    }

    pub fn to_pretty_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("serialisation failed: {e}"))
    }
}

impl Matrix {
    pub const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(x: f32, y: f32) -> Self {
        Self { e: x, f: y, ..Self::IDENTITY }
    }

    pub fn rotate(deg: f32) -> Self {
        let (sin, cos) = deg.to_radians().sin_cos();
        Self { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Self { a: sx, d: sy, ..Self::IDENTITY }
    }

    /// Applies `self` first, then `next`.
    pub fn then(self, next: Self) -> Self {
        Self {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }

    pub fn apply(self, (x, y): (f32, f32)) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

/// One piece of one copy on a mat. `transform` maps its shape's local frame, where the piece's
/// contours are, in points onto the mat.
pub struct Placement<'a> {
    pub shape: &'a Shape,
    geometry: Rc<Geometry>,
    piece: usize,
    pub copy: u32,
    pub transform: Matrix,
}

impl Placement<'_> {
    pub fn piece(&self) -> &Piece {
        &self.geometry.pieces()[self.piece]
    }
}

/// Lays one composed mat out in points: each piece of each copy at its packed offset, optionally mirrored.
pub fn mat_placements<'a>(design: &'a Design, mat: &Mat, setup: &MatSetup) -> Vec<Placement<'a>> {
    let mirror = mirror_matrix(setup);
    let place = |item: &MatItem| {
        let shape = design.shapes.iter().find(|s| s.id == item.piece.owner)?;
        let geometry = design.geometry(shape);
        let piece = geometry.pieces().iter().position(|p| p.id == item.piece)?;
        let b = geometry.pieces()[piece].bounds;
        let ((ox, oy), (min_x, min_y), (cx, cy)) = (item.offset_in, (b.min_x, b.min_y), shape.centre_in());
        let (hw, hh) = (shape.width_in * POINTS_PER_INCH / 2.0, shape.height_in * POINTS_PER_INCH / 2.0);
        let (x, y) = ((ox + cx - min_x) * POINTS_PER_INCH, (oy + cy - min_y) * POINTS_PER_INCH);
        let flip = Matrix::scale(if shape.flip_x { -1.0 } else { 1.0 }, if shape.flip_y { -1.0 } else { 1.0 });
        let placed = Matrix::translate(-hw, -hh).then(flip).then(Matrix::rotate(shape.rotation_deg)).then(Matrix::translate(x, y));
        let transform = nested_turn(placed, item, &geometry.pieces()[piece]).then(mirror);
        Some(Placement { shape, geometry, piece, copy: item.copy, transform })
    };
    mat.items.iter().filter_map(place).collect()
}

/// `placed` puts the piece's bounds top-left at `item.offset_in`. A nested item turns about the placed
/// centre, then moves its turned bounds' top-left back onto `offset_in`.
fn nested_turn(placed: Matrix, item: &MatItem, piece: &Piece) -> Matrix {
    if item.rotation_deg == 0.0 {
        return placed;
    }
    let ((ox, oy), b) = (item.offset_in, piece.bounds);
    let r = crate::composition::turned_extent(piece, item.rotation_deg);
    let pt = |v: f32| v * POINTS_PER_INCH;
    let (px, py) = (pt(ox + b.width() / 2.0), pt(oy + b.height() / 2.0));
    placed.then(Matrix::translate(-px, -py)).then(Matrix::rotate(item.rotation_deg)).then(Matrix::translate(pt(ox - r.min_x), pt(oy - r.min_y)))
}

fn mirror_matrix(setup: &MatSetup) -> Matrix {
    match setup.mirror {
        true => Matrix::scale(-1.0, 1.0).then(Matrix::translate(setup.mat_size.width_in * POINTS_PER_INCH, 0.0)),
        false => Matrix::IDENTITY,
    }
}

pub fn machine_margins() -> MachineMargins {
    let margin = MarginPadding { top: MAT_MARGIN_TB_PT, bottom: MAT_MARGIN_TB_PT, left: MAT_MARGIN_LR_PT, right: MAT_MARGIN_LR_PT };
    let padding = MarginPadding { top: MAT_PADDING_PT, bottom: MAT_PADDING_PT, left: MAT_PADDING_PT, right: MAT_PADDING_PT };
    MachineMargins { use_this_info: true, mat_margin: margin, mat_padding: padding, matless_margin: margin, matless_padding: padding }
}

/// Only Pen paths carry a colour, their pen's; everything else sends `"NONE"`.
pub fn build_mat_path_data(design: &Design, mat: &Mat, setup: &MatSetup, material: &Material) -> MatPathData {
    let image = |p: Placement| {
        let piece = p.piece();
        let op = piece.style.operation;
        Some(ImageData {
            path_data: design::to_svg_path_d(&piece.contours),
            transform: p.transform,
            actual_path_type: op.art_type()?,
            art_tool: op.load(material).0,
            path_color: if op == Operation::Pen { piece.style.pen.hex() } else { "NONE".into() },
            contour_active_flags: vec![1],
            image_id: format!("{}{}", piece.id.owner, piece.id.suffix()),
            path_id: format!("{}-{}{}", piece.id.owner, p.copy, piece.id.suffix()),
            fiducial_id: op.fiducial_id(),
        })
    };
    let size = setup.mat_size;
    let image_data: Vec<ImageData> = mat_placements(design, mat, setup).into_iter().filter_map(image).collect();
    MatPathData {
        material_size: Size { width: size.width_in * POINTS_PER_INCH, height: size.height_in * POINTS_PER_INCH },
        fiducial_id: image_data.iter().map(|i| i.fiducial_id).max().unwrap_or(0),
        image_data,
        path_encrypted: false,
    }
}

pub fn build_material_selected(setup: &MatSetup) -> MaterialSelected {
    let mat_mode = setup.load_type.mat_mode();
    MaterialSelected {
        selected: true,
        mat_height: setup.mat_size.height_in * POINTS_PER_INCH,
        mat_width: setup.mat_size.width_in * POINTS_PER_INCH,
        matless: mat_mode == MatMode::MatLess,
        skip_mat_measure: false,
        mat_mode,
        machine_margins: machine_margins(),
        prescribed_mat_height_inches: setup.mat_size.height_in,
    }
}

/// One `PBToolInfo` per distinct tool and art type the mat's operations need, in plot order.
pub fn build_selected_tools(mat: &Mat, material: &Material, adjust: PressureAdjust, setup: &MatSetup) -> BridgeSelectedTools {
    let mat_mode = setup.load_type.mat_mode();
    let is_matless = mat_mode == MatMode::MatLess;
    let mode = mode_api(material, adjust);
    let tool_info = |tool: Tool, line: ArtType| ToolInfo {
        tool,
        line,
        tool_from_api: ToolSpec {
            display_name: tool.display_name().into(),
            name: tool.display_name().into(),
            tool_type_enum: tool,
            matless_mode: is_matless.then(|| mode.clone()),
            precision_mode: mode.clone(),
        },
        path_to_points_algorithm: PathToPointsAlgorithm::DefaultRules,
        custom_offset: 0.0,
        use_custom_offset: false,
    };
    let mut tools: Vec<ToolInfo> = Vec::new();
    for &op in &mat.operations {
        let Some(line) = op.art_type() else { continue };
        let tool = op.load(material).0;
        if !tools.iter().any(|t| t.tool == tool && t.line == line) {
            tools.push(tool_info(tool, line));
        }
    }
    BridgeSelectedTools {
        first_pen: String::new(),
        tools,
        is_matless,
        use_walk_away_cutting: false,
        mat_mode,
        perform_auto_cutoff: is_matless,
        machine_margins: machine_margins(),
    }
}

fn mode_api(material: &Material, adjust: PressureAdjust) -> ModeApi {
    ModeApi {
        pressure: material.adjusted_pressure(adjust),
        original_pressure: material.pressure,
        multi_pass: material.multi_pass,
        cut_speed: material.cut_speed as f32,
        cut_accel: 2.0,
        move_to_speed: 12.0,
        z_speed: 4.0,
        delta_adjustment: material.delta_adjustment.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{FoilWeight, ShapeKind};
    use crate::machine::{LoadType, MachineFamily};
    use crate::material::{Clamp, MATERIALS};

    fn one_square() -> Design {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Square);
        design.shapes[0].x_in = 1.0;
        design.shapes[0].y_in = 1.0;
        design
    }

    fn one_square_doing(op: Operation) -> Design {
        let mut design = one_square();
        design.shapes[0].operation = op;
        design
    }

    fn only_mat(design: &Design) -> Mat {
        let mut mats = crate::composition::compose(design, setup_12x12().mat_size, 1);
        assert_eq!(mats.len(), 1, "{mats:?}");
        mats.remove(0)
    }

    fn setup_12x12() -> MatSetup {
        MatSetup { load_type: LoadType::OnMat, mat_size: MachineFamily::StudioPro.preferred_mat(), mirror: false, copies: 1 }
    }

    #[test]
    fn builds_the_three_make_payloads_for_one_square() {
        let (design, setup) = (one_square(), setup_12x12());
        let cardstock = &MATERIALS[0];

        let paths = build_mat_path_data(&design, &only_mat(&design), &setup, cardstock);
        assert_eq!(paths.material_size, Size { width: 864.0, height: 864.0 });
        assert_eq!(paths.image_data.len(), 1);
        assert_eq!(paths.image_data[0].path_data, "M0 0 L144 0 L144 144 L0 144 Z");
        assert_eq!(paths.image_data[0].transform, Matrix::translate(18.0, 18.0));
        assert_eq!(paths.image_data[0].actual_path_type, ArtType::Cut);
        assert_eq!(paths.image_data[0].art_tool, Tool::DragKnife);
        assert_eq!(paths.image_data[0].path_color, "NONE");
        assert!(!paths.path_encrypted);

        let material = build_material_selected(&setup);
        assert_eq!((material.mat_width, material.mat_height), (864.0, 864.0));
        assert_eq!(material.mat_mode, MatMode::NormalMat);
        assert_eq!(material.machine_margins.mat_margin.left, 50.0);
        assert_eq!(material.machine_margins.mat_margin.top, 100.0);
        assert_eq!(material.machine_margins.mat_padding.top, 18.0);

        let pressure = |adjust| build_selected_tools(&only_mat(&design), cardstock, adjust, &setup).tools[0].tool_from_api.precision_mode.pressure;
        assert_eq!(cardstock.delta_adjustment, [-20, 0, 20]);
        assert_eq!(pressure(PressureAdjust::Default), 300);
        assert_eq!(pressure(PressureAdjust::More), 300 + 20);
        assert_eq!(pressure(PressureAdjust::Less), 300 - 20);

        let mut msg = BridgeMessage::new(BridgeStatus::MatCutSetPathData);
        msg.mat_path_data = Some(paths);
        let json = msg.to_pretty_json();
        assert!(json.contains("\"status\": \"riMATCUTSetPathData\""), "{json}");
        assert!(json.contains("\"pathEncrypted\": false"), "{json}");
    }

    #[test]
    fn mirror_flips_across_the_mat_width() {
        let setup = MatSetup { mirror: true, ..setup_12x12() };
        let paths = build_mat_path_data(&one_square(), &only_mat(&one_square()), &setup, &MATERIALS[0]);
        let t = paths.image_data[0].transform;
        assert_eq!(t.apply((0.0, 0.0)), (864.0 - 18.0, 18.0));
        assert_eq!(t.apply((144.0, 144.0)), (864.0 - 18.0 - 144.0, 18.0 + 144.0));
    }

    #[test]
    fn a_pen_square_draws_in_its_colour_with_the_pen_in_clamp_a() {
        let (design, setup, cardstock) = (one_square_doing(Operation::Pen), setup_12x12(), &MATERIALS[0]);
        let image = &build_mat_path_data(&design, &only_mat(&design), &setup, cardstock).image_data[0];
        assert_eq!((image.actual_path_type, image.art_tool), (ArtType::Draw, Tool::Pen));
        assert_eq!(image.path_color, design.shapes[0].hex_colour());
        assert!(image.path_color.starts_with('#'));
        let tools = build_selected_tools(&only_mat(&design), cardstock, PressureAdjust::Default, &setup).tools;
        assert_eq!(tools.iter().map(|t| (t.tool, t.line)).collect::<Vec<_>>(), [(Tool::Pen, ArtType::Draw)]);
        assert_eq!(Operation::Pen.load(cardstock), (Tool::Pen, Clamp::A));
        let json = serde_json::to_string(&tools[0]).unwrap();
        assert!(json.contains("\"tool\":\"PEN_DT_TT\""), "{json}");
    }

    #[test]
    fn a_pen_layer_sends_its_pen_colour_as_path_colour() {
        let mut design = one_square();
        let wine = crate::pens::Pen::named("Wine", crate::pens::PenSize::Medium);
        design.shapes[0].set_operation(Operation::Pen);
        design.shapes[0].set_pen(wine);
        let image = &build_mat_path_data(&design, &only_mat(&design), &setup_12x12(), &MATERIALS[0]).image_data[0];
        assert_eq!(image.path_color, "#8a3333", "{} pen", wine.colour_name());
        assert_eq!(image.path_color, design.shapes[0].hex_colour(), "the canvas shows the same colour");
    }

    #[test]
    fn a_score_square_uses_the_scoring_wheel() {
        let (design, setup, cardstock) = (one_square_doing(Operation::Score), setup_12x12(), &MATERIALS[0]);
        let image = &build_mat_path_data(&design, &only_mat(&design), &setup, cardstock).image_data[0];
        assert_eq!((image.actual_path_type, image.art_tool), (ArtType::Score, Tool::ScoringWheel));
        assert_eq!(image.path_color, "NONE");
        let tools = build_selected_tools(&only_mat(&design), cardstock, PressureAdjust::Default, &setup).tools;
        assert_eq!(tools.iter().map(|t| (t.tool.code(), t.line)).collect::<Vec<_>>(), [(6, ArtType::Score)]);
    }

    #[test]
    fn operations_map_to_the_recovered_art_types_and_tools() {
        let cardstock = &MATERIALS[0];
        let cases = [
            (Operation::WavyCut, 6, 11),
            (Operation::Perforate, 8, 9),
            (Operation::Foil(FoilWeight::Fine), 10, 13),
            (Operation::Foil(FoilWeight::Medium), 11, 14),
            (Operation::Foil(FoilWeight::Bold), 12, 15),
            (Operation::Deboss, 5, 10),
            (Operation::Engrave, 7, 12),
        ];
        for (op, art, tool) in cases {
            assert_eq!(op.art_type().map(ArtType::code), Some(art), "{op:?}");
            assert_eq!(op.load(cardstock).0.code(), tool, "{op:?}");
        }
        let ptc_design = one_square_doing(Operation::PrintThenCut);
        let ptc = build_mat_path_data(&ptc_design, &only_mat(&ptc_design), &setup_12x12(), cardstock);
        assert_eq!((ptc.fiducial_id, ptc.image_data[0].fiducial_id, ptc.image_data[0].art_tool), (1, 1, Tool::DragKnife));
        let guide = one_square_doing(Operation::Guide);
        assert!(crate::composition::compose(&guide, setup_12x12().mat_size, 1).is_empty(), "Guide never reaches a mat");
    }

    #[test]
    fn shapes_are_sent_at_their_packed_offsets_not_their_canvas_positions() {
        let mut design = one_square();
        let far = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(far).unwrap();
        (s.x_in, s.y_in) = (8.0, 9.0);
        let setup = MatSetup { copies: 2, ..setup_12x12() };
        let mat = crate::composition::compose(&design, setup.mat_size, setup.copies).remove(0);
        let transforms: Vec<Matrix> = build_mat_path_data(&design, &mat, &setup, &MATERIALS[0]).image_data.iter().map(|i| i.transform).collect();
        let step = 144.0 + 18.0;
        let expected = [0.0, 1.0, 2.0, 3.0].map(|i| Matrix::translate(18.0 + i * step, 18.0));
        assert_eq!(transforms, expected, "the second copy packs on along the row like any other unit");
    }

    /// A heart and a circle, both curved, attached so they pack as one unit.
    fn curved_pair(edit: impl Fn(&mut Shape)) -> Design {
        let mut design = Design::default();
        let heart = design.add_shape(ShapeKind::Heart);
        let circle = design.add_shape(ShapeKind::Circle);
        for (id, at) in [(heart, (1.0, 1.0)), (circle, (3.5, 1.5))] {
            let s = design.shape_mut(id).unwrap();
            (s.x_in, s.y_in) = at;
            edit(s);
        }
        design.attach(&[heart, circle]);
        design
    }

    fn path_message(design: &Design, setup: &MatSetup) -> Vec<BridgeMessage> {
        let mats = crate::composition::compose(design, setup.mat_size, setup.copies);
        let message = |mat: &Mat| {
            let paths = build_mat_path_data(design, mat, setup, &MATERIALS[0]);
            assert_inside_cutting_area(design, &paths);
            BridgeMessage { mat_path_data: Some(paths), ..BridgeMessage::new(BridgeStatus::MatCutSetPathData) }
        };
        mats.iter().map(message).collect()
    }

    /// Every flattened point of every path, as the transform places it, lies in the padded cutting area.
    fn assert_inside_cutting_area(design: &Design, paths: &MatPathData) {
        let (w, h) = (paths.material_size.width, paths.material_size.height);
        let inside = |v: f32, max: f32| (MAT_PADDING_PT - 0.01..=max - MAT_PADDING_PT + 0.01).contains(&v);
        for image in &paths.image_data {
            let shape = design.shapes.iter().find(|s| s.id.to_string() == image.image_id).unwrap();
            for (x, y) in shape.contours().iter().flat_map(design::Contour::flatten) {
                let (mx, my) = image.transform.apply((x * POINTS_PER_INCH, y * POINTS_PER_INCH));
                assert!(inside(mx, w) && inside(my, h), "{} reaches ({mx}, {my}) on a {w} x {h} pt mat", image.path_id);
            }
        }
    }

    fn golden_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.json"))
    }

    /// Parses the committed payload and compares it field by field. `UPDATE_GOLDEN=1 cargo test golden` rewrites it.
    fn assert_golden(name: &str, messages: &[BridgeMessage]) {
        let json = serde_json::to_string_pretty(messages).unwrap() + "\n";
        let path = golden_path(name);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &json).unwrap();
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}; run UPDATE_GOLDEN=1 cargo test golden", path.display()));
        let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        if let Some(diff) = json_diff(&parse(&expected), &parse(&json), "$") {
            panic!("{name} differs from tests/golden/{name}.json at {diff}");
        }
    }

    /// The first difference between two JSON values; numbers match within a thousandth of a point.
    fn json_diff(expected: &serde_json::Value, actual: &serde_json::Value, at: &str) -> Option<String> {
        use serde_json::Value as V;
        match (expected, actual) {
            (V::Number(e), V::Number(a)) => {
                let (e, a) = (e.as_f64().unwrap_or(f64::NAN), a.as_f64().unwrap_or(f64::NAN));
                ((e - a).abs() > 1e-3 || e.is_nan() || a.is_nan()).then(|| format!("{at}: {e} != {a}"))
            }
            (V::Array(e), V::Array(a)) if e.len() == a.len() => e.iter().zip(a).enumerate().find_map(|(i, (e, a))| json_diff(e, a, &format!("{at}[{i}]"))),
            (V::Object(e), V::Object(a)) if e.len() == a.len() => e.iter().find_map(|(k, e)| match a.get(k) {
                Some(a) => json_diff(e, a, &format!("{at}.{k}")),
                None => Some(format!("{at}.{k} is missing")),
            }),
            _ => (expected != actual).then(|| format!("{at}: {expected} != {actual}")),
        }
    }

    #[test]
    fn golden_rotated() {
        let design = curved_pair(|s| s.rotation_deg = 30.0);
        assert_golden("rotated", &path_message(&design, &setup_12x12()));
    }

    #[test]
    fn golden_flipped() {
        let design = curved_pair(|s| (s.flip_x, s.flip_y) = (true, s.kind == ShapeKind::Circle));
        assert_golden("flipped", &path_message(&design, &setup_12x12()));
    }

    #[test]
    fn golden_mirrored() {
        let design = curved_pair(|s| (s.rotation_deg, s.flip_x) = (45.0, true));
        assert_golden("mirrored", &path_message(&design, &MatSetup { mirror: true, ..setup_12x12() }));
    }

    #[test]
    fn golden_multi_copy() {
        let design = curved_pair(|s| s.rotation_deg = 15.0);
        let messages = path_message(&design, &MatSetup { copies: 4, mirror: true, ..setup_12x12() });
        assert_eq!(messages.iter().map(|m| m.mat_path_data.as_ref().unwrap().image_data.len()).sum::<usize>(), 8);
        assert_golden("multi_copy", &messages);
    }

    /// An 11.8 in strip that only fits a 12 x 24 mat on its side, nested beside a heart.
    #[test]
    fn golden_nested() {
        let long = MachineFamily::StudioPro.mat_sizes().iter().copied().find(|m| (m.width_in, m.height_in) == (12.0, 24.0) && !m.matless).unwrap();
        let setup = MatSetup { mat_size: long, ..setup_12x12() };
        let mut design = curved_pair(|_| {});
        let strip = design.add_shape(ShapeKind::Rectangle);
        let s = design.shape_mut(strip).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (0.0, 5.0, 11.8, 0.5);
        let layout = crate::composition::nested::optimise(&design, long, 1, Default::default(), std::time::Duration::from_secs(600), &Default::default());
        design.nesting = Some(layout);
        let mats = crate::composition::compose(&design, long, 1);
        let turned = mats[0].items.iter().find(|i| i.piece.owner == strip).unwrap();
        assert_eq!(turned.rotation_deg.rem_euclid(180.0), 90.0, "the strip lies on its side");
        for p in mat_placements(&design, &mats[0], &setup) {
            let item = mats[0].items.iter().find(|i| i.piece == p.piece().id).unwrap();
            let r = crate::composition::turned_extent(p.piece(), item.rotation_deg);
            let got = placed_bounds(&p);
            let want = [item.offset_in.0, item.offset_in.1, item.offset_in.0 + r.width(), item.offset_in.1 + r.height()].map(|v| v * POINTS_PER_INCH);
            for (g, w) in [got.min_x, got.min_y, got.max_x, got.max_y].into_iter().zip(want) {
                assert!((g - w).abs() < 0.05, "{} lands where Prepare paints it: {got:?} vs {want:?}", p.shape.id);
            }
        }
        assert_golden("nested", &path_message(&design, &setup));
    }

    #[test]
    fn a_one_point_transform_change_breaks_the_golden() {
        let mut actual = serde_json::to_value(path_message(&curved_pair(|s| s.rotation_deg = 30.0), &setup_12x12())).unwrap();
        let expected = serde_json::from_str(&std::fs::read_to_string(golden_path("rotated")).unwrap()).unwrap();
        assert_eq!(json_diff(&expected, &actual, "$"), None);
        let e = &mut actual[0]["matPathData"]["imageData"][0]["transform"]["e"];
        *e = serde_json::json!(e.as_f64().unwrap() + 1.0);
        assert!(json_diff(&expected, &actual, "$").is_some_and(|d| d.contains("transform.e")));
    }

    #[test]
    fn a_quarter_turn_and_a_flip_land_the_corners_where_the_canvas_shows_them() {
        let mut design = one_square();
        let s = &mut design.shapes[0];
        (s.width_in, s.rotation_deg, s.flip_x) = (4.0, 90.0, true);
        let t = build_mat_path_data(&design, &only_mat(&design), &setup_12x12(), &MATERIALS[0]).image_data[0].transform;
        let corner = |p| {
            let (x, y) = t.apply(p);
            (x.round(), y.round())
        };
        // 4 x 2 in, flipped then turned a quarter clockwise: a 2 x 4 in footprint at the padded corner.
        assert_eq!(corner((0.0, 0.0)), (18.0 + 144.0, 18.0 + 288.0));
        assert_eq!(corner((288.0, 0.0)), (18.0 + 144.0, 18.0));
        assert_eq!(corner((288.0, 144.0)), (18.0, 18.0));
        assert_eq!(corner((0.0, 144.0)), (18.0, 18.0 + 288.0));
        let (min_x, min_y, _, _) = design.shapes[0].world_bounds();
        for p in [(0.0, 0.0), (288.0, 0.0), (288.0, 144.0), (0.0, 144.0)] {
            let (wx, wy) = design.shapes[0].to_world((p.0 / POINTS_PER_INCH, p.1 / POINTS_PER_INCH));
            let canvas = ((wx - min_x) * POINTS_PER_INCH + 18.0, (wy - min_y) * POINTS_PER_INCH + 18.0);
            assert_eq!(corner(p), (canvas.0.round(), canvas.1.round()), "the mat agrees with the canvas at {p:?}");
        }
    }

    /// Every flattened point of `p`'s piece, as its transform places it, in inches.
    fn placed_bounds(p: &Placement) -> crate::snap::Aabb {
        let pts = p.piece().contours.iter().flat_map(design::Contour::flatten).map(|(x, y)| p.transform.apply((x * POINTS_PER_INCH, y * POINTS_PER_INCH)));
        pts.fold(crate::snap::Aabb::EMPTY, |b, (x, y)| crate::snap::Aabb { min_x: b.min_x.min(x), min_y: b.min_y.min(y), max_x: b.max_x.max(x), max_y: b.max_y.max(y) })
    }

    #[test]
    fn a_turned_item_puts_its_turned_bounds_at_its_offset() {
        let mut design = one_square();
        let s = &mut design.shapes[0];
        (s.kind, s.width_in, s.height_in, s.rotation_deg) = (ShapeKind::Heart, 4.0, 2.0, 20.0);
        let mut mat = only_mat(&design);
        let piece = design.geometry(&design.shapes[0]).pieces()[0].bounds;
        for deg in [90.0, 37.0, 180.0] {
            mat.items[0].offset_in = (1.0, 2.0);
            mat.items[0].rotation_deg = deg;
            let placements = mat_placements(&design, &mat, &setup_12x12());
            let r = crate::composition::turned_extent(placements[0].piece(), deg);
            let got = placed_bounds(&placements[0]);
            let want = (72.0, 144.0, 72.0 * (1.0 + r.width()), 72.0 * (2.0 + r.height()));
            assert!((got.min_x - want.0).abs() < 0.05 && (got.min_y - want.1).abs() < 0.05, "{deg}: {got:?} vs {want:?}");
            assert!((got.max_x - want.2).abs() < 0.05 && (got.max_y - want.3).abs() < 0.05, "{deg}: {got:?} vs {want:?}");
        }
        let quarter = crate::composition::turned_extent(&design.geometry(&design.shapes[0]).pieces()[0], 90.0);
        assert!((quarter.width() - piece.height()).abs() < 1e-3 && (quarter.height() - piece.width()).abs() < 1e-3, "a quarter turn swaps the extent");
    }

    #[test]
    fn twenty_copies_of_a_three_inch_square_spill_onto_further_mats_and_stay_inside_them() {
        let mut design = Design::default();
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.width_in, s.height_in) = (3.0, 3.0);
        for mirror in [false, true] {
            let setup = MatSetup { copies: 20, mirror, ..setup_12x12() };
            let messages = path_message(&design, &setup);
            let per_mat: Vec<usize> = messages.iter().map(|m| m.mat_path_data.as_ref().unwrap().image_data.len()).collect();
            assert_eq!(per_mat, [9, 9, 2], "three rows of three fit on a 12 x 12 mat");
        }
    }

    #[test]
    fn every_copy_of_every_library_shape_stays_inside_its_mat() {
        for kind in [ShapeKind::Heart, ShapeKind::Star, ShapeKind::Circle, ShapeKind::Arch, ShapeKind::Triangle, ShapeKind::Square] {
            let mut design = Design::default();
            let id = design.add_shape(kind.clone());
            let s = design.shape_mut(id).unwrap();
            (s.width_in, s.rotation_deg, s.flip_y) = (5.0, 30.0, true);
            for (copies, mirror) in (1..=20).flat_map(|n| [(n, false), (n, true)]) {
                let messages = path_message(&design, &MatSetup { copies, mirror, ..setup_12x12() });
                let sent: usize = messages.iter().map(|m| m.mat_path_data.as_ref().unwrap().image_data.len()).sum();
                assert_eq!(sent, copies as usize, "{kind:?} x {copies}");
            }
        }
    }

    #[test]
    fn status_codes_match_the_evidence() {
        assert_eq!(BridgeStatus::MatCutSimulateLoadButtonPressed.code(), 719);
        assert_eq!(BridgeStatus::MatCutSimulateGoButtonPressed.code(), 720);
        assert_eq!(BridgeStatus::MatCutSimulatePauseButtonPressed.code(), 721);
        assert_eq!(BridgeStatus::MatCutCompleteSuccess.code(), 727);
        assert_eq!(ConnectionType::IpTcp.code(), 5);
    }

    #[test]
    fn a_bitmap_lands_on_a_print_then_cut_mat_and_cuts_its_clip_outline() {
        let mut design = Design::default();
        let diamond = crate::design::Contour {
            start: (100.0, 0.0),
            segs: vec![crate::design::Seg::Line((200.0, 100.0)), crate::design::Seg::Line((100.0, 200.0)), crate::design::Seg::Line((0.0, 100.0))],
            closed: true,
        };
        let asset = crate::assets::id_of(&image::RgbaImage::new(1, 1));
        let id = design.add_shape(ShapeKind::Bitmap { asset, px: (200, 200), clip: vec![diamond] });
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in) = (1.0, 1.0);
        assert_eq!((s.width_in, s.height_in), (2.0, 2.0), "100 pixels per inch");
        let mat = only_mat(&design);
        assert!(mat.print_then_cut, "{mat:?}");
        assert_eq!(mat.label(), "Print Then Cut");
        let paths = build_mat_path_data(&design, &mat, &setup_12x12(), &MATERIALS[0]);
        assert_eq!(paths.image_data[0].path_data, "M72 0 L144 72 L72 144 L0 72 Z", "the clip, not the image rectangle");
        assert_eq!(paths.fiducial_id, 1);
    }
}

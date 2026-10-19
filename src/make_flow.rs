use crate::bridge::{self, BridgeMessage, BridgeStatus as S, DeviceInfo, InteractionHandle, Progress};
use crate::composition::{Mat, compose_on};
use crate::design::{Design, Operation};
use crate::device::{self, DeviceState};
use crate::machine::{ConnectionType, LoadType, MachineFamily, MatSize};
use crate::material::{Clamp, Material, PressureAdjust, Tool};
use crate::output::{DeviceCommand, DeviceEvent, DeviceProfile, Job, OutputDevice};
use crate::pens::Pen;
use crate::print_then_cut::{PaperSize, PrintSettings};

/// Something the flow wants done outside itself. The app carries each one out in `send_all`.
#[derive(Clone, Debug)]
pub enum Effect {
    /// A message for the simulated machine, through the `Transport`.
    Bridge(BridgeMessage),
    Device(DeviceCommand),
    /// (Re)open the runner on this profile's port. Nothing is sent until a `Device` command follows.
    Open(DeviceProfile),
    /// Write the job's bytes to a file chosen in a save dialog, through the same streaming as the port.
    DryRun {
        job: Job,
        mat: usize,
    },
    /// Remember this profile and start the next Make on it; `None` goes back to the simulated machine.
    UseDevice(Option<DeviceProfile>),
    DeleteProfile(u64),
}

fn bridge(msgs: impl IntoIterator<Item = BridgeMessage>) -> Vec<Effect> {
    msgs.into_iter().map(Effect::Bridge).collect()
}

/// The open device's connection test, shown on the setup screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TestState {
    #[default]
    Untested,
    Testing,
    Passed(String),
    Failed(String),
}

/// One line of the Load step. Pens are told apart by size and colour, other tools by tool alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolLoad {
    pub tool: Tool,
    pub clamp: Clamp,
    pub pen: Option<Pen>,
}

impl ToolLoad {
    /// "Load Black Fine Point Pen in Clamp A".
    pub fn label(&self) -> String {
        let name = self.pen.map_or_else(|| self.tool.display_name().to_owned(), |p| p.display_name());
        ["Load ", &name, " in ", self.clamp.label()].concat()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatSetup {
    pub load_type: LoadType,
    pub mat_size: MatSize,
    pub mirror: bool,
    pub copies: u32,
}

impl MatSetup {
    pub fn preferred(family: MachineFamily) -> Self {
        Self { load_type: LoadType::OnMat, mat_size: family.preferred_mat(), mirror: false, copies: 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectPhase {
    Listing,
    Connecting(ConnectionType),
    Connected,
    /// An open device's profile, port and connection test.
    Setup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Complete,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DonePhase {
    Checking,
    Unloading,
    Unloaded,
}

/// `mat/preview` (Prepare), the machine modal, then `mat/machine` with steps PRINT=0, SET=1, LOAD=2, GO=3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MakeStep {
    Prepare,
    /// A Print Then Cut page waits to be printed before Set (`PRINT = 0`).
    Print,
    ConnectMachine(ConnectPhase),
    Set,
    Load,
    Go,
    Cutting,
    Done {
        outcome: Outcome,
        phase: DonePhase,
    },
    /// The transport reported the machine gone; nothing more can be sent.
    Lost,
}

pub struct MakeFlow {
    pub step: MakeStep,
    pub design: Design,
    pub setup: MatSetup,
    pub print: PrintSettings,
    /// `compose_on(design, setup.mat_size, setup.copies, print.paper)`; Set, Load and Go run for `mats[current]`.
    pub mats: Vec<Mat>,
    pub current: usize,
    pub family: MachineFamily,
    pub machine: Option<DeviceInfo>,
    pub devices: Vec<DeviceInfo>,
    /// The remembered machine, when it was absent from the last device list.
    pub missing: Option<DeviceInfo>,
    pub material: Option<&'static Material>,
    pub material_query: String,
    pub pressure: PressureAdjust,
    pub device_state: DeviceState,
    pub progress: Option<Progress>,
    handle: Option<InteractionHandle>,
    /// The simulated machine, or an open device the same steps drive.
    pub device: OutputDevice,
    pub test: TestState,
    /// Lines sent of the open device's job.
    pub sent: Option<(usize, usize)>,
    /// What the open device is waiting on the user to swap, while streaming is stopped.
    pub tool_change: Option<String>,
    /// Why the open device stopped, shown on the Lost screen.
    pub failure: Option<String>,
    /// Print Then Cut mats left out, since an open device has no registration step.
    pub skipped_print_then_cut: usize,
}

impl MakeStep {
    /// Set Load Go step number.
    pub fn slg_number(self) -> Option<u8> {
        match self {
            Self::Print => Some(0),
            Self::Set => Some(1),
            Self::Load => Some(2),
            Self::Go => Some(3),
            _ => None,
        }
    }
}

impl MakeFlow {
    pub fn new(design: Design, remembered: Option<DeviceInfo>) -> Self {
        let family = remembered.as_ref().map_or(MachineFamily::StudioPro, |d| d.machine_family);
        let setup = MatSetup::preferred(family);
        let print = PrintSettings::default();
        Self {
            step: MakeStep::Prepare,
            mats: compose_on(&design, setup.mat_size, setup.copies, print.paper),
            current: 0,
            design,
            setup,
            print,
            family,
            machine: remembered,
            devices: Vec::new(),
            missing: None,
            material: None,
            material_query: String::new(),
            pressure: PressureAdjust::Default,
            device_state: DeviceState::SelectDevice,
            progress: None,
            handle: None,
            device: OutputDevice::Simulator,
            test: TestState::Untested,
            sent: None,
            tool_change: None,
            failure: None,
            skipped_print_then_cut: 0,
        }
    }

    /// The flow driving `device` instead of the simulated machine, its mats composed at the work area.
    pub fn on(mut self, device: OutputDevice) -> Self {
        self.use_device(device);
        self
    }

    fn use_device(&mut self, device: OutputDevice) {
        self.setup = match &device {
            OutputDevice::Open(p) => MatSetup { load_type: LoadType::OnMat, mat_size: work_area(p), ..self.setup },
            OutputDevice::Simulator => MatSetup { mat_size: self.family.preferred_mat(), ..self.setup },
        };
        self.device = device;
        self.test = TestState::Untested;
        self.recompose();
    }

    pub fn profile(&self) -> Option<&DeviceProfile> {
        match &self.device {
            OutputDevice::Open(p) => Some(p),
            OutputDevice::Simulator => None,
        }
    }

    pub fn profile_mut(&mut self) -> Option<&mut DeviceProfile> {
        match &mut self.device {
            OutputDevice::Open(p) => Some(p),
            OutputDevice::Simulator => None,
        }
    }

    /// Opens the setup screen for a saved profile, or a new one when the user chose Add device.
    pub fn choose_profile(&mut self, profile: DeviceProfile) {
        self.use_device(OutputDevice::Open(profile));
        self.step = MakeStep::ConnectMachine(ConnectPhase::Setup);
    }

    /// Edits to the profile's work area recompose, so Prepare and Go see the new size.
    pub fn profile_changed(&mut self) {
        if let Some(p) = self.profile() {
            self.setup.mat_size = work_area(p);
            self.recompose();
        }
    }

    /// Opens the port and asks the device who it is. Never motion.
    pub fn test_connection(&mut self) -> Vec<Effect> {
        let Some(p) = self.profile() else { return Vec::new() };
        let open = Effect::Open(p.clone());
        self.test = TestState::Testing;
        vec![open, Effect::Device(DeviceCommand::Test)]
    }

    pub fn finish_setup(&mut self) -> Vec<Effect> {
        let (Some(p), TestState::Passed(_)) = (self.profile(), &self.test) else { return Vec::new() };
        let remember = Effect::UseDevice(Some(p.clone()));
        let mut out = vec![remember];
        out.extend(self.start_mat());
        out
    }

    /// Back from an open device to the machine list, which asks the simulator for its machines.
    pub fn change_machine(&mut self) -> Vec<Effect> {
        self.use_device(OutputDevice::Simulator);
        self.machine = None;
        self.missing = None;
        self.start_mat()
    }

    pub fn delete_profile(&mut self) -> Vec<Effect> {
        let Some(id) = self.profile().map(|p| p.id) else { return Vec::new() };
        let mut out = vec![Effect::DeleteProfile(id)];
        out.extend(self.change_machine());
        out
    }

    /// The open device's bytes for the current mat, with the Set step's material and More or Less.
    pub fn job(&self) -> Option<Job> {
        let (p, material, mat) = (self.profile()?, self.material?, self.mat()?);
        Some(crate::output::job(&self.design, mat, &self.setup, p, p.drive(material, self.pressure)))
    }

    /// The Go step's Save to file: the same job the port would get, written to a file instead.
    pub fn save_to_file(&self) -> Vec<Effect> {
        match (self.step, self.job()) {
            (MakeStep::Go, Some(job)) => vec![Effect::DryRun { job, mat: self.current + 1 }],
            _ => Vec::new(),
        }
    }

    /// Tools the current mat needs, in plot order, without repeats. Pens follow the mat's item order.
    pub fn tool_loads(&self) -> Vec<ToolLoad> {
        let (Some(material), Some(mat)) = (self.material, self.mat()) else { return Vec::new() };
        let mut loads = Vec::new();
        for &op in mat.operations.iter().filter(|op| op.art_type().is_some()) {
            let (tool, clamp) = op.load(material);
            let styles = mat.items.iter().filter_map(|i| self.design.piece_style(i.piece));
            for pen in styles.filter(|s| s.operation == op).map(|s| (op == Operation::Pen).then_some(s.pen)) {
                let load = ToolLoad { tool, clamp, pen };
                if !loads.contains(&load) {
                    loads.push(load);
                }
            }
        }
        loads
    }

    pub fn tools_loaded(&self) -> bool {
        use DeviceState as D;
        !matches!(self.device_state, D::SelectDevice | D::DeviceConnected | D::WaitingOnMaterial | D::RetrievingPressureSettings | D::DetectingTool)
    }

    pub fn set_load_type(&mut self, load_type: LoadType) {
        self.setup.load_type = load_type;
        let sizes = self.mat_sizes();
        if !sizes.contains(&self.setup.mat_size) {
            self.set_mat_size(sizes[0]);
        }
    }

    /// Recomposes the mats, since packing depends on the mat size.
    pub fn set_mat_size(&mut self, size: MatSize) {
        self.setup.mat_size = size;
        self.recompose();
    }

    /// Recomposes the mats, since every copy packs onto them.
    pub fn set_copies(&mut self, copies: u32) {
        self.setup.copies = copies;
        self.recompose();
    }

    pub fn with_paper(mut self, paper: PaperSize) -> Self {
        self.set_paper(paper);
        self
    }

    /// Recomposes the mats, since the paper decides how many images fit a page.
    pub fn set_paper(&mut self, paper: PaperSize) {
        self.print.paper = paper;
        self.recompose();
    }

    /// Takes the canvas design after an edit made from Prepare, such as Optimise layout, and recomposes.
    pub fn replace_design(&mut self, design: Design) {
        self.design = design;
        self.recompose();
    }

    fn recompose(&mut self) {
        self.mats = compose_on(&self.design, self.setup.mat_size, self.setup.copies, self.print.paper);
        self.skipped_print_then_cut = 0;
        if self.profile().is_some() {
            let before = self.mats.len();
            self.mats.retain(|m| !m.print_then_cut);
            self.skipped_print_then_cut = before - self.mats.len();
        }
        self.current = self.current.min(self.mats.len().saturating_sub(1));
    }

    pub fn mat(&self) -> Option<&Mat> {
        self.mats.get(self.current)
    }

    pub fn has_next_mat(&self) -> bool {
        self.current + 1 < self.mats.len()
    }

    /// Roll sizes for matless loading, mats otherwise; families without roll sizes show every mat.
    pub fn mat_sizes(&self) -> Vec<MatSize> {
        if let Some(p) = self.profile() {
            return vec![work_area(p)];
        }
        let all = self.family.mat_sizes();
        let matless = self.setup.load_type.mat_mode() == crate::machine::MatMode::MatLess;
        let fitting: Vec<MatSize> = all.iter().copied().filter(|m| m.matless == matless).collect();
        if fitting.is_empty() { all.to_vec() } else { fitting }
    }

    pub fn continue_from_prepare(&mut self) -> Vec<Effect> {
        self.current = 0;
        self.start_mat()
    }

    /// After the current mat is unloaded, starts a fresh `riMATCUT` interaction for the following mat.
    pub fn next_mat(&mut self) -> Vec<Effect> {
        if !self.has_next_mat() {
            return Vec::new();
        }
        self.current += 1;
        self.progress = None;
        self.start_mat()
    }

    /// A Print Then Cut page stops at Print; every other mat goes straight to the machine. Open devices
    /// never see a page, since their Print Then Cut mats are skipped.
    fn start_mat(&mut self) -> Vec<Effect> {
        if self.mat().is_some_and(|m| m.page.is_some()) {
            self.step = MakeStep::Print;
            return Vec::new();
        }
        self.printed()
    }

    /// Starts the current mat once its page, if any, is printed. An open device goes straight to Set once
    /// its connection test has passed; it has no interaction to start and waits on nothing but the user.
    pub fn printed(&mut self) -> Vec<Effect> {
        self.sent = None;
        self.tool_change = None;
        if self.profile().is_some() {
            self.step = match self.test {
                TestState::Passed(_) => MakeStep::Set,
                _ => MakeStep::ConnectMachine(ConnectPhase::Setup),
            };
            self.device_state = DeviceState::WaitingOnMaterial;
            return Vec::new();
        }
        self.step = match self.machine {
            Some(_) => MakeStep::Set,
            None => MakeStep::ConnectMachine(ConnectPhase::Listing),
        };
        self.devices.clear();
        self.device_state = DeviceState::SelectDevice;
        bridge([BridgeMessage { interaction: Some(S::MatCut), ..BridgeMessage::new(S::MatCut) }])
    }

    pub fn connect(&mut self, device: &DeviceInfo) -> Vec<Effect> {
        self.missing = None;
        self.step = MakeStep::ConnectMachine(ConnectPhase::Connecting(device.connection_type));
        let mut out = vec![Effect::UseDevice(None)];
        out.extend(bridge([self.select_device(device)]));
        out
    }

    pub fn dismiss_machine_modal(&mut self) {
        self.step = MakeStep::Prepare;
    }

    pub fn finish_connecting(&mut self) {
        self.step = MakeStep::Set;
    }

    pub fn confirm_material(&mut self) -> Vec<Effect> {
        if self.material.is_none() {
            return Vec::new();
        }
        if self.device_state != DeviceState::WaitingOnMaterial {
            return Vec::new();
        }
        self.step = MakeStep::Load;
        if self.profile().is_some() {
            self.device_state = DeviceState::WaitingOnLoad;
            return Vec::new();
        }
        let payload = bridge::build_material_selected(&self.setup);
        bridge([BridgeMessage { material_selected_payload: Some(payload), ..self.outgoing(S::MaterialSelected) }])
    }

    /// An open device has no Load/Unload button: the user loads by hand and presses Continue, and after a
    /// cut removes the material and says so.
    pub fn press_load_unload(&mut self) -> Vec<Effect> {
        if self.profile().is_none() {
            return bridge([self.outgoing(S::MatCutSimulateLoadButtonPressed)]);
        }
        match self.step {
            MakeStep::Load => {
                self.step = MakeStep::Go;
                self.device_state = DeviceState::ReadyToGo;
            }
            MakeStep::Done { phase: DonePhase::Unloading, .. } => {
                self.device_state = DeviceState::Unloaded;
                self.mark_unloaded();
            }
            _ => {}
        }
        Vec::new()
    }

    /// The only way an open device's job starts: on the Go step, ready to go.
    pub fn press_go(&mut self) -> Vec<Effect> {
        if self.profile().is_none() {
            return bridge([self.outgoing(S::MatCutSimulateGoButtonPressed)]);
        }
        if (self.step, self.device_state) != (MakeStep::Go, DeviceState::ReadyToGo) {
            return Vec::new();
        }
        let Some(job) = self.job() else { return Vec::new() };
        self.step = MakeStep::Cutting;
        self.device_state = DeviceState::GoPressed;
        self.sent = Some((0, job.lines.len()));
        vec![Effect::Device(DeviceCommand::Start(job))]
    }

    pub fn press_pause(&self) -> Vec<Effect> {
        if self.profile().is_none() {
            return bridge([self.outgoing(S::MatCutSimulatePauseButtonPressed)]);
        }
        match (self.step, self.device_state) {
            (MakeStep::Cutting, DeviceState::GoPressed) => vec![Effect::Device(DeviceCommand::Pause)],
            (MakeStep::Cutting, DeviceState::Paused) => vec![Effect::Device(DeviceCommand::Resume)],
            _ => Vec::new(),
        }
    }

    pub fn cancel(&mut self) -> Vec<Effect> {
        if self.profile().is_none() {
            return bridge([self.outgoing(S::MatCutAbortCut)]);
        }
        let running = self.step == MakeStep::Cutting;
        let (phase, state) = match self.step {
            MakeStep::Go | MakeStep::Cutting => (DonePhase::Unloading, DeviceState::WaitingOnUnload),
            _ => (DonePhase::Unloaded, DeviceState::Unloaded),
        };
        self.finish(Outcome::Cancelled, phase);
        self.device_state = state;
        self.tool_change = None;
        match running {
            true => vec![Effect::Device(DeviceCommand::Cancel)],
            false => Vec::new(),
        }
    }

    /// Whether the open device is running a job, which closing the flow must cancel first.
    pub fn job_running(&self) -> bool {
        self.profile().is_some() && self.step == MakeStep::Cutting
    }

    pub fn looks_good(&mut self) {
        if let MakeStep::Done { outcome, .. } = self.step {
            self.step = MakeStep::Done { outcome, phase: DonePhase::Unloading };
        }
    }

    /// An open device runs the same job again over material that never moved.
    pub fn cut_more(&mut self) -> Vec<Effect> {
        if self.profile().is_some() {
            self.step = MakeStep::Go;
            self.device_state = DeviceState::ReadyToGo;
        }
        self.press_go()
    }

    /// After a finished cut the machine still holds the material and tools, so only the paths go again. A
    /// cancel may have come before either was sent, so the mat starts over from Set, its page printed already.
    /// An open device keeps nothing between jobs, so it always starts over from Set.
    pub fn rerun(&mut self) -> Vec<Effect> {
        self.progress = None;
        match self.step {
            MakeStep::Done { outcome: Outcome::Complete, .. } if self.profile().is_none() => {
                self.step = MakeStep::Load;
                bridge(self.path_data_message())
            }
            _ => self.printed(),
        }
    }

    pub fn lose_machine(&mut self) {
        self.step = MakeStep::Lost;
        self.device_state = DeviceState::MachineLostConnection;
    }

    /// The open device's counterpart of `on_message`.
    pub fn on_device_event(&mut self, event: DeviceEvent) {
        match event {
            DeviceEvent::TestPassed(words) => self.test = TestState::Passed(words),
            DeviceEvent::TestFailed(words) => self.test = TestState::Failed(words),
            _ if self.step != MakeStep::Cutting => {}
            DeviceEvent::Progress { sent, total } => self.sent = Some((sent, total)),
            DeviceEvent::Paused => self.device_state = DeviceState::Paused,
            DeviceEvent::Resumed => {
                self.device_state = DeviceState::GoPressed;
                self.tool_change = None;
            }
            DeviceEvent::ToolChange(words) => {
                self.device_state = DeviceState::Paused;
                self.tool_change = Some(words);
            }
            DeviceEvent::Done => {
                self.finish(Outcome::Complete, DonePhase::Checking);
                self.device_state = DeviceState::WaitingOnUnload;
            }
            DeviceEvent::Failed(words) => {
                self.failure = Some(words);
                self.lose_machine();
            }
        }
    }

    pub fn on_message(&mut self, msg: &BridgeMessage) -> Vec<Effect> {
        if self.step == MakeStep::Lost {
            return Vec::new();
        }
        self.device_state = device::reduce(self.device_state, msg);
        self.handle = msg.handle.or(self.handle);
        match msg.status {
            S::SingleDeviceConnected => return bridge(self.on_device_list(&msg.device_list)),
            S::DeviceOpenSuccess => self.on_device_open(msg.device.clone()),
            S::SendToolArray => return bridge(self.tool_info_message()),
            S::MatCutNeedPathData => return bridge(self.path_data_message()),
            S::WaitOnGo if self.step == MakeStep::Load => self.step = MakeStep::Go,
            S::GoPressed => self.step = MakeStep::Cutting,
            S::MatCutSetProgress => self.progress = msg.progress.clone(),
            S::MatCutCompleteSuccess => self.finish(Outcome::Complete, DonePhase::Checking),
            S::MatCutAbortCut => self.finish(Outcome::Cancelled, DonePhase::Unloading),
            S::MatUnloaded | S::Complete => self.mark_unloaded(),
            _ => {}
        }
        Vec::new()
    }

    fn on_device_list(&mut self, list: &[DeviceInfo]) -> Vec<BridgeMessage> {
        self.devices = list.to_vec();
        self.devices.sort_by_key(|d| d.connection_type != ConnectionType::Usb);
        let Some(remembered) = self.machine.as_ref().map(|m| m.serial.clone()) else { return Vec::new() };
        match self.devices.iter().find(|d| d.serial == remembered) {
            Some(d) => vec![self.select_device(d)],
            None => {
                self.missing = self.machine.take();
                self.step = MakeStep::ConnectMachine(ConnectPhase::Listing);
                Vec::new()
            }
        }
    }

    fn on_device_open(&mut self, device: Option<DeviceInfo>) {
        if let Some(d) = &device {
            self.family = d.machine_family;
        }
        self.machine = device.or(self.machine.take());
        if let MakeStep::ConnectMachine(ConnectPhase::Connecting(_)) = self.step {
            self.step = MakeStep::ConnectMachine(ConnectPhase::Connected);
        }
    }

    fn finish(&mut self, outcome: Outcome, phase: DonePhase) {
        self.step = MakeStep::Done { outcome, phase };
    }

    fn mark_unloaded(&mut self) {
        if let MakeStep::Done { outcome, .. } = self.step {
            self.step = MakeStep::Done { outcome, phase: DonePhase::Unloaded };
        }
    }

    fn select_device(&self, device: &DeviceInfo) -> BridgeMessage {
        BridgeMessage { device: Some(device.clone()), ..self.outgoing(S::SelectDevice) }
    }

    fn tool_info_message(&self) -> Option<BridgeMessage> {
        let tools = bridge::build_selected_tools(self.mat()?, self.material?, self.pressure, &self.setup);
        Some(BridgeMessage { device: self.machine.clone(), tool_info: Some(tools), ..self.outgoing(S::ToolInfoReceived) })
    }

    fn path_data_message(&self) -> Option<BridgeMessage> {
        let paths = bridge::build_mat_path_data(&self.design, self.mat()?, &self.setup, self.material?);
        Some(BridgeMessage { mat_path_data: Some(paths), ..self.outgoing(S::MatCutSetPathData) })
    }

    fn outgoing(&self, status: S) -> BridgeMessage {
        BridgeMessage { handle: self.handle, interaction: Some(S::MatCut), ..BridgeMessage::new(status) }
    }
}

/// An open device's work area as a matless sheet, which `compose` packs like any mat.
fn work_area(profile: &DeviceProfile) -> MatSize {
    let (width_in, height_in) = profile.work_area_in();
    MatSize { imperial_name: "Work area", metric_name: "Work area", width_in, height_in, preferred: true, matless: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::device::DeviceState;
    use crate::material::MATERIALS;
    use crate::pens::PenSize;

    fn pen(name: &str) -> Pen {
        Pen::named(name, PenSize::Fine)
    }

    /// A Pen square per pen, with a cut circle after the first so pens and blade interleave in z-order,
    /// all attached so they share one mat.
    fn flow_with_pens(pens: &[Pen]) -> MakeFlow {
        let mut design = Design::default();
        for (i, &p) in pens.iter().enumerate() {
            let id = design.add_shape(ShapeKind::Square);
            let s = design.shape_mut(id).unwrap();
            s.set_operation(Operation::Pen);
            s.set_pen(p);
            if i == 0 {
                design.add_shape(ShapeKind::Circle);
            }
        }
        let ids: Vec<u64> = design.shapes.iter().map(|s| s.id).collect();
        design.attach(&ids);
        let mut flow = MakeFlow::new(design, None);
        flow.material = Some(&MATERIALS[0]);
        flow
    }

    #[test]
    fn pens_of_different_colours_are_separate_loads() {
        let labels: Vec<String> = flow_with_pens(&[pen("Black"), pen("Wine")]).tool_loads().iter().map(ToolLoad::label).collect();
        assert_eq!(labels, ["Load Black Fine Point Pen in Clamp A", "Load Wine Fine Point Pen in Clamp A", "Load Fine-Point Blade in Clamp B"]);
    }

    #[test]
    fn pens_of_one_size_and_colour_load_once() {
        let loads = flow_with_pens(&[pen("Black"), pen("Black")]).tool_loads();
        assert_eq!(loads.iter().filter(|l| l.tool == Tool::Pen).count(), 1);
    }

    #[test]
    fn the_same_colour_in_another_size_is_another_pen() {
        let bold = Pen { size: PenSize::Bold, ..pen("Black") };
        let loads = flow_with_pens(&[pen("Black"), bold]).tool_loads();
        assert_eq!(loads.iter().filter(|l| l.tool == Tool::Pen).count(), 2);
    }

    #[test]
    fn each_mat_loads_only_its_own_tools_and_next_mat_restarts_the_interaction() {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Square);
        let score = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(score).unwrap();
        s.set_operation(Operation::Score);
        s.fill = egui::Color32::WHITE;
        let mut flow = MakeFlow::new(design, Some(crate::device::fake_devices().remove(0)));
        flow.material = Some(&MATERIALS[0]);
        assert_eq!(flow.mats.len(), 2);

        let tools = |flow: &MakeFlow| flow.tool_loads().iter().map(|l| l.tool).collect::<Vec<_>>();
        assert_eq!(tools(&flow), [Tool::DragKnife]);
        assert!(flow.has_next_mat());

        let sent = flow.next_mat();
        let statuses: Vec<S> = sent.iter().filter_map(|e| if let Effect::Bridge(m) = e { Some(m.status) } else { None }).collect();
        assert_eq!(statuses, [S::MatCut]);
        assert_eq!((flow.current, flow.step), (1, MakeStep::Set));
        assert_eq!(tools(&flow), [Tool::ScoringWheel]);
        assert!(!flow.has_next_mat());
        assert!(flow.next_mat().is_empty(), "the flow ends after the last mat");
    }

    /// The flow wired to a thread-free machine, so each transition runs to completion inside the test.
    struct Rig {
        flow: MakeFlow,
        machine: device::Loopback,
    }

    impl Rig {
        fn new(design: Design) -> Self {
            let mut flow = MakeFlow::new(design, Some(device::fake_devices().remove(0)));
            flow.material = Some(&MATERIALS[0]);
            Self { flow, machine: device::Loopback::new() }
        }

        fn square() -> Self {
            let mut design = Design::default();
            design.add_shape(ShapeKind::Square);
            Self::new(design)
        }

        /// Sends `out`, then hands every reply to the flow until the machine goes quiet or `until` holds.
        fn run(&mut self, out: Vec<Effect>, until: impl Fn(&MakeFlow) -> bool) {
            self.send(out);
            while !until(&self.flow) {
                let Some(msg) = self.machine.recv() else { return };
                let out = self.flow.on_message(&msg);
                self.send(out);
            }
        }

        fn send(&mut self, out: Vec<Effect>) {
            for effect in out {
                if let Effect::Bridge(m) = effect {
                    self.machine.send(&m);
                }
            }
        }

        fn settle(&mut self, out: Vec<Effect>) {
            self.run(out, |_| false);
        }

        fn at(&self) -> (MakeStep, DeviceState) {
            (self.flow.step, self.flow.device_state)
        }

        fn reach_set(&mut self) {
            let out = self.flow.continue_from_prepare();
            self.settle(out);
            assert_eq!(self.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial));
        }

        fn reach_load(&mut self) {
            let out = self.flow.confirm_material();
            self.settle(out);
            assert_eq!(self.at(), (MakeStep::Load, DeviceState::WaitingOnLoad));
        }

        fn reach_go(&mut self) {
            let out = self.flow.press_load_unload();
            self.settle(out);
            assert_eq!(self.at(), (MakeStep::Go, DeviceState::ReadyToGo));
        }

        fn cut(&mut self) {
            let out = self.flow.press_go();
            self.settle(out);
            assert_eq!(self.flow.step, done(Outcome::Complete, DonePhase::Checking));
        }

        fn cut_until(&mut self, percent: f64) {
            let out = self.flow.press_go();
            self.run(out, |f| f.progress.as_ref().is_some_and(|p| p.current_percentage >= percent));
            assert_eq!(self.at(), (MakeStep::Cutting, DeviceState::GoPressed));
        }

        fn unload(&mut self, outcome: Outcome) {
            let out = self.flow.press_load_unload();
            self.settle(out);
            assert_eq!(self.at(), (done(outcome, DonePhase::Unloaded), DeviceState::Unloaded));
        }

        fn cancel(&mut self) {
            let out = self.flow.cancel();
            self.settle(out);
        }

        fn sent_since(&self, n: usize) -> &[S] {
            &self.machine.sent[n..]
        }
    }

    fn done(outcome: Outcome, phase: DonePhase) -> MakeStep {
        MakeStep::Done { outcome, phase }
    }

    const MAKE_ONE_MAT: [S; 5] = [S::MatCut, S::SelectDevice, S::MaterialSelected, S::ToolInfoReceived, S::MatCutSetPathData];

    #[test]
    fn the_happy_path_sends_the_documented_sequence() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut();
        rig.flow.looks_good();
        rig.unload(Outcome::Complete);
        let presses = [S::MatCutSimulateLoadButtonPressed, S::MatCutSimulateGoButtonPressed, S::MatCutSimulateLoadButtonPressed];
        assert_eq!(rig.machine.sent, [&MAKE_ONE_MAT[..], &presses].concat());
    }

    #[test]
    fn cancel_in_set_ends_the_mat_unloaded() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.cancel();
        assert_eq!(rig.flow.step, done(Outcome::Cancelled, DonePhase::Unloaded));
    }

    #[test]
    fn cancel_in_load_ends_the_mat_unloaded() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.cancel();
        assert_eq!(rig.flow.step, done(Outcome::Cancelled, DonePhase::Unloaded));
    }

    #[test]
    fn cancel_in_go_asks_for_the_loaded_mat_to_be_unloaded() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cancel();
        assert_eq!(rig.at(), (done(Outcome::Cancelled, DonePhase::Unloading), DeviceState::WaitingOnUnload));
        rig.unload(Outcome::Cancelled);
    }

    #[test]
    fn cancel_while_cutting_stops_the_cut_and_asks_for_unload() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut_until(20.0);
        rig.cancel();
        assert_eq!(rig.at(), (done(Outcome::Cancelled, DonePhase::Unloading), DeviceState::WaitingOnUnload));
        assert!(rig.flow.progress.as_ref().is_some_and(|p| p.current_percentage < 100.0));
        rig.unload(Outcome::Cancelled);
    }

    #[test]
    fn pause_holds_the_cut_and_resume_finishes_it() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut_until(20.0);
        let out = rig.flow.press_pause();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Cutting, DeviceState::Paused));
        let out = rig.flow.press_pause();
        rig.settle(out);
        assert_eq!(rig.flow.step, done(Outcome::Complete, DonePhase::Checking));
    }

    #[test]
    fn cut_more_cuts_the_same_mat_again() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut();
        let n = rig.machine.sent.len();
        let out = rig.flow.cut_more();
        rig.run(out, |f| f.step == MakeStep::Cutting);
        assert_eq!(rig.at(), (MakeStep::Cutting, DeviceState::GoPressed));
        rig.settle(Vec::new());
        assert_eq!(rig.flow.step, done(Outcome::Complete, DonePhase::Checking));
        assert_eq!(rig.sent_since(n), [S::MatCutSimulateGoButtonPressed]);
    }

    #[test]
    fn rerun_after_a_complete_cut_resends_only_the_paths() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut();
        rig.flow.looks_good();
        rig.unload(Outcome::Complete);
        let n = rig.machine.sent.len();
        let out = rig.flow.rerun();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Load, DeviceState::WaitingOnLoad));
        assert_eq!(rig.sent_since(n), [S::MatCutSetPathData]);
        rig.reach_go();
        rig.cut();
    }

    /// AUDIT-020: the machine never received material or tools, so Rerun must start the mat again from Set.
    #[test]
    fn rerun_after_cancel_in_set_resends_material_and_tools() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.cancel();
        let n = rig.machine.sent.len();
        let out = rig.flow.rerun();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial));
        rig.reach_load();
        assert_eq!(rig.sent_since(n), MAKE_ONE_MAT);
        rig.reach_go();
        rig.cut();
    }

    #[test]
    fn rerun_after_cancel_with_no_material_never_strands_in_load() {
        let mut rig = Rig::square();
        rig.flow.material = None;
        rig.reach_set();
        rig.cancel();
        let out = rig.flow.rerun();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial));
        rig.flow.material = Some(&MATERIALS[0]);
        rig.reach_load();
    }

    #[test]
    fn rerun_after_cancel_mid_cut_starts_the_mat_again() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cut_until(20.0);
        rig.cancel();
        rig.unload(Outcome::Cancelled);
        let out = rig.flow.rerun();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial));
        assert!(rig.flow.progress.is_none(), "the cancelled cut's progress is gone");
    }

    #[test]
    fn next_mat_after_cancel_makes_the_following_mat() {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Square);
        let red = design.add_shape(ShapeKind::Circle);
        design.shape_mut(red).unwrap().fill = egui::Color32::RED;
        let mut rig = Rig::new(design);
        assert_eq!(rig.flow.mats.len(), 2);
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.cancel();
        rig.unload(Outcome::Cancelled);
        let n = rig.machine.sent.len();
        let out = rig.flow.next_mat();
        rig.settle(out);
        assert_eq!((rig.flow.current, rig.at()), (1, (MakeStep::Set, DeviceState::WaitingOnMaterial)));
        rig.reach_load();
        assert_eq!(rig.sent_since(n), MAKE_ONE_MAT);
        rig.reach_go();
        rig.cut();
    }

    #[test]
    fn losing_the_machine_ends_the_flow_and_late_replies_do_not_revive_it() {
        let mut rig = Rig::square();
        rig.reach_set();
        rig.reach_load();
        rig.reach_go();
        rig.flow.lose_machine();
        assert_eq!(rig.at(), (MakeStep::Lost, DeviceState::MachineLostConnection));
        rig.flow.on_message(&BridgeMessage::new(S::WaitOnGo));
        assert_eq!(rig.flow.step, MakeStep::Lost, "a stray reply does not revive the flow");
    }

    #[test]
    fn a_remembered_machine_missing_from_the_list_opens_the_machine_picker() {
        let gone = DeviceInfo { serial: "GONE-0001".into(), ..device::fake_devices().remove(0) };
        let mut rig = Rig::square();
        rig.flow.machine = Some(gone.clone());
        let out = rig.flow.continue_from_prepare();
        rig.settle(out);
        assert_eq!(rig.flow.step, MakeStep::ConnectMachine(ConnectPhase::Listing));
        assert_eq!(rig.flow.missing.as_ref().map(|d| &d.serial), Some(&gone.serial));
        assert_eq!(rig.flow.devices.len(), 2, "the machines that did answer are offered");
        let usb = rig.flow.devices[0].clone();
        let out = rig.flow.connect(&usb);
        rig.settle(out);
        assert!(rig.flow.missing.is_none());
        assert_eq!(rig.flow.step, MakeStep::ConnectMachine(ConnectPhase::Connected));
    }

    #[test]
    fn changing_the_mat_size_recomposes_the_mats() {
        let mut design = Design::default();
        for _ in 0..12 {
            design.add_shape(ShapeKind::Square);
        }
        let mut flow = MakeFlow::new(design, None);
        assert_eq!(flow.mats.len(), 1);
        let small = *flow.mat_sizes().iter().find(|m| m.height_in <= 6.0).expect("a small mat");
        flow.set_mat_size(small);
        assert!(flow.mats.len() > 1, "{} mats", flow.mats.len());
    }

    fn print_then_cut_square(design: &mut Design, size_in: f32) {
        let id = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(id).unwrap();
        (s.width_in, s.height_in) = (size_in, size_in);
        s.set_operation(Operation::PrintThenCut);
    }

    /// A Print Then Cut square on a page, then a cut square on a mat.
    fn page_then_mat() -> Rig {
        let mut design = Design::default();
        print_then_cut_square(&mut design, 2.0);
        design.add_shape(ShapeKind::Square);
        let rig = Rig::new(design);
        assert_eq!(rig.flow.mats.iter().map(|m| m.page.is_some()).collect::<Vec<_>>(), [true, false]);
        rig
    }

    #[test]
    fn a_print_then_cut_page_stops_at_print_and_printed_starts_the_machine() {
        let mut rig = page_then_mat();
        let out = rig.flow.continue_from_prepare();
        assert!(out.is_empty(), "nothing reaches the machine before the page is printed");
        assert_eq!((rig.flow.step, rig.flow.step.slg_number()), (MakeStep::Print, Some(0)));
        let out = rig.flow.printed();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial));
        rig.reach_load();
        rig.reach_go();
        rig.cut();
        rig.flow.looks_good();
        rig.unload(Outcome::Complete);
        assert_eq!(rig.machine.sent[..MAKE_ONE_MAT.len()], MAKE_ONE_MAT);
        let out = rig.flow.next_mat();
        rig.settle(out);
        assert_eq!((rig.flow.current, rig.at()), (1, (MakeStep::Set, DeviceState::WaitingOnMaterial)), "a cut mat never shows Print");
    }

    #[test]
    fn printed_without_a_machine_opens_the_machine_picker() {
        let mut design = Design::default();
        print_then_cut_square(&mut design, 2.0);
        let mut flow = MakeFlow::new(design, None);
        flow.continue_from_prepare();
        assert_eq!(flow.step, MakeStep::Print);
        let sent = flow.printed();
        assert!(matches!(&sent[..], [Effect::Bridge(m)] if m.status == S::MatCut), "{sent:?}");
        assert_eq!(flow.step, MakeStep::ConnectMachine(ConnectPhase::Listing));
    }

    #[test]
    fn rerun_of_a_cancelled_page_skips_print() {
        let mut rig = page_then_mat();
        rig.flow.continue_from_prepare();
        let out = rig.flow.printed();
        rig.settle(out);
        rig.cancel();
        let out = rig.flow.rerun();
        rig.settle(out);
        assert_eq!(rig.at(), (MakeStep::Set, DeviceState::WaitingOnMaterial), "the sheet is printed already");
    }

    #[test]
    fn a_smaller_paper_recomposes_onto_more_pages() {
        let mut design = Design::default();
        for _ in 0..4 {
            print_then_cut_square(&mut design, 3.25);
        }
        let mut flow = MakeFlow::new(design, None);
        assert_eq!(flow.mats.len(), 1, "two rows of two fit Letter");
        flow.set_paper(crate::print_then_cut::A4);
        assert_eq!(flow.mats.len(), 2, "A4 is too narrow for two side by side");
        assert!(flow.mats.iter().all(|m| m.page.is_some_and(|p| p.paper == crate::print_then_cut::A4)));
    }

    #[test]
    fn more_copies_spill_onto_more_mats() {
        let mut rig = Rig::square();
        assert_eq!(rig.flow.mats.len(), 1);
        rig.flow.set_copies(30);
        assert_eq!(rig.flow.mats.iter().map(|m| m.items.len()).collect::<Vec<_>>(), [25, 5]);
        rig.flow.current = 1;
        rig.flow.set_copies(1);
        assert_eq!((rig.flow.mats.len(), rig.flow.current), (1, 0));
    }

    mod open_device {
        use super::*;
        use crate::output::{DeviceCommand, DeviceEvent, DeviceProfile, OutputDevice};

        fn plotter() -> MakeFlow {
            let mut design = Design::default();
            design.add_shape(ShapeKind::Square);
            let mut flow = MakeFlow::new(design, None).on(OutputDevice::Open(DeviceProfile::grbl_pen_plotter()));
            flow.material = Some(&MATERIALS[1]);
            flow
        }

        fn commands(out: &[Effect]) -> Vec<&DeviceCommand> {
            out.iter().filter_map(|e| if let Effect::Device(c) = e { Some(c) } else { None }).collect()
        }

        /// From Prepare through a passed test to the Go step, checking no motion leaves on the way.
        fn at_go() -> MakeFlow {
            let mut flow = plotter();
            assert!(flow.continue_from_prepare().is_empty());
            assert_eq!(flow.step, MakeStep::ConnectMachine(ConnectPhase::Setup));
            let test = flow.test_connection();
            assert!(matches!(test[..], [Effect::Open(_), Effect::Device(DeviceCommand::Test)]));
            flow.on_device_event(DeviceEvent::TestPassed("Grbl 1.1h".into()));
            let out = flow.finish_setup();
            assert!(matches!(out[..], [Effect::UseDevice(Some(_))]), "the profile is remembered, nothing is sent");
            assert_eq!((flow.step, flow.device_state), (MakeStep::Set, DeviceState::WaitingOnMaterial));
            assert!(flow.confirm_material().is_empty());
            assert!(flow.press_load_unload().is_empty());
            assert_eq!((flow.step, flow.device_state), (MakeStep::Go, DeviceState::ReadyToGo));
            flow
        }

        #[test]
        fn composes_at_the_work_area_without_load_type() {
            let flow = plotter();
            assert_eq!(flow.setup.mat_size.imperial_name, "Work area");
            assert!((flow.setup.mat_size.width_in - 300.0 / 25.4).abs() < 1e-4);
            assert_eq!(flow.mat_sizes(), [flow.setup.mat_size]);
        }

        #[test]
        fn print_then_cut_mats_are_skipped() {
            let mut design = Design::default();
            design.add_shape(ShapeKind::Square);
            let ptc = design.add_shape(ShapeKind::Circle);
            design.shape_mut(ptc).unwrap().set_operation(Operation::PrintThenCut);
            let flow = MakeFlow::new(design, None).on(OutputDevice::Open(DeviceProfile::grbl_laser()));
            assert_eq!((flow.mats.len(), flow.skipped_print_then_cut), (1, 1));
            assert!(flow.mats.iter().all(|m| !m.print_then_cut));
        }

        #[test]
        fn go_is_the_only_way_a_job_starts() {
            let mut flow = plotter();
            flow.continue_from_prepare();
            assert!(flow.press_go().is_empty(), "no job before a passed test and the Go step");
            let mut flow = at_go();
            assert!(flow.cancel().is_empty(), "cancelling before Go sends nothing");
            let mut flow = at_go();
            let out = flow.press_go();
            assert!(matches!(commands(&out)[..], [DeviceCommand::Start(_)]));
            assert_eq!(flow.step, MakeStep::Cutting);
            assert!(flow.press_go().is_empty(), "a second Go starts nothing");
        }

        #[test]
        fn pause_resume_cancel_and_done_follow_the_device() {
            let mut flow = at_go();
            flow.press_go();
            assert!(matches!(commands(&flow.press_pause())[..], [DeviceCommand::Pause]));
            flow.on_device_event(DeviceEvent::Paused);
            assert!(matches!(commands(&flow.press_pause())[..], [DeviceCommand::Resume]));
            flow.on_device_event(DeviceEvent::Resumed);
            flow.on_device_event(DeviceEvent::Progress { sent: 3, total: 9 });
            assert_eq!(flow.sent, Some((3, 9)));
            assert!(flow.job_running());
            assert!(matches!(commands(&flow.cancel())[..], [DeviceCommand::Cancel]));
            assert_eq!(flow.step, MakeStep::Done { outcome: Outcome::Cancelled, phase: DonePhase::Unloading });
            flow.on_device_event(DeviceEvent::Done);
            assert_eq!(flow.step.slg_number(), None, "a late Done does not revive the cancelled mat");

            let mut flow = at_go();
            flow.press_go();
            flow.on_device_event(DeviceEvent::Done);
            assert_eq!(flow.step, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Checking });
            flow.looks_good();
            flow.press_load_unload();
            assert_eq!(flow.step, MakeStep::Done { outcome: Outcome::Complete, phase: DonePhase::Unloaded });
        }

        #[test]
        fn a_grbl_error_stops_the_flow_with_its_meaning() {
            let mut flow = at_go();
            flow.press_go();
            flow.on_device_event(DeviceEvent::Failed("error:20 Unsupported command".into()));
            assert_eq!(flow.step, MakeStep::Lost);
            assert_eq!(flow.failure.as_deref(), Some("error:20 Unsupported command"));
        }

        #[test]
        fn save_to_file_carries_the_same_job_go_would_start() {
            let mut flow = at_go();
            let Some(Effect::DryRun { job, mat: 1 }) = flow.save_to_file().pop() else { panic!("a dry run of mat 1") };
            let Some(Effect::Device(DeviceCommand::Start(started))) = flow.press_go().pop() else { panic!("a job") };
            assert_eq!(job, started);
        }

        #[test]
        fn change_machine_goes_back_to_the_simulator_list() {
            let mut flow = plotter();
            flow.choose_profile(DeviceProfile::grbl_laser());
            let out = flow.change_machine();
            assert_eq!(flow.device, OutputDevice::Simulator);
            assert_eq!(flow.step, MakeStep::ConnectMachine(ConnectPhase::Listing));
            assert!(matches!(out[..], [Effect::Bridge(_)]), "the simulator is asked for its machines");
        }
    }
}

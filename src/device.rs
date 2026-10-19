use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::bridge::{ArtType, BridgeMessage, BridgeStatus as S, CommonToolCut, DeviceInfo, InteractionHandle, Progress};
use crate::machine::{ConnectionType, MachineFamily};

/// The machine's device-state values.
#[allow(dead_code, reason = "mirrors the full device-state set; the simulator never reaches some states")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceState {
    SelectDevice,
    DeviceConnected,
    WaitingOnMaterial,
    RetrievingPressureSettings,
    DetectingTool,
    WaitingOnLoad,
    MeasuringMat,
    Prefeed,
    Loaded,
    ReadyToGo,
    GoPressed,
    ScanningFiducials,
    NeedTool,
    IncorrectToolDetected,
    ToolChanged,
    Paused,
    PrefeedPaused,
    WaitForEndMoveProgress,
    WaitingOnUnload,
    Unloaded,
    MachineLostConnection,
}

/// Statuses that do not move `deviceState` leave it unchanged.
pub fn reduce(state: DeviceState, msg: &BridgeMessage) -> DeviceState {
    use DeviceState as D;
    match msg.status {
        S::NoDeviceConnected | S::SingleDeviceConnected => D::SelectDevice,
        S::DeviceOpenSuccess => D::DeviceConnected,
        S::WaitingOnMaterialSelected => D::WaitingOnMaterial,
        S::MatCutGettingDevicePressureSettings => D::RetrievingPressureSettings,
        S::SendToolArray | S::DetectingTool => D::DetectingTool,
        S::MatCutNeedAccessoryChange => D::NeedTool,
        S::WaitOnMatLoad => D::WaitingOnLoad,
        S::MatLoaded => D::Loaded,
        S::WaitOnGo => D::ReadyToGo,
        S::GoPressed | S::DeviceResumed => D::GoPressed,
        S::PausePressed | S::DevicePaused => D::Paused,
        S::WaitOnMatUnload => D::WaitingOnUnload,
        S::MatUnloaded => D::Unloaded,
        _ => state,
    }
}

/// The machine's end has gone: its thread died or the link dropped. Nothing sent will be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disconnected;

pub trait Transport {
    fn send(&self, msg: BridgeMessage) -> Result<(), Disconnected>;
    /// `Ok(None)` when nothing has arrived yet.
    fn try_recv(&self) -> Result<Option<BridgeMessage>, Disconnected>;
}

/// Kerf's simulated machine on its own thread. It owns all machine state; the UI only sees channels.
pub struct SimulatedMachine {
    /// `None` is a tick, which only a stepped machine is sent.
    to_machine: Sender<Option<BridgeMessage>>,
    from_machine: Receiver<BridgeMessage>,
}

/// What moves a running cut on by one tick.
#[derive(Clone, Copy)]
enum Pace {
    /// Every `tick` of wall-clock time.
    Clock,
    /// Each `advance`, so a test decides how far the cut gets between frames.
    #[cfg(test)]
    Stepped,
}

impl SimulatedMachine {
    pub const PROGRESS_TICK: Duration = Duration::from_millis(100);

    pub fn spawn(tick: Duration) -> Self {
        Self::start(tick, Pace::Clock, Arc::default())
    }

    /// A machine whose cut moves on only at `advance`, reporting `tick` as each step's duration.
    #[cfg(test)]
    pub fn stepped(tick: Duration) -> Self {
        Self::start(tick, Pace::Stepped, Arc::default())
    }

    /// A stepped machine whose thread exits when the returned function is called, as if it had crashed.
    #[cfg(test)]
    pub fn stepped_killable(tick: Duration) -> (Self, impl FnOnce()) {
        let stop = Arc::new(AtomicBool::new(false));
        let machine = Self::start(tick, Pace::Stepped, stop.clone());
        let wake = machine.to_machine.clone();
        let kill = move || {
            stop.store(true, Ordering::SeqCst);
            let _ = wake.send(None);
        };
        (machine, kill)
    }

    fn start(tick: Duration, pace: Pace, stop: Arc<AtomicBool>) -> Self {
        let (to_machine, inbox) = mpsc::channel();
        let (outbox, from_machine) = mpsc::channel();
        thread::Builder::new().name("simulated-machine".into()).spawn(move || run(&inbox, &outbox, tick, pace, &stop)).expect("spawn simulated machine thread");
        Self { to_machine, from_machine }
    }

    /// Moves a running, unpaused cut on by one tick of a stepped machine, after anything already sent.
    #[cfg(test)]
    pub fn advance(&self) {
        _ = self.to_machine.send(None);
    }

    /// Blocks for the next reply, so a test waits on the machine rather than on a clock.
    #[cfg(test)]
    pub fn recv_timeout(&self, timeout: Duration) -> Result<BridgeMessage, RecvTimeoutError> {
        self.from_machine.recv_timeout(timeout)
    }
}

impl Transport for SimulatedMachine {
    fn send(&self, msg: BridgeMessage) -> Result<(), Disconnected> {
        self.to_machine.send(Some(msg)).map_err(|_| Disconnected)
    }

    fn try_recv(&self) -> Result<Option<BridgeMessage>, Disconnected> {
        match self.from_machine.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Disconnected),
        }
    }
}

pub fn fake_devices() -> Vec<DeviceInfo> {
    let device = |connection_type| DeviceInfo {
        device_type: "Kerf Studio Pro".into(),
        serial: "SIM-STUDIOPRO-0001".into(),
        connection_type,
        bluetooth_name: "Kerf Studio Pro-SIM".into(),
        allows_virtual_buttons: true,
        machine_family: MachineFamily::StudioPro,
    };
    vec![device(ConnectionType::Usb), device(ConnectionType::Bluetooth)]
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    AwaitDevice,
    AwaitMaterial,
    AwaitTools,
    AwaitPath,
    AwaitLoad,
    AwaitGo,
    Cutting { percent: u32, ticks: u32, paused: bool },
    AwaitUnload,
    Finished,
}

const PERCENT_STEP: u32 = 2;

struct Machine {
    phase: Phase,
    passes: u32,
    tick: Duration,
}

fn run(inbox: &Receiver<Option<BridgeMessage>>, outbox: &Sender<BridgeMessage>, tick: Duration, pace: Pace, stop: &AtomicBool) {
    let mut machine = Machine { phase: Phase::Idle, passes: 1, tick };
    loop {
        let event = next_event(inbox, machine.phase, tick, pace);
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let replies = match event {
            Ok(Some(msg)) => machine.on_message(&msg),
            Ok(None) => machine.on_tick(),
            Err(()) => return,
        };
        for reply in replies {
            if outbox.send(reply).is_err() {
                return;
            }
        }
    }
}

/// `Ok(None)` is a progress tick; `Err` means the UI hung up.
fn next_event(inbox: &Receiver<Option<BridgeMessage>>, phase: Phase, tick: Duration, pace: Pace) -> Result<Option<BridgeMessage>, ()> {
    match (phase, pace) {
        (Phase::Cutting { paused: false, .. }, Pace::Clock) => match inbox.recv_timeout(tick) {
            Ok(event) => Ok(event),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        },
        _ => inbox.recv().map_err(|_| ()),
    }
}

fn reply(status: S) -> BridgeMessage {
    let handle = InteractionHandle { current_interaction: S::MatCut };
    BridgeMessage { handle: Some(handle), ..BridgeMessage::new(status) }
}

impl Machine {
    fn on_message(&mut self, msg: &BridgeMessage) -> Vec<BridgeMessage> {
        let (next, replies) = match (self.phase, msg.status) {
            (_, S::MatCut) => (Phase::AwaitDevice, vec![self.device_list()]),
            (_, S::MatCutAbortCut) => self.abort(),
            (Phase::AwaitDevice, S::SelectDevice) => (Phase::AwaitMaterial, open_device(msg)),
            (Phase::AwaitMaterial, S::MaterialSelected) => (Phase::AwaitTools, vec![reply(S::MatCutGettingDevicePressureSettings), reply(S::SendToolArray)]),
            (Phase::AwaitTools, S::ToolInfoReceived) => {
                self.passes = requested_passes(msg);
                (Phase::AwaitPath, vec![reply(S::MatCutNeedPathData)])
            }
            (Phase::AwaitPath | Phase::Finished, S::MatCutSetPathData) => (Phase::AwaitLoad, vec![reply(S::WaitOnMatLoad)]),
            (Phase::AwaitLoad, S::MatCutSimulateLoadButtonPressed) => (Phase::AwaitGo, vec![reply(S::MatLoaded), reply(S::WaitOnGo)]),
            (Phase::AwaitGo | Phase::AwaitUnload, S::MatCutSimulateGoButtonPressed) => {
                (Phase::Cutting { percent: 0, ticks: 0, paused: false }, vec![reply(S::GoPressed)])
            }
            (Phase::Cutting { percent, ticks, paused }, S::MatCutSimulatePauseButtonPressed) => {
                let status = if paused { S::DeviceResumed } else { S::DevicePaused };
                (Phase::Cutting { percent, ticks, paused: !paused }, vec![reply(status)])
            }
            (Phase::AwaitUnload, S::MatCutSimulateLoadButtonPressed) => (Phase::Finished, vec![reply(S::MatUnloaded), reply(S::Complete)]),
            (phase, _) => (phase, Vec::new()),
        };
        self.phase = next;
        replies
    }

    fn on_tick(&mut self) -> Vec<BridgeMessage> {
        let Phase::Cutting { percent, ticks, paused: false } = self.phase else { return Vec::new() };
        if percent > 100 {
            self.phase = Phase::AwaitUnload;
            return vec![reply(S::MatCutCompleteSuccess), reply(S::WaitOnMatUnload)];
        }
        self.phase = Phase::Cutting { percent: percent + PERCENT_STEP, ticks: ticks + 1, paused: false };
        vec![self.progress(percent, ticks)]
    }

    fn abort(&self) -> (Phase, Vec<BridgeMessage>) {
        match self.phase {
            Phase::AwaitGo | Phase::Cutting { .. } | Phase::AwaitUnload => (Phase::AwaitUnload, vec![reply(S::MatCutAbortCut), reply(S::WaitOnMatUnload)]),
            _ => (Phase::Finished, vec![reply(S::MatCutAbortCut), reply(S::Complete)]),
        }
    }

    fn device_list(&self) -> BridgeMessage {
        BridgeMessage { device_list: fake_devices(), ..reply(S::SingleDeviceConnected) }
    }

    fn progress(&self, percent: u32, ticks: u32) -> BridgeMessage {
        let remaining_ticks = (100 - percent) / PERCENT_STEP;
        let secs = |n: u32| (self.tick * n).as_secs_f64();
        let cut = CommonToolCut { pct: percent.into(), total_num_passes: self.passes, current_pass: (percent * self.passes / 100 + 1).min(self.passes) };
        let progress = Progress {
            cut,
            current_operation: ArtType::Cut,
            current_percentage: percent.into(),
            estimated_seconds_left: secs(remaining_ticks),
            elapsed_seconds: secs(ticks),
        };
        BridgeMessage { progress: Some(progress), ..reply(S::MatCutSetProgress) }
    }
}

fn open_device(msg: &BridgeMessage) -> Vec<BridgeMessage> {
    let opened = BridgeMessage { device: msg.device.clone(), ..reply(S::DeviceOpenSuccess) };
    vec![opened, reply(S::WaitingOnMaterialSelected)]
}

fn requested_passes(msg: &BridgeMessage) -> u32 {
    let tools = msg.tool_info.as_ref().map(|t| t.tools.as_slice()).unwrap_or_default();
    tools.iter().map(|t| t.tool_from_api.precision_mode.multi_pass).max().unwrap_or(1).max(1)
}

/// The simulator's machine without its thread, for deterministic tests: each `send` answers at once, and
/// `recv` advances the cut by one tick whenever the machine has nothing else to say.
#[cfg(test)]
pub struct Loopback {
    machine: Machine,
    inbox: std::collections::VecDeque<BridgeMessage>,
    pub sent: Vec<S>,
}

#[cfg(test)]
impl Loopback {
    pub fn new() -> Self {
        Self { machine: Machine { phase: Phase::Idle, passes: 1, tick: Duration::ZERO }, inbox: Default::default(), sent: Vec::new() }
    }

    pub fn send(&mut self, msg: &BridgeMessage) {
        self.sent.push(msg.status);
        let replies = self.machine.on_message(msg);
        self.inbox.extend(replies);
    }

    pub fn recv(&mut self) -> Option<BridgeMessage> {
        if self.inbox.is_empty() && matches!(self.machine.phase, Phase::Cutting { paused: false, .. }) {
            self.inbox.extend(self.machine.on_tick());
        }
        self.inbox.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeviceState as D;

    fn recv(machine: &SimulatedMachine) -> BridgeMessage {
        machine.recv_timeout(Duration::from_secs(5)).expect("simulated machine went quiet")
    }

    fn respond(status: S) -> Option<BridgeMessage> {
        let next = match status {
            S::SingleDeviceConnected => S::SelectDevice,
            S::WaitingOnMaterialSelected => S::MaterialSelected,
            S::SendToolArray => S::ToolInfoReceived,
            S::MatCutNeedPathData => S::MatCutSetPathData,
            S::WaitOnMatLoad | S::WaitOnMatUnload => S::MatCutSimulateLoadButtonPressed,
            S::WaitOnGo => S::MatCutSimulateGoButtonPressed,
            _ => return None,
        };
        Some(BridgeMessage::new(next))
    }

    #[test]
    fn walks_the_happy_path() {
        let machine = SimulatedMachine::spawn(Duration::from_millis(1));
        machine.send(BridgeMessage { interaction: Some(S::MatCut), ..BridgeMessage::new(S::MatCut) }).unwrap();

        let mut state = D::SelectDevice;
        let mut states = vec![state];
        let mut percents = Vec::new();
        loop {
            let msg = recv(&machine);
            if let Some(p) = &msg.progress {
                percents.push(p.current_percentage);
            }
            state = reduce(state, &msg);
            if states.last() != Some(&state) {
                states.push(state);
            }
            if msg.status == S::Complete {
                break;
            }
            if let Some(next) = respond(msg.status) {
                machine.send(next).unwrap();
            }
        }

        let expected = [
            D::SelectDevice,
            D::DeviceConnected,
            D::WaitingOnMaterial,
            D::RetrievingPressureSettings,
            D::DetectingTool,
            D::WaitingOnLoad,
            D::Loaded,
            D::ReadyToGo,
            D::GoPressed,
            D::WaitingOnUnload,
            D::Unloaded,
        ];
        assert_eq!(states, expected);
        assert_eq!(percents.first(), Some(&0.0));
        assert_eq!(percents.last(), Some(&100.0));
    }

    fn machine(phase: Phase) -> Machine {
        Machine { phase, passes: 1, tick: Duration::ZERO }
    }

    fn statuses(replies: &[BridgeMessage]) -> Vec<S> {
        replies.iter().map(|r| r.status).collect()
    }

    const CUTTING: Phase = Phase::Cutting { percent: 40, ticks: 20, paused: false };
    const PAUSED: Phase = Phase::Cutting { percent: 40, ticks: 20, paused: true };
    const FROM_ZERO: Phase = Phase::Cutting { percent: 0, ticks: 0, paused: false };

    /// One row per README transition and per cancel, plus the presses a machine must ignore.
    #[test]
    fn every_phase_answers_every_input_as_documented() {
        use Phase as P;
        let rows: [(Phase, S, Phase, &[S]); 25] = [
            (P::Idle, S::MatCut, P::AwaitDevice, &[S::SingleDeviceConnected]),
            (CUTTING, S::MatCut, P::AwaitDevice, &[S::SingleDeviceConnected]),
            (P::AwaitDevice, S::SelectDevice, P::AwaitMaterial, &[S::DeviceOpenSuccess, S::WaitingOnMaterialSelected]),
            (P::AwaitMaterial, S::MaterialSelected, P::AwaitTools, &[S::MatCutGettingDevicePressureSettings, S::SendToolArray]),
            (P::AwaitTools, S::ToolInfoReceived, P::AwaitPath, &[S::MatCutNeedPathData]),
            (P::AwaitPath, S::MatCutSetPathData, P::AwaitLoad, &[S::WaitOnMatLoad]),
            (P::Finished, S::MatCutSetPathData, P::AwaitLoad, &[S::WaitOnMatLoad]),
            (P::AwaitLoad, S::MatCutSimulateLoadButtonPressed, P::AwaitGo, &[S::MatLoaded, S::WaitOnGo]),
            (P::AwaitGo, S::MatCutSimulateGoButtonPressed, FROM_ZERO, &[S::GoPressed]),
            (P::AwaitUnload, S::MatCutSimulateGoButtonPressed, FROM_ZERO, &[S::GoPressed]),
            (CUTTING, S::MatCutSimulatePauseButtonPressed, PAUSED, &[S::DevicePaused]),
            (PAUSED, S::MatCutSimulatePauseButtonPressed, CUTTING, &[S::DeviceResumed]),
            (P::AwaitUnload, S::MatCutSimulateLoadButtonPressed, P::Finished, &[S::MatUnloaded, S::Complete]),
            (P::AwaitMaterial, S::MatCutAbortCut, P::Finished, &[S::MatCutAbortCut, S::Complete]),
            (P::AwaitTools, S::MatCutAbortCut, P::Finished, &[S::MatCutAbortCut, S::Complete]),
            (P::AwaitLoad, S::MatCutAbortCut, P::Finished, &[S::MatCutAbortCut, S::Complete]),
            (P::AwaitGo, S::MatCutAbortCut, P::AwaitUnload, &[S::MatCutAbortCut, S::WaitOnMatUnload]),
            (CUTTING, S::MatCutAbortCut, P::AwaitUnload, &[S::MatCutAbortCut, S::WaitOnMatUnload]),
            (PAUSED, S::MatCutAbortCut, P::AwaitUnload, &[S::MatCutAbortCut, S::WaitOnMatUnload]),
            (P::AwaitUnload, S::MatCutAbortCut, P::AwaitUnload, &[S::MatCutAbortCut, S::WaitOnMatUnload]),
            (P::AwaitGo, S::MatCutSimulateLoadButtonPressed, P::AwaitGo, &[]),
            (P::AwaitLoad, S::MatCutSimulateGoButtonPressed, P::AwaitLoad, &[]),
            (CUTTING, S::MatCutSimulateGoButtonPressed, CUTTING, &[]),
            (P::Finished, S::MatCutSimulateLoadButtonPressed, P::Finished, &[]),
            (P::AwaitMaterial, S::MatCutSetPathData, P::AwaitMaterial, &[]),
        ];
        for (from, input, to, replies) in rows {
            let mut m = machine(from);
            let got = statuses(&m.on_message(&BridgeMessage::new(input)));
            assert_eq!((m.phase, got.as_slice()), (to, replies), "{from:?} + {}", input.wire_name());
        }
    }

    #[test]
    fn a_tick_advances_the_cut_and_the_last_tick_waits_for_unload() {
        let mut m = machine(FROM_ZERO);
        assert_eq!(statuses(&m.on_tick()), [S::MatCutSetProgress]);
        assert_eq!(m.phase, Phase::Cutting { percent: PERCENT_STEP, ticks: 1, paused: false });
        let mut m = machine(Phase::Cutting { percent: 100 + PERCENT_STEP, ticks: 51, paused: false });
        assert_eq!(statuses(&m.on_tick()), [S::MatCutCompleteSuccess, S::WaitOnMatUnload]);
        assert_eq!(m.phase, Phase::AwaitUnload);
        let mut m = machine(Phase::AwaitGo);
        assert!(m.on_tick().is_empty(), "only a running cut ticks");
        assert_eq!(m.phase, Phase::AwaitGo);
        let paused = Phase::Cutting { percent: 40, ticks: 20, paused: true };
        let mut m = machine(paused);
        assert!(m.on_tick().is_empty(), "a paused cut holds");
        assert_eq!(m.phase, paused);
    }

    #[test]
    fn a_dead_machine_thread_fails_both_send_and_receive() {
        let (machine, kill) = SimulatedMachine::stepped_killable(Duration::from_millis(1));
        machine.send(BridgeMessage::new(S::MatCut)).unwrap();
        assert_eq!(recv(&machine).status, S::SingleDeviceConnected);
        kill();
        while let Ok(msg) = machine.recv_timeout(Duration::from_secs(5)) {
            assert_ne!(msg.status, S::SingleDeviceConnected, "no new conversation after the kill");
        }
        assert!(matches!(machine.try_recv(), Err(Disconnected)), "the receive side reports the dead thread");
        assert_eq!(machine.send(BridgeMessage::new(S::SelectDevice)), Err(Disconnected));
    }

    #[test]
    fn device_list_prefers_usb_and_carries_both_transports() {
        let types: Vec<_> = fake_devices().iter().map(|d| d.connection_type).collect();
        assert_eq!(types, [ConnectionType::Usb, ConnectionType::Bluetooth]);
    }
}

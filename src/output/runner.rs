//! Owns one open device's link on its own thread. The UI and the runner share no state: commands go
//! one way and events the other.
//!
//! Every wait is a count of 100 ms `read_line` calls, never a wall clock, so a real port blocks for
//! real while the fake and file links answer at once. Commands are checked between calls.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use super::link::{Opener, SerialLink};
use super::profile::{DeviceProfile, Protocol};
use super::{DeviceCommand, DeviceEvent, Job};
use crate::device::Disconnected;

pub struct Runner {
    /// `None` only while dropping.
    to_runner: Option<Sender<DeviceCommand>>,
    from_runner: Receiver<DeviceEvent>,
    /// Events `await_event` has taken off the channel, handed out first by `try_recv`.
    held: RefCell<VecDeque<DeviceEvent>>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    /// Opens the link with `open` on the new thread, then serves commands for `profile`'s protocol.
    pub fn spawn(open: Opener, profile: DeviceProfile) -> Self {
        let (to_runner, commands) = mpsc::channel();
        let (events, from_runner) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("open-device".into())
            .spawn(move || serve(open().map_err(|e| e.to_string()), &profile, commands, events))
            .expect("spawning the open device thread");
        Self { to_runner: Some(to_runner), from_runner, held: RefCell::default(), thread: Some(thread) }
    }

    pub fn send(&self, command: DeviceCommand) -> Result<(), Disconnected> {
        self.to_runner.as_ref().ok_or(Disconnected)?.send(command).map_err(|_| Disconnected)
    }

    /// `Ok(None)` when nothing has arrived yet.
    pub fn try_recv(&self) -> Result<Option<DeviceEvent>, Disconnected> {
        if let Some(e) = self.held.borrow_mut().pop_front() {
            return Ok(Some(e));
        }
        match self.from_runner.try_recv() {
            Ok(e) => Ok(Some(e)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Disconnected),
        }
    }

    /// Blocks until `try_recv` has something to say, keeping the event for it. False only on timeout.
    #[cfg(test)]
    pub fn await_event(&self, guard: Duration) -> bool {
        if !self.held.borrow().is_empty() {
            return true;
        }
        match self.from_runner.recv_timeout(guard) {
            Ok(e) => {
                self.held.borrow_mut().push_back(e);
                true
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        }
    }
}

/// The thread sees the disconnect after any command already sent, cancels a running job (writing its
/// stop bytes) and exits. Every wait is sliced, so the join is bounded.
impl Drop for Runner {
    fn drop(&mut self) {
        self.to_runner = None;
        if let Some(thread) = self.thread.take() {
            _ = thread.join();
        }
    }
}

/// Streams `job` to the end with the same flow control the runner uses: the dry run.
pub fn run_to_end(link: &mut dyn SerialLink, job: &Job) -> Result<(), String> {
    match stream(link, job, true, &mut DryRun) {
        Ok(()) => Ok(()),
        Err(End::Failed(e)) => Err(e),
        Err(End::Cancelled) => unreachable!("a dry run has no one to cancel it"),
    }
}

const SLICE: Duration = Duration::from_millis(100);
/// 2 s in `SLICE`s.
const TEST_SLICES: u32 = 20;
const HPGL_CHUNK: usize = 256;
const NO_IDENTITY: &str = "Port opened; this cutter does not report an identity";

fn serve(mut link: Result<Box<dyn SerialLink>, String>, profile: &DeviceProfile, commands: Receiver<DeviceCommand>, events: Sender<DeviceEvent>) {
    while let Ok(command) = commands.recv() {
        match (command, &mut link) {
            (DeviceCommand::Test, Err(e)) => _ = events.send(DeviceEvent::TestFailed(e.clone())),
            (DeviceCommand::Start(_), Err(e)) => _ = events.send(DeviceEvent::Failed(e.clone())),
            (DeviceCommand::Test, Ok(link)) => _ = events.send(test(link.as_mut(), profile)),
            (DeviceCommand::Start(job), Ok(link)) => {
                let mut control = Channel { commands: &commands, events: &events };
                let pace = job.protocol == Protocol::Hpgl && !profile.xon_xoff;
                match stream(link.as_mut(), &job, pace, &mut control) {
                    Ok(()) => _ = events.send(DeviceEvent::Done),
                    Err(End::Failed(e)) => _ = events.send(DeviceEvent::Failed(e)),
                    Err(End::Cancelled) => {}
                }
            }
            (DeviceCommand::Pause | DeviceCommand::Resume | DeviceCommand::Cancel, _) => {}
        }
    }
}

/// Sends only identity queries, never motion.
fn test(link: &mut dyn SerialLink, profile: &DeviceProfile) -> DeviceEvent {
    let outcome = match (profile.protocol, &profile.identify) {
        (Protocol::Grbl, _) => test_grbl(link),
        (Protocol::Hpgl, Some(query)) => ask(link, query.as_bytes(), |l| !l.trim().is_empty()).map(|reply| match reply {
            Some(id) => DeviceEvent::TestPassed(format!("Port opened; the cutter says {id}")),
            None => DeviceEvent::TestPassed(NO_IDENTITY.into()),
        }),
        (Protocol::Hpgl, None) => Ok(DeviceEvent::TestPassed(NO_IDENTITY.into())),
    };
    outcome.unwrap_or_else(|e| DeviceEvent::TestFailed(e.to_string()))
}

fn test_grbl(link: &mut dyn SerialLink) -> io::Result<DeviceEvent> {
    if let Some(banner) = ask(link, b"\n", |l| l.starts_with("Grbl"))? {
        return Ok(DeviceEvent::TestPassed(banner));
    }
    Ok(match ask(link, b"$I\n", |l| !l.trim().is_empty())? {
        Some(info) => DeviceEvent::TestPassed(info),
        None => DeviceEvent::TestFailed("No reply from GRBL. Check the port and the baud rate, then try again".into()),
    })
}

/// Writes `query` and waits up to 2 s for a `wanted` line.
fn ask(link: &mut dyn SerialLink, query: &[u8], wanted: fn(&str) -> bool) -> io::Result<Option<String>> {
    link.write(query)?;
    for _ in 0..TEST_SLICES {
        if let Some(line) = link.read_line(SLICE)?.filter(|l| wanted(l)) {
            return Ok(Some(line));
        }
    }
    Ok(None)
}

/// Why streaming stopped early.
enum End {
    Failed(String),
    Cancelled,
}

impl From<io::Error> for End {
    fn from(e: io::Error) -> Self {
        End::Failed(e.to_string())
    }
}

/// Where a streaming job hears commands and reports progress.
trait Control {
    /// A command that has already arrived, without waiting.
    fn poll(&mut self) -> Option<DeviceCommand>;
    /// The next command, waiting for it.
    fn wait(&mut self) -> DeviceCommand;
    fn send(&mut self, event: DeviceEvent);
}

struct Channel<'a> {
    commands: &'a Receiver<DeviceCommand>,
    events: &'a Sender<DeviceEvent>,
}

/// A dropped app cancels the job.
impl Control for Channel<'_> {
    fn poll(&mut self) -> Option<DeviceCommand> {
        match self.commands.try_recv() {
            Ok(c) => Some(c),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(DeviceCommand::Cancel),
        }
    }

    fn wait(&mut self) -> DeviceCommand {
        self.commands.recv().unwrap_or(DeviceCommand::Cancel)
    }

    fn send(&mut self, event: DeviceEvent) {
        _ = self.events.send(event);
    }
}

/// Nobody is listening, and tool changes carry straight on.
struct DryRun;

impl Control for DryRun {
    fn poll(&mut self) -> Option<DeviceCommand> {
        None
    }

    fn wait(&mut self) -> DeviceCommand {
        DeviceCommand::Resume
    }

    fn send(&mut self, _event: DeviceEvent) {}
}

struct Streaming<'a> {
    link: &'a mut dyn SerialLink,
    job: &'a Job,
    control: &'a mut dyn Control,
}

impl Streaming<'_> {
    /// Acts on every command that has arrived. Returns only once streaming may go on.
    fn check(&mut self) -> Result<(), End> {
        while let Some(command) = self.control.poll() {
            match command {
                DeviceCommand::Cancel => return self.cancel(),
                DeviceCommand::Pause => {
                    if let Some(pause) = &self.job.pause {
                        self.link.write(pause)?;
                    }
                    self.control.send(DeviceEvent::Paused);
                    self.until_resume()?;
                    if let Some(resume) = &self.job.resume {
                        self.link.write(resume)?;
                    }
                    self.control.send(DeviceEvent::Resumed);
                }
                DeviceCommand::Test | DeviceCommand::Start(_) | DeviceCommand::Resume => {}
            }
        }
        Ok(())
    }

    fn until_resume(&mut self) -> Result<(), End> {
        loop {
            match self.control.wait() {
                DeviceCommand::Resume => return Ok(()),
                DeviceCommand::Cancel => return self.cancel(),
                DeviceCommand::Test | DeviceCommand::Start(_) | DeviceCommand::Pause => {}
            }
        }
    }

    fn cancel(&mut self) -> Result<(), End> {
        self.link.write(&self.job.stop)?;
        Err(End::Cancelled)
    }

    fn tool_change(&mut self, line: usize) -> Result<(), End> {
        let Some(change) = self.job.tool_changes.iter().find(|c| c.before_line == line) else { return Ok(()) };
        self.control.send(DeviceEvent::ToolChange(change.message.clone()));
        self.until_resume()?;
        self.control.send(DeviceEvent::Resumed);
        Ok(())
    }

    /// Waits for GRBL to accept the line just sent.
    fn acknowledged(&mut self) -> Result<(), End> {
        loop {
            self.check()?;
            let Some(reply) = self.link.read_line(SLICE)? else { continue };
            let reply = reply.trim();
            if reply == "ok" {
                return Ok(());
            }
            if let Some(code) = reply.strip_prefix("error:") {
                return Err(End::Failed(described(reply, code, GRBL_ERRORS)));
            }
            if let Some(code) = reply.strip_prefix("ALARM:") {
                return Err(End::Failed(described(reply, code, GRBL_ALARMS)));
            }
        }
    }

    /// Discards replies left from before the job, such as the `ok` GRBL gives the connection test's bare
    /// newline, so none is taken for the first line's acknowledgement. Bounded, in case the board chatters.
    fn drain(&mut self) -> Result<(), End> {
        for _ in 0..TEST_SLICES {
            if self.link.read_line(SLICE)?.is_none() {
                break;
            }
        }
        self.check()
    }

    /// Lets the machine drain `slices` of moves, still listening for commands.
    fn idle(&mut self, slices: u32) -> Result<(), End> {
        for _ in 0..slices {
            self.check()?;
            self.link.read_line(SLICE)?;
        }
        self.check()
    }

    fn grbl(&mut self) -> Result<(), End> {
        self.drain()?;
        let total = self.job.lines.len();
        for (i, line) in self.job.lines.iter().enumerate() {
            self.tool_change(i)?;
            self.check()?;
            self.link.write(format!("{line}\n").as_bytes())?;
            self.acknowledged()?;
            self.control.send(DeviceEvent::Progress { sent: i + 1, total });
        }
        Ok(())
    }

    /// Chunks of at most `HPGL_CHUNK` bytes that never straddle a tool change, each followed by the
    /// time its completed lines take to run when `pace` is set.
    fn hpgl(&mut self, pace: bool) -> Result<(), End> {
        let job = self.job;
        let total = job.lines.len();
        let bytes = job.bytes();
        let mut ends = Vec::with_capacity(total);
        let mut at = 0;
        for line in &job.lines {
            at += line.len() + 1;
            ends.push(at);
        }
        let starts = |line: usize| if line == 0 { 0 } else { ends[line - 1] };
        let (mut sent, mut done, mut owed) = (0, 0, 0.0f32);
        while sent < bytes.len() {
            if sent == starts(done) {
                self.tool_change(done)?;
            }
            let stop_at = job.tool_changes.iter().map(|c| starts(c.before_line)).filter(|&b| b > sent).min().unwrap_or(bytes.len());
            let end = (sent + HPGL_CHUNK).min(stop_at);
            self.check()?;
            self.link.write(&bytes[sent..end])?;
            sent = end;
            let finished = ends.partition_point(|&e| e <= sent);
            owed += job.seconds[done..finished].iter().sum::<f32>();
            if done < finished {
                done = finished;
                self.control.send(DeviceEvent::Progress { sent: done, total });
            }
            if pace {
                let slices = (owed / SLICE.as_secs_f32()).floor();
                owed -= slices * SLICE.as_secs_f32();
                self.idle(slices as u32)?;
            }
        }
        Ok(())
    }
}

fn stream(link: &mut dyn SerialLink, job: &Job, pace: bool, control: &mut dyn Control) -> Result<(), End> {
    let mut s = Streaming { link, job, control };
    match job.protocol {
        Protocol::Grbl => s.grbl(),
        Protocol::Hpgl => s.hpgl(pace),
    }
}

fn described(reply: &str, code: &str, table: &[(u32, &str)]) -> String {
    match code.trim().parse::<u32>().ok().and_then(|n| table.iter().find(|(c, _)| *c == n)) {
        Some((_, meaning)) => format!("{reply} {meaning}"),
        None => reply.to_owned(),
    }
}

/// GRBL 1.1's `error:N` codes.
const GRBL_ERRORS: &[(u32, &str)] = &[
    (1, "Expected command letter"),
    (2, "Bad number format"),
    (3, "Invalid statement"),
    (4, "Value < 0"),
    (5, "Setting disabled"),
    (6, "Value < 3 usec"),
    (7, "EEPROM read fail. Using defaults"),
    (8, "Not idle"),
    (9, "G-code lock"),
    (10, "Homing not enabled"),
    (11, "Line overflow"),
    (12, "Step rate > 30kHz"),
    (13, "Check door"),
    (14, "Line length exceeded"),
    (15, "Travel exceeded"),
    (16, "Invalid jog command"),
    (17, "Setting disabled"),
    (20, "Unsupported command"),
    (21, "Modal group violation"),
    (22, "Undefined feed rate"),
    (23, "Invalid G-code ID:23"),
    (24, "Invalid G-code ID:24"),
    (25, "Invalid G-code ID:25"),
    (26, "Invalid G-code ID:26"),
    (27, "Invalid G-code ID:27"),
    (28, "Invalid G-code ID:28"),
    (29, "Invalid G-code ID:29"),
    (30, "Invalid G-code ID:30"),
    (31, "Invalid G-code ID:31"),
    (32, "Invalid G-code ID:32"),
    (33, "Invalid G-code ID:33"),
    (34, "Invalid G-code ID:34"),
    (35, "Invalid G-code ID:35"),
    (36, "Invalid G-code ID:36"),
    (37, "Invalid G-code ID:37"),
    (38, "Invalid G-code ID:38"),
];

/// GRBL 1.1's `ALARM:N` codes.
const GRBL_ALARMS: &[(u32, &str)] = &[
    (1, "Hard limit"),
    (2, "Soft limit"),
    (3, "Abort during cycle"),
    (4, "Probe fail"),
    (5, "Probe fail"),
    (6, "Homing fail"),
    (7, "Homing fail"),
    (8, "Homing fail"),
    (9, "Homing fail"),
    (10, "Homing fail"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::ToolChange;
    use crate::output::link::{FakeLink, FileLink, GRBL_BANNER};

    const GUARD: Duration = Duration::from_secs(5);

    fn spawn(link: &FakeLink, profile: DeviceProfile) -> Runner {
        let link = link.clone();
        Runner::spawn(Box::new(move || Ok(Box::new(link) as Box<dyn SerialLink>)), profile)
    }

    fn next(runner: &Runner) -> DeviceEvent {
        assert!(runner.await_event(GUARD), "an event within 5 s");
        runner.try_recv().expect("the runner is alive").expect("the event await_event saw")
    }

    /// Drops the runner, which joins its thread once every byte is written, and returns the events it
    /// had not yet handed out.
    fn finish(mut runner: Runner) -> Vec<DeviceEvent> {
        let mut events: Vec<_> = runner.held.take().into();
        let from_runner = std::mem::replace(&mut runner.from_runner, mpsc::channel().1);
        drop(runner);
        events.extend(from_runner.try_iter());
        events
    }

    fn until_done(runner: &Runner) -> Vec<DeviceEvent> {
        let mut events = vec![next(runner)];
        while !matches!(events.last(), Some(DeviceEvent::Done | DeviceEvent::Failed(_))) {
            events.push(next(runner));
        }
        events
    }

    fn grbl_job(lines: &[&str]) -> Job {
        let profile = DeviceProfile::grbl_pen_plotter();
        let path = crate::output::toolpath::Toolpath::default();
        let drive = crate::output::Drive { speed_mm_s: 10.0, force: None, power: None, passes: 1 };
        Job { lines: lines.iter().map(|l| l.to_string()).collect(), seconds: vec![0.0; lines.len()], ..crate::output::gcode::emit(&path, &profile, drive) }
    }

    fn lines_written(link: &FakeLink) -> usize {
        link.written_text().matches('\n').count()
    }

    /// Answers `ok` once line `n` is out. An earlier `ok` would be drained as stale before the job.
    fn ok_after(link: &FakeLink, n: usize) {
        assert!(link.wait_for(GUARD, |b| b.iter().filter(|&&c| c == b'\n').count() >= n), "line {n} sent");
        link.reply("ok");
    }

    /// The first line, maybe the second (Cancel can land just before or after it goes out), then the
    /// stop bytes once and nothing after them.
    fn stopped_after_first_line(fake: &FakeLink) {
        let text = fake.written_text();
        let before = text.strip_suffix("!\u{18}M5\n").unwrap_or_else(|| panic!("no stop bytes last: {text:?}"));
        assert!(["G21\n", "G21\nG0 X1 Y1\n"].contains(&before), "{text:?}");
    }

    fn no_motion(text: &str) {
        for word in ["G0", "G1", "PU", "PD"] {
            assert!(!text.contains(word), "motion {word}: {text:?}");
        }
    }

    /// A stale `ok` from before the job (GRBL answers the test's bare newline) must not count for line 1.
    #[test]
    fn grbl_waits_for_ok_before_each_line() {
        let fake = FakeLink::silent();
        fake.reply("ok");
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        let job = grbl_job(&["G21", "G90", "G0 X1 Y1", "G1 X2 Y2 F600"]);
        runner.send(DeviceCommand::Start(job.clone())).unwrap();
        for sent in 1..=job.lines.len() {
            assert!(fake.wait_for(GUARD, |b| b.iter().filter(|&&c| c == b'\n').count() >= sent), "line {sent} sent");
            assert_eq!(lines_written(&fake), sent, "line {sent} waits for its own ok");
            fake.reply("ok");
            assert_eq!(next(&runner), DeviceEvent::Progress { sent, total: 4 });
        }
        assert_eq!(next(&runner), DeviceEvent::Done);
        assert_eq!(fake.written(), job.bytes());
        assert_eq!(fake.writes(), 4, "one write per line");
    }

    #[test]
    fn a_grbl_error_stops_the_job_with_its_meaning() {
        let fake = FakeLink::grbl();
        fake.fail_on("G1", "error:20");
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Start(grbl_job(&["G21", "G1 X2 Y2 F600", "G1 X3 Y3 F600"]))).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Progress { sent: 1, total: 3 });
        assert_eq!(next(&runner), DeviceEvent::Failed("error:20 Unsupported command".into()));
        assert!(!fake.written_text().contains("X3"));
    }

    #[test]
    fn alarms_and_unknown_codes_read_back_their_code() {
        assert_eq!(described("ALARM:1", "1", GRBL_ALARMS), "ALARM:1 Hard limit");
        assert_eq!(described("error:99", "99", GRBL_ERRORS), "error:99");
    }

    #[test]
    fn cancel_mid_job_sends_the_stop_bytes_last() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        let job = grbl_job(&["G21", "G0 X1 Y1", "G1 X2 Y2 F600", "G1 X3 Y3 F600"]);
        runner.send(DeviceCommand::Start(job)).unwrap();
        ok_after(&fake, 1);
        assert_eq!(next(&runner), DeviceEvent::Progress { sent: 1, total: 4 });
        runner.send(DeviceCommand::Cancel).unwrap();
        assert_eq!(finish(runner), []);
        stopped_after_first_line(&fake);
    }

    #[test]
    fn dropping_the_runner_mid_job_cancels_it() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Start(grbl_job(&["G21", "G0 X1 Y1"]))).unwrap();
        ok_after(&fake, 1);
        assert_eq!(next(&runner), DeviceEvent::Progress { sent: 1, total: 2 });
        finish(runner);
        assert!(fake.written().ends_with(b"!\x18M5\n"), "{:?}", fake.written_text());
    }

    #[test]
    fn cancel_then_drop_stops_once() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Start(grbl_job(&["G21", "G0 X1 Y1"]))).unwrap();
        ok_after(&fake, 1);
        assert_eq!(next(&runner), DeviceEvent::Progress { sent: 1, total: 2 });
        runner.send(DeviceCommand::Cancel).unwrap();
        drop(runner);
        stopped_after_first_line(&fake);
    }

    #[test]
    fn pause_sends_feed_hold_and_resume_cycle_start() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        let job = grbl_job(&["G21", "G0 X1 Y1"]);
        runner.send(DeviceCommand::Start(job.clone())).unwrap();
        runner.send(DeviceCommand::Pause).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Paused);
        assert!(fake.written_text().ends_with('!') && lines_written(&fake) <= 1, "{:?}", fake.written_text());
        runner.send(DeviceCommand::Resume).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Resumed);
        assert!(fake.written_text().contains("!~"), "{:?}", fake.written_text());
        fake.reply("ok");
        fake.reply("ok");
        assert_eq!(until_done(&runner).last(), Some(&DeviceEvent::Done));
        assert_eq!(fake.written_text().replace(['!', '~'], "").into_bytes(), job.bytes());
    }

    #[test]
    fn commands_without_a_job_are_ignored() {
        let fake = FakeLink::grbl();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Pause).unwrap();
        runner.send(DeviceCommand::Cancel).unwrap();
        assert_eq!(finish(runner), []);
        assert!(fake.written().is_empty());
    }

    #[test]
    fn a_grbl_test_sends_only_a_newline_when_the_banner_answers() {
        let fake = FakeLink::grbl();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Test).unwrap();
        assert_eq!(next(&runner), DeviceEvent::TestPassed(GRBL_BANNER.into()));
        assert_eq!(fake.written(), b"\n");
    }

    #[test]
    fn a_grbl_test_asks_for_build_info_without_a_banner() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Test).unwrap();
        assert!(matches!(next(&runner), DeviceEvent::TestFailed(m) if m.starts_with("No reply from GRBL")));
        assert_eq!(fake.written(), b"\n$I\n");
        no_motion(&fake.written_text());
    }

    #[test]
    fn an_hpgl_test_sends_only_the_identify_query() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::hpgl_vinyl_cutter());
        runner.send(DeviceCommand::Test).unwrap();
        assert_eq!(next(&runner), DeviceEvent::TestPassed(NO_IDENTITY.into()));
        fake.reply("CUTTER 1");
        runner.send(DeviceCommand::Test).unwrap();
        assert_eq!(next(&runner), DeviceEvent::TestPassed("Port opened; the cutter says CUTTER 1".into()));
        assert_eq!(fake.written_text(), "OI;OI;");
        no_motion(&fake.written_text());
    }

    #[test]
    fn a_port_that_will_not_open_fails_test_and_start() {
        let open: Opener = Box::new(|| Err(io::Error::new(io::ErrorKind::NotFound, "no such port")));
        let runner = Runner::spawn(open, DeviceProfile::grbl_pen_plotter());
        runner.send(DeviceCommand::Test).unwrap();
        assert_eq!(next(&runner), DeviceEvent::TestFailed("no such port".into()));
        runner.send(DeviceCommand::Start(grbl_job(&["G21"]))).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Failed("no such port".into()));
    }

    #[derive(Default)]
    struct Recorder {
        writes: Vec<Vec<u8>>,
        reads: usize,
    }

    impl SerialLink for Recorder {
        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.writes.push(bytes.to_vec());
            Ok(())
        }

        fn read_line(&mut self, _timeout: Duration) -> io::Result<Option<String>> {
            self.reads += 1;
            Ok(None)
        }
    }

    fn hpgl_job() -> Job {
        let design = crate::output::toolpath::tests::two_inch_square();
        crate::output::toolpath::tests::job_for(&design, &DeviceProfile::hpgl_vinyl_cutter())
    }

    #[test]
    fn hpgl_streams_in_paced_chunks_of_at_most_256_bytes() {
        let job = hpgl_job();
        let mut link = Recorder::default();
        run_to_end(&mut link, &job).unwrap();
        assert!(link.writes.len() > 1 && link.writes.iter().all(|w| w.len() <= HPGL_CHUNK), "{:?}", link.writes.iter().map(Vec::len).collect::<Vec<_>>());
        assert_eq!(link.writes.concat(), job.bytes());
        let slices = (job.seconds.iter().sum::<f32>() / 0.1).floor() as usize;
        assert!(slices > 0 && link.reads.abs_diff(slices) <= 1, "{} reads for {slices} slices", link.reads);
    }

    #[test]
    fn hpgl_with_xon_xoff_is_not_paced() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile { xon_xoff: true, ..DeviceProfile::hpgl_vinyl_cutter() });
        let job = hpgl_job();
        runner.send(DeviceCommand::Start(job.clone())).unwrap();
        let events = until_done(&runner);
        assert_eq!(events.last(), Some(&DeviceEvent::Done));
        assert_eq!(events.iter().rfind(|e| matches!(e, DeviceEvent::Progress { .. })), Some(&DeviceEvent::Progress { sent: job.lines.len(), total: job.lines.len() }));
        assert_eq!(fake.written(), job.bytes());
    }

    #[test]
    fn hpgl_tool_changes_and_pauses_add_no_bytes() {
        let fake = FakeLink::silent();
        let runner = spawn(&fake, DeviceProfile::hpgl_vinyl_cutter());
        let job = Job { tool_changes: vec![ToolChange { before_line: 4, message: "Swap".into() }], ..hpgl_job() };
        assert!(job.lines[4].len() > HPGL_CHUNK, "the change sits before a line that spans chunks");
        runner.send(DeviceCommand::Start(job.clone())).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Progress { sent: 4, total: job.lines.len() });
        assert_eq!(next(&runner), DeviceEvent::ToolChange("Swap".into()));
        assert_eq!(fake.written_text(), "IN;\nVS40;\nFS55;\nPU39746,23746;\n", "streaming stops before the tool change");
        runner.send(DeviceCommand::Pause).unwrap();
        runner.send(DeviceCommand::Resume).unwrap();
        assert_eq!(next(&runner), DeviceEvent::Resumed);
        runner.send(DeviceCommand::Pause).unwrap();
        runner.send(DeviceCommand::Resume).unwrap();
        let events = until_done(&runner);
        assert_eq!(events.last(), Some(&DeviceEvent::Done));
        assert!(!events.iter().any(|e| matches!(e, DeviceEvent::ToolChange(_))), "one stop per tool change: {events:?}");
        assert_eq!(fake.written(), job.bytes(), "pause and resume add no bytes");
    }

    #[test]
    fn the_dry_run_writes_exactly_the_jobs_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Mat 1.hpgl");
        let job = hpgl_job();
        let mut link = FileLink::new(path.clone());
        run_to_end(&mut link, &job).unwrap();
        link.finish().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), job.bytes());

        let gcode = grbl_job(&["G21", "G0 X1 Y1", "M5"]);
        let mut link = FileLink::new(dir.path().join("Mat 1.gcode"));
        run_to_end(&mut link, &gcode).unwrap();
        link.finish().unwrap();
        assert_eq!(std::fs::read(dir.path().join("Mat 1.gcode")).unwrap(), gcode.bytes());
    }
}

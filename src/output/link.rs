//! The byte pipe the runner streams through, and the host that lists ports, opens them and picks
//! dry-run paths. The app gets `SystemHost`; tests inject a fake one so they never touch a real port.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use super::profile::DeviceProfile;

pub trait SerialLink: Send {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// The next line without its terminator, or `None` when nothing complete arrives within `timeout`.
    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>>;
}

/// Opens a link on the runner thread, so a slow port never blocks the UI.
pub type Opener = Box<dyn FnOnce() -> io::Result<Box<dyn SerialLink>> + Send>;

#[derive(Clone, Debug, PartialEq)]
pub struct PortInfo {
    pub name: String,
    /// "USB: FT232R (FTDI)" where the OS reports it.
    pub description: String,
    /// The USB descriptor the OS already parsed, for a USB serial port; `None` for any other kind.
    pub usb: Option<UsbId>,
}

/// A USB serial port's identity, as the OS reports it. Observed data, so a plain struct parsed at the
/// `SystemHost` boundary; `output::machines` recognises a machine from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbId {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
}

pub trait Host {
    fn ports(&self) -> Vec<PortInfo>;
    fn open(&self, profile: &DeviceProfile) -> Opener;
    /// Where the dry run writes, from a save dialog suggesting `file_name`.
    fn save_path(&self, file_name: &str) -> Option<PathBuf>;
}

pub struct SystemHost;

impl Host for SystemHost {
    fn ports(&self) -> Vec<PortInfo> {
        let ports = serialport::available_ports().unwrap_or_default();
        ports.into_iter().map(|p| PortInfo { usb: usb_id(&p.port_type), description: describe(&p.port_type), name: p.port_name }).collect()
    }

    fn open(&self, profile: &DeviceProfile) -> Opener {
        let (port, baud, xon_xoff) = (profile.port.clone(), profile.baud, profile.xon_xoff);
        Box::new(move || {
            let port = port.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no port chosen"))?;
            Ok(Box::new(PortLink::open(&port, baud, xon_xoff)?) as Box<dyn SerialLink>)
        })
    }

    fn save_path(&self, file_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_file_name(file_name).save_file()
    }
}

fn usb_id(kind: &serialport::SerialPortType) -> Option<UsbId> {
    match kind {
        serialport::SerialPortType::UsbPort(u) => {
            Some(UsbId { vid: u.vid, pid: u.pid, serial: u.serial_number.clone(), product: u.product.clone(), manufacturer: u.manufacturer.clone() })
        }
        _ => None,
    }
}

fn describe(kind: &serialport::SerialPortType) -> String {
    match kind {
        serialport::SerialPortType::UsbPort(usb) => {
            let names = [usb.product.as_deref(), usb.manufacturer.as_deref()];
            match names {
                [Some(p), Some(m)] => format!("USB: {p} ({m})"),
                [Some(n), None] | [None, Some(n)] => format!("USB: {n}"),
                [None, None] => format!("USB: {:04x}:{:04x}", usb.vid, usb.pid),
            }
        }
        serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
        serialport::SerialPortType::PciPort => "PCI".into(),
        serialport::SerialPortType::Unknown => String::new(),
    }
}

pub struct PortLink {
    port: Box<dyn serialport::SerialPort>,
    pending: Vec<u8>,
}

impl PortLink {
    pub fn open(name: &str, baud: u32, xon_xoff: bool) -> io::Result<Self> {
        let flow = if xon_xoff { serialport::FlowControl::Software } else { serialport::FlowControl::None };
        let port = serialport::new(name, baud).flow_control(flow).timeout(Duration::from_millis(100)).open()?;
        Ok(Self { port, pending: Vec::new() })
    }

    fn take_line(&mut self) -> Option<String> {
        let end = self.pending.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        Some(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_owned())
    }
}

impl SerialLink for PortLink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        if let Some(line) = self.take_line() {
            return Ok(Some(line));
        }
        self.port.set_timeout(timeout)?;
        let mut buf = [0u8; 256];
        match self.port.read(&mut buf) {
            Ok(n) => self.pending.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }
        Ok(self.take_line())
    }
}

/// The dry run's link: collects what a port would receive, answers `ok` like an idle GRBL board, and
/// writes the file atomically on `finish`.
pub struct FileLink {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl FileLink {
    pub fn new(path: PathBuf) -> Self {
        Self { path, bytes: Vec::new() }
    }

    pub fn finish(self) -> io::Result<()> {
        crate::store::write_atomically(&self.path, &self.bytes)
    }
}

impl SerialLink for FileLink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn read_line(&mut self, _timeout: Duration) -> io::Result<Option<String>> {
        Ok(Some("ok".into()))
    }
}

#[cfg(test)]
pub use fake::{Answer, FAKE_PORT, FakeHost, FakeLink, GRBL_BANNER};

#[cfg(test)]
mod fake {
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex};

    use super::*;

    pub const GRBL_BANNER: &str = "Grbl 1.1h ['$' for help]";

    /// How a fake machine answers each line it is sent.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Answer {
        /// Never says anything, like most HPGL cutters.
        Silent,
        /// The GRBL banner for a bare newline, `ok` for every other line.
        Grbl,
    }

    #[derive(Debug)]
    struct State {
        written: Vec<u8>,
        answer: Answer,
        /// Replies queued ahead of the automatic ones, in order.
        replies: VecDeque<String>,
        /// A reply that replaces `ok` for the first line starting with the prefix.
        fail_on: Option<(String, String)>,
        /// Bytes of the line being written, until its `\n`.
        partial: Vec<u8>,
        writes: usize,
    }

    /// Records every byte and answers from a script. Clones share one recording, so a test keeps a
    /// clone while the runner owns another.
    #[derive(Clone, Debug)]
    pub struct FakeLink(Arc<Mutex<State>>, Arc<Condvar>);

    impl FakeLink {
        pub fn new(answer: Answer) -> Self {
            let state = State { written: Vec::new(), answer, replies: VecDeque::new(), fail_on: None, partial: Vec::new(), writes: 0 };
            Self(Arc::new(Mutex::new(state)), Arc::default())
        }

        /// Blocks until the bytes written so far satisfy `done`, or `guard` passes. A failure bound only.
        pub fn wait_for(&self, guard: Duration, done: impl Fn(&[u8]) -> bool) -> bool {
            let state = self.0.lock().unwrap();
            let (state, _) = self.1.wait_timeout_while(state, guard, |s| !done(&s.written)).unwrap();
            done(&state.written)
        }

        pub fn grbl() -> Self {
            Self::new(Answer::Grbl)
        }

        pub fn silent() -> Self {
            Self::new(Answer::Silent)
        }

        pub fn reply(&self, line: &str) {
            self.0.lock().unwrap().replies.push_back(line.into());
        }

        pub fn fail_on(&self, prefix: &str, reply: &str) {
            self.0.lock().unwrap().fail_on = Some((prefix.into(), reply.into()));
        }

        pub fn written(&self) -> Vec<u8> {
            self.0.lock().unwrap().written.clone()
        }

        pub fn written_text(&self) -> String {
            String::from_utf8_lossy(&self.written()).into_owned()
        }

        pub fn writes(&self) -> usize {
            self.0.lock().unwrap().writes
        }

        pub fn clear(&self) {
            self.0.lock().unwrap().written.clear();
        }
    }

    impl SerialLink for FakeLink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.written.extend_from_slice(bytes);
            s.writes += 1;
            for &b in bytes {
                if b != b'\n' {
                    s.partial.push(b);
                    continue;
                }
                let line = String::from_utf8_lossy(&std::mem::take(&mut s.partial)).into_owned();
                // Realtime bytes (`!`, `~`, 0x18) arrive unterminated and get no reply.
                let line = line.trim_start_matches(['!', '~', '\u{18}']).to_owned();
                let failing = s.fail_on.as_ref().is_some_and(|(prefix, _)| line.starts_with(prefix.as_str()));
                let reply = match s.answer {
                    Answer::Silent => None,
                    Answer::Grbl if failing => s.fail_on.take().map(|(_, r)| r),
                    Answer::Grbl if line.is_empty() => Some(GRBL_BANNER.into()),
                    Answer::Grbl => Some("ok".into()),
                };
                s.replies.extend(reply);
            }
            self.1.notify_all();
            Ok(())
        }

        fn read_line(&mut self, _timeout: Duration) -> io::Result<Option<String>> {
            Ok(self.0.lock().unwrap().replies.pop_front())
        }
    }

    /// Lists one fake port, opens `link` on it, and saves dry runs into `dir`.
    pub struct FakeHost {
        pub link: FakeLink,
        pub dir: PathBuf,
    }

    pub const FAKE_PORT: &str = "/dev/cu.usbserial-FAKE";

    impl Host for FakeHost {
        fn ports(&self) -> Vec<PortInfo> {
            vec![PortInfo { name: FAKE_PORT.into(), description: "USB: Fake plotter (Kerf tests)".into(), usb: None }]
        }

        fn open(&self, _profile: &DeviceProfile) -> Opener {
            let link = self.link.clone();
            Box::new(move || Ok(Box::new(link) as Box<dyn SerialLink>))
        }

        fn save_path(&self, file_name: &str) -> Option<PathBuf> {
            Some(self.dir.join(file_name))
        }
    }
}

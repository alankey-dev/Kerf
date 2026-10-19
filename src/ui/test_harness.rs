//! The kittest harness every UI test drives the app through, and the input helpers they share.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{Kerf, canvas};
use crate::bridge::BridgeMessage;
use crate::design::{Design, History};
use crate::device::{DeviceState, Disconnected, SimulatedMachine, Transport};
use crate::make_flow::MakeStep;

/// A failure bound only: `pump` never waits this long unless the machine has stopped answering.
const HANG_GUARD: Duration = Duration::from_secs(10);
const MAX_PUMP_STEPS: usize = 2_000;

/// A rendering harness whose app keeps its projects and settings in its own temp directory, removed
/// on drop. Every UI test goes through this, so none can reach the real app-data directory.
pub(super) struct Fixture {
    pub(super) harness: Harness<'static, Kerf>,
    pub(super) projects: PathBuf,
    pub(super) root: tempfile::TempDir,
    machine: MachineLink,
}

impl std::ops::Deref for Fixture {
    type Target = Harness<'static, Kerf>;
    fn deref(&self) -> &Self::Target {
        &self.harness
    }
}

impl std::ops::DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.harness
    }
}

/// The simulated machine, shared by the app and the test so the test can wait for its next reply.
/// Replies the test has waited for are held here and handed to the app first, in order.
struct SharedMachine {
    machine: SimulatedMachine,
    held: RefCell<VecDeque<BridgeMessage>>,
}

#[derive(Clone)]
struct MachineLink(Rc<SharedMachine>);

impl Transport for MachineLink {
    fn send(&self, msg: BridgeMessage) -> Result<(), Disconnected> {
        self.0.machine.send(msg)
    }

    fn try_recv(&self) -> Result<Option<BridgeMessage>, Disconnected> {
        match self.0.held.borrow_mut().pop_front() {
            Some(msg) => Ok(Some(msg)),
            None => self.0.machine.try_recv(),
        }
    }
}

impl MachineLink {
    /// Whether the app has something new to see next frame (a reply, or the machine going away),
    /// blocking on the machine until it does. A running cut moves on one tick per wait, so how far it
    /// gets follows the frames, not how fast this thread is scheduled.
    fn await_reply(&self) -> bool {
        if !self.0.held.borrow().is_empty() {
            return true;
        }
        self.0.machine.advance();
        match self.0.machine.recv_timeout(HANG_GUARD) {
            Ok(msg) => {
                self.0.held.borrow_mut().push_back(msg);
                true
            }
            Err(RecvTimeoutError::Disconnected) => true,
            Err(RecvTimeoutError::Timeout) => false,
        }
    }
}

/// A fresh launch with a blank tab opened from Home, where most tests start.
pub(super) fn snapshot_harness(size: egui::Vec2) -> Fixture {
    open_blank_tab(launch(tempfile::tempdir().unwrap(), size))
}

/// Starts the app over `root`'s app data, as launching it again after quitting does. It opens on Home.
pub(super) fn launch(root: tempfile::TempDir, size: egui::Vec2) -> Fixture {
    build(root, size, SimulatedMachine::stepped(Duration::from_millis(1)))
}

/// File > New from Home: the launch's untouched tab, now on screen.
pub(super) fn open_blank_tab(mut fixture: Fixture) -> Fixture {
    let ctx = fixture.harness.ctx.clone();
    fixture.harness.state_mut().run(super::commands::Command::App(crate::edit::AppCommand::New), &ctx);
    fixture.harness.run();
    fixture
}

/// A harness whose machine reports `tick` per step of a cut, as the Make flow's time readouts show.
pub(super) fn ticking_harness(size: egui::Vec2, tick: Duration) -> Fixture {
    machine_harness(size, SimulatedMachine::stepped(tick))
}

/// A harness over a stepped machine the test made, for example one it can kill.
pub(super) fn machine_harness(size: egui::Vec2, machine: SimulatedMachine) -> Fixture {
    open_blank_tab(build(tempfile::tempdir().unwrap(), size, machine))
}

fn build(root: tempfile::TempDir, size: egui::Vec2, machine: SimulatedMachine) -> Fixture {
    let machine = MachineLink(Rc::new(SharedMachine { machine, held: RefCell::default() }));
    let transport = Box::new(machine.clone());
    let projects = root.path().join("projects");
    let dir = projects.clone();
    let mut harness = Harness::builder().with_size(size).with_pixels_per_point(1.0).wgpu().build_eframe(|cc| {
        // Shortcut labels and text-editing keys follow the OS. Tests drive one, macOS, so the snapshots
        // and the label assertions read the same on every CI runner.
        cc.egui_ctx.set_os(egui::os::OperatingSystem::Mac);
        Kerf::new(&cc.egui_ctx, transport, Some(dir), crate::system_fonts::FontCatalogue::fixture()).with_clock(super::autosave::Clock::fixed())
    });
    // No real ports, so a modal snapshot never depends on what the dev machine has plugged in. Tests
    // that exercise USB detection or an open device swap in their own host.
    harness.state_mut().host = Box::new(NoPorts);
    harness.run();
    Fixture { harness, projects, root, machine }
}

/// A `Host` that lists nothing and opens nothing, for the deterministic default harness.
struct NoPorts;

impl crate::output::link::Host for NoPorts {
    fn ports(&self) -> Vec<crate::output::link::PortInfo> {
        Vec::new()
    }

    fn open(&self, _profile: &crate::output::DeviceProfile) -> crate::output::link::Opener {
        Box::new(|| Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "the test harness opens no ports")))
    }

    fn save_path(&self, _file_name: &str) -> Option<PathBuf> {
        None
    }
}

/// Steps the app until `done` holds, then once more to paint it. Between frames it blocks on the
/// machine's next reply, so the wait follows the conversation with the machine rather than a clock.
pub(super) fn pump(fixture: &mut Fixture, what: &str, done: impl Fn(&Kerf) -> bool) {
    let mut steps = 0;
    while !done(fixture.harness.state()) {
        assert!(steps < MAX_PUMP_STEPS, "{what} not reached in {MAX_PUMP_STEPS} steps");
        fixture.harness.step();
        steps += 1;
        if !done(fixture.harness.state()) {
            assert!(fixture.machine.await_reply(), "timed out waiting for {what}");
        }
    }
    fixture.harness.step();
}

pub(super) fn device_is(app: &Kerf, state: DeviceState) -> bool {
    app.flow.as_ref().is_some_and(|f| f.device_state == state)
}

pub(super) fn step_is(app: &Kerf, step: MakeStep) -> bool {
    app.flow.as_ref().is_some_and(|f| f.step == step)
}

/// Settles, then writes `target/snapshots/<name>` for a person to look at. Nothing compares it, so a
/// test using this must assert what it drew some other way; it exists for the few captures that are
/// worth opening, such as the frames sized to the reference screenshots.
pub(super) fn look_at_snapshot(harness: &mut Harness<'_, Kerf>, name: &str) -> image::RgbaImage {
    harness.run();
    write_snapshot(harness, name)
}

/// The old name of [`look_at_snapshot`], kept while specs 11, 13 and 20 still call it.
pub(super) fn save_snapshot(harness: &mut Harness<'_, Kerf>, name: &str) -> image::RgbaImage {
    look_at_snapshot(harness, name)
}

/// The settled frame, for pixel assertions; nothing is written.
pub(super) fn frame(harness: &mut Harness<'_, Kerf>) -> image::RgbaImage {
    harness.run();
    harness.render().expect("wgpu render")
}

/// The Make flow repaints on a timer, so `run` never settles there; a few steps do.
pub(super) fn flow_frame(harness: &mut Harness<'_, Kerf>) -> image::RgbaImage {
    harness.run_steps(3);
    harness.render().expect("wgpu render")
}

/// Settles like [`frame`], then fails unless the frame matches the committed baseline
/// `tests/snapshots/<name>.png`. `UPDATE_SNAPSHOTS=1 cargo test` rewrites the baselines.
pub(super) fn assert_snapshot(harness: &mut Harness<'_, Kerf>, name: &str) -> image::RgbaImage {
    let image = frame(harness);
    assert_image_snapshot(&image, name);
    image
}

/// `assert_snapshot` for the Make flow, settled as [`flow_frame`] settles it.
pub(super) fn assert_flow_snapshot(harness: &mut Harness<'_, Kerf>, name: &str) -> image::RgbaImage {
    let image = flow_frame(harness);
    assert_image_snapshot(&image, name);
    image
}

/// The largest per-pixel colour difference a snapshot may show: dify's squared YIQ distance, which
/// egui_kittest passes on unscaled, so one step in every RGB channel of a grey costs about 0.5. The
/// committed snapshots come from macOS, where Metal renders them bit-identical run to run.
#[cfg(target_os = "macos")]
const SNAPSHOT_THRESHOLD: f32 = 0.1;
/// Windows renders with the WARP software adapter, which rounds blended edges and gradients one step
/// away from Metal. Measured on windows-latest: every differing pixel is within 0.6.
#[cfg(windows)]
const SNAPSHOT_THRESHOLD: f32 = 1.0;
/// Linux renders with Mesa's lavapipe, which rounds blended edges and gradients up to two steps away
/// from Metal. Measured on ubuntu-latest: every differing pixel is within 4.0, and 2.0 still fails 8
/// snapshots (24-machine-modal by 302 pixels).
#[cfg(not(any(target_os = "macos", windows)))]
const SNAPSHOT_THRESHOLD: f32 = 4.0;

/// Snapshots with an image whose border falls on a pixel boundary. The software renderers sample that
/// border texel differently from Metal, so these pixels take the neighbouring colour outright, which no
/// threshold short of ignoring colour would accept. Each count is the most measured on either runner:
/// 27-prepare-print-page 1 on both; 28-background 14 on Windows, 12 on Linux; 28-trace-panel 4 on
/// Windows, 2 on Linux. macOS allows none.
const EDGE_PIXELS: [(&str, i32); 3] = [("27-prepare-print-page", 1), ("28-background", 14), ("28-trace-panel", 4)];

/// Fails unless `image` matches the committed baseline `tests/snapshots/<name>.png`, for harnesses that
/// are not a whole [`Kerf`], such as the icon sheet.
pub(super) fn assert_image_snapshot(image: &image::RgbaImage, name: &str) {
    let baselines = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    if let Err(err) = compare_image(image, name, &baselines) {
        panic!("{err}");
    }
}

/// The comparison itself, against `<baselines>/<name>.png`, with the suite's threshold and the
/// snapshot's [`EDGE_PIXELS`] allowance off macOS.
fn compare_image(image: &image::RgbaImage, name: &str, baselines: &std::path::Path) -> Result<(), egui_kittest::SnapshotError> {
    let options = egui_kittest::SnapshotOptions::new().threshold(SNAPSHOT_THRESHOLD).output_path(baselines);
    let allowed = if cfg!(target_os = "macos") { 0 } else { EDGE_PIXELS.iter().find(|(n, _)| *n == name).map_or(0, |&(_, px)| px) };
    match egui_kittest::try_image_snapshot_options(image, name, &options) {
        Err(egui_kittest::SnapshotError::Diff { diff, .. }) if diff <= allowed => Ok(()),
        result => result,
    }
}

fn write_snapshot(harness: &mut Harness<'_, Kerf>, name: &str) -> image::RgbaImage {
    let image = harness.render().expect("wgpu render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(name)).unwrap();
    image
}

pub(super) fn rgb(image: &image::RgbaImage, at: egui::Pos2) -> [u8; 3] {
    let p = image.get_pixel(at.x as u32, at.y as u32).0;
    [p[0], p[1], p[2]]
}

pub(super) fn pixel(harness: &mut Harness<'_, Kerf>, at: egui::Pos2) -> [u8; 3] {
    rgb(&harness.render().expect("wgpu render"), at)
}

/// The frame with every shape hidden: the artboard and its grid. The shapes are shown again after.
pub(super) fn bare_frame(harness: &mut Harness<'_, Kerf>) -> image::RgbaImage {
    let hidden: Vec<bool> = harness.state().design.shapes.iter().map(|s| s.hidden).collect();
    harness.state_mut().design.shapes.iter_mut().for_each(|s| s.hidden = true);
    harness.run();
    let image = harness.render().expect("wgpu render");
    harness.state_mut().design.shapes.iter_mut().zip(hidden).for_each(|(s, h)| s.hidden = h);
    harness.run();
    image
}

/// The pixel at `at` with every shape hidden.
pub(super) fn bare(harness: &mut Harness<'_, Kerf>, at: egui::Pos2) -> [u8; 3] {
    rgb(&bare_frame(harness), at)
}

/// The screen rectangle of the widget labelled `label`.
pub(super) fn rect_of(harness: &Harness<'_, Kerf>, label: &str) -> egui::Rect {
    let r = harness.get_by_label(label).bounding_box().unwrap_or_else(|| panic!("{label} has no bounds"));
    egui::Rect::from_min_max(egui::pos2(r.x0 as f32, r.y0 as f32), egui::pos2(r.x1 as f32, r.y1 as f32))
}

/// Whether any pixel in `area` is within `tolerance` of `colour` on every channel.
pub(super) fn any_near(image: &image::RgbaImage, area: egui::Rect, colour: egui::Color32, tolerance: u8) -> bool {
    let near = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
    (area.top() as u32..area.bottom() as u32).any(|y| {
        (area.left() as u32..area.right() as u32).any(|x| {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            near(r, colour.r()) && near(g, colour.g()) && near(b, colour.b())
        })
    })
}

/// The reference screenshots are 2000 px wide at 1.143 px per CSS px, below a 40 px menu bar.
pub(super) const REFERENCE_CSS: egui::Vec2 = egui::vec2(1750.0, 1096.0);

pub(super) fn press_at(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2, pressed: bool) {
    let button = egui::PointerButton::Primary;
    harness.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE });
    harness.run();
}

pub(super) fn move_to(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    harness.run();
}

pub(super) fn click_at(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2) {
    move_to(harness, pos);
    press_at(harness, pos, true);
    press_at(harness, pos, false);
}

/// A kittest step is a quarter second and egui's double-click window 0.3 s, so each click is one frame.
pub(super) fn double_click_at(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2) {
    let button = egui::PointerButton::Primary;
    harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    harness.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE });
        }
        harness.step();
    }
    harness.run();
}

pub(super) fn right_click_at(harness: &mut Harness<'_, Kerf>, pos: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
    }
    harness.run();
}

pub(super) fn drag_along(harness: &mut Harness<'_, Kerf>, path: &[egui::Pos2]) {
    move_to(harness, path[0]);
    press_at(harness, path[0], true);
    for &p in &path[1..] {
        move_to(harness, p);
    }
    press_at(harness, *path.last().unwrap(), false);
}

pub(super) fn key(harness: &mut Harness<'_, Kerf>, key: egui::Key, modifiers: egui::Modifiers) {
    harness.input_mut().modifiers = modifiers;
    harness.input_mut().events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    harness.step();
    harness.input_mut().modifiers = egui::Modifiers::NONE;
    harness.run();
}

/// Clicks the last widget labelled `label`: a context menu draws last, so its row is the last match.
pub(super) fn pick(harness: &mut Harness<'_, Kerf>, label: &str) {
    harness.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label}")).click();
    harness.run();
}

/// Two squares side by side, unselected, and a map from inches to screen points.
pub(super) fn two_squares_apart(harness: &mut Harness<'_, Kerf>) -> impl Fn((f32, f32)) -> egui::Pos2 + use<> {
    harness.state_mut().design.set_snapping(false);
    harness.state_mut().tool = canvas::Tool::Text;
    let probe = egui::pos2(600.0, 400.0);
    click_at(harness, probe);
    let text = harness.state().design.shapes[0].clone();
    let px = 72.0 * harness.state().view.zoom;
    let origin = probe - egui::vec2(text.x_in, text.y_in) * px;
    let mut design = Design::default();
    design.add_shape(crate::design::ShapeKind::Square);
    design.add_shape(crate::design::ShapeKind::Square);
    (design.shapes[0].x_in, design.shapes[0].y_in) = (1.0, 1.0);
    (design.shapes[1].x_in, design.shapes[1].y_in) = (4.0, 1.0);
    design.set_snapping(false);
    harness.state_mut().design = design;
    harness.state_mut().selection.clear();
    harness.state_mut().history = History::default();
    harness.run();
    move |(x, y)| origin + egui::vec2(x, y) * px
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 96 x 64 frame with a smooth gradient, so a changed block cannot hide in a flat area.
    fn frame_of_gradient() -> image::RgbaImage {
        image::RgbaImage::from_fn(96, 64, |x, y| image::Rgba([(x * 2) as u8, (y * 3) as u8, 120, 255]))
    }

    /// Guards the suite's own premise: an assertion that cannot fail makes every baseline decorative.
    #[test]
    fn a_baseline_comparison_fails_on_a_small_one_region_change() {
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            return; // update mode rewrites the baseline instead of comparing
        }
        let dir = tempfile::tempdir().unwrap();
        let baseline = frame_of_gradient();
        baseline.save(dir.path().join("gradient.png")).unwrap();
        compare_image(&baseline, "gradient", dir.path()).expect("an identical frame matches its baseline");

        let mut changed = baseline.clone();
        for (x, y) in (40..46).flat_map(|x| (20..26).map(move |y| (x, y))) {
            changed.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
        assert!(compare_image(&changed, "gradient", dir.path()).is_err(), "a 6 x 6 block of 96 x 64 changed colour must fail the comparison");
    }
}

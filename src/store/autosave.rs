//! When to autosave, driven by the frame's monotonic time in seconds: 2 s after the last change, at
//! least every 30 s while changes keep coming, never while a gesture is open, and a version every
//! 10 minutes.

pub const QUIET_S: f64 = 2.0;
pub const MAX_WAIT_S: f64 = 30.0;
pub const VERSION_EVERY_S: f64 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    Idle,
    /// Write the autosave, and record an Auto version too when `version`.
    Write {
        version: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Autosave {
    /// The first change not yet written, and the latest.
    pending: Option<(f64, f64)>,
    last_version: f64,
}

impl Autosave {
    pub fn new(now: f64) -> Self {
        Self { pending: None, last_version: now }
    }

    pub fn tick(&mut self, now: f64, changed: bool, gesture_open: bool) -> Due {
        if changed {
            let first = self.pending.map_or(now, |(first, _)| first);
            self.pending = Some((first, now));
        }
        let Some((first, last)) = self.pending else { return Due::Idle };
        if gesture_open || (now - last < QUIET_S && now - first < MAX_WAIT_S) {
            return Due::Idle;
        }
        self.pending = None;
        let version = now - self.last_version >= VERSION_EVERY_S;
        if version {
            self.last_version = now;
        }
        Due::Write { version }
    }

    /// A Save wrote the project and recorded a version, so nothing is pending.
    pub fn saved(&mut self, now: f64) {
        *self = Self::new(now);
    }

    /// Whether a write is owed now, without waiting for quiet: a change this frame or one still pending.
    /// Owed or not, nothing is pending afterwards. A tab leaving the foreground or the app quitting asks.
    pub fn flush(&mut self, changed: bool) -> bool {
        changed | self.pending.take().is_some()
    }

    /// Seconds until the pending write falls due, for the frame to wake then.
    pub fn wake_in(&self, now: f64) -> Option<f64> {
        let (first, last) = self.pending?;
        Some(((last + QUIET_S).min(first + MAX_WAIT_S) - now).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks every quarter second, as a kittest step does, from `from` up to `to`.
    fn run(a: &mut Autosave, from: f64, to: f64, changed: impl Fn(f64) -> bool, gesture: bool) -> Vec<(f64, Due)> {
        let mut writes = Vec::new();
        let mut t = from;
        while t <= to {
            let due = a.tick(t, changed(t), gesture);
            if due != Due::Idle {
                writes.push((t, due));
            }
            t += 0.25;
        }
        writes
    }

    #[test]
    fn a_change_then_two_quiet_seconds_writes_the_autosave_file_and_not_before() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::project::Project::new(crate::design::Design::default());
        let file = crate::store::autosave_path(dir.path(), &project.id);
        let mut a = Autosave::new(0.0);
        let mut t = 1.0;
        let on_tick = |a: &mut Autosave, t: f64, changed: bool| {
            if a.tick(t, changed, false) != Due::Idle {
                crate::store::write_autosave(dir.path(), &project).unwrap();
            }
        };
        on_tick(&mut a, t, true);
        while t < 2.75 {
            t += 0.25;
            on_tick(&mut a, t, false);
        }
        assert!(!file.exists(), "1.75 s after the change is not yet quiet enough");
        on_tick(&mut a, 3.0, false);
        assert!(file.exists(), "2 s after the change writes autosave/<id>.json");
        assert_eq!(crate::store::read_autosave(dir.path(), &project.id).unwrap().unwrap().id, project.id);
    }

    #[test]
    fn a_change_then_two_quiet_seconds_writes_once() {
        let mut a = Autosave::new(0.0);
        let writes = run(&mut a, 0.0, 10.0, |t| t == 1.0, false);
        assert_eq!(writes, [(3.0, Due::Write { version: false })]);
        assert_eq!(a.wake_in(10.0), None);
    }

    #[test]
    fn continuous_changes_still_write_within_thirty_seconds() {
        let mut a = Autosave::new(0.0);
        let every_half_second = |t: f64| (t * 2.0).fract() == 0.0;
        let writes = run(&mut a, 0.0, 61.0, every_half_second, false);
        assert_eq!(writes.iter().map(|w| w.0).collect::<Vec<_>>(), [30.0, 60.5], "every 30 s from the first unwritten change");
    }

    #[test]
    fn nothing_is_written_while_a_gesture_is_open() {
        let mut a = Autosave::new(0.0);
        assert!(run(&mut a, 0.0, 40.0, |t| t < 35.0, true).is_empty());
        assert_eq!(a.tick(40.25, false, false), Due::Write { version: false }, "written once the gesture ends");
    }

    #[test]
    fn a_version_is_due_every_ten_minutes_and_a_save_restarts_the_count() {
        let mut a = Autosave::new(0.0);
        assert_eq!(a.tick(1.0, true, false), Due::Idle);
        assert_eq!(a.tick(3.0, false, false), Due::Write { version: false });
        a.tick(600.0, true, false);
        assert_eq!(a.tick(602.0, false, false), Due::Write { version: true });
        a.saved(700.0);
        a.tick(1250.0, true, false);
        assert_eq!(a.tick(1252.0, false, false), Due::Write { version: false }, "552 s since the save's version");
    }

    #[test]
    fn a_flush_owes_a_pending_change_once_without_waiting() {
        let mut a = Autosave::new(0.0);
        a.tick(1.0, true, false);
        assert!(a.flush(false), "the change is written at once");
        assert!(!a.flush(false), "and only once");
        assert_eq!(a.tick(10.0, false, false), Due::Idle, "nothing waits for quiet after it");
        assert!(a.flush(true), "a change this frame is owed too");
    }

    #[test]
    fn wake_in_counts_down_to_the_due_write() {
        let mut a = Autosave::new(0.0);
        a.tick(1.0, true, false);
        assert_eq!(a.wake_in(1.5), Some(1.5));
    }
}

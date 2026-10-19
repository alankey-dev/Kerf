//! Runs a nesting [`Job`] on its own thread, so Prepare never blocks while it works.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use super::{Job, NestedLayout};

pub struct Worker {
    started: Instant,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    done: Receiver<NestedLayout>,
    #[cfg(test)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn start(job: Job, budget: Duration) -> Worker {
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, done) = channel();
        let flag = cancel.clone();
        tracing::info!(target: "flow", groups = job.groups.len(), pieces = job.groups.iter().map(|g| g.pieces.len()).sum::<usize>(), ?budget, "nesting started");
        let thread = std::thread::Builder::new()
            .name("nesting".into())
            .spawn(move || {
                let started = Instant::now();
                let layout = job.run(budget, &flag);
                tracing::info!(target: "flow", mats = layout.mats.len(), elapsed = ?started.elapsed(), cancelled = flag.load(Ordering::Relaxed), "nesting finished");
                _ = send.send(layout);
            })
            .expect("the nesting thread starts");
        #[cfg(not(test))]
        drop(thread);
        Worker {
            started: Instant::now(),
            budget,
            cancel,
            done,
            #[cfg(test)]
            thread: Some(thread),
        }
    }

    /// Elapsed time over the budget, 0 to 1.
    pub fn progress(&self) -> f32 {
        if self.budget.is_zero() {
            return 1.0;
        }
        (self.started.elapsed().as_secs_f32() / self.budget.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Stops the improvement loop; the best layout so far still arrives.
    pub fn cancel(&self) {
        tracing::info!(target: "flow", "nesting cancelled");
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Blocks until the thread has sent its layout, which `try_result` then returns. Tests join the
    /// thread instead of sleeping and polling.
    #[cfg(test)]
    pub fn wait_finished(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread.join().expect("the nesting thread finished");
        }
    }

    /// The finished layout, once.
    pub fn try_result(&self) -> Option<NestedLayout> {
        self.done.try_recv().ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{Local, PieceId};
    use crate::nesting::{Group, MatArea, NestSettings, Piece};

    fn job(n: usize) -> Job {
        let square = |i: usize| Piece { lines: vec![(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], true)], grain_locked: i == 0 };
        let group = Group {
            colour: egui::Color32::BLACK,
            units: (0..n).map(|i| (PieceId { owner: i as u64, local: Local::Body }, 0)).collect(),
            pieces: (0..n).map(square).collect(),
        };
        Job { fingerprint: 7, settings: NestSettings::default(), mat: MatArea { width_in: 12.0, height_in: 12.0 }, groups: vec![group] }
    }

    fn wait(worker: &mut Worker) -> NestedLayout {
        worker.wait_finished();
        worker.try_result().expect("the worker sent a layout")
    }

    #[test]
    fn the_worker_returns_the_same_layout_as_running_the_job_directly() {
        let mut worker = Worker::start(job(5), Duration::from_secs(20));
        let layout = wait(&mut worker);
        assert_eq!(layout, job(5).run(Duration::from_secs(20), &AtomicBool::new(false)));
        assert_eq!(layout.fingerprint, 7);
        assert_eq!(layout.mats[0].placements.len(), 5);
        assert!(worker.try_result().is_none(), "a result arrives once");
    }

    #[test]
    fn cancel_still_returns_a_layout_and_progress_stays_in_range() {
        let mut worker = Worker::start(job(12), Duration::from_secs(60));
        assert!((0.0..=1.0).contains(&worker.progress()));
        worker.cancel();
        let layout = wait(&mut worker);
        assert_eq!(layout.mats.iter().map(|m| m.placements.len()).sum::<usize>(), 12, "the greedy pass always completes");
    }
}

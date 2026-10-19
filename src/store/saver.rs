//! The saver thread: the only writer of the projects directory while the app runs. The UI sends it
//! cloned projects over a channel and never serialises or touches the disk itself. Jobs run in the
//! order sent. Background jobs report through [`Done`]; the user's own Save, Restore and Delete wait
//! for their answer, which keeps their results in the same frame.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use super::session::Session;
use super::{VersionId, VersionKind};
use crate::components::Component;
use crate::design::{Design, Shape};
use crate::nesting::NestedLayout;
use crate::project::Project;
use crate::system_fonts::FontCatalogue;

/// A project as it crosses to the saver thread. A `Design` shares a geometry cache that is not `Send`,
/// so the saver rebuilds each design around a cache of its own.
pub struct Snapshot {
    id: String,
    name: String,
    shapes: Vec<Shape>,
    nesting: Option<NestedLayout>,
    components: Vec<Component>,
    link_names: BTreeMap<u64, String>,
    guides: Vec<crate::guides::RulerGuide>,
    guides_locked: bool,
    guides_hidden: bool,
    grid: Option<crate::grid::GridSettings>,
    snap: crate::snap::SnapOptions,
    swatches: Vec<crate::settings::Swatch>,
    created: u64,
    modified: u64,
}

impl From<&Project> for Snapshot {
    fn from(p: &Project) -> Self {
        // A master open for editing is closed on the copy, so the canvas is what gets saved.
        let closed;
        let design = match p.design.editing {
            Some(_) => {
                closed = Design { cache: Default::default(), ..p.design.clone() }.closed();
                &closed
            }
            None => &p.design,
        };
        // Exhaustive, so a new `Design` field fails to compile here instead of being dropped on the way.
        let Design { shapes, nesting, components, editing: _, link_names, guides, guides_locked, guides_hidden, grid, snap, swatches, cache: _ } = design;
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            shapes: shapes.clone(),
            nesting: nesting.clone(),
            components: components.clone(),
            link_names: link_names.clone(),
            guides: guides.clone(),
            guides_locked: *guides_locked,
            guides_hidden: *guides_hidden,
            grid: *grid,
            snap: *snap,
            swatches: swatches.clone(),
            created: p.created,
            modified: p.modified,
        }
    }
}

impl Snapshot {
    pub fn project(self) -> Project {
        Project {
            id: self.id,
            name: self.name,
            design: Design {
                shapes: self.shapes,
                nesting: self.nesting,
                components: self.components,
                link_names: self.link_names,
                guides: self.guides.clone(),
                guides_locked: self.guides_locked,
                guides_hidden: self.guides_hidden,
                grid: self.grid,
                snap: self.snap,
                swatches: self.swatches,
                ..Design::default()
            },
            created: self.created,
            modified: self.modified,
        }
    }
}

pub enum Job {
    Autosave {
        project: Snapshot,
        seq: u64,
    },
    ClearAutosave {
        id: String,
    },
    Version {
        project: Snapshot,
        kind: VersionKind,
        now_ms: u64,
    },
    NameVersion {
        id: String,
        version: VersionId,
        name: String,
    },
    DiscardAutosave {
        id: String,
        now_ms: u64,
    },
    /// Removes a closed never-saved design's autosave, unless a tab in `open` has it.
    DeleteUnsaved {
        id: String,
        open: Vec<String>,
    },
    Session(Session),
    Save {
        project: Snapshot,
        now_ms: u64,
        reply: Sender<io::Result<()>>,
    },
    Restore {
        current: Snapshot,
        version: VersionId,
        now_ms: u64,
        reply: Sender<io::Result<Snapshot>>,
    },
    Delete {
        id: String,
        reply: Sender<io::Result<()>>,
    },
    ClearAll {
        reply: Sender<io::Result<usize>>,
    },
    /// Answers once every job sent before it has run.
    Flush {
        reply: Sender<()>,
    },
}

/// What a background job did, for the UI to read each frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Autosaved { id: String, seq: u64 },
    Versioned { id: String },
    Failed { what: &'static str, error: String },
}

pub struct Saver {
    jobs: Option<Sender<Job>>,
    done: Receiver<Done>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    /// Starts the thread. Thumbnails lay text out in `fonts`, as the canvas does.
    pub fn spawn(dir: PathBuf, fonts: FontCatalogue) -> Self {
        let (jobs, inbox) = channel();
        let (outbox, done) = channel();
        let work = move || {
            fonts.make_current();
            inbox.into_iter().for_each(|job| run(&dir, job, &outbox));
        };
        let thread = std::thread::Builder::new().name("saver".into()).spawn(work).expect("spawn the saver thread");
        Self { jobs: Some(jobs), done, thread: Some(thread) }
    }

    /// Queues a background job.
    pub fn send(&self, job: Job) {
        if self.jobs.as_ref().is_none_or(|j| j.send(job).is_err()) {
            tracing::error!(target: "error", "the saver thread stopped");
        }
    }

    pub fn save(&self, project: &Project, now_ms: u64) -> io::Result<()> {
        self.ask(|reply| Job::Save { project: project.into(), now_ms, reply })
    }

    /// Records `current` as BeforeRestore and returns `version`'s project.
    pub fn restore(&self, current: &Project, version: VersionId, now_ms: u64) -> io::Result<Project> {
        self.ask(|reply| Job::Restore { current: current.into(), version, now_ms, reply }).map(Snapshot::project)
    }

    pub fn delete(&self, id: String) -> io::Result<()> {
        self.ask(|reply| Job::Delete { id, reply })
    }

    pub fn clear_all(&self) -> io::Result<usize> {
        self.ask(|reply| Job::ClearAll { reply })
    }

    /// Waits until every job sent so far has run.
    pub fn flush(&self) {
        let (reply, answer) = channel();
        self.send(Job::Flush { reply });
        _ = answer.recv();
    }

    /// The next result of a background job, if one is ready.
    pub fn try_recv(&self) -> Option<Done> {
        self.done.try_recv().ok()
    }

    fn ask<T>(&self, job: impl FnOnce(Sender<io::Result<T>>) -> Job) -> io::Result<T> {
        let (reply, answer) = channel();
        self.send(job(reply));
        answer.recv().unwrap_or_else(|_| Err(io::Error::other("the saver thread stopped")))
    }
}

impl Drop for Saver {
    /// Closes the queue and waits for the jobs already in it, so a dropped app leaves nothing writing.
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            _ = thread.join();
        }
    }
}

fn run(dir: &std::path::Path, job: Job, done: &Sender<Done>) {
    let report = |what: &'static str, result: io::Result<()>, ok: Option<Done>| {
        let event = match result {
            Ok(()) => ok,
            Err(e) => {
                tracing::error!(target: "error", what, error = %e, "background save failed");
                Some(Done::Failed { what, error: e.to_string() })
            }
        };
        if let Some(event) = event {
            _ = done.send(event);
        }
    };
    match job {
        Job::Autosave { project, seq } => {
            let project = project.project();
            let result = super::write_autosave(dir, &project);
            tracing::debug!(target: "save", id = %project.id, seq, ok = result.is_ok(), "autosave");
            report("autosave", result, Some(Done::Autosaved { id: project.id, seq }));
        }
        Job::ClearAutosave { id } => report("autosave", super::clear_autosave(dir, &id), None),
        Job::Version { project, kind, now_ms } => {
            let project = project.project();
            let result = super::record_version(dir, &project, kind, now_ms).map(drop);
            report("version", result, Some(Done::Versioned { id: project.id }));
        }
        Job::NameVersion { id, version, name } => report("version name", super::name_version(dir, &id, version, name), None),
        Job::DiscardAutosave { id, now_ms } => report("autosave", super::discard_autosave(dir, &id, now_ms), None),
        Job::DeleteUnsaved { id, open } => report("unsaved delete", super::unsaved::delete(dir, &id, &open), None),
        Job::Session(session) => report("session", super::session::write(dir, &session), None),
        Job::Save { project, now_ms, reply } => _ = reply.send(super::save_project(dir, &project.project(), now_ms)),
        Job::Restore { current, version, now_ms, reply } => _ = reply.send(super::restore(dir, &current.project(), version, now_ms).map(|p| (&p).into())),
        Job::Delete { id, reply } => _ = reply.send(super::delete_project(dir, &id)),
        Job::ClearAll { reply } => _ = reply.send(super::clear_all(dir)),
        Job::Flush { reply } => _ = reply.send(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, ShapeKind};
    use crate::store;

    fn star(id: &str) -> Project {
        let mut design = Design::default();
        design.add_shape(ShapeKind::Star);
        Project { id: id.into(), name: "Star".into(), design, created: 1, modified: 1 }
    }

    #[test]
    fn jobs_run_in_order_and_report_back() {
        let dir = tempfile::tempdir().unwrap();
        let saver = Saver::spawn(dir.path().to_owned(), FontCatalogue::fixture());
        saver.send(Job::Autosave { project: (&star("p")).into(), seq: 1 });
        saver.flush();
        assert!(store::autosave_path(dir.path(), "p").exists());
        assert_eq!(saver.try_recv(), Some(Done::Autosaved { id: "p".into(), seq: 1 }));

        saver.save(&star("p"), 5_000).unwrap();
        assert!(!store::autosave_path(dir.path(), "p").exists(), "Save clears the autosave");
        assert_eq!(store::read_index(dir.path(), "p").unwrap().versions[0].kind, VersionKind::Manual);

        saver.send(Job::Autosave { project: (&Project { id: "../x".into(), ..star("p") }).into(), seq: 2 });
        saver.flush();
        assert!(matches!(saver.try_recv(), Some(Done::Failed { what: "autosave", .. })));
    }

    #[test]
    fn a_saved_project_keeps_its_optimised_layout() {
        let dir = tempfile::tempdir().unwrap();
        let saver = Saver::spawn(dir.path().to_owned(), FontCatalogue::fixture());
        let mut project = star("p");
        let mat = crate::machine::MachineFamily::StudioPro.preferred_mat();
        project.design.nesting =
            Some(crate::composition::nested::optimise(&project.design, mat, 1, Default::default(), std::time::Duration::from_secs(5), &Default::default()));
        saver.save(&project, 5_000).unwrap();
        assert_eq!(crate::project::load(dir.path(), "p").unwrap().design.nesting, project.design.nesting);
    }

    #[test]
    fn dropping_the_saver_finishes_the_queue_first() {
        let dir = tempfile::tempdir().unwrap();
        let saver = Saver::spawn(dir.path().to_owned(), FontCatalogue::fixture());
        for seq in 0..20 {
            saver.send(Job::Autosave { project: (&star(&format!("p{seq}"))).into(), seq });
        }
        drop(saver);
        assert!(store::autosave_path(dir.path(), "p19").exists());
    }
}

//! The open documents as tabs. The active one is `Kerf.doc`, so app code never looks it up; the others
//! wait on a [`Shelf`] either side of it, so there is always exactly one active document.
//!
//! Every change of tab runs after `History::observe` (see `AppCommand::after_observe`) or before the
//! frame's design snapshot (the dialogs), so no undo step ever spans two documents.

use crate::store::session::Session;

use super::commands::Command;
use super::document::Document;
use super::modal::{self, Choice};
use super::{Kerf, chrome, tab_bar};

/// The tabs other than the active one, both sides in strip order: `left` precedes the active tab and
/// `right[0]` follows it.
#[derive(Default)]
pub(super) struct Shelf {
    left: Vec<Document>,
    right: Vec<Document>,
}

impl Shelf {
    /// Every document in strip order, `active` in its place between the two sides.
    pub(super) fn with_active_mut<'a>(&'a mut self, active: &'a mut Document) -> impl Iterator<Item = &'a mut Document> {
        self.left.iter_mut().chain(std::iter::once(active)).chain(&mut self.right)
    }
}

impl Kerf {
    pub(super) fn active_index(&self) -> usize {
        self.shelf.left.len()
    }

    pub(super) fn tab_count(&self) -> usize {
        self.shelf.left.len() + 1 + self.shelf.right.len()
    }

    /// Every open document in strip order, the active one included.
    pub(super) fn docs(&self) -> impl Iterator<Item = &Document> {
        self.shelf.left.iter().chain(std::iter::once(&self.doc)).chain(&self.shelf.right)
    }

    pub(super) fn docs_mut(&mut self) -> impl Iterator<Item = &mut Document> {
        self.shelf.with_active_mut(&mut self.doc)
    }

    /// The document whose Make flow is open. There is at most one, so machine and device messages go
    /// to it whichever tab is active.
    pub(super) fn flow_owner_mut(&mut self) -> Option<&mut Document> {
        self.docs_mut().find(|d| d.flow.is_some())
    }

    /// What `session.json` records: the open documents' ids in strip order and which is active. A
    /// transient tab is left out, and the tab before it stands in when it is active.
    pub(super) fn session(&self, pid: Option<u32>) -> Session {
        let tabs: Vec<String> = self.docs().filter(|d| !d.transient).map(|d| d.autosave.id.clone()).collect();
        let before = self.docs().take(self.active_index()).filter(|d| !d.transient).count();
        let active = if self.doc.transient { before.saturating_sub(1) } else { before };
        Session { tabs, active, pid }
    }

    /// The tab strip, when it shows: always on Home, elsewhere once two documents are open.
    pub(super) fn tab_strip(&self, ctx: &egui::Context, on_home: bool) -> Option<Command> {
        if !on_home && self.tab_count() < 2 {
            return None;
        }
        let chips: Vec<tab_bar::Chip> = self.docs().map(|d| tab_bar::Chip { title: &d.title, edited: d.has_unsaved_changes() && !d.transient }).collect();
        tab_bar::show(ctx, &chips, (!on_home).then(|| self.active_index()))
    }

    /// Cycles to the tab `by` places along, wrapping at either end.
    pub(super) fn step_tab(&mut self, by: isize) {
        let n = self.tab_count() as isize;
        self.switch_to((self.active_index() as isize + by).rem_euclid(n) as usize);
    }

    /// The strip index of the document with this id.
    pub(super) fn tab_of(&self, id: &str) -> Option<usize> {
        self.docs().position(|d| d.autosave.id == id)
    }

    /// "Untitled-N" with the lowest N no open tab is called.
    fn untitled(&self) -> String {
        (1..).map(|n| format!("Untitled-{n}")).find(|t| !self.docs().any(|d| &d.title == t)).expect("some number is free")
    }

    /// A blank canvas under `id`, as a never-saved document with an autosave reopens.
    pub(super) fn blank_under(&self, id: String) -> Document {
        let mut doc = Document::blank(self.untitled());
        doc.autosave.id = id;
        doc
    }

    /// File > New: an empty canvas in a new tab.
    pub(super) fn new_tab(&mut self) {
        let title = if self.doc.is_pristine() { self.doc.title.clone() } else { self.untitled() };
        self.push_tab(Document::blank(title));
    }

    /// Shows the saved project `id`: its tab if open, else a new tab with it, offering a newer autosave.
    /// Returns whether it is showing.
    pub(super) fn open_tab(&mut self, id: &str) -> bool {
        if let Some(i) = self.tab_of(id) {
            self.switch_to(i);
            return true;
        }
        let Some(project) = self.load(id) else { return false };
        let offer = self.newer_autosave(id, Some(project.modified));
        self.push_tab(Document::open(project));
        self.doc.autosave.offer = offer;
        true
    }

    /// Home > Open on an unsaved card: a blank tab under the design's id, offering its autosave for
    /// Restore or Discard as a reopened never-saved tab does. Its tab, if it has one, shows instead.
    pub(super) fn open_unsaved(&mut self, id: &str) {
        if let Some(i) = self.tab_of(id) {
            self.switch_to(i);
            return;
        }
        let Some(autosave) = self.newer_autosave(id, None) else {
            self.report("Couldn't open the unsaved work: it is gone or unreadable.".into());
            self.refresh_home();
            return;
        };
        let offer = Some(autosave);
        let doc = self.blank_under(id.to_owned());
        self.push_tab(doc);
        self.doc.autosave.offer = offer;
    }

    /// Opens `doc` in a tab right after the active one and makes it active. A pristine active tab gives
    /// way to it instead, unless `doc` is transient.
    pub(super) fn push_tab(&mut self, mut doc: Document) {
        doc.mat = self.doc.mat;
        self.tab = chrome::Tab::Canvas;
        if self.doc.is_pristine() && !doc.transient {
            self.doc = doc;
        } else {
            self.leave_foreground();
            let was = std::mem::replace(&mut self.doc, doc);
            self.shelf.left.push(was);
        }
        self.tabs_changed();
    }

    /// Reopens `docs` in their order with `docs[active]` active, as launch and crash recovery do. They
    /// take the place of a lone untouched blank tab, and otherwise follow the active tab.
    pub(super) fn restore_tabs(&mut self, mut docs: Vec<Document>, active: usize) {
        let Some(last) = docs.len().checked_sub(1) else { return };
        let active = active.min(last);
        if self.tab_count() == 1 && self.doc.is_pristine() {
            let right = docs.split_off(active + 1);
            self.doc = docs.pop().expect("active is in range");
            self.shelf = Shelf { left: docs, right };
            self.tabs_changed();
            return;
        }
        let id = docs[active].autosave.id.clone();
        for doc in docs {
            self.push_tab(doc);
        }
        if let Some(i) = self.tab_of(&id) {
            self.switch_to(i);
        }
    }

    /// Makes tab `i` active.
    pub(super) fn switch_to(&mut self, i: usize) {
        let active = self.active_index();
        self.tab = chrome::Tab::Canvas;
        if i == active || i >= self.tab_count() {
            return;
        }
        self.leave_foreground();
        let mut strip: Vec<Document> = std::mem::take(&mut self.shelf.left);
        strip.append(&mut self.shelf.right);
        let next = strip.remove(if i < active { i } else { i - 1 });
        let was = std::mem::replace(&mut self.doc, next);
        strip.insert(if active < i { active } else { active - 1 }, was);
        self.shelf.right = strip.split_off(i);
        self.shelf.left = strip;
        self.tabs_changed();
    }

    /// Closes tab `i` without asking: its owed autosave is written, so the next open offers it. A Make
    /// running there is cancelled on the device first. Closing the last tab leaves a blank one.
    pub(super) fn close_tab(&mut self, i: usize) {
        let Self { doc, shelf, saving, .. } = self;
        let Some(closing) = shelf.with_active_mut(doc).nth(i) else { return };
        saving.flush(closing);
        self.drop_tab(i);
        if self.tab == chrome::Tab::Home {
            self.refresh_home();
        }
    }

    /// Closes every tab `gone` picks, writing nothing for them: Home deleted their projects.
    pub(super) fn drop_tabs(&mut self, gone: impl Fn(&Document) -> bool) {
        let doomed: Vec<usize> = self.docs().enumerate().filter(|(_, d)| gone(d)).map(|(i, _)| i).collect();
        for i in doomed.into_iter().rev() {
            self.drop_tab(i);
        }
    }

    pub(super) fn drop_tab(&mut self, i: usize) {
        let active = self.active_index();
        let making = self.docs().nth(i).is_some_and(|d| d.flow.is_some());
        if making {
            self.close_device();
        }
        tracing::info!(target: "session", tab = i, active, making, "close tab");
        let gone = match i.cmp(&active) {
            std::cmp::Ordering::Less => self.shelf.left.remove(i),
            std::cmp::Ordering::Greater => self.shelf.right.remove(i - active - 1),
            std::cmp::Ordering::Equal => {
                let next = if self.shelf.right.is_empty() { self.shelf.left.pop() } else { Some(self.shelf.right.remove(0)) };
                let next = next.unwrap_or_else(|| Document { mat: self.doc.mat, ..Document::blank("Untitled-1".into()) });
                std::mem::replace(&mut self.doc, next)
            }
        };
        if self.image_owner.as_deref() == Some(gone.autosave.id.as_str()) {
            self.image_owner = None;
        }
        self.tabs_changed();
    }

    /// Asks what to do when Make started in one tab while another tab's Make has the machine: leave it,
    /// cancel that Make and start this one, or go to that tab.
    pub(super) fn one_make_dialog(&mut self, ctx: &egui::Context) {
        let Some(owner) = self.make_elsewhere.clone() else { return };
        let Some((i, title)) = self.docs().enumerate().find(|(_, d)| d.autosave.id == owner && d.flow.is_some()).map(|(i, d)| (i, d.title.clone())) else {
            self.make_elsewhere = None;
            return;
        };
        let go = format!("Go to {title}");
        let choices = [
            Choice { label: "Not now", a11y: "Not now", primary: false },
            Choice { label: "Cancel that Make", a11y: "Cancel that Make", primary: false },
            Choice { label: &go, a11y: &go, primary: true },
        ];
        let detail = format!("{title} is using the machine. One Make runs at a time.");
        let Some(choice) = modal::show(ctx, "one-make", "Another Make is open", &detail, &choices, 0) else { return };
        tracing::info!(target: "cmd", owner = %owner, choice = ["not now", "cancel that make", "go to"][choice], "one make");
        self.make_elsewhere = None;
        match choice {
            1 => {
                self.close_device();
                if let Some(d) = self.docs_mut().nth(i) {
                    d.flow = None;
                }
                self.start_make();
            }
            2 => self.switch_to(i),
            _ => {}
        }
    }

    /// The active tab is going to the background: its pending autosave is written now, and the master
    /// and any gesture on its canvas close, so nothing is left half done behind the strip.
    fn leave_foreground(&mut self) {
        self.saving.flush(&mut self.doc);
        self.doc.close_isolation();
        self.doc.drop_canvas_sessions();
    }

    fn tabs_changed(&mut self) {
        tracing::info!(target: "session", tabs = self.tab_count(), active = self.active_index(), "tabs");
        self.claim_session();
    }
}

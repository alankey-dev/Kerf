//! One open document: the design and everything that belongs to it rather than to the app.

use crate::design::{Design, History};
use crate::machine::{MachineFamily, MatSize};
use crate::make_flow::MakeFlow;

use super::{autosave, canvas, component_isolation, cut_preview, export_dialog, pattern_panel, persistence, prepare_nest, shapes_panel, trace_panel};

pub struct Document {
    pub(super) design: Design,
    pub(super) selection: Vec<u64>,
    pub(super) history: History,
    pub(super) view: canvas::View,
    pub(super) tool: canvas::Tool,
    pub(super) title: String,
    pub(super) project: Option<crate::project::Project>,
    pub(super) saved: persistence::Saved,
    /// Its autosave. The `id` there is the document's id.
    pub(super) autosave: autosave::DocSave,
    pub(super) flow: Option<MakeFlow>,
    /// Prepare's Optimise layout run and summary.
    pub(super) nest: prepare_nest::Nest,
    /// The open cut preview, from Prepare or View > Cut preview.
    pub(super) cut_preview: Option<cut_preview::CutPreview>,
    /// The component master open for editing, if any (spec 31).
    pub(super) isolation: Option<component_isolation::Isolation>,
    pub(super) shapes_open: bool,
    pub(super) shapes_filter: shapes_panel::ShapeFilter,
    /// The open Monogram panel, in the Shapes panel's slot (spec 42).
    pub(super) monogram: Option<super::monogram_panel::MonogramPanel>,
    pub(super) preview_open: bool,
    pub(super) contour_open: bool,
    /// The mat the artboard follows: the machine's preferred mat until a Make flow chooses another.
    pub(super) mat: MatSize,
    /// The open File > Export dialog.
    pub(super) export: Option<export_dialog::Dialog>,
    /// The open Replace colour dialog (spec 56).
    pub(super) replace_colour: Option<super::replace_colour::Dialog>,
    /// The open trace panel, which takes the window until it applies or closes (spec 28).
    pub(super) trace: Option<trace_panel::TracePanel>,
    /// The shapes a pattern image being uploaded will fill, until the image worker answers.
    pub(super) pattern_upload: Option<(Vec<u64>, pattern_panel::Target)>,
    /// Never saved, autosaved or reopened: the tutorial's tab.
    pub(super) transient: bool,
    /// The last duplicate's step, so Cmd+D repeats it and Cmd+Shift+D applies it without a copy (spec
    /// 52). Cleared when the selection becomes something other than the copies it holds.
    pub(super) duplicate_memory: Option<crate::repeat::DuplicateMemory>,
    /// The open Step and repeat or Rotate copies popover, with its live preview (spec 52).
    pub(super) repeat_panel: Option<super::repeat_panel::RepeatPanel>,
}

impl Document {
    /// An empty canvas that was never saved, under a fresh id.
    pub(super) fn blank(title: String) -> Self {
        Self::new(Design::default(), title, None, crate::project::new_id())
    }

    /// A saved project, as its file holds it.
    pub(super) fn open(project: crate::project::Project) -> Self {
        let mut design = project.design.clone();
        design.settle(None);
        let (title, id) = (project.name.clone(), project.id.clone());
        Self::new(design, title, Some(project), id)
    }

    fn new(design: Design, title: String, project: Option<crate::project::Project>, id: String) -> Self {
        Self {
            selection: Vec::new(),
            history: History::default(),
            view: canvas::View::default(),
            tool: canvas::Tool::Select,
            saved: persistence::Saved::new(&design, &title),
            autosave: autosave::DocSave::new(id, &design, &title),
            design,
            title,
            project,
            flow: None,
            nest: Default::default(),
            cut_preview: None,
            isolation: None,
            shapes_open: false,
            shapes_filter: Default::default(),
            monogram: None,
            preview_open: false,
            contour_open: false,
            mat: MachineFamily::StudioPro.preferred_mat(),
            export: None,
            replace_colour: None,
            trace: None,
            pattern_upload: None,
            transient: false,
            duplicate_memory: None,
            repeat_panel: None,
        }
    }

    /// Never saved, unchanged, with nothing to undo and no autosave on offer: an Open or New may take its
    /// place without losing anything.
    pub(super) fn is_pristine(&self) -> bool {
        !self.transient
            && self.project.is_none()
            && !self.has_unsaved_changes()
            && !self.history.can_undo()
            && self.autosave.offer.is_none()
            && self.flow.is_none()
            && self.trace.is_none()
    }
}

/// Tests read the active document's fields straight off the app, as they did before documents existed.
#[cfg(test)]
impl std::ops::Deref for super::Kerf {
    type Target = Document;

    fn deref(&self) -> &Document {
        &self.doc
    }
}

#[cfg(test)]
impl std::ops::DerefMut for super::Kerf {
    fn deref_mut(&mut self) -> &mut Document {
        &mut self.doc
    }
}

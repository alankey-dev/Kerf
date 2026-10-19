//! The Layers list flattened to the rows it shows, each with where it starts, so a frame lays out only
//! the rows in view and the drag still sees every row.

use std::collections::HashMap;
use std::ops::Range;

use egui::{Rect, Ui};

use super::drag::Slot;
use super::{GAP, Layer, ROW_H, RowKey, collapsed_id, effects_open, layers, row_height};
use crate::components::part_ids;
use crate::design::Design;
use crate::links::Link;
use crate::names::Named;
use crate::ui::effects_list;

/// What a row draws.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    /// `design.shapes[index]`, with its effect list under it while `open`.
    Shape {
        index: usize,
        open: bool,
    },
    Link {
        link: Link,
        collapsed: bool,
    },
    /// Part `part`, in the master's order, of the instance at `design.shapes[instance]`.
    Part {
        instance: usize,
        part: usize,
    },
}

/// Every row in list order, top first. The vectors run in step, one entry per row.
pub(super) struct Outline {
    pub kinds: Vec<Kind>,
    /// What each row selects, and the key the Shift-click anchor goes by.
    pub rows: Vec<(RowKey, Vec<u64>)>,
    pub slots: Vec<Slot>,
    /// Where each row starts on screen; an open effect list or a row's chips lengthen the row.
    pub tops: Vec<f32>,
    top: f32,
    bottom: f32,
}

impl Outline {
    /// The rows of `design`, the first starting at `top`.
    pub fn new(ui: &Ui, design: &Design, top: f32) -> Outline {
        let mut outline = Outline { kinds: Vec::new(), rows: Vec::new(), slots: Vec::new(), tops: Vec::new(), top, bottom: top };
        let at: HashMap<u64, usize> = design.shapes.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        outline.extend(ui, design, &at, &layers(design), 0);
        outline
    }

    fn push(&mut self, kind: Kind, key: RowKey, ids: Vec<u64>, slot: Slot, height: f32) {
        self.kinds.push(kind);
        self.rows.push((key, ids));
        self.slots.push(slot);
        self.tops.push(self.bottom);
        self.bottom += height + GAP;
    }

    fn extend(&mut self, ui: &Ui, design: &Design, at: &HashMap<u64, usize>, layers: &[Layer], depth: usize) {
        for layer in layers {
            match layer {
                &Layer::Shape(id) => self.shape(ui, design, at[&id], depth, false),
                Layer::Container { id, children } => {
                    self.shape(ui, design, at[id], depth, true);
                    self.extend(ui, design, at, children, depth + 1);
                }
                Layer::Link { link, members } => {
                    let collapsed = ui.data(|d| d.get_temp(collapsed_id(link.id)).unwrap_or(false));
                    self.push(Kind::Link { link: *link, collapsed }, RowKey::Link(link.id), members.clone(), Slot { node: link.id, depth, parent: true }, ROW_H);
                    if !collapsed {
                        for id in members {
                            self.shape(ui, design, at[id], depth + 1, false);
                        }
                    }
                }
            }
        }
    }

    /// `design.shapes[index]`'s row, its open effect list, and an instance's part rows.
    fn shape(&mut self, ui: &Ui, design: &Design, index: usize, depth: usize, parent: bool) {
        let shape = &design.shapes[index];
        let open = effects_list::any(shape) && ui.data(|d| d.get_temp(effects_open(shape.id)).unwrap_or(false));
        let list = if open { effects_list::list_height(shape, GAP) } else { 0.0 };
        self.push(Kind::Shape { index, open }, RowKey::Shape(shape.id), vec![shape.id], Slot { node: shape.id, depth, parent }, row_height(shape) + list);
        for (part, id) in part_ids(design, shape).into_iter().enumerate().rev() {
            self.push(Kind::Part { instance: index, part }, RowKey::Part(id), vec![shape.id], Slot { node: id, depth: depth + 1, parent: false }, ROW_H);
        }
    }

    /// How tall the list is, for the scroll area to make room for.
    pub fn height(&self) -> f32 {
        (self.bottom - self.top - GAP).max(0.0)
    }

    /// The rows that reach into `view`.
    pub fn visible(&self, view: Rect) -> Range<usize> {
        let first = self.tops.partition_point(|&t| t <= view.top()).saturating_sub(1);
        let end = self.tops.partition_point(|&t| t < view.bottom());
        first..end.max(first)
    }

    /// Where row `at`'s name line runs, across `x`.
    pub fn line(&self, at: usize, x: egui::Rangef) -> Rect {
        Rect::from_x_y_ranges(x, self.tops[at]..=self.tops[at] + ROW_H)
    }

    /// The first row all of whose objects are in `selection`.
    pub fn first_selected(&self, selection: &[u64]) -> Option<usize> {
        self.rows.iter().position(|(key, ids)| !matches!(key, RowKey::Part(_)) && ids.iter().all(|id| selection.contains(id)))
    }

    /// The row that lists `target`, if it is listed.
    pub fn row_of(&self, target: Named) -> Option<usize> {
        self.rows.iter().position(|(key, _)| match (*key, target) {
            (RowKey::Shape(id), Named::Shape(t)) | (RowKey::Link(id), Named::Link(t)) => id == t,
            _ => false,
        })
    }
}

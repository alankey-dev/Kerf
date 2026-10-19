//! Components (spec 31): reusable masters and the instances placed from them.
//!
//! A component's master is a list of shapes in the component's own frame, top-left at the origin, kept in
//! `Design::components`. An instance is a shape of kind `ShapeKind::Instance`; it holds no copy of the
//! master. [`expand`] turns an instance into its parts: the master's shapes moved into the instance's box,
//! overrides applied, under ids derived from the instance's. Evaluation (`effects::cache`) draws an
//! instance through those parts, so editing a master redraws every instance.
//!
//! While a master is open for editing, `Design::editing` names it and the master's shapes sit in
//! `Design::shapes`, the canvas's in the component's slot. Whatever draws a master reads it through
//! [`master_shapes`], which sees through that swap.

use std::collections::{BTreeMap, HashMap, HashSet};

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::composition::is_cut;
use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::edit::{Blocked, Edit};
use crate::effects::{Local, Piece, Style};
use crate::links::{Link, LinkKind};
use crate::patterns::PatternFill;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Component {
    pub id: u64,
    pub name: String,
    /// The master, in the component's frame.
    pub shapes: Vec<Shape>,
    /// The frame, fixed when the component was made: editing the master never changes it, so instances
    /// keep their own boxes and scale the master by their size over this one.
    pub size_in: Pt,
}

/// What one instance changes about one part, keyed by the master shape's id.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PartOverride {
    #[serde(default, with = "opt_colour", skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<crate::pens::Pen>,
    /// The part's pattern on this instance: `Some(None)` prints its flat colour where the master has one.
    #[serde(default, with = "double_option", skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Option<PatternFill>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// One change an instance makes to a part.
#[derive(Clone, Debug, PartialEq)]
pub enum PartChange {
    Fill(Color32),
    Operation(Operation),
    Pen(crate::pens::Pen),
    Pattern(Option<PatternFill>),
    #[cfg_attr(not(test), expect(dead_code, reason = "the model can hide a part per instance; no control raises it yet"))]
    Hidden(bool),
}

mod opt_colour {
    use egui::Color32;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(c: &Option<Color32>, s: S) -> Result<S::Ok, S::Error> {
        c.map(|c| c.to_array()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color32>, D::Error> {
        Ok(Option::<[u8; 4]>::deserialize(d)?.map(|[r, g, b, a]| Color32::from_rgba_premultiplied(r, g, b, a)))
    }
}

/// An override that can be absent (follow the master), null (no pattern) or a value.
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref().and_then(Option::as_ref).serialize(s)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

impl PartOverride {
    pub fn is_empty(&self) -> bool {
        *self == PartOverride::default()
    }

    fn apply(&self, part: &mut Shape) {
        if let Some(fill) = self.fill {
            part.fill = fill;
        }
        if let Some(op) = self.operation {
            part.set_operation(op);
        }
        if let Some(pen) = self.pen {
            part.set_pen(pen);
        }
        if let Some(pattern) = &self.pattern {
            part.pattern = pattern.clone();
        }
        part.hidden |= self.hidden;
    }

    fn change(&mut self, change: PartChange) {
        match change {
            PartChange::Fill(c) => self.fill = Some(c),
            PartChange::Operation(op) => self.operation = Some(op),
            PartChange::Pen(pen) => self.pen = Some(pen),
            PartChange::Pattern(fill) => self.pattern = Some(fill),
            PartChange::Hidden(h) => self.hidden = h,
        }
    }
}

/// The id master shape (or link) `master` takes as a part of `instance`. Deterministic, and with the top
/// bit set, so it never meets an id `Design::next_id` hands out. Parts are never stored.
pub fn part_id(instance: u64, master: u64) -> u64 {
    let mut z = instance.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ master.rotate_left(32);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) | 1 << 63
}

impl Shape {
    /// The component this shape is an instance of.
    pub fn instance_of(&self) -> Option<u64> {
        match self.kind {
            ShapeKind::Instance { component, .. } => Some(component),
            _ => None,
        }
    }
}

impl Design {
    pub fn component(&self, id: u64) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }
}

/// The live master of `component`: the canvas's shapes while it is being edited, else its own.
pub fn master_shapes(design: &Design, component: u64) -> Option<&[Shape]> {
    match design.editing == Some(component) {
        true => Some(&design.shapes),
        false => design.component(component).map(|c| c.shapes.as_slice()),
    }
}

/// The canvas's shapes, wherever they sit: in a component's slot while its master is open.
pub fn canvas_shapes(design: &Design) -> &[Shape] {
    match design.editing.and_then(|c| design.component(c)) {
        Some(open) => &open.shapes,
        None => &design.shapes,
    }
}

/// A stand-in instance of `component` at its master size, top-left at the origin, for drawing the
/// component on its own (a panel tile). Its id is derived, so it never meets a real shape's.
pub fn specimen(design: &Design, component: u64) -> Option<Shape> {
    let (width_in, height_in) = design.component(component)?.size_in;
    Some(Shape { width_in, height_in, ..Shape::new(part_id(0, component), ShapeKind::Instance { component, overrides: BTreeMap::new() }) })
}

/// How many instances of `component` there are, on the canvas and in every master.
pub fn instance_count(design: &Design, component: u64) -> usize {
    let lists = std::iter::once(&design.shapes).chain(design.components.iter().map(|c| &c.shapes));
    lists.flatten().filter(|s| s.instance_of() == Some(component)).count()
}

/// `instance`'s parts on the canvas, back to front; `None` when its component is missing.
pub fn expand(design: &Design, instance: &Shape) -> Option<Vec<Shape>> {
    parts_in(design, instance, instance)
}

/// The ids `expand` gives `instance`'s parts, in its order; none when its component is missing.
pub fn part_ids(design: &Design, instance: &Shape) -> Vec<u64> {
    let Some(component) = instance.instance_of().filter(|&c| design.component(c).is_some()) else { return Vec::new() };
    master_shapes(design, component).unwrap_or_default().iter().map(|m| part_id(instance.id, m.id)).collect()
}

/// `instance`'s parts with the master's frame mapped onto `frame`'s box, rotation and flips. Evaluation
/// passes the instance's box at the origin, unturned, so moving or turning an instance leaves its parts
/// as they were.
pub(crate) fn parts_in(design: &Design, instance: &Shape, frame: &Shape) -> Option<Vec<Shape>> {
    let ShapeKind::Instance { component, overrides } = &instance.kind else { return None };
    let size = design.component(*component)?.size_in;
    let master = master_shapes(design, *component)?;
    let was = Shape { width_in: size.0, height_in: size.1, ..Shape::new(0, ShapeKind::Rectangle) };
    let derive = |id: u64| part_id(instance.id, id);
    Some(
        master
            .iter()
            .map(|m| {
                let mut part = Shape { id: derive(m.id), parent: m.parent.map(derive), link: m.link.map(|l| Link { id: derive(l.id), ..l }), ..m.clone() };
                crate::containers::remap(&mut part, &was, frame);
                if let Some(o) = overrides.get(&m.id) {
                    o.apply(&mut part);
                }
                part
            })
            .collect(),
    )
}

/// Whether putting an instance of `placing` into `host`'s master (`None` is the canvas) would make a
/// component contain itself, directly or through other components.
pub fn would_cycle(design: &Design, host: Option<u64>, placing: u64) -> bool {
    let Some(host) = host else { return false };
    let mut seen = HashSet::new();
    let mut next = vec![placing];
    while let Some(c) = next.pop() {
        if c == host {
            return true;
        }
        if seen.insert(c) {
            next.extend(master_shapes(design, c).unwrap_or_default().iter().filter_map(Shape::instance_of));
        }
    }
    false
}

impl Design {
    /// Opens `component`'s master for editing: its shapes swap into `shapes`, the canvas's into its slot.
    /// Any master already open is closed first. Returns false when there is no such component.
    pub fn enter_master(&mut self, component: u64) -> bool {
        self.leave_master();
        let Some(c) = self.components.iter_mut().find(|c| c.id == component) else { return false };
        std::mem::swap(&mut self.shapes, &mut c.shapes);
        self.editing = Some(component);
        true
    }

    /// Closes the open master, if any, and drops the overrides of any master shape it lost.
    pub fn leave_master(&mut self) {
        let Some(open) = self.editing.take() else { return };
        if let Some(c) = self.components.iter_mut().find(|c| c.id == open) {
            std::mem::swap(&mut self.shapes, &mut c.shapes);
        }
        self.prune_overrides();
    }

    /// Whether this design, with its open master closed, equals `other`, without cloning to close it.
    pub fn eq_closed(&self, other: &Design) -> bool {
        if self.editing.is_none() || other.editing.is_some() {
            return self == other;
        }
        let same = |a: &Component, b: &Component| a.id == b.id && a.name == b.name && a.size_in == b.size_in && master_shapes(self, a.id) == Some(b.shapes.as_slice());
        self.nesting == other.nesting
            && canvas_shapes(self) == other.shapes.as_slice()
            && self.components.len() == other.components.len()
            && self.components.iter().zip(&other.components).all(|(a, b)| same(a, b))
    }

    /// This design with any open master closed.
    pub fn closed(mut self) -> Design {
        self.leave_master();
        self
    }

    /// Every shape list in the design: the canvas (or open master) and each component's.
    fn shape_lists_mut(&mut self) -> impl Iterator<Item = &mut Vec<Shape>> {
        std::iter::once(&mut self.shapes).chain(self.components.iter_mut().map(|c| &mut c.shapes))
    }

    /// Drops every override, on the canvas and in masters, whose master shape is gone, and every empty one.
    pub fn prune_overrides(&mut self) {
        let masters: HashMap<u64, HashSet<u64>> =
            self.components.iter().map(|c| (c.id, master_shapes(self, c.id).unwrap_or_default().iter().map(|s| s.id).collect())).collect();
        for shapes in self.shape_lists_mut() {
            for s in shapes.iter_mut() {
                if let ShapeKind::Instance { component, overrides } = &mut s.kind {
                    let live = masters.get(component);
                    overrides.retain(|id, o| !o.is_empty() && live.is_none_or(|ids| ids.contains(id)));
                }
            }
        }
    }

    /// Sets one change on `part` (a master shape id) of `instance`.
    pub fn set_override(&mut self, instance: u64, part: u64, change: PartChange) {
        let Some(ShapeKind::Instance { overrides, .. }) = self.shape_mut(instance).map(|s| &mut s.kind) else { return };
        let o = overrides.entry(part).or_default();
        o.change(change);
        if o.is_empty() {
            overrides.remove(&part);
        }
    }

    pub fn reset_overrides(&mut self, instance: u64) {
        if let Some(ShapeKind::Instance { overrides, .. }) = self.shape_mut(instance).map(|s| &mut s.kind) {
            overrides.clear();
        }
    }

    pub fn reset_part(&mut self, instance: u64, part: u64) {
        if let Some(ShapeKind::Instance { overrides, .. }) = self.shape_mut(instance).map(|s| &mut s.kind) {
            overrides.remove(&part);
        }
    }

    pub fn rename_component(&mut self, component: u64, name: &str) {
        if let Some(c) = self.components.iter_mut().find(|c| c.id == component) {
            c.name = name.trim().to_owned();
        }
    }

    /// "Component N" with the lowest N no component uses.
    fn fresh_component_name(&self) -> String {
        let taken: HashSet<&str> = self.components.iter().map(|c| c.name.as_str()).collect();
        (1..).map(|n| format!("Component {n}")).find(|n| !taken.contains(n.as_str())).unwrap_or_default()
    }

    /// A new component with a copy of `component`'s master under fresh ids, named after it. Returns its id.
    pub fn duplicate_component(&mut self, component: u64) -> Option<u64> {
        let source = self.component(component)?;
        let (name, size_in) = (format!("{} copy", source.name), source.size_in);
        let master = master_shapes(self, component)?.to_vec();
        let id = self.next_id();
        let mut fresh = (id + 1)..;
        let (shapes, ids) = renumbered(master, &mut || fresh.next().unwrap_or_default());
        self.copy_link_names(&ids);
        self.components.push(Component { id, name, shapes, size_in });
        Some(id)
    }

    /// Detaches every instance of `component`, on the canvas and in other masters, then deletes it.
    pub fn delete_component(&mut self, component: u64) {
        let open = self.editing.filter(|&e| e != component);
        self.leave_master();
        let hosts: Vec<u64> = self.components.iter().filter(|c| c.id != component && c.shapes.iter().any(|s| s.instance_of() == Some(component))).map(|c| c.id).collect();
        for host in hosts {
            self.enter_master(host);
            self.detach_all(component);
            self.leave_master();
        }
        self.detach_all(component);
        self.components.retain(|c| c.id != component);
        if let Some(open) = open {
            self.enter_master(open);
        }
    }

    fn detach_all(&mut self, component: u64) {
        let ids: Vec<u64> = self.shapes.iter().filter(|s| s.instance_of() == Some(component)).map(|s| s.id).collect();
        self.detach_instances(&ids);
    }
}

/// `shapes` under fresh ids from `next`, their parents and links following, and each old id with its new one.
fn renumbered(shapes: Vec<Shape>, next: &mut impl FnMut() -> u64) -> (Vec<Shape>, Vec<(u64, u64)>) {
    let mut ids: HashMap<u64, u64> = HashMap::new();
    let mut fresh = |old: u64| *ids.entry(old).or_insert_with(&mut *next);
    let shapes: Vec<Shape> = shapes.into_iter().map(|s| Shape { id: fresh(s.id), ..s }).collect();
    let shapes = shapes.into_iter().map(|s| Shape { parent: s.parent.map(&mut fresh), link: s.link.map(|l| Link { id: fresh(l.id), ..l }), ..s }).collect();
    (shapes, ids.into_iter().collect())
}

impl Design {
    /// Detach instance: each instance in `ids` becomes its parts as plain shapes under fresh ids, overrides
    /// baked, at its z slot and under its parent. Two or more roots join one Group; an instance with
    /// effects puts its parts in an effect group carrying them instead. Returns the new roots.
    pub fn detach_instances(&mut self, ids: &[u64]) -> Vec<u64> {
        let instances: Vec<Shape> = self.shapes.iter().filter(|s| ids.contains(&s.id) && s.instance_of().is_some()).cloned().collect();
        let mut held = Vec::new();
        for inst in instances {
            let Some(parts) = expand(self, &inst) else { continue };
            let mut fresh = self.next_id()..;
            let (mut parts, ids) = renumbered(parts, &mut || fresh.next().unwrap_or_default());
            let named: Vec<(u64, u64)> =
                self.link_names.keys().flat_map(|&link| ids.iter().filter(move |(part, _)| *part == part_id(inst.id, link)).map(move |&(_, new)| (link, new))).collect();
            self.copy_link_names(&named);
            let roots: Vec<u64> = parts.iter().filter(|p| p.parent.is_none()).map(|p| p.id).collect();
            let group = (!inst.effects.is_empty()).then(|| fresh.next().unwrap_or_default());
            for p in parts.iter_mut().filter(|p| p.parent.is_none()) {
                p.parent = group.or(inst.parent);
            }
            let Some(at) = self.shapes.iter().position(|s| s.id == inst.id) else { continue };
            if let Some(g) = group {
                let effect_group =
                    Shape { effects: inst.effects.clone(), kind: ShapeKind::Compound(crate::design::CompoundOp::Effects), ..Shape { id: g, ..inst.clone() } };
                parts.insert(0, effect_group);
            }
            self.shapes.splice(at..=at, parts);
            match group {
                Some(g) => held.push(g),
                None if roots.len() >= 2 => {
                    let named = match self.named(&roots) {
                        Some(crate::names::Named::Link(link)) => self.link_names.get(&link).cloned(),
                        _ => None,
                    };
                    if let (Some(group), Some(name)) = (self.group(&roots), named) {
                        self.link_names.insert(group, name);
                    }
                    held.extend(roots);
                }
                None => {
                    if let Some(root) = roots.first().and_then(|&r| self.shape_mut(r)) {
                        root.link = inst.link;
                    }
                    held.extend(roots);
                }
            }
        }
        self.prune_links();
        self.settle(None);
        held
    }

    /// Make component: the roots of `ids` with their subtrees move into a new master, framed by their
    /// world bounds, and one instance of it takes the back-most root's z slot and parent. Links the moved
    /// shapes shared with shapes left behind lose them. Returns the instance's id.
    pub fn make_component(&mut self, ids: &[u64]) -> Option<u64> {
        let roots = self.roots(ids);
        let first = *roots.first()?;
        let tree = self.with_descendants(&roots);
        let compounds: HashSet<u64> = self.shapes.iter().filter(|s| tree.contains(&s.id) && s.compound().is_some()).map(|s| s.id).collect();
        let inside_compound = |s: &Shape| {
            let mut at = s.parent;
            while let Some(p) = at.filter(|p| tree.contains(p)) {
                if compounds.contains(&p) {
                    return true;
                }
                at = self.parent_of(p);
            }
            false
        };
        let frame = self
            .shapes
            .iter()
            .filter(|s| tree.contains(&s.id) && !inside_compound(s))
            .map(|s| self.geometry(s).bounds())
            .fold(crate::snap::Aabb::EMPTY, crate::snap::Aabb::union);
        if frame.is_empty() {
            return None;
        }
        let at = self.shapes.iter().position(|s| s.id == first)?;
        let parent = self.parent_of(first);
        let (component, instance) = (self.next_id(), self.next_id() + 1);
        let leaving: HashSet<u64> = self.shapes.iter().filter(|s| !tree.contains(&s.id)).filter_map(|s| s.link).map(|l| l.id).collect();
        let mut master: Vec<Shape> = Vec::new();
        self.shapes.retain(|s| {
            if !tree.contains(&s.id) {
                return true;
            }
            let link = s.link.filter(|l| !leaving.contains(&l.id));
            let parent = s.parent.filter(|_| !roots.contains(&s.id));
            master.push(Shape { x_in: s.x_in - frame.min_x, y_in: s.y_in - frame.min_y, link, parent, ..s.clone() });
            false
        });
        let size_in = (frame.width(), frame.height());
        let name = self.fresh_component_name();
        self.components.push(Component { id: component, name, shapes: master, size_in });
        let placed = Shape {
            x_in: frame.min_x,
            y_in: frame.min_y,
            width_in: size_in.0,
            height_in: size_in.1,
            parent,
            ..Shape::new(instance, ShapeKind::Instance { component, overrides: BTreeMap::new() })
        };
        self.shapes.insert(at.min(self.shapes.len()), placed);
        self.prune_links();
        self.settle(None);
        Some(instance)
    }

    /// Places an instance of `component` at its master's size, centred on `at` or in the next free slot.
    pub fn place_instance(&mut self, component: u64, at: Option<Pt>) -> Option<u64> {
        let (w, h) = self.component(component)?.size_in;
        let (x_in, y_in) = match at {
            Some((cx, cy)) => (cx - w / 2.0, cy - h / 2.0),
            None => self.next_free_slot(w.max(h)),
        };
        let id = self.next_id();
        self.shapes.push(Shape { x_in, y_in, width_in: w, height_in: h, ..Shape::new(id, ShapeKind::Instance { component, overrides: BTreeMap::new() }) });
        Some(id)
    }
}

impl Design {
    /// The enable rules of the component edits that need no subject; see `Design::check`.
    pub(crate) fn check_component(&self, edit: &Edit) -> Result<(), Blocked> {
        let exists = |c: &u64| if self.component(*c).is_some() { Ok(()) } else { Err(Blocked::NoComponent) };
        match edit {
            Edit::PlaceInstance { component, .. } => {
                exists(component)?;
                if would_cycle(self, self.editing, *component) { Err(Blocked::ComponentCycle) } else { Ok(()) }
            }
            Edit::RenameComponent { component, name } => {
                exists(component)?;
                if name.trim().is_empty() { Err(Blocked::EmptyName) } else { Ok(()) }
            }
            Edit::DuplicateComponent(c) | Edit::DeleteComponent(c) => exists(c),
            _ => Ok(()),
        }
    }

    /// The enable rules of the component edits on a subject; see `Design::check`.
    pub(crate) fn check_instances(&self, edit: &Edit, on: &[u64]) -> Result<(), Blocked> {
        let instances: Vec<&Shape> = self.shapes.iter().filter(|s| on.contains(&s.id) && s.instance_of().is_some()).collect();
        let overrides = |s: &&Shape| match &s.kind {
            ShapeKind::Instance { overrides, .. } => overrides.clone(),
            _ => BTreeMap::new(),
        };
        match edit {
            Edit::MakeComponent => Ok(()),
            _ if instances.is_empty() => Err(Blocked::NoInstance),
            Edit::Override { part, .. } => {
                let [one] = instances[..] else { return Err(Blocked::NoInstance) };
                let master = one.instance_of().and_then(|c| master_shapes(self, c)).ok_or(Blocked::NoComponent)?;
                if master.iter().any(|m| m.id == *part) { Ok(()) } else { Err(Blocked::NoPart) }
            }
            Edit::ResetOverrides if instances.iter().all(|s| overrides(s).is_empty()) => Err(Blocked::NoOverrides),
            Edit::ResetPart(part) if !instances.iter().any(|s| overrides(s).contains_key(part)) => Err(Blocked::NoOverrides),
            _ => Ok(()),
        }
    }

    /// Runs a component edit whose check passed. Returns the ids the user holds afterwards.
    pub(crate) fn apply_component(&mut self, edit: &Edit, on: &[u64]) -> Vec<u64> {
        let instances: Vec<u64> = self.shapes.iter().filter(|s| on.contains(&s.id) && s.instance_of().is_some()).map(|s| s.id).collect();
        match edit {
            Edit::MakeComponent => return self.make_component(on).map_or_else(|| on.to_vec(), |id| vec![id]),
            Edit::DetachInstance => return self.detach_instances(&instances),
            Edit::PlaceInstance { component, at } => return self.place_instance(*component, *at).into_iter().collect(),
            Edit::RenameComponent { component, name } => self.rename_component(*component, name),
            Edit::DuplicateComponent(c) => _ = self.duplicate_component(*c),
            Edit::DeleteComponent(c) => self.delete_component(*c),
            Edit::Override { part, change } => instances.iter().for_each(|&i| self.set_override(i, *part, change.clone())),
            Edit::ResetOverrides => instances.iter().for_each(|&i| self.reset_overrides(i)),
            Edit::ResetPart(part) => instances.iter().for_each(|&i| self.reset_part(i, *part)),
            _ => {}
        }
        on.iter().copied().filter(|id| self.shapes.iter().any(|s| s.id == *id)).collect()
    }
}

impl Design {
    /// Load-time repair of the masters: each is sanitised as the canvas is, a frame that is not a usable
    /// size becomes one, and an instance that would make its component contain itself is dropped. An
    /// instance of a missing component stays; it draws as a box. Overrides of missing parts go.
    pub(crate) fn sanitise_components(&mut self, repairs: &mut crate::sanitise::Repairs) {
        let usable = |v: f32| if v.is_finite() { v.clamp(crate::sanitise::MIN_SIZE_IN, crate::transform::MAX_SIZE_IN) } else { 1.0 };
        for i in 0..self.components.len() {
            let c = &mut self.components[i];
            c.size_in = (usable(c.size_in.0), usable(c.size_in.1));
            crate::sanitise::sanitise_shapes(&mut c.shapes, repairs);
            let mut master = Design { shapes: std::mem::take(&mut c.shapes), ..Design::default() };
            master.break_parent_cycles();
            self.components[i].shapes = master.shapes;
            let host = Some(self.components[i].id);
            let cyclic: Vec<u64> = self.components[i].shapes.iter().filter(|s| s.instance_of().is_some_and(|k| would_cycle(self, host, k))).map(|s| s.id).collect();
            self.components[i].shapes.retain(|s| !cyclic.contains(&s.id));
            repairs.dropped.extend(cyclic);
        }
        self.prune_overrides();
    }
}

/// Which rigid unit of an instance one of its pieces lands in on the mats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartUnit {
    /// The link of an attached part, an id derived for this instance.
    Attached(u64),
    /// The instance's pieces bound for one mat: a print page, or one material colour.
    Mat { instance: u64, print: bool, colour: Color32 },
}

/// The unit each of `pieces` (some of `instance`'s pieces, back to front) lands in, so an instance stays
/// rigid on each mat it reaches: an attached part's pieces with its link, other cut pieces with the mat
/// their operation and colour pick, and every other piece (a Pen line, a Score) with the back-most cut
/// piece.
pub fn part_units(design: &Design, instance: &Shape, pieces: &[&Piece]) -> Vec<PartUnit> {
    let parts = expand(design, instance).unwrap_or_default();
    let attached = |p: &Piece| match p.id.local {
        Local::Part { part, .. } => parts.iter().find(|s| s.id == part)?.link.filter(|l| l.kind == LinkKind::Attach).map(|l| PartUnit::Attached(l.id)),
        _ => None,
    };
    let mat = |style: Style| {
        let print = style.operation == Operation::PrintThenCut;
        PartUnit::Mat { instance: instance.id, print, colour: if print { Color32::WHITE } else { style.fill } }
    };
    let unit = |p: &Piece| attached(p).unwrap_or_else(|| mat(p.style));
    let anchor = pieces.iter().find(|p| is_cut(p.style.operation)).map_or_else(|| mat(Style::of(instance)), |p| unit(p));
    pieces.iter().map(|p| if is_cut(p.style.operation) { unit(p) } else { attached(p).unwrap_or(anchor) }).collect()
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;

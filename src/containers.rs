//! Containers: any shape can take the container role and hold child objects that Align, Distribute and
//! auto layout place against its padded box (spec 18).
//!
//! Invariants, kept only by the methods here and restored by `Design::settle`:
//! 1. `parent`, when set, names an existing shape that holds children (a container or a Compound), and
//!    the chain is acyclic. A Compound is never a container, and one with no children dissolves.
//! 2. A container is never a link member. A link may sit inside a container as one child unit.
//! 3. Every member of a link has the same `parent`.
//! 4. A container is directly followed in `shapes` by all its descendants, depth first, so children draw
//!    above it and sibling z-order is layout order.
//! 5. Every container with a layout has its children where `layout` says, to within `EPS_IN`, and every
//!    Compound's box hugs its visible children's boxes unless its own pose was what changed.
//! 6. `settle` on a settled design changes nothing, bit for bit, so it never makes a phantom undo step.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use crate::design::{Align, Arrange, Design, Operation, Pt, Shape, ShapeKind, Unit, UnitKey, units_by};
use crate::links::LinkKind;
use crate::snap::Aabb;

pub const DEFAULT_PADDING_IN: f32 = 0.25;
/// Layout moves smaller than this are float noise and are skipped, which keeps `settle` idempotent.
const EPS_IN: f32 = 1e-4;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ContainerRole {
    pub padding_in: f32,
    /// `None` is the spec's "Layout: None": children stay where the user puts them.
    #[serde(default)]
    pub layout: Option<Flow>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Flow {
    pub axis: Axis,
    pub gap_in: f32,
    pub cross: Cross,
    pub sizing: Sizing,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    Row,
    Column,
    Grid {
        columns: NonZeroU16,
    },
    /// A ring of children round the container's centre: `radius_in` out, from `start_deg`, across
    /// `sweep_deg`, each turned to face outward when `rotate_items` (spec 52). 0 degrees is straight up,
    /// angles increasing clockwise, matching the rotate handle.
    Radial {
        radius_in: f32,
        start_deg: f32,
        sweep_deg: f32,
        rotate_items: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum Cross {
    #[default]
    Start,
    Centre,
    End,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum Sizing {
    /// The container's box becomes children + gaps + padding; its top-left stays put.
    #[default]
    Hug,
    /// The box keeps its size; a Row or Column spaces its children between the inner edges.
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlignTo {
    Selection,
    #[default]
    Container,
}

impl AlignTo {
    /// Selection: `op.min_units()`. Container: 1 to align, 2 to distribute.
    pub fn min_units(self, op: Align) -> usize {
        match (self, op) {
            (AlignTo::Selection, _) => op.min_units(),
            (AlignTo::Container, Align::DistributeH | Align::DistributeV) => 2,
            (AlignTo::Container, _) => 1,
        }
    }
}

/// The pure layout's answer in world inches: the new `(x_in, y_in)` of each child shape, and the
/// container's new `(width_in, height_in)` when it hugs and the size changes.
#[derive(Debug, PartialEq)]
pub struct Placed {
    pub size: Option<(f32, f32)>,
    pub moves: Vec<(u64, f32, f32)>,
    /// A child's new rotation, set only by the Radial layout with Rotate items on.
    pub rotations: Vec<(u64, f32)>,
}

/// `shapes` as units by any link, dropping shapes with no outline, which have no bounds to place.
fn placeable<'a>(design: &Design, shapes: impl IntoIterator<Item = &'a Shape>) -> Vec<Unit> {
    units_by(design, shapes.into_iter().filter(|s| !design.geometry(s).bounds().is_empty()), UnitKey::AnyLink)
}

/// The container's world bounds inset by its padding.
pub fn inner_box(design: &Design, container: &Shape) -> Aabb {
    let pad = container.container.map_or(0.0, |r| r.padding_in);
    let b = design.geometry(container).bounds();
    Aabb { min_x: b.min_x + pad, min_y: b.min_y + pad, max_x: b.max_x - pad, max_y: b.max_y - pad }
}

/// Start positions for `cells` laid end to end from `from` with `gap` between.
fn run(cells: &[f32], from: f32, gap: f32) -> Vec<f32> {
    cells
        .iter()
        .scan(from, |at, len| {
            let start = *at;
            *at += len + gap;
            Some(start)
        })
        .collect()
}

/// The gap that spreads `cells` across `len` with the first and last touching its ends.
fn between(cells: &[f32], len: f32) -> f32 {
    match cells.len() {
        0 | 1 => 0.0,
        n => (len - cells.iter().sum::<f32>()) / (n - 1) as f32,
    }
}

/// Pure. `children` are the container's direct children in z-order; link-mates form one unit. A Row is
/// a one-row grid and a Column a one-column grid; each unit sits in its cell per `Cross`. Works in world
/// axes on the container's padded world bounds, so Hug is exact only for an unrotated container.
pub fn layout(design: &Design, container: &Shape, children: &[&Shape]) -> Placed {
    let none = Placed { size: None, moves: Vec::new(), rotations: Vec::new() };
    let Some(ContainerRole { padding_in, layout: Some(flow) }) = container.container else { return none };
    let units = placeable(design, children.iter().copied());
    if units.is_empty() {
        return none;
    }
    if let Axis::Radial { .. } = flow.axis {
        return radial(design, container, children, &units, flow.axis);
    }
    let n = units.len();
    let cols = match flow.axis {
        Axis::Row => n,
        Axis::Column => 1,
        Axis::Grid { columns } => usize::from(columns.get()).min(n),
        Axis::Radial { .. } => unreachable!("handled above"),
    };
    let size = |u: &Unit| (u.bounds.width(), u.bounds.height());
    let mut col_w = vec![0.0f32; cols];
    let mut row_h = vec![0.0f32; n.div_ceil(cols)];
    for (i, u) in units.iter().enumerate() {
        let (w, h) = size(u);
        col_w[i % cols] = col_w[i % cols].max(w);
        row_h[i / cols] = row_h[i / cols].max(h);
    }
    let Aabb { min_x: x0, min_y: y0, max_x: x1, max_y: y1 } = inner_box(design, container);
    let (mut gap_x, mut gap_y) = (flow.gap_in, flow.gap_in);
    if flow.sizing == Sizing::Fixed {
        match flow.axis {
            Axis::Row => (gap_x, row_h[0]) = (between(&col_w, x1 - x0), y1 - y0),
            Axis::Column => (gap_y, col_w[0]) = (between(&row_h, y1 - y0), x1 - x0),
            Axis::Grid { .. } | Axis::Radial { .. } => {}
        }
    }
    let (xs, ys) = (run(&col_w, x0, gap_x), run(&row_h, y0, gap_y));
    let offset = |cell: f32, len: f32| match flow.cross {
        Cross::Start => 0.0,
        Cross::Centre => (cell - len) / 2.0,
        Cross::End => cell - len,
    };
    let mut moves = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let (c, r) = (i % cols, i / cols);
        let (w, h) = size(u);
        let (dx, dy) = (xs[c] + offset(col_w[c], w) - u.bounds.min_x, ys[r] + offset(row_h[r], h) - u.bounds.min_y);
        moves.extend(children.iter().filter(|s| u.ids.contains(&s.id)).map(|s| (s.id, s.x_in + dx, s.y_in + dy)));
    }
    let span = |cells: &[f32], gap: f32| cells.iter().sum::<f32>() + gap * (cells.len() - 1) as f32 + 2.0 * padding_in;
    let hug = (span(&col_w, gap_x), span(&row_h, gap_y));
    let changed = (hug.0 - container.width_in).abs() > EPS_IN || (hug.1 - container.height_in).abs() > EPS_IN;
    Placed { size: (flow.sizing == Sizing::Hug && changed).then_some(hug), moves, rotations: Vec::new() }
}

/// Lays `units` round the container's centre on a circle of `radius_in`, from `start_deg` across
/// `sweep_deg`. A full turn divides the sweep by the count so the first and last do not land together;
/// a partial arc puts one at each end. With `rotate_items`, each child turns to face outward.
fn radial(design: &Design, container: &Shape, children: &[&Shape], units: &[Unit], axis: Axis) -> Placed {
    let Axis::Radial { radius_in, start_deg, sweep_deg, rotate_items } = axis else { unreachable!("radial only") };
    let n = units.len();
    let full = (sweep_deg.abs() - 360.0).abs() < EPS_IN;
    let step = if full {
        sweep_deg / n as f32
    } else if n > 1 {
        sweep_deg / (n - 1) as f32
    } else {
        0.0
    };
    let b = inner_box(design, container);
    let (cx, cy) = b.centre();
    let (mut moves, mut rotations) = (Vec::new(), Vec::new());
    for (i, u) in units.iter().enumerate() {
        let angle = (start_deg + i as f32 * step).to_radians();
        let (tx, ty) = (cx + radius_in * angle.sin(), cy - radius_in * angle.cos());
        let (uc_x, uc_y) = (u.bounds.centre().0, u.bounds.centre().1);
        let (dx, dy) = (tx - uc_x, ty - uc_y);
        for s in children.iter().filter(|s| u.ids.contains(&s.id)) {
            moves.push((s.id, s.x_in + dx, s.y_in + dy));
            if rotate_items {
                rotations.push((s.id, (start_deg + i as f32 * step).rem_euclid(360.0)));
            }
        }
    }
    Placed { size: None, moves, rotations }
}

/// Make support for `composition.rs`: the id keying this shape's placement unit. The outermost visible
/// non-Guide container ancestor-or-self that has children; else its Attach link id; else `None`.
pub fn placement_key(design: &Design, s: &Shape) -> Option<u64> {
    let places = |c: &Shape| c.container.is_some() && !c.hidden && c.operation != Operation::Guide && design.shapes.iter().any(|k| k.parent == Some(c.id));
    let mut key = None;
    let mut at = Some(s);
    while let Some(c) = at {
        if places(c) {
            key = Some(c.id);
        }
        at = c.parent.and_then(|p| find(design, p));
    }
    key.or_else(|| s.link.filter(|l| l.kind == LinkKind::Attach).map(|l| l.id))
}

fn find(design: &Design, id: u64) -> Option<&Shape> {
    design.shapes.iter().find(|s| s.id == id)
}

/// Siblings in z-order as indices into `shapes`: the one tree this module reasons over.
struct Forest {
    roots: Vec<usize>,
    kids: Vec<Vec<usize>>,
}

impl Forest {
    fn of(shapes: &[Shape]) -> Forest {
        let index: HashMap<u64, usize> = shapes.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        let mut forest = Forest { roots: Vec::new(), kids: vec![Vec::new(); shapes.len()] };
        for (i, s) in shapes.iter().enumerate() {
            match s.parent.and_then(|p| index.get(&p)) {
                Some(&p) => forest.kids[p].push(i),
                None => forest.roots.push(i),
            }
        }
        forest
    }

    /// Depth first, each container followed by its subtree (invariant 4). Shapes stranded on a parent
    /// cycle are appended rather than lost; `repair` breaks such cycles before this matters.
    fn emit(&self) -> Vec<usize> {
        fn visit(f: &Forest, i: usize, seen: &mut [bool], out: &mut Vec<usize>) {
            if std::mem::replace(&mut seen[i], true) {
                return;
            }
            out.push(i);
            for &k in &f.kids[i] {
                visit(f, k, seen, out);
            }
        }
        let mut seen = vec![false; self.kids.len()];
        let mut out = Vec::with_capacity(self.kids.len());
        for r in self.roots.iter().copied().chain(0..self.kids.len()) {
            visit(self, r, &mut seen, &mut out);
        }
        out
    }
}

impl Design {
    /// One shape: give it the role, pulling it out of any link. Two or more: wrap their units in a new
    /// Guide rectangle fitted to their bounds plus `DEFAULT_PADDING_IN`, just below the back-most of
    /// them and inside their common parent if they share one. Returns the container's id.
    pub fn contain(&mut self, ids: &[u64]) -> Option<u64> {
        if let [id] = self.roots(ids)[..] {
            let s = self.shape_mut(id)?;
            if s.container.is_none() {
                s.container = Some(ContainerRole { padding_in: DEFAULT_PADDING_IN, layout: None });
                s.link = None;
                self.prune_links();
            }
            self.settle(None);
            return Some(id);
        }
        let roots = self.roots(&self.expand(ids));
        let first = *roots.first()?;
        let parent = self.parent_of(first).filter(|_| roots.iter().all(|&r| self.parent_of(r) == self.parent_of(first)));
        let covered = self.with_descendants(&roots);
        let Aabb { min_x: x0, min_y: y0, max_x: x1, max_y: y1 } =
            self.shapes.iter().filter(|s| covered.contains(&s.id)).map(|s| self.geometry(s).bounds()).reduce(Aabb::union)?;
        let pad = DEFAULT_PADDING_IN;
        let id = self.next_id();
        let kind = ShapeKind::Rectangle;
        let fill = kind.default_fill();
        let frame = Shape {
            x_in: x0 - pad,
            y_in: y0 - pad,
            width_in: x1 - x0 + 2.0 * pad,
            height_in: y1 - y0 + 2.0 * pad,
            fill,
            operation: Operation::Guide,
            pen: crate::pens::Pen::nearest(fill, Default::default()),
            container: Some(ContainerRole { padding_in: pad, layout: None }),
            parent,
            ..Shape::new(id, kind)
        };
        let at = self.shapes.iter().position(|s| s.id == first)?;
        self.shapes.insert(at, frame);
        for s in self.shapes.iter_mut().filter(|s| roots.contains(&s.id)) {
            s.parent = Some(id);
        }
        self.settle(None);
        Some(id)
    }

    /// Whether `contain` would wrap (2+ shapes) rather than make (1 shape). Drives the menu label.
    pub fn contain_wraps(&self, ids: &[u64]) -> bool {
        self.roots(ids).len() >= 2
    }

    pub fn can_contain(&self, ids: &[u64]) -> bool {
        match self.roots(ids)[..] {
            [] => false,
            [id] => find(self, id).is_some_and(|s| !s.holds_children()),
            _ => true,
        }
    }

    /// Clears the role on every container in `ids`. Every shape stays put; children move up to the
    /// removed container's own parent.
    pub fn uncontain(&mut self, ids: &[u64]) {
        let removed: Vec<(u64, Option<u64>)> = self.shapes.iter().filter(|s| ids.contains(&s.id) && s.container.is_some()).map(|s| (s.id, s.parent)).collect();
        for &(id, up) in &removed {
            for s in &mut self.shapes {
                if s.id == id {
                    s.container = None;
                } else if s.parent == Some(id) {
                    s.parent = up;
                }
            }
        }
        self.settle(None);
    }

    pub fn has_container(&self, ids: &[u64]) -> bool {
        self.shapes.iter().any(|s| ids.contains(&s.id) && s.container.is_some())
    }

    /// Moves the units of `ids` (whole links, roots only) into `parent`, or to the top level, front-most
    /// in that scope. Units already there stay put; a move into a shape's own subtree is refused.
    pub fn reparent(&mut self, ids: &[u64], parent: Option<u64>) {
        if parent.is_some_and(|p| !find(self, p).is_some_and(Shape::holds_children)) {
            return;
        }
        let moving: Vec<u64> = self.roots(&self.expand(ids)).into_iter().filter(|&id| self.parent_of(id) != parent).collect();
        if moving.is_empty() || parent.is_some_and(|p| self.with_descendants(&moving).contains(&p)) {
            return;
        }
        for s in self.shapes.iter_mut().filter(|s| moving.contains(&s.id)) {
            s.parent = parent;
        }
        let order: Vec<u64> = self.children(parent).map(|s| s.id).filter(|id| !moving.contains(id)).chain(moving.iter().copied()).collect();
        self.order_siblings(parent, &order);
        self.settle(None);
    }

    /// The front-most visible container whose padded bounds hold `at`, excluding `moving` and their subtrees.
    pub fn drop_target(&self, moving: &[u64], at: Pt) -> Option<u64> {
        let skip = self.with_descendants(&self.expand(moving));
        let holds = |b: Aabb| (b.min_x..=b.max_x).contains(&at.0) && (b.min_y..=b.max_y).contains(&at.1);
        self.shapes.iter().rev().find(|s| s.container.is_some() && !s.hidden && !skip.contains(&s.id) && holds(inner_box(self, s))).map(|s| s.id)
    }

    /// Drops `moving` into the container under `at`, or out to the top level. What sits in a Compound
    /// stays there: only Layers moves shapes in or out of one.
    pub fn drop_at(&mut self, moving: &[u64], at: Pt) {
        let in_compound = |id: &u64| self.parent_of(*id).and_then(|p| find(self, p)).is_some_and(|p| p.compound().is_some());
        let free: Vec<u64> = self.roots(&self.expand(moving)).into_iter().filter(|id| !in_compound(id)).collect();
        if !free.is_empty() {
            self.reparent(&free, self.drop_target(moving, at));
        }
    }

    pub fn parent_of(&self, id: u64) -> Option<u64> {
        find(self, id).and_then(|s| s.parent)
    }

    /// Direct children of `parent` (`None` is the top level) in z-order.
    pub fn children(&self, parent: Option<u64>) -> impl Iterator<Item = &Shape> {
        self.shapes.iter().filter(move |s| s.parent == parent)
    }

    /// `ids` plus every descendant, in z-order.
    pub fn with_descendants(&self, ids: &[u64]) -> Vec<u64> {
        let mut set: HashSet<u64> = ids.iter().copied().collect();
        loop {
            let found: Vec<u64> = self.shapes.iter().filter(|s| !set.contains(&s.id) && s.parent.is_some_and(|p| set.contains(&p))).map(|s| s.id).collect();
            if found.is_empty() {
                break;
            }
            set.extend(found);
        }
        self.shapes.iter().filter(|s| set.contains(&s.id)).map(|s| s.id).collect()
    }

    /// The shapes of `ids` with no ancestor also in `ids`, in z-order.
    pub fn roots(&self, ids: &[u64]) -> Vec<u64> {
        let has_picked_ancestor = |s: &Shape| {
            let mut at = s.parent;
            for _ in 0..self.shapes.len() {
                match at {
                    Some(p) if ids.contains(&p) => return true,
                    Some(p) => at = self.parent_of(p),
                    None => return false,
                }
            }
            false
        };
        self.shapes.iter().filter(|s| ids.contains(&s.id) && !has_picked_ancestor(s)).map(|s| s.id).collect()
    }

    /// The container Align to Container uses for `ids`, or `None` when the toggle does not apply:
    /// 1. every unit of `ids` shares one parent with no layout: that parent;
    /// 2. else `ids` is one top-level container with children and no layout: it, acting on its children.
    pub fn align_container(&self, ids: &[u64]) -> Option<u64> {
        let free = |id: u64| find(self, id).is_some_and(|s| s.container.is_some_and(|r| r.layout.is_none()));
        let roots = self.roots(&self.expand(ids));
        let parent = self.parent_of(*roots.first()?);
        if let Some(p) = parent
            && free(p)
            && roots.iter().all(|&r| self.parent_of(r) == parent)
        {
            return Some(p);
        }
        match roots[..] {
            [c] if parent.is_none() && free(c) && self.children(Some(c)).next().is_some() => Some(c),
            _ => None,
        }
    }

    /// What Align uses as its reference for `ids`: the `preferred` choice while they sit in one container
    /// Align to Container can use, else the selection itself.
    pub fn align_target(&self, ids: &[u64], preferred: AlignTo) -> AlignTo {
        match self.align_container(ids) {
            Some(_) => preferred,
            None => AlignTo::Selection,
        }
    }

    /// How many units Align or Distribute to `to` would move for `ids`.
    pub fn align_units_to(&self, ids: &[u64], to: AlignTo) -> usize {
        match self.container_units(ids, to) {
            Some((_, units)) => units.len(),
            None => self.units_by(&self.expand(ids), UnitKey::AnyLink).len(),
        }
    }

    /// The single Align entry point. `Selection`, or `Container` with no target, is `Design::align`.
    /// `Container` aligns each unit to the container's padded box; distribute orders units by centre and
    /// packs them with equal gaps, the first and last touching the padded edges.
    pub fn align_to(&mut self, ids: &[u64], op: Align, to: AlignTo) {
        let Some((c, units)) = self.container_units(ids, to) else {
            self.align(ids, op);
            self.settle(None);
            return;
        };
        let Some(Aabb { min_x: x0, min_y: y0, max_x: x1, max_y: y1 }) = find(self, c).map(|c| inner_box(self, c)) else { return };
        if units.len() < to.min_units(op) {
            return;
        }
        let shifts: Vec<(&Unit, f32, f32)> = match op {
            Align::DistributeH | Align::DistributeV => {
                let horizontal = op == Align::DistributeH;
                let span = |b: &Aabb| if horizontal { (b.min_x, b.max_x) } else { (b.min_y, b.max_y) };
                let mut order: Vec<&Unit> = units.iter().collect();
                order.sort_by(|a, b| {
                    let (a, b) = (span(&a.bounds), span(&b.bounds));
                    (a.0 + a.1).total_cmp(&(b.0 + b.1))
                });
                let lens: Vec<f32> = order.iter().map(|u| span(&u.bounds).1 - span(&u.bounds).0).collect();
                let (from, to) = if horizontal { (x0, x1) } else { (y0, y1) };
                let starts = run(&lens, from, between(&lens, to - from));
                order
                    .into_iter()
                    .zip(starts)
                    .map(|(u, at)| {
                        let d = at - span(&u.bounds).0;
                        if horizontal { (u, d, 0.0) } else { (u, 0.0, d) }
                    })
                    .collect()
            }
            _ => units
                .iter()
                .map(|u| {
                    let b = u.bounds;
                    let (mid_x, mid_y) = ((x0 + x1 - b.min_x - b.max_x) / 2.0, (y0 + y1 - b.min_y - b.max_y) / 2.0);
                    let (dx, dy) = match op {
                        Align::Left => (x0 - b.min_x, 0.0),
                        Align::HCentre => (mid_x, 0.0),
                        Align::Right => (x1 - b.max_x, 0.0),
                        Align::Top => (0.0, y0 - b.min_y),
                        Align::VCentre => (0.0, mid_y),
                        Align::Bottom => (0.0, y1 - b.max_y),
                        Align::Centre => (mid_x, mid_y),
                        Align::DistributeH | Align::DistributeV => unreachable!("handled above"),
                    };
                    (u, dx, dy)
                })
                .collect(),
        };
        let shifts: Vec<(Vec<u64>, f32, f32)> = shifts.into_iter().map(|(u, dx, dy)| (u.ids.clone(), dx, dy)).collect();
        for (ids, dx, dy) in shifts {
            if self.position_locked(&ids) {
                continue;
            }
            self.shift_tree(&ids, dx, dy);
        }
        self.settle(None);
    }

    /// The Align to Container target and the units it moves, or `None` when Align uses the selection.
    fn container_units(&self, ids: &[u64], to: AlignTo) -> Option<(u64, Vec<Unit>)> {
        if to == AlignTo::Selection {
            return None;
        }
        let c = self.align_container(ids)?;
        let roots = self.roots(&self.expand(ids));
        let units = if roots == [c] { placeable(self, self.children(Some(c))) } else { placeable(self, self.shapes.iter().filter(|s| roots.contains(&s.id))) };
        Some((c, units))
    }

    /// Whether every root of `ids` is a child of a container with a layout, so dragging reorders.
    pub fn in_layout(&self, ids: &[u64]) -> bool {
        let laid_out = |id: u64| self.parent_of(id).and_then(|p| find(self, p)).is_some_and(|p| p.container.is_some_and(|r| r.layout.is_some()));
        let roots = self.roots(ids);
        !roots.is_empty() && roots.into_iter().all(laid_out)
    }

    /// Moves the dragged unit to the sibling slot nearest `at` along the layout's flow. Z-order is the
    /// layout order, so the layout pass then places it there.
    pub fn slide_in_layout(&mut self, ids: &[u64], at: Pt) {
        if !self.in_layout(ids) {
            return;
        }
        let moving = self.roots(&self.expand(ids));
        let parent = self.parent_of(moving[0]);
        let Some(flow) = parent.and_then(|p| find(self, p)).and_then(|p| p.container).and_then(|r| r.layout) else { return };
        let others = placeable(self, self.children(parent).filter(|s| !moving.contains(&s.id)));
        let centre = |u: &Unit| u.bounds.centre();
        let slot = match flow.axis {
            Axis::Row => others.iter().filter(|u| centre(u).0 < at.0).count(),
            Axis::Column => others.iter().filter(|u| centre(u).1 < at.1).count(),
            Axis::Grid { .. } | Axis::Radial { .. } => {
                let dist = |u: &Unit| (centre(u).0 - at.0).hypot(centre(u).1 - at.1);
                others.iter().enumerate().min_by(|a, b| dist(a.1).total_cmp(&dist(b.1))).map_or(0, |(i, _)| i)
            }
        };
        let (before, after) = others.split_at(slot);
        let order: Vec<u64> = before
            .iter()
            .flat_map(|u| u.ids.iter().copied())
            .chain(self.children(parent).map(|s| s.id).filter(|id| moving.contains(id)))
            .chain(after.iter().flat_map(|u| u.ids.iter().copied()))
            .collect();
        self.order_siblings(parent, &order);
        self.settle(None);
    }

    /// Restores every invariant in the module docs. With `before`, it first carries the descendants of
    /// any container that moved or turned (not resized or flipped) since `before`, skipping descendants
    /// the edit already changed. Idempotent, and a no-op when `before` equals `self`.
    pub fn settle(&mut self, before: Option<&Design>) {
        if before.is_some_and(|b| b == self) {
            return;
        }
        self.repair();
        self.normalise_z();
        if let Some(before) = before {
            self.carry(before);
        }
        self.relayout(before);
    }

    /// Spec 02 arrange, among siblings: each scope (the top level or one container) reorders its own
    /// blocks, a block being a link run or a lone shape, each carrying its subtree.
    pub fn arrange(&mut self, ids: &[u64], op: Arrange) {
        let ids = self.expand(ids);
        let mut scopes: Vec<Option<u64>> = Vec::new();
        for s in self.shapes.iter().filter(|s| ids.contains(&s.id)) {
            if !scopes.contains(&s.parent) {
                scopes.push(s.parent);
            }
        }
        for scope in scopes {
            let mut blocks: Vec<(bool, Vec<u64>, Option<u64>)> = Vec::new();
            for s in self.children(scope) {
                let link = s.link.map(|l| l.id);
                match blocks.last_mut() {
                    Some((picked, members, l)) if link.is_some() && *l == link => {
                        members.push(s.id);
                        *picked |= ids.contains(&s.id);
                    }
                    _ => blocks.push((ids.contains(&s.id), vec![s.id], link)),
                }
            }
            let n = blocks.len();
            match op {
                Arrange::ToFront => blocks.sort_by_key(|b| b.0),
                Arrange::ToBack => blocks.sort_by_key(|b| !b.0),
                // Each selected block steps one slot, the one nearest the destination first.
                Arrange::Forward => {
                    for i in (0..n.saturating_sub(1)).rev() {
                        if blocks[i].0 && !blocks[i + 1].0 {
                            blocks.swap(i, i + 1);
                        }
                    }
                }
                Arrange::Backward => {
                    for i in 1..n {
                        if blocks[i].0 && !blocks[i - 1].0 {
                            blocks.swap(i, i - 1);
                        }
                    }
                }
            }
            let order: Vec<u64> = blocks.into_iter().flat_map(|(_, members, _)| members).collect();
            self.order_siblings(scope, &order);
        }
    }

    /// Copies whose parent was also copied point at the parent's copy. `pairs` is `(original, copy)`.
    pub(crate) fn reparent_copies(&mut self, pairs: &[(u64, u64)]) {
        for s in &mut self.shapes {
            if pairs.iter().any(|&(_, copy)| copy == s.id)
                && let Some(&(_, copy)) = pairs.iter().find(|&&(original, _)| Some(original) == s.parent)
            {
                s.parent = Some(copy);
            }
        }
        self.settle(None);
    }

    fn shift_tree(&mut self, ids: &[u64], dx: f32, dy: f32) {
        let tree = self.with_descendants(ids);
        for s in self.shapes.iter_mut().filter(|s| tree.contains(&s.id)) {
            s.x_in += dx;
            s.y_in += dy;
        }
    }

    /// Rewrites the z-order so the children of `parent` run in `order`, keeping every subtree contiguous.
    fn order_siblings(&mut self, parent: Option<u64>, order: &[u64]) {
        let mut forest = Forest::of(&self.shapes);
        let index = |id: u64| self.shapes.iter().position(|s| s.id == id);
        let scope: Vec<usize> = order.iter().filter_map(|&id| index(id)).collect();
        match parent.and_then(index) {
            Some(p) => forest.kids[p] = scope,
            None => forest.roots = scope,
        }
        self.permute(forest.emit());
    }

    fn permute(&mut self, order: Vec<usize>) {
        if order.iter().enumerate().all(|(i, &j)| i == j) {
            return;
        }
        let mut slots: Vec<Option<Shape>> = std::mem::take(&mut self.shapes).into_iter().map(Some).collect();
        self.shapes = order.into_iter().filter_map(|i| slots[i].take()).collect();
    }

    /// Invariants 1 to 3.
    fn repair(&mut self) {
        for s in self.shapes.iter_mut().filter(|s| s.compound().is_some()) {
            s.container = None;
        }
        let parents: HashSet<u64> = self.shapes.iter().filter(|s| s.holds_children()).map(|s| s.id).collect();
        for s in &mut self.shapes {
            if s.parent.is_some_and(|p| p == s.id || !parents.contains(&p)) {
                s.parent = None;
            }
        }
        self.break_parent_cycles();
        self.dissolve_empty_compounds();
        let mut unlinked = false;
        for s in self.shapes.iter_mut().filter(|s| s.container.is_some() && s.link.is_some()) {
            s.link = None;
            unlinked = true;
        }
        if unlinked {
            self.prune_links();
        }
        let mut link_parents: HashMap<u64, Option<u64>> = HashMap::new();
        let mut straddling: HashSet<u64> = HashSet::new();
        for s in &self.shapes {
            if let Some(l) = s.link
                && *link_parents.entry(l.id).or_insert(s.parent) != s.parent
            {
                straddling.insert(l.id);
            }
        }
        for s in self.shapes.iter_mut().filter(|s| s.link.is_some_and(|l| straddling.contains(&l.id))) {
            s.parent = None;
        }
    }

    /// Clears the parent of any shape whose ancestor chain comes back to it.
    pub(crate) fn break_parent_cycles(&mut self) {
        let mut parents: HashMap<u64, Option<u64>> = self.shapes.iter().map(|s| (s.id, s.parent)).collect();
        for s in &mut self.shapes {
            let mut at = s.parent;
            for _ in 0..parents.len() {
                match at {
                    Some(p) if p == s.id => {
                        s.parent = None;
                        parents.insert(s.id, None);
                        break;
                    }
                    Some(p) => at = parents.get(&p).copied().flatten(),
                    None => break,
                }
            }
        }
    }

    /// Removes every Compound with no children, repeating as a removal empties its parent.
    fn dissolve_empty_compounds(&mut self) {
        loop {
            let empty: Vec<u64> = self.shapes.iter().filter(|c| c.compound().is_some() && !self.shapes.iter().any(|s| s.parent == Some(c.id))).map(|c| c.id).collect();
            if empty.is_empty() {
                return;
            }
            self.shapes.retain(|s| !empty.contains(&s.id));
            self.prune_links();
        }
    }

    /// Invariant 4.
    fn normalise_z(&mut self) {
        self.permute(Forest::of(&self.shapes).emit());
    }

    /// Rigidly moves the descendants of each container that moved or turned since `before`, and remaps
    /// those of each Compound whose box, rotation or flips changed. Runs on the normalised z-order, so
    /// ancestors come first and a shape carried by its parent is not carried again by itself.
    fn carry(&mut self, before: &Design) {
        let pose = |s: &Shape| (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg);
        let posed = |s: &Shape| (pose(s), s.flip_x, s.flip_y);
        let mut carried: HashSet<u64> = HashSet::new();
        for i in 0..self.shapes.len() {
            let now = &self.shapes[i];
            if !now.holds_children() || carried.contains(&now.id) {
                continue;
            }
            let Some(was) = find(before, now.id).cloned() else { continue };
            let now = now.clone();
            let tree = self.with_descendants(&[now.id]);
            // A box that already fits the children with the same turn and flips is a refit, as when a
            // child joins or leaves inside the edit, not the user moving or sizing the group.
            let turned = (now.rotation_deg, now.flip_x, now.flip_y) != (was.rotation_deg, was.flip_x, was.flip_y);
            let refitted = now.compound().is_some() && !turned && self.fit(&now).is_none_or(|f| !moves_box(&now, f));
            let moves = self.shapes.iter_mut().filter(|s| s.id != now.id && tree.contains(&s.id));
            if now.compound().is_some() {
                if posed(&now) == posed(&was) || refitted {
                    continue;
                }
                for s in moves.filter(|s| find(before, s.id).is_some_and(|b| posed(b) == posed(s))) {
                    remap(s, &was, &now);
                    carried.insert(s.id);
                }
                continue;
            }
            let rigid = (now.width_in, now.height_in, now.flip_x, now.flip_y) == (was.width_in, was.height_in, was.flip_x, was.flip_y);
            if !rigid || pose(&now) == pose(&was) {
                continue;
            }
            let (from, to, turn) = (was.centre_in(), now.centre_in(), now.rotation_deg - was.rotation_deg);
            for s in moves.filter(|s| find(before, s.id).is_some_and(|b| pose(b) == pose(s))) {
                carry_rigidly(s, from, to, turn);
                carried.insert(s.id);
            }
        }
    }

    /// Invariant 5. Children before parents (reverse z-order), so an inner Hug size or Compound box
    /// settles before its parent measures it. A moved child takes its own subtree along.
    fn relayout(&mut self, before: Option<&Design>) {
        for i in (0..self.shapes.len()).rev() {
            if self.shapes[i].compound().is_some() {
                self.refit(i, before);
                continue;
            }
            if !self.shapes[i].container.is_some_and(|r| r.layout.is_some()) {
                continue;
            }
            let id = self.shapes[i].id;
            let placed = layout(self, &self.shapes[i], &self.children(Some(id)).collect::<Vec<_>>());
            for (child, x, y) in placed.moves {
                let Some(s) = find(self, child) else { continue };
                let (dx, dy) = (x - s.x_in, y - s.y_in);
                if dx.abs() > EPS_IN || dy.abs() > EPS_IN {
                    self.shift_tree(&[child], dx, dy);
                }
            }
            for (child, rotation_deg) in placed.rotations {
                let off = |s: &Shape| (s.rotation_deg - rotation_deg).rem_euclid(360.0).min((rotation_deg - s.rotation_deg).rem_euclid(360.0)) > EPS_IN;
                if let Some(s) = self.shape_mut(child).filter(|s| off(s)) {
                    s.rotation_deg = rotation_deg;
                }
            }
            if let Some((w, h)) = placed.size {
                (self.shapes[i].width_in, self.shapes[i].height_in) = (w, h);
            }
        }
    }

    /// Fits the Compound at index `i` to its visible direct children's boxes, measured in its own frame,
    /// keeping its rotation and flips. Skipped when its own box, rotation or flips changed since `before`:
    /// `carry` has already moved the children to it, and the user's box stands.
    fn refit(&mut self, i: usize, before: Option<&Design>) {
        let g = &self.shapes[i];
        let own = |s: &Shape| (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg, s.flip_x, s.flip_y);
        if before.and_then(|b| find(b, g.id)).is_some_and(|was| own(was) != own(g)) {
            return;
        }
        if let Some(fit) = self.fit(g).filter(|&f| moves_box(g, f)) {
            let g = &mut self.shapes[i];
            (g.x_in, g.y_in, g.width_in, g.height_in) = fit;
        }
    }

    /// The box, as x, y, width and height, that fits Compound `g`'s visible direct children's boxes in
    /// its own frame, keeping its rotation and flips. None with no visible child.
    fn fit(&self, g: &Shape) -> Option<(f32, f32, f32, f32)> {
        let box_corners = |k: &Shape| [(0.0, 0.0), (k.width_in, 0.0), (k.width_in, k.height_in), (0.0, k.height_in)].map(|c| g.to_local(k.to_world(c)));
        let corners = self.children(Some(g.id)).filter(|k| !k.hidden).flat_map(box_corners);
        let b = corners.map(|(x, y)| Aabb { min_x: x, min_y: y, max_x: x, max_y: y }).reduce(Aabb::union)?;
        let (cx, cy) = g.to_world(b.centre());
        let (w, h) = (b.width(), b.height());
        Some((cx - w / 2.0, cy - h / 2.0, w, h))
    }
}

/// Whether writing box `(x, y, w, h)` would move any edge of `g` by more than `EPS_IN`.
fn moves_box(g: &Shape, (x, y, w, h): (f32, f32, f32, f32)) -> bool {
    [x - g.x_in, y - g.y_in, w - g.width_in, h - g.height_in].iter().any(|d| d.abs() > EPS_IN)
}

/// Moves `s` with a Compound whose box went from `was` to `now`: its centre keeps its place in the box,
/// its size scales along its own axes (exact for a child aligned with the box, close otherwise), and it
/// turns and mirrors as the box did.
pub(crate) fn remap(s: &mut Shape, was: &Shape, now: &Shape) {
    let ratio = |a: f32, b: f32| if b.abs() > f32::EPSILON { a / b } else { 1.0 };
    let (sx, sy) = (ratio(now.width_in, was.width_in), ratio(now.height_in, was.height_in));
    let (lx, ly) = was.to_local(s.centre_in());
    let (cx, cy) = now.to_world((lx * sx, ly * sy));
    let (sin, cos) = (s.rotation_deg - was.rotation_deg).to_radians().sin_cos();
    s.width_in *= (sx * cos).hypot(sy * sin);
    s.height_in *= (sx * sin).hypot(sy * cos);
    (s.x_in, s.y_in) = (cx - s.width_in / 2.0, cy - s.height_in / 2.0);
    let turn = now.rotation_deg - was.rotation_deg;
    let rotation = match (now.flip_x != was.flip_x, now.flip_y != was.flip_y) {
        (false, false) => s.rotation_deg + turn,
        (true, true) => s.rotation_deg + turn + 180.0,
        (true, false) => {
            s.flip_x = !s.flip_x;
            now.rotation_deg + was.rotation_deg - s.rotation_deg
        }
        (false, true) => {
            s.flip_y = !s.flip_y;
            now.rotation_deg + was.rotation_deg - s.rotation_deg
        }
    };
    s.rotation_deg = rotation.rem_euclid(360.0);
}

/// Moves `s` as if pinned to a container whose centre went from `from` to `to` while turning `turn_deg`.
fn carry_rigidly(s: &mut Shape, from: Pt, to: Pt, turn_deg: f32) {
    let (cx, cy) = s.centre_in();
    let (sin, cos) = turn_deg.to_radians().sin_cos();
    let (dx, dy) = (cx - from.0, cy - from.1);
    s.x_in = to.0 + dx * cos - dy * sin - s.width_in / 2.0;
    s.y_in = to.1 + dx * sin + dy * cos - s.height_in / 2.0;
    if turn_deg != 0.0 {
        s.rotation_deg = (s.rotation_deg + turn_deg).rem_euclid(360.0);
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::*;
    use crate::design::{Align, Arrange, ShapeKind};
    use crate::links::LinkKind;

    fn square(d: &mut Design, (x, y): Pt, size: f32) -> u64 {
        let id = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, size, size);
        id
    }

    fn shape(d: &Design, id: u64) -> &Shape {
        d.shapes.iter().find(|s| s.id == id).unwrap()
    }

    fn bounds(d: &Design, id: u64) -> (f32, f32, f32, f32) {
        shape(d, id).world_bounds()
    }

    fn order(d: &Design) -> Vec<u64> {
        d.shapes.iter().map(|s| s.id).collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn role(padding_in: f32, layout: Option<Flow>) -> ContainerRole {
        ContainerRole { padding_in, layout }
    }

    fn row(gap_in: f32, sizing: Sizing) -> Flow {
        Flow { axis: Axis::Row, gap_in, cross: Cross::Start, sizing }
    }

    /// A `w` x `h` container at `at` holding `kids`, settled.
    fn card(d: &mut Design, at: Pt, (w, h): Pt, role: ContainerRole, kids: &[u64]) -> u64 {
        let id = square(d, at, w);
        let s = d.shape_mut(id).unwrap();
        s.height_in = h;
        s.container = Some(role);
        d.reparent(kids, Some(id));
        id
    }

    #[test]
    fn align_left_to_container_puts_each_child_on_the_inner_left_edge() {
        let mut d = Design::default();
        let a = square(&mut d, (2.0, 1.0), 1.0);
        let b = square(&mut d, (3.5, 3.0), 0.5);
        let c = card(&mut d, (0.0, 0.0), (6.0, 6.0), role(0.5, None), &[a, b]);
        assert_eq!(d.align_container(&[a, b]), Some(c));
        d.align_to(&[a, b], Align::Left, AlignTo::Container);
        assert!(close(bounds(&d, a).0, 0.5) && close(bounds(&d, b).0, 0.5));
        assert!(close(bounds(&d, a).1, 1.0) && close(bounds(&d, b).1, 3.0), "vertical positions are untouched");
    }

    #[test]
    fn distribute_to_container_packs_from_padding_edge_to_padding_edge() {
        let mut d = Design::default();
        let kids = [square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (2.2, 1.0), 1.0), square(&mut d, (2.5, 2.0), 1.0)];
        card(&mut d, (0.0, 0.0), (6.0, 4.0), role(0.5, None), &kids);
        assert_eq!(d.align_units_to(&kids, AlignTo::Container), 3);
        d.align_to(&kids, Align::DistributeH, AlignTo::Container);
        let lefts: Vec<f32> = kids.iter().map(|&k| bounds(&d, k).0).collect();
        assert!(close(lefts[0], 0.5) && close(bounds(&d, kids[2]).2, 5.5), "first and last touch the padding edges: {lefts:?}");
        let gaps = [lefts[1] - (lefts[0] + 1.0), lefts[2] - (lefts[1] + 1.0)];
        assert!(gaps.iter().all(|&g| close(g, 1.0)), "(6 - 2 x 0.5 - 3 x 1) / 2 = 1.0 in; the spec's 0.5 in is an arithmetic slip: {gaps:?}");
    }

    #[test]
    fn distribute_to_container_needs_two_units_and_align_one() {
        assert_eq!(AlignTo::Container.min_units(Align::DistributeH), 2);
        assert_eq!(AlignTo::Container.min_units(Align::Left), 1);
        assert_eq!(AlignTo::Selection.min_units(Align::DistributeH), 3);
    }

    #[test]
    fn row_hug_sizes_the_container_and_fixed_spreads_the_children() {
        let mut d = Design::default();
        let kids = [square(&mut d, (9.0, 9.0), 1.0), square(&mut d, (7.0, 9.0), 1.0), square(&mut d, (8.0, 7.0), 1.0)];
        let c = card(&mut d, (0.0, 0.0), (2.0, 2.0), role(0.5, Some(row(0.25, Sizing::Hug))), &kids);
        let placed = layout(&d, shape(&d, c), &kids.iter().map(|&k| shape(&d, k)).collect::<Vec<_>>());
        assert_eq!(placed.size, None, "already settled, so the hug size holds");
        let s = shape(&d, c);
        assert!(close(s.width_in, 0.5 + 3.0 + 2.0 * 0.25 + 0.5) && close(s.height_in, 2.0), "{} x {}", s.width_in, s.height_in);
        for (i, &k) in kids.iter().enumerate() {
            assert!(close(bounds(&d, k).0, 0.5 + i as f32 * 1.25) && close(bounds(&d, k).1, 0.5), "child {i} at {:?}", bounds(&d, k));
        }

        let s = d.shape_mut(c).unwrap();
        s.width_in = 6.0;
        s.container = Some(role(0.5, Some(row(0.25, Sizing::Fixed))));
        d.settle(None);
        assert!(close(shape(&d, c).width_in, 6.0), "Fixed keeps the size");
        let lefts: Vec<f32> = kids.iter().map(|&k| bounds(&d, k).0).collect();
        assert!(close(lefts[0], 0.5) && close(lefts[1], 2.5) && close(lefts[2], 4.5), "space between: {lefts:?}");
    }

    #[test]
    fn a_radial_container_rings_its_children_and_faces_them_outward() {
        let mut d = Design::default();
        let kids = [square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (1.0, 0.0), 1.0), square(&mut d, (2.0, 0.0), 1.0), square(&mut d, (3.0, 0.0), 1.0)];
        let radial =
            Flow { axis: Axis::Radial { radius_in: 3.0, start_deg: 0.0, sweep_deg: 360.0, rotate_items: true }, gap_in: 0.0, cross: Cross::Start, sizing: Sizing::Fixed };
        card(&mut d, (0.0, 0.0), (10.0, 10.0), role(0.0, Some(radial)), &kids);
        d.settle(None);
        let centre = |id: u64| {
            let (x0, y0, x1, y1) = bounds(&d, id);
            ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
        };
        let want = [(5.0, 2.0), (8.0, 5.0), (5.0, 8.0), (2.0, 5.0)];
        for (k, (wx, wy)) in want.into_iter().enumerate() {
            let (cx, cy) = centre(kids[k]);
            assert!(close(cx, wx) && close(cy, wy), "child {k} at ({cx}, {cy}), wanted ({wx}, {wy})");
            let rot = shape(&d, kids[k]).rotation_deg;
            assert!(close(rot, k as f32 * 90.0), "child {k} faces {rot}, wanted {}", k as f32 * 90.0);
        }
    }

    #[test]
    fn pure_layout_reports_moves_and_hug_size() {
        let kid = |id: u64, x: f32| Shape { id, x_in: x, y_in: 5.0, width_in: 1.0, height_in: 1.0, ..blank() };
        let container = Shape { width_in: 1.0, height_in: 1.0, container: Some(role(0.5, Some(row(0.25, Sizing::Hug)))), ..blank() };
        let (a, b, c) = (kid(2, 4.0), kid(3, 8.0), kid(4, 6.0));
        let placed = layout(&Design::default(), &container, &[&a, &b, &c]);
        let (w, h) = placed.size.unwrap();
        assert!(close(w, 1.0 + 3.0 + 0.5) && close(h, 2.0));
        let (id, x, y) = placed.moves[1];
        assert!(id == 3 && close(x, 0.5 + 1.0 + 0.25) && close(y, 0.5));
    }

    fn blank() -> Shape {
        let mut d = Design::default();
        let id = d.add_shape(ShapeKind::Square);
        let mut s = d.shapes.remove(0);
        (s.id, s.x_in, s.y_in) = (id, 0.0, 0.0);
        s
    }

    #[test]
    fn column_and_grid_place_cells_with_gaps() {
        let mut d = Design::default();
        let kids = [square(&mut d, (9.0, 9.0), 1.0), square(&mut d, (7.0, 9.0), 0.5), square(&mut d, (8.0, 7.0), 1.0)];
        let col = Flow { axis: Axis::Column, gap_in: 0.25, cross: Cross::Centre, sizing: Sizing::Hug };
        let c = card(&mut d, (0.0, 0.0), (2.0, 2.0), role(0.5, Some(col)), &kids);
        assert!(close(bounds(&d, kids[1]).0, 0.75) && close(bounds(&d, kids[1]).1, 1.75), "centred in the column: {:?}", bounds(&d, kids[1]));
        assert!(close(shape(&d, c).height_in, 2.5 + 2.0 * 0.25 + 2.0 * 0.5));

        let grid = Flow { axis: Axis::Grid { columns: NonZeroU16::new(2).unwrap() }, gap_in: 0.25, cross: Cross::Start, sizing: Sizing::Hug };
        d.shape_mut(c).unwrap().container = Some(role(0.5, Some(grid)));
        d.settle(None);
        assert!(close(bounds(&d, kids[1]).0, 1.75) && close(bounds(&d, kids[1]).1, 0.5));
        assert!(close(bounds(&d, kids[2]).0, 0.5) && close(bounds(&d, kids[2]).1, 1.75));
        assert!(close(shape(&d, c).width_in, 1.0 + 1.25 + 0.5) && close(shape(&d, c).height_in, 1.0 + 2.25));
    }

    #[test]
    fn a_grouped_pair_aligns_and_lays_out_as_one_unit() {
        let mut d = Design::default();
        let a = square(&mut d, (2.0, 2.0), 1.0);
        let b = square(&mut d, (3.5, 2.5), 1.0);
        let lone = square(&mut d, (1.0, 4.0), 1.0);
        d.group(&[a, b]).unwrap();
        let c = card(&mut d, (0.0, 0.0), (8.0, 8.0), role(0.5, None), &[a, lone]);
        assert_eq!(shape(&d, b).parent, Some(c), "the whole link moves in");
        assert_eq!(d.align_units_to(&[a, b, lone], AlignTo::Container), 2);
        d.align_to(&[a, b], Align::Right, AlignTo::Container);
        assert!(close(bounds(&d, b).2, 7.5) && close(bounds(&d, a).0, 7.5 - 2.5), "the pair aligns as one and keeps its offset");

        d.shape_mut(c).unwrap().container = Some(role(0.5, Some(row(0.25, Sizing::Hug))));
        d.settle(None);
        let (ba, bb, bl) = (bounds(&d, a), bounds(&d, b), bounds(&d, lone));
        assert!(close(ba.0, 0.5) && close(bb.0 - ba.0, 1.5) && close(bb.1 - ba.1, 0.5), "pair keeps its internal offset: {ba:?} {bb:?}");
        assert!(close(bl.0, 0.5 + 2.5 + 0.25), "the lone square follows the pair: {bl:?}");
    }

    #[test]
    fn moving_and_rotating_a_container_carries_its_children() {
        let mut d = Design::default();
        let kid = square(&mut d, (3.5, 1.5), 1.0);
        let nested_kid = square(&mut d, (1.0, 1.0), 0.5);
        let inner = card(&mut d, (0.5, 0.5), (1.5, 1.5), role(0.1, None), &[nested_kid]);
        let c = card(&mut d, (0.0, 0.0), (6.0, 4.0), role(0.25, None), &[kid, inner]);
        assert_eq!(shape(&d, inner).parent, Some(c));

        let before = d.clone();
        d.shape_mut(c).unwrap().x_in += 2.0;
        d.settle(Some(&before));
        assert!(close(shape(&d, kid).x_in, 5.5) && close(shape(&d, nested_kid).x_in, 3.0) && close(shape(&d, inner).x_in, 2.5));

        let before = d.clone();
        d.shape_mut(c).unwrap().rotation_deg = 90.0;
        d.settle(Some(&before));
        let (cx, cy) = shape(&d, c).centre_in();
        let (kx, ky) = shape(&d, kid).centre_in();
        assert!(close(kx - cx, -0.0) && close(ky - cy, 1.0), "the child at (+1, 0) from the centre turns to (0, +1): {:?}", (kx - cx, ky - cy));
        assert!(close(shape(&d, kid).rotation_deg, 90.0) && close(shape(&d, nested_kid).rotation_deg, 90.0));

        let kept = d.clone();
        d.uncontain(&[c]);
        assert!(shape(&d, c).container.is_none() && shape(&d, kid).parent.is_none() && shape(&d, inner).parent.is_none());
        for (a, b) in d.shapes.iter().zip(&kept.shapes) {
            assert_eq!((a.x_in, a.y_in, a.rotation_deg), (b.x_in, b.y_in, b.rotation_deg), "Remove container moved shape {}", a.id);
        }
        assert_eq!(shape(&d, nested_kid).parent, Some(inner), "nested containers keep their own children");
    }

    #[test]
    fn a_child_the_edit_moved_is_not_carried_twice() {
        let mut d = Design::default();
        let kid = square(&mut d, (1.0, 1.0), 1.0);
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[kid]);
        let before = d.clone();
        for id in [c, kid] {
            d.shape_mut(id).unwrap().x_in += 1.0;
        }
        d.settle(Some(&before));
        assert!(close(shape(&d, kid).x_in, 2.0));
    }

    #[test]
    fn resizing_a_container_does_not_carry() {
        let mut d = Design::default();
        let kid = square(&mut d, (1.0, 1.0), 1.0);
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[kid]);
        let before = d.clone();
        let s = d.shape_mut(c).unwrap();
        (s.x_in, s.width_in) = (-1.0, 5.0);
        d.settle(Some(&before));
        assert_eq!(shape(&d, kid).x_in, 1.0);
    }

    #[test]
    fn a_guide_container_makes_nothing_and_a_cut_container_shares_its_mat() {
        use crate::composition::compose;
        use crate::machine::MachineFamily;
        let mut d = Design::default();
        let star = square(&mut d, (1.0, 1.0), 0.5);
        let dot = square(&mut d, (3.0, 2.0), 0.5);
        let id = d.add_shape(ShapeKind::Circle);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (0.0, 0.0, 4.0, 4.0);
        d.contain(&[id]);
        d.reparent(&[star, dot], Some(id));

        let mats = compose(&d, MachineFamily::StudioPro.preferred_mat(), 1);
        assert_eq!(mats.len(), 1);
        let at = |i: u64| mats[0].items.iter().find(|m| m.piece.owner == i).unwrap().offset_in;
        for kid in [star, dot] {
            let (cb, kb) = (bounds(&d, id), bounds(&d, kid));
            let (o, k) = (at(id), at(kid));
            assert!(close(k.0 - o.0, kb.0 - cb.0) && close(k.1 - o.1, kb.1 - cb.1), "child {kid} keeps its offset on the circle's mat");
        }

        d.shape_mut(id).unwrap().set_operation(crate::design::Operation::Guide);
        let mats = compose(&d, MachineFamily::StudioPro.preferred_mat(), 1);
        let items: Vec<u64> = mats.iter().flat_map(|m| m.items.iter().map(|i| i.piece.owner)).collect();
        assert!(!items.contains(&id), "a Guide container contributes nothing");
        assert_eq!(items.len(), 2);
        assert!(close(mats[0].items[0].offset_in.0, mats[0].items[1].offset_in.0 - 0.75), "children pack on their own: {mats:?}");
    }

    #[test]
    fn a_project_saved_before_containers_loads_without_them() {
        let json = r#"{"id":"pre-containers","name":"Before containers","design":{"shapes":[{"id":1,"kind":"Square","x_in":0.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]},{"id":2,"kind":"Circle","x_in":3.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]}]},"created":1,"modified":1}"#;
        let project: crate::project::Project = serde_json::from_str(json).expect("an old project still loads");
        assert_eq!(project.design.shapes.len(), 2);
        assert!(project.design.shapes.iter().all(|s| s.container.is_none() && s.parent.is_none()));
    }

    #[test]
    fn containers_round_trip_through_json() {
        let mut d = Design::default();
        let kid = square(&mut d, (1.0, 1.0), 1.0);
        let grid = Flow { axis: Axis::Grid { columns: NonZeroU16::new(3).unwrap() }, gap_in: 0.1, cross: Cross::End, sizing: Sizing::Fixed };
        card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, Some(grid)), &[kid]);
        let back: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn settle_is_idempotent_and_leaves_flat_designs_alone() {
        let mut flat = Design::default();
        for i in 0..4 {
            square(&mut flat, (i as f32 * 0.7, 0.3), 1.0);
        }
        flat.attach(&[2, 3]).unwrap();
        let before = flat.clone();
        flat.settle(None);
        assert_eq!(flat, before);

        let mut d = Design::default();
        let kids = [square(&mut d, (9.0, 9.0), 1.3), square(&mut d, (7.0, 9.0), 0.7)];
        let flow = Flow { axis: Axis::Row, gap_in: 0.3, cross: Cross::Centre, sizing: Sizing::Hug };
        card(&mut d, (0.1, 0.2), (2.0, 2.0), role(0.35, Some(flow)), &kids);
        let settled = d.clone();
        d.settle(None);
        d.settle(Some(&settled));
        assert_eq!(d, settled, "a settled design is unchanged bit for bit");
    }

    #[test]
    fn align_to_container_with_one_child_centres_it() {
        let mut d = Design::default();
        let kid = square(&mut d, (0.5, 0.5), 1.0);
        let c = card(&mut d, (0.0, 0.0), (5.0, 3.0), role(0.25, None), &[kid]);
        assert_eq!(d.align_units_to(&[kid], AlignTo::Container), 1);
        assert_eq!(d.align_units_to(&[kid], AlignTo::Selection), 1);
        d.align_to(&[kid], Align::Centre, AlignTo::Container);
        let (x, y) = shape(&d, kid).centre_in();
        assert!(close(x, 2.5) && close(y, 1.5), "{x}, {y}");
        assert_eq!(d.align_container(&[c]), Some(c));
    }

    #[test]
    fn selecting_the_container_aligns_its_children() {
        let mut d = Design::default();
        let kids = [square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (3.0, 2.0), 1.0)];
        let c = card(&mut d, (0.0, 0.0), (6.0, 4.0), role(0.5, None), &kids);
        assert_eq!(d.align_container(&[c]), Some(c));
        assert_eq!(d.align_units_to(&[c], AlignTo::Container), 2);
        d.align_to(&[c], Align::Bottom, AlignTo::Container);
        assert!(kids.iter().all(|&k| close(bounds(&d, k).3, 3.5)));
        assert_eq!(shape(&d, c).y_in, 0.0, "the container stays put");
    }

    #[test]
    fn align_to_container_is_off_under_a_layout_or_mixed_parents() {
        let mut d = Design::default();
        let (a, b) = (square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (8.0, 1.0), 1.0));
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[a]);
        assert_eq!(d.align_container(&[a, b]), None);
        d.shape_mut(c).unwrap().container = Some(role(0.25, Some(row(0.0, Sizing::Hug))));
        assert_eq!(d.align_container(&[a]), None);
        assert_eq!(d.align_container(&[c]), None);
    }

    #[test]
    fn reparent_refuses_cycles() {
        let mut d = Design::default();
        let inner = square(&mut d, (1.0, 1.0), 1.0);
        d.contain(&[inner]);
        let outer = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[inner]);
        let before = d.clone();
        d.reparent(&[outer], Some(inner));
        d.reparent(&[outer], Some(outer));
        assert_eq!(d, before);
    }

    #[test]
    fn drop_target_picks_the_front_most_container_and_drop_at_reparents() {
        let mut d = Design::default();
        let back = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[]);
        let front = card(&mut d, (2.0, 2.0), (4.0, 4.0), role(0.25, None), &[]);
        let kid = square(&mut d, (9.0, 9.0), 1.0);
        assert_eq!(d.drop_target(&[kid], (3.0, 3.0)), Some(front));
        assert_eq!(d.drop_target(&[kid], (1.0, 1.0)), Some(back));
        assert_eq!(d.drop_target(&[kid], (0.1, 1.0)), None, "container padding is outside the drop target");
        assert_eq!(d.drop_target(&[front], (3.0, 3.0)), Some(back), "a dragged container never drops into itself");
        assert_eq!(d.drop_target(&[kid], (9.0, 9.0)), None);

        d.drop_at(&[kid], (3.0, 3.0));
        assert_eq!(d.parent_of(kid), Some(front));
        let settled = d.clone();
        d.drop_at(&[kid], (3.0, 3.0));
        assert_eq!(d, settled, "dropping where it already is changes nothing");
        d.drop_at(&[kid], (9.0, 9.0));
        assert_eq!(d.parent_of(kid), None);
    }

    #[test]
    fn wrap_in_container_makes_a_guide_rectangle_around_the_selection() {
        let mut d = Design::default();
        let other = square(&mut d, (9.0, 9.0), 1.0);
        let (a, b) = (square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (3.0, 2.0), 1.0));
        assert!(d.can_contain(&[a, b]) && d.contain_wraps(&[a, b]));
        assert!(d.can_contain(&[a]) && !d.contain_wraps(&[a]));
        let c = d.contain(&[a, b]).unwrap();
        let s = shape(&d, c);
        assert_eq!(s.operation, crate::design::Operation::Guide);
        assert_eq!(s.container, Some(role(DEFAULT_PADDING_IN, None)));
        let cb = bounds(&d, c);
        assert!(close(cb.0, 0.75) && close(cb.1, 0.75) && close(cb.2, 4.25) && close(cb.3, 3.25), "{cb:?}");
        assert_eq!(d.children(Some(c)).map(|s| s.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(order(&d), [other, c, a, b]);
        assert!(!d.can_contain(&[c]) && d.has_container(&[c]) && !d.has_container(&[a]));
        assert_eq!(d.contain(&[c]), Some(c), "making a container a container again returns it");
    }

    #[test]
    fn make_container_unlinks_and_links_refuse_containers() {
        let mut d = Design::default();
        let (a, b, x) = (square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (3.0, 1.0), 1.0), square(&mut d, (5.0, 1.0), 1.0));
        d.group(&[a, b]).unwrap();
        assert_eq!(d.contain(&[a]), Some(a));
        assert!(d.shapes.iter().all(|s| s.link.is_none()), "the container left its group, which dissolved");
        assert_eq!(d.group(&[a, x]), None);
        assert_eq!(d.attach(&[x, a]), None);
    }

    #[test]
    fn z_order_keeps_children_directly_after_their_container() {
        let mut d = Design::default();
        let kid = square(&mut d, (1.0, 1.0), 1.0);
        let c = square(&mut d, (0.0, 0.0), 4.0);
        let top = square(&mut d, (6.0, 0.0), 1.0);
        d.contain(&[c]);
        d.shape_mut(kid).unwrap().parent = Some(c);
        d.settle(None);
        assert_eq!(order(&d), [c, kid, top]);
        assert_eq!(d.roots(&[kid, c, top]), [c, top]);
        assert_eq!(d.with_descendants(&[c]), [c, kid]);
    }

    #[test]
    fn repair_drops_dangling_and_cyclic_parents() {
        let mut d = Design::default();
        let (a, b) = (square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (2.0, 0.0), 1.0));
        let plain = square(&mut d, (4.0, 0.0), 1.0);
        d.contain(&[a]);
        d.contain(&[b]);
        d.shape_mut(a).unwrap().parent = Some(b);
        d.shape_mut(b).unwrap().parent = Some(a);
        d.shape_mut(plain).unwrap().parent = Some(99);
        d.settle(None);
        assert_eq!(d.shapes.len(), 3, "no shape is lost to a cycle");
        assert!(d.parent_of(a).is_none() || d.parent_of(b).is_none());
        assert_eq!(d.parent_of(plain), None);
    }

    #[test]
    fn arrange_is_sibling_scoped_and_carries_subtrees() {
        let mut d = Design::default();
        let (k1, k2) = (square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (2.0, 1.0), 1.0));
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[k1, k2]);
        let x = square(&mut d, (6.0, 0.0), 1.0);
        assert_eq!(order(&d), [c, k1, k2, x]);
        d.arrange(&[k1], Arrange::ToFront);
        assert_eq!(order(&d), [c, k2, k1, x], "a child steps forward only among its siblings");
        d.arrange(&[c], Arrange::ToFront);
        assert_eq!(order(&d), [x, c, k2, k1], "the container takes its children with it");
        d.arrange(&[k1], Arrange::ToBack);
        assert_eq!(order(&d), [x, c, k1, k2]);
    }

    #[test]
    fn deleting_a_container_deletes_its_subtree() {
        let mut d = Design::default();
        let nested = square(&mut d, (1.0, 1.0), 0.5);
        let inner = card(&mut d, (0.5, 0.5), (2.0, 2.0), role(0.1, None), &[nested]);
        let outer = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[inner]);
        let keep = square(&mut d, (6.0, 0.0), 1.0);
        d.remove_many(&[outer]);
        assert_eq!(order(&d), [keep]);
    }

    #[test]
    fn combining_children_keeps_the_result_in_their_container() {
        let mut d = Design::default();
        let a = square(&mut d, (1.0, 1.0), 1.0);
        let b = square(&mut d, (1.5, 1.0), 1.0);
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[a, b]);
        let welded = d.combine(&[a, b], crate::design::Combine::Weld);
        assert_eq!(d.parent_of(welded[0]), Some(c));
    }

    #[test]
    fn duplicating_a_container_copies_its_subtree() {
        let mut d = Design::default();
        let kid = square(&mut d, (1.0, 1.0), 1.0);
        let c = card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[kid]);
        let copies = d.duplicate(&[c], 0.25);
        assert_eq!(copies.len(), 2);
        let (c2, kid2) = (copies[0], copies[1]);
        assert_eq!(d.parent_of(kid2), Some(c2), "the copy points at the copied container");
        assert_eq!(d.parent_of(kid), Some(c), "the original keeps its own");
        assert_eq!(order(&d), [c, kid, c2, kid2]);
    }

    #[test]
    fn a_newly_reparented_child_lands_last_in_a_row() {
        let mut d = Design::default();
        let kids = [square(&mut d, (5.0, 5.0), 1.0), square(&mut d, (7.0, 5.0), 1.0)];
        let c = card(&mut d, (0.0, 0.0), (2.0, 2.0), role(0.25, Some(row(0.25, Sizing::Hug))), &kids);
        let late = square(&mut d, (0.3, 0.3), 1.0);
        d.reparent(&[late], Some(c));
        assert_eq!(d.children(Some(c)).map(|s| s.id).collect::<Vec<_>>(), [kids[0], kids[1], late]);
        assert!(close(bounds(&d, late).0, 0.25 + 2.0 * 1.25));
    }

    #[test]
    fn slide_in_layout_reorders_by_pointer() {
        let mut d = Design::default();
        let kids = [square(&mut d, (5.0, 5.0), 1.0), square(&mut d, (7.0, 5.0), 1.0), square(&mut d, (8.0, 5.0), 1.0)];
        let c = card(&mut d, (0.0, 0.0), (2.0, 2.0), role(0.25, Some(row(0.25, Sizing::Hug))), &kids);
        assert!(d.in_layout(&[kids[0]]) && !d.in_layout(&[c]));
        d.slide_in_layout(&[kids[0]], (3.5, 0.5));
        assert_eq!(d.children(Some(c)).map(|s| s.id).collect::<Vec<_>>(), [kids[1], kids[2], kids[0]]);
        assert!(close(bounds(&d, kids[0]).0, 0.25 + 2.0 * 1.25));
    }

    #[test]
    fn link_members_in_different_containers_are_freed() {
        let mut d = Design::default();
        let (a, b) = (square(&mut d, (1.0, 1.0), 1.0), square(&mut d, (6.0, 1.0), 1.0));
        card(&mut d, (0.0, 0.0), (4.0, 4.0), role(0.25, None), &[a]);
        d.group(&[a, b]).unwrap();
        d.settle(None);
        assert_eq!((d.parent_of(a), d.parent_of(b)), (None, None));
        assert!(d.shapes.iter().any(|s| s.link.is_some_and(|l| l.kind == LinkKind::Group)));
    }
}

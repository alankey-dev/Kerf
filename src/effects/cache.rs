//! The geometry cache behind [`Design::geometry`].
//!
//! One cache is shared by every clone of a design (History snapshots, drag originals, Make). It is not
//! part of the design's value: it always compares equal, clones share it, and serde skips it. Sharing is
//! safe because a hit is an exact comparison of every evaluation input, never a counter or a hash, so it
//! cannot go stale under direct field writes, undo, load or duplicate ids.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::compound::{Member, evaluate_compound, members};
use super::eval::{Geometry, LocalPieces, PieceId, Placement, TooThin, evaluate_local, place};
use super::instance::{at_origin, missing, owned};
use super::model::Style;
use super::model::{Effect, EffectId};
use crate::design::{CompoundOp, Design, Pt, Shape, ShapeKind};

/// Recently used slots kept per shape id, so drag originals or a preview under a live shape's id do
/// not evict it.
const SLOTS_PER_ID: usize = 2;
/// The fewest ids kept before a prune; see [`Slots::prune`].
const MIN_BUDGET: usize = 64;

#[derive(Default, Clone)]
pub struct Cache(Rc<RefCell<Slots>>);

#[derive(Default)]
struct Slots {
    by_id: HashMap<u64, Recent>,
    /// Counts prunes; an id used since the last one carries the current value.
    epoch: u64,
    /// How many ids may be kept before the next prune.
    budget: usize,
    #[cfg(test)]
    evaluations: usize,
}

#[derive(Default)]
struct Recent {
    used: u64,
    slots: Vec<Slot>,
}

impl Slots {
    /// Once more ids are kept than the budget allows, drops every id used in neither this prune interval
    /// nor the one before, so ids evaluated earlier in the same frame survive a prune that frame. It asks
    /// nothing of the design, so ids that never sit in `shapes` (an instance's parts) live as long as they
    /// are drawn.
    fn prune(&mut self) {
        if self.by_id.len() <= self.budget.max(MIN_BUDGET) {
            return;
        }
        let since = self.epoch.saturating_sub(1);
        self.by_id.retain(|_, r| r.used >= since);
        self.epoch += 1;
        self.budget = (2 * self.by_id.len()).max(MIN_BUDGET);
    }
}

struct Slot {
    key: Key,
    local: Rc<LocalPieces>,
    placed: Option<(Placement, Rc<Geometry>)>,
}

#[derive(PartialEq)]
enum Key {
    Leaf(LeafKey),
    /// A Compound evaluates its members in its own frame, so its placement is an input too.
    Compound {
        own: LeafKey,
        placement: Placement,
        members: Vec<Member>,
    },
    /// An instance evaluates its parts in its own box at the origin, so its placement is not an input.
    /// `None` is a missing component.
    Instance {
        own: LeafKey,
        members: Option<Vec<Member>>,
    },
}

impl PartialEq for Cache {
    fn eq(&self, _: &Cache) -> bool {
        true
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cache")
    }
}

/// Every input of a leaf's local evaluation. Built and compared by exhaustive destructures of `Shape`, so a
/// field added to `Shape` is a compile error in both until it is classified as an input or ignored.
#[derive(PartialEq)]
struct LeafKey {
    kind: ShapeKind,
    size: Pt,
    contours: super::ContourMask,
    effects: super::Effects,
    style: Style,
    pattern: Option<crate::patterns::PatternFill>,
    /// The font catalogue's generation when `kind` lays out text from it. A scan landing or a writing
    /// font arriving changes it, so text laid out in a fallback is evaluated again.
    fonts: Option<u64>,
}

impl LeafKey {
    fn of(shape: &Shape, fonts: Option<u64>) -> LeafKey {
        #[rustfmt::skip]
        let Shape { id: _, kind, x_in: _, y_in: _, width_in, height_in, rotation_deg: _, flip_x: _, flip_y: _, fill, operation, hidden: _, contours, link: _, pen, effects, container: _, parent: _, skew_deg: _, pivot: _, grain_lock: _, lock_aspect: _, lock_position: _, locked: _, pattern, name: _, monogram: _ } = shape;
        LeafKey {
            kind: kind.clone(),
            size: (*width_in, *height_in),
            contours: contours.clone(),
            effects: effects.clone(),
            style: Style { fill: *fill, operation: *operation, pen: *pen },
            pattern: pattern.clone(),
            fonts,
        }
    }

    /// `self == LeafKey::of(shape, fonts)`, without cloning the shape's kind.
    fn matches(&self, shape: &Shape, fonts: Option<u64>) -> bool {
        #[rustfmt::skip]
        let Shape { id: _, kind, x_in: _, y_in: _, width_in, height_in, rotation_deg: _, flip_x: _, flip_y: _, fill, operation, hidden: _, contours, link: _, pen, effects, container: _, parent: _, skew_deg: _, pivot: _, grain_lock: _, lock_aspect: _, lock_position: _, locked: _, pattern, name: _, monogram: _ } = shape;
        self.fonts == fonts
            && self.size == (*width_in, *height_in)
            && self.style == Style { fill: *fill, operation: *operation, pen: *pen }
            && self.contours == *contours
            && self.effects == *effects
            && self.pattern == *pattern
            && self.kind == *kind
    }
}

/// Whether `kind` lays out text from the font catalogue (a system family or a writing font), directly
/// or in a Flattened part.
fn uses_font_catalogue(kind: &ShapeKind) -> bool {
    use crate::text::FontRef;
    match kind {
        ShapeKind::Text { font: FontRef::System(_) | FontRef::Stroke(_), .. } => true,
        ShapeKind::Flattened { parts, .. } => parts.iter().any(|p| uses_font_catalogue(&p.kind)),
        _ => false,
    }
}

impl Design {
    /// The one geometry entry point: `shape` evaluated and placed. Memoised, so calling it many times a
    /// frame is cheap. `shape` need not be in the design (drag originals, previews): any shape works.
    pub fn geometry(&self, shape: &Shape) -> Rc<Geometry> {
        let local = self.local(&self.shapes, shape);
        let placement = Placement::of(shape);
        let placed = |slots: &Slots| slots.by_id.get(&shape.id)?.slots.iter().find(|s| Rc::ptr_eq(&s.local, &local))?.placed.clone();
        if let Some((_, geometry)) = placed(&self.cache.0.borrow()).filter(|(p, _)| *p == placement) {
            return geometry;
        }
        let geometry = Rc::new(place(&local, shape.id, placement));
        let mut slots = self.cache.0.borrow_mut();
        if let Some(slot) = slots.by_id.get_mut(&shape.id).and_then(|e| e.slots.iter_mut().find(|s| Rc::ptr_eq(&s.local, &local))) {
            slot.placed = Some((placement, geometry.clone()));
        }
        geometry
    }

    /// `shape`'s pieces in its own frame, memoised. A Compound's members are looked up in `scope`. No
    /// borrow of the cache is held while evaluating, since a Compound's evaluation asks for its members'
    /// pieces.
    fn local(&self, scope: &[Shape], shape: &Shape) -> Rc<LocalPieces> {
        match shape.kind {
            ShapeKind::Compound(op) => self.compound_local(scope, shape, op),
            ShapeKind::Instance { .. } => self.instance_local(shape),
            _ => {
                let fonts = uses_font_catalogue(&shape.kind).then(|| crate::system_fonts::current().generation());
                if let Some(hit) = self.hit(shape.id, |k| matches!(k, Key::Leaf(k) if k.matches(shape, fonts))) {
                    return hit;
                }
                self.insert(shape.id, Key::Leaf(LeafKey::of(shape, fonts)), evaluate_local(shape))
            }
        }
    }

    fn compound_local(&self, scope: &[Shape], shape: &Shape, op: CompoundOp) -> Rc<LocalPieces> {
        let members = self.evaluated_members(scope, Some(shape.id));
        let key = Key::Compound { own: LeafKey::of(shape, None), placement: Placement::of(shape), members };
        if let Some(hit) = self.hit(shape.id, |k| *k == key) {
            return hit;
        }
        let Key::Compound { members, .. } = &key else { unreachable!("built above") };
        let local = evaluate_compound(shape, op, members);
        self.insert(shape.id, key, local)
    }

    /// An instance as an effect group over its parts, which are cached under their own ids.
    fn instance_local(&self, shape: &Shape) -> Rc<LocalPieces> {
        let frame = at_origin(shape);
        let key = Key::Instance { own: LeafKey::of(shape, None), members: self.instance_members(shape, &frame) };
        if let Some(hit) = self.hit(shape.id, |k| *k == key) {
            return hit;
        }
        let Key::Instance { members, .. } = &key else { unreachable!("built above") };
        let local = match members {
            Some(members) => owned(evaluate_compound(&frame, CompoundOp::Effects, members)),
            None => missing(shape),
        };
        self.insert(shape.id, key, local)
    }

    /// `instance`'s parts laid out in `frame`, evaluated; `None` when its component is missing.
    fn instance_members(&self, instance: &Shape, frame: &Shape) -> Option<Vec<Member>> {
        let parts = crate::components::parts_in(self, instance, frame)?;
        Some(self.evaluated_members(&parts, None))
    }

    fn evaluated_members(&self, scope: &[Shape], parent: Option<u64>) -> Vec<Member> {
        members(scope, parent)
            .into_iter()
            .map(|(s, unit, link)| Member { id: s.id, link, style: Style::of(s), unit, local: self.local(scope, s), placement: Placement::of(s) })
            .collect()
    }

    /// Whether entry `id` of `shape`, an Offset inset, drops parts too thin for it: the shape's own body,
    /// or an effect group's children or an instance's parts. Worked out on demand for the one entry an
    /// editor shows, so the cached evaluation stays as it is.
    pub fn too_thin(&self, shape: &Shape, id: EffectId) -> Option<TooThin> {
        let Effect::Offset(o) = shape.effects.get(id)?.effect else { return None };
        let before = Shape { effects: shape.effects.before(id), ..shape.clone() };
        let input = match before.kind {
            ShapeKind::Compound(op) => evaluate_compound(&before, op, &self.evaluated_members(&self.shapes, Some(shape.id))),
            ShapeKind::Instance { .. } => {
                let frame = at_origin(&before);
                evaluate_compound(&frame, CompoundOp::Effects, &self.instance_members(&before, &frame)?)
            }
            _ => evaluate_local(&before),
        };
        super::eval::too_thin(&input.pieces, o)
    }

    /// The pieces of `id`'s slot whose key passes `matches`, made most recently used.
    fn hit(&self, id: u64, matches: impl Fn(&Key) -> bool) -> Option<Rc<LocalPieces>> {
        let mut slots = self.cache.0.borrow_mut();
        let epoch = slots.epoch;
        let recent = slots.by_id.get_mut(&id)?;
        let slot = recent.slots.remove(recent.slots.iter().position(|s| matches(&s.key))?);
        let local = slot.local.clone();
        recent.slots.insert(0, slot);
        recent.used = epoch;
        Some(local)
    }

    fn insert(&self, id: u64, key: Key, local: LocalPieces) -> Rc<LocalPieces> {
        let local = Rc::new(local);
        let mut slots = self.cache.0.borrow_mut();
        #[cfg(test)]
        {
            slots.evaluations += 1;
        }
        let epoch = slots.epoch;
        let recent = slots.by_id.entry(id).or_default();
        recent.slots.insert(0, Slot { key, local: local.clone(), placed: None });
        recent.slots.truncate(SLOTS_PER_ID);
        recent.used = epoch;
        slots.prune();
        local
    }

    /// The style of the piece `id` names, when its shape is in the design and still has that piece.
    pub fn piece_style(&self, id: PieceId) -> Option<Style> {
        let shape = self.shapes.iter().find(|s| s.id == id.owner)?;
        self.geometry(shape).piece(id).map(|p| p.style)
    }

    /// How many local evaluations this design's cache has run.
    #[cfg(test)]
    pub(crate) fn evaluations(&self) -> usize {
        self.cache.0.borrow().evaluations
    }
}

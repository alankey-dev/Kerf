//! The design-level operations behind the effect edits, and the consume primitive every destructive
//! operation shares.

use std::collections::HashSet;

use super::eval::{Local, Piece};
use super::model::{Effect, Effects};
use super::rows::{EntryRef, Row};
use crate::design::{Contour, Design, Seg, Shape, ShapeKind};
use crate::geometry::FLATTEN_TOL_IN;

/// What an `Edit::Effect` does to the row it names. `MoveTo` and `Replace` apply to stack entries only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectEdit {
    Enable(bool),
    Remove,
    MoveTo(usize),
    Replace(Effect),
}

/// Whether a new Offset or Sticker goes on each selected shape, or once around all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// One live entry per shape.
    EachShape,
    /// One outline welded around every root; over several roots it wraps them in an effect group.
    Together,
}

impl Shape {
    /// Whether Expand does anything to it: it has effects and is not an instance (Detach instance first), or
    /// it is a Compound.
    pub fn expands(&self) -> bool {
        (!self.effects.is_empty() && self.instance_of().is_none()) || self.compound().is_some()
    }

    /// The one door into a stack: runs `change`, then refits the box about its centre when the leading
    /// Warp changed, so the frame keeps hugging the body. An edit that leaves the leading Warp alone leaves
    /// the box alone, and removing or disabling it restores the size exactly.
    pub(crate) fn change_effects<R>(&mut self, change: impl FnOnce(&mut Effects) -> R) -> R {
        let before = self.effects.leading_warp();
        let out = change(&mut self.effects);
        let after = self.effects.leading_warp();
        self.refit_warp(before, after);
        out
    }
}

impl Design {
    /// Whether adding `reach` over `ids` wraps them in an effect group rather than adding to each.
    pub(crate) fn wraps_together(&self, ids: &[u64], reach: Reach) -> bool {
        reach == Reach::Together && self.roots(ids).len() >= 2
    }

    /// Adds `effect` live to each target, or a welded Offset or a Sticker once around several roots by
    /// wrapping them in an effect group that carries it. Returns the ids the user holds afterwards.
    pub(crate) fn add_effect(&mut self, ids: &[u64], effect: Effect, reach: Reach) -> Vec<u64> {
        if self.wraps_together(ids, reach) && effect.makes_pieces() {
            let mut stack = Effects::default();
            stack.push(effect);
            return self.wrap(ids, crate::design::CompoundOp::Effects, stack).map_or_else(|| ids.to_vec(), |id| vec![id]);
        }
        let targets = if reach == Reach::Together && effect.makes_pieces() { self.roots(ids) } else { ids.to_vec() };
        for s in self.shapes.iter_mut().filter(|s| targets.contains(&s.id)) {
            match effect {
                Effect::Warp(w) => s.set_warp(Some(w)),
                _ => _ = s.change_effects(|fx| fx.push(effect)),
            }
        }
        ids.to_vec()
    }

    /// Edits the row `at` names: a stack entry, or the text curve or contour mask pinned above it.
    pub(crate) fn edit_effect(&mut self, at: EntryRef, op: EffectEdit) {
        let Some(s) = self.shape_mut(at.shape) else { return };
        match (at.row, op) {
            (Row::Entry(id), op) => s.change_effects(|fx| match op {
                EffectEdit::Enable(on) => fx.set_enabled(id, on),
                EffectEdit::Remove => _ = fx.remove(id),
                EffectEdit::MoveTo(index) => fx.move_to(id, index),
                EffectEdit::Replace(effect) => _ = fx.replace(id, effect),
            }),
            (Row::Curve, EffectEdit::Enable(on)) => {
                let before = s.kind.clone();
                if let ShapeKind::Text { curve: Some(c), .. } = &mut s.kind {
                    c.enabled = on;
                }
                self.refit_text(at.shape, &before);
            }
            (Row::Curve, EffectEdit::Remove) => self.set_text_curve(at.shape, 0.0),
            (Row::Contour, EffectEdit::Enable(on)) => s.contours.enabled = on,
            (Row::Contour, EffectEdit::Remove) => s.contours = Default::default(),
            (Row::Curve | Row::Contour, EffectEdit::MoveTo(_) | EffectEdit::Replace(_)) => {}
        }
    }

    /// Expand: each of `ids` with effects becomes plain shapes, as the destructive actions made them
    /// before effects were live. The body keeps the shape's id, links and children; each Offset becomes a
    /// shape behind it in the Offset's style; a Sticker becomes a Flattened shape whose parts are its white
    /// backing and the source with the entries before the Sticker. Disabled entries are dropped. A Compound
    /// expands one level; see [`Design::expand_compound`]. Returns the ids the user holds afterwards.
    pub(crate) fn bake(&mut self, ids: &[u64]) -> Vec<u64> {
        let baking: Vec<u64> = self.shapes.iter().filter(|s| ids.contains(&s.id) && s.expands()).map(|s| s.id).collect();
        let mut held: Vec<u64> = ids.iter().copied().filter(|id| !baking.contains(id)).collect();
        for id in baking {
            let compound = self.shapes.iter().any(|s| s.id == id && s.compound().is_some());
            held.extend(if compound { self.expand_compound(id) } else { self.bake_one(id) });
        }
        held
    }

    fn bake_one(&mut self, id: u64) -> Vec<u64> {
        let Some(at) = self.shapes.iter().position(|s| s.id == id) else { return Vec::new() };
        let source = self.shapes[at].clone();
        let geometry = self.geometry(&source);
        let mut fresh = self.next_id()..;
        let mut out: Vec<Shape> = Vec::new();
        for piece in geometry.pieces() {
            let made = match piece.id.local {
                Local::Body => Some(baked_body(&source, piece)),
                Local::Effect(e) => match source.effects.get(e).map(|e| e.effect) {
                    Some(Effect::Offset(_)) => {
                        let style = piece.style;
                        let rings = geometry.piece_rings(piece, FLATTEN_TOL_IN);
                        crate::design::shape_from_rings(fresh.next().unwrap_or_default(), rings, style.fill, style.operation).map(|s| Shape { pen: style.pen, ..s })
                    }
                    Some(Effect::Sticker(_)) => {
                        let backing = crate::design::shape_from_rings(
                            fresh.next().unwrap_or_default(),
                            geometry.piece_rings(piece, FLATTEN_TOL_IN),
                            egui::Color32::WHITE,
                            crate::design::Operation::PrintThenCut,
                        );
                        let art = Shape { effects: source.effects.before(e), container: None, parent: None, link: None, ..source.clone() };
                        backing.and_then(|b| crate::flatten::flattened(fresh.next().unwrap_or_default(), vec![b, art])).map(|f| Shape {
                            fill: source.fill,
                            pen: source.pen,
                            ..f
                        })
                    }
                    Some(Effect::Warp(_)) | None => None,
                },
                Local::Slice(_) | Local::Part { .. } => None,
            };
            out.extend(made);
        }
        if out.is_empty() {
            self.shapes[at].change_effects(Effects::clear);
            return vec![id];
        }
        if !out.iter().any(|s| s.id == id)
            && let Some(front) = out.last_mut()
        {
            (front.id, front.link, front.container) = (id, source.link, source.container);
        }
        let parent = source.parent;
        // A ring parented into a laid-out container would be laid out as one more item, off its body.
        let laid_out = parent.is_some_and(|p| self.shapes.iter().any(|s| s.id == p && s.container.as_ref().is_some_and(|c| c.layout.is_some())));
        let made: Vec<u64> = out.iter().map(|s| s.id).collect();
        self.shapes.splice(at..=at, out.into_iter().map(|s| Shape { parent: if s.id == id || !laid_out { parent } else { None }, ..s }));
        self.prune_links();
        made
    }

    /// Removes `ids` and all their descendants, as Delete would. Every destructive operation (Combine,
    /// Slice, Flatten, Unflatten, Sticker, Split) calls it, so no child is left behind without its parent.
    pub(crate) fn consume(&mut self, ids: &[u64]) -> Consumed {
        let all = self.with_descendants(ids);
        let at = self.shapes.iter().position(|s| all.contains(&s.id)).unwrap_or(self.shapes.len());
        let parent = self.shapes.iter().find(|s| all.contains(&s.id) && !s.parent.is_some_and(|p| all.contains(&p))).and_then(|s| s.parent);
        self.shapes.retain(|s| !all.contains(&s.id));
        self.prune_links();
        Consumed { at, parent }
    }

    /// Inserts `shapes` at z index `at` under `parent`. Returns their ids.
    pub(crate) fn insert_at(&mut self, at: usize, parent: Option<u64>, shapes: Vec<Shape>) -> Vec<u64> {
        let ids = shapes.iter().map(|s| s.id).collect();
        let at = at.min(self.shapes.len());
        self.shapes.splice(at..at, shapes.into_iter().map(|s| Shape { parent, ..s }));
        self.prune_links();
        ids
    }

    /// The shapes `ids` and their descendants would consume, back to front, each starting clean: no
    /// container role and no parent, so a part never claims children it no longer has. Every Compound
    /// among them is expanded first and every instance detached, so no part is either and each cut path
    /// appears once. An instance of a missing component contributes nothing.
    pub(crate) fn subtree_parts(&self, ids: &[u64]) -> Vec<Shape> {
        let within = |d: &Design, roots: &[u64]| -> HashSet<u64> { d.with_descendants(roots).into_iter().collect() };
        let mut scratch = None::<Design>;
        let mut roots = ids.to_vec();
        loop {
            let d = scratch.as_ref().unwrap_or(self);
            let within = within(d, &roots);
            let unfolding = d.shapes.iter().find(|s| within.contains(&s.id) && (s.compound().is_some() || s.instance_of().is_some()));
            let Some((g, instance)) = unfolding.map(|s| (s.id, s.instance_of().is_some())) else { break };
            let d = scratch.get_or_insert_with(|| self.clone());
            let held = match instance {
                true => d.detach_instances(&[g]),
                false => d.expand_compound(g),
            };
            if held.is_empty() {
                d.remove_many(&[g]);
            }
            if let Some(at) = roots.iter().position(|&r| r == g) {
                roots.splice(at..=at, held);
            }
        }
        let d = scratch.as_ref().unwrap_or(self);
        let all = within(d, &roots);
        d.shapes.iter().filter(|s| all.contains(&s.id)).map(|s| Shape { container: None, parent: None, ..s.clone() }).collect()
    }
}

/// Where [`Design::consume`] took shapes out: the back-most root's z slot and its parent.
pub(crate) struct Consumed {
    pub at: usize,
    pub parent: Option<u64>,
}

/// The body of `source` with its stack baked: unchanged but for the stack when no Warp bends it, else a
/// Path of the bent outline, framed to its bounds and placed where it was drawn.
fn baked_body(source: &Shape, piece: &Piece) -> Shape {
    if source.effects.warp().is_none() {
        return Shape { effects: source.effects.cleared(), ..source.clone() };
    }
    let pts = piece.contours.iter().flat_map(Contour::flatten);
    let (min, max) = pts.fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(a, b), (x, y)| ((a.0.min(x), a.1.min(y)), (b.0.max(x), b.1.max(y))));
    let (w, h) = ((max.0 - min.0).max(0.01), (max.1 - min.1).max(0.01));
    let (cx, cy) = source.to_world((min.0 + w / 2.0, min.1 + h / 2.0));
    let off = |(x, y): crate::design::Pt| (x - min.0, y - min.1);
    let seg = |s: &Seg| match *s {
        Seg::Line(p) => Seg::Line(off(p)),
        Seg::Cubic(a, b, p) => Seg::Cubic(off(a), off(b), off(p)),
    };
    let contours = piece.contours.iter().map(|c| Contour { start: off(c.start), segs: c.segs.iter().map(seg).collect(), closed: c.closed }).collect();
    Shape {
        kind: ShapeKind::Path { contours, view_box: (w, h), node_kinds: Vec::new(), corners: Vec::new() },
        x_in: cx - w / 2.0,
        y_in: cy - h / 2.0,
        width_in: w,
        height_in: h,
        contours: Default::default(),
        effects: source.effects.cleared(),
        ..source.clone()
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::design::{Combine, Design, ShapeKind};
    use crate::edit::{Blocked, Edit};
    use crate::effects::{Corners, Effect, Reach, Sticker};
    use crate::machine::MachineFamily;
    use crate::text::FontRef;
    use crate::text_ops::{Split, TextChange};

    /// A container square 1 holding squares 2 and 3, and a loose circle 4 overlapping it.
    fn holder() -> Design {
        let mut d = Design::default();
        for (kind, x) in [(ShapeKind::Square, 0.0), (ShapeKind::Square, 0.2), (ShapeKind::Square, 0.6), (ShapeKind::Circle, 1.0)] {
            let id = d.add_shape(kind);
            let s = d.shape_mut(id).unwrap();
            (s.x_in, s.y_in) = (x, 0.0);
        }
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2, 3], Some(c));
        d
    }

    /// No dangling parent, no container left but those named, and every cut piece composed exactly once.
    pub(crate) fn assert_whole(d: &Design, containers: &[u64]) {
        for s in &d.shapes {
            assert!(s.parent.is_none_or(|p| d.shapes.iter().any(|q| q.id == p)), "{} keeps a dangling parent", s.id);
            assert!(s.container.is_none() || containers.contains(&s.id), "{} is unexpectedly a container", s.id);
        }
        let mats = crate::composition::compose(d, MachineFamily::StudioPro.preferred_mat(), 1);
        let mut items: Vec<_> = mats.iter().flat_map(|m| &m.items).map(|i| i.piece).collect();
        let pieces = d.drawn().filter(|s| !s.hidden).map(|s| d.geometry(s).pieces().len()).sum::<usize>();
        assert_eq!(items.len(), pieces, "each cut piece once");
        items.sort_by_key(|p| (p.owner, format!("{:?}", p.local)));
        items.dedup();
        assert_eq!(items.len(), pieces, "no piece twice");
    }

    #[test]
    fn combine_slice_flatten_and_sticker_then_expand_consume_a_containers_children() {
        let edits = [
            Edit::Combine(Combine::Weld),
            Edit::Combine(Combine::Slice),
            Edit::Flatten,
            Edit::AddEffect { effect: Effect::Sticker(Sticker { border_in: 0.1, corners: Corners::Round }), reach: Reach::Together },
        ];
        for edit in edits {
            let mut d = holder();
            let mut made = d.apply(&edit, &[1, 4]).unwrap_or_else(|e| panic!("{edit:?}: {e:?}"));
            if d.shapes.iter().any(|s| made.contains(&s.id) && s.compound().is_some()) {
                assert_whole(&d, &[1]);
                made = d.apply(&Edit::Expand, &made).unwrap();
            }
            assert!(!d.shapes.iter().any(|s| [1, 2, 3, 4].contains(&s.id)), "{edit:?} consumes the children with their container");
            assert!(!made.is_empty(), "{edit:?}");
            assert_whole(&d, &[]);
        }
    }

    #[test]
    fn unflatten_restores_a_container_and_its_children_as_plain_shapes() {
        let mut d = holder();
        let flat = d.flatten(&[1]).unwrap();
        let ShapeKind::Flattened { parts, .. } = &d.shapes.iter().find(|s| s.id == flat).unwrap().kind else { unreachable!() };
        assert_eq!(parts.len(), 3, "the container and both children are parts");
        let restored = d.unflatten(flat);
        assert_eq!(restored.len(), 3);
        assert_whole(&d, &[]);
    }

    #[test]
    fn split_letters_take_the_texts_parent_and_slot_and_group() {
        let mut d = holder();
        let t = d.add_text("Hey".into(), FontRef::Inter, 0.5, egui::Color32::BLACK, 0.0, 0.0);
        d.reparent(&[t], Some(1));
        let letters = d.apply(&Edit::Text(TextChange::Split(Split::Letters)), &[t]).unwrap();
        assert_eq!(letters.len(), 3);
        for &id in &letters {
            let s = d.shapes.iter().find(|s| s.id == id).unwrap();
            assert_eq!((s.parent, s.container), (Some(1), None), "a letter sits where the text did, never a container");
        }
        d.unlink(&letters);
        assert!(d.group(&letters).is_some(), "the letters are siblings, so they group");
        assert_whole(&d, &[1]);
    }

    #[test]
    fn split_refuses_text_with_effects_and_container_text() {
        let mut d = Design::default();
        let t = d.add_text("Hey".into(), FontRef::Inter, 0.5, egui::Color32::BLACK, 0.0, 0.0);
        let split = Edit::Text(TextChange::Split(Split::Letters));
        d.shape_mut(t).unwrap().set_warp(Some(crate::warp::Warp { style: crate::warp::WarpStyle::Arch, bend: 0.3, vertical: false }));
        assert_eq!(d.check(&split, &[t]), Err(Blocked::SplitEffects));
        assert_eq!(Blocked::SplitEffects.reason(), "Expand or remove effects to split.");
        d.shape_mut(t).unwrap().set_warp(None);
        d.contain(&[t]).unwrap();
        assert_eq!(d.check(&split, &[t]), Err(Blocked::SplitContainer));
    }
}

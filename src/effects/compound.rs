//! Compounds (spec 21 phase 2): shapes whose children stay editable and draw through them. An effect
//! group draws its children as themselves under its own Offset and Sticker; a boolean group draws only
//! what combining them makes.

use std::collections::HashSet;
use std::rc::Rc;

use egui::Color32;

use super::eval::{Ink, Local, LocalPiece, LocalPieces, Paint, PieceId, Placement, image_corners, lines_of, ring_contours, run_stack, welded};
use super::model::{Effect, Effects, Style};
use crate::design::{Combine, CompoundOp, Contour, Design, MIN_PIECE_AREA_IN2, Operation, Pt, Seg, Shape, ShapeKind, shape_from_rings};
use crate::geometry::{Boolean, FLATTEN_TOL_IN, Ring};
use crate::links::LinkKind;

/// One shape a Compound evaluates over, with everything its share of the result depends on. Part of the
/// Compound's cache key: `local` compares by pointer, which is exact because the key holds the `Rc`.
pub(super) struct Member {
    pub id: u64,
    /// The Attach link of the direct child it came from: Slice cuts an attached set as one unit.
    pub link: Option<u64>,
    pub style: Style,
    /// Which direct child it came from, counting from the back.
    pub unit: usize,
    pub local: Rc<LocalPieces>,
    pub placement: Placement,
}

impl PartialEq for Member {
    fn eq(&self, other: &Member) -> bool {
        self.id == other.id
            && self.link == other.link
            && self.style == other.style
            && self.unit == other.unit
            && Rc::ptr_eq(&self.local, &other.local)
            && self.placement == other.placement
    }
}

/// What the children of `parent` among `scope` evaluate as, back to front, with each one's unit and its
/// direct child's Attach link: a Compound's members, or with `None` an instance's top-level parts. A
/// nested Compound is one member, since its geometry covers its subtree; a container contributes itself
/// and its children. Hidden shapes are skipped, a hidden Compound with its subtree.
pub(super) fn members(scope: &[Shape], parent: Option<u64>) -> Vec<(&Shape, usize, Option<u64>)> {
    fn collect<'a>(scope: &'a [Shape], s: &'a Shape, unit: usize, link: Option<u64>, out: &mut Vec<(&'a Shape, usize, Option<u64>)>) {
        if !s.hidden {
            out.push((s, unit, link));
        }
        if s.compound().is_none() {
            for k in scope.iter().filter(|k| k.parent == Some(s.id)) {
                collect(scope, k, unit, link, out);
            }
        }
    }
    let mut out = Vec::new();
    for (unit, c) in scope.iter().filter(|s| s.parent == parent).enumerate() {
        let link = c.link.filter(|l| l.kind == LinkKind::Attach).map(|l| l.id);
        collect(scope, c, unit, link, &mut out);
    }
    out
}

impl Design {
    /// Shapes with no Compound above them: what is painted, hit at the top level, composed and exported.
    pub fn drawn(&self) -> impl DoubleEndedIterator<Item = &Shape> {
        let compounds: Vec<u64> = self.shapes.iter().filter(|s| s.compound().is_some()).map(|s| s.id).collect();
        let inside: HashSet<u64> = match compounds.is_empty() {
            true => HashSet::new(),
            false => {
                let children: Vec<u64> = self.shapes.iter().filter(|s| s.parent.is_some_and(|p| compounds.contains(&p))).map(|s| s.id).collect();
                self.with_descendants(&children).into_iter().collect()
            }
        };
        self.shapes.iter().filter(move |s| !inside.contains(&s.id))
    }

    /// The front-most shape under `p`, looking inside groups: each drawn shape's visible children, front
    /// first, are tried before the shape itself, so a child inside a Subtract hole is still found.
    pub fn hit_test_deep(&self, p: Pt) -> Option<u64> {
        fn deep(d: &Design, s: &Shape, p: Pt) -> Option<u64> {
            let kids: Vec<&Shape> = d.children(Some(s.id)).filter(|k| !k.hidden).collect();
            kids.into_iter().rev().find_map(|k| deep(d, k, p)).or_else(|| d.geometry(s).contains(p).then_some(s.id))
        }
        self.drawn().rev().find_map(|s| deep(self, s, p))
    }
}

/// `compound` evaluated over `members` in its own local frame, then through its own Offset and Sticker
/// entries. Warp entries are never applied to a Compound.
pub(super) fn evaluate_compound(compound: &Shape, op: CompoundOp, members: &[Member]) -> LocalPieces {
    let frame = Placement::of(compound);
    let moved: Vec<(&Member, Vec<LocalPiece>)> = members.iter().map(|m| (m, m.local.pieces.iter().map(|p| into_frame(p, m, frame)).collect())).collect();
    let style = Style::of(compound);
    let mut pieces = match op {
        CompoundOp::Effects => drawn_as_themselves(moved),
        CompoundOp::Boolean(Combine::Unite) => {
            let contours = moved.iter().flat_map(|(_, ps)| ps).flat_map(|p| &p.contours).filter(|c| c.closed).cloned().collect();
            body(contours, style)
        }
        CompoundOp::Boolean(Combine::Slice) => {
            let units = grouped(&moved, |m| m.link.map_or(Unit::Child(m.unit), Unit::Link));
            match units[..] {
                [ref back, ref front] => sliced(back, front),
                _ => drawn_as_themselves(moved),
            }
        }
        CompoundOp::Boolean(other) => {
            let boolean = match other {
                Combine::Weld => Boolean::Weld,
                Combine::Subtract => Boolean::Subtract,
                Combine::Intersect => Boolean::Intersect,
                Combine::Exclude => Boolean::Exclude,
                Combine::Unite | Combine::Slice => unreachable!("matched above"),
            };
            let operands: Vec<Vec<Ring>> = grouped(&moved, |m| Unit::Child(m.unit)).into_iter().map(|u| welded(u.pieces)).collect();
            body(ring_contours(crate::geometry::boolean(&operands, boolean)), style)
        }
    };
    run_stack(&mut pieces, compound, compound.effects.enabled().filter(|e| e.effect.makes_pieces()));
    LocalPieces { pieces, grab: false }
}

#[derive(PartialEq)]
enum Unit {
    Child(usize),
    Link(u64),
}

/// The moved pieces of one unit, with the style of its back-most member.
struct UnitPieces<'a> {
    style: Style,
    pieces: Vec<&'a LocalPiece>,
}

/// `moved` in units by `key`, in order of each unit's back-most member.
fn grouped<'a>(moved: &'a [(&Member, Vec<LocalPiece>)], key: impl Fn(&Member) -> Unit) -> Vec<UnitPieces<'a>> {
    let mut units: Vec<(Unit, UnitPieces)> = Vec::new();
    for (m, pieces) in moved {
        let k = key(m);
        match units.iter_mut().find(|(u, _)| *u == k) {
            Some((_, u)) => u.pieces.extend(pieces),
            None => units.push((k, UnitPieces { style: m.style, pieces: pieces.iter().collect() })),
        }
    }
    units.into_iter().map(|(_, u)| u).collect()
}

fn drawn_as_themselves(moved: Vec<(&Member, Vec<LocalPiece>)>) -> Vec<LocalPiece> {
    moved.into_iter().flat_map(|(_, pieces)| pieces).collect()
}

fn body(contours: Vec<Contour>, style: Style) -> Vec<LocalPiece> {
    match contours.is_empty() {
        true => Vec::new(),
        false => vec![LocalPiece { owner: None, local: Local::Body, style, contours, paint: Paint::Fill }],
    }
}

/// Back minus front, front minus back and their overlap, in the back's, the front's and the front's
/// styles, as Slice always cut them. Slivers under `MIN_PIECE_AREA_IN2` are boolean noise.
fn sliced(back: &UnitPieces, front: &UnitPieces) -> Vec<LocalPiece> {
    let parts = crate::geometry::slice(&welded(back.pieces.iter().copied()), &welded(front.pieces.iter().copied()));
    let styles = [back.style, front.style, front.style];
    (0u8..)
        .zip(parts.into_iter().zip(styles))
        .filter(|(_, (rings, _))| crate::geometry::even_odd_area(rings) > MIN_PIECE_AREA_IN2)
        .map(|(n, (rings, style))| LocalPiece { owner: None, local: Local::Slice(n), style, contours: ring_contours(rings), paint: Paint::Fill })
        .collect()
}

/// `piece` from `member`'s local frame into the Compound's, keeping who made it.
fn into_frame(piece: &LocalPiece, member: &Member, frame: Placement) -> LocalPiece {
    let place = member.placement.placer();
    let to = |p: Pt| frame.to_local(place(p));
    let contours: Vec<Contour> = piece
        .contours
        .iter()
        .map(|c| Contour {
            start: to(c.start),
            segs: c
                .segs
                .iter()
                .map(|s| match *s {
                    Seg::Line(p) => Seg::Line(to(p)),
                    Seg::Cubic(a, b, p) => Seg::Cubic(to(a), to(b), to(p)),
                })
                .collect(),
            closed: c.closed,
        })
        .collect();
    let paint = match &piece.paint {
        Paint::Print(layers) => Paint::Print(
            layers.iter().map(|(lines, ink)| (lines.iter().map(|(pts, closed)| (pts.iter().map(|&p| to(p)).collect(), *closed)).collect(), ink.mapped(to))).collect(),
        ),
        // The group's frame is not the image's, so its pixels travel as print artwork pinned at their own corners.
        Paint::Image(asset) => {
            let corners = image_corners((member.placement.half.0 * 2.0, member.placement.half.1 * 2.0), to);
            Paint::Print(vec![(lines_of(&contours), Ink::Image { asset: asset.clone(), corners, repeat: false })])
        }
        Paint::Fill => Paint::Fill,
    };
    LocalPiece { owner: Some(piece.owner.unwrap_or(member.id)), local: piece.local, style: piece.style, contours, paint }
}

/// What Expand has made of a Compound so far, back to front.
enum Item {
    /// A new plain shape.
    Made(Box<Shape>),
    /// A child released with its subtree.
    Kept(u64),
}

impl Design {
    /// Wraps the roots of `ids` (two or more) in a new Compound at the back-most root's z slot, under its
    /// parent and in its fill, operation and pen. A link the roots share with shapes left outside loses
    /// them; a link wholly inside moves in as one. Returns the Compound's id.
    pub fn wrap(&mut self, ids: &[u64], op: CompoundOp, effects: Effects) -> Option<u64> {
        let roots = self.roots(ids);
        if roots.len() < 2 {
            return None;
        }
        let at = self.shapes.iter().position(|s| s.id == roots[0])?;
        let back = &self.shapes[at];
        let id = self.next_id();
        let group = Shape {
            x_in: back.x_in,
            y_in: back.y_in,
            width_in: back.width_in,
            height_in: back.height_in,
            fill: back.fill,
            operation: back.operation,
            pen: back.pen,
            parent: back.parent,
            effects,
            ..Shape::new(id, ShapeKind::Compound(op))
        };
        let outside = |l: u64| self.shapes.iter().any(|s| s.link.is_some_and(|k| k.id == l) && !roots.contains(&s.id));
        let straddling: Vec<u64> = self.shapes.iter().filter(|s| roots.contains(&s.id)).filter_map(|s| s.link).map(|l| l.id).filter(|&l| outside(l)).collect();
        for s in self.shapes.iter_mut().filter(|s| roots.contains(&s.id)) {
            s.parent = Some(id);
            if s.link.is_some_and(|l| straddling.contains(&l.id)) {
                s.link = None;
            }
        }
        self.shapes.insert(at, group);
        self.prune_links();
        self.settle(None);
        Some(id)
    }

    /// Expand on the Compound `id`, one level: a boolean group's result becomes plain shapes as Combine
    /// and Slice used to make them, consuming its children; an effect group releases its children with
    /// their subtrees, stacks and links. Then each of its own enabled entries bakes as on a single shape:
    /// an Offset into a plain shape behind (an inset in front), a Sticker into a Flattened shape holding everything so far.
    /// The front-most new shape takes the Compound's id and link. Returns the ids the user holds.
    pub(crate) fn expand_compound(&mut self, id: u64) -> Vec<u64> {
        let Some(g) = self.shapes.iter().find(|s| s.id == id).cloned() else { return Vec::new() };
        let mut fresh = self.next_id()..;
        let mut next = move || fresh.next().unwrap_or_default();
        let made = |id: u64, rings: Vec<Ring>, style: Style| shape_from_rings(id, rings, style.fill, style.operation).map(|s| Shape { pen: style.pen, ..s });
        let releases = self.draws_members(&g);
        let mut items: Vec<Item> = match releases {
            true => self.children(Some(id)).map(|c| Item::Kept(c.id)).collect(),
            false => {
                let inner = self.geometry(&Shape { effects: g.effects.cleared(), ..g.clone() });
                inner.pieces().iter().filter_map(|p| made(next(), inner.piece_rings(p, FLATTEN_TOL_IN), p.style)).map(|s| Item::Made(Box::new(s))).collect()
            }
        };
        let mut consumed = vec![id];
        let geometry = self.geometry(&g);
        for entry in g.effects.enabled() {
            let Some(piece) = geometry.piece(PieceId { owner: id, local: Local::Effect(entry.id) }) else { continue };
            let rings = geometry.piece_rings(piece, FLATTEN_TOL_IN);
            match entry.effect {
                Effect::Offset(o) => {
                    let shape = made(next(), rings, piece.style).map(|s| Item::Made(Box::new(s)));
                    match o.direction {
                        super::Direction::Outset => items.splice(0..0, shape).for_each(drop),
                        super::Direction::Inset => items.extend(shape),
                    }
                }
                Effect::Sticker(_) => {
                    let white = Style { fill: Color32::WHITE, operation: Operation::PrintThenCut, pen: g.pen };
                    let Some(backing) = made(next(), rings, white) else { continue };
                    let mut parts = vec![backing];
                    for item in items.drain(..) {
                        match item {
                            Item::Made(s) => parts.push(*s),
                            Item::Kept(k) => {
                                parts.extend(self.subtree_parts(&[k]).into_iter().map(|s| Shape { link: None, ..s }));
                                consumed.push(k);
                            }
                        }
                    }
                    let sticker = crate::flatten::flattened(next(), parts).map(|f| Shape { fill: g.fill, pen: g.pen, ..f });
                    items.extend(sticker.map(|s| Item::Made(Box::new(s))));
                }
                Effect::Warp(_) => {}
            }
        }
        if let Some(Item::Made(front)) = items.iter_mut().rev().find(|i| matches!(i, Item::Made(_))) {
            (front.id, front.link) = (id, g.link);
        }
        let laid_out = g.parent.is_some_and(|p| self.shapes.iter().any(|s| s.id == p && s.container.is_some_and(|c| c.layout.is_some())));
        let mut out: Vec<Shape> = Vec::new();
        let mut held = Vec::new();
        for item in items {
            match item {
                Item::Made(s) => {
                    held.push(s.id);
                    // A ring parented into a laid-out container would be laid out as one more item.
                    out.push(Shape { parent: if s.id == id || !laid_out { g.parent } else { None }, ..*s });
                }
                Item::Kept(k) => {
                    held.push(k);
                    let tree = self.with_descendants(&[k]);
                    out.extend(self.shapes.iter().filter(|s| tree.contains(&s.id)).map(|s| Shape { parent: if s.id == k { g.parent } else { s.parent }, ..s.clone() }));
                }
            }
        }
        let gone = self.with_descendants(&consumed);
        let at = self.shapes.iter().position(|s| s.id == id).unwrap_or(self.shapes.len());
        self.shapes.retain(|s| !gone.contains(&s.id) && !out.iter().any(|o| o.id == s.id));
        let at = at.min(self.shapes.len());
        self.shapes.splice(at..at, out);
        self.prune_links();
        held
    }

    /// Whether `g` draws its children as themselves: an effect group, or a Slice group that does not
    /// have exactly two units to cut.
    fn draws_members(&self, g: &Shape) -> bool {
        match g.compound() {
            Some(CompoundOp::Boolean(Combine::Slice)) => {
                let mut units: Vec<Unit> = Vec::new();
                for (_, unit, link) in members(&self.shapes, Some(g.id)) {
                    let u = link.map_or(Unit::Child(unit), Unit::Link);
                    if !units.contains(&u) {
                        units.push(u);
                    }
                }
                units.len() != 2
            }
            Some(CompoundOp::Boolean(_)) => false,
            Some(CompoundOp::Effects) | None => true,
        }
    }
}

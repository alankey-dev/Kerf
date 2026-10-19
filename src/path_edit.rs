//! Node editing for drawn paths. A path is read as contours of nodes, each an anchor with optional
//! absolute in and out control points and a [`NodeKind`] that decides how its handles move. Every edit
//! runs on a [`NodePath`] in the shape's local frame, and `write` puts it back so untouched nodes keep
//! their place on the canvas and the kinds are stored with the geometry.
//!
//! Stored kinds are hints: `read` keeps one only if its contour has the same node count and the
//! geometry still satisfies it, and infers the rest. Old projects, imports and boolean results carry no
//! kinds and so are inferred on first read.

use std::collections::BTreeSet;

use egui::Color32;

use crate::design::{Contour, Design, Operation, Pt, Seg, Shape, ShapeKind};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// No handles: both neighbouring segments meet this node as lines.
    Straight,
    /// Independent handles.
    Cusp,
    /// Collinear handles of any lengths.
    Smooth,
    /// Collinear handles of equal length.
    Symmetric,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    pub pt: Pt,
    pub inn: Option<Pt>,
    pub out: Option<Pt>,
    pub kind: NodeKind,
    /// The live corner (spec 55) that rounds, chamfers or scoops this node when the outline is
    /// evaluated. Unset by default; it travels with the node through every edit, so inserting or
    /// removing nodes keeps the rest in step.
    pub corner: crate::corners::Corner,
}

/// A contour as nodes. A closed contour stores its closing node once.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeContour {
    pub nodes: Vec<Node>,
    pub closed: bool,
}

/// Every contour of a path shape as nodes, in the shape's local frame with any warp baked in.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePath {
    pub contours: Vec<NodeContour>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub contour: usize,
    pub node: usize,
}

pub type Selection = BTreeSet<NodeId>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Anchor,
    Handle(Side),
}

/// One grabbable point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: NodeId,
    pub part: Part,
}

/// A point on segment `seg` of a contour, which runs from node `seg` to node `seg + 1` (wrapping to
/// node 0 on a closed contour). `pt` is in the local frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegHit {
    pub contour: usize,
    pub seg: usize,
    pub t: f32,
    pub pt: Pt,
}

/// Everything the node toolbar and the node context menu can do to a selection of nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeAction {
    Convert(NodeKind),
    /// A node at the middle of each segment whose two ends are both selected.
    AddMidpoints,
    Remove,
    /// Splits a closed contour at the first selected node on it.
    OpenAt,
    /// Joins the two ends of each open contour the selection touches (every open contour when nothing
    /// is selected) with a line.
    Close,
    /// Strength 0..=100. With nodes selected, recomputes their handles from their neighbours; with none,
    /// simplifies and refits the whole path.
    Smooth(u8),
}

/// Why an action cannot run; the text fills the disabled control's tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    NothingSelected,
    TooFewNodes { min: usize },
    NoClosedContour,
    NoOpenContour,
    NoAdjacentPair,
}

impl Blocked {
    pub fn reason(self) -> String {
        match self {
            Self::NothingSelected => "Select one or more nodes first.".into(),
            Self::TooFewNodes { min } => format!("A path needs at least {min} nodes."),
            Self::NoClosedContour => "Select a node on a closed path.".into(),
            Self::NoOpenContour => "The path is already closed.".into(),
            Self::NoAdjacentPair => "Select two neighbouring nodes.".into(),
        }
    }
}

const SAME_PT: f32 = 1e-4;
/// Samples per original segment when refitting a removed span.
const FIT_SAMPLES: usize = 32;
/// Samples per segment before `nearest` refines the closest one.
const COARSE: usize = 24;
/// How finely a whole-path Smooth flattens before simplifying.
const SMOOTH_FLATTEN_IN: f32 = 0.001;
const SMOOTH_CHECK_IN: f32 = 0.01;

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn mul(a: Pt, k: f32) -> Pt {
    (a.0 * k, a.1 * k)
}

fn len(a: Pt) -> f32 {
    a.0.hypot(a.1)
}

fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn unit(a: Pt) -> Option<Pt> {
    let l = len(a);
    (l > SAME_PT).then(|| (a.0 / l, a.1 / l))
}

fn near(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() < SAME_PT && (a.1 - b.1).abs() < SAME_PT
}

/// A handle, or `None` when it lies on its anchor.
fn handle(h: Pt, anchor: Pt) -> Option<Pt> {
    (!near(h, anchor)).then_some(h)
}

fn bez(b: &[Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    let (k0, k1, k2, k3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (k0 * b[0].0 + k1 * b[1].0 + k2 * b[2].0 + k3 * b[3].0, k0 * b[0].1 + k1 * b[1].1 + k2 * b[2].1 + k3 * b[3].1)
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Cusp => "Cusp",
            Self::Smooth => "Smooth",
            Self::Symmetric => "Symmetric",
        }
    }

    /// Whether `n`'s handles satisfy this kind. Straight: no handles. Smooth: the two handles point in
    /// opposite directions (within about 3 degrees). Symmetric: also equal lengths (within 1 %). With
    /// fewer than two handles Smooth and Symmetric hold vacuously. Cusp always holds.
    pub fn holds(self, n: &Node) -> bool {
        let (Some(i), Some(o)) = (n.inn, n.out) else {
            return self != Self::Straight || (n.inn.is_none() && n.out.is_none());
        };
        let (a, b) = (sub(i, n.pt), sub(o, n.pt));
        let (la, lb) = (len(a), len(b));
        let opposite = la <= SAME_PT || lb <= SAME_PT || (a.0 * b.0 + a.1 * b.1) / (la * lb) < -0.998;
        match self {
            Self::Straight => false,
            Self::Cusp => true,
            Self::Smooth => opposite,
            Self::Symmetric => opposite && (la - lb).abs() <= 0.01 * la.max(lb),
        }
    }
}

impl Node {
    pub fn new(pt: Pt, kind: NodeKind) -> Self {
        Self { pt, inn: None, out: None, kind, corner: crate::corners::Corner::default() }
    }

    /// No handles: Straight. One handle: Cusp. Collinear and equal: Symmetric. Collinear: Smooth. Else Cusp.
    pub fn infer(&self) -> NodeKind {
        match (self.inn, self.out) {
            (None, None) => NodeKind::Straight,
            (Some(_), Some(_)) => [NodeKind::Symmetric, NodeKind::Smooth].into_iter().find(|k| k.holds(self)).unwrap_or(NodeKind::Cusp),
            _ => NodeKind::Cusp,
        }
    }

    /// Moves one handle to `to` and enforces the kind on its twin. Cusp leaves the twin. Smooth swings the
    /// twin opposite, keeping its length. Symmetric sets the twin to `2 * pt - to`, creating it if
    /// absent. A Straight node becomes Cusp first.
    pub fn move_handle(&mut self, side: Side, to: Pt) {
        if self.kind == NodeKind::Straight {
            self.kind = NodeKind::Cusp;
        }
        let pt = self.pt;
        let (grabbed, twin) = match side {
            Side::In => (&mut self.inn, &mut self.out),
            Side::Out => (&mut self.out, &mut self.inn),
        };
        *grabbed = Some(to);
        match self.kind {
            NodeKind::Smooth => {
                if let (Some(t), Some(dir)) = (*twin, unit(sub(pt, to))) {
                    *twin = Some(add(pt, mul(dir, len(sub(t, pt)))));
                }
            }
            NodeKind::Symmetric => *twin = Some(sub(mul(pt, 2.0), to)),
            NodeKind::Straight | NodeKind::Cusp => {}
        }
    }

    /// Re-infers the kind if an edit broke it.
    fn settle(&mut self) {
        if !self.kind.holds(self) {
            self.kind = self.infer();
        }
    }

    /// Turns both handles onto the bisector of their directions, keeping their lengths or, for
    /// `equal`, their mean.
    fn align(&mut self, equal: bool) {
        let (Some(i), Some(o)) = (self.inn, self.out) else { return };
        let (vi, vo) = (sub(i, self.pt), sub(o, self.pt));
        let Some(t) = unit(sub(unit(vo).unwrap_or_default(), unit(vi).unwrap_or_default())) else { return };
        let (li, lo) = if equal { ((len(vi) + len(vo)) / 2.0, (len(vi) + len(vo)) / 2.0) } else { (len(vi), len(vo)) };
        self.inn = Some(sub(self.pt, mul(t, li)));
        self.out = Some(add(self.pt, mul(t, lo)));
    }

    /// The direction a refit must leave this node in on `side`, when its kind pins one.
    fn held_tangent(&self, side: Side) -> Option<Pt> {
        if !matches!(self.kind, NodeKind::Smooth | NodeKind::Symmetric) {
            return None;
        }
        let (own, twin) = match side {
            Side::In => (self.inn, self.out),
            Side::Out => (self.out, self.inn),
        };
        own.and_then(|h| unit(sub(h, self.pt))).or_else(|| twin.and_then(|h| unit(sub(self.pt, h))))
    }
}

impl NodeContour {
    /// Handles lying on their anchor become `None`. `stored` is used only if its length matches the node
    /// count, and each stored kind only if it `holds`; everything else is inferred.
    pub fn from_contour(c: &Contour, stored: Option<&[NodeKind]>) -> Self {
        let mut nodes = vec![Node::new(c.start, NodeKind::Straight)];
        for seg in &c.segs {
            let (out, inn, end) = match *seg {
                Seg::Line(p) => (None, None, p),
                Seg::Cubic(c1, c2, p) => (Some(c1), Some(c2), p),
            };
            let last = nodes.last_mut().unwrap();
            last.out = out.and_then(|h| handle(h, last.pt));
            nodes.push(Node { pt: end, inn: inn.and_then(|h| handle(h, end)), out: None, kind: NodeKind::Straight, corner: crate::corners::Corner::default() });
        }
        if c.closed && nodes.len() > 2 && near(nodes[0].pt, nodes.last().unwrap().pt) {
            let last = nodes.pop().unwrap();
            nodes[0].inn = last.inn;
        }
        let stored = stored.filter(|k| k.len() == nodes.len());
        for (i, n) in nodes.iter_mut().enumerate() {
            n.kind = match stored.map(|k| k[i]) {
                Some(k) if k.holds(n) => k,
                _ => n.infer(),
            };
        }
        Self { nodes, closed: c.closed }
    }

    pub fn to_contour(&self) -> Contour {
        let seg = |a: &Node, b: &Node| match (a.out, b.inn) {
            (None, None) => Seg::Line(b.pt),
            (o, i) => Seg::Cubic(o.unwrap_or(a.pt), i.unwrap_or(b.pt), b.pt),
        };
        let mut segs: Vec<Seg> = self.nodes.windows(2).map(|w| seg(&w[0], &w[1])).collect();
        if self.closed && self.nodes.len() > 1 {
            segs.push(seg(self.nodes.last().unwrap(), &self.nodes[0]));
        }
        Contour { start: self.nodes.first().map_or((0.0, 0.0), |n| n.pt), segs, closed: self.closed }
    }

    pub fn kinds(&self) -> Vec<NodeKind> {
        self.nodes.iter().map(|n| n.kind).collect()
    }

    /// The live corner of every node, in step with [`kinds`](Self::kinds).
    pub fn corners(&self) -> Vec<crate::corners::Corner> {
        self.nodes.iter().map(|n| n.corner).collect()
    }

    /// The fewest nodes the contour keeps: a line, or a closed triangle-like loop.
    pub fn min_nodes(&self) -> usize {
        if self.closed { 3 } else { 2 }
    }

    /// A Catmull-Rom curve through `pts` (wrapping when closed). Interior nodes are Smooth; the two ends
    /// of an open curve are Cusp.
    pub fn catmull_rom(pts: &[Pt], closed: bool) -> Self {
        let mut c = Self { nodes: pts.iter().map(|&p| Node::new(p, NodeKind::Cusp)).collect(), closed };
        for i in 0..pts.len() {
            if let (Some(p), Some(q)) = c.neighbours(i) {
                let (inn, out) = catmull_rom_handles(pts[p], pts[i], pts[q]);
                c.nodes[i] = Node { pt: pts[i], inn: handle(inn, pts[i]), out: handle(out, pts[i]), kind: NodeKind::Smooth, corner: crate::corners::Corner::default() };
            }
        }
        c
    }

    fn seg_count(&self) -> usize {
        match self.nodes.len() {
            0 | 1 => 0,
            n if self.closed => n,
            n => n - 1,
        }
    }

    /// Segment `i`'s control points, with a line's at its thirds so `t` runs evenly along it, and
    /// whether it is a line.
    fn bezier(&self, i: usize) -> ([Pt; 4], bool) {
        let (a, b) = (&self.nodes[i], &self.nodes[(i + 1) % self.nodes.len()]);
        match (a.out, b.inn) {
            (None, None) => ([a.pt, lerp(a.pt, b.pt, 1.0 / 3.0), lerp(a.pt, b.pt, 2.0 / 3.0), b.pt], true),
            (o, i) => ([a.pt, o.unwrap_or(a.pt), i.unwrap_or(b.pt), b.pt], false),
        }
    }

    fn neighbours(&self, i: usize) -> (Option<usize>, Option<usize>) {
        let n = self.nodes.len();
        let wrap = self.closed && n > 1;
        let prev = if i > 0 { Some(i - 1) } else { wrap.then(|| n - 1) };
        let next = if i + 1 < n { Some(i + 1) } else { wrap.then_some(0) };
        (prev, next)
    }

    /// Catmull-Rom handles for node `i`; an open end gets one handle a third of the way to its neighbour.
    fn fresh_handles(&self, i: usize) -> (Option<Pt>, Option<Pt>) {
        let pt = self.nodes[i].pt;
        match self.neighbours(i) {
            (Some(p), Some(q)) => {
                let (inn, out) = catmull_rom_handles(self.nodes[p].pt, pt, self.nodes[q].pt);
                (handle(inn, pt), handle(out, pt))
            }
            (Some(p), None) => (handle(lerp(pt, self.nodes[p].pt, 1.0 / 3.0), pt), None),
            (None, Some(q)) => (None, handle(lerp(pt, self.nodes[q].pt, 1.0 / 3.0), pt)),
            (None, None) => (None, None),
        }
    }

    fn convert(&mut self, i: usize, kind: NodeKind) {
        let (prev, next) = self.neighbours(i);
        // A corner belongs to a Straight or Cusp node; converting to Smooth or Symmetric drops it.
        let keep = match kind {
            NodeKind::Smooth | NodeKind::Symmetric => crate::corners::Corner::default(),
            _ => self.nodes[i].corner,
        };
        if kind == NodeKind::Straight {
            self.nodes[i] = Node::new(self.nodes[i].pt, kind);
            self.nodes[i].corner = keep;
            if let Some(p) = prev {
                self.nodes[p].out = None;
                self.nodes[p].settle();
            }
            if let Some(q) = next {
                self.nodes[q].inn = None;
                self.nodes[q].settle();
            }
            return;
        }
        let fresh = self.fresh_handles(i);
        let n = &mut self.nodes[i];
        if n.kind == NodeKind::Straight || (n.inn.is_none() && n.out.is_none()) {
            (n.inn, n.out) = fresh;
        } else if kind != NodeKind::Cusp {
            (n.inn, n.out) = (n.inn.or(fresh.0), n.out.or(fresh.1));
        }
        n.kind = kind;
        n.corner = keep;
        if kind != NodeKind::Cusp {
            n.align(kind == NodeKind::Symmetric);
        }
        n.settle();
    }

    fn smooth_node(&mut self, i: usize, strength: f32) {
        let fresh = self.fresh_handles(i);
        let n = &mut self.nodes[i];
        let pt = n.pt;
        let blend = |cur: Option<Pt>, fresh: Option<Pt>| match fresh {
            Some(f) => handle(lerp(cur.unwrap_or(pt), f, strength), pt),
            None => cur,
        };
        (n.inn, n.out) = (blend(n.inn, fresh.0), blend(n.out, fresh.1));
        n.kind = NodeKind::Smooth;
        n.align(false);
        n.settle();
    }

    /// Catmull-Rom curves overshoot sparse points, so the simplification tightens until the refitted
    /// curve stays within `tol` of the original.
    fn smooth_whole(&mut self, tol: f32) {
        let pts = crate::geometry::flatten(&self.to_contour(), SMOOTH_FLATTEN_IN);
        if pts.len() < if self.closed { 3 } else { 2 } {
            return;
        }
        let simplify = |eps: f32| {
            if !self.closed {
                return crate::geometry::rdp(&pts, eps);
            }
            let mut ring = pts.clone();
            ring.push(pts[0]);
            let mut r = crate::geometry::rdp(&ring, eps);
            r.pop();
            if r.len() < 3 { vec![pts[0], pts[pts.len() / 3], pts[2 * pts.len() / 3]] } else { r }
        };
        let mut eps = tol;
        let fitted = loop {
            let fit = Self::catmull_rom(&simplify(eps), self.closed);
            let stray = crate::geometry::flatten(&fit.to_contour(), SMOOTH_CHECK_IN).iter().map(|&p| dist_to_polyline(p, &pts, self.closed)).fold(0.0, f32::max);
            if stray <= tol || eps < SMOOTH_FLATTEN_IN {
                break fit;
            }
            eps *= 0.7;
        };
        *self = fitted;
    }

    fn remove(&mut self, gone: &BTreeSet<usize>) {
        let n = self.nodes.len();
        let keep: Vec<usize> = (0..n).filter(|i| !gone.contains(i)).collect();
        let mut nodes: Vec<Node> = keep.iter().map(|&i| self.nodes[i]).collect();
        let pairs = if self.closed { keep.len() } else { keep.len().saturating_sub(1) };
        for k in 0..pairs {
            let (a, b) = (keep[k], keep[(k + 1) % keep.len()]);
            let span: Vec<([Pt; 4], bool)> = (0..(b + n - a) % n).map(|j| self.bezier((a + j) % n)).collect();
            if span.len() < 2 {
                continue;
            }
            let (na, nb) = (k, (k + 1) % nodes.len());
            if span.iter().all(|(_, line)| *line) {
                nodes[na].out = None;
                nodes[nb].inn = None;
                continue;
            }
            let mut samples = vec![span[0].0[0]];
            for (b, _) in &span {
                samples.extend((1..=FIT_SAMPLES).map(|j| bez(b, j as f32 / FIT_SAMPLES as f32)));
            }
            let (c1, c2) = fit_cubic(&samples, self.nodes[a].held_tangent(Side::Out), self.nodes[b].held_tangent(Side::In));
            nodes[na].out = handle(c1, nodes[na].pt);
            nodes[nb].inn = handle(c2, nodes[nb].pt);
        }
        if !self.closed {
            if let Some(first) = nodes.first_mut() {
                first.inn = None;
            }
            if let Some(last) = nodes.last_mut() {
                last.out = None;
            }
        }
        for node in &mut nodes {
            node.settle();
        }
        self.nodes = nodes;
    }
}

impl NodePath {
    /// The shape's contours as nodes, with its stored kinds checked against the geometry and its stored
    /// corners attached to the matching nodes.
    pub fn read(shape: &Shape) -> Self {
        let (stored, stored_corners): (&[Vec<NodeKind>], &[Vec<crate::corners::Corner>]) = match &shape.kind {
            ShapeKind::Path { node_kinds, corners, .. } => (node_kinds, corners),
            _ => (&[], &[]),
        };
        let mut contours: Vec<NodeContour> =
            crate::effects::body_contours(shape).iter().enumerate().map(|(i, c)| NodeContour::from_contour(c, stored.get(i).map(Vec::as_slice))).collect();
        for (i, nc) in contours.iter_mut().enumerate() {
            if let Some(cs) = stored_corners.get(i) {
                for (node, corner) in nc.nodes.iter_mut().zip(cs) {
                    node.corner = *corner;
                }
            }
        }
        Self { contours }
    }

    /// The edited shape: bounds refitted so untouched nodes keep their world position under any rotation
    /// or flip, kinds stored, and the leading warp baked in (it was applied on `read`). Later Offset and
    /// Sticker entries stay live. Hidden contours are kept while the contour count holds, else cleared.
    pub fn write(&self, original: &Shape) -> Shape {
        let contours: Vec<Contour> = self.contours.iter().map(NodeContour::to_contour).collect();
        let pts: Vec<Pt> = contours.iter().flat_map(|c| c.flatten()).collect();
        let (min_x, min_y) = pts.iter().fold((f32::MAX, f32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
        let (max_x, max_y) = pts.iter().fold((f32::MIN, f32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
        let (w, h) = ((max_x - min_x).max(0.01), (max_y - min_y).max(0.01));
        let (cx, cy) = original.to_world((min_x + w / 2.0, min_y + h / 2.0));
        let off = |(x, y): Pt| (x - min_x, y - min_y);
        let contours = contours
            .into_iter()
            .map(|c| Contour {
                start: off(c.start),
                segs: c
                    .segs
                    .iter()
                    .map(|s| match *s {
                        Seg::Line(p) => Seg::Line(off(p)),
                        Seg::Cubic(a, b, p) => Seg::Cubic(off(a), off(b), off(p)),
                    })
                    .collect(),
                closed: c.closed,
            })
            .collect();
        let mut shape = original.clone();
        if !matches!(original.kind, ShapeKind::Path { .. }) {
            shape.name.get_or_insert_with(|| original.kind.label().to_owned());
        }
        let corners: Vec<Vec<crate::corners::Corner>> = self.contours.iter().map(NodeContour::corners).collect();
        let corners = if crate::corners::all_clear(&corners) { Vec::new() } else { corners };
        shape.kind = ShapeKind::Path { contours, view_box: (w, h), node_kinds: self.contours.iter().map(NodeContour::kinds).collect(), corners };
        shape.width_in = w;
        shape.height_in = h;
        shape.x_in = cx - w / 2.0;
        shape.y_in = cy - h / 2.0;
        shape.effects.take_leading_warp();
        if self.contours.len() != crate::effects::body_contours(original).len() {
            shape.contours = Default::default();
        }
        shape
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.contours.get(id.contour)?.nodes.get(id.node)
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.contours.get_mut(id.contour)?.nodes.get_mut(id.node)
    }

    pub fn all(&self) -> Selection {
        self.contours.iter().enumerate().flat_map(|(c, nc)| (0..nc.nodes.len()).map(move |n| NodeId { contour: c, node: n })).collect()
    }

    /// Whether a live corner (spec 55) can sit on `id`: a Straight or Cusp node that is not the loose end
    /// of an open contour, so two edges actually meet there.
    pub fn corner_eligible(&self, id: NodeId) -> bool {
        let Some(nc) = self.contours.get(id.contour) else { return false };
        let endpoint = !nc.closed && (id.node == 0 || id.node + 1 == nc.nodes.len());
        nc.nodes.get(id.node).is_some_and(|n| matches!(n.kind, NodeKind::Straight | NodeKind::Cusp)) && !endpoint
    }

    /// The eligible nodes among `sel`, for the Corner toolbar's read-out and enabling.
    pub fn eligible(&self, sel: &Selection) -> Vec<NodeId> {
        sel.iter().copied().filter(|id| self.corner_eligible(*id)).collect()
    }

    /// Applies one [`crate::corners::CornerOp`] to the nodes' live corners. Only eligible nodes change.
    pub fn set_corners(&mut self, op: &crate::corners::CornerOp) {
        use crate::corners::{Corner, CornerOp};
        match op {
            CornerOp::Set { nodes, radius, style } => {
                let ids: Vec<NodeId> = nodes.iter().copied().filter(|id| self.corner_eligible(*id)).collect();
                for id in ids {
                    if let Some(n) = self.node_mut(id) {
                        if let Some(r) = radius {
                            n.corner.radius_in = r.max(0.0);
                        }
                        if let Some(s) = style {
                            n.corner.style = *s;
                        }
                    }
                }
            }
            CornerOp::Cycle { nodes } => {
                let ids: Vec<NodeId> = nodes.iter().copied().filter(|id| self.corner_eligible(*id)).collect();
                for id in ids {
                    if let Some(n) = self.node_mut(id) {
                        n.corner.style = n.corner.style.next();
                    }
                }
            }
            CornerOp::RoundAll { radius_in } => {
                let ids: Vec<NodeId> = self.all().into_iter().filter(|id| self.corner_eligible(*id)).collect();
                for id in ids {
                    if let Some(n) = self.node_mut(id) {
                        n.corner = Corner::round(radius_in.max(0.0));
                    }
                }
            }
        }
    }

    /// The selected node indices on contour `c`.
    fn picked(&self, sel: &Selection, c: usize) -> BTreeSet<usize> {
        let n = self.contours[c].nodes.len();
        sel.iter().filter(|id| id.contour == c && id.node < n).map(|id| id.node).collect()
    }

    /// Grabbable points in world inches: every anchor, and the handles (off their anchor) of the
    /// selected nodes and their neighbours.
    pub fn targets(&self, shape: &Shape, sel: &Selection) -> Vec<(Target, Pt)> {
        let mut shown = BTreeSet::new();
        for id in sel.iter().filter(|id| self.node(**id).is_some()) {
            let (prev, next) = self.contours[id.contour].neighbours(id.node);
            shown.extend([Some(id.node), prev, next].into_iter().flatten().map(|node| NodeId { contour: id.contour, node }));
        }
        let mut out = Vec::new();
        for (c, nc) in self.contours.iter().enumerate() {
            for (i, n) in nc.nodes.iter().enumerate() {
                let id = NodeId { contour: c, node: i };
                out.push((Target { id, part: Part::Anchor }, shape.to_world(n.pt)));
                if shown.contains(&id) {
                    for (side, h) in [(Side::In, n.inn), (Side::Out, n.out)] {
                        if let Some(h) = h.filter(|h| !near(*h, n.pt)) {
                            out.push((Target { id, part: Part::Handle(side) }, shape.to_world(h)));
                        }
                    }
                }
            }
        }
        out
    }

    /// The nearest point on any segment to `at` (local inches), if within `within`.
    pub fn nearest(&self, at: Pt, within: f32) -> Option<SegHit> {
        let dist = |t: f32, b: &[Pt; 4]| len(sub(bez(b, t), at));
        let mut best: Option<(f32, SegHit)> = None;
        for (contour, nc) in self.contours.iter().enumerate() {
            for seg in 0..nc.seg_count() {
                let (b, _) = nc.bezier(seg);
                let k = (0..=COARSE).min_by(|&i, &j| dist(i as f32 / COARSE as f32, &b).total_cmp(&dist(j as f32 / COARSE as f32, &b))).unwrap();
                let (mut lo, mut hi) = ((k.saturating_sub(1)) as f32 / COARSE as f32, ((k + 1).min(COARSE)) as f32 / COARSE as f32);
                for _ in 0..40 {
                    let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                    if dist(m1, &b) < dist(m2, &b) { hi = m2 } else { lo = m1 }
                }
                let t = (lo + hi) / 2.0;
                let d = dist(t, &b);
                if d <= within && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, SegHit { contour, seg, t, pt: bez(&b, t) }));
                }
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// Splits the segment at `hit.t` with de Casteljau, so the outline does not change. The new node is
    /// Smooth on a curve and Straight on a line. Returns its id; later nodes on the contour shift up one.
    pub fn insert(&mut self, hit: SegHit) -> NodeId {
        let nc = &mut self.contours[hit.contour];
        let (b, line) = nc.bezier(hit.seg);
        let after = (hit.seg + 1) % nc.nodes.len();
        let new = if line {
            Node::new(lerp(b[0], b[3], hit.t), NodeKind::Straight)
        } else {
            let (l, r) = split_cubic(b[0], b[1], b[2], b[3], hit.t);
            nc.nodes[hit.seg].out = handle(l[1], l[0]);
            nc.nodes[hit.seg].settle();
            nc.nodes[after].inn = handle(r[2], r[3]);
            nc.nodes[after].settle();
            let mut n = Node { pt: l[3], inn: handle(l[2], l[3]), out: handle(r[1], l[3]), kind: NodeKind::Smooth, corner: crate::corners::Corner::default() };
            n.settle();
            n
        };
        nc.nodes.insert(hit.seg + 1, new);
        NodeId { contour: hit.contour, node: hit.seg + 1 }
    }

    /// Moves the selected anchors by `d` (local inches), each carrying its own handles.
    pub fn move_anchors(&mut self, sel: &Selection, d: Pt) {
        for id in sel {
            if let Some(n) = self.node_mut(*id) {
                n.pt = add(n.pt, d);
                n.inn = n.inn.map(|h| add(h, d));
                n.out = n.out.map(|h| add(h, d));
            }
        }
    }

    /// Moves one handle, enforcing the node's kind (see [`Node::move_handle`]).
    pub fn move_handle(&mut self, id: NodeId, side: Side, to: Pt) {
        if let Some(n) = self.node_mut(id) {
            n.move_handle(side, to);
        }
    }

    /// The kind every selected node shares; `None` when mixed or nothing is selected.
    pub fn kind_of(&self, sel: &Selection) -> Option<NodeKind> {
        let mut kinds = sel.iter().filter_map(|id| self.node(*id)).map(|n| n.kind);
        let first = kinds.next()?;
        kinds.all(|k| k == first).then_some(first)
    }

    /// Open contours of at least three nodes that Close would join: those the selection touches, or
    /// all of them when nothing is selected.
    fn closable(&self, sel: &Selection) -> Vec<usize> {
        let any = sel.iter().any(|id| self.node(*id).is_some());
        (0..self.contours.len()).filter(|&c| !self.contours[c].closed && self.contours[c].nodes.len() >= 3 && (!any || !self.picked(sel, c).is_empty())).collect()
    }

    /// Segments of contour `c` whose two ends are both selected, ascending.
    fn midpoint_segs(&self, sel: &Selection, c: usize) -> Vec<usize> {
        let (nc, picked) = (&self.contours[c], self.picked(sel, c));
        (0..nc.seg_count()).filter(|&s| picked.contains(&s) && picked.contains(&((s + 1) % nc.nodes.len()))).collect()
    }

    /// Whether `action` can run on `sel`: the rule `apply` enforces, for disabling a control.
    pub fn check(&self, sel: &Selection, action: NodeAction) -> Result<(), Blocked> {
        let picked: Vec<NodeId> = sel.iter().filter(|id| self.node(**id).is_some()).copied().collect();
        if picked.is_empty() && matches!(action, NodeAction::Convert(_) | NodeAction::AddMidpoints | NodeAction::Remove | NodeAction::OpenAt) {
            return Err(Blocked::NothingSelected);
        }
        match action {
            NodeAction::Remove => {
                for (c, nc) in self.contours.iter().enumerate() {
                    let k = self.picked(sel, c).len();
                    if k > 0 && nc.nodes.len() - k < nc.min_nodes() {
                        return Err(Blocked::TooFewNodes { min: nc.min_nodes() });
                    }
                }
                Ok(())
            }
            NodeAction::OpenAt if !picked.iter().any(|id| self.contours[id.contour].closed) => Err(Blocked::NoClosedContour),
            NodeAction::Close if self.closable(sel).is_empty() => Err(Blocked::NoOpenContour),
            NodeAction::AddMidpoints if (0..self.contours.len()).all(|c| self.midpoint_segs(sel, c).is_empty()) => Err(Blocked::NoAdjacentPair),
            _ => Ok(()),
        }
    }

    /// Runs `action` and returns the selection remapped to the new node indices.
    ///
    /// - Convert to Straight drops the node's handles and its neighbours' facing handles, so both
    ///   adjacent segments become lines. From Straight to a curved kind, handles go one third of the way
    ///   to each neighbour along the bisector tangent (Catmull-Rom), equal length for Symmetric. Cusp or
    ///   Smooth to Smooth or Symmetric keeps lengths (averages them for Symmetric) and aligns to the
    ///   bisector of the two handle directions.
    /// - Remove refits one cubic across each run of removed nodes by least squares on the original span's
    ///   samples, holding the surviving neighbours' tangents when they are Smooth or Symmetric. A run of
    ///   lines stays a line. Removing an open end just drops it.
    /// - Smooth with nodes selected blends each node's handles towards the Catmull-Rom handles by the
    ///   strength and makes it Smooth. With none selected, each contour is flattened, simplified with
    ///   Ramer-Douglas-Peucker at a tolerance scaled by the strength, and refitted with Catmull-Rom.
    pub fn apply(&mut self, sel: &Selection, action: NodeAction) -> Result<Selection, Blocked> {
        self.check(sel, action)?;
        let sel: Selection = sel.iter().filter(|id| self.node(**id).is_some()).copied().collect();
        let contours = 0..self.contours.len();
        Ok(match action {
            NodeAction::Convert(kind) => {
                for id in &sel {
                    self.contours[id.contour].convert(id.node, kind);
                }
                sel
            }
            NodeAction::Smooth(strength) => {
                if sel.is_empty() {
                    let tol = smooth_tolerance(strength);
                    // Strength 0 would keep every flattened vertex as a node; it means leave alone.
                    if tol > 0.0 {
                        self.contours.iter_mut().for_each(|nc| nc.smooth_whole(tol));
                    }
                } else {
                    for id in &sel {
                        self.contours[id.contour].smooth_node(id.node, f32::from(strength.min(100)) / 100.0);
                    }
                }
                sel
            }
            NodeAction::Remove => {
                for c in contours {
                    let gone = self.picked(&sel, c);
                    if !gone.is_empty() {
                        self.contours[c].remove(&gone);
                    }
                }
                Selection::new()
            }
            NodeAction::OpenAt => {
                let mut next = Selection::new();
                for c in contours {
                    let nc = &mut self.contours[c];
                    let Some(k) = sel.iter().find(|id| id.contour == c).map(|id| id.node).filter(|_| nc.closed) else { continue };
                    nc.nodes.rotate_left(k);
                    let mut last = nc.nodes[0];
                    last.out = None;
                    last.settle();
                    nc.nodes[0].inn = None;
                    nc.nodes[0].settle();
                    nc.nodes.push(last);
                    nc.closed = false;
                    next.extend([0, nc.nodes.len() - 1].map(|node| NodeId { contour: c, node }));
                }
                next
            }
            NodeAction::Close => {
                for c in self.closable(&sel) {
                    let nc = &mut self.contours[c];
                    nc.nodes[0].inn = None;
                    nc.nodes[0].settle();
                    let last = nc.nodes.last_mut().unwrap();
                    last.out = None;
                    last.settle();
                    nc.closed = true;
                }
                sel
            }
            NodeAction::AddMidpoints => {
                let mut next = Selection::new();
                for c in contours {
                    let segs = self.midpoint_segs(&sel, c);
                    for &seg in segs.iter().rev() {
                        let pt = bez(&self.contours[c].bezier(seg).0, 0.5);
                        self.insert(SegHit { contour: c, seg, t: 0.5, pt });
                    }
                    let shift = |j: usize| j + segs.iter().filter(|&&s| s < j).count();
                    next.extend(self.picked(&sel, c).into_iter().map(|j| NodeId { contour: c, node: shift(j) }));
                    next.extend(segs.iter().enumerate().map(|(k, &s)| NodeId { contour: c, node: s + 1 + k }));
                }
                next
            }
        })
    }
}

fn dist_to_polyline(p: Pt, poly: &[Pt], closed: bool) -> f32 {
    let n = poly.len();
    let segs = if closed { n } else { n - 1 };
    (0..segs)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
        })
        .fold(f32::MAX, f32::min)
}

/// The tolerance, in inches, a whole-path Smooth at `strength` (0..=100) simplifies to.
pub fn smooth_tolerance(strength: u8) -> f32 {
    0.15 * f32::from(strength.min(100)) / 100.0
}

/// Turns an editable shape into a sharp `Path` in place, handing a parametric shape's corner radii over
/// as live corners (spec 55). A shape already a Path is left alone. The Corner tool and `Edit::Corners`
/// call this before cornering, so corners always land on a path.
pub fn to_path(shape: &mut Shape) {
    match &shape.kind {
        ShapeKind::Path { .. } => {}
        ShapeKind::Parametric(p) => {
            let (p, size) = (*p, (shape.width_in, shape.height_in));
            let (contours, corners) = crate::parametric::to_corner_path(&p, size).unwrap_or_else(|| (crate::parametric::outline(&p, size), Vec::new()));
            shape.name.get_or_insert_with(|| shape.kind.label().to_owned());
            shape.kind = ShapeKind::Path { contours, view_box: size, node_kinds: Vec::new(), corners };
        }
        _ => *shape = NodePath::read(shape).write(shape),
    }
}

/// The local-frame offset matching a world-space offset `d` on `shape`, so nudges follow the canvas
/// under rotation and flip.
pub fn local_delta(shape: &Shape, d: Pt) -> Pt {
    let (a, b) = (shape.to_local((0.0, 0.0)), shape.to_local(d));
    (b.0 - a.0, b.1 - a.1)
}

/// Adds a drawn path whose node kinds are known, such as a Pen path, in one design change.
pub fn add_drawn(design: &mut Design, drawn: &NodeContour, fill: Color32, operation: Operation) -> Option<u64> {
    let id = design.next_id();
    let mut shape = crate::design::shape_from_contours(id, vec![drawn.to_contour()], fill, operation)?;
    if let ShapeKind::Path { node_kinds, .. } = &mut shape.kind {
        *node_kinds = vec![drawn.kinds()];
    }
    design.shapes.push(shape);
    Some(id)
}

/// Whether `shape`'s nodes can be edited: any plain outline. The first edit turns it into a Path. Text
/// and Instance keep their own double-click, a Compound's outline belongs to its children, a Flattened
/// shape would lose its printed parts, and an image has no outline to edit.
pub fn editable(shape: &Shape) -> bool {
    match shape.kind {
        ShapeKind::Line
        | ShapeKind::TriangleDown
        | ShapeKind::Triangle
        | ShapeKind::Diamond
        | ShapeKind::Star
        | ShapeKind::StarThin
        | ShapeKind::Octagon
        | ShapeKind::Pentagon
        | ShapeKind::Hexagon
        | ShapeKind::Heart
        | ShapeKind::Plus
        | ShapeKind::Equals
        | ShapeKind::Circle
        | ShapeKind::Rectangle
        | ShapeKind::Square
        | ShapeKind::Pill
        | ShapeKind::RoundedSquare
        | ShapeKind::Oval
        | ShapeKind::Arch
        | ShapeKind::HalfCircle
        | ShapeKind::Builtin(_)
        | ShapeKind::Imported { .. }
        | ShapeKind::Parametric(_)
        | ShapeKind::Path { .. } => true,
        ShapeKind::Text { .. } | ShapeKind::Flattened { .. } | ShapeKind::Bitmap { .. } | ShapeKind::Compound(_) | ShapeKind::Instance { .. } => false,
    }
}

/// Why an editable path's nodes cannot be edited as they stand. A Warp after an Offset or Sticker bends
/// them together, which no edit of the body's nodes can reproduce.
pub fn refusal(shape: &Shape) -> Option<&'static str> {
    shape.effects.has_trailing_warp().then_some("Expand to edit nodes of a warped offset.")
}

/// The notice node editing shows on entry, when its first edit will change the stack.
pub fn notice(shape: &Shape) -> Option<&'static str> {
    refusal(shape).or_else(|| shape.effects.leading_warp().is_some().then_some("Editing nodes applies the warp."))
}

fn split_cubic(p0: Pt, c1: Pt, c2: Pt, p3: Pt, t: f32) -> ([Pt; 4], [Pt; 4]) {
    let (p01, p12, p23) = (lerp(p0, c1, t), lerp(c1, c2, t), lerp(c2, p3, t));
    let (p012, p123) = (lerp(p01, p12, t), lerp(p12, p23, t));
    let m = lerp(p012, p123, t);
    ([p0, p01, p012, m], [m, p123, p23, p3])
}

/// Least-squares cubic through `samples` (first and last are the fixed ends), chord-length
/// parameterised. With tangents given, solves only the two handle lengths (Schneider); without, solves
/// both control points. Returns the two control points.
fn fit_cubic(samples: &[Pt], t0: Option<Pt>, t1: Option<Pt>) -> (Pt, Pt) {
    let (p0, p3) = (samples[0], samples[samples.len() - 1]);
    let mut acc = 0.0;
    let mut u: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            if i > 0 {
                acc += len(sub(s, samples[i - 1]));
            }
            acc
        })
        .collect();
    let total = acc;
    let last = (samples.len() - 1).max(1) as f32;
    u.iter_mut().enumerate().for_each(|(i, t)| *t = if total > 0.0 { *t / total } else { i as f32 / last });
    let mut ctrl = least_squares(samples, &u, t0, t1);
    // Newton reparameterisation: move each sample's t to its nearest point on the current fit.
    for _ in 0..2 {
        let b = [p0, ctrl.0, ctrl.1, p3];
        for (t, &s) in u.iter_mut().zip(samples) {
            *t = newton_step(&b, s, *t);
        }
        ctrl = least_squares(samples, &u, t0, t1);
    }
    ctrl
}

fn newton_step(b: &[Pt; 4], s: Pt, t: f32) -> f32 {
    let d1: [Pt; 3] = [mul(sub(b[1], b[0]), 3.0), mul(sub(b[2], b[1]), 3.0), mul(sub(b[3], b[2]), 3.0)];
    let d2: [Pt; 2] = [mul(sub(d1[1], d1[0]), 2.0), mul(sub(d1[2], d1[1]), 2.0)];
    let u = 1.0 - t;
    let q1 = add(add(mul(d1[0], u * u), mul(d1[1], 2.0 * u * t)), mul(d1[2], t * t));
    let q2 = add(mul(d2[0], u), mul(d2[1], t));
    let e = sub(bez(b, t), s);
    let num = e.0 * q1.0 + e.1 * q1.1;
    let den = q1.0 * q1.0 + q1.1 * q1.1 + e.0 * q2.0 + e.1 * q2.1;
    if den.abs() < 1e-12 { t } else { (t - num / den).clamp(0.0, 1.0) }
}

/// One least-squares solve at fixed parameters `u`. Each end's control point is either `end + a * tangent`
/// (one unknown) or free (two unknowns).
fn least_squares(samples: &[Pt], u: &[f32], t0: Option<Pt>, t1: Option<Pt>) -> (Pt, Pt) {
    let (p0, p3) = (samples[0], samples[samples.len() - 1]);
    let chord = len(sub(p3, p0));
    let basis = |t: f64| {
        let v = 1.0 - t;
        [v * v * v, 3.0 * v * v * t, 3.0 * v * t * t, t * t * t]
    };
    let f = |p: Pt| (f64::from(p.0), f64::from(p.1));
    let m = [t0, t1].iter().map(|t| if t.is_some() { 1 } else { 2 }).sum();
    let mut a = vec![vec![0.0f64; m + 1]; m];
    for (&s, &t) in samples.iter().zip(u) {
        // The x and y coefficient of each unknown at this sample, and the residual the fixed terms leave.
        let b = basis(f64::from(t));
        let mut cols = Vec::with_capacity(m);
        let (mut rx, mut ry) = (f64::from(s.0) - b[0] * f(p0).0 - b[3] * f(p3).0, f64::from(s.1) - b[0] * f(p0).1 - b[3] * f(p3).1);
        for (tangent, k, end) in [(t0, 1, p0), (t1, 2, p3)] {
            match tangent {
                Some(d) => {
                    cols.push((b[k] * f(d).0, b[k] * f(d).1));
                    rx -= b[k] * f(end).0;
                    ry -= b[k] * f(end).1;
                }
                None => cols.extend([(b[k], 0.0), (0.0, b[k])]),
            }
        }
        for i in 0..m {
            for j in 0..m {
                a[i][j] += cols[i].0 * cols[j].0 + cols[i].1 * cols[j].1;
            }
            a[i][m] += cols[i].0 * rx + cols[i].1 * ry;
        }
    }
    let x = gauss(a);
    let mut k = 0;
    let mut end = |tangent: Option<Pt>, anchor: Pt, toward: Pt| -> Pt {
        match tangent {
            Some(d) => {
                let alpha = x.as_ref().map(|x| x[k] as f32).filter(|a| *a > 1e-6 * chord).unwrap_or(chord / 3.0);
                k += 1;
                add(anchor, mul(d, alpha))
            }
            None => {
                let p = x.as_ref().map_or_else(|| lerp(anchor, toward, 1.0 / 3.0), |x| (x[k] as f32, x[k + 1] as f32));
                k += 2;
                p
            }
        }
    };
    let c1 = end(t0, p0, p3);
    let c2 = end(t1, p3, p0);
    (c1, c2)
}

/// Solves an augmented `m x (m + 1)` system by Gaussian elimination; `None` when singular.
fn gauss(mut a: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let m = a.len();
    for col in 0..m {
        let pivot = (col..m).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let (top, rest) = a.split_at_mut(col + 1);
        let pivot_row = &top[col];
        for row in rest {
            let f = row[col] / pivot_row[col];
            for (x, p) in row.iter_mut().zip(pivot_row).skip(col) {
                *x -= f * p;
            }
        }
    }
    let mut x = vec![0.0; m];
    for row in (0..m).rev() {
        x[row] = (a[row][m] - (row + 1..m).map(|j| a[row][j] * x[j]).sum::<f64>()) / a[row][row];
    }
    Some(x)
}

/// Handles for a node at `pt` between `prev` and `next`: along the bisector tangent, one third of the
/// distance to each neighbour.
fn catmull_rom_handles(prev: Pt, pt: Pt, next: Pt) -> (Pt, Pt) {
    let (back, ahead) = (sub(pt, prev), sub(next, pt));
    let t = unit(add(unit(back).unwrap_or_default(), unit(ahead).unwrap_or_default())).or_else(|| unit(sub(next, prev))).unwrap_or_default();
    (sub(pt, mul(t, len(back) / 3.0)), add(pt, mul(t, len(ahead) / 3.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;

    fn open() -> Contour {
        Contour { start: (0.0, 0.0), segs: vec![Seg::Line((2.0, 0.0)), Seg::Cubic((3.0, 0.0), (3.0, 1.0), (2.0, 2.0))], closed: false }
    }

    fn closed() -> Contour {
        Contour { start: (0.0, 0.0), segs: vec![Seg::Line((2.0, 0.0)), Seg::Line((1.0, 2.0)), Seg::Line((0.0, 0.0))], closed: true }
    }

    fn shape(c: Contour) -> Shape {
        crate::design::shape_from_contours(1, vec![c], Color32::BLACK, Operation::Pen).unwrap()
    }

    fn path(c: NodeContour) -> NodePath {
        NodePath { contours: vec![c] }
    }

    fn polyline(pts: &[Pt]) -> NodeContour {
        NodeContour { nodes: pts.iter().map(|&p| Node::new(p, NodeKind::Straight)).collect(), closed: false }
    }

    fn id(node: usize) -> NodeId {
        NodeId { contour: 0, node }
    }

    fn sel(nodes: &[usize]) -> Selection {
        nodes.iter().map(|&n| id(n)).collect()
    }

    fn close(a: Pt, b: Pt, eps: f32) -> bool {
        (a.0 - b.0).hypot(a.1 - b.1) < eps
    }

    fn circle(n: usize) -> NodeContour {
        let pts: Vec<Pt> = (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                (1.5 + 1.5 * a.cos(), 1.5 + 1.5 * a.sin())
            })
            .collect();
        NodeContour::catmull_rom(&pts, true)
    }

    /// The larger of the two one-sided distances between densely flattened outlines.
    fn deviation(a: &Contour, b: &Contour) -> f32 {
        let (pa, pb) = (geometry::flatten(a, 0.0002), geometry::flatten(b, 0.0002));
        let one = |from: &[Pt], to: &[Pt], closed: bool| from.iter().map(|p| dist_to_polyline(*p, to, closed)).fold(0.0, f32::max);
        one(&pa, &pb, b.closed).max(one(&pb, &pa, a.closed))
    }

    fn collinear_opposite(n: &Node) -> bool {
        let (i, o) = (n.inn.unwrap(), n.out.unwrap());
        let (a, b) = ((i.0 - n.pt.0, i.1 - n.pt.1), (o.0 - n.pt.0, o.1 - n.pt.1));
        (a.0 * b.1 - a.1 * b.0).abs() < 1e-4 && a.0 * b.0 + a.1 * b.1 < 0.0
    }

    #[test]
    fn nodes_round_trip() {
        for c in [open(), closed()] {
            assert_eq!(NodeContour::from_contour(&c, None).to_contour(), c);
        }
        assert_eq!(NodeContour::from_contour(&closed(), None).nodes.len(), 3, "the closing node is stored once");
    }

    #[test]
    fn handles_on_their_anchor_are_none() {
        let c = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((0.0, 0.0), (1.0, 1.0), (2.0, 0.0))], closed: false };
        let nc = NodeContour::from_contour(&c, None);
        assert_eq!(nc.nodes[0].out, None);
        assert_eq!(nc.nodes[1].inn, Some((1.0, 1.0)));
    }

    #[test]
    fn inserting_on_a_cubic_keeps_the_outline() {
        let mut p = path(NodeContour::from_contour(&open(), None));
        let before = p.contours[0].to_contour();
        let new = p.insert(SegHit { contour: 0, seg: 1, t: 0.37, pt: (0.0, 0.0) });
        assert_eq!(new, id(2));
        assert_eq!(p.contours[0].nodes.len(), 4);
        assert_eq!(p.contours[0].nodes[2].kind, NodeKind::Smooth);
        let d = deviation(&before, &p.contours[0].to_contour());
        assert!(d < 0.001, "outline moved by {d}");
    }

    #[test]
    fn inserting_on_a_line_adds_a_straight_node() {
        let mut p = path(NodeContour::from_contour(&open(), None));
        let new = p.insert(SegHit { contour: 0, seg: 0, t: 0.25, pt: (0.5, 0.0) });
        let n = p.node(new).unwrap();
        assert!(close(n.pt, (0.5, 0.0), 1e-5) && n.kind == NodeKind::Straight);
        assert!(matches!(p.contours[0].to_contour().segs[..2], [Seg::Line(_), Seg::Line(_)]));
    }

    #[test]
    fn nearest_finds_the_closest_point_on_a_curve() {
        let p = path(NodeContour::from_contour(&open(), None));
        let hit = p.nearest((3.0, 1.0), 0.5).expect("a hit");
        assert_eq!((hit.contour, hit.seg), (0, 1));
        let c = open();
        let Seg::Cubic(c1, c2, end) = c.segs[1] else { unreachable!() };
        let (l, _) = split_cubic((2.0, 0.0), c1, c2, end, hit.t);
        assert!(close(l[3], hit.pt, 1e-4), "pt matches t");
        let d = (hit.pt.0 - 3.0).hypot(hit.pt.1 - 1.0);
        let coarse = (0..=1000).map(|i| split_cubic((2.0, 0.0), c1, c2, end, i as f32 / 1000.0).0[3]).map(|q| (q.0 - 3.0).hypot(q.1 - 1.0)).fold(f32::MAX, f32::min);
        assert!(d <= coarse + 1e-4, "{d} vs {coarse}");
        assert!(p.nearest((10.0, 10.0), 0.5).is_none());
    }

    #[test]
    fn removing_from_a_circle_refits_within_tolerance() {
        let mut p = path(circle(16));
        let before = p.contours[0].to_contour();
        let next = p.apply(&sel(&[5]), NodeAction::Remove).unwrap();
        assert!(next.is_empty());
        assert_eq!(p.contours[0].nodes.len(), 15);
        assert!(p.contours[0].nodes.iter().all(|n| n.kind == NodeKind::Smooth), "the held tangents keep the neighbours smooth");
        let d = deviation(&before, &p.contours[0].to_contour());
        assert!(d < 0.02, "refit strays by {d}");
    }

    #[test]
    fn removing_a_node_rejoins_its_neighbours() {
        let mut p = path(NodeContour::from_contour(&open(), None));
        p.apply(&sel(&[1]), NodeAction::Remove).unwrap();
        let c = p.contours[0].to_contour();
        assert_eq!(c.segs.len(), 1);
        assert!(matches!(c.segs[0], Seg::Cubic(..)), "a span with a curve refits to a curve");
    }

    #[test]
    fn removing_from_a_run_of_lines_stays_a_line() {
        let mut p = path(polyline(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)]));
        p.apply(&sel(&[1, 2]), NodeAction::Remove).unwrap();
        assert_eq!(p.contours[0].to_contour().segs, vec![Seg::Line((3.0, 1.0))]);
    }

    #[test]
    fn removing_an_open_end_drops_it() {
        let mut p = path(NodeContour::from_contour(&open(), None));
        p.apply(&sel(&[2]), NodeAction::Remove).unwrap();
        let c = p.contours[0].to_contour();
        assert_eq!(c.segs, vec![Seg::Line((2.0, 0.0))]);
        assert_eq!(p.contours[0].nodes[1].out, None, "no dangling handle");
    }

    #[test]
    fn a_cusp_handle_moves_alone() {
        let mut n = Node { pt: (0.0, 0.0), inn: Some((-1.0, 0.0)), out: Some((2.0, 0.0)), kind: NodeKind::Cusp, corner: crate::corners::Corner::default() };
        n.move_handle(Side::Out, (0.0, 3.0));
        assert_eq!((n.inn, n.out), (Some((-1.0, 0.0)), Some((0.0, 3.0))));
    }

    #[test]
    fn a_smooth_handle_swings_its_twin_keeping_its_length() {
        let mut n = Node { pt: (0.0, 0.0), inn: Some((-1.0, 0.0)), out: Some((2.0, 0.0)), kind: NodeKind::Smooth, corner: crate::corners::Corner::default() };
        n.move_handle(Side::Out, (0.0, 3.0));
        assert!(close(n.inn.unwrap(), (0.0, -1.0), 1e-5), "{:?}", n.inn);
        n.move_handle(Side::In, (4.0, 0.0));
        assert!(close(n.out.unwrap(), (-3.0, 0.0), 1e-5), "{:?}", n.out);
    }

    #[test]
    fn a_symmetric_handle_mirrors_its_twin() {
        let mut n = Node { pt: (1.0, 1.0), inn: Some((0.0, 1.0)), out: Some((2.0, 1.0)), kind: NodeKind::Symmetric, corner: crate::corners::Corner::default() };
        n.move_handle(Side::Out, (1.5, 3.0));
        assert!(close(n.inn.unwrap(), (0.5, -1.0), 1e-5), "{:?}", n.inn);
        let mut fresh = Node::new((0.0, 0.0), NodeKind::Symmetric);
        fresh.move_handle(Side::Out, (1.0, 2.0));
        assert_eq!(fresh.inn, Some((-1.0, -2.0)), "the twin is created");
    }

    #[test]
    fn dragging_out_of_a_straight_node_makes_it_a_cusp() {
        let mut n = Node::new((0.0, 0.0), NodeKind::Straight);
        n.move_handle(Side::Out, (1.0, 1.0));
        assert_eq!((n.kind, n.inn, n.out), (NodeKind::Cusp, None, Some((1.0, 1.0))));
    }

    #[test]
    fn moving_an_anchor_carries_its_handles() {
        let mut p = path(NodeContour::from_contour(&open(), None));
        p.move_anchors(&sel(&[1]), (0.0, 1.0));
        assert_eq!(p.contours[0].nodes[1], Node { pt: (2.0, 1.0), inn: None, out: Some((3.0, 1.0)), kind: NodeKind::Cusp, corner: crate::corners::Corner::default() });
    }

    #[test]
    fn straight_to_smooth_makes_collinear_handles() {
        let mut p = path(polyline(&[(0.0, 0.0), (1.0, 1.0), (3.0, 0.0)]));
        p.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Smooth)).unwrap();
        let n = p.contours[0].nodes[1];
        assert_eq!(n.kind, NodeKind::Smooth);
        assert!(collinear_opposite(&n), "{n:?}");
        let (li, lo) = ((n.inn.unwrap().0 - 1.0).hypot(n.inn.unwrap().1 - 1.0), (n.out.unwrap().0 - 1.0).hypot(n.out.unwrap().1 - 1.0));
        assert!((li - 2f32.sqrt() / 3.0).abs() < 1e-4 && (lo - 5f32.sqrt() / 3.0).abs() < 1e-4, "a third of the way to each neighbour: {li} {lo}");
        p.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Symmetric)).unwrap();
        assert!(NodeKind::Symmetric.holds(&p.contours[0].nodes[1]));
    }

    #[test]
    fn smooth_to_straight_drops_handles_and_makes_lines() {
        let mut p = path(NodeContour::catmull_rom(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)], false));
        assert_eq!(p.contours[0].nodes[1].kind, NodeKind::Smooth);
        p.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Straight)).unwrap();
        let n = p.contours[0].nodes[1];
        assert_eq!((n.kind, n.inn, n.out), (NodeKind::Straight, None, None));
        let segs = p.contours[0].to_contour().segs;
        assert!(matches!(segs[0], Seg::Line(_)) && matches!(segs[1], Seg::Line(_)), "{segs:?}");
        assert!(matches!(segs[2], Seg::Cubic(..)));
    }

    #[test]
    fn cusp_to_symmetric_aligns_and_averages() {
        let n = Node { pt: (0.0, 0.0), inn: Some((-1.0, 0.0)), out: Some((0.0, 3.0)), kind: NodeKind::Cusp, corner: crate::corners::Corner::default() };
        let mut p = path(NodeContour { nodes: vec![Node::new((-2.0, 0.0), NodeKind::Straight), n, Node::new((2.0, 2.0), NodeKind::Straight)], closed: false });
        p.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Symmetric)).unwrap();
        let m = p.contours[0].nodes[1];
        assert!(collinear_opposite(&m) && NodeKind::Symmetric.holds(&m), "{m:?}");
        assert!(((m.out.unwrap().0).hypot(m.out.unwrap().1) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn whole_path_smooth_simplifies_a_pencil_stroke() {
        let raw: Vec<Pt> = (0..200)
            .map(|i| {
                let x = i as f32 * 0.03;
                let jitter = ((i * 7919 % 13) as f32 / 13.0 - 0.5) * 0.03;
                (x, 2.0 + (x * 1.3).sin() + jitter)
            })
            .collect();
        let pencil = geometry::smooth_open_path(&geometry::rdp(&raw, 0.02));
        let s = shape(pencil);
        let mut p = NodePath::read(&s);
        let before = p.contours[0].nodes.len();
        p.apply(&Selection::new(), NodeAction::Smooth(100)).unwrap();
        let after = p.contours[0].nodes.len();
        assert!(after < before, "{after} nodes from {before}");
        assert!(p.contours[0].nodes[1..after - 1].iter().all(|n| n.kind == NodeKind::Smooth));
        let out = p.write(&s);
        let input: Vec<Pt> = geometry::flatten(&s.all_contours()[0], 0.0005).into_iter().map(|q| s.to_world(q)).collect();
        let allowance = 0.02;
        let worst = geometry::flatten(&out.all_contours()[0], 0.0005).into_iter().map(|q| dist_to_polyline(out.to_world(q), &input, false)).fold(0.0, f32::max);
        assert!(worst <= smooth_tolerance(100) + allowance, "strays by {worst}");
    }

    #[test]
    fn whole_path_smooth_wraps_a_closed_contour_without_a_seam() {
        let ring: Vec<Pt> = (0..120)
            .map(|i| {
                let a = i as f32 / 120.0 * std::f32::consts::TAU;
                let r = 1.5 + if i % 2 == 0 { 0.01 } else { -0.01 };
                (2.0 + r * a.cos(), 2.0 + r * a.sin())
            })
            .collect();
        let mut p = path(polyline(&ring));
        p.contours[0].closed = true;
        p.apply(&Selection::new(), NodeAction::Smooth(60)).unwrap();
        let c = &p.contours[0];
        assert!(c.closed && c.nodes.len() < 40, "{} nodes", c.nodes.len());
        assert!(c.nodes.iter().all(|n| n.kind == NodeKind::Smooth && collinear_opposite(n)), "every node, the first included, is smooth");
    }

    #[test]
    fn smoothing_selected_nodes_makes_them_smooth() {
        let mut p = path(polyline(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)]));
        let kept = p.apply(&sel(&[1, 2]), NodeAction::Smooth(50)).unwrap();
        assert_eq!(kept, sel(&[1, 2]));
        for i in [1, 2] {
            let n = p.contours[0].nodes[i];
            assert!(n.kind == NodeKind::Smooth && collinear_opposite(&n), "{n:?}");
        }
        assert_eq!(p.contours[0].nodes[0].kind, NodeKind::Straight);
    }

    #[test]
    fn an_old_project_without_node_kinds_infers_them() {
        let json = r#"{"id":1,"kind":{"Path":{"contours":[{"start":[0.0,0.0],"segs":[{"Cubic":[[0.0,0.0],[1.0,0.0],[2.0,0.0]]},{"Cubic":[[3.0,0.0],[4.0,1.0],[4.0,2.0]]},{"Cubic":[[4.0,4.0],[0.0,4.0],[0.0,4.0]]}],"closed":false}],"view_box":[4.0,4.0]}},"x_in":0.0,"y_in":0.0,"width_in":4.0,"height_in":4.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"Pen","hidden":false,"contour_hidden":[],"link":null,"pen":{"size":"Fine","colour":0},"warp":null}"#;
        let s: Shape = serde_json::from_str(json).unwrap();
        assert!(matches!(&s.kind, ShapeKind::Path { node_kinds, .. } if node_kinds.is_empty()));
        let kinds = NodePath::read(&s).contours[0].kinds();
        assert_eq!(kinds, vec![NodeKind::Straight, NodeKind::Symmetric, NodeKind::Smooth, NodeKind::Straight]);
    }

    #[test]
    fn kinds_round_trip_through_write_and_read() {
        let mut p = NodePath::read(&shape(open()));
        p.apply(&sel(&[0]), NodeAction::Convert(NodeKind::Cusp)).unwrap();
        let mut s = p.write(&shape(open()));
        s = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(matches!(&s.kind, ShapeKind::Path { node_kinds, .. } if node_kinds == &vec![p.contours[0].kinds()]));
        assert_eq!(NodePath::read(&s).contours[0].kinds(), p.contours[0].kinds());
    }

    #[test]
    fn a_stored_cusp_with_collinear_handles_survives_read() {
        let c = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((0.0, 0.0), (1.0, 0.0), (2.0, 0.0)), Seg::Cubic((3.0, 0.0), (4.0, 0.0), (4.0, 0.0))], closed: false };
        assert_eq!(NodeContour::from_contour(&c, None).nodes[1].kind, NodeKind::Symmetric);
        let stored = [NodeKind::Straight, NodeKind::Cusp, NodeKind::Straight];
        assert_eq!(NodeContour::from_contour(&c, Some(&stored)).kinds(), stored.to_vec());
    }

    #[test]
    fn a_stored_symmetric_that_no_longer_holds_is_demoted() {
        let c = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((0.0, 0.0), (1.0, 0.0), (2.0, 0.0)), Seg::Cubic((2.0, 2.0), (4.0, 0.0), (4.0, 0.0))], closed: false };
        let stored = [NodeKind::Straight, NodeKind::Symmetric, NodeKind::Straight];
        assert_eq!(NodeContour::from_contour(&c, Some(&stored)).nodes[1].kind, NodeKind::Cusp);
        let wrong_count = [NodeKind::Cusp, NodeKind::Cusp];
        assert_eq!(NodeContour::from_contour(&c, Some(&wrong_count)).nodes[0].kind, NodeKind::Straight, "a mismatched list is ignored");
    }

    #[test]
    fn refit_keeps_untouched_nodes_in_place_under_rotation_and_flip() {
        let mut s = shape(closed());
        s.rotation_deg = 30.0;
        s.flip_x = true;
        let p = NodePath::read(&s);
        let mut moved = p.clone();
        moved.move_anchors(&sel(&[2]), (0.0, 2.0));
        let edited = moved.write(&s);
        let again = NodePath::read(&edited);
        for i in [0, 1] {
            let (a, b) = (s.to_world(p.contours[0].nodes[i].pt), edited.to_world(again.contours[0].nodes[i].pt));
            assert!(close(a, b, 1e-4), "node {i} moved: {a:?} -> {b:?}");
        }
        assert!((edited.height_in - 4.0).abs() < 1e-4);
        assert_eq!(edited.effects.warp(), None);
    }

    #[test]
    fn targets_show_handles_of_selected_nodes_and_neighbours() {
        let p = path(circle(8));
        let s = shape(p.contours[0].to_contour());
        let p = NodePath::read(&s);
        let t = p.targets(&s, &sel(&[0]));
        assert_eq!(t.iter().filter(|(t, _)| t.part == Part::Anchor).count(), 8);
        let mut with_handles: Vec<usize> = t.iter().filter(|(t, _)| t.part != Part::Anchor).map(|(t, _)| t.id.node).collect();
        with_handles.dedup();
        assert_eq!(with_handles, vec![0, 1, 7]);
    }

    #[test]
    fn open_at_splits_a_closed_contour() {
        let mut p = path(circle(6));
        let before = p.contours[0].to_contour();
        let next = p.apply(&sel(&[2]), NodeAction::OpenAt).unwrap();
        let c = &p.contours[0];
        assert!(!c.closed && c.nodes.len() == 7);
        assert_eq!(next, sel(&[0, 6]));
        assert!(close(c.nodes[0].pt, c.nodes[6].pt, 1e-6));
        assert_eq!((c.nodes[0].inn, c.nodes[6].out), (None, None));
        assert!(deviation(&before, &c.to_contour()) < 1e-4, "the outline is unchanged");
    }

    #[test]
    fn close_joins_the_ends_with_a_line() {
        let mut p = path(NodeContour::catmull_rom(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)], false));
        p.apply(&Selection::new(), NodeAction::Close).unwrap();
        let c = p.contours[0].to_contour();
        assert!(c.closed);
        assert_eq!(c.segs.last(), Some(&Seg::Line((0.0, 0.0))));
    }

    #[test]
    fn add_midpoints_splits_between_selected_neighbours() {
        let mut p = path(polyline(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]));
        let next = p.apply(&sel(&[0, 1, 3]), NodeAction::AddMidpoints).unwrap();
        let pts: Vec<Pt> = p.contours[0].nodes.iter().map(|n| n.pt).collect();
        assert_eq!(pts, vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        assert_eq!(next, sel(&[0, 1, 2, 4]));
        p.contours[0].closed = true;
        let next = p.apply(&sel(&[0, 4]), NodeAction::AddMidpoints).unwrap();
        assert_eq!(p.contours[0].nodes[5].pt, (0.0, 1.0), "the closing segment splits too");
        assert_eq!(next, sel(&[0, 4, 5]));
    }

    #[test]
    fn check_says_why_an_action_is_blocked() {
        let p = path(polyline(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]));
        assert_eq!(p.check(&Selection::new(), NodeAction::Remove), Err(Blocked::NothingSelected));
        assert_eq!(p.check(&Selection::new(), NodeAction::Convert(NodeKind::Cusp)), Err(Blocked::NothingSelected));
        assert_eq!(p.check(&sel(&[0, 1]), NodeAction::Remove), Err(Blocked::TooFewNodes { min: 2 }));
        assert_eq!(p.check(&sel(&[1]), NodeAction::Remove), Ok(()));
        assert_eq!(p.check(&sel(&[1]), NodeAction::OpenAt), Err(Blocked::NoClosedContour));
        assert_eq!(p.check(&sel(&[0, 2]), NodeAction::AddMidpoints), Err(Blocked::NoAdjacentPair));
        assert_eq!(p.check(&Selection::new(), NodeAction::Smooth(50)), Ok(()));
        assert_eq!(p.check(&Selection::new(), NodeAction::Close), Ok(()));
        assert_eq!(path(polyline(&[(0.0, 0.0), (1.0, 0.0)])).check(&Selection::new(), NodeAction::Close), Err(Blocked::NoOpenContour));
        let ring = path(circle(3));
        assert_eq!(ring.check(&sel(&[0]), NodeAction::Close), Err(Blocked::NoOpenContour));
        assert_eq!(ring.check(&sel(&[0]), NodeAction::Remove), Err(Blocked::TooFewNodes { min: 3 }));
        assert_eq!(ring.check(&sel(&[0]), NodeAction::OpenAt), Ok(()));
        let mut q = p.clone();
        assert_eq!(q.apply(&sel(&[0, 1]), NodeAction::Remove), Err(Blocked::TooFewNodes { min: 2 }));
        assert_eq!(q, p, "a blocked action changes nothing");
        assert_eq!(Blocked::TooFewNodes { min: 2 }.reason(), "A path needs at least 2 nodes.");
    }

    #[test]
    fn kind_of_reports_a_shared_kind() {
        let mut p = path(polyline(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]));
        assert_eq!(p.kind_of(&sel(&[0, 1])), Some(NodeKind::Straight));
        assert_eq!(p.kind_of(&Selection::new()), None);
        p.contours[0].nodes[1].kind = NodeKind::Cusp;
        assert_eq!(p.kind_of(&sel(&[0, 1])), None);
    }

    #[test]
    fn add_drawn_stores_the_pen_kinds() {
        let mut design = Design::default();
        let mut mid = Node::new((1.0, 0.0), NodeKind::Cusp);
        mid.move_handle(Side::Out, (1.5, 1.0));
        let drawn = NodeContour { nodes: vec![Node::new((0.0, 0.0), NodeKind::Straight), mid, Node::new((2.0, 1.0), NodeKind::Straight)], closed: false };
        let id = add_drawn(&mut design, &drawn, Color32::BLACK, Operation::Pen).unwrap();
        let s = design.shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(NodePath::read(s).contours[0].kinds(), drawn.kinds());
        assert!(matches!(&s.kind, ShapeKind::Path { node_kinds, .. } if node_kinds.len() == 1));
    }

    #[test]
    fn smooth_tolerance_scales_with_strength() {
        assert_eq!(smooth_tolerance(0), 0.0);
        assert!(smooth_tolerance(50) < smooth_tolerance(100) && (smooth_tolerance(100) - 0.15).abs() < 1e-6);
    }

    #[test]
    fn only_plain_outlines_are_node_editable() {
        let mut d = Design::default();
        let heart = d.add_shape(ShapeKind::Heart);
        let cloud = d.add_shape(ShapeKind::Builtin(crate::shape_library::BuiltinId::named("Cloud").unwrap()));
        let text = d.add_text("Hi".into(), crate::text::FontRef::Inter, 1.0, Color32::BLACK, 0.0, 0.0);
        let by_id = |id: u64| d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        assert!(editable(&by_id(heart)) && editable(&by_id(cloud)) && editable(&shape(open())));
        assert!(!editable(&by_id(text)));
        for kind in [
            ShapeKind::Compound(crate::design::CompoundOp::Effects),
            ShapeKind::Instance { component: 1, overrides: Default::default() },
            ShapeKind::Flattened { parts: vec![], view_box: (1.0, 1.0) },
        ] {
            assert!(!editable(&Shape::new(9, kind.clone())), "{kind:?}");
        }
    }

    #[test]
    fn a_converted_shape_keeps_its_label_as_its_name() {
        let mut d = Design::default();
        let id = d.add_shape(ShapeKind::Star);
        let star = d.shapes.iter().find(|s| s.id == id).unwrap().clone();
        let out = NodePath::read(&star).write(&star);
        assert!(matches!(out.kind, ShapeKind::Path { .. }));
        assert_eq!(out.name.as_deref(), Some("Star"));
        assert_eq!((out.operation, out.fill), (star.operation, star.fill));
        let renamed = Shape { name: Some("Badge".into()), ..star };
        assert_eq!(NodePath::read(&renamed).write(&renamed).name.as_deref(), Some("Badge"));
        let pen = shape(open());
        assert_eq!(NodePath::read(&pen).write(&pen).name, None, "a pen path stays unnamed");
    }

    #[test]
    fn corners_stay_in_step_when_nodes_are_added_or_removed() {
        use crate::corners::Corner;
        // A closed square with a round corner on two of its four nodes.
        let nodes: Vec<Node> = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)].into_iter().map(|p| Node::new(p, NodeKind::Straight)).collect();
        let mut path = NodePath { contours: vec![NodeContour { nodes, closed: true }] };
        path.contours[0].nodes[1].corner = Corner::round(0.3);
        path.contours[0].nodes[3].corner = Corner::round(0.1);

        // Insert a node on the top edge, between nodes 0 and 1.
        let hit = path.nearest((1.0, 0.0), 0.5).expect("a point on the top edge");
        let new = path.insert(hit);
        let after_insert = path.contours[0].corners();
        assert_eq!(after_insert.iter().filter(|c| c.is_set()).count(), 2, "both corners survive the insert");
        assert!(!path.contours[0].nodes[new.node].corner.is_set(), "the new node has no corner");

        // Remove the 0.3 in cornered node; only its corner goes.
        let target = (0..path.contours[0].nodes.len()).find(|&i| (path.contours[0].nodes[i].corner.radius_in - 0.3).abs() < 1e-6).unwrap();
        path.apply(&sel(&[target]), NodeAction::Remove).unwrap();
        let after_remove = path.contours[0].corners();
        assert_eq!(after_remove.iter().filter(|c| c.is_set()).count(), 1, "only the removed node's corner is dropped");
        assert!((after_remove.iter().find(|c| c.is_set()).unwrap().radius_in - 0.1).abs() < 1e-6, "the surviving corner is the 0.1 in one");
    }

    #[test]
    fn converting_a_cornered_node_to_smooth_drops_its_corner_but_straight_keeps_it() {
        use crate::corners::Corner;
        let nodes: Vec<Node> = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)].into_iter().map(|p| Node::new(p, NodeKind::Straight)).collect();
        let mut path = NodePath { contours: vec![NodeContour { nodes, closed: true }] };
        path.contours[0].nodes[1].corner = Corner::round(0.3);

        path.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Straight)).unwrap();
        assert!(path.contours[0].nodes[1].corner.is_set(), "Straight leaves the corner alone");
        path.apply(&sel(&[1]), NodeAction::Convert(NodeKind::Smooth)).unwrap();
        assert!(!path.contours[0].nodes[1].corner.is_set(), "Smooth removes the corner");
    }
}

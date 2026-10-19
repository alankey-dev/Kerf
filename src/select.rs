//! Pure selection predicates shared by the Select Same menu, the legend pill and the lasso (spec 50).
//!
//! Every rule that answers "which shapes should this gesture select" lives here, so the Edit menu, the
//! context menu, the legend pill and the lasso all agree. Nothing here touches egui or a selection; each
//! function returns the ids a surface then holds.

use egui::Color32;

use crate::assets::AssetId;
use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::text::{FontRef, FontStyle};

/// What a Select Same row matches on. The first seven are "alike" rows that read the selection's shared
/// value; the last two ignore the selection and pick every text or path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Colour,
    Operation,
    Pen,
    Font,
    /// The `ShapeKind` variant.
    Kind,
    Effects,
    /// All of the above alike.
    Everything,
    Text,
    Paths,
}

impl Match {
    /// The rows of the Select Same submenu, in order.
    pub const MENU: [Match; 9] = [Match::Colour, Match::Operation, Match::Pen, Match::Font, Match::Kind, Match::Effects, Match::Everything, Match::Text, Match::Paths];

    /// The submenu row's label.
    pub fn label(self) -> &'static str {
        match self {
            Match::Colour => "Colour",
            Match::Operation => "Operation",
            Match::Pen => "Pen",
            Match::Font => "Font",
            Match::Kind => "Shape kind",
            Match::Effects => "Effects",
            Match::Everything => "Everything alike",
            Match::Text => "All text",
            Match::Paths => "All paths",
        }
    }

    /// Why an alike row is greyed when the selection does not share one value. Never shown for the two
    /// kind rows, which read no shared value.
    pub fn mixed_reason(self) -> &'static str {
        match self {
            Match::Colour => "Selection has mixed colours.",
            Match::Operation => "Selection has mixed operations.",
            Match::Pen => "Selection has mixed pens.",
            Match::Font => "Selection has mixed fonts.",
            Match::Kind => "Selection has mixed shape kinds.",
            Match::Effects => "Selection has mixed effects.",
            Match::Everything => "The selection is not all alike.",
            Match::Text | Match::Paths => "Nothing of that kind to select.",
        }
    }

    /// The two kind rows pick every text or path regardless of the selection.
    fn reads_selection(self) -> bool {
        !matches!(self, Match::Text | Match::Paths)
    }
}

/// Visible, unlocked shapes the canvas can pick: drawn (not inside a Compound), not hidden, not locked.
pub fn selectable(design: &Design) -> impl Iterator<Item = &Shape> {
    design.drawn().filter(|s| !s.hidden && !s.locked)
}

/// Every selectable shape's id, in z-order.
pub fn all_selectable(design: &Design) -> Vec<u64> {
    selectable(design).map(|s| s.id).collect()
}

fn is_text(kind: &ShapeKind) -> bool {
    matches!(kind, ShapeKind::Text { .. })
}

fn is_path(kind: &ShapeKind) -> bool {
    matches!(kind, ShapeKind::Path { .. } | ShapeKind::Imported { .. })
}

/// What `Match::Colour` compares: the pattern's asset, the pen's colour on a Pen layer, else the fill.
#[derive(PartialEq)]
enum ColourKey {
    Rgb([u8; 4]),
    Pattern(AssetId),
}

fn colour_key(s: &Shape) -> ColourKey {
    if let Some(p) = &s.pattern {
        return ColourKey::Pattern(p.asset.clone());
    }
    let c: Color32 = if s.operation == Operation::Pen { s.pen.rgb() } else { s.fill };
    ColourKey::Rgb([c.r(), c.g(), c.b(), c.a()])
}

/// A text's family and style, or `None` for a shape that carries no font.
fn font_key(s: &Shape) -> Option<(FontRef, FontStyle)> {
    match &s.kind {
        ShapeKind::Text { font, style, .. } => Some((font.clone(), *style)),
        _ => None,
    }
}

/// The ordered effect kinds on a shape, ignoring their settings and ids.
fn effect_kinds(s: &Shape) -> Vec<std::mem::Discriminant<crate::effects::Effect>> {
    s.effects.entries().iter().map(|e| std::mem::discriminant(&e.effect)).collect()
}

/// Whether `s` shares `t`'s `m`-value. Font alone needs both to be text; the other rows compare the one
/// value they name.
fn alike(t: &Shape, s: &Shape, m: Match) -> bool {
    match m {
        Match::Colour => colour_key(t) == colour_key(s),
        Match::Operation => t.operation == s.operation,
        Match::Pen => t.pen == s.pen,
        Match::Font => font_key(t).zip(font_key(s)).is_some_and(|(a, b)| a == b),
        Match::Kind => std::mem::discriminant(&t.kind) == std::mem::discriminant(&s.kind),
        Match::Effects => effect_kinds(t) == effect_kinds(s),
        Match::Everything => {
            colour_key(t) == colour_key(s)
                && t.operation == s.operation
                && t.pen == s.pen
                && font_key(t) == font_key(s)
                && std::mem::discriminant(&t.kind) == std::mem::discriminant(&s.kind)
                && effect_kinds(t) == effect_kinds(s)
        }
        Match::Text | Match::Paths => true,
    }
}

/// The ids Select Same picks: every selectable shape alike to the selection's first shape, or, for the
/// two kind rows, every selectable text or path.
pub fn same(design: &Design, on: &[u64], m: Match) -> Vec<u64> {
    match m {
        Match::Text => return selectable(design).filter(|s| is_text(&s.kind)).map(|s| s.id).collect(),
        Match::Paths => return selectable(design).filter(|s| is_path(&s.kind)).map(|s| s.id).collect(),
        _ => {}
    }
    let Some(target) = design.shapes.iter().find(|s| on.contains(&s.id)) else { return Vec::new() };
    selectable(design).filter(|s| alike(target, s, m)).map(|s| s.id).collect()
}

/// Whether the selection carries more than one `m`-value, so the alike row is greyed. The two kind rows
/// read no shared value and are never mixed.
pub fn mixed(design: &Design, on: &[u64], m: Match) -> bool {
    if !m.reads_selection() {
        return false;
    }
    let mut chosen = design.shapes.iter().filter(|s| on.contains(&s.id));
    let Some(first) = chosen.next() else { return false };
    chosen.any(|s| !alike(first, s, m))
}

/// Every selectable shape not in `on`.
pub fn inverse(design: &Design, on: &[u64]) -> Vec<u64> {
    selectable(design).filter(|s| !on.contains(&s.id)).map(|s| s.id).collect()
}

/// The single container every shape in `on` sits directly inside, if they share one.
fn common_container(design: &Design, on: &[u64]) -> Option<u64> {
    let mut parents = on.iter().map(|&id| design.parent_of(id));
    let first = parents.next()??;
    parents.all(|p| p == Some(first)).then_some(first)
}

/// Cmd+A as Figma widens it: the children of the container the selection sits inside, or, once those are
/// all selected (or the selection sits at the top level), every unlocked shape.
pub fn select_all(design: &Design, on: &[u64]) -> Vec<u64> {
    if let Some(c) = common_container(design, on) {
        let kids: Vec<u64> = design.children(Some(c)).filter(|s| !s.locked).map(|s| s.id).collect();
        let have: std::collections::HashSet<u64> = on.iter().copied().collect();
        if !kids.is_empty() && have != kids.iter().copied().collect() {
            return kids;
        }
    }
    design.shapes.iter().filter(|s| !s.locked).map(|s| s.id).collect()
}

/// Tab and Shift+Tab: the next or previous selectable shape in z-order, wrapping. From nothing, the first
/// or last.
pub fn walk(design: &Design, on: &[u64], forward: bool) -> Vec<u64> {
    let order = all_selectable(design);
    if order.is_empty() {
        return Vec::new();
    }
    let next = match order.iter().position(|id| on.contains(id)) {
        Some(i) if forward => (i + 1) % order.len(),
        Some(i) => (i + order.len() - 1) % order.len(),
        None if forward => 0,
        None => order.len() - 1,
    };
    vec![order[next]]
}

/// Every selectable shape with this operation: the rule behind the legend pill's click.
pub fn operation(design: &Design, op: Operation) -> Vec<u64> {
    selectable(design).filter(|s| s.operation == op).map(|s| s.id).collect()
}

/// Whether `p` lies inside the closed lasso polygon.
pub fn in_lasso(polygon: &[Pt], p: Pt) -> bool {
    polygon.len() >= 3 && crate::geometry::contains(polygon, p)
}

/// The ids the lasso picks: every shape whose evaluated outline has a node inside the polygon, following
/// the marquee's rules for guides and locked layers, then expanded to whole links.
pub fn lasso_hits(design: &Design, polygon: &[Pt]) -> Vec<u64> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let hits: Vec<u64> = design
        .drawn()
        .filter(|s| !s.locked && (s.operation != Operation::Guide || design.children(Some(s.id)).next().is_some()))
        .filter(|s| design.geometry(s).lines().any(|(pts, _)| pts.iter().any(|&p| in_lasso(polygon, p))))
        .map(|s| s.id)
        .collect();
    design.expand(&hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pens::{Pen, PenSize};

    fn square_at(d: &mut Design, x: f32, y: f32, fill: Color32) -> u64 {
        let id = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(id).unwrap();
        (s.x_in, s.y_in) = (x, y);
        s.fill = fill;
        id
    }

    #[test]
    fn select_same_colour_picks_every_red_and_skips_hidden_and_locked() {
        // A Pen layer carries its colour on its pen, so compare against that pen's rgb.
        let red = Pen::named("Red", PenSize::Fine).rgb();
        let mut d = Design::default();
        let red1 = square_at(&mut d, 0.0, 0.0, red);
        let red2 = square_at(&mut d, 2.0, 0.0, red);
        let _blue = square_at(&mut d, 4.0, 0.0, Color32::BLUE);
        let hidden_red = square_at(&mut d, 6.0, 0.0, red);
        d.shape_mut(hidden_red).unwrap().hidden = true;
        let locked_red = square_at(&mut d, 8.0, 0.0, red);
        d.shape_mut(locked_red).unwrap().locked = true;
        // A red Pen layer counts as red through its pen colour.
        let pen_red = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(pen_red).unwrap();
        s.set_operation(Operation::Pen);
        s.set_pen(Pen::named("Red", PenSize::Fine));

        assert_eq!(same(&d, &[red1], Match::Colour), vec![red1, red2, pen_red], "every visible unlocked red, pen layer included");
    }

    #[test]
    fn two_different_colours_report_mixed_and_select_nothing_useful() {
        let mut d = Design::default();
        let red = square_at(&mut d, 0.0, 0.0, Color32::RED);
        let blue = square_at(&mut d, 2.0, 0.0, Color32::BLUE);
        assert!(mixed(&d, &[red, blue], Match::Colour), "two colours are mixed");
        assert!(!mixed(&d, &[red], Match::Colour), "one shape is never mixed");
    }

    #[test]
    fn inverse_is_the_selectable_rest_and_leaves_out_a_locked_shape() {
        let mut d = Design::default();
        let a = square_at(&mut d, 0.0, 0.0, Color32::RED);
        let b = square_at(&mut d, 2.0, 0.0, Color32::RED);
        let c = square_at(&mut d, 4.0, 0.0, Color32::RED);
        let dd = square_at(&mut d, 6.0, 0.0, Color32::RED);
        assert_eq!(inverse(&d, &[a, b]), vec![c, dd]);
        d.shape_mut(dd).unwrap().locked = true;
        assert_eq!(inverse(&d, &[a, b]), vec![c], "a locked shape is not selectable");
    }

    #[test]
    fn cmd_a_inside_a_container_picks_its_children_then_everything() {
        let mut d = Design::default();
        let outside = square_at(&mut d, 0.0, 0.0, Color32::RED);
        let seed = square_at(&mut d, 2.0, 0.0, Color32::RED);
        let child1 = square_at(&mut d, 4.0, 0.0, Color32::RED);
        let child2 = square_at(&mut d, 6.0, 0.0, Color32::RED);
        // `contain` turns `seed` into the container; the other two move inside it.
        let c = d.contain(&[seed]).unwrap();
        d.reparent(&[child1, child2], Some(c));
        let kids = select_all(&d, &[child1]);
        assert_eq!(kids.len(), 2, "first Cmd+A picks the container's children");
        assert!(kids.contains(&child1) && kids.contains(&child2));
        let everything = select_all(&d, &kids);
        assert!(everything.contains(&outside) && everything.contains(&c), "a second Cmd+A widens to everything");
    }

    #[test]
    fn a_lasso_takes_a_node_inside_it_and_skips_a_frame_that_only_overlaps() {
        let mut d = Design::default();
        let small = square_at(&mut d, 1.0, 1.0, Color32::RED); // 1 x 1 at (1,1), corners inside the lasso
        // A large square around the lasso: its frame overlaps, but its corners are all well outside.
        let big = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(big).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in) = (-2.0, -2.0, 8.0, 8.0);

        let poly: Vec<Pt> = vec![(0.5, 0.5), (2.2, 0.5), (2.2, 2.2), (0.5, 2.2)];
        assert!(in_lasso(&poly, (1.0, 1.0)), "a corner of the small square is inside");
        let hits = lasso_hits(&d, &poly);
        assert!(hits.contains(&small), "the small square has nodes inside the lasso");
        assert!(!hits.contains(&big), "the big square only overlaps the lasso; its nodes are all outside");
    }

    #[test]
    fn tab_walks_the_selectable_shapes_in_order() {
        let mut d = Design::default();
        let a = square_at(&mut d, 0.0, 0.0, Color32::RED);
        let b = square_at(&mut d, 2.0, 0.0, Color32::RED);
        let c = square_at(&mut d, 4.0, 0.0, Color32::RED);
        assert_eq!(walk(&d, &[a], true), vec![b]);
        assert_eq!(walk(&d, &[c], true), vec![a], "forward wraps round");
        assert_eq!(walk(&d, &[a], false), vec![c], "back wraps round");
        assert_eq!(walk(&d, &[], true), vec![a], "from nothing, the first");
    }
}

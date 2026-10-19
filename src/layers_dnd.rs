//! Layers drag and drop as pure design operations: `plan_drop` works out what a drop would do, and
//! `apply_drop` does it. The Layers panel only turns the pointer into a `DropTarget`.

use crate::design::Design;
use crate::links::{Link, LinkKind};

/// Where the dragged rows would land, named by the rows around the drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// The insertion line between two neighbouring rows of one level, by their ids: a shape id, or a
    /// link id for a Group or Attach parent row. `None` is that level's top or bottom end, and two `None`s
    /// are the top level of an empty list.
    Between { above: Option<u64>, below: Option<u64> },
    /// Onto a parent row (a link id, or the id of a shape that holds children), joining it at the top of
    /// its members.
    Into(u64),
}

/// Where dropped objects end up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    /// Loose in the list. A link dragged whole stays linked; a member dragged on its own leaves its link.
    Top,
    Link(Link),
    /// Loose inside this shape that holds children (a container or a Compound), as `Top` is loose at the
    /// top level.
    Container(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    /// The dragged shapes and their descendants, back first.
    pub ids: Vec<u64>,
    /// Every shape id in its new z-order, back first.
    pub order: Vec<u64>,
    pub parent: Parent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropPlan {
    /// Not allowed: the pointer shows not-allowed and releasing does nothing.
    Invalid,
    /// Allowed, but it leaves the design as it is.
    Unchanged,
    Move(Move),
}

/// What dropping `dragged` on `target` would do. The dragged shapes keep their relative order and land
/// as one run at the target; dropping inside a link joins it, and dropping at the top level leaves any link
/// that was not dragged whole.
pub fn plan_drop(design: &Design, dragged: &[u64], target: DropTarget) -> DropPlan {
    let ids = design.with_descendants(&design.roots(dragged));
    let Some((parent, pivot)) = resolve(design, target) else {
        return DropPlan::Invalid;
    };
    let into_itself = match parent {
        Parent::Top => false,
        Parent::Link(link) => members(design, link.id).iter().all(|m| ids.contains(m)),
        Parent::Container(c) => ids.contains(&c),
    };
    if ids.is_empty() || into_itself {
        return DropPlan::Invalid;
    }
    let mut order: Vec<u64> = design.shapes.iter().map(|s| s.id).filter(|id| !ids.contains(id)).collect();
    let at = design.shapes[..pivot].iter().filter(|s| !ids.contains(&s.id)).count();
    order.splice(at..at, ids.iter().copied());
    let link_of = |id: u64| design.shapes.iter().find(|s| s.id == id).and_then(|s| s.link);
    let roots = design.roots(&ids);
    let changes_container = roots.iter().any(|&id| design.parent_of(id) != scope(design, parent));
    let regroups = changes_container
        || match parent {
            Parent::Top | Parent::Container(_) => !split_links(design, &ids).is_empty(),
            Parent::Link(link) => roots.iter().any(|&id| link_of(id) != Some(link)),
        };
    if !regroups && order.iter().eq(design.shapes.iter().map(|s| &s.id)) {
        return DropPlan::Unchanged;
    }
    DropPlan::Move(Move { ids, order, parent })
}

/// Reorders and relinks the design as `plan` says, through the same link operations as the Edit menu.
pub fn apply_drop(design: &mut Design, plan: &DropPlan) {
    let DropPlan::Move(Move { ids, order, parent }) = plan else {
        return;
    };
    let roots = design.roots(ids);
    let container = scope(design, *parent);
    let mut shapes = std::mem::take(&mut design.shapes);
    design.shapes = order.iter().filter_map(|id| shapes.iter().position(|s| s.id == *id).map(|i| shapes.swap_remove(i))).collect();
    match *parent {
        Parent::Top | Parent::Container(_) => {
            for link in split_links(design, ids) {
                let (leaving, rest): (Vec<u64>, Vec<u64>) = members(design, link.id).into_iter().partition(|id| ids.contains(id));
                if rest.len() >= 2 {
                    relink(design, &rest, link);
                }
                design.unlink(&leaving);
            }
        }
        Parent::Link(link) => {
            if roots.iter().any(|&id| design.shapes.iter().any(|s| s.id == id && s.link != Some(link))) {
                let all: Vec<u64> = members(design, link.id).into_iter().chain(roots.iter().copied()).collect();
                relink(design, &all, link);
            }
        }
    }
    for s in design.shapes.iter_mut().filter(|s| roots.contains(&s.id)) {
        s.parent = container;
    }
    design.settle(None);
}

/// The container a drop under `parent` lands in. Joining a link takes that link's container.
fn scope(design: &Design, parent: Parent) -> Option<u64> {
    match parent {
        Parent::Top => None,
        Parent::Container(c) => Some(c),
        Parent::Link(link) => design.shapes.iter().find(|s| s.link == Some(link)).and_then(|s| s.parent),
    }
}

/// The parent a target drops into, and the z index the dragged shapes land below: the back of the row above
/// the line, or just in front of the row below it and everything listed under that row.
fn resolve(design: &Design, target: DropTarget) -> Option<(Parent, usize)> {
    let span = |id: u64| -> Option<(usize, usize)> {
        let rows: Vec<u64> = design.with_descendants(&[id]).into_iter().chain(members(design, id)).collect();
        let at: Vec<usize> = design.shapes.iter().enumerate().filter(|(_, s)| rows.contains(&s.id)).map(|(i, _)| i).collect();
        Some((*at.first()?, *at.last()?))
    };
    let loose_in = |parent: Option<u64>| parent.map_or(Parent::Top, Parent::Container);
    match target {
        DropTarget::Between { above, below } => {
            // A shape row is at its link's level, else its container's; a link row is at its members' container's.
            let level = |id: Option<u64>| {
                id.map(|id| match design.shapes.iter().find(|s| s.id == id) {
                    Some(s) => s.link.map_or(loose_in(s.parent), Parent::Link),
                    None => loose_in(design.shapes.iter().find(|s| s.link.is_some_and(|l| l.id == id)).and_then(|s| s.parent)),
                })
            };
            let parent = match (level(above), level(below)) {
                (Some(a), Some(b)) if a != b => return None,
                (a, b) => a.or(b).unwrap_or(Parent::Top),
            };
            let pivot = match (above, below) {
                (Some(id), _) => span(id)?.0,
                (None, Some(id)) => span(id)?.1 + 1,
                (None, None) => design.shapes.len(),
            };
            Some((parent, pivot))
        }
        DropTarget::Into(id) => match design.shapes.iter().find_map(|s| s.link.filter(|l| l.id == id)) {
            Some(link) => Some((Parent::Link(link), span(id)?.1 + 1)),
            None if design.shapes.iter().any(|s| s.id == id && s.holds_children()) => Some((Parent::Container(id), span(id)?.1 + 1)),
            None => None,
        },
    }
}

/// The links that `ids` drag only some members of, so dropping at the top level splits them.
fn split_links(design: &Design, ids: &[u64]) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    for link in design.shapes.iter().filter(|s| ids.contains(&s.id)).filter_map(|s| s.link) {
        if !out.contains(&link) && !members(design, link.id).iter().all(|m| ids.contains(m)) {
            out.push(link);
        }
    }
    out
}

fn members(design: &Design, link: u64) -> Vec<u64> {
    design.shapes.iter().filter(|s| s.link.is_some_and(|l| l.id == link)).map(|s| s.id).collect()
}

fn relink(design: &mut Design, ids: &[u64], like: Link) {
    match like.kind {
        LinkKind::Group => design.group(ids),
        LinkKind::Attach => design.attach(ids),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn design(n: usize) -> Design {
        let mut d = Design::default();
        for _ in 0..n {
            d.add_shape(ShapeKind::Square);
        }
        d
    }

    fn order(d: &Design) -> Vec<u64> {
        d.shapes.iter().map(|s| s.id).collect()
    }

    fn link_of(d: &Design, id: u64) -> Option<Link> {
        d.shapes.iter().find(|s| s.id == id).unwrap().link
    }

    fn drop(d: &mut Design, dragged: &[u64], target: DropTarget) -> DropPlan {
        let plan = plan_drop(d, dragged, target);
        apply_drop(d, &plan);
        plan
    }

    fn between(above: Option<u64>, below: Option<u64>) -> DropTarget {
        DropTarget::Between { above, below }
    }

    #[test]
    fn dropping_a_row_above_another_moves_it_in_front() {
        let mut d = design(3);
        drop(&mut d, &[1], between(None, Some(3)));
        assert_eq!(order(&d), vec![2, 3, 1], "the bottom row goes to the top of the list");
        drop(&mut d, &[3], between(Some(2), None));
        assert_eq!(order(&d), vec![3, 2, 1], "and back down to the bottom");
    }

    #[test]
    fn a_multi_selection_lands_together_keeping_its_order() {
        let mut d = design(5);
        drop(&mut d, &[4, 1], between(Some(3), Some(2)));
        assert_eq!(order(&d), vec![2, 1, 4, 3, 5]);
    }

    #[test]
    fn dropping_on_a_parent_row_joins_its_link_at_the_top() {
        let mut d = design(3);
        d.group(&[1, 2]).unwrap();
        let group = link_of(&d, 1).unwrap();
        let plan = drop(&mut d, &[3], DropTarget::Into(group.id));
        assert!(matches!(plan, DropPlan::Move(Move { parent: Parent::Link(l), .. }) if l == group));
        let links: Vec<_> = (1..=3).map(|id| link_of(&d, id)).collect();
        assert!(
            links.iter().all(|l| l.is_some_and(|l| l.kind == LinkKind::Group)) && links[0] == links[1] && links[1] == links[2],
            "all three share one Group: {links:?}"
        );
        assert_eq!(order(&d), vec![1, 2, 3], "the new member is the top of the Group");
    }

    #[test]
    fn dropping_between_members_joins_the_link_there() {
        let mut d = design(4);
        d.attach(&[1, 2, 3]).unwrap();
        drop(&mut d, &[4], between(Some(2), Some(1)));
        assert_eq!(order(&d), vec![1, 4, 2, 3]);
        let kinds: Vec<_> = (1..=4).map(|id| link_of(&d, id).map(|l| l.kind)).collect();
        assert_eq!(kinds, vec![Some(LinkKind::Attach); 4]);
        assert_eq!(link_of(&d, 4), link_of(&d, 1));
    }

    #[test]
    fn dropping_a_member_at_the_top_level_takes_it_out_of_the_link() {
        let mut d = design(4);
        d.attach(&[1, 2, 3]).unwrap();
        drop(&mut d, &[2], between(None, Some(4)));
        assert_eq!(order(&d), vec![1, 3, 4, 2]);
        assert_eq!(link_of(&d, 2), None);
        assert_eq!(link_of(&d, 1).map(|l| l.kind), Some(LinkKind::Attach), "the other two stay attached");
        assert_eq!(link_of(&d, 1), link_of(&d, 3));
    }

    #[test]
    fn dragging_out_the_second_to_last_member_dissolves_the_link() {
        let mut d = design(3);
        d.group(&[1, 2]).unwrap();
        drop(&mut d, &[2], between(None, Some(3)));
        assert_eq!(order(&d), vec![1, 3, 2]);
        assert!(d.shapes.iter().all(|s| s.link.is_none()), "as Ungroup does");
    }

    #[test]
    fn a_whole_link_moves_as_a_block_and_stays_linked() {
        let mut d = design(4);
        let group = d.group(&[1, 2]).unwrap();
        drop(&mut d, &[1, 2], between(None, Some(4)));
        assert_eq!(order(&d), vec![3, 4, 1, 2]);
        assert_eq!(link_of(&d, 1).map(|l| l.id), Some(group), "the link keeps its id");
        assert_eq!(link_of(&d, 2).map(|l| l.id), Some(group));
    }

    #[test]
    fn a_no_op_drop_leaves_the_design_unchanged() {
        let mut d = design(3);
        d.group(&[2, 3]).unwrap();
        let link = link_of(&d, 2).unwrap().id;
        let before = d.clone();
        for (dragged, target) in [
            (vec![1], between(Some(link), None)),
            (vec![1], between(Some(1), None)),
            (vec![3], between(None, Some(3))),
            (vec![2, 3], between(None, Some(link))),
            (vec![2], between(Some(3), None)),
        ] {
            assert_eq!(drop(&mut d, &dragged, target), DropPlan::Unchanged, "{dragged:?} to {target:?}");
            assert_eq!(d, before);
        }
    }

    #[test]
    fn reordering_inside_a_link_keeps_its_id() {
        let mut d = design(3);
        let group = d.group(&[1, 2, 3]).unwrap();
        drop(&mut d, &[1], between(None, Some(3)));
        assert_eq!(order(&d), vec![2, 3, 1]);
        assert!(d.shapes.iter().all(|s| s.link.map(|l| l.id) == Some(group)));
    }

    #[test]
    fn a_link_cannot_drop_into_itself() {
        let mut d = design(3);
        let group = d.group(&[1, 2]).unwrap();
        assert_eq!(plan_drop(&d, &[1, 2], DropTarget::Into(group)), DropPlan::Invalid);
        assert_eq!(plan_drop(&d, &[1, 2], between(Some(2), Some(1))), DropPlan::Invalid);
    }

    #[test]
    fn rows_from_two_levels_or_unknown_ids_are_not_a_target() {
        let mut d = design(3);
        d.group(&[1, 2]).unwrap();
        assert_eq!(plan_drop(&d, &[3], between(Some(1), Some(3))), DropPlan::Invalid, "a member above, a loose row below");
        assert_eq!(plan_drop(&d, &[3], between(Some(99), None)), DropPlan::Invalid);
        assert_eq!(plan_drop(&d, &[99], between(None, Some(3))), DropPlan::Invalid, "nothing dragged");
    }

    #[test]
    fn a_shape_row_takes_no_drops_inside_it() {
        let d = design(2);
        assert_eq!(plan_drop(&d, &[1], DropTarget::Into(2)), DropPlan::Invalid);
    }

    #[test]
    fn joining_a_link_pulls_members_out_of_their_old_one() {
        let mut d = design(5);
        d.group(&[1, 2]).unwrap();
        d.attach(&[3, 4]).unwrap();
        let attach = link_of(&d, 3).unwrap();
        drop(&mut d, &[2], DropTarget::Into(attach.id));
        assert_eq!(link_of(&d, 1), None, "the Group left with one member dissolves");
        assert_eq!(link_of(&d, 2).map(|l| l.kind), Some(LinkKind::Attach));
        assert_eq!(link_of(&d, 2), link_of(&d, 3));
        assert_eq!(order(&d), vec![1, 3, 4, 2, 5]);
    }

    fn parent_of(d: &Design, id: u64) -> Option<u64> {
        d.shapes.iter().find(|s| s.id == id).unwrap().parent
    }

    #[test]
    fn dropping_on_a_container_row_makes_a_child_at_the_top_of_its_children() {
        let mut d = design(3);
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2], Some(c));
        drop(&mut d, &[3], DropTarget::Into(c));
        assert_eq!(parent_of(&d, 3), Some(c));
        assert_eq!(order(&d), vec![1, 2, 3], "listed first under the container");
        assert_eq!(plan_drop(&d, &[3], DropTarget::Into(c)), DropPlan::Unchanged, "already there");
    }

    #[test]
    fn dropping_at_the_top_level_takes_a_child_out_of_its_container() {
        let mut d = design(3);
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2], Some(c));
        drop(&mut d, &[2], between(None, Some(3)));
        assert_eq!(parent_of(&d, 2), None);
        assert_eq!(order(&d), vec![1, 3, 2]);
    }

    #[test]
    fn dropping_between_children_lands_inside_the_container() {
        let mut d = design(4);
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2, 3], Some(c));
        drop(&mut d, &[4], between(Some(3), Some(2)));
        assert_eq!(parent_of(&d, 4), Some(c));
        assert_eq!(order(&d), vec![1, 2, 4, 3]);
    }

    #[test]
    fn dragging_a_container_carries_its_children_and_refuses_its_own_subtree() {
        let mut d = design(4);
        let c = d.contain(&[2]).unwrap();
        d.reparent(&[3], Some(c));
        drop(&mut d, &[c], between(None, Some(4)));
        assert_eq!(order(&d), vec![1, 4, 2, 3], "the child moves with its container");
        assert_eq!(plan_drop(&d, &[c], DropTarget::Into(c)), DropPlan::Invalid);
        let inner = d.contain(&[3]).unwrap();
        assert_eq!(plan_drop(&d, &[c], DropTarget::Into(inner)), DropPlan::Invalid, "nor into a descendant");
    }

    #[test]
    fn joining_a_link_inside_a_container_takes_the_links_container() {
        let mut d = design(4);
        let c = d.contain(&[1]).unwrap();
        d.reparent(&[2, 3], Some(c));
        let group = d.group(&[2, 3]).unwrap();
        drop(&mut d, &[4], DropTarget::Into(group));
        assert_eq!(parent_of(&d, 4), Some(c));
        assert_eq!(link_of(&d, 4).map(|l| l.id), link_of(&d, 2).map(|l| l.id));
    }

    #[test]
    fn a_dragged_link_and_a_loose_member_at_the_top_level() {
        let mut d = design(6);
        let group = d.group(&[1, 2]).unwrap();
        d.attach(&[3, 4, 5]).unwrap();
        drop(&mut d, &[1, 2, 4], between(None, Some(6)));
        assert_eq!(order(&d), vec![3, 5, 6, 1, 2, 4]);
        assert_eq!(link_of(&d, 1).map(|l| l.id), Some(group));
        assert_eq!(link_of(&d, 4), None);
        assert_eq!(link_of(&d, 3), link_of(&d, 5));
        assert!(link_of(&d, 3).is_some());
    }
}

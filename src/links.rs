//! Group and Attach: one level of linking between shapes, stored on each member as `Shape::link`.
//!
//! Invariants, kept only by the methods here:
//! 1. Every shape sharing a `link.id` has the same `kind`.
//! 2. A link with fewer than two members is removed (`prune_links`).
//! 3. Link ids come from `Design::next_id`, the shape id counter, so they never collide.

use serde::{Deserialize, Serialize};

use crate::design::{Design, Shape};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    /// Moves and selects together; each member is placed on the mat on its own at Make time.
    Group,
    /// As Group, and the members keep their relative positions on the mat.
    Attach,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Link {
    pub id: u64,
    pub kind: LinkKind,
}

impl LinkKind {
    pub fn label(self) -> &'static str {
        match self {
            LinkKind::Group => "Group",
            LinkKind::Attach => "Attached",
        }
    }
}

impl Design {
    pub fn group(&mut self, ids: &[u64]) -> Option<u64> {
        self.link(ids, LinkKind::Group)
    }

    pub fn attach(&mut self, ids: &[u64]) -> Option<u64> {
        self.link(ids, LinkKind::Attach)
    }

    /// Gives `ids` one new link, pulling them out of any link they were in. The members move together in
    /// the z-order to sit at the top-most member's slot. Returns `None` when fewer than two shapes match
    /// or any of them is a container (containers are never link members; see `containers.rs`).
    fn link(&mut self, ids: &[u64], kind: LinkKind) -> Option<u64> {
        let picked = || self.shapes.iter().filter(|s| ids.contains(&s.id));
        if picked().count() < 2 || picked().any(|s| s.container.is_some()) {
            return None;
        }
        let link = Link { id: self.next_id(), kind };
        let top = self.shapes.iter().rposition(|s| ids.contains(&s.id))?;
        let below_top = self.shapes[..top].iter().filter(|s| !ids.contains(&s.id)).count();
        let (mut members, mut rest): (Vec<Shape>, Vec<Shape>) = self.shapes.drain(..).partition(|s| ids.contains(&s.id));
        for s in &mut members {
            s.link = Some(link);
        }
        rest.splice(below_top..below_top, members);
        self.shapes = rest;
        self.prune_links();
        Some(link.id)
    }

    /// Removes the link from every shape that shares a link with any of `ids`.
    pub fn unlink(&mut self, ids: &[u64]) {
        let links = self.link_ids(ids);
        for s in &mut self.shapes {
            if s.link.is_some_and(|l| links.contains(&l.id)) {
                s.link = None;
            }
        }
    }

    /// `ids` plus every shape sharing a link with one of them, in z-order.
    pub fn expand(&self, ids: &[u64]) -> Vec<u64> {
        let links = self.link_ids(ids);
        self.shapes.iter().filter(|s| ids.contains(&s.id) || s.link.is_some_and(|l| links.contains(&l.id))).map(|s| s.id).collect()
    }

    /// The shapes among `ids` whose link is of `kind`.
    pub fn members_of_kind(&self, ids: &[u64], kind: LinkKind) -> Vec<u64> {
        self.shapes.iter().filter(|s| ids.contains(&s.id) && s.link.is_some_and(|l| l.kind == kind)).map(|s| s.id).collect()
    }

    /// Drops every link left with fewer than two members, and the names of links that are gone.
    pub fn prune_links(&mut self) {
        let mut members: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        for link in self.shapes.iter().filter_map(|s| s.link) {
            *members.entry(link.id).or_default() += 1;
        }
        for s in &mut self.shapes {
            if s.link.is_some_and(|l| members[&l.id] < 2) {
                s.link = None;
            }
        }
        let live: std::collections::HashSet<u64> =
            self.shapes.iter().chain(self.components.iter().flat_map(|c| &c.shapes)).filter_map(|s| s.link).map(|l| l.id).collect();
        self.link_names.retain(|id, _| live.contains(id));
    }

    /// Gives freshly duplicated `copies` new link ids of the same kind, so copies of one link form a new
    /// link rather than joining the original.
    pub fn relink_copies(&mut self, copies: &[u64]) {
        let mut next = self.next_id();
        let mut fresh: Vec<(u64, u64)> = Vec::new();
        for s in self.shapes.iter_mut().filter(|s| copies.contains(&s.id)) {
            if let Some(link) = s.link.as_mut() {
                let id = match fresh.iter().find(|(old, _)| *old == link.id) {
                    Some(&(_, new)) => new,
                    None => {
                        fresh.push((link.id, next));
                        next += 1;
                        next - 1
                    }
                };
                link.id = id;
            }
        }
        self.copy_link_names(&fresh);
        self.prune_links();
    }

    fn link_ids(&self, ids: &[u64]) -> Vec<u64> {
        self.shapes.iter().filter(|s| ids.contains(&s.id)).filter_map(|s| s.link).map(|l| l.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Arrange, Combine, History, ShapeKind};

    fn design(n: usize) -> Design {
        let mut d = Design::default();
        for _ in 0..n {
            d.add_shape(ShapeKind::Square);
        }
        d
    }

    fn links(d: &Design) -> Vec<Option<Link>> {
        d.shapes.iter().map(|s| s.link).collect()
    }

    fn order(d: &Design) -> Vec<u64> {
        d.shapes.iter().map(|s| s.id).collect()
    }

    #[test]
    fn group_links_every_member_with_one_kind() {
        let mut d = design(3);
        let id = d.group(&[1, 3]).unwrap();
        let g = Some(Link { id, kind: LinkKind::Group });
        assert_eq!(d.shapes.iter().filter(|s| s.link == g).count(), 2);
        assert!(d.shape_mut(2).unwrap().link.is_none());
    }

    #[test]
    fn attach_uses_the_attach_kind() {
        let mut d = design(2);
        let id = d.attach(&[1, 2]).unwrap();
        assert_eq!(links(&d), vec![Some(Link { id, kind: LinkKind::Attach }); 2]);
    }

    #[test]
    fn linking_fewer_than_two_shapes_does_nothing() {
        let mut d = design(2);
        let before = d.clone();
        assert_eq!(d.group(&[1]), None);
        assert_eq!(d.attach(&[1, 99]), None);
        assert_eq!(d.group(&[]), None);
        assert_eq!(d, before);
    }

    #[test]
    fn link_ids_never_collide_with_shape_ids() {
        let mut d = design(2);
        let link = d.group(&[1, 2]).unwrap();
        assert!(link > 2, "link id {link} is past the shape ids");
        let next = d.add_shape(ShapeKind::Circle);
        assert!(next > link, "the next shape id {next} skips the link id {link}");
        let again = d.attach(&[1, next]).unwrap();
        assert!(again > next);
    }

    #[test]
    fn regrouping_pulls_members_out_of_their_old_link() {
        let mut d = design(4);
        let old = d.attach(&[1, 2, 3]).unwrap();
        let new = d.group(&[3, 4]).unwrap();
        let kind_of = |d: &Design, id: u64| d.shapes.iter().find(|s| s.id == id).unwrap().link;
        assert_eq!(kind_of(&d, 1), Some(Link { id: old, kind: LinkKind::Attach }));
        assert_eq!(kind_of(&d, 2), Some(Link { id: old, kind: LinkKind::Attach }));
        assert_eq!(kind_of(&d, 3), Some(Link { id: new, kind: LinkKind::Group }));
        assert_eq!(kind_of(&d, 4), Some(Link { id: new, kind: LinkKind::Group }));
    }

    #[test]
    fn regrouping_one_of_a_pair_dissolves_the_old_link() {
        let mut d = design(3);
        d.attach(&[1, 2]).unwrap();
        d.group(&[2, 3]).unwrap();
        assert!(d.shapes.iter().find(|s| s.id == 1).unwrap().link.is_none(), "a one-member link is removed");
    }

    #[test]
    fn linking_gathers_members_at_the_top_member_slot() {
        let mut d = design(4);
        d.group(&[1, 3]).unwrap();
        assert_eq!(order(&d), vec![2, 1, 3, 4]);
    }

    #[test]
    fn unlink_clears_the_whole_link_from_any_member() {
        let mut d = design(3);
        d.group(&[1, 2]).unwrap();
        d.unlink(&[2]);
        assert_eq!(links(&d), vec![None; 3]);
    }

    #[test]
    fn expand_adds_link_mates_in_z_order() {
        let mut d = design(4);
        d.attach(&[2, 4]).unwrap();
        assert_eq!(d.expand(&[4]), vec![2, 4]);
        assert_eq!(d.expand(&[1]), vec![1]);
        assert_eq!(d.expand(&[4, 1]), vec![1, 2, 4]);
    }

    #[test]
    fn members_of_kind_filters_the_selection() {
        let mut d = design(4);
        d.group(&[1, 2]).unwrap();
        d.attach(&[3, 4]).unwrap();
        assert_eq!(d.members_of_kind(&[1, 3], LinkKind::Group), vec![1]);
        assert_eq!(d.members_of_kind(&[1, 2], LinkKind::Attach), Vec::<u64>::new());
    }

    #[test]
    fn duplicate_gives_copies_a_fresh_link_of_the_same_kind() {
        let mut d = design(2);
        let link = d.attach(&[1, 2]).unwrap();
        let copies = d.duplicate(&[1, 2], 0.25);
        let copy_links: Vec<Link> = d.shapes.iter().filter(|s| copies.contains(&s.id)).map(|s| s.link.unwrap()).collect();
        assert_eq!(copy_links.len(), 2);
        assert_eq!(copy_links[0], copy_links[1]);
        assert_eq!(copy_links[0].kind, LinkKind::Attach);
        assert_ne!(copy_links[0].id, link);
        assert!(!copies.contains(&copy_links[0].id), "the fresh link id is not a shape id");
        assert_eq!(d.expand(&[1]), vec![1, 2], "the originals keep their own link");
    }

    #[test]
    fn duplicating_one_member_leaves_the_copy_unlinked() {
        let mut d = design(2);
        d.group(&[1, 2]).unwrap();
        let copies = d.duplicate(&[1], 0.25);
        assert!(d.shapes.iter().find(|s| s.id == copies[0]).unwrap().link.is_none());
    }

    #[test]
    fn deleting_down_to_one_member_removes_the_link() {
        let mut d = design(3);
        d.group(&[1, 2, 3]).unwrap();
        d.remove_many(&[1]);
        assert!(links(&d).iter().all(Option::is_some), "two members remain linked");
        d.remove_many(&[2]);
        assert_eq!(links(&d), vec![None]);
    }

    #[test]
    fn combine_shrinks_links_and_the_result_is_unlinked() {
        let mut d = design(3);
        for (i, s) in d.shapes.iter_mut().enumerate() {
            s.x_in = i as f32;
        }
        d.group(&[2, 3]).unwrap();
        let result = d.combine(&[1, 2], Combine::Unite)[0];
        assert_eq!(links(&d), vec![None, None, None, None], "shape 3 is alone, so its link is gone");
        assert!(d.shapes.iter().any(|s| s.id == result));
    }

    #[test]
    fn arrange_moves_a_link_as_a_block() {
        let mut d = design(5);
        d.group(&[2, 3]).unwrap();
        d.arrange(&[2], Arrange::Forward);
        assert_eq!(order(&d), vec![1, 4, 2, 3, 5]);
        d.arrange(&[3], Arrange::ToFront);
        assert_eq!(order(&d), vec![1, 4, 5, 2, 3]);
        d.arrange(&[2], Arrange::ToBack);
        assert_eq!(order(&d), vec![2, 3, 1, 4, 5]);
        d.arrange(&[1], Arrange::Backward);
        assert_eq!(order(&d), vec![1, 2, 3, 4, 5], "a single shape steps past a whole block");
    }

    #[test]
    fn arrange_never_splits_a_neighbouring_link() {
        let mut d = design(4);
        d.group(&[1, 2]).unwrap();
        d.attach(&[3, 4]).unwrap();
        d.arrange(&[1], Arrange::Forward);
        assert_eq!(order(&d), vec![3, 4, 1, 2]);
    }

    #[test]
    fn undo_restores_the_previous_links_in_one_step() {
        let mut d = design(2);
        let mut history = History::default();
        let before = d.clone();
        d.attach(&[1, 2]).unwrap();
        history.observe(before.clone(), &d, false);
        history.undo(&mut d);
        assert_eq!(d, before);
    }

    #[test]
    fn a_project_saved_before_links_loads_unlinked() {
        let json = r#"{"id":"pre-links","name":"Before links","design":{"shapes":[{"id":1,"kind":"Square","x_in":0.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]},{"id":2,"kind":"Circle","x_in":3.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contour_hidden":[]}]},"created":1,"modified":1}"#;
        let project: crate::project::Project = serde_json::from_str(json).expect("an old project still loads");
        assert_eq!(project.design.shapes.len(), 2);
        assert!(project.design.shapes.iter().all(|s| s.link.is_none()));
    }

    #[test]
    fn links_round_trip_through_json() {
        let mut d = design(2);
        d.group(&[1, 2]).unwrap();
        let back: Design = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }
}

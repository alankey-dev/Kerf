//! Layer names (spec 40). A shape, or a Group or Attach link, can carry a name the user gave it, shown in
//! Layers, export and the log in place of the label it would otherwise show. A shape keeps its own in
//! `Shape::name`; a link's lives in `Design::link_names`, since a link is stored on each of its members.

use crate::design::{Design, Shape};
use crate::links::Link;

/// What a rename names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Shape(u64),
    Link(u64),
}

impl Design {
    /// What renaming `on` names: its one shape, or the link it is every member of.
    pub fn named(&self, on: &[u64]) -> Option<Named> {
        if let [id] = on {
            return self.shapes.iter().any(|s| s.id == *id).then_some(Named::Shape(*id));
        }
        let link = self.shapes.iter().find(|s| on.first() == Some(&s.id))?.link?;
        let mut members: Vec<u64> = self.shapes.iter().filter(|s| s.link.is_some_and(|l| l.id == link.id)).map(|s| s.id).collect();
        let mut on = on.to_vec();
        members.sort_unstable();
        on.sort_unstable();
        on.dedup();
        (members == on).then_some(Named::Link(link.id))
    }

    /// What `shape` is called: the name the user gave it, else an instance's component name, else its label.
    pub fn layer_name(&self, shape: &Shape) -> String {
        if let Some(name) = &shape.name {
            return name.clone();
        }
        match shape.instance_of() {
            Some(c) => self.component(c).map_or_else(|| "Missing component".to_owned(), |c| c.name.clone()),
            None => shape.kind.label().to_owned(),
        }
    }

    /// What `link`'s parent row is called: the name the user gave it, else "Group" or "Attached".
    pub fn link_name(&self, link: Link) -> String {
        self.link_names.get(&link.id).cloned().unwrap_or_else(|| link.kind.label().to_owned())
    }

    /// The name `target` shows now; `None` once it is gone.
    pub fn name_of(&self, target: Named) -> Option<String> {
        match target {
            Named::Shape(id) => self.shapes.iter().find(|s| s.id == id).map(|s| self.layer_name(s)),
            Named::Link(id) => self.shapes.iter().find_map(|s| s.link.filter(|l| l.id == id)).map(|l| self.link_name(l)),
        }
    }

    /// Gives `target` `name`, trimmed; a blank or absent name gives it back its label.
    pub(crate) fn rename(&mut self, target: Named, name: Option<&str>) {
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
        match target {
            Named::Shape(id) => {
                if let Some(s) = self.shape_mut(id) {
                    s.name = name;
                }
            }
            Named::Link(id) => match name {
                Some(name) => _ = self.link_names.insert(id, name),
                None => _ = self.link_names.remove(&id),
            },
        }
    }

    /// Gives each new link id the name of the old one it copies, for `(old, new)` id pairs.
    pub(crate) fn copy_link_names(&mut self, ids: &[(u64, u64)]) {
        for (old, new) in ids {
            if let Some(name) = self.link_names.get(old).cloned() {
                self.link_names.insert(*new, name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;

    fn squares(n: usize) -> Design {
        let mut d = Design::default();
        for _ in 0..n {
            d.add_shape(ShapeKind::Square);
        }
        d
    }

    #[test]
    fn one_shape_or_a_whole_link_is_named_and_anything_else_is_not() {
        let mut d = squares(3);
        let link = d.group(&[1, 2]).unwrap();
        assert_eq!(d.named(&[3]), Some(Named::Shape(3)));
        assert_eq!(d.named(&[1]), Some(Named::Shape(1)), "a member on its own row is a shape");
        assert_eq!(d.named(&[2, 1]), Some(Named::Link(link)));
        assert_eq!(d.named(&[1, 3]), None);
        assert_eq!(d.named(&[1, 2, 3]), None);
        assert_eq!(d.named(&[]), None);
        assert_eq!(d.named(&[99]), None);
    }

    #[test]
    fn a_name_shows_in_place_of_the_label_and_a_blank_one_gives_the_label_back() {
        let mut d = squares(2);
        let link = d.group(&[1, 2]).unwrap();
        d.rename(Named::Shape(1), Some("  Speech Bubble  "));
        d.rename(Named::Link(link), Some("Badge"));
        assert_eq!(d.name_of(Named::Shape(1)).unwrap(), "Speech Bubble", "trimmed");
        assert_eq!(d.name_of(Named::Link(link)).unwrap(), "Badge");
        d.rename(Named::Shape(1), Some(" "));
        d.rename(Named::Link(link), None);
        assert_eq!(d.name_of(Named::Shape(1)).unwrap(), "Square");
        assert_eq!(d.name_of(Named::Link(link)).unwrap(), "Group");
        assert!(d.shapes[0].name.is_none() && d.link_names.is_empty());
    }

    #[test]
    fn a_duplicated_group_keeps_its_name_under_its_new_id() {
        let mut d = squares(2);
        let link = d.group(&[1, 2]).unwrap();
        d.rename(Named::Link(link), Some("Badge"));
        let copies = d.duplicate(&[1, 2], 0.25);
        let copy = d.named(&copies).unwrap();
        assert_ne!(copy, Named::Link(link));
        assert_eq!(d.name_of(copy).unwrap(), "Badge");
    }

    #[test]
    fn a_named_group_keeps_its_name_through_a_component_its_copy_and_detaching() {
        let mut d = squares(2);
        let link = d.group(&[1, 2]).unwrap();
        d.rename(Named::Link(link), Some("Badge"));
        let instance = d.make_component(&[1, 2]).unwrap();
        let copy = d.duplicate_component(d.components[0].id).unwrap();
        let copied = d.component(copy).unwrap().shapes.iter().find_map(|s| s.link).unwrap();
        assert_ne!(copied.id, link);
        assert_eq!(d.link_name(copied), "Badge", "the copy's master keeps the name");
        let roots = d.detach_instances(&[instance]);
        assert_eq!(d.named(&roots).and_then(|t| d.name_of(t)).as_deref(), Some("Badge"), "and so do the detached shapes");
    }

    #[test]
    fn an_ungrouped_name_goes_and_never_reaches_a_later_group() {
        let mut d = squares(2);
        let link = d.group(&[1, 2]).unwrap();
        d.rename(Named::Link(link), Some("Badge"));
        d.unlink(&[1, 2]);
        let again = d.group(&[1, 2]).unwrap();
        assert_ne!(again, link, "a named link's id is not handed out again");
        assert_eq!(d.name_of(Named::Link(again)).unwrap(), "Group");
        assert!(!d.link_names.contains_key(&link), "and grouping again prunes the old name");
    }
}

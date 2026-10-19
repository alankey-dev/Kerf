//! Ruler guides (spec 45): lines the user pulls out of a ruler and keeps. They live in the design, so
//! they save, autosave and undo with it, but nothing that makes or exports a design reads them.
//! This module is pure: the model, hit testing and the rules for where a released drag lands. The
//! canvas side is `ui::guides`.

use crate::design::{Design, Pt};
use crate::edit::Blocked;

/// How close, in screen pixels, the pointer must come to a guide to grab it.
pub const HIT_PX: f32 = 4.0;

/// Which way a guide runs. A `Vertical` guide sits at a world x, a `Horizontal` one at a world y.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    /// The coordinate of `p` this axis measures.
    pub fn of(self, p: Pt) -> f32 {
        match self {
            Self::Vertical => p.0,
            Self::Horizontal => p.1,
        }
    }
}

/// One guide, spanning the whole artboard.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct RulerGuide {
    pub id: u64,
    pub axis: Axis,
    pub at_in: f32,
    #[serde(default)]
    pub locked: bool,
}

/// Every change to a design's guides, each one undo step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuideEdit {
    Add {
        axis: Axis,
        at_in: f32,
    },
    Move {
        id: u64,
        at_in: f32,
    },
    SetLocked {
        id: u64,
        locked: bool,
    },
    Delete(u64),
    /// Locks every guide, or unlocks them all, each guide's own lock included.
    LockAll,
    /// Removes every guide.
    Clear,
    /// Shows or hides every guide.
    Show,
}

/// Where a guide dragged with the pointer ends up when it is released.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Release {
    /// Over the artboard: the guide lands at this position.
    Land(f32),
    /// Back over the ruler it came from: a new guide is cancelled, a moved one deleted.
    OnRuler,
    /// Anywhere else: nothing changes.
    Away,
}

/// What a guide on `axis` released at `pointer` does, on an artboard `board_in` in size. A horizontal
/// guide comes from the top ruler, a vertical one from the left ruler, so anything above or left of the
/// artboard counts as back on its ruler.
pub fn release(axis: Axis, pointer: Pt, board_in: Pt) -> Release {
    let (along, across, board_along, board_across) = match axis {
        Axis::Horizontal => (pointer.1, pointer.0, board_in.1, board_in.0),
        Axis::Vertical => (pointer.0, pointer.1, board_in.0, board_in.1),
    };
    if along < 0.0 && (0.0..=board_across).contains(&across) {
        Release::OnRuler
    } else if (0.0..=board_along).contains(&along) && (0.0..=board_across).contains(&across) {
        Release::Land(along)
    } else {
        Release::Away
    }
}

/// The unlocked guide nearest `pointer` within [`HIT_PX`] at `px_per_in`, if any. A locked guide is
/// never hit, nor any guide while `all_locked`, so clicks pass through to the shapes beneath.
pub fn hit(guides: &[RulerGuide], all_locked: bool, pointer: Pt, px_per_in: f32) -> Option<u64> {
    if all_locked {
        return None;
    }
    let reach = HIT_PX / px_per_in;
    guides
        .iter()
        .filter(|g| !g.locked)
        .map(|g| (g.id, (g.axis.of(pointer) - g.at_in).abs()))
        .filter(|&(_, d)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

impl Design {
    /// The guides that snap and draw: none while they are hidden.
    pub fn shown_guides(&self) -> &[RulerGuide] {
        if self.guides_hidden { &[] } else { &self.guides }
    }

    pub fn guide(&self, id: u64) -> Option<&RulerGuide> {
        self.guides.iter().find(|g| g.id == id)
    }

    /// Whether `edit` would do anything, and if not, why.
    pub(crate) fn check_guide(&self, edit: &GuideEdit) -> Result<(), Blocked> {
        let unlocked = |id: u64| match self.guide(id) {
            None => Err(Blocked::NoGuide),
            Some(g) if g.locked || self.guides_locked => Err(Blocked::GuideLocked),
            Some(_) => Ok(()),
        };
        match *edit {
            GuideEdit::Add { at_in, .. } => {
                if at_in.is_finite() {
                    Ok(())
                } else {
                    Err(Blocked::NoGuide)
                }
            }
            GuideEdit::Move { id, at_in } if at_in.is_finite() => unlocked(id),
            GuideEdit::Move { .. } => Err(Blocked::NoGuide),
            GuideEdit::Delete(id) => unlocked(id),
            GuideEdit::SetLocked { id, .. } => self.guide(id).map(|_| ()).ok_or(Blocked::NoGuide),
            GuideEdit::LockAll | GuideEdit::Show => Ok(()),
            GuideEdit::Clear => {
                if self.guides.is_empty() {
                    Err(Blocked::NoGuides)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Runs a guide edit whose check has passed.
    pub(crate) fn apply_guide(&mut self, edit: &GuideEdit) {
        match *edit {
            GuideEdit::Add { axis, at_in } => {
                let id = self.guides.iter().map(|g| g.id).max().map_or(1, |m| m + 1);
                self.guides.push(RulerGuide { id, axis, at_in, locked: false });
                self.guides_hidden = false;
            }
            GuideEdit::Move { id, at_in } => {
                if let Some(g) = self.guides.iter_mut().find(|g| g.id == id) {
                    g.at_in = at_in;
                }
            }
            GuideEdit::SetLocked { id, locked } => {
                if let Some(g) = self.guides.iter_mut().find(|g| g.id == id) {
                    g.locked = locked;
                }
            }
            GuideEdit::Delete(id) => self.guides.retain(|g| g.id != id),
            GuideEdit::LockAll => {
                self.guides_locked = !self.guides_locked;
                // Unlocking all is also the way back to a guide locked on its own, which no click reaches.
                if !self.guides_locked {
                    self.guides.iter_mut().for_each(|g| g.locked = false);
                }
            }
            GuideEdit::Clear => self.guides.clear(),
            GuideEdit::Show => self.guides_hidden = !self.guides_hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;

    fn guide(id: u64, axis: Axis, at_in: f32) -> RulerGuide {
        RulerGuide { id, axis, at_in, locked: false }
    }

    #[test]
    fn a_guide_is_hit_within_four_pixels_and_a_locked_one_never() {
        let guides = [guide(1, Axis::Vertical, 3.0)];
        assert_eq!(hit(&guides, false, (3.02, 5.0), 72.0), Some(1));
        assert_eq!(hit(&guides, false, (3.2, 5.0), 72.0), None);
        assert_eq!(hit(&guides, false, (5.0, 3.02), 72.0), None, "a vertical guide measures x only");
        assert_eq!(hit(&guides, true, (3.0, 5.0), 72.0), None, "Lock guides passes every click through");
        let locked = [RulerGuide { locked: true, ..guides[0] }];
        assert_eq!(hit(&locked, false, (3.0, 5.0), 72.0), None);
    }

    #[test]
    fn the_nearest_of_two_guides_wins() {
        let guides = [guide(1, Axis::Horizontal, 2.0), guide(2, Axis::Horizontal, 2.04)];
        assert_eq!(hit(&guides, false, (1.0, 2.03), 72.0), Some(2));
    }

    #[test]
    fn a_release_lands_on_the_artboard_and_deletes_over_its_ruler() {
        let board = (12.0, 12.0);
        assert_eq!(release(Axis::Horizontal, (4.0, 2.0), board), Release::Land(2.0));
        assert_eq!(release(Axis::Horizontal, (4.0, -0.1), board), Release::OnRuler);
        assert_eq!(release(Axis::Horizontal, (-0.1, 2.0), board), Release::Away, "the left ruler is not a horizontal guide's");
        assert_eq!(release(Axis::Vertical, (-0.1, 2.0), board), Release::OnRuler);
        assert_eq!(release(Axis::Vertical, (3.0, 13.0), board), Release::Away);
    }

    #[test]
    fn guide_edits_are_checked_and_applied() {
        let mut d = Design::default();
        assert_eq!(d.check(&Edit::Guide(GuideEdit::Clear), &[]), Err(Blocked::NoGuides));
        d.apply(&Edit::Guide(GuideEdit::Add { axis: Axis::Vertical, at_in: 1.0 }), &[]).unwrap();
        d.apply(&Edit::Guide(GuideEdit::Add { axis: Axis::Horizontal, at_in: 2.0 }), &[]).unwrap();
        assert_eq!(d.guides.iter().map(|g| g.id).collect::<Vec<_>>(), [1, 2]);
        d.apply(&Edit::Guide(GuideEdit::Move { id: 2, at_in: 3.0 }), &[]).unwrap();
        assert_eq!(d.guide(2).map(|g| g.at_in), Some(3.0));
        d.apply(&Edit::Guide(GuideEdit::SetLocked { id: 2, locked: true }), &[]).unwrap();
        assert_eq!(d.check(&Edit::Guide(GuideEdit::Move { id: 2, at_in: 4.0 }), &[]), Err(Blocked::GuideLocked));
        d.apply(&Edit::Guide(GuideEdit::LockAll), &[]).unwrap();
        assert_eq!(d.check(&Edit::Guide(GuideEdit::Delete(1)), &[]), Err(Blocked::GuideLocked));
        d.apply(&Edit::Guide(GuideEdit::LockAll), &[]).unwrap();
        assert!(!d.guides_locked && d.guides.iter().all(|g| !g.locked), "unlocking all frees each guide too");
        d.apply(&Edit::Guide(GuideEdit::Show), &[]).unwrap();
        assert!(d.shown_guides().is_empty());
        d.apply(&Edit::Guide(GuideEdit::Delete(1)), &[]).unwrap();
        assert_eq!(d.check(&Edit::Guide(GuideEdit::Delete(1)), &[]), Err(Blocked::NoGuide));
        d.apply(&Edit::Guide(GuideEdit::Clear), &[]).unwrap();
        assert!(d.guides.is_empty());
    }

    #[test]
    fn composition_and_export_ignore_guides() {
        let mut plain = Design::default();
        let id = plain.add_shape(crate::design::ShapeKind::Square);
        plain.shape_mut(id).unwrap().x_in = 3.0;
        let mut guided = plain.clone();
        guided.apply(&Edit::Guide(GuideEdit::Add { axis: Axis::Vertical, at_in: 3.0 }), &[]).unwrap();
        guided.apply(&Edit::Guide(GuideEdit::Add { axis: Axis::Horizontal, at_in: 5.0 }), &[]).unwrap();
        let setup = crate::make_flow::MatSetup::preferred(crate::machine::MachineFamily::StudioPro);
        assert_eq!(crate::composition::compose(&plain, setup.mat_size, 1), crate::composition::compose(&guided, setup.mat_size, 1));
        for scope in [crate::export::Scope::Design, crate::export::Scope::PerMat] {
            assert_eq!(crate::export::sheets(&plain, scope, &setup), crate::export::sheets(&guided, scope, &setup), "{scope:?}");
        }
    }
}

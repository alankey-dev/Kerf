//! The effect stack: what was done to a layer, in order.

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::design::{Operation, Shape};
use crate::geometry::{DEFAULT_MITER_LIMIT, Join};
use crate::pens::Pen;
use crate::warp::Warp;

/// A shape's effects, back to front in evaluation order. Changed only through the effect edits.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Effects {
    entries: Vec<Entry>,
    /// Monotonic. An id is never reused within one stack, even after its entry is removed.
    #[serde(default)]
    next: u16,
}

/// Stable within one stack, so a panel or a piece can name an entry across edits.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectId(u16);

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EffectId,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub enabled: bool,
    pub effect: Effect,
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Bends every piece so far inside the current frame and stretches the result back onto it. A Warp
    /// before any Offset or Sticker bends the body inside its box, and the box refits to hug it.
    Warp(Warp),
    /// A closed outline `distance_in` outside the weld of the pieces so far and behind them, or inside it
    /// and in front.
    Offset(Offset),
    /// Replaces the pieces so far with one Print Then Cut piece: a white backing `border_in` outside their
    /// weld, printed with the pieces in their own fills.
    Sticker(Sticker),
}

impl Effect {
    /// Whether two effects are the same kind, which an entry's edits may not change.
    pub fn same_kind(&self, other: &Effect) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Whether it makes a piece of its own, which Combine and the other baking operations cannot take in.
    pub fn makes_pieces(&self) -> bool {
        !matches!(self, Effect::Warp(_))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    Round,
    Sharp,
}

impl Corners {
    pub fn join(self) -> Join {
        match self {
            Corners::Round => Join::Round,
            Corners::Sharp => Join::Miter { limit: DEFAULT_MITER_LIMIT },
        }
    }
}

/// Reads files saved before spec 34 too, through `upgrade::OffsetRepr`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(from = "super::upgrade::OffsetRepr")]
pub struct Offset {
    /// Never negative: `direction` says which side.
    pub distance_in: f32,
    pub direction: Direction,
    pub join: Join,
    /// `None` follows the layer's own fill, operation and pen live.
    pub style: Option<Style>,
}

impl Offset {
    /// The distance as `geometry::offset` takes it: negative insets.
    pub fn signed_in(&self) -> f32 {
        match self.direction {
            Direction::Outset => self.distance_in,
            Direction::Inset => -self.distance_in,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Outset,
    Inset,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Sticker {
    pub border_in: f32,
    pub corners: Corners,
}

/// How a piece is made: what Make cuts, draws or prints it with, and the colour the canvas fills.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Style {
    #[serde(with = "crate::design::color_serde")]
    pub fill: Color32,
    pub operation: Operation,
    pub pen: Pen,
}

impl Style {
    pub fn of(shape: &Shape) -> Style {
        Style { fill: shape.fill, operation: shape.operation, pen: shape.pen }
    }
}

/// A text's curve: the signed diameter of the circle it follows (negative curves down), and its eye.
/// Straight text has no curve at all, so "diameter 0 with the eye off" cannot be stored.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub diameter_in: f32,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Curve {
    /// What older files stored as `curve_in`: 0 is straight.
    pub fn from_diameter(diameter_in: f32) -> Option<Curve> {
        (diameter_in != 0.0).then_some(Curve { diameter_in, enabled: true })
    }
}

/// Reads `curve` as saved now, or `curve_in` as a plain number from files before format 4.
pub(crate) fn curve_compat<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Curve>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Number(f32),
        Curve(Curve),
    }
    Ok(match Option::<Saved>::deserialize(d)? {
        None => None,
        Some(Saved::Number(n)) => Curve::from_diameter(n),
        Some(Saved::Curve(c)) => Some(c),
    })
}

/// Which body contours are hidden, indexed as `effects::body_contours`, and the eye that shows them all
/// without losing the choice.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ContourMask {
    pub hidden: Vec<bool>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Default for ContourMask {
    fn default() -> Self {
        Self { hidden: Vec::new(), enabled: true }
    }
}

impl ContourMask {
    /// The hidden flags evaluation applies: none while the eye is off.
    pub fn active(&self) -> &[bool] {
        if self.enabled { &self.hidden } else { &[] }
    }

    /// Whether any contour is hidden, which is when Layers lists the Contour row.
    pub fn any(&self) -> bool {
        self.hidden.iter().any(|&h| h)
    }
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: EffectId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The id the next added entry takes, so a panel can follow the entry its Apply creates.
    pub fn next_id(&self) -> EffectId {
        EffectId(self.next)
    }

    /// The first Warp entry's value, on or off: what the toolbar Warp panel shows and edits.
    pub fn first_warp(&self) -> Option<Warp> {
        self.entries.iter().find_map(|e| match e.effect {
            Effect::Warp(w) => Some(w),
            _ => None,
        })
    }

    /// The enabled Warp applied, if any. A stack holds at most one Warp.
    pub fn warp(&self) -> Option<Warp> {
        self.enabled().find_map(|e| match e.effect {
            Effect::Warp(w) => Some(w),
            _ => None,
        })
    }

    pub fn enabled(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.enabled)
    }

    /// The enabled Warp before any enabled Offset or Sticker. Only it bends the body inside its box,
    /// refits the box, and bakes on a node edit.
    pub fn leading_warp(&self) -> Option<Warp> {
        match self.enabled().next()?.effect {
            Effect::Warp(w) => Some(w),
            _ => None,
        }
    }

    /// The enabled entries evaluated after the body: everything but the leading Warp.
    pub(crate) fn after_body(&self) -> impl Iterator<Item = &Entry> {
        self.enabled().skip(usize::from(self.leading_warp().is_some()))
    }

    /// Whether an enabled Warp follows an enabled Offset or Sticker, bending them all together.
    pub fn has_trailing_warp(&self) -> bool {
        self.after_body().any(|e| matches!(e.effect, Effect::Warp(_)))
    }

    /// Whether an enabled entry makes a piece of its own (an Offset or a Sticker).
    pub fn has_live_pieces(&self) -> bool {
        self.enabled().any(|e| e.effect.makes_pieces())
    }

    /// The toolbar Warp: edits and enables the first Warp entry, or inserts one before the first Offset or
    /// Sticker. `None` removes that entry. Idempotent for a repeated value.
    pub(crate) fn set_warp(&mut self, warp: Option<Warp>) {
        let at = self.entries.iter().position(|e| matches!(e.effect, Effect::Warp(_)));
        match (at, warp) {
            (Some(i), Some(w)) => (self.entries[i].effect, self.entries[i].enabled) = (Effect::Warp(w), true),
            (Some(i), None) => _ = self.entries.remove(i),
            (None, Some(w)) => {
                let id = self.mint();
                let before = self.entries.iter().position(|e| e.effect.makes_pieces()).unwrap_or(self.entries.len());
                self.entries.insert(before, Entry { id, enabled: true, effect: Effect::Warp(w) });
            }
            (None, None) => {}
        }
    }

    /// Takes the enabled leading Warp out, as a node edit bakes it into the outline. A disabled Warp stays.
    pub(crate) fn take_leading_warp(&mut self) {
        if self.leading_warp().is_some()
            && let Some(at) = self.entries.iter().position(|e| e.enabled)
        {
            self.entries.remove(at);
        }
    }

    /// Load-time repair: non-finite Offset and Sticker lengths become 0 and the rest clamp to `max_in`.
    /// Returns whether anything changed.
    pub(crate) fn clamp_lengths(&mut self, max_in: f32) -> bool {
        let mut changed = false;
        for e in &mut self.entries {
            let length = match &mut e.effect {
                Effect::Offset(o) => &mut o.distance_in,
                Effect::Sticker(s) => &mut s.border_in,
                Effect::Warp(_) => continue,
            };
            let fixed = if length.is_finite() { length.clamp(-max_in, max_in) } else { 0.0 };
            changed |= fixed != *length;
            *length = fixed;
        }
        changed
    }

    /// Every entry removed, `next` kept, so an id is not reused after Expand.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn cleared(&self) -> Effects {
        Effects { entries: Vec::new(), next: self.next }
    }

    /// Whether an enabled Warp follows an enabled Sticker. Expand could not print the artwork as bent, so
    /// no edit makes this stack.
    pub fn warp_after_sticker(&self) -> bool {
        self.enabled().skip_while(|e| !matches!(e.effect, Effect::Sticker(_))).any(|e| matches!(e.effect, Effect::Warp(_)))
    }

    /// Appends `effect`, enabled, on top of the stack. Returns its new id.
    pub(crate) fn push(&mut self, effect: Effect) -> EffectId {
        let id = self.mint();
        self.entries.push(Entry { id, enabled: true, effect });
        id
    }

    /// Replaces entry `id`'s effect with one of the same kind. Refuses (returns false) any other kind.
    pub(crate) fn replace(&mut self, id: EffectId, effect: Effect) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) if e.effect.same_kind(&effect) => {
                e.effect = effect;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn set_enabled(&mut self, id: EffectId, on: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.enabled = on;
        }
    }

    pub(crate) fn remove(&mut self, id: EffectId) -> Option<Entry> {
        let at = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(at))
    }

    /// Moves entry `id` to `index` in the stack, clamped to the end.
    pub(crate) fn move_to(&mut self, id: EffectId, index: usize) {
        if let Some(entry) = self.remove(id) {
            let index = index.min(self.entries.len());
            self.entries.insert(index, entry);
        }
    }

    /// The stack up to, not including, entry `id`, without disabled entries: what a Sticker's Expand
    /// keeps live on the source it prints.
    pub(crate) fn before(&self, id: EffectId) -> Effects {
        let entries = self.entries.iter().take_while(|e| e.id != id).filter(|e| e.enabled).cloned().collect();
        Effects { entries, next: self.next }
    }

    fn mint(&mut self) -> EffectId {
        let id = EffectId(self.next);
        self.next += 1;
        id
    }

    /// Load-time repair: distinct ids with `next` above them, then the first Warp only.
    /// Keeps the first Warp, and only effects `kind` can take: no Warp on a kind that cannot warp, and no
    /// Sticker on an image, whose picture a sticker backing would paint over.
    pub(crate) fn validated(self, kind: &crate::design::ShapeKind) -> Effects {
        let mut fx = self.renumbered();
        let mut warps = 0;
        let image = matches!(kind, crate::design::ShapeKind::Bitmap { .. });
        fx.entries.retain(|e| match e.effect {
            Effect::Warp(_) => {
                warps += 1;
                warps == 1 && kind.can_warp()
            }
            Effect::Sticker(_) => !image,
            Effect::Offset(_) => true,
        });
        fx
    }

    fn renumbered(mut self) -> Effects {
        self.next = self.entries.iter().map(|e| e.id.0 + 1).max().unwrap_or(0).max(self.next);
        let mut seen = std::collections::HashSet::new();
        for i in 0..self.entries.len() {
            if !seen.insert(self.entries[i].id) {
                self.entries[i].id = self.mint();
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::WarpStyle;

    const ARCH: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };
    const RING: Effect = Effect::Offset(Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None });

    #[test]
    fn set_warp_edits_one_entry_and_never_reuses_an_id() {
        let mut fx = Effects::default();
        fx.set_warp(Some(ARCH));
        fx.set_warp(Some(Warp { bend: 0.2, ..ARCH }));
        assert_eq!(fx.entries().len(), 1, "a second value edits the entry");
        assert_eq!(fx.warp(), Some(Warp { bend: 0.2, ..ARCH }));
        let first = fx.entries()[0].id;
        fx.set_warp(None);
        assert!(fx.is_empty());
        fx.set_warp(Some(ARCH));
        assert_ne!(fx.entries()[0].id, first, "a removed id is not reused");
    }

    #[test]
    fn the_toolbar_warp_goes_before_an_offset_and_leads() {
        let mut fx = Effects::default();
        let ring = fx.push(RING);
        fx.set_warp(Some(ARCH));
        assert!(matches!(fx.entries()[0].effect, Effect::Warp(_)), "inserted before the Offset");
        assert_eq!((fx.leading_warp(), fx.has_trailing_warp()), (Some(ARCH), false));
        fx.move_to(ring, 0);
        assert_eq!((fx.leading_warp(), fx.has_trailing_warp()), (None, true), "after a reorder the Warp trails");
        fx.set_enabled(ring, false);
        assert_eq!(fx.leading_warp(), Some(ARCH), "a disabled Offset does not count");
    }

    #[test]
    fn replace_keeps_the_kind_and_ids_stay_monotonic() {
        let mut fx = Effects::default();
        let ring = fx.push(RING);
        assert!(!fx.replace(ring, Effect::Warp(ARCH)), "an Offset never turns into a Warp");
        assert!(fx.replace(ring, Effect::Offset(Offset { distance_in: 0.5, direction: Direction::Inset, join: Join::Bevel, style: None })));
        fx.remove(ring);
        assert_ne!(fx.push(RING), ring);
    }

    #[test]
    fn a_disabled_warp_is_not_applied() {
        let mut fx = Effects::default();
        fx.set_warp(Some(ARCH));
        fx.entries[0].enabled = false;
        assert_eq!(fx.warp(), None);
    }

    #[test]
    fn an_image_keeps_only_its_offsets() {
        let mut fx = Effects::default();
        fx.push(Effect::Warp(ARCH));
        fx.push(Effect::Sticker(Sticker { border_in: 0.125, corners: Corners::Round }));
        let offset = Effect::Offset(Offset { distance_in: 0.1, direction: Direction::Outset, join: Join::Round, style: None });
        fx.push(offset);
        let image = crate::design::ShapeKind::Bitmap { asset: crate::assets::id_of(&image::RgbaImage::new(2, 2)), px: (2, 2), clip: Vec::new() };
        let kept: Vec<Effect> = fx.clone().validated(&image).entries().iter().map(|e| e.effect).collect();
        assert_eq!(kept, [offset], "an image cannot warp, and a sticker would paint over its picture");
        assert_eq!(fx.validated(&crate::design::ShapeKind::Square).entries().len(), 3);
    }

    #[test]
    fn duplicate_ids_load_distinct_with_next_above_them() {
        let json = r#"{"entries":[{"id":4,"effect":{"Warp":{"style":"Arch","bend":0.5,"vertical":false}}},{"id":4,"effect":{"Warp":{"style":"Wave","bend":0.1,"vertical":false}}}],"next":0}"#;
        let loaded: Effects = serde_json::from_str(json).unwrap();
        let fx = loaded.clone().renumbered();
        assert_eq!(fx.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [EffectId(4), EffectId(5)]);
        assert_eq!(fx.next, 6, "next is above every id");
        let valid = loaded.validated(&crate::design::ShapeKind::Square);
        assert_eq!((valid.entries().len(), valid.warp()), (1, Some(ARCH)), "a stack keeps its first Warp");
    }
}

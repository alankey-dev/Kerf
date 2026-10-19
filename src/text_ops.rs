//! Text operations over the layout: splitting a text into letters or lines, and curving it.
//!
//! A split piece is a text shape of its own whose natural box lands in the source's natural box at
//! `origin`, turned by `angle` about that point. Placing it goes through the source's scale, flip and
//! rotation, so the pieces look exactly like the source did.

use std::ops::RangeInclusive;

use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::edit::TextPathEdit;
use crate::text::{FontRef, FontStyle, TextAlign, VAlign};
use crate::text_path::TextPath;

pub const POINTS_PER_IN: f32 = 72.0;
pub const SIZE_PT: RangeInclusive<f32> = 1.0..=1440.0;
pub const LETTER_SPACE: RangeInclusive<f32> = -200.0..=1000.0;
pub const LINE_SPACE: RangeInclusive<f32> = -500.0..=2000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Letters,
    Lines,
}

/// One text property set across a selection. Sizes and spacings clamp to the limits above.
#[derive(Clone, Debug, PartialEq)]
pub enum TextChange {
    Font(FontRef),
    SizePt(f32),
    Style(FontStyle),
    Align(TextAlign),
    LetterSpace(f32),
    LineSpace(f32),
    Wrap(bool),
    VAlign(VAlign),
    /// Replaces each text with its letters or lines, grouped.
    Split(Split),
}

struct Piece {
    kind: ShapeKind,
    origin: Pt,
    angle: f32,
}

/// The pieces `kind` splits into; empty for a non-text kind.
fn pieces(kind: &ShapeKind, split: Split) -> Vec<Piece> {
    let (Some(layout), ShapeKind::Text { content, .. }) = (kind.text_layout(), kind) else { return Vec::new() };
    let chars: Vec<char> = content.chars().collect();
    let with_content = |text: String| {
        let mut piece = kind.clone();
        if let ShapeKind::Text { content, wrap_in, curve, .. } = &mut piece {
            (*content, *wrap_in, *curve) = (text, None, None);
        }
        piece
    };
    match split {
        Split::Letters => layout
            .glyphs
            .iter()
            .filter_map(|g| {
                let kind = with_content(chars[g.char_index].to_string());
                let own_pen = kind.text_layout()?.glyphs.first()?.pen;
                let (sin, cos) = g.angle.sin_cos();
                let origin = (g.pen.0 - (own_pen.0 * cos - own_pen.1 * sin), g.pen.1 - (own_pen.0 * sin + own_pen.1 * cos));
                Some(Piece { kind, origin, angle: g.angle })
            })
            .collect(),
        Split::Lines => layout
            .lines
            .iter()
            .enumerate()
            .filter(|(i, _)| layout.glyphs.iter().any(|g| g.line == *i))
            .map(|(_, line)| Piece { kind: with_content(line.text.clone()), origin: line.origin, angle: 0.0 })
            .collect(),
    }
}

/// Why `shape` may not split, whatever its words. Letters cannot share one Offset or Sticker, and a warp
/// bends the outline in ways separate letters cannot reproduce, so text with effects does not split; nor
/// does a container, whose children N letters cannot share.
pub fn split_refusal(shape: &Shape) -> Option<crate::edit::Blocked> {
    use crate::edit::Blocked;
    match () {
        _ if !shape.effects.is_empty() => Some(Blocked::SplitEffects),
        _ if shape.container.is_some() => Some(Blocked::SplitContainer),
        _ => None,
    }
}

/// Whether `shape` splits into two or more pieces.
pub fn can_split(shape: &Shape, split: Split) -> bool {
    split_refusal(shape).is_none() && pieces(&shape.kind, split).len() >= 2
}

/// The piece as a shape where it sat inside `source`, sharing its fill, operation and flips.
fn place(source: &Shape, piece: Piece, id: u64) -> Shape {
    let (natural_w, natural_h) = source.kind.default_size_in();
    let scale = (source.width_in / natural_w, source.height_in / natural_h);
    let (w, h) = piece.kind.default_size_in();
    let (sin, cos) = piece.angle.sin_cos();
    let (hw, hh) = (w / 2.0, h / 2.0);
    let centre = (piece.origin.0 + hw * cos - hh * sin, piece.origin.1 + hw * sin + hh * cos);
    let (cx, cy) = source.to_world((centre.0 * scale.0, centre.1 * scale.1));
    let (width_in, height_in) = (w * scale.0, h * scale.1);
    // A single mirror reverses the sense of the piece's turn.
    let turn = if source.flip_x != source.flip_y { -piece.angle } else { piece.angle };
    Shape {
        x_in: cx - width_in / 2.0,
        y_in: cy - height_in / 2.0,
        width_in,
        height_in,
        rotation_deg: source.rotation_deg + turn.to_degrees(),
        fill: source.fill,
        operation: source.operation,
        pen: source.pen,
        flip_x: source.flip_x,
        flip_y: source.flip_y,
        hidden: source.hidden,
        ..Shape::new(id, piece.kind)
    }
}

/// A single unwrapped line can curve from the smallest circle that fits it to ten times its width.
/// `None` when the text cannot curve.
pub fn curve_range(kind: &ShapeKind) -> Option<std::ops::RangeInclusive<f32>> {
    let ShapeKind::Text { content, wrap_in: None, .. } = kind else { return None };
    if content.contains('\n') || content.is_empty() {
        return None;
    }
    let mut straight = kind.clone();
    if let ShapeKind::Text { curve, .. } = &mut straight {
        *curve = None;
    }
    let width = straight.default_size_in().0;
    Some(width / std::f32::consts::PI..=width * 10.0)
}

impl Design {
    /// Replaces text `id` in its z position with its letters or lines in one new Group. Returns the
    /// shapes now standing where `id` stood: the pieces, or `id` alone when it does not split.
    pub fn ungroup_text(&mut self, id: u64, split: Split) -> Vec<u64> {
        let Some(at) = self.shapes.iter().position(|s| s.id == id && can_split(s, split)) else { return vec![id] };
        let source = &self.shapes[at];
        let placed: Vec<Shape> = pieces(&source.kind, split).into_iter().zip(self.next_id()..).map(|(piece, id)| place(source, piece, id)).collect();
        let ids = self.replace_with(&[id], placed);
        self.group(&ids);
        ids
    }

    /// Applies `change` to text `id`, re-fitting its box so its scale is kept. Returns the shapes now
    /// standing where `id` stood: the pieces of a split, else `id`. A writing font has one weight, so
    /// bold styles become regular or italic, and it is drawn with a pen, so a cut becomes Pen.
    pub fn change_text(&mut self, id: u64, change: &TextChange) -> Vec<u64> {
        if let TextChange::Split(split) = *change {
            return self.ungroup_text(id, split);
        }
        let Some(s) = self.shape_mut(id) else { return vec![id] };
        let before = s.kind.clone();
        let ShapeKind::Text { font, size_in, style, letter_space, line_space, align, wrap_in, v_align, .. } = &mut s.kind else { return vec![id] };
        match change.clone() {
            TextChange::Font(f) => *font = f,
            TextChange::SizePt(pt) => *size_in = pt.clamp(*SIZE_PT.start(), *SIZE_PT.end()) / POINTS_PER_IN,
            TextChange::Style(st) => *style = st,
            TextChange::Align(a) => *align = a,
            TextChange::LetterSpace(v) => *letter_space = v.clamp(*LETTER_SPACE.start(), *LETTER_SPACE.end()),
            TextChange::LineSpace(v) => *line_space = v.clamp(*LINE_SPACE.start(), *LINE_SPACE.end()),
            TextChange::Wrap(on) => *wrap_in = on.then(|| before.default_size_in().0),
            TextChange::VAlign(v) => *v_align = v,
            TextChange::Split(_) => unreachable!("splits return above"),
        }
        if font.is_writing() {
            *style = if style.italic() { FontStyle::Italic } else { FontStyle::Regular };
            if matches!(change, TextChange::Font(_)) && matches!(s.operation, Operation::BasicCut | Operation::WavyCut | Operation::Perforate) {
                s.set_operation(Operation::Pen);
            }
        }
        self.refit_text(id, &before);
        vec![id]
    }

    /// Sets the curve of text `id`, keeping its scale over the natural size.
    pub fn set_text_curve(&mut self, id: u64, curve: f32) {
        let Some(s) = self.shape_mut(id) else { return };
        let before = s.kind.clone();
        let ShapeKind::Text { curve: stored, .. } = &mut s.kind else { return };
        let enabled = stored.is_none_or(|c| c.enabled);
        *stored = crate::effects::Curve::from_diameter(curve).map(|c| crate::effects::Curve { enabled, ..c });
        self.refit_text(id, &before);
    }

    /// Runs a path edit (spec 51) over the subject, returning the ids the user then holds.
    pub fn apply_text_path(&mut self, op: TextPathEdit, on: &[u64]) -> Vec<u64> {
        match op {
            TextPathEdit::Attach => match self.text_and_path(on) {
                Some((text, path)) => {
                    self.text_on_path(text, path);
                    vec![text]
                }
                None => on.to_vec(),
            },
            TextPathEdit::Remove => {
                for &id in on {
                    self.remove_from_path(id);
                }
                on.to_vec()
            }
            TextPathEdit::Side(side) => self.edit_text_path(on, |p| p.side = side),
            TextPathEdit::Along(along) => self.edit_text_path(on, |p| p.align = along),
            TextPathEdit::Offset(offset_in) => self.edit_text_path(on, |p| p.offset_in = offset_in),
        }
    }

    /// The one text and the one shape it is to follow among `on`, when the selection is exactly those two.
    pub fn text_and_path(&self, on: &[u64]) -> Option<(u64, u64)> {
        let chosen: Vec<&Shape> = self.shapes.iter().filter(|s| on.contains(&s.id)).collect();
        if chosen.len() != 2 {
            return None;
        }
        let text = chosen.iter().copied().find(|s| matches!(s.kind, ShapeKind::Text { .. }))?;
        let path = chosen.iter().copied().find(|s| !matches!(s.kind, ShapeKind::Text { .. }))?;
        Some((text.id, path.id))
    }

    /// Puts text `text_id` on the evaluated outline of shape `path_id`: the text takes its own copy of
    /// the path, the source stays (becoming a Guide unless it already cuts) and the text centres over
    /// the path's box. Keeps the text's scale over its new natural size.
    pub fn text_on_path(&mut self, text_id: u64, path_id: u64) {
        let Some(source) = self.shapes.iter().find(|s| s.id == path_id) else { return };
        let (sw, sh, sx, sy) = (source.width_in, source.height_in, source.x_in, source.y_in);
        // A circle source keeps its exact analytic curve; any other shape takes a copy of its outline.
        let new_path = if matches!(source.kind, ShapeKind::Circle) {
            TextPath::circle(sw)
        } else {
            TextPath { contours: source.kind.outline(sw, sh), view_box: (sw, sh), offset_in: 0.0, side: Default::default(), align: Default::default() }
        };
        let cuts = matches!(source.operation, Operation::BasicCut | Operation::PrintThenCut | Operation::WavyCut | Operation::Perforate);
        if !cuts && let Some(s) = self.shape_mut(path_id) {
            s.set_operation(Operation::Guide);
        }
        let Some(s) = self.shape_mut(text_id) else { return };
        let before = s.kind.clone();
        let ShapeKind::Text { path, .. } = &mut s.kind else { return };
        *path = Some(new_path);
        self.refit_text(text_id, &before);
        if let Some(s) = self.shape_mut(text_id) {
            s.x_in = sx + (sw - s.width_in) / 2.0;
            s.y_in = sy + (sh - s.height_in) / 2.0;
        }
    }

    /// Straightens path text `id`, keeping its size and font, as Remove from path.
    pub fn remove_from_path(&mut self, id: u64) {
        let Some(s) = self.shape_mut(id) else { return };
        let before = s.kind.clone();
        let ShapeKind::Text { path, .. } = &mut s.kind else { return };
        if path.is_none() {
            return;
        }
        *path = None;
        self.refit_text(id, &before);
    }

    /// Applies `f` to each subject text's path, reflowing it. Returns `on` unchanged.
    fn edit_text_path(&mut self, on: &[u64], f: impl Fn(&mut TextPath)) -> Vec<u64> {
        for &id in on {
            let Some(s) = self.shape_mut(id) else { continue };
            let before = s.kind.clone();
            let ShapeKind::Text { path: Some(p), .. } = &mut s.kind else { continue };
            f(p);
            self.refit_text(id, &before);
        }
        on.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::LinkKind;
    use crate::text::FontRef;
    use egui::Color32;

    fn design_with(content: &str) -> (Design, u64) {
        let mut d = Design::default();
        let id = d.add_text(content.into(), FontRef::Inter, 1.0, Color32::BLACK, 2.0, 3.0);
        (d, id)
    }

    fn world_points(d: &Design) -> Vec<Pt> {
        d.shapes.iter().flat_map(|s| s.world_polylines()).flat_map(|(pts, _)| pts).collect()
    }

    /// The Hausdorff distance between the two point sets stays under 0.001 in.
    fn assert_same_ink(before: &[Pt], after: &[Pt]) {
        assert_eq!(before.len(), after.len());
        let gap = |from: &[Pt], to: &[Pt]| from.iter().map(|a| to.iter().map(|b| (a.0 - b.0).hypot(a.1 - b.1)).fold(f32::MAX, f32::min)).fold(0.0, f32::max);
        let worst = gap(before, after).max(gap(after, before));
        assert!(worst < 0.001, "outline moved by {worst}");
    }

    #[test]
    fn letters_of_hi_are_two_grouped_texts_where_the_glyphs_were() {
        let (mut d, id) = design_with("Hi");
        let before = world_points(&d);
        let ids = d.ungroup_text(id, Split::Letters);
        assert_eq!(ids.len(), 2);
        let contents: Vec<&str> = d.shapes.iter().map(|s| if let ShapeKind::Text { content, .. } = &s.kind { content.as_str() } else { "" }).collect();
        assert_eq!(contents, ["H", "i"]);
        let link = d.shapes[0].link.expect("grouped");
        assert_eq!(link.kind, LinkKind::Group);
        assert_eq!(d.shapes[1].link, Some(link));
        assert_same_ink(&before, &world_points(&d));
    }

    #[test]
    fn lines_of_a_b_are_two_texts() {
        let (mut d, id) = design_with("a\nb");
        let before = world_points(&d);
        assert_eq!(d.ungroup_text(id, Split::Lines).len(), 2);
        assert_same_ink(&before, &world_points(&d));
    }

    #[test]
    fn letters_keep_their_place_through_scale_rotation_flip_and_alignment() {
        let (mut d, id) = design_with("Ta\nWy");
        let s = d.shape_mut(id).unwrap();
        (s.width_in, s.height_in, s.rotation_deg, s.flip_x) = (s.width_in * 1.5, s.height_in * 0.8, 30.0, true);
        if let ShapeKind::Text { align, .. } = &mut s.kind {
            *align = crate::text::TextAlign::Centre;
        }
        let before = world_points(&d);
        d.ungroup_text(id, Split::Letters);
        assert_eq!(d.shapes.len(), 4);
        assert_same_ink(&before, &world_points(&d));
    }

    #[test]
    fn curved_letters_keep_their_place_and_turn() {
        let (mut d, id) = design_with("curve");
        d.set_text_curve(id, 3.0);
        let before = world_points(&d);
        d.ungroup_text(id, Split::Letters);
        assert!(d.shapes[0].rotation_deg < -1.0, "the first letter leans left: {}", d.shapes[0].rotation_deg);
        assert_same_ink(&before, &world_points(&d));
    }

    #[test]
    fn the_pieces_take_the_sources_z_position() {
        let (mut d, id) = design_with("ab");
        let below = d.add_shape(ShapeKind::Square);
        d.shapes.swap(0, 1);
        let above = d.add_shape(ShapeKind::Circle);
        let ids = d.ungroup_text(id, Split::Letters);
        let order: Vec<u64> = d.shapes.iter().map(|s| s.id).collect();
        assert_eq!(order, [below, ids[0], ids[1], above]);
    }

    #[test]
    fn single_pieces_and_warped_text_do_not_split() {
        let (mut d, id) = design_with("ab");
        assert_eq!(d.ungroup_text(id, Split::Lines), [id], "one line");
        d.shape_mut(id).unwrap().set_warp(Some(crate::warp::Warp { style: crate::warp::WarpStyle::Arch, bend: 0.5, vertical: false }));
        assert_eq!(d.ungroup_text(id, Split::Letters), [id], "warped");
        assert_eq!(d.shapes.len(), 1);
    }

    #[test]
    fn curving_keeps_the_scale_and_straightening_restores_the_box() {
        let (mut d, id) = design_with("Curve");
        d.shape_mut(id).unwrap().width_in *= 2.0;
        let straight = d.shapes[0].clone();
        d.set_text_curve(id, 4.0);
        assert!(d.shapes[0].height_in > straight.height_in, "the arc is taller");
        d.set_text_curve(id, 0.0);
        assert!((d.shapes[0].width_in - straight.width_in).abs() < 1e-4 && (d.shapes[0].height_in - straight.height_in).abs() < 1e-4);
    }

    #[test]
    fn curve_range_runs_from_the_fitting_circle_to_ten_widths() {
        let (d, _) = design_with("Curve");
        let width = d.shapes[0].width_in;
        let range = curve_range(&d.shapes[0].kind).unwrap();
        assert!((range.start() * std::f32::consts::PI - width).abs() < 1e-4);
        assert!((range.end() - 10.0 * width).abs() < 1e-4);
        assert!(curve_range(&design_with("a\nb").0.shapes[0].kind).is_none(), "several lines do not curve");
    }

    #[test]
    fn editing_warped_text_matches_warping_the_edited_text() {
        let warp = Some(crate::warp::Warp { style: crate::warp::WarpStyle::Arch, bend: 0.6, vertical: false });
        let edit = |d: &mut Design, id: u64| {
            let before = d.shapes[0].kind.clone();
            if let ShapeKind::Text { content, .. } = &mut d.shape_mut(id).unwrap().kind {
                *content = "Hiya there".into();
            }
            d.refit_text(id, &before);
        };
        let (mut warped_first, a) = design_with("Hi");
        warped_first.shape_mut(a).unwrap().set_warp(warp);
        edit(&mut warped_first, a);
        let (mut edited_first, b) = design_with("Hi");
        edit(&mut edited_first, b);
        edited_first.shape_mut(b).unwrap().set_warp(warp);
        let (x, y) = (&warped_first.shapes[0], &edited_first.shapes[0]);
        assert!((x.width_in - y.width_in).abs() < 1e-4 && (x.height_in - y.height_in).abs() < 1e-4, "{:?} vs {:?}", (x.width_in, x.height_in), (y.width_in, y.height_in));
    }

    #[test]
    fn a_text_saved_before_curves_loads_straight() {
        let saved = r#"{"shapes":[{"id":1,"kind":{"Text":{"content":"Hi","font":"Inter","size_in":1.0,"style":"Bold","letter_space":0.0,"line_space":0.0,"align":"Left","wrap_in":null,"v_align":"Top"}},"x_in":1.0,"y_in":1.0,"width_in":1.0,"height_in":1.2,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"Pen","hidden":false,"contour_hidden":[]}]}"#;
        let d: Design = serde_json::from_str(saved).unwrap();
        assert!(matches!(d.shapes[0].kind, ShapeKind::Text { curve: None, .. }));
    }

    fn writing(id: &str) -> FontRef {
        FontRef::Stroke(crate::stroke_font::StrokeFontId(id.into()))
    }

    #[test]
    fn choosing_a_writing_font_turns_a_cut_into_a_pen_and_bold_into_regular() {
        let (mut d, id) = design_with("Hi");
        d.shape_mut(id).unwrap().set_operation(Operation::WavyCut);
        d.change_text(id, &TextChange::Style(FontStyle::BoldItalic));
        d.change_text(id, &TextChange::Font(writing("hershey-script-simplex")));
        let s = &d.shapes[0];
        assert_eq!(s.operation, Operation::Pen, "a cut becomes Pen");
        assert!(matches!(s.kind, ShapeKind::Text { style: FontStyle::Italic, .. }), "bold italic becomes italic: {:?}", s.kind);
        d.change_text(id, &TextChange::Style(FontStyle::Bold));
        assert!(matches!(d.shapes[0].kind, ShapeKind::Text { style: FontStyle::Regular, .. }), "a writing font has no bold");
    }

    #[test]
    fn choosing_a_writing_font_leaves_draw_operations_and_other_fonts_leave_cuts() {
        let foil = Operation::Foil(crate::design::FoilWeight::Fine);
        let (mut d, id) = design_with("Hi");
        d.shape_mut(id).unwrap().set_operation(foil);
        d.change_text(id, &TextChange::Font(writing("hershey-simplex")));
        assert_eq!(d.shapes[0].operation, foil, "Foil is allowed");

        let (mut d, id) = design_with("Hi");
        d.shape_mut(id).unwrap().set_operation(Operation::BasicCut);
        d.change_text(id, &TextChange::Font(FontRef::SpaceGrotesk));
        assert_eq!(d.shapes[0].operation, Operation::BasicCut, "an outline font never touches the operation");
    }

    #[test]
    fn letters_and_lines_of_writing_text_keep_the_writing_font() {
        let font = writing("relief-singleline");
        for (split, count) in [(Split::Letters, 4), (Split::Lines, 2)] {
            let mut d = Design::default();
            let id = d.add_text("ab\ncd".into(), font.clone(), 1.0, Color32::BLACK, 2.0, 3.0);
            let pieces = d.ungroup_text(id, split);
            assert_eq!(pieces.len(), count, "{split:?}");
            assert!(d.shapes.iter().all(|s| matches!(&s.kind, ShapeKind::Text { font: f, .. } if *f == font)), "{split:?} keeps the font");
            assert!(d.shapes.iter().flat_map(|s| s.kind.outline(s.width_in, s.height_in)).all(|c| !c.closed), "{split:?} pieces stay open strokes");
        }
    }
}

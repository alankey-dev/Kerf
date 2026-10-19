//! Live Offset and Sticker entries: evaluation, editing, Expand and Make.

use egui::Color32;

use super::{Corners, Direction, Effect, EffectEdit, EntryRef, Local, Offset, Reach, Row, Sticker, Style};
use crate::design::{Design, Operation, Pt, ShapeKind};
use crate::edit::{Blocked, Edit};
use crate::geometry::{FLATTEN_TOL_IN, Join};
use crate::snap::Aabb;
use crate::warp::{Warp, WarpStyle};

const RING: Offset = Offset { distance_in: 0.25, direction: Direction::Outset, join: Join::Round, style: None };
const ARCH: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };

/// A 2 x 2 square at (3, 3), as the only shape.
fn square() -> (Design, u64) {
    let mut d = Design::default();
    let id = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (3.0, 3.0, 2.0, 2.0);
    (d, id)
}

fn add(d: &mut Design, id: u64, effect: Effect) {
    d.apply(&Edit::AddEffect { effect, reach: Reach::EachShape }, &[id]).unwrap();
}

fn entry(d: &Design, id: u64, n: usize) -> EntryRef {
    EntryRef { shape: id, row: Row::Entry(d.shapes.iter().find(|s| s.id == id).unwrap().effects.entries()[n].id) }
}

fn geometry(d: &Design, id: u64) -> std::rc::Rc<super::Geometry> {
    d.geometry(d.shapes.iter().find(|s| s.id == id).unwrap())
}

fn width(b: Aabb) -> f32 {
    b.max_x - b.min_x
}

fn assert_close(a: Aabb, b: Aabb, tol: f32) {
    let worst = [a.min_x - b.min_x, a.min_y - b.min_y, a.max_x - b.max_x, a.max_y - b.max_y].into_iter().fold(0.0_f32, |m, v| m.max(v.abs()));
    assert!(worst <= tol, "{a:?} != {b:?} ({worst})");
}

/// Every vertex of `a` lies within `tol` of an edge of `b`, and the reverse.
pub(super) fn assert_same_rings(a: &[Vec<Pt>], b: &[Vec<Pt>], tol: f32) {
    let to_edge = |p: Pt, (s, e): (Pt, Pt)| {
        let (dx, dy) = (e.0 - s.0, e.1 - s.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 { 0.0 } else { (((p.0 - s.0) * dx + (p.1 - s.1) * dy) / len2).clamp(0.0, 1.0) };
        (p.0 - s.0 - t * dx).hypot(p.1 - s.1 - t * dy)
    };
    let edges = |r: &[Vec<Pt>]| r.iter().flat_map(|ring| (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()]))).collect::<Vec<_>>();
    let gap = |from: &[Vec<Pt>], to: &[Vec<Pt>]| {
        let edges = edges(to);
        from.iter().flatten().map(|&p| edges.iter().map(|&e| to_edge(p, e)).fold(f32::MAX, f32::min)).fold(0.0, f32::max)
    };
    let worst = gap(a, b).max(gap(b, a));
    assert!(worst <= tol, "outlines differ by {worst}");
}

#[test]
fn an_offset_tracks_a_resize_and_leaves_evaluation_and_make_when_disabled() {
    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(RING));
    let g = geometry(&d, id);
    assert_eq!(g.pieces().len(), 2);
    assert_eq!(g.pieces()[0].id.local, Local::Effect(d.shapes[0].effects.entries()[0].id), "the ring sits behind the body");
    assert!((width(g.pieces()[0].bounds) - 2.5).abs() < 1e-3);
    assert_eq!((d.shapes[0].width_in, d.shapes[0].height_in), (2.0, 2.0), "an Offset never changes the box");

    d.shape_mut(id).unwrap().width_in = 4.0;
    assert!((width(geometry(&d, id).pieces()[0].bounds) - 4.5).abs() < 1e-3, "the ring follows the new size");

    let at = entry(&d, id, 0);
    d.apply(&Edit::Effect { at, op: EffectEdit::Enable(false) }, &[id]).unwrap();
    assert_eq!(geometry(&d, id).pieces().len(), 1, "a disabled Offset adds nothing");
    let mats = crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
    assert_eq!(mats.iter().flat_map(|m| &m.items).count(), 1, "and Make cuts the square alone");
}

#[test]
fn expand_on_an_offset_matches_the_offset_made_before_effects_were_live() {
    for rotation in [0.0, 30.0] {
        let (mut live, id) = square();
        live.shape_mut(id).unwrap().rotation_deg = rotation;
        let source = live.shapes[0].clone();
        let today = crate::geometry::offset(&live.geometry(&source).rings(FLATTEN_TOL_IN), 0.25, Join::Round);
        add(&mut live, id, Effect::Offset(RING));
        let held = live.apply(&Edit::Expand, &[id]).unwrap();
        assert_eq!(live.shapes.len(), 2, "a ring shape and the square");
        assert_eq!(live.shapes[1].id, id, "the square keeps its id, in front");
        assert!(live.shapes[1].effects.is_empty() && live.shapes[1].kind == ShapeKind::Square, "and is a plain square again");
        assert_eq!(held.len(), 2);
        let ring = |d: &Design, at: usize| d.geometry(&d.shapes[at]).rings(FLATTEN_TOL_IN);
        // A round corner is a polygon of ARC_STEPS sides. The live ring builds it in the shape's frame and
        // the old Offset in the canvas's, so turned shapes differ by at most that polygon's sagitta.
        let sagitta = 0.25 * (1.0 - (std::f32::consts::PI / crate::geometry::ARC_STEPS as f32).cos());
        let tol = if rotation == 0.0 { 1e-3 } else { sagitta + 1e-4 };
        assert_same_rings(&ring(&live, 0), &today, tol);
        assert_eq!((live.shapes[0].fill, live.shapes[0].operation), (source.fill, source.operation));
    }
}

#[test]
fn reordering_a_warp_above_an_offset_warps_the_ring_too() {
    let (mut d, id) = square();
    d.shape_mut(id).unwrap().set_warp(Some(ARCH));
    add(&mut d, id, Effect::Offset(RING));
    let warped_box = (d.shapes[0].width_in, d.shapes[0].height_in);
    let warped_then_offset = geometry(&d, id);
    let ring = warped_then_offset.pieces()[0].bounds;
    assert!((width(ring) - (warped_box.0 + 0.5)).abs() < 2e-3, "the ring is 0.25 in round the arch, which fills its box");

    let at = entry(&d, id, 1);
    d.apply(&Edit::Effect { at, op: EffectEdit::MoveTo(0) }, &[id]).unwrap();
    assert!(d.shapes[0].effects.has_trailing_warp());
    let unwarped = (d.shapes[0].width_in, d.shapes[0].height_in);
    assert!((unwarped.0 - 2.0).abs() < 1e-4 && (unwarped.1 - 2.0).abs() < 1e-4, "the Warp no longer leads, so the box is the square's again");
    let offset_then_warped = geometry(&d, id);
    assert_ne!(warped_then_offset.pieces()[0].contours, offset_then_warped.pieces()[0].contours, "the ring differs");
    let (ring, body) = (offset_then_warped.pieces()[0].bounds, offset_then_warped.pieces()[1].bounds);
    assert!((width(ring) - 2.5).abs() < 2e-3 && (ring.max_y - ring.min_y - 2.5).abs() < 2e-3, "body and ring bend together, fitted back onto the offset's frame");
    assert!(width(body) < 2.0 - 1e-3 || body.max_y - body.min_y < 2.0 - 1e-3, "so the body no longer fills its box");
}

#[test]
fn zero_vanishing_and_nested_offsets_add_no_duplicate_piece() {
    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(Offset { distance_in: 0.0, ..RING }));
    assert_eq!(geometry(&d, id).pieces().len(), 1, "distance 0 adds nothing to cut twice");
    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(Offset { distance_in: -1.5, ..RING }));
    assert_eq!(geometry(&d, id).pieces().len(), 1, "an inset that eats the square adds nothing");

    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(RING));
    add(&mut d, id, Effect::Offset(RING));
    let g = geometry(&d, id);
    assert_eq!(g.pieces().len(), 3);
    let (mut alone, alone_id) = square();
    add(&mut alone, alone_id, Effect::Offset(Offset { distance_in: 0.5, ..RING }));
    let expected = geometry(&alone, alone_id).pieces()[0].bounds;
    assert_close(g.pieces()[0].bounds, expected, 2e-3);
}

#[test]
fn every_piece_id_in_an_effected_geometry_is_distinct() {
    let (mut d, id) = square();
    d.shape_mut(id).unwrap().set_warp(Some(ARCH));
    add(&mut d, id, Effect::Offset(RING));
    add(&mut d, id, Effect::Offset(RING));
    let g = geometry(&d, id);
    let ids: std::collections::HashSet<_> = g.pieces().iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), g.pieces().len());
}

#[test]
fn a_sticker_on_a_basic_cut_square_goes_on_a_print_then_cut_mat_with_a_blade_and_fiducial() {
    let (mut d, id) = square();
    add(&mut d, id, Effect::Sticker(Sticker { border_in: 0.125, corners: Corners::Round }));
    assert_eq!(d.shapes[0].operation, Operation::BasicCut, "the layer keeps its own operation");
    let g = geometry(&d, id);
    assert_eq!(g.pieces().len(), 1);
    assert_eq!(g.pieces()[0].style.operation, Operation::PrintThenCut);
    assert!((width(g.bounds()) - 2.25).abs() < 1e-3, "the backing goes round the square");

    let mut flow = crate::make_flow::MakeFlow::new(d.clone(), None);
    flow.material = Some(&crate::material::MATERIALS[0]);
    let mat = flow.mat().unwrap().clone();
    assert!(mat.print_then_cut);
    let loads = flow.tool_loads();
    assert_eq!(loads.len(), 1, "{loads:?}");
    assert_eq!(loads[0].pen, None, "a blade, not a pen");
    let paths = crate::bridge::build_mat_path_data(&d, &mat, &flow.setup, &crate::material::MATERIALS[0]);
    assert_eq!((paths.fiducial_id, paths.image_data[0].fiducial_id), (1, 1));
    assert!(paths.image_data[0].path_id.ends_with("-e0"), "{}", paths.image_data[0].path_id);
}

#[test]
fn an_offset_with_its_own_style_cuts_with_that_operation_and_colour() {
    let (mut d, id) = square();
    let style = Style { fill: Color32::RED, operation: Operation::Score, pen: Default::default() };
    add(&mut d, id, Effect::Offset(Offset { style: Some(style), ..RING }));
    let mats = crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
    let ring = mats.iter().flat_map(|m| m.items.iter().map(move |i| (m, i))).find(|(_, i)| i.piece.local != Local::Body).expect("the ring is cut");
    assert!(ring.0.operations.contains(&Operation::Score));
    assert_eq!(d.piece_style(ring.1.piece).map(|s| (s.operation, s.fill)), Some((Operation::Score, Color32::RED)));
}

#[test]
fn flatten_refuses_a_live_offset_until_expanded_but_combine_and_a_sticker_group_take_it() {
    let (mut d, id) = square();
    let other = d.add_shape(ShapeKind::Circle);
    add(&mut d, id, Effect::Offset(RING));
    assert_eq!(d.check(&Edit::Flatten, &[id, other]), Err(Blocked::LiveEffects));
    for edit in [
        Edit::Combine(crate::design::Combine::Weld),
        Edit::AddEffect { effect: Effect::Sticker(Sticker { border_in: 0.1, corners: Corners::Round }), reach: Reach::Together },
    ] {
        assert_eq!(d.check(&edit, &[id, other]), Ok(()), "{edit:?} groups them live");
    }
    let at = entry(&d, id, 0);
    d.apply(&Edit::Effect { at, op: EffectEdit::Enable(false) }, &[id]).unwrap();
    assert_eq!(d.check(&Edit::Flatten, &[id, other]), Ok(()), "a disabled Offset is not live");
}

#[test]
fn a_sticker_is_refused_on_a_container_but_an_offset_offsets_its_frame() {
    let (mut d, id) = square();
    d.contain(&[id]).unwrap();
    let sticker = Edit::AddEffect { effect: Effect::Sticker(Sticker { border_in: 0.1, corners: Corners::Round }), reach: Reach::Together };
    assert_eq!(d.check(&sticker, &[id]), Err(Blocked::StickerOnContainer));
    add(&mut d, id, Effect::Offset(RING));
    assert_eq!(geometry(&d, id).pieces().len(), 2);
}

#[test]
fn edits_refuse_a_stale_or_wrong_target() {
    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(RING));
    let at = entry(&d, id, 0);
    let warp = Edit::Effect { at, op: EffectEdit::Replace(Effect::Warp(ARCH)) };
    assert_eq!(d.check(&warp, &[id]), Err(Blocked::WrongEffect));
    d.apply(&Edit::Effect { at, op: EffectEdit::Remove }, &[id]).unwrap();
    assert_eq!(d.check(&Edit::Effect { at, op: EffectEdit::Enable(true) }, &[id]), Err(Blocked::NoEntry));
    add(&mut d, id, Effect::Offset(RING));
    assert_ne!(entry(&d, id, 0), at, "the new entry has a new id");
    let pinned = EntryRef { shape: id, row: Row::Contour };
    d.shape_mut(id).unwrap().contours.hidden = vec![true];
    assert_eq!(d.check(&Edit::Effect { at: pinned, op: EffectEdit::MoveTo(1) }, &[id]), Err(Blocked::PinnedRow));
}

#[test]
fn the_curve_eye_refits_the_text_and_the_contour_eye_shows_every_contour() {
    let mut d = Design::default();
    let t = d.add_text("Curved".into(), crate::text::FontRef::Inter, 0.5, Color32::BLACK, 1.0, 1.0);
    let straight = (d.shapes[0].width_in, d.shapes[0].height_in);
    d.set_text_curve(t, 3.0);
    let curved = (d.shapes[0].width_in, d.shapes[0].height_in);
    let eye = |on| Edit::Effect { at: EntryRef { shape: t, row: Row::Curve }, op: EffectEdit::Enable(on) };
    d.apply(&eye(false), &[t]).unwrap();
    let off = (d.shapes[0].width_in, d.shapes[0].height_in);
    assert!((off.0 - straight.0).abs() < 1e-4 && (off.1 - straight.1).abs() < 1e-4, "the straight layout gets its own box: {off:?} vs {straight:?}");
    d.apply(&eye(true), &[t]).unwrap();
    let on = (d.shapes[0].width_in, d.shapes[0].height_in);
    assert!((on.0 - curved.0).abs() < 1e-4 && (on.1 - curved.1).abs() < 1e-4);

    let (mut d, id) = square();
    d.shape_mut(id).unwrap().kind = ShapeKind::Imported {
        contours: [(0.0, 4.0), (1.0, 3.0)]
            .map(|(a, b): (f32, f32)| crate::design::Contour { start: (a, a), segs: [(b, a), (b, b), (a, b)].map(crate::design::Seg::Line).to_vec(), closed: true })
            .to_vec(),
        view_box: (4.0, 4.0),
    };
    d.shape_mut(id).unwrap().contours.hidden = vec![false, true];
    assert_eq!(geometry(&d, id).contours().count(), 1);
    d.apply(&Edit::Effect { at: EntryRef { shape: id, row: Row::Contour }, op: EffectEdit::Enable(false) }, &[id]).unwrap();
    assert_eq!(geometry(&d, id).contours().count(), 2, "the eye shows the hidden contour");
    assert_eq!(d.shapes[0].contours.hidden, [false, true], "without losing which one");
}

#[test]
fn split_letters_is_refused_on_text_with_an_offset() {
    let mut d = Design::default();
    let t = d.add_text("Hey".into(), crate::text::FontRef::Inter, 0.5, Color32::BLACK, 0.0, 0.0);
    add(&mut d, t, Effect::Offset(RING));
    let split = Edit::Text(crate::text_ops::TextChange::Split(crate::text_ops::Split::Letters));
    assert_eq!(d.check(&split, &[t]), Err(Blocked::SplitEffects));
}

#[test]
fn expand_turns_a_sticker_into_todays_flattened_sticker() {
    let (mut live, id) = square();
    let today = crate::geometry::offset(&live.geometry(&live.shapes[0]).rings(FLATTEN_TOL_IN), 0.125, Join::Round);
    add(&mut live, id, Effect::Sticker(Sticker { border_in: 0.125, corners: Corners::Round }));
    live.apply(&Edit::Expand, &[id]).unwrap();
    assert_eq!(live.shapes.len(), 1);
    let a = &live.shapes[0];
    assert_eq!(a.id, id, "the sticker keeps the square's id");
    let ShapeKind::Flattened { parts, .. } = &a.kind else { panic!("{:?}", a.kind) };
    assert_eq!((parts.len(), parts[0].fill, a.operation), (2, Color32::WHITE, Operation::PrintThenCut), "a white backing and the square");
    assert_same_rings(&live.geometry(a).rings(FLATTEN_TOL_IN), &today, 1e-3);
}

#[test]
fn the_first_node_edit_bakes_a_leading_warp_and_keeps_the_offset_cut() {
    let (mut d, id) = square();
    let star = crate::design::shape_from_contours(id, ShapeKind::Star.outline(2.0, 2.0), Color32::BLACK, Operation::BasicCut).unwrap();
    d.shapes[0] = crate::design::Shape { x_in: 3.0, y_in: 3.0, rotation_deg: 20.0, ..star };
    d.shape_mut(id).unwrap().set_warp(Some(ARCH));
    add(&mut d, id, Effect::Offset(RING));
    assert_eq!(crate::path_edit::notice(&d.shapes[0]), Some("Editing nodes applies the warp."));
    let before = d.geometry(&d.shapes[0]).rings(FLATTEN_TOL_IN);
    let edited = crate::path_edit::NodePath::read(&d.shapes[0]).write(&d.shapes[0]);
    assert_eq!(edited.effects.warp(), None, "the warp is baked into the nodes");
    assert!(matches!(edited.effects.entries(), [e] if matches!(e.effect, Effect::Offset(_))), "the Offset stays live");
    assert_same_rings(&d.geometry(&edited).rings(FLATTEN_TOL_IN), &before, 1e-3);

    let at = entry(&d, id, 1);
    d.apply(&Edit::Effect { at, op: EffectEdit::MoveTo(0) }, &[id]).unwrap();
    assert_eq!(crate::path_edit::refusal(&d.shapes[0]), Some("Expand to edit nodes of a warped offset."), "a Warp after an Offset refuses node editing");
}

#[test]
fn a_live_offset_reaches_the_cut_preview_toolpath() {
    let (mut d, id) = square();
    let plain = d.clone();
    add(&mut d, id, Effect::Offset(RING));
    let strokes = |d: &Design| {
        let mats = crate::composition::compose(d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
        let setup = crate::make_flow::MakeFlow::new(d.clone(), None).setup;
        crate::output::mat_toolpath(d, &mats[0], &setup, None, 1).strokes.len()
    };
    assert_eq!(strokes(&d), strokes(&plain) + 1, "the ring is cut too");
}

#[test]
fn a_warp_cannot_move_below_a_sticker_and_expand_never_reuses_an_id() {
    let (mut d, id) = square();
    d.shape_mut(id).unwrap().set_warp(Some(ARCH));
    add(&mut d, id, Effect::Sticker(Sticker { border_in: 0.1, corners: Corners::Round }));
    let warp = entry(&d, id, 0);
    assert_eq!(d.check(&Edit::Effect { at: warp, op: EffectEdit::MoveTo(1) }, &[id]), Err(Blocked::WarpAfterSticker));

    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(RING));
    let first = entry(&d, id, 0);
    d.apply(&Edit::Expand, &[id]).unwrap();
    add(&mut d, id, Effect::Offset(RING));
    assert_ne!(entry(&d, id, 0), first, "the square keeps counting after Expand");
}

#[test]
fn a_toolbar_warp_over_several_shapes_warps_each() {
    let (mut d, id) = square();
    let other = d.add_shape(ShapeKind::Circle);
    d.apply(&Edit::AddEffect { effect: Effect::Warp(ARCH), reach: Reach::Together }, &[id, other]).unwrap();
    assert!(d.shapes.iter().all(|s| s.effects.warp() == Some(ARCH)));
}

#[test]
fn editing_a_turned_off_curve_keeps_it_off() {
    let mut d = Design::default();
    let t = d.add_text("Curved".into(), crate::text::FontRef::Inter, 0.5, Color32::BLACK, 1.0, 1.0);
    d.set_text_curve(t, 3.0);
    d.apply(&Edit::Effect { at: EntryRef { shape: t, row: Row::Curve }, op: EffectEdit::Enable(false) }, &[t]).unwrap();
    d.set_text_curve(t, 4.0);
    assert!(matches!(d.shapes[0].kind, ShapeKind::Text { curve: Some(c), .. } if !c.enabled && c.diameter_in == 4.0));
}

/// A 2 in square and a 0.1 in bar beside it, as one imported shape.
fn square_and_bar() -> (Design, u64) {
    let ring =
        |x0: f32, x1: f32| crate::design::Contour { start: (x0, 0.0), segs: [(x1, 0.0), (x1, 2.0), (x0, 2.0)].map(crate::design::Seg::Line).to_vec(), closed: true };
    let mut d = Design::default();
    let id = d.add_shape(ShapeKind::Imported { contours: vec![ring(0.0, 2.0), ring(3.0, 3.1)], view_box: (3.1, 2.0) });
    let s = d.shape_mut(id).unwrap();
    (s.width_in, s.height_in) = (3.1, 2.0);
    (d, id)
}

#[test]
fn an_inset_drops_a_bar_too_thin_for_it_and_reports_it() {
    let (mut d, id) = square_and_bar();
    add(&mut d, id, Effect::Offset(Offset { distance_in: 0.1, direction: Direction::Inset, join: Join::Miter { limit: 4.0 }, style: None }));
    let g = geometry(&d, id);
    let inset = g.pieces().iter().find(|p| p.id.local != Local::Body).expect("the square's inset remains");
    assert!((width(inset.bounds) - 1.8).abs() < 1e-3, "only the square's inset: {:?}", inset.bounds);
    let shape = &d.shapes[0];
    assert_eq!(d.too_thin(shape, shape.effects.entries()[0].id), Some(super::TooThin::Some));
}

#[test]
fn an_inset_that_removes_everything_stays_in_place_adding_nothing() {
    let (mut d, id) = square_and_bar();
    add(&mut d, id, Effect::Offset(Offset { distance_in: 1.5, direction: Direction::Inset, join: Join::Round, style: None }));
    let shape = &d.shapes[0];
    let entry = &shape.effects.entries()[0];
    assert!(entry.enabled, "the entry stays, still on");
    assert_eq!(geometry(&d, id).pieces().len(), 1, "it adds nothing to the body");
    assert_eq!(d.too_thin(shape, entry.id), Some(super::TooThin::All));
    let row = &super::rows(shape, &geometry(&d, id), crate::units::Units::Imperial)[0];
    assert!(row.enabled && row.idle && !row.active(), "its row looks turned off");
}

#[test]
fn an_outset_or_a_roomy_inset_reports_nothing() {
    let (mut d, id) = square();
    add(&mut d, id, Effect::Offset(RING));
    add(&mut d, id, Effect::Offset(Offset { distance_in: 0.25, direction: Direction::Inset, join: Join::Bevel, style: None }));
    let shape = &d.shapes[0];
    assert!(shape.effects.entries().iter().all(|e| d.too_thin(shape, e.id).is_none()));
}

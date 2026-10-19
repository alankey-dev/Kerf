//! Compounds: live boolean groups and effect groups, their settle rules, Expand and Make.

use std::collections::HashSet;

use egui::Color32;

use super::live_tests::assert_same_rings;
use super::ops::tests::assert_whole;
use super::{Corners, Effect, Local, Offset, PieceId, Reach};
use crate::containers::{Axis, ContainerRole, Cross, Flow, Sizing};
use crate::design::{Combine, CompoundOp, Design, Operation, Pt, Shape, ShapeKind};
use crate::edit::{Blocked, Edit};
use crate::geometry::{Boolean, FLATTEN_TOL_IN, Ring};
use crate::machine::MachineFamily;
use crate::transform::Field;

const RING: Offset = Offset { distance_in: 0.25, direction: crate::effects::Direction::Outset, join: crate::geometry::Join::Round, style: None };

fn square(d: &mut Design, (x, y): Pt, size: f32) -> u64 {
    let id = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (x, y, size, size);
    id
}

fn shape(d: &Design, id: u64) -> &Shape {
    d.shapes.iter().find(|s| s.id == id).unwrap()
}

fn rings(d: &Design, id: u64) -> Vec<Ring> {
    d.geometry(shape(d, id)).rings(FLATTEN_TOL_IN)
}

fn area(d: &Design, id: u64) -> f32 {
    crate::geometry::even_odd_area(&rings(d, id))
}

fn one(held: Vec<u64>) -> u64 {
    let [id] = held[..] else { panic!("one id, not {held:?}") };
    id
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn moved(d: &mut Design, id: u64, dx: f32) {
    let before = d.clone();
    d.shape_mut(id).unwrap().x_in += dx;
    d.settle(Some(&before));
}

fn together(d: &mut Design, ids: &[u64], effect: Effect) -> u64 {
    one(d.apply(&Edit::AddEffect { effect, reach: Reach::Together }, ids).unwrap())
}

#[test]
fn a_weld_group_follows_a_moved_child_and_expands_to_todays_weld() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 0.5), 2.0));
    let weld = |d: &Design| crate::geometry::boolean(&[rings(d, a), rings(d, b)], Boolean::Weld);
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    assert_eq!(shape(&d, g).kind, ShapeKind::Compound(CompoundOp::Boolean(Combine::Weld)));
    assert_same_rings(&rings(&d, g), &weld(&d), 1e-3);
    assert_eq!(d.drawn().map(|s| s.id).collect::<Vec<_>>(), [g], "only the group draws");

    let before = rings(&d, g);
    moved(&mut d, b, 1.0);
    assert_ne!(rings(&d, g), before, "moving a child changes the weld");
    let today = weld(&d);
    assert_same_rings(&rings(&d, g), &today, 1e-3);

    assert_eq!(d.apply(&Edit::Expand, &[g]).unwrap(), [g], "the welded shape takes the group's id");
    assert_eq!(d.shapes.len(), 1, "the children were consumed");
    assert!(matches!(d.shapes[0].kind, ShapeKind::Imported { .. }));
    assert_same_rings(&rings(&d, g), &today, 1e-3);
}

#[test]
fn changing_unite_to_subtract_re_evaluates() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 1.0), 2.0));
    let g = one(d.apply(&Edit::Combine(Combine::Unite), &[a, b]).unwrap());
    assert_eq!(rings(&d, g).len(), 2, "Unite keeps every outline");
    d.apply(&Edit::BooleanOp(Combine::Subtract), &[g]).unwrap();
    assert_eq!(shape(&d, g).kind, ShapeKind::Compound(CompoundOp::Boolean(Combine::Subtract)));
    assert!(close(area(&d, g), 3.0), "the back square less the overlap: {}", area(&d, g));
    d.apply(&Edit::BooleanOp(Combine::Intersect), &[g]).unwrap();
    assert!(close(area(&d, g), 1.0), "the overlap: {}", area(&d, g));
}

#[test]
fn boolean_op_needs_a_boolean_group_and_slice_two_units() {
    let mut d = Design::default();
    let ids: Vec<u64> = (0..3).map(|i| square(&mut d, (i as f32, 0.0), 2.0)).collect();
    assert_eq!(d.check(&Edit::BooleanOp(Combine::Weld), &ids), Err(Blocked::NotBooleanGroup));
    assert_eq!(Blocked::NotBooleanGroup.reason(), "Select a combined group to change its operation.");
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &ids).unwrap());
    assert_eq!(d.check(&Edit::BooleanOp(Combine::Slice), &[g]), Err(Blocked::SliceNeedsTwo));
    assert_eq!(d.check(&Edit::BooleanOp(Combine::Exclude), &[g]), Ok(()));
    let effects = together(&mut d, &[g], Effect::Offset(RING));
    assert_eq!(d.check(&Edit::BooleanOp(Combine::Weld), &[effects]), Ok(()), "an effect group is not a boolean group, but the subject has one");
}

#[test]
fn a_compound_inside_a_compound_evaluates_with_each_piece_where_its_owner_draws_it() {
    let mut d = Design::default();
    let (a, b, c) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 0.0), 2.0), square(&mut d, (5.0, 0.0), 1.0));
    let inner = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    let outer = together(&mut d, &[inner, c], Effect::Offset(RING));
    assert_eq!(d.drawn().map(|s| s.id).collect::<Vec<_>>(), [outer]);
    let g = d.geometry(shape(&d, outer));
    let owners: Vec<(u64, Local)> = g.pieces().iter().map(|p| (p.id.owner, p.id.local)).collect();
    assert_eq!(owners.len(), 3, "{owners:?}");
    assert!(matches!(owners[0], (o, Local::Effect(_)) if o == outer), "the group's ring sits behind: {owners:?}");
    assert_eq!(owners[1..], [(inner, Local::Body), (c, Local::Body)]);
    for piece in &g.pieces()[1..] {
        let own = d.geometry(shape(&d, piece.id.owner));
        let theirs = own.piece(piece.id).expect("the owner has the piece too");
        assert_same_rings(&g.piece_rings(piece, FLATTEN_TOL_IN), &own.piece_rings(theirs, FLATTEN_TOL_IN), 1e-4);
    }
    moved(&mut d, a, -0.5);
    assert_ne!(d.geometry(shape(&d, outer)).pieces()[1].contours, g.pieces()[1].contours, "a grandchild's move reaches the outer group");
}

#[test]
fn every_piece_id_in_a_compound_geometry_is_distinct() {
    let mut d = Design::default();
    let (a, b, c) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 1.0), 2.0), square(&mut d, (4.0, 0.0), 1.0));
    for id in [a, b] {
        d.apply(&Edit::AddEffect { effect: Effect::Offset(RING), reach: Reach::EachShape }, &[id]).unwrap();
    }
    let effects = together(&mut d, &[a, b], Effect::Offset(RING));
    let slice = one(d.apply(&Edit::Combine(Combine::Slice), &[effects, c]).unwrap());
    let mut all = HashSet::new();
    for s in &d.shapes {
        let g = d.geometry(s);
        let ids: HashSet<PieceId> = g.pieces().iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), g.pieces().len(), "{} {:?}", s.id, s.kind);
    }
    for p in d.geometry(shape(&d, slice)).pieces() {
        assert!(all.insert(p.id));
        assert!(matches!(p.id.local, Local::Slice(_)) && p.id.owner == slice);
    }
    assert_eq!(all.len(), 2, "the disjoint square and the group: back minus front and front, no overlap");
}

#[test]
fn make_cuts_two_childrens_offsets_and_the_groups_own_once_each() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (3.0, 0.0), 1.0));
    for id in [a, b] {
        d.apply(&Edit::AddEffect { effect: Effect::Offset(Offset { distance_in: 0.1, ..RING }), reach: Reach::EachShape }, &[id]).unwrap();
    }
    let g = together(&mut d, &[a, b], Effect::Offset(RING));
    assert_whole(&d, &[]);
    let mats = crate::composition::compose(&d, MachineFamily::StudioPro.preferred_mat(), 1);
    let offsets: Vec<u64> = mats.iter().flat_map(|m| &m.items).filter(|i| matches!(i.piece.local, Local::Effect(_))).map(|i| i.piece.owner).collect();
    let mut sorted = offsets.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, [a, b, g], "three distinct offsets, once each: {offsets:?}");
}

#[test]
fn an_attached_pen_label_inside_a_welded_offset_lands_on_its_cuts_mat() {
    let mut d = Design::default();
    let cut = square(&mut d, (0.0, 0.0), 2.0);
    let label = square(&mut d, (0.5, 0.5), 0.5);
    let s = d.shape_mut(cut).unwrap();
    s.fill = Color32::RED;
    let s = d.shape_mut(label).unwrap();
    (s.fill, s.operation) = (Color32::BLACK, Operation::Pen);
    d.attach(&[cut, label]).unwrap();
    let other = square(&mut d, (4.0, 0.0), 1.0);
    together(&mut d, &[cut, label, other], Effect::Offset(RING));
    let mats = crate::composition::compose(&d, MachineFamily::StudioPro.preferred_mat(), 1);
    let mat_of = |id: u64| mats.iter().position(|m| m.items.iter().any(|i| i.piece == PieceId { owner: id, local: Local::Body })).unwrap();
    assert_eq!(mat_of(label), mat_of(cut), "the label keeps registration with its cut");
    assert_eq!(mats[mat_of(cut)].colour, Color32::RED);
}

#[test]
fn weld_flatten_and_expand_on_a_compound_selection_leave_no_orphans_or_duplicate_cuts() {
    let build = || {
        let mut d = Design::default();
        let (a, b, c) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 1.0), 2.0), square(&mut d, (2.5, 0.0), 1.0));
        let effects = together(&mut d, &[a, b], Effect::Offset(RING));
        let weld = one(d.apply(&Edit::Combine(Combine::Subtract), &[effects, c]).unwrap());
        (d, weld, effects, c)
    };
    let (mut d, weld, ..) = build();
    assert_whole(&d, &[]);
    let other = square(&mut d, (6.0, 0.0), 1.0);
    let outer = one(d.apply(&Edit::Combine(Combine::Weld), &[weld, other]).unwrap());
    assert_whole(&d, &[]);
    let mut held = vec![outer];
    while d.shapes.iter().any(|s| s.compound().is_some()) {
        held = d.apply(&Edit::Expand, &held).unwrap();
        assert_whole(&d, &[]);
    }
    assert_eq!(held, [outer]);

    let (mut d, weld, ..) = build();
    let other = square(&mut d, (6.0, 0.0), 1.0);
    let flat = one(d.apply(&Edit::Flatten, &[weld, other]).unwrap());
    assert_eq!(d.shapes.len(), 1, "the group's subtree was consumed");
    let ShapeKind::Flattened { parts, .. } = &shape(&d, flat).kind else { panic!("flattened") };
    assert!(parts.iter().all(|p| p.compound().is_none() && p.parent.is_none()), "no part is a group");
    assert_eq!(parts.len(), 2, "the subtract result, its inputs consumed into it, and the square");
    assert_whole(&d, &[]);

    let (mut d, _, effects_id, c) = build();
    let held = d.apply(&Edit::Expand, &[effects_id]).unwrap();
    assert_eq!(held.len(), 3, "the ring takes the group's id and the two children come out: {held:?}");
    assert_eq!(held[0], effects_id);
    assert!(d.shapes.iter().any(|s| s.id == c));
    assert_whole(&d, &[]);
}

#[test]
fn a_sticker_group_expands_to_one_flattened_sticker_of_every_child() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (2.0, 0.0), 1.0));
    d.shape_mut(b).unwrap().fill = Color32::RED;
    let g = together(&mut d, &[a, b], Effect::Sticker(super::Sticker { border_in: 0.125, corners: Corners::Round }));
    let backing = rings(&d, g);
    assert_eq!(d.apply(&Edit::Expand, &[g]).unwrap(), [g]);
    let ShapeKind::Flattened { parts, .. } = &d.shapes[0].kind else { panic!("{:?}", d.shapes[0].kind) };
    assert_eq!(parts.iter().map(|p| p.fill).collect::<Vec<_>>(), [Color32::WHITE, Color32::BLACK, Color32::RED]);
    assert_eq!(d.shapes.len(), 1);
    assert_same_rings(&rings(&d, g), &backing, 1e-3);
}

#[test]
fn a_rotate_drag_in_steps_reads_90_with_the_children_turned_90() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (3.0, 0.0), 1.0));
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    let start = d.clone();
    let centre = shape(&d, g).centre_in();
    for step in 1..=9 {
        let before = d.clone();
        let turned = crate::transform::rotate_many(&start, std::slice::from_ref(shape(&start, g)), 10.0 * step as f32);
        *d.shape_mut(g).unwrap() = turned[0].clone();
        d.settle(Some(&before));
    }
    assert!(close(shape(&d, g).rotation_deg, 90.0), "{}", shape(&d, g).rotation_deg);
    for id in [a, b] {
        let (was, now) = (shape(&start, id), shape(&d, id));
        assert!(close(now.rotation_deg, 90.0), "{id} turned {}", now.rotation_deg);
        let (dx, dy) = (was.centre_in().0 - centre.0, was.centre_in().1 - centre.1);
        let want = (centre.0 - dy, centre.1 + dx);
        assert!(close(now.centre_in().0, want.0) && close(now.centre_in().1, want.1), "{id} at {:?}, not {want:?}", now.centre_in());
        assert!(close(now.width_in, was.width_in) && close(now.height_in, was.height_in));
    }
    let settled = d.clone();
    d.settle(None);
    assert!(close(shape(&d, g).width_in, shape(&settled, g).width_in) && close(shape(&d, g).rotation_deg, 90.0), "the box still hugs the children");
}

#[test]
fn a_typed_width_reads_back_and_flips_mirror_the_children() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (3.0, 1.0), 1.0));
    let g = one(d.apply(&Edit::Combine(Combine::Unite), &[a, b]).unwrap());
    assert!(close(shape(&d, g).width_in, 4.0) && close(shape(&d, g).height_in, 2.0), "the group hugs its children");
    let before = d.clone();
    d.apply(&Edit::Field { field: Field::W, value: crate::expr::Typed::To(6.0), board_in: (12.0, 12.0) }, &[g]).unwrap();
    d.settle(Some(&before));
    assert!(close(shape(&d, g).width_in, 6.0), "reads back {}", shape(&d, g).width_in);
    assert!(close(shape(&d, a).width_in, 3.0) && close(shape(&d, b).x_in, 4.5), "children stretch with the box");
    d.settle(None);
    assert!(close(shape(&d, g).width_in, 6.0), "a refit agrees with the stretched children");

    let before = d.clone();
    d.apply(&Edit::Flip(crate::transform::Axis::Horizontal), &[g]).unwrap();
    d.settle(Some(&before));
    assert!(shape(&d, a).flip_x && shape(&d, b).flip_x, "children mirror with the group");
    assert!(close(shape(&d, b).x_in, 0.0), "the right child is now on the left: {}", shape(&d, b).x_in);
}

#[test]
fn a_compound_in_a_hug_row_settles_to_the_same_design_twice() {
    let mut d = Design::default();
    let (a, b, c) = (square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (0.5, 0.5), 1.0), square(&mut d, (4.0, 0.0), 1.0));
    let row = d.contain(&[a, b, c]).unwrap();
    d.shape_mut(row).unwrap().container =
        Some(ContainerRole { padding_in: 0.25, layout: Some(Flow { axis: Axis::Row, gap_in: 0.25, cross: Cross::Start, sizing: Sizing::Hug }) });
    d.settle(None);
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    assert_eq!(shape(&d, g).parent, Some(row));
    let once = d.clone();
    d.settle(None);
    assert_eq!(d, once, "settle is idempotent");
    moved(&mut d, a, 0.5);
    let grown = d.clone();
    d.settle(None);
    assert_eq!(d, grown, "and stays put after a child's edit");
    let g_box = |d: &Design| crate::effects::Placement::of(shape(d, g));
    assert!(close(shape(&d, row).width_in, 0.25 + g_box(&d).half.0 * 2.0 + 0.25 + 1.0 + 0.25), "the row hugs the refitted group");
}

#[test]
fn a_compound_dissolves_with_no_children_and_stays_with_one() {
    let mut d = Design::default();
    let ids: Vec<u64> = (0..3).map(|i| square(&mut d, (i as f32 * 2.0, 0.0), 1.0)).collect();
    let g = one(d.apply(&Edit::Combine(Combine::Subtract), &ids[..2]).unwrap());
    d.remove_many(&ids[..1]);
    d.settle(None);
    assert!(d.shapes.iter().any(|s| s.id == g), "a boolean group with one child is kept");
    d.apply(&Edit::SetHidden(true), &ids[1..2]).unwrap();
    d.settle(None);
    assert!(d.shapes.iter().any(|s| s.id == g), "and with every child hidden");
    d.remove_many(&ids[1..2]);
    d.settle(None);
    assert!(!d.shapes.iter().any(|s| s.id == g), "with none it dissolves");
}

#[test]
fn hit_test_deep_finds_the_front_child_inside_a_subtract_hole() {
    let mut d = Design::default();
    let (back, front) = (square(&mut d, (0.0, 0.0), 4.0), square(&mut d, (1.0, 1.0), 2.0));
    let g = one(d.apply(&Edit::Combine(Combine::Subtract), &[back, front]).unwrap());
    assert_eq!(d.hit_test(0.5, 0.5), Some(g), "the canvas hits the group");
    assert_eq!(d.hit_test_deep((2.0, 2.0)), Some(front));
    assert_eq!(d.hit_test_deep((0.5, 0.5)), Some(back));
}

#[test]
fn wrapping_non_adjacent_roots_puts_the_group_at_the_back_most_slot() {
    let mut d = Design::default();
    let ids: Vec<u64> = (0..5).map(|i| square(&mut d, (i as f32 * 2.0, 0.0), 1.0)).collect();
    d.group(&[ids[3], ids[4]]).unwrap();
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[ids[1], ids[3]]).unwrap());
    let order: Vec<u64> = d.shapes.iter().map(|s| s.id).collect();
    assert_eq!(order, [ids[0], g, ids[1], ids[3], ids[2], ids[4]]);
    assert!(shape(&d, ids[3]).link.is_none() && shape(&d, ids[4]).link.is_none(), "a link the group splits loses its members");
}

#[test]
fn canvas_drops_leave_a_compounds_children_but_layers_moves_them_in() {
    let mut d = Design::default();
    let (a, b, c) = (square(&mut d, (0.0, 0.0), 1.0), square(&mut d, (2.0, 0.0), 1.0), square(&mut d, (6.0, 0.0), 1.0));
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    d.drop_at(&[a], (20.0, 20.0));
    assert_eq!(shape(&d, a).parent, Some(g), "a canvas drop leaves the child in its group");
    let plan = crate::layers_dnd::plan_drop(&d, &[c], crate::layers_dnd::DropTarget::Into(g));
    crate::layers_dnd::apply_drop(&mut d, &plan);
    assert_eq!(shape(&d, c).parent, Some(g), "Layers drops a third shape into the weld");
    assert_eq!(d.children(Some(g)).count(), 3);
}

#[test]
fn a_group_refuses_a_warp_saying_to_expand_it() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 1.0), 2.0));
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    let warp = crate::warp::Warp { style: crate::warp::WarpStyle::Arch, bend: 0.5, vertical: false };
    assert_eq!(d.check(&Edit::Warp(Some(warp)), &[g]), Err(Blocked::GroupCannotWarp));
    assert_eq!(Blocked::GroupCannotWarp.reason(), "Expand the group to warp what it makes.");
}

#[test]
fn a_child_joining_or_leaving_inside_an_edit_refits_the_group_and_leaves_the_others_be() {
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (1.0, 0.0), 2.0));
    let c = square(&mut d, (2.5, 0.0), 2.0);
    let g = one(d.apply(&Edit::Combine(Combine::Weld), &[a, b]).unwrap());
    let kept = |d: &Design| [a, b].map(|id| shape(d, id).clone());
    let was = kept(&d);
    let before = d.clone();
    d.reparent(&[c], Some(g));
    d.settle(Some(&before));
    assert!(close(shape(&d, g).width_in, 4.5), "the group refits round its new child: {}", shape(&d, g).width_in);
    assert_eq!(kept(&d), was, "the children it had keep their size and place");
    let before = d.clone();
    d.reparent(&[c], None);
    d.settle(Some(&before));
    assert!(close(shape(&d, g).width_in, 3.0), "it refits again when the child leaves");
    assert_eq!(kept(&d), was);
}

#[test]
fn an_image_inside_an_effect_group_keeps_its_pixels_at_its_own_corners() {
    let mut d = Design::default();
    let image = d.add_shape(ShapeKind::Bitmap { asset: crate::assets::id_of(&image::RgbaImage::new(2, 2)), px: (2, 2), clip: Vec::new() });
    {
        let s = d.shape_mut(image).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg) = (1.0, 1.0, 2.0, 1.0, 30.0);
    }
    let other = square(&mut d, (4.0, 1.0), 1.0);
    let group = together(&mut d, &[image, other], Effect::Offset(RING));
    let expected = super::image_corners((2.0, 1.0), super::Placement::of(shape(&d, image)).placer());
    let geometry = d.geometry(shape(&d, group));
    let piece = geometry.piece(PieceId { owner: image, local: Local::Body }).expect("the image draws in the group");
    let super::Paint::Print(layers) = &piece.paint else { panic!("{:?}", piece.paint) };
    let [(_, super::Ink::Image { corners, .. })] = &layers[..] else { panic!("{layers:?}") };
    for (got, want) in corners.iter().zip(expected) {
        assert!(close(got.0, want.0) && close(got.1, want.1), "{corners:?} vs {expected:?}");
    }
}

#[test]
fn an_effect_groups_offset_takes_the_same_inset_and_corners() {
    use super::{Direction, TooThin};
    use crate::geometry::{DEFAULT_MITER_LIMIT, Join};
    let mut d = Design::default();
    let (a, b) = (square(&mut d, (0.0, 0.0), 2.0), square(&mut d, (4.0, 0.0), 2.0));
    let thin = square(&mut d, (8.0, 0.0), 0.1);
    let miter = Offset { join: Join::Miter { limit: DEFAULT_MITER_LIMIT }, ..RING };
    let g = together(&mut d, &[a, b], Effect::Offset(miter));
    let entry = shape(&d, g).effects.entries()[0].id;
    let ring = |d: &Design| {
        let geometry = d.geometry(shape(d, g));
        crate::geometry::even_odd_area(&geometry.piece_rings(geometry.piece(PieceId { owner: g, local: Local::Effect(entry) }).unwrap(), FLATTEN_TOL_IN))
    };
    assert!(close(ring(&d), 2.0 * 2.5 * 2.5), "a mitred outset keeps square corners: {}", ring(&d));

    let inset = Offset { distance_in: 0.1, direction: Direction::Inset, join: Join::Bevel, style: None };
    d.apply(&Edit::Effect { at: super::EntryRef { shape: g, row: super::Row::Entry(entry) }, op: super::EffectEdit::Replace(Effect::Offset(inset)) }, &[g]).unwrap();
    assert!(close(ring(&d), 2.0 * 1.8 * 1.8), "each child inset by 0.1: {}", ring(&d));
    let geometry = d.geometry(shape(&d, g));
    assert_eq!(geometry.pieces().last().unwrap().id.local, Local::Effect(entry), "the inset draws in front");
    assert_eq!(d.too_thin(shape(&d, g), entry), None, "both children are wide enough");

    d.reparent(&[thin], Some(g));
    d.settle(None);
    assert_eq!(d.too_thin(shape(&d, g), entry), Some(TooThin::Some), "the thin child is dropped and reported");

    let held = d.apply(&Edit::Expand, &[g]).unwrap();
    let front = d.shapes.last().unwrap();
    assert!(held.contains(&front.id) && close(front.width_in, 5.8), "the expanded inset sits in front: {front:?}");
}

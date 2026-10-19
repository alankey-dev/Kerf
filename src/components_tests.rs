use std::collections::BTreeMap;

use egui::Color32;

use super::*;
use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::edit::{Blocked, Edit};
use crate::geometry::FLATTEN_TOL_IN;

const MASTER: u64 = 100;

fn shape(id: u64, kind: ShapeKind, (x, y): Pt, (w, h): Pt, fill: Color32) -> Shape {
    Shape { x_in: x, y_in: y, width_in: w, height_in: h, fill, ..Shape::new(id, kind) }
}

/// A component 2 x 1 in: a black square, a red rectangle turned 30 degrees and a blue circle.
fn design() -> Design {
    let shapes = vec![
        shape(1, ShapeKind::Square, (0.0, 0.0), (1.0, 1.0), Color32::BLACK),
        Shape { rotation_deg: 30.0, ..shape(2, ShapeKind::Rectangle, (1.0, 0.25), (1.0, 0.5), Color32::RED) },
        shape(3, ShapeKind::Circle, (1.25, 0.0), (0.5, 0.5), Color32::BLUE),
    ];
    Design { components: vec![Component { id: MASTER, name: "Tag".into(), shapes, size_in: (2.0, 1.0) }], ..Default::default() }
}

fn place(d: &mut Design, (x, y): Pt, (w, h): Pt, rotation_deg: f32) -> u64 {
    let id = d.place_instance(MASTER, None).unwrap();
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg) = (x, y, w, h, rotation_deg);
    id
}

fn get(d: &Design, id: u64) -> Shape {
    d.shapes.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no shape {id}")).clone()
}

/// The world positions of a shape's box corners and centre.
fn corners(s: &Shape) -> [Pt; 5] {
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.5, 0.5)].map(|(u, v)| s.to_world((u * s.width_in, v * s.height_in)))
}

fn near(a: Pt, b: Pt, tol: f32) -> bool {
    (a.0 - b.0).abs() < tol && (a.1 - b.1).abs() < tol
}

fn fills(d: &Design, id: u64) -> Vec<Color32> {
    d.geometry(&get(d, id)).pieces().iter().map(|p| p.style.fill).collect()
}

/// The summed area of every piece `s` draws.
fn area(d: &Design, s: &Shape) -> f32 {
    let g = d.geometry(s);
    g.pieces().iter().map(|p| crate::geometry::even_odd_area(&g.piece_rings(p, FLATTEN_TOL_IN))).sum()
}

#[test]
fn expand_places_each_master_shape_scaled_twice_and_turned_90_within_a_thousandth() {
    for flip_x in [false, true] {
        let mut d = design();
        let id = place(&mut d, (3.0, 3.0), (4.0, 2.0), 90.0);
        d.shape_mut(id).unwrap().flip_x = flip_x;
        let inst = get(&d, id);
        let parts = expand(&d, &inst).unwrap();
        let master = d.component(MASTER).unwrap().shapes.clone();
        assert_eq!(parts.len(), master.len());
        assert_eq!(part_ids(&d, &inst), parts.iter().map(|p| p.id).collect::<Vec<_>>(), "the ids a list expects are the parts' own");
        for (part, m) in parts.iter().zip(&master) {
            let want = corners(m).map(|(x, y)| inst.to_world((x * 2.0, y * 2.0)));
            for (got, want) in corners(part).into_iter().zip(want) {
                assert!(near(got, want, 1e-3), "flip {flip_x}: part {} corner {got:?}, want {want:?}", m.id);
            }
            assert_eq!(part.id, part_id(id, m.id));
            assert!(part.id >= 1 << 63, "a derived id never meets a stored one");
        }
        let union = parts.iter().map(|p| d.geometry(p).bounds()).fold(crate::snap::Aabb::EMPTY, crate::snap::Aabb::union);
        let drawn = d.geometry(&inst).bounds();
        assert!(near((drawn.min_x, drawn.min_y), (union.min_x, union.min_y), 1e-3) && near((drawn.max_x, drawn.max_y), (union.max_x, union.max_y), 1e-3));
    }
}

#[test]
fn every_piece_an_instance_draws_is_its_own_and_named_by_its_part() {
    let mut d = design();
    let id = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let geometry = d.geometry(&get(&d, id));
    let ids: Vec<_> = geometry.pieces().iter().map(|p| p.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|p| p.owner == id), "consumers find every piece through the instance");
    let parts: Vec<u64> = [1, 2, 3].map(|m| part_id(id, m)).into();
    assert_eq!(
        ids.iter().map(|p| p.local).collect::<Vec<_>>(),
        parts.iter().map(|&part| Local::Part { part, inner: crate::effects::PartLocal::Body }).collect::<Vec<_>>()
    );
    assert_eq!(ids[0].suffix(), format!("-p{}", parts[0]));
    assert_eq!(d.piece_style(ids[1]).map(|s| s.fill), Some(Color32::RED));
}

#[test]
fn resizing_a_master_shape_changes_every_instance_but_not_their_frames() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let b = place(&mut d, (5.0, 1.0), (4.0, 2.0), 45.0);
    let frames = [get(&d, a), get(&d, b)];
    let areas = frames.clone().map(|s| area(&d, &s));
    assert!(d.enter_master(MASTER));
    d.shape_mut(1).unwrap().width_in = 2.0;
    d.leave_master();
    for (was, before) in frames.iter().zip(areas) {
        let now = get(&d, was.id);
        assert_eq!(&now, was, "the instance's own frame is unchanged");
        let after = area(&d, &now);
        assert!(after > before + 0.5, "instance {} grew from {before} to {after}", was.id);
    }
    assert_eq!(d.component(MASTER).unwrap().size_in, (2.0, 1.0), "the component frame is fixed");
}

#[test]
fn a_fill_override_changes_only_that_instance_and_reset_overrides_restores_the_master() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let b = place(&mut d, (5.0, 1.0), (2.0, 1.0), 0.0);
    let change = PartChange::Fill(Color32::GREEN);
    d.apply(&Edit::Override { part: 2, change: change.clone() }, &[a]).unwrap();
    assert_eq!(fills(&d, a), [Color32::BLACK, Color32::GREEN, Color32::BLUE]);
    assert_eq!(fills(&d, b), [Color32::BLACK, Color32::RED, Color32::BLUE], "the other instance keeps the master fill");
    assert_eq!(d.check(&Edit::ResetOverrides, &[b]), Err(Blocked::NoOverrides));
    assert_eq!(d.check(&Edit::Override { part: 99, change: change.clone() }, &[a]), Err(Blocked::NoPart));
    assert_eq!(d.check(&Edit::Override { part: 2, change }, &[a, b]), Err(Blocked::NoInstance), "one instance at a time");
    d.apply(&Edit::ResetOverrides, &[a]).unwrap();
    assert_eq!(fills(&d, a), [Color32::BLACK, Color32::RED, Color32::BLUE]);
    assert!(matches!(&get(&d, a).kind, ShapeKind::Instance { overrides, .. } if overrides.is_empty()));
}

#[test]
fn override_operation_pen_and_hidden_reach_the_part_and_reset_part_clears_one() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.apply(&Edit::Override { part: 1, change: PartChange::Operation(Operation::Pen) }, &[a]).unwrap();
    d.apply(&Edit::Override { part: 3, change: PartChange::Hidden(true) }, &[a]).unwrap();
    let ops: Vec<Operation> = d.geometry(&get(&d, a)).pieces().iter().map(|p| p.style.operation).collect();
    assert_eq!(ops, [Operation::Pen, Operation::BasicCut], "the hidden circle draws nothing");
    d.apply(&Edit::ResetPart(3), &[a]).unwrap();
    assert_eq!(d.geometry(&get(&d, a)).pieces().len(), 3);
    assert_eq!(d.check(&Edit::ResetPart(3), &[a]), Err(Blocked::NoOverrides));
    d.apply(&Edit::Override { part: 1, change: PartChange::Hidden(false) }, &[a]).unwrap();
    let ShapeKind::Instance { overrides, .. } = &get(&d, a).kind else { panic!() };
    assert_eq!(overrides.keys().copied().collect::<Vec<_>>(), [1], "the operation override stays");
    d.set_override(a, 1, PartChange::Pen(crate::pens::Pen::default()));
    assert_eq!(d.geometry(&get(&d, a)).pieces()[0].style.pen, crate::pens::Pen::default());
}

#[test]
fn deleting_a_master_shape_drops_its_overrides_from_every_instance() {
    let mut d = design();
    let other = Component { id: 200, name: "Host".into(), shapes: Vec::new(), size_in: (2.0, 1.0) };
    d.components.push(other);
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(a, 2, PartChange::Fill(Color32::GREEN));
    d.set_override(a, 3, PartChange::Fill(Color32::GREEN));
    assert!(d.enter_master(200));
    let nested = place(&mut d, (0.0, 0.0), (2.0, 1.0), 0.0);
    d.set_override(nested, 2, PartChange::Fill(Color32::GREEN));
    d.leave_master();
    assert!(d.enter_master(MASTER));
    d.remove_many(&[2]);
    d.leave_master();
    let keys = |s: &Shape| match &s.kind {
        ShapeKind::Instance { overrides, .. } => overrides.keys().copied().collect::<Vec<_>>(),
        _ => panic!("not an instance"),
    };
    assert_eq!(keys(&get(&d, a)), [3], "the canvas instance keeps the override of a part that remains");
    assert!(keys(&d.component(200).unwrap().shapes[0]).is_empty(), "the instance inside another master loses it too");
}

#[test]
fn detach_yields_plain_shapes_equal_to_expand_grouped_with_no_instance_left() {
    let mut d = design();
    let a = place(&mut d, (2.0, 2.0), (3.0, 1.5), 90.0);
    d.set_override(a, 3, PartChange::Fill(Color32::GREEN));
    let parts = expand(&d, &get(&d, a)).unwrap();
    let held = d.apply(&Edit::DetachInstance, &[a]).unwrap();
    assert!(d.shapes.iter().all(|s| s.instance_of().is_none()), "no instance left");
    assert_eq!(held.len(), 3);
    let plain = |s: &Shape| Shape { id: 0, link: None, parent: None, ..s.clone() };
    let detached: Vec<Shape> = held.iter().map(|&id| plain(&get(&d, id))).collect();
    assert_eq!(detached, parts.iter().map(plain).collect::<Vec<_>>(), "the parts, overrides baked, bar ids");
    let link = get(&d, held[0]).link.expect("grouped");
    assert_eq!(link.kind, crate::links::LinkKind::Group);
    assert!(held.iter().all(|&id| get(&d, id).link == Some(link)));
    assert_eq!(d.check(&Edit::DetachInstance, &held), Err(Blocked::NoInstance));
}

#[test]
fn detaching_an_instance_with_an_offset_wraps_its_parts_in_an_effect_group() {
    let mut d = design();
    let a = place(&mut d, (2.0, 2.0), (2.0, 1.0), 0.0);
    let offset = crate::effects::Effect::Offset(crate::effects::Offset {
        distance_in: 0.25,
        direction: crate::effects::Direction::Outset,
        join: crate::geometry::Join::Round,
        style: None,
    });
    d.shape_mut(a).unwrap().effects.push(offset);
    let before = area(&d, &get(&d, a));
    let held = d.apply(&Edit::DetachInstance, &[a]).unwrap();
    let [group] = held[..] else { panic!("one effect group: {held:?}") };
    let g = get(&d, group);
    assert_eq!(g.compound(), Some(crate::design::CompoundOp::Effects));
    assert_eq!(d.children(Some(group)).count(), 3);
    assert!((area(&d, &g) - before).abs() < 1e-2, "it draws what the instance drew");
}

#[test]
fn geometry_through_the_pipeline_equals_what_detach_produces() {
    let mut d = design();
    let a = place(&mut d, (2.0, 2.0), (3.0, 2.0), 90.0);
    d.shape_mut(a).unwrap().flip_y = true;
    let g = d.geometry(&get(&d, a));
    let mut detached = d.clone();
    let held = detached.apply(&Edit::DetachInstance, &[a]).unwrap();
    assert_eq!(g.pieces().len(), held.len());
    for (piece, id) in g.pieces().iter().zip(&held) {
        let plain = detached.geometry(&get(&detached, *id));
        let (x, y) = (piece.bounds, plain.bounds());
        assert!(near((x.min_x, x.min_y), (y.min_x, y.min_y), 1e-3) && near((x.max_x, x.max_y), (y.max_x, y.max_y), 1e-3), "{x:?} vs {y:?}");
        let (pa, pb) = (crate::geometry::even_odd_area(&g.piece_rings(piece, FLATTEN_TOL_IN)), crate::geometry::even_odd_area(&plain.rings(FLATTEN_TOL_IN)));
        assert!((pa - pb).abs() < 1e-3, "area {pa} vs {pb}");
    }
}

#[test]
fn a_component_cannot_contain_itself_directly_or_through_another() {
    let mut d = design();
    d.components.push(Component { id: 200, name: "Outer".into(), shapes: Vec::new(), size_in: (2.0, 1.0) });
    assert!(d.enter_master(MASTER));
    assert_eq!(d.check(&Edit::PlaceInstance { component: MASTER, at: None }, &[]), Err(Blocked::ComponentCycle));
    assert_eq!(Blocked::ComponentCycle.reason(), "A component can't contain itself.");
    d.leave_master();
    assert!(d.enter_master(200));
    d.apply(&Edit::PlaceInstance { component: MASTER, at: None }, &[]).unwrap();
    d.leave_master();
    assert!(d.enter_master(MASTER));
    assert_eq!(d.check(&Edit::PlaceInstance { component: 200, at: None }, &[]), Err(Blocked::ComponentCycle), "200 holds 100");
    let made = d.apply(&Edit::MakeComponent, &[1]).unwrap();
    assert!(!would_cycle(&d, Some(MASTER), get(&d, made[0]).instance_of().unwrap()), "making a component inside a master is no cycle");
    d.leave_master();
    assert_eq!(d.check(&Edit::PlaceInstance { component: 200, at: None }, &[]), Ok(()), "the canvas is no component");
    assert_eq!(d.check(&Edit::PlaceInstance { component: 9, at: None }, &[]), Err(Blocked::NoComponent));
}

#[test]
fn a_hand_made_cycle_loses_the_instance_that_closes_it_on_load() {
    let mut d = design();
    d.components.push(Component { id: 200, name: "Outer".into(), shapes: Vec::new(), size_in: (2.0, 1.0) });
    let inner = |id: u64, of: u64| Shape { width_in: 2.0, ..Shape::new(id, ShapeKind::Instance { component: of, overrides: BTreeMap::new() }) };
    d.components[0].shapes.push(inner(10, 200));
    d.components[1].shapes.push(inner(11, MASTER));
    let repairs = d.sanitise();
    assert_eq!(repairs.dropped, [10]);
    assert!(!would_cycle(&d, Some(200), MASTER));
    let a = place(&mut d, (0.0, 0.0), (2.0, 1.0), 0.0);
    d.geometry(&get(&d, a));
}

#[test]
fn make_component_replaces_the_selection_with_one_instance_that_draws_the_same() {
    let mut d = Design::default();
    let ids: Vec<u64> = [(1.0, 1.0), (2.5, 1.0), (1.0, 3.0)]
        .into_iter()
        .map(|at| {
            let id = d.add_shape(ShapeKind::Heart);
            (d.shape_mut(id).unwrap().x_in, d.shape_mut(id).unwrap().y_in) = at;
            id
        })
        .collect();
    let lone = d.add_shape(ShapeKind::Star);
    d.group(&[ids[2], lone]);
    let before: f32 = ids.iter().map(|&id| area(&d, &get(&d, id))).sum();
    let bounds = |d: &Design, ids: &[u64]| ids.iter().map(|&id| d.geometry(&get(d, id)).bounds()).fold(crate::snap::Aabb::EMPTY, crate::snap::Aabb::union);
    let was = bounds(&d, &ids);
    let held = d.apply(&Edit::MakeComponent, &ids).unwrap();
    let [inst] = held[..] else { panic!("one instance") };
    assert_eq!(d.shapes.iter().map(|s| s.id).collect::<Vec<_>>(), [inst, lone], "the instance takes the back-most slot");
    assert_eq!(get(&d, lone).link, None, "the group it straddled lost the moved member");
    let c = &d.components[0];
    assert_eq!((c.name.as_str(), c.shapes.len()), ("Component 1", 3));
    assert!(c.shapes.iter().all(|s| s.link.is_none()));
    assert!((area(&d, &get(&d, inst)) - before).abs() < 1e-3);
    let now = bounds(&d, &[inst]);
    assert!(near((now.min_x, now.min_y), (was.min_x, was.min_y), 1e-4) && near((now.max_x, now.max_y), (was.max_x, was.max_y), 1e-4));
    let second = d.apply(&Edit::MakeComponent, &[lone]).unwrap();
    assert_eq!(d.components[1].name, "Component 2");
    assert!(second[0] > d.components[1].id, "component and shape ids share one counter");
}

#[test]
fn compose_keeps_an_instance_rigid_per_mat_and_spreads_two_colours_over_two_mats() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (4.0, 2.0), 0.0);
    d.set_override(a, 2, PartChange::Fill(Color32::BLACK));
    let mats = crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
    assert_eq!(mats.len(), 2, "black and blue");
    let g = d.geometry(&get(&d, a));
    let black = mats.iter().find(|m| m.colour == Color32::BLACK).unwrap();
    assert_eq!(black.items.len(), 2, "the square and the overridden rectangle");
    let bounds = |i: usize| g.piece(black.items[i].piece).unwrap().bounds;
    let mat_gap = (black.items[1].offset_in.0 - black.items[0].offset_in.0, black.items[1].offset_in.1 - black.items[0].offset_in.1);
    let world_gap = (bounds(1).min_x - bounds(0).min_x, bounds(1).min_y - bounds(0).min_y);
    assert!(near(mat_gap, world_gap, 1e-4), "the parts keep their layout: {mat_gap:?} vs {world_gap:?}");
    let blue = mats.iter().find(|m| m.colour == Color32::BLUE).unwrap();
    assert_eq!(blue.items.len(), 1);
    assert!(mats.iter().flat_map(|m| &m.items).all(|i| i.piece.owner == a));
}

#[test]
fn a_pen_part_rides_with_the_back_most_cut_part() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(a, 3, PartChange::Operation(Operation::Pen));
    let mats = crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1);
    let pen = mats.iter().find(|m| m.operations.contains(&Operation::Pen)).expect("a mat with the pen line");
    assert_eq!(pen.colour, Color32::BLACK, "with the black square, the back-most cut part");
}

#[test]
fn moving_forty_instances_re_places_them_without_evaluating_again() {
    let shapes = (0..5).map(|i| shape(i + 1, ShapeKind::Heart, (i as f32 * 0.5, 0.0), (0.5, 0.5), Color32::RED)).collect();
    let mut d = Design { components: vec![Component { id: MASTER, name: "Five".into(), shapes, size_in: (2.5, 0.5) }], ..Default::default() };
    let ids: Vec<u64> = (0..40).map(|i| place(&mut d, ((i % 4) as f32 * 3.0, (i / 4) as f32), (2.5, 0.5), 0.0)).collect();
    let draw = |d: &Design| d.drawn().map(|s| d.geometry(s).pieces().len()).sum::<usize>();
    assert_eq!(draw(&d), 200);
    let cold = d.evaluations();
    assert_eq!(cold, 40 * 6, "each part and each instance once");
    for &id in &ids {
        let s = d.shape_mut(id).unwrap();
        s.x_in += 1.0;
        s.rotation_deg = 30.0;
    }
    assert_eq!(draw(&d), 200);
    assert_eq!(d.evaluations(), cold, "a moved or turned instance is only placed again");
    d.shape_mut(ids[0]).unwrap().width_in = 5.0;
    draw(&d);
    assert_eq!(d.evaluations(), cold + 6, "a resized one evaluates its parts and itself");
}

#[test]
fn an_instance_of_a_missing_component_draws_its_box_as_a_guide() {
    let mut d = Design::default();
    d.shapes.push(Shape { x_in: 1.0, y_in: 1.0, width_in: 2.0, height_in: 1.0, ..Shape::new(1, ShapeKind::Instance { component: 9, overrides: BTreeMap::new() }) });
    let g = d.geometry(&d.shapes[0]);
    let [piece] = g.pieces() else { panic!("one box") };
    assert_eq!(piece.style.operation, Operation::Guide);
    assert!(near((piece.bounds.width(), piece.bounds.height()), (2.0, 1.0), 1e-4));
    assert!(crate::composition::compose(&d, crate::machine::MachineFamily::StudioPro.preferred_mat(), 1).is_empty(), "nothing to make");
}

#[test]
fn delete_component_detaches_its_instances_everywhere_first() {
    let mut d = design();
    d.components.push(Component { id: 200, name: "Host".into(), shapes: Vec::new(), size_in: (2.0, 1.0) });
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    assert!(d.enter_master(200));
    place(&mut d, (0.0, 0.0), (2.0, 1.0), 0.0);
    d.leave_master();
    let mut sel = vec![a];
    crate::edit::run(&mut d, &mut sel, &Edit::DeleteComponent(MASTER), crate::edit::Subject::Selection).unwrap();
    assert!(sel.is_empty(), "the detached instance leaves the selection");
    assert!(d.component(MASTER).is_none());
    assert_eq!(d.shapes.len(), 3, "the canvas holds the parts");
    assert_eq!(d.component(200).unwrap().shapes.len(), 3, "so does the other master");
    assert!(d.shapes.iter().chain(&d.component(200).unwrap().shapes).all(|s| s.instance_of().is_none()));
    assert_eq!(d.check(&Edit::DeleteComponent(MASTER), &[]), Err(Blocked::NoComponent));
}

#[test]
fn duplicate_and_rename_make_a_separate_component() {
    let mut d = design();
    d.apply(&Edit::DuplicateComponent(MASTER), &[]).unwrap();
    let copy = d.components[1].clone();
    assert_eq!((copy.name.as_str(), copy.size_in, copy.shapes.len()), ("Tag copy", (2.0, 1.0), 3));
    let ids: Vec<u64> = copy.shapes.iter().map(|s| s.id).collect();
    assert!(ids.iter().all(|id| *id > copy.id), "fresh ids: {ids:?}");
    assert_eq!(d.check(&Edit::RenameComponent { component: copy.id, name: "  ".into() }, &[]), Err(Blocked::EmptyName));
    d.apply(&Edit::RenameComponent { component: copy.id, name: " Label ".into() }, &[]).unwrap();
    assert_eq!(d.component(copy.id).unwrap().name, "Label");
    assert_eq!(d.component(MASTER).unwrap().name, "Tag");
}

#[test]
fn a_locked_instance_lets_canvas_clicks_through_and_its_lock_is_its_own() {
    let mut d = design();
    let under = d.add_shape(ShapeKind::Square);
    let s = d.shape_mut(under).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (0.0, 0.0, 4.0, 4.0);
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    assert_eq!(d.pick_at(1.5, 1.5), Some(a));
    d.apply(&Edit::Lock { lock: crate::edit::Lock::Layer, on: true }, &[a]).unwrap();
    assert!(get(&d, a).locked);
    assert!(!get(&d, under).locked);
    assert_eq!(d.pick_at(1.5, 1.5), Some(under), "the click reaches the square beneath");
    d.apply(&Edit::Lock { lock: crate::edit::Lock::Position, on: true }, &[a]).unwrap();
    assert_eq!(d.check(&Edit::Nudge { dx_in: 1.0, dy_in: 0.0, board_in: (12.0, 12.0) }, &[a]), Err(Blocked::PositionLocked));
}

#[test]
fn enter_and_leave_master_swap_the_canvas_and_close_before_saving() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let canvas = d.shapes.clone();
    assert!(d.enter_master(MASTER));
    assert_eq!(d.editing, Some(MASTER));
    assert_eq!(d.shapes.len(), 3);
    assert_eq!(d.geometry(&canvas[0]).pieces().len(), 3, "an instance of the open master reads it live");
    assert_eq!(d.next_id(), a + 1, "ids count the canvas while it is put away");
    assert!(!d.enter_master(9));
    assert_eq!(d.editing, None, "entering a missing master leaves none open");
    assert_eq!(d.shapes, canvas);
    assert!(d.enter_master(MASTER));
    assert_eq!(d.clone().closed().shapes, canvas);
}

#[test]
fn components_and_overridden_instances_round_trip_and_older_projects_load_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(a, 2, PartChange::Fill(Color32::GREEN));
    d.set_override(a, 3, PartChange::Hidden(true));
    let b = place(&mut d, (5.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(b, 1, PartChange::Pattern(Some(pattern(7))));
    d.set_override(b, 2, PartChange::Pattern(None));
    let project = crate::project::Project::new(d);
    crate::project::save(dir.path(), &project).unwrap();
    let text = std::fs::read_to_string(crate::project::json_path(dir.path(), &project.id)).unwrap();
    assert!(
        text.contains(&format!(r#""format_version": {}"#, crate::project::FORMAT_VERSION)) && text.contains(r#""components""#) && text.contains(r#""Instance""#),
        "{text}"
    );
    let loaded = crate::project::load(dir.path(), &project.id).unwrap();
    assert_eq!(loaded.design, project.design, "round-trips");
    assert_eq!(fills(&loaded.design, a), [Color32::BLACK, Color32::GREEN]);

    let old = r#"{"format_version":9,"id":"o","name":"Old","design":{"shapes":[{"id":1,"kind":"Star","x_in":1.0,"y_in":1.0,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contours":{"hidden":[],"enabled":true}}]},"created":1,"modified":1}"#;
    let (old, repairs) = crate::project::parse(old).expect("format 9 loads");
    assert!(repairs.is_empty());
    assert!(old.design.components.is_empty() && old.design.shapes.iter().all(|s| s.instance_of().is_none()));
}

#[test]
fn an_instance_of_a_missing_component_loads_and_draws_a_box() {
    let text = r#"{"format_version":10,"id":"m","name":"Missing","design":{"shapes":[{"id":1,"kind":{"Instance":{"component":42}},"x_in":1.0,"y_in":1.0,"width_in":2.0,"height_in":1.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"BasicCut","hidden":false,"contours":{"hidden":[],"enabled":true}}]},"created":1,"modified":1}"#;
    let (project, repairs) = crate::project::parse(text).expect("loads");
    assert!(repairs.is_empty());
    let d = &project.design;
    assert_eq!(d.shapes[0].instance_of(), Some(42));
    let g = d.geometry(&d.shapes[0]);
    assert_eq!(g.pieces().iter().map(|p| p.style.operation).collect::<Vec<_>>(), [Operation::Guide]);
}

#[test]
fn a_thumbnail_draws_instances_expanded() {
    let mut d = design();
    d.components[0].shapes.retain(|s| s.fill == Color32::RED);
    place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let png = crate::project::render_thumbnail(&d, 64, &crate::assets::Pixels::new()).expect("a thumbnail");
    let thumb = image::load_from_memory(&png).unwrap().into_rgba8();
    let red = thumb.pixels().filter(|p| p.0[0] > 200 && p.0[1] < 60 && p.0[2] < 60).count();
    assert!(red > 200, "the master's red rectangle is drawn: {red} red pixels");
}

/// A one-pixel pattern image whose content (and so its asset id) follows `seed`.
fn pattern(seed: u8) -> crate::patterns::PatternFill {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([seed, 0, 0, 255]));
    crate::patterns::PatternFill::new(crate::assets::id_of(&img), 1.0, crate::patterns::PatternMode::Tile)
}

fn part_pattern(d: &Design, instance: u64, n: usize) -> Option<crate::patterns::PatternFill> {
    expand(d, &get(d, instance)).unwrap()[n].pattern.clone()
}

#[test]
fn a_pattern_override_reaches_only_that_instance_and_reset_restores_the_masters_pattern() {
    let mut d = design();
    let square = &mut d.components[0].shapes[0];
    square.operation = Operation::PrintThenCut;
    square.pattern = Some(pattern(1));
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    let b = place(&mut d, (5.0, 1.0), (2.0, 1.0), 0.0);
    d.apply(&Edit::Override { part: 1, change: PartChange::Pattern(Some(pattern(2))) }, &[a]).unwrap();
    assert_eq!(part_pattern(&d, a, 0), Some(pattern(2)), "the override reaches the part");
    assert_eq!(part_pattern(&d, b, 0), Some(pattern(1)), "the other instance keeps the master's pattern");
    assert_ne!(d.geometry(&get(&d, a)).pieces()[0].paint, d.geometry(&get(&d, b)).pieces()[0].paint, "and draws it");
    d.apply(&Edit::Override { part: 1, change: PartChange::Pattern(None) }, &[a]).unwrap();
    assert_eq!(part_pattern(&d, a, 0), None, "an override can take the master's pattern away");
    d.apply(&Edit::ResetOverrides, &[a]).unwrap();
    assert_eq!(part_pattern(&d, a, 0), Some(pattern(1)), "Reset overrides restores the master's pattern");
}

#[test]
fn an_instance_takes_no_pattern_of_its_own() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.shape_mut(a).unwrap().operation = Operation::PrintThenCut;
    assert!(!get(&d, a).takes_pattern());
    assert_eq!(d.check(&Edit::Pattern(Some(pattern(1))), &[a]), Err(Blocked::PatternNeedsPrint));
}

#[test]
fn assets_in_masters_and_pattern_overrides_count_as_used() {
    let mut d = design();
    d.components[0].shapes[0].pattern = Some(pattern(1));
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(a, 2, PartChange::Pattern(Some(pattern(2))));
    let used = crate::assets::ids(&d);
    assert!(used.contains(&pattern(1).asset) && used.contains(&pattern(2).asset), "{used:?}");
}

#[test]
fn a_pattern_override_out_of_range_is_clamped_on_load() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    d.set_override(a, 1, PartChange::Pattern(Some(crate::patterns::PatternFill { scale: f32::NAN, ..pattern(1) })));
    let repairs = d.sanitise();
    assert!(repairs.clamped.contains(&a));
    assert_eq!(part_pattern(&d, a, 0).unwrap().scale, crate::patterns::MIN_SCALE);
}

#[test]
fn the_canvas_specimens_and_instance_counts_see_through_an_open_master() {
    let mut d = design();
    let a = place(&mut d, (1.0, 1.0), (2.0, 1.0), 0.0);
    place(&mut d, (5.0, 1.0), (2.0, 1.0), 0.0);
    let saved = d.clone();
    assert_eq!(instance_count(&d, MASTER), 2);
    assert!(d.enter_master(MASTER));
    assert_eq!(canvas_shapes(&d).len(), 2, "the canvas, put away while the master is open");
    assert_eq!(instance_count(&d, MASTER), 2);
    assert!(d.eq_closed(&saved), "an open master is not an unsaved change");
    d.shapes[0].fill = Color32::GREEN;
    assert!(!d.eq_closed(&saved), "an edit to it is");
    let specimen = specimen(&d, MASTER).unwrap();
    assert!(specimen.id != a && specimen.id >= 1 << 63, "a specimen never takes a real id");
    assert_eq!((specimen.width_in, specimen.height_in), (2.0, 1.0));
    assert_eq!(d.geometry(&specimen).pieces()[0].style.fill, Color32::GREEN, "it draws the live master");
}

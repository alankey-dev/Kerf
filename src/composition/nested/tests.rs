use std::sync::atomic::AtomicBool;
use std::time::Duration;

use egui::Color32;

use super::*;
use crate::bridge::mat_placements;
use crate::composition::compose;
use crate::design::{Operation, POINTS_PER_INCH, Pt, ShapeKind};
use crate::machine::{LoadType, MachineFamily};
use crate::make_flow::MatSetup;

const BUDGET: Duration = Duration::from_secs(600);

fn twelve() -> MatSize {
    MachineFamily::StudioPro.preferred_mat()
}

fn long() -> MatSize {
    MachineFamily::StudioPro.mat_sizes().iter().copied().find(|m| (m.width_in, m.height_in) == (12.0, 24.0) && !m.matless).unwrap()
}

fn add(design: &mut Design, kind: ShapeKind, (x, y, w, h): (f32, f32, f32, f32)) -> u64 {
    let id = design.add_shape(kind);
    let s = design.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (x, y, w, h, Color32::BLACK);
    s.set_operation(Operation::BasicCut);
    id
}

fn optimised(design: &mut Design, size: MatSize, settings: NestSettings) {
    design.nesting = Some(optimise(design, size, 1, settings, BUDGET, &AtomicBool::new(false)));
}

/// Where `id`'s local centre lands on the mat, in inches, through the payload transform.
fn centre_on_mat(design: &Design, mat: &Mat, id: u64) -> Pt {
    let setup = MatSetup { load_type: LoadType::OnMat, mat_size: long(), mirror: false, copies: 1 };
    let placements = mat_placements(design, mat, &setup);
    let p = placements.iter().find(|p| p.shape.id == id).unwrap();
    let (x, y) = p.transform.apply((p.shape.width_in * POINTS_PER_INCH / 2.0, p.shape.height_in * POINTS_PER_INCH / 2.0));
    (x / POINTS_PER_INCH, y / POINTS_PER_INCH)
}

fn mat_of(mats: &[Mat], id: u64) -> &Mat {
    mats.iter().find(|m| m.items.iter().any(|i| i.piece.owner == id)).unwrap()
}

#[test]
fn six_squares_and_an_upright_strip_take_less_material_than_packing() {
    let mut design = Design::default();
    for i in 0..6 {
        add(&mut design, ShapeKind::Square, (i as f32 * 1.5, 0.0, 1.0, 1.0));
    }
    add(&mut design, ShapeKind::Rectangle, (10.0, 0.0, 1.0, 4.0));
    let packed = material_length(&design, &compose(&design, twelve(), 1), twelve());
    optimised(&mut design, twelve(), NestSettings::default());
    let nested = material_length(&design, &compose(&design, twelve(), 1), twelve());
    assert!(nested <= 2.5, "nested into {nested}");
    assert!(packed > nested, "packing uses {packed}, nesting {nested}");
}

#[test]
fn an_attach_link_and_a_container_turn_and_move_as_rigid_units() {
    let mut design = Design::default();
    let strip = add(&mut design, ShapeKind::Rectangle, (0.0, 0.0, 11.8, 1.0));
    let dot = add(&mut design, ShapeKind::Circle, (5.0, 1.5, 0.75, 0.75));
    design.attach(&[strip, dot]);
    let holder = add(&mut design, ShapeKind::Circle, (0.0, 4.0, 4.0, 4.0));
    let kid = add(&mut design, ShapeKind::Star, (1.0, 5.0, 1.0, 1.0));
    let kid2 = add(&mut design, ShapeKind::Square, (2.5, 6.0, 0.5, 0.5));
    design.contain(&[holder]);
    design.reparent(&[kid, kid2], Some(holder));
    add(&mut design, ShapeKind::Heart, (6.0, 6.0, 2.0, 2.0));

    optimised(&mut design, long(), NestSettings::default());
    assert_eq!(status(&design, long(), 1), LayoutStatus::Current);
    let mats = compose(&design, long(), 1);
    assert!(mats.iter().all(|m| !m.overflow));
    let turn = mat_of(&mats, strip).items.iter().find(|i| i.piece.owner == strip).unwrap().rotation_deg;
    assert!([90.0, 270.0].contains(&turn), "an 11.8 in strip turns to fit: {turn}");

    let canvas = |id: u64| design.shapes.iter().find(|s| s.id == id).unwrap().centre_in();
    for (anchor, member) in [(strip, dot), (holder, kid), (holder, kid2)] {
        let mat = mat_of(&mats, anchor);
        assert!(mat.items.iter().any(|i| i.piece.owner == member), "{member} shares {anchor}'s mat");
        let deg = mat.items.iter().find(|i| i.piece.owner == anchor).unwrap().rotation_deg;
        let (a, m) = (centre_on_mat(&design, mat, anchor), centre_on_mat(&design, mat, member));
        let (ca, cm) = (canvas(anchor), canvas(member));
        let (sin, cos) = deg.to_radians().sin_cos();
        let (dx, dy) = (cm.0 - ca.0, cm.1 - ca.1);
        let want = (dx * cos - dy * sin, dx * sin + dy * cos);
        let got = (m.0 - a.0, m.1 - a.1);
        assert!((got.0 - want.0).abs() < 1e-3 && (got.1 - want.1).abs() < 1e-3, "{member} keeps its offset from {anchor}: {got:?} vs {want:?}");
    }
}

#[test]
fn changing_a_shape_changes_the_fingerprint_and_compose_falls_back_to_packing() {
    let mut design = Design::default();
    let a = add(&mut design, ShapeKind::Heart, (1.0, 1.0, 3.0, 3.0));
    add(&mut design, ShapeKind::Star, (5.0, 1.0, 2.0, 2.0));
    let packed = compose(&design, twelve(), 1);
    assert_eq!(status(&design, twelve(), 1), LayoutStatus::Absent);
    optimised(&mut design, twelve(), NestSettings::default());
    let before = fingerprint(&design, twelve(), 1);
    assert_eq!(design.nesting.as_ref().unwrap().fingerprint, before);
    assert_ne!(compose(&design, twelve(), 1), packed, "the layout is used");

    design.shape_mut(a).unwrap().width_in = 3.5;
    assert_ne!(fingerprint(&design, twelve(), 1), before);
    assert_eq!(status(&design, twelve(), 1), LayoutStatus::Stale);
    let mut plain = design.clone();
    plain.nesting = None;
    assert_eq!(compose(&design, twelve(), 1), compose(&plain, twelve(), 1), "a stale layout is ignored");

    design.shape_mut(a).unwrap().width_in = 3.0;
    assert_eq!(status(&design, twelve(), 1), LayoutStatus::Current, "undoing the change makes it current again");
    assert_eq!(status(&design, twelve(), 2), LayoutStatus::Stale, "copies are part of the fingerprint");
    let other = MachineFamily::StudioPro.mat_sizes().iter().copied().find(|m| *m != twelve()).unwrap();
    assert_eq!(status(&design, other, 1), LayoutStatus::Stale, "so is the mat size");
    design.shape_mut(a).unwrap().grain_lock = true;
    assert_eq!(status(&design, twelve(), 1), LayoutStatus::Stale, "and the grain lock");
}

#[test]
fn print_then_cut_pages_keep_their_own_packing() {
    let mut design = Design::default();
    add(&mut design, ShapeKind::Heart, (1.0, 1.0, 3.0, 3.0));
    let printed = add(&mut design, ShapeKind::Circle, (5.0, 1.0, 2.0, 2.0));
    design.shape_mut(printed).unwrap().set_operation(Operation::PrintThenCut);
    let before = compose(&design, twelve(), 1);
    optimised(&mut design, twelve(), NestSettings::default());
    let after = compose(&design, twelve(), 1);
    assert_eq!(before[0], after[0], "the print page is untouched");
    assert!(after[0].print_then_cut && !after[1].print_then_cut);
    assert_eq!(design.nesting.as_ref().unwrap().mats.len(), 1, "only the cut mat is nested");
}

#[test]
fn a_loaded_layout_with_unusable_numbers_is_dropped() {
    let mut design = Design::default();
    add(&mut design, ShapeKind::Heart, (1.0, 1.0, 3.0, 3.0));
    optimised(&mut design, twelve(), NestSettings::default());
    let sound = design.clone();
    design.sanitise();
    assert_eq!(design, sound, "a sound layout is kept");
    design.nesting.as_mut().unwrap().mats[0].placements[0].x_in = f32::NAN;
    design.sanitise();
    assert_eq!(design.nesting, None);
}

#[test]
fn an_image_is_never_turned_by_nesting() {
    let mut design = Design::default();
    let strip = add(&mut design, ShapeKind::Rectangle, (0.0, 0.0, 13.0, 1.0));
    let asset = crate::assets::id_of(&image::RgbaImage::new(2, 2));
    let image = design.add_shape(ShapeKind::Bitmap { asset, px: (200, 900), clip: Vec::new() });
    let s = design.shape_mut(image).unwrap();
    (s.x_in, s.y_in, s.width_in, s.height_in) = (0.0, 2.0, 2.0, 9.0);
    optimised(&mut design, long(), NestSettings { rotation: crate::nesting::Rotation::Free(crate::nesting::FreeStep::Fifteen), ..NestSettings::default() });
    let mats = compose(&design, long(), 1);
    let turns = |id: u64| -> Vec<f32> { mats.iter().flat_map(|m| &m.items).filter(|i| i.piece.owner == id).map(|i| i.rotation_deg).collect() };
    assert!(turns(strip).iter().all(|d| d.rem_euclid(180.0) != 0.0), "a cut strip too wide for the mat turns: {:?}", turns(strip));
    assert_eq!(turns(image), [0.0], "a Print Then Cut image keeps its print upright");
    assert!(mat_of(&mats, image).print_then_cut, "nesting leaves Print Then Cut pages alone");
}

//! `Design::geometry` against the pipeline it replaced, and the cache's invalidation rules.

use std::collections::HashSet;

use egui::Color32;

use super::{Local, PieceId};
use crate::design::{Contour, Design, Pt, Seg, Shape, ShapeKind};
use crate::geometry::FLATTEN_TOL_IN;
use crate::warp::{Warp, WarpStyle};

#[test]
fn a_pattern_paints_a_print_then_cut_shape_and_nothing_else() {
    use crate::patterns::{PatternMode, fixture};
    let design = fixture::circle(PatternMode::Tile);
    let circle = &design.shapes[0];
    let geometry = design.geometry(circle);
    let [piece] = geometry.pieces() else { panic!("one piece") };
    let super::Paint::Print(layers) = &piece.paint else { panic!("{:?}", piece.paint) };
    let [(lines, super::Ink::Image { asset, corners, repeat: true })] = &layers[..] else { panic!("one repeating image: {layers:?}") };
    assert_eq!(*asset, fixture::halves().1);
    assert_eq!(lines, &piece.lines, "a tile paints the whole outline");
    let across = (corners[1].0 - corners[0].0, corners[1].1 - corners[0].1);
    assert!((across.0 - 0.5).abs() < 1e-5 && across.1.abs() < 1e-5, "the tile is 0.5 in wide on the canvas: {corners:?}");

    let mut cut = design.clone();
    cut.shapes[0].set_operation(crate::design::Operation::BasicCut);
    assert_eq!(cut.geometry(&cut.shapes[0]).pieces()[0].paint, super::Paint::Fill, "a cut shape keeps its colour");
}

#[test]
fn an_image_drawn_once_paints_only_where_it_overlaps_the_outline() {
    use crate::patterns::{PatternMode, fixture};
    let mut design = fixture::circle(PatternMode::Fit);
    let fill = design.shapes[0].pattern.as_mut().unwrap();
    (fill.scale, fill.offset_in) = (0.5, (0.5, 0.0));
    let geometry = design.geometry(&design.shapes[0]);
    let super::Paint::Print(layers) = &geometry.pieces()[0].paint else { panic!() };
    let [(lines, super::Ink::Image { corners, repeat: false, .. })] = &layers[..] else { panic!("{layers:?}") };
    let pts: Vec<Pt> = lines.iter().flat_map(|(pts, _)| pts.iter().copied()).collect();
    assert!(!pts.is_empty());
    for p in pts {
        let (u, v) = crate::patterns::uv(*corners, p).unwrap();
        assert!((-1e-3..=1.001).contains(&u) && (-1e-3..=1.001).contains(&v), "{p:?} maps to ({u}, {v}), outside the image");
    }
}

/// The geometry methods `Shape` had before spec 21, kept verbatim as the reference evaluation must match.
mod pre_spec {
    use super::*;

    pub fn contours(s: &Shape) -> Vec<Contour> {
        let base = s.kind.outline(s.width_in, s.height_in);
        let all = match s.effects.warp() {
            Some(warp) => crate::warp::apply(warp, &base, (s.width_in, s.height_in)),
            None => base,
        };
        all.into_iter().enumerate().filter(|(i, _)| !s.contours.hidden.get(*i).copied().unwrap_or(false)).map(|(_, c)| c).collect()
    }

    fn world_placer(s: &Shape) -> impl Fn(Pt) -> Pt + use<> {
        let (cx, cy) = s.centre_in();
        let (sin, cos) = s.rotation_deg.to_radians().sin_cos();
        let (hw, hh) = (s.width_in / 2.0, s.height_in / 2.0);
        let (fx, fy) = (s.flip_x, s.flip_y);
        move |(x, y): Pt| {
            let (mut lx, mut ly) = (x - hw, y - hh);
            if fx {
                lx = -lx;
            }
            if fy {
                ly = -ly;
            }
            (cx + lx * cos - ly * sin, cy + lx * sin + ly * cos)
        }
    }

    pub fn world_rings(s: &Shape, tol: f32) -> Vec<Vec<Pt>> {
        let place = world_placer(s);
        contours(s).iter().filter(|c| c.closed).map(|c| crate::geometry::flatten(c, tol).into_iter().map(&place).collect()).collect()
    }

    pub fn world_polylines(s: &Shape) -> Vec<(Vec<Pt>, bool)> {
        let place = world_placer(s);
        contours(s).iter().map(|c| (c.flatten().into_iter().map(&place).collect(), c.closed)).collect()
    }

    pub fn world_contours(s: &Shape) -> Vec<Contour> {
        let place = world_placer(s);
        let seg = |sg: &Seg| match *sg {
            Seg::Line(p) => Seg::Line(place(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(place(a), place(b), place(p)),
        };
        contours(s).into_iter().map(|c| Contour { start: place(c.start), segs: c.segs.iter().map(seg).collect(), closed: c.closed }).collect()
    }

    pub fn world_bounds(s: &Shape) -> (f32, f32, f32, f32) {
        let pts = world_polylines(s).into_iter().flat_map(|(pts, _)| pts);
        pts.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), (x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
    }
}

const ARCH: Warp = Warp { style: WarpStyle::Arch, bend: 0.5, vertical: false };

fn text(design: &mut Design, content: &str, font: crate::text::FontRef) -> u64 {
    design.add_text(content.into(), font, 0.5, Color32::BLACK, 1.0, 1.0)
}

/// Every free shape, text straight and curved, a holed import with a hidden contour, a drawn open path,
/// a warp and a sticker, each also turned and mirrored.
fn corpus() -> Design {
    let mut d = Design::default();
    for kind in ShapeKind::free() {
        d.add_shape(kind);
    }
    let curved = text(&mut d, "Curved", crate::text::FontRef::Inter);
    if let ShapeKind::Text { curve, .. } = &mut d.shape_mut(curved).unwrap().kind {
        *curve = crate::effects::Curve::from_diameter(3.0);
    }
    text(&mut d, "Two\nlines", crate::text::FontRef::SpaceGrotesk);
    let ring = |a: f32, b: f32| Contour { start: (a, a), segs: [(b, a), (b, b), (a, b)].map(Seg::Line).to_vec(), closed: true };
    let holed = d.add_imported(vec![crate::shape_library::Import {
        x_in: 2.0,
        y_in: 2.0,
        view_box: (4.0, 4.0),
        contours: vec![ring(0.0, 4.0), ring(1.0, 3.0)],
        fill: Color32::RED,
        ..Default::default()
    }])[0];
    d.shape_mut(holed).unwrap().contours.hidden = vec![false, true];
    let open = Contour { start: (0.0, 0.0), segs: vec![Seg::Cubic((1.0, -1.0), (2.0, 1.0), (3.0, 0.0))], closed: false };
    d.add_path(open, Color32::BLUE, crate::design::Operation::Pen);
    let star = d.add_shape(ShapeKind::Star);
    d.shape_mut(star).unwrap().set_warp(Some(ARCH));
    let word = text(&mut d, "Hi", crate::text::FontRef::Inter);
    d.shape_mut(word).unwrap().set_warp(Some(Warp { style: WarpStyle::Wave, bend: 0.4, vertical: true }));
    let heart = d.add_shape(ShapeKind::Heart);
    let sticker = d.expanded_sticker(&[word, heart], 0.1, super::Corners::Round).expect("a sticker");
    assert!(d.shapes.iter().any(|s| s.id == sticker));
    let turned: Vec<Shape> =
        (d.next_id()..).zip(d.shapes.clone()).map(|(id, s)| Shape { id, rotation_deg: 37.0, flip_x: true, flip_y: id % 2 == 0, x_in: s.x_in + 0.3, ..s }).collect();
    d.shapes.extend(turned);
    d
}

#[test]
fn geometry_equals_the_pre_spec_pipeline_bit_for_bit() {
    let d = corpus();
    for s in &d.shapes {
        let g = d.geometry(s);
        let what = format!("{} {}", s.kind.label(), s.id);
        assert_eq!(g.contours().cloned().collect::<Vec<_>>(), pre_spec::contours(s), "{what}: local contours");
        assert_eq!(g.lines().cloned().collect::<Vec<_>>(), pre_spec::world_polylines(s), "{what}: lines");
        assert_eq!(g.rings(FLATTEN_TOL_IN), pre_spec::world_rings(s, FLATTEN_TOL_IN), "{what}: rings");
        assert_eq!(g.pieces().iter().flat_map(|p| g.world_contours(p)).collect::<Vec<_>>(), pre_spec::world_contours(s), "{what}: world contours");
        let b = g.bounds();
        assert_eq!((b.min_x, b.min_y, b.max_x, b.max_y), pre_spec::world_bounds(s), "{what}: bounds");
        for piece in g.pieces() {
            assert_eq!(piece.style, super::Style::of(s), "{what}: a body piece takes the shape's style");
        }
    }
}

#[test]
fn every_piece_id_in_a_geometry_is_distinct() {
    let d = corpus();
    for s in &d.shapes {
        let g = d.geometry(s);
        let ids: HashSet<PieceId> = g.pieces().iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), g.pieces().len(), "{}", s.kind.label());
        assert!(g.pieces().iter().all(|p| p.id == PieceId { owner: s.id, local: Local::Body }));
    }
}

#[test]
fn moving_one_shape_reevaluates_nothing_and_editing_it_reevaluates_only_it() {
    let mut d = corpus();
    let ids: Vec<u64> = d.shapes.iter().map(|s| s.id).collect();
    let all = |d: &Design| d.shapes.iter().for_each(|s| _ = d.geometry(s));
    let built = d.evaluations();
    all(&d);
    let cold = d.evaluations();
    assert_eq!(cold - built, ids.len(), "one evaluation per shape");
    let before = d.clone();
    d.shapes[0].x_in += 1.0;
    d.shapes[1].rotation_deg += 10.0;
    all(&d);
    assert_eq!(d.evaluations(), cold, "placement changes re-place only");
    d.shapes[2].width_in += 1.0;
    all(&d);
    assert_eq!(d.evaluations(), cold + 1, "a resize re-evaluates that shape");
    all(&before);
    assert_eq!(before.evaluations(), cold + 1, "a clone shares the cache and its warm entries");
}

#[test]
fn a_direct_field_write_never_reads_stale_geometry() {
    let mut d = corpus();
    let s = d.shapes[0].clone();
    let first = d.geometry(&s);
    d.shapes[0].kind = ShapeKind::Circle;
    let after = d.geometry(&d.shapes[0]);
    assert_ne!(first.contours().collect::<Vec<_>>(), after.contours().collect::<Vec<_>>());
    assert_eq!(after.contours().cloned().collect::<Vec<_>>(), pre_spec::contours(&d.shapes[0]));
}

#[test]
fn system_font_text_changes_once_the_catalogue_is_ready() {
    let fonts = crate::system_fonts::FontCatalogue::scanning();
    fonts.make_current();
    let mut d = Design::default();
    let id = text(&mut d, "Hamburgefonts", crate::text::FontRef::System(crate::system_fonts::FIXTURE_FAMILIES[0].into()));
    let s = d.shapes.iter().find(|s| s.id == id).unwrap().clone();
    let fallback = d.geometry(&s);
    assert_eq!(d.geometry(&s).contours().count(), fallback.contours().count(), "a repeat lookup hits");
    fonts.land_fixture();
    let ready = d.geometry(&s);
    assert_ne!(fallback.contours().collect::<Vec<_>>(), ready.contours().collect::<Vec<_>>(), "laid out in the system family, not Inter");
    assert_eq!(ready.contours().cloned().collect::<Vec<_>>(), pre_spec::contours(&s));
}

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;
use crate::design::{Shape, ShapeKind};

const SPACING: f32 = 0.125;
const PAD: f32 = MAT_PADDING_PT / POINTS_PER_INCH;
const GREEDY: Duration = Duration::ZERO;
const PLENTY: Duration = Duration::from_secs(600);

fn rect(x: f32, y: f32, w: f32, h: f32) -> Vec<Pt> {
    vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

fn piece(rings: Vec<Vec<Pt>>) -> Piece {
    Piece { lines: rings.into_iter().map(|r| (r, true)).collect(), grain_locked: false }
}

fn circle(d: f32) -> Piece {
    piece(vec![(0..64).map(|i| (i as f32 / 64.0 * std::f32::consts::TAU).sin_cos()).map(|(s, c)| (d / 2.0 * (1.0 + c), d / 2.0 * (1.0 + s))).collect()])
}

/// A library shape's evaluated world lines, `w` by `h` turned by `deg`.
fn shape(kind: ShapeKind, w: f32, h: f32, deg: f32) -> Piece {
    let mut s = Shape::new(1, kind);
    (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg) = (3.0, 4.0, w, h, deg);
    let geometry = crate::effects::evaluate(&s);
    Piece { lines: geometry.lines().cloned().collect(), grain_locked: false }
}

fn area(w: f32, h: f32) -> MatArea {
    MatArea { width_in: w, height_in: h }
}

fn settings(rotation: Rotation) -> NestSettings {
    NestSettings { rotation, ..NestSettings::default() }
}

fn run(pieces: &[Piece], mat: MatArea, settings: &NestSettings, budget: Duration) -> NestResult {
    nest(pieces, mat, settings, budget, &AtomicBool::new(false))
}

/// `p`'s lines where `placed` puts them on the mat.
fn on_mat(p: &Piece, placed: &Placed) -> Lines {
    let centre = raster::piece_centre(p);
    let e = turned_bounds(&p.lines, placed.rotation_deg, centre);
    let turn = turner(placed.rotation_deg, centre);
    let to = |pt: Pt| {
        let (x, y) = turn(pt);
        (x - e.min_x + placed.x_in, y - e.min_y + placed.y_in)
    };
    p.lines.iter().map(|(pts, closed)| (pts.iter().map(|&q| to(q)).collect(), *closed)).collect()
}

fn bounds(lines: &Lines) -> Aabb {
    turned_bounds(lines, 0.0, (0.0, 0.0))
}

fn segments(lines: &Lines) -> Vec<(Pt, Pt)> {
    let mut out = Vec::new();
    for (pts, closed) in lines {
        out.extend(pts.windows(2).map(|w| (w[0], w[1])));
        if *closed && pts.len() > 2 {
            out.push((pts[pts.len() - 1], pts[0]));
        }
    }
    out
}

fn point_segment((px, py): Pt, (a, b): (Pt, Pt)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) };
    ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt()
}

fn cross(o: Pt, a: Pt, b: Pt) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

fn segment_distance(s: (Pt, Pt), t: (Pt, Pt)) -> f32 {
    let crosses = cross(s.0, s.1, t.0).signum() * cross(s.0, s.1, t.1).signum() < 0.0 && cross(t.0, t.1, s.0).signum() * cross(t.0, t.1, s.1).signum() < 0.0;
    if crosses {
        return 0.0;
    }
    [point_segment(s.0, t), point_segment(s.1, t), point_segment(t.0, s), point_segment(t.1, s)].into_iter().fold(f32::MAX, f32::min)
}

/// Inside `lines`' closed rings by even-odd, so a hole counts as outside.
fn inside(lines: &Lines, (px, py): Pt) -> bool {
    let mut odd = false;
    for (pts, _) in lines.iter().filter(|(p, closed)| *closed && p.len() > 2) {
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
            if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                odd = !odd;
            }
            j = i;
        }
    }
    odd
}

/// The exact gap between two placed outlines, or 0 when one reaches inside the other.
fn gap(a: &Lines, b: &Lines) -> f32 {
    if a.iter().flat_map(|(p, _)| p).any(|&p| inside(b, p)) || b.iter().flat_map(|(p, _)| p).any(|&p| inside(a, p)) {
        return 0.0;
    }
    let (sa, sb) = (segments(a), segments(b));
    sa.iter().flat_map(|&s| sb.iter().map(move |&t| segment_distance(s, t))).fold(f32::MAX, f32::min)
}

fn near(a: Aabb, b: Aabb, by: f32) -> bool {
    a.min_x - by <= b.max_x && b.min_x - by <= a.max_x && a.min_y - by <= b.max_y && b.min_y - by <= a.max_y
}

/// Every placement inside the padded area, and every pair of pieces on a mat at least `spacing` apart.
/// Returns the smallest gap found.
fn assert_spaced(pieces: &[Piece], result: &NestResult, mat: MatArea, spacing: f32) -> f32 {
    let mut smallest = f32::MAX;
    for sheet in result.sheets.iter().filter(|s| !s.overflow) {
        let placed: Vec<Lines> = sheet.placed.iter().map(|p| on_mat(&pieces[p.piece], p)).collect();
        for (i, a) in placed.iter().enumerate() {
            let b = bounds(a);
            assert!(b.min_x >= PAD - 1e-3 && b.min_y >= PAD - 1e-3, "{:?} crosses the padding at {b:?}", sheet.placed[i]);
            assert!(b.max_x <= mat.width_in - PAD + 1e-3 && b.max_y <= mat.height_in - PAD + 1e-3, "{:?} crosses the padding at {b:?}", sheet.placed[i]);
            for (j, c) in placed.iter().enumerate().skip(i + 1) {
                if !near(b, bounds(c), spacing) {
                    continue;
                }
                let g = gap(a, c);
                assert!(g >= spacing - 1e-3, "pieces {} and {} are {g} apart", sheet.placed[i].piece, sheet.placed[j].piece);
                smallest = smallest.min(g);
            }
        }
    }
    smallest
}

/// Thirty shapes of every sort: curves, points, rings, an L, an open line, turned and not.
fn thirty() -> Vec<Piece> {
    let kinds = [
        ShapeKind::Heart,
        ShapeKind::Star,
        ShapeKind::Circle,
        ShapeKind::Triangle,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::Plus,
        ShapeKind::Pill,
        ShapeKind::Arch,
        ShapeKind::HalfCircle,
        ShapeKind::StarThin,
        ShapeKind::Diamond,
        ShapeKind::Oval,
        ShapeKind::Octagon,
        ShapeKind::RoundedSquare,
        ShapeKind::Equals,
    ];
    let mut out: Vec<Piece> =
        kinds.iter().enumerate().map(|(i, k)| shape(k.clone(), 1.0 + (i % 4) as f32 * 0.6, 1.0 + (i % 3) as f32 * 0.7, (i * 23 % 90) as f32)).collect();
    out.push(piece(vec![vec![(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)]]));
    out.push(piece(vec![rect(0.0, 0.0, 3.0, 3.0), rect(0.5, 0.5, 2.0, 2.0)]));
    out.push(Piece { lines: vec![(vec![(0.0, 0.0), (2.0, 1.0), (4.0, 0.0)], false)], grain_locked: false });
    for i in 0..11 {
        out.push(match i % 3 {
            0 => piece(vec![rect(0.0, 0.0, 0.5 + i as f32 * 0.2, 0.75)]),
            1 => circle(0.6 + i as f32 * 0.1),
            _ => shape(ShapeKind::Heart, 0.8, 0.8, 45.0),
        });
    }
    assert_eq!(out.len(), 30);
    out
}

#[test]
fn six_squares_and_an_upright_strip_nest_within_two_and_a_half_inches() {
    let mut pieces: Vec<Piece> = (0..6).map(|_| piece(vec![rect(0.0, 0.0, 1.0, 1.0)])).collect();
    pieces.push(piece(vec![rect(0.0, 0.0, 1.0, 4.0)]));
    let result = run(&pieces, area(12.0, 12.0), &NestSettings::default(), GREEDY);
    assert_eq!(result.sheets.len(), 1);
    assert!(result.sheets[0].used_length_in <= 2.5, "used {}", result.sheets[0].used_length_in);
    assert_spaced(&pieces, &result, area(12.0, 12.0), SPACING);
}

#[test]
fn no_two_of_thirty_mixed_shapes_come_closer_than_the_spacing() {
    let pieces = thirty();
    let mat = area(12.0, 24.0);
    for settings in [NestSettings::default(), NestSettings { rotation: Rotation::Free(FreeStep::Fifteen), spacing_in: 0.25, use_holes: true }] {
        let result = run(&pieces, mat, &settings, GREEDY);
        assert_eq!(result.sheets.iter().map(|s| s.placed.len()).sum::<usize>(), 30);
        assert!(result.sheets.iter().all(|s| !s.overflow));
        let smallest = assert_spaced(&pieces, &result, mat, settings.spacing_in);
        println!("{settings:?}: {} mats, last {} long, closest pair {smallest}", result.sheets.len(), result.sheets.last().unwrap().used_length_in);
        assert!(smallest < settings.spacing_in + 0.1, "pieces pack up to the spacing, not loosely: {smallest}");
    }
}

#[test]
#[ignore = "timing; run on the release build"]
fn the_greedy_pass_over_thirty_shapes_is_quick() {
    let pieces = thirty();
    let started = Instant::now();
    run(&pieces, area(12.0, 24.0), &NestSettings::default(), GREEDY);
    let took = started.elapsed();
    println!("greedy pass over 30 shapes: {took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}; the target is under 300 ms in a debug build");
}

#[test]
fn a_strip_wider_than_the_mat_turns_to_fit_when_rotation_allows() {
    let strip = [piece(vec![rect(0.0, 0.0, 12.0, 1.0)])];
    let mat = area(12.0, 24.0);
    let still = run(&strip, mat, &settings(Rotation::None), GREEDY);
    assert!(still.sheets[0].overflow, "12 in does not fit the 11.5 in between the padding");
    let turned = run(&strip, mat, &settings(Rotation::Quarter), GREEDY);
    assert!(!turned.sheets[0].overflow);
    assert!([90.0, 270.0].contains(&turned.sheets[0].placed[0].rotation_deg), "{turned:?}");
    assert_spaced(&strip, &turned, mat, SPACING);
}

#[test]
fn a_grain_locked_strip_only_turns_half_way() {
    let mut strip = piece(vec![rect(0.0, 0.0, 12.0, 1.0)]);
    strip.grain_locked = true;
    assert_eq!(Rotation::Free(FreeStep::Fifteen).angles(true), [0.0, 180.0]);
    assert_eq!(Rotation::None.angles(true), [0.0]);
    let pieces = [strip.clone(), Piece { grain_locked: true, ..shape(ShapeKind::Heart, 2.0, 3.0, 0.0) }, shape(ShapeKind::Heart, 2.0, 3.0, 0.0)];
    let wide = area(13.0, 24.0);
    for rotation in [Rotation::Quarter, Rotation::Free(FreeStep::Five)] {
        let result = run(&pieces, wide, &settings(rotation), GREEDY);
        for p in result.sheets.iter().flat_map(|s| &s.placed).filter(|p| p.piece < 2) {
            assert!([0.0, 180.0].contains(&p.rotation_deg), "{p:?}");
        }
    }
    let narrow = run(&[strip], area(12.0, 24.0), &NestSettings::default(), GREEDY);
    assert!(narrow.sheets[0].overflow, "a locked strip cannot turn to fit");
}

#[test]
fn use_holes_puts_a_circle_inside_a_frame_only_when_on() {
    let pieces = [piece(vec![rect(0.0, 0.0, 6.0, 6.0), rect(1.0, 1.0, 4.0, 4.0)]), circle(1.0)];
    let mat = area(7.0, 12.0);
    let inside_hole = |result: &NestResult| {
        let sheet = &result.sheets[0];
        let frame = bounds(&on_mat(&pieces[0], sheet.placed.iter().find(|p| p.piece == 0).unwrap()));
        let c = bounds(&on_mat(&pieces[1], sheet.placed.iter().find(|p| p.piece == 1).unwrap()));
        c.min_x >= frame.min_x + 1.0 && c.max_x <= frame.min_x + 5.0 && c.min_y >= frame.min_y + 1.0 && c.max_y <= frame.min_y + 5.0
    };
    let on = run(&pieces, mat, &NestSettings::default(), GREEDY);
    assert!(inside_hole(&on), "{on:?}");
    assert_spaced(&pieces, &on, mat, SPACING);
    let off = run(&pieces, mat, &NestSettings { use_holes: false, ..NestSettings::default() }, GREEDY);
    assert!(!inside_hole(&off), "{off:?}");
    assert_eq!(off.sheets.len(), 1);
    assert_spaced(&pieces, &off, mat, SPACING);
}

#[test]
fn the_same_input_nests_the_same_way_and_cancel_keeps_the_best_so_far() {
    let pieces: Vec<Piece> = thirty().into_iter().take(8).collect();
    let mat = area(12.0, 12.0);
    let first = run(&pieces, mat, &NestSettings::default(), PLENTY);
    assert_eq!(first, run(&pieces, mat, &NestSettings::default(), PLENTY), "the round count, not the clock, ends the loop");
    let greedy = run(&pieces, mat, &NestSettings::default(), GREEDY);
    assert!(first.score() <= greedy.score(), "improvement never makes it worse");
    let cancelled = nest(&pieces, mat, &NestSettings::default(), PLENTY, &AtomicBool::new(true));
    assert_eq!(cancelled, greedy, "cancel before any round returns the greedy packing");
}

#[test]
fn cancelling_mid_improvement_keeps_the_best_layout_found_so_far() {
    let pieces: Vec<Piece> = thirty().into_iter().take(12).collect();
    let mat = area(12.0, 12.0);
    let settings = NestSettings::default();
    let greedy = nest_until(&pieces, mat, &settings, |_| true);
    let full = nest_until(&pieces, mat, &settings, |_| false);
    let after = |rounds: usize| nest_until(&pieces, mat, &settings, |round| round >= rounds);

    let key = |r: &NestResult| {
        let (mats, last, total) = r.score();
        (mats, last.0, total.0)
    };
    let scores: Vec<_> = [0, 1, 2, 4, 8, 16, 32].into_iter().map(|n| (n, key(&after(n)))).collect();
    let (greedy_score, full_score) = (key(&greedy), key(&full));
    assert!(scores.windows(2).all(|w| w[1].1 <= w[0].1), "more rounds never lose what an earlier stop had: {scores:?}");
    assert!(scores.iter().all(|(_, s)| *s <= greedy_score && full_score <= *s), "every stop sits between the greedy packing and the full run: {scores:?}");
    assert_eq!(after(4), after(4), "the same stop gives the same layout");
    let kept = scores.iter().find(|(_, s)| *s < greedy_score && *s > full_score);
    let (n, _) = kept.expect("some stop lands strictly between greedy and the full run, so a mid-loop cancel is observable");
    let partial = after(*n);
    assert_eq!(partial.sheets.iter().map(|s| s.placed.len()).sum::<usize>(), pieces.len(), "a stopped run still places every piece");
}

#[test]
fn a_piece_too_large_in_every_rotation_overflows_onto_its_own_mat() {
    let pieces = [piece(vec![rect(0.0, 0.0, 13.0, 13.0)]), piece(vec![rect(0.0, 0.0, 1.0, 1.0)])];
    let result = run(&pieces, area(12.0, 12.0), &NestSettings::default(), GREEDY);
    assert_eq!(result.sheets.iter().map(|s| (s.overflow, s.placed.len())).collect::<Vec<_>>(), [(true, 1), (false, 1)]);
    assert_eq!((result.sheets[0].placed[0].x_in, result.sheets[0].placed[0].y_in), (PAD, PAD));
}

#[test]
fn a_full_mat_spills_onto_a_second() {
    let pieces: Vec<Piece> = (0..30).map(|_| piece(vec![rect(0.0, 0.0, 2.0, 2.0)])).collect();
    let result = run(&pieces, area(12.0, 12.0), &NestSettings::default(), GREEDY);
    assert_eq!(result.sheets.len(), 2);
    assert_eq!(result.sheets[0].placed.len(), 25, "five rows of five 2 in squares fit 11.5 in with 0.125 in gaps");
    assert_spaced(&pieces, &result, area(12.0, 12.0), SPACING);
}

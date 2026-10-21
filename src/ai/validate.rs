//! Model output is untrusted input. Every candidate SVG passes through [`check`] before a tile shows it.
//!
//! The raw XML is screened first, before usvg sees it: usvg's default resolver would read an
//! `<image href>` from disk. What passes is unioned into one compound outline 4 in wide and judged
//! as a cut: budgets, weedability, and for Stencil, holes that would fall out. Pure: no file,
//! network or clock.

use std::collections::HashMap;

use egui::Color32;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::simplify::SimplifyShape;
use roxmltree::{Document, Node, NodeId};

use crate::design::{Contour, Design, Operation, Pt, Seg, Shape, ShapeKind};
use crate::geometry::{FLATTEN_TOL_IN, Ring, flatten, rdp, signed_area};
use crate::shape_library::{Import, import_svg};

use super::{Detail, Style, WIDTH_IN};

/// The largest SVG the validator reads.
const MAX_BYTES: usize = 256 * 1024;
/// The most elements a design may have, counting each `<use>` as the elements it copies.
const MAX_ELEMENTS: u64 = 5000;
/// The deepest element nesting, `<use>` chains included. usvg recurses per level on a thread
/// with a small stack.
const MAX_DEPTH: usize = 64;
/// The share of the area tiny pieces may take before the candidate is rejected rather than trimmed.
const MAX_TINY_SHARE: f32 = 0.05;
/// How far, in inches, simplifying may move the piece's outline.
const SIMPLIFY_IN: f32 = 0.004;

const TEXT: &str = "it contains text";
const IMAGE: &str = "it contains an image";
const SCRIPT: &str = "it contains a script";
const EXTERNAL: &str = "it references an external file";
const GRADIENT: &str = "it uses a gradient or pattern";
const CLIP: &str = "it uses a clip path or mask";
const STROKE: &str = "a path is drawn as a stroke, not a filled shape";

/// One outer ring and its holes, in inches.
type Island = Vec<Ring>;

/// The cut-ready piece for `svg`, or every reason it is not.
pub fn check(svg: &str, style: Style, detail: Detail) -> Result<Shape, Vec<String>> {
    let mut reasons = screen(svg);
    if !reasons.is_empty() {
        return Err(reasons);
    }
    let stroked = stroked_paths(svg).map_err(|why| vec![why])?;
    if stroked > 0 {
        reasons.push(STROKE.into());
    }
    let imports = import_svg(svg.as_bytes()).map_err(|_| vec!["it draws no filled shape".to_owned()])?;
    reasons.extend(over_budget(&imports, detail));
    // `import_svg` reads a stroke as if it were filled, so the outline would be wrong.
    if stroked > 0 {
        return Err(reasons);
    }
    let Some(islands) = outline(&imports) else {
        reasons.push("it draws nothing that can be cut".into());
        return Err(reasons);
    };
    let islands = match weed(islands, detail) {
        Ok(kept) => kept,
        Err(why) => {
            reasons.push(why);
            return Err(reasons);
        }
    };
    let holes: usize = islands.iter().map(|i| i.len() - 1).sum();
    if style == Style::Stencil && holes > 0 {
        let noun = if holes == 1 { "hole" } else { "holes" };
        reasons.push(format!("{holes} enclosed {noun} would fall out of the stencil"));
    }
    if !reasons.is_empty() {
        return Err(reasons);
    }
    piece(islands).map_err(|why| vec![why])
}

/// Every reason the raw XML is unsafe or unsupported, before any SVG parser runs.
fn screen(svg: &str) -> Vec<String> {
    if svg.len() > MAX_BYTES {
        return vec![format!("it is larger than {} KB", MAX_BYTES / 1024)];
    }
    let doc = match Document::parse(svg) {
        Ok(doc) => doc,
        Err(roxmltree::Error::DtdDetected) => return vec!["it has a DOCTYPE".into()],
        Err(_) => return vec!["it is not valid XML".into()],
    };
    if doc.root_element().tag_name().name() != "svg" {
        return vec!["its root element is not <svg>".into()];
    }
    let mut reasons: Vec<String> = Vec::new();
    let mut note = |why: &str| {
        if !reasons.iter().any(|r| r == why) {
            reasons.push(why.into());
        }
    };
    for node in doc.descendants() {
        if let Some(pi) = node.pi() {
            if pi.target.eq_ignore_ascii_case("xml-stylesheet") {
                note(EXTERNAL);
            }
            continue;
        }
        if !node.is_element() {
            continue;
        }
        match node.tag_name().name().to_ascii_lowercase().as_str() {
            "text" | "tspan" | "textpath" => note(TEXT),
            "image" => note(IMAGE),
            "script" | "foreignobject" | "iframe" => note(SCRIPT),
            "lineargradient" | "radialgradient" | "pattern" => note(GRADIENT),
            "clippath" | "mask" => note(CLIP),
            "style" => {
                let css: String = node.descendants().filter_map(|n| n.text()).collect();
                if !urls_are_local(&css) || css.to_ascii_lowercase().contains("@import") {
                    note(EXTERNAL);
                }
            }
            _ => {}
        }
        for attr in node.attributes() {
            let name = attr.name();
            if name.len() > 2 && name[..2].eq_ignore_ascii_case("on") {
                note(SCRIPT);
            }
            if (name == "href" && !attr.value().trim_start().starts_with('#')) || !urls_are_local(attr.value()) {
                note(EXTERNAL);
            }
        }
    }
    let raw = doc.descendants().filter(Node::is_element).count() as u64;
    let mut walk = Walk { ids: doc.descendants().filter_map(|n| n.attribute("id").map(|id| (id, n))).collect(), memo: HashMap::new(), depth: 0, too_deep: false };
    let expanded = if raw > MAX_ELEMENTS { raw } else { walk.weight(doc.root_element()) };
    if walk.too_deep {
        reasons.push("it nests elements too deeply".into());
    } else if expanded > MAX_ELEMENTS {
        reasons.push(format!("it has more than {MAX_ELEMENTS} elements"));
    }
    reasons
}

/// Whether every `url(...)` in `text` points inside the document.
fn urls_are_local(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.match_indices("url(").all(|(at, open)| {
        let rest = &text[at + open.len()..];
        rest.find(')').is_some_and(|end| rest[..end].trim().trim_matches(['"', '\'']).trim_start().starts_with('#'))
    })
}

/// Counts elements with each `<use>` standing for what it copies, and notes nesting past [`MAX_DEPTH`].
struct Walk<'a, 'input> {
    ids: HashMap<&'a str, Node<'a, 'input>>,
    memo: HashMap<NodeId, u64>,
    depth: usize,
    too_deep: bool,
}

impl<'a, 'input> Walk<'a, 'input> {
    fn weight(&mut self, node: Node<'a, 'input>) -> u64 {
        if let Some(&w) = self.memo.get(&node.id()) {
            return w;
        }
        if self.depth >= MAX_DEPTH {
            // Also where a `<use>` cycle ends up.
            self.too_deep = true;
            return MAX_ELEMENTS + 1;
        }
        self.depth += 1;
        let mut w = 1u64;
        for child in node.children().filter(Node::is_element) {
            w = w.saturating_add(self.weight(child));
        }
        if node.tag_name().name() == "use" {
            let target = node.attributes().find(|a| a.name() == "href").and_then(|a| a.value().trim().strip_prefix('#')).and_then(|id| self.ids.get(id).copied());
            if let Some(target) = target {
                w = w.saturating_add(self.weight(target));
            }
        }
        self.depth -= 1;
        self.memo.insert(node.id(), w);
        w
    }
}

/// How many paths have a stroke and no fill. A path with neither never reaches here: usvg drops it.
fn stroked_paths(svg: &str) -> Result<usize, String> {
    let tree = crate::svg_import::parse_tree(svg.as_bytes()).map_err(|_| "it is not valid SVG".to_owned())?;
    let stroked = tree.root.descendants().filter(|node| matches!(&*node.borrow(), usvg::NodeKind::Path(p) if p.stroke.is_some() && p.fill.is_none())).count();
    Ok(stroked)
}

/// The path and node budget reasons, if any. Nodes are segment ends plus each subpath's start.
fn over_budget(imports: &[Import], detail: Detail) -> Vec<String> {
    let mut reasons = Vec::new();
    let paths = imports.len();
    if paths > detail.path_budget() {
        reasons.push(format!("it uses {paths} paths; {} allows {}", detail.label(), detail.path_budget()));
    }
    let nodes: usize = imports.iter().flat_map(|i| &i.contours).map(|c| 1 + c.segs.len()).sum();
    if nodes > detail.node_budget() {
        reasons.push(format!("it uses {nodes} nodes; {} allows {}", detail.label(), detail.node_budget()));
    }
    reasons
}

/// Every fill unioned into islands, scaled so the whole is [`WIDTH_IN`] wide at the origin. Each
/// path is read even-odd (holes are subpaths), then paths are unioned non-zero. `None` when nothing
/// with area remains.
fn outline(imports: &[Import]) -> Option<Vec<Island>> {
    // The imports' bounds include curve control points, so this scale is near the final one, and
    // flattening at it keeps the tolerance meaningful whatever the SVG's own units.
    let x0 = imports.iter().map(|i| i.x_in).fold(f32::INFINITY, f32::min);
    let y0 = imports.iter().map(|i| i.y_in).fold(f32::INFINITY, f32::min);
    let x1 = imports.iter().map(|i| i.x_in + i.view_box.0).fold(f32::NEG_INFINITY, f32::max);
    let scale = WIDTH_IN / (x1 - x0);
    if !(scale.is_finite() && scale > 0.0 && y0.is_finite()) {
        return None;
    }
    let mut subject: Vec<Vec<[f64; 2]>> = Vec::new();
    for imp in imports {
        let at = |p: Pt| ((p.0 + imp.x_in - x0) * scale, (p.1 + imp.y_in - y0) * scale);
        let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
        for c in &imp.contours {
            let segs = c.segs.iter().map(|s| match *s {
                Seg::Line(p) => Seg::Line(at(p)),
                Seg::Cubic(a, b, p) => Seg::Cubic(at(a), at(b), at(p)),
            });
            let placed = Contour { start: at(c.start), segs: segs.collect(), closed: true };
            let ring = flatten(&placed, FLATTEN_TOL_IN / 2.0);
            if ring.iter().any(|p| !(p.0.is_finite() && p.1.is_finite())) {
                return None;
            }
            if ring.len() >= 3 {
                rings.push(ring.iter().map(|&(x, y)| [x as f64, y as f64]).collect());
            }
        }
        subject.extend(rings.simplify_shape(FillRule::EvenOdd).into_iter().flatten());
    }
    let islands: Vec<Island> = subject
        .simplify_shape(FillRule::NonZero)
        .into_iter()
        .map(|shape| shape.into_iter().map(|c| c.into_iter().map(|p| (p[0] as f32, p[1] as f32)).collect::<Ring>()).filter(|r| r.len() >= 3).collect::<Island>())
        .filter(|island| island.first().is_some_and(|outer| signed_area(outer).abs() > 0.0))
        .collect();
    fit(islands)
}

/// Moves the islands to the origin and scales them to [`WIDTH_IN`] wide. `None` when degenerate.
fn fit(mut islands: Vec<Island>) -> Option<Vec<Island>> {
    let (min, max) = bounds(islands.iter().map(|i| &i[0]))?;
    let scale = WIDTH_IN / (max.0 - min.0);
    if !(scale.is_finite() && scale > 0.0 && max.1 > min.1) {
        return None;
    }
    for p in islands.iter_mut().flatten().flatten() {
        *p = ((p.0 - min.0) * scale, (p.1 - min.1) * scale);
    }
    Some(islands)
}

fn bounds<'a>(rings: impl Iterator<Item = &'a Ring>) -> Option<(Pt, Pt)> {
    let mut points = rings.flatten().peekable();
    points.peek()?;
    Some(
        points.fold(((f32::INFINITY, f32::INFINITY), (f32::NEG_INFINITY, f32::NEG_INFINITY)), |(lo, hi), p| {
            ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1)))
        }),
    )
}

fn area(island: &Island) -> f32 {
    signed_area(&island[0]).abs() - island[1..].iter().map(|h| signed_area(h).abs()).sum::<f32>()
}

/// Drops islands narrower than the detail's minimum feature, then refits what is left to
/// [`WIDTH_IN`]. Rejects when the dropped islands were more than [`MAX_TINY_SHARE`] of the area.
fn weed(islands: Vec<Island>, detail: Detail) -> Result<Vec<Island>, String> {
    let min = detail.min_feature_in();
    let total: f32 = islands.iter().map(area).sum();
    let (kept, tiny): (Vec<Island>, Vec<Island>) =
        islands.into_iter().partition(|island| bounds(island.iter().take(1)).is_some_and(|(lo, hi)| (hi.0 - lo.0).min(hi.1 - lo.1) >= min));
    if !(total.is_finite() && total > 0.0) || kept.is_empty() {
        return Err("it draws nothing that can be cut".into());
    }
    let share = tiny.iter().map(area).sum::<f32>() / total;
    if share > MAX_TINY_SHARE {
        // Never "5 %" for a share just over the limit.
        let percent = (share * 100.0).round().max(6.0);
        return Err(format!("tiny pieces make up {percent} % of it; {} needs every piece at least {min} inches across", detail.label()));
    }
    fit(kept).ok_or_else(|| "it draws nothing that can be cut".into())
}

/// The islands, simplified, as one closed-polyline Imported shape at the origin, checked by
/// `Design::sanitise`.
fn piece(islands: Vec<Island>) -> Result<Shape, String> {
    let simplify = |ring: &Ring| {
        let mut pts = ring.clone();
        pts.push(ring[0]);
        let mut pts = rdp(&pts, SIMPLIFY_IN);
        pts.pop();
        (pts.len() >= 3).then_some(pts)
    };
    let simplified = islands.iter().filter_map(|island| {
        let outer = simplify(&island[0])?;
        Some(std::iter::once(outer).chain(island[1..].iter().filter_map(simplify)).collect::<Island>())
    });
    // Simplifying may shave an extreme point, so fit again for an exact width.
    let Some(islands) = fit(simplified.collect()) else {
        return Err("it draws nothing that can be cut".into());
    };
    let height = bounds(islands.iter().map(|i| &i[0])).map_or(0.0, |(lo, hi)| hi.1 - lo.1);
    let contours: Vec<Contour> = islands.iter().flatten().map(|r| Contour { start: r[0], segs: r[1..].iter().map(|&p| Seg::Line(p)).collect(), closed: true }).collect();
    let shape = Shape {
        width_in: WIDTH_IN,
        height_in: height,
        fill: Color32::BLACK,
        pen: crate::pens::Pen::nearest(Color32::BLACK, Default::default()),
        ..Shape::new(0, ShapeKind::Imported { contours, view_box: (WIDTH_IN, height) })
    };
    debug_assert_eq!(shape.operation, Operation::BasicCut);
    let mut scratch = Design::default();
    scratch.shapes.push(shape);
    let repairs = scratch.sanitise();
    match scratch.shapes.pop() {
        Some(shape) if repairs.is_empty() => Ok(shape),
        _ => Err("its outline cannot be placed at 4 in wide".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_SVG: &str = include_str!("fixtures/text.svg");
    const IMAGE_SVG: &str = include_str!("fixtures/image.svg");
    const STROKE_SVG: &str = include_str!("fixtures/stroke-only.svg");
    const TINY_SVG: &str = include_str!("fixtures/tiny-islands.svg");
    const FEW_TINY_SVG: &str = include_str!("fixtures/few-tiny-islands.svg");
    const RING_SVG: &str = include_str!("fixtures/stencil-ring.svg");
    const BRIDGED_SVG: &str = include_str!("fixtures/stencil-bridged.svg");

    fn contours(piece: &Shape) -> &[Contour] {
        match &piece.kind {
            ShapeKind::Imported { contours, .. } => contours,
            other => panic!("not Imported: {other:?}"),
        }
    }

    fn rejected(svg: &str, style: Style, detail: Detail) -> Vec<String> {
        check(svg, style, detail).expect_err("rejected")
    }

    #[test]
    fn the_clean_fixtures_pass_as_silhouettes_and_stickers() {
        for (title, svg) in crate::ai::fixtures::CLEAN {
            for style in [Style::Silhouette, Style::Sticker] {
                let piece = check(svg, style, Detail::Medium).unwrap_or_else(|r| panic!("{title} as {style:?}: {r:?}"));
                assert_eq!(piece.operation, Operation::BasicCut);
                assert_eq!(piece.fill, Color32::BLACK);
                assert!(!contours(&piece).is_empty());
            }
        }
    }

    #[test]
    fn a_piece_is_four_inches_wide_at_the_origin() {
        let piece = check(crate::ai::fixtures::DINOSAUR, Style::Silhouette, Detail::Medium).unwrap();
        assert_eq!((piece.x_in, piece.y_in, piece.width_in), (0.0, 0.0, WIDTH_IN));
        let ShapeKind::Imported { view_box, .. } = &piece.kind else { unreachable!() };
        assert_eq!(*view_box, (WIDTH_IN, piece.height_in));
        let points: Vec<Pt> = contours(&piece).iter().flat_map(|c| flatten(c, FLATTEN_TOL_IN)).collect();
        let (lo, hi) = bounds(std::iter::once(&points)).unwrap();
        assert!(lo.0.abs() < 1e-3 && lo.1.abs() < 1e-3, "{lo:?}");
        assert!((hi.0 - WIDTH_IN).abs() < 1e-3 && (hi.1 - piece.height_in).abs() < 1e-3, "{hi:?}");
    }

    #[test]
    fn text_an_image_or_a_stroke_only_path_is_rejected() {
        assert_eq!(rejected(TEXT_SVG, Style::Silhouette, Detail::Medium), [TEXT]);
        assert_eq!(rejected(IMAGE_SVG, Style::Silhouette, Detail::Medium), [IMAGE, EXTERNAL]);
        assert_eq!(rejected(STROKE_SVG, Style::Silhouette, Detail::Medium), [STROKE]);
    }

    #[test]
    fn an_image_of_a_local_file_is_rejected_before_any_svg_parser_reads_it() {
        assert_eq!(screen(IMAGE_SVG), [IMAGE, EXTERNAL], "the XML screen stops it, so usvg never resolves the href");
        let use_of_file =
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><use xlink:href="file:///etc/passwd#x"/></svg>"#;
        assert_eq!(screen(use_of_file), [EXTERNAL]);
    }

    #[test]
    fn scripts_external_urls_and_gradients_are_each_named() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" onload="x()">
            <style>path { fill: url(https://example.com/a.svg#g) }</style>
            <linearGradient id="g"/>
            <path d="M0 0 H10 V10 H0 Z" fill="url(#g)"/>
        </svg>"##;
        assert_eq!(screen(svg), [SCRIPT, EXTERNAL, GRADIENT]);
        assert_eq!(screen("<html/>"), ["its root element is not <svg>"]);
        assert_eq!(screen("<svg"), ["it is not valid XML"]);
        assert!(screen(&" ".repeat(MAX_BYTES + 1))[0].contains("256 KB"));
    }

    #[test]
    fn a_use_bomb_is_rejected_by_its_expanded_size() {
        let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs><path id="a0" d="M0 0 H1 V1 Z"/>"#);
        for i in 1..8 {
            svg += &format!(r#"<g id="a{i}">{}</g>"#, (0..10).map(|_| format!(r##"<use href="#a{}"/>"##, i - 1)).collect::<String>());
        }
        svg += r##"</defs><use href="#a7"/></svg>"##;
        assert_eq!(screen(&svg), ["it has more than 5000 elements"]);
    }

    #[test]
    fn tiny_islands_are_removed_from_the_piece() {
        let piece = check(FEW_TINY_SVG, Style::Silhouette, Detail::Medium).unwrap();
        assert_eq!(contours(&piece).len(), 1, "the two dots weed away, the bar stays");
        assert_eq!(import_svg(FEW_TINY_SVG.as_bytes()).unwrap().iter().map(|i| i.contours.len()).sum::<usize>(), 3);
    }

    #[test]
    fn tiny_islands_over_five_percent_of_the_area_are_rejected() {
        let reasons = rejected(TINY_SVG, Style::Silhouette, Detail::Medium);
        assert_eq!(reasons, ["tiny pieces make up 8 % of it; Medium needs every piece at least 0.15 inches across"]);
        assert!(check(TINY_SVG, Style::Silhouette, Detail::Detailed).is_ok(), "at Detailed the dots are big enough");
    }

    #[test]
    fn an_over_budget_path_count_is_rejected_with_its_reason() {
        let squares: String = (0..9).map(|i| format!(r#"<path d="M{x} 0 h8 v8 h-8 Z"/>"#, x = i * 10)).collect();
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 90 10">{squares}</svg>"#);
        assert_eq!(rejected(&svg, Style::Silhouette, Detail::Simple), ["it uses 9 paths; Simple allows 8"]);
        assert!(check(&svg, Style::Silhouette, Detail::Medium).is_ok());
    }

    #[test]
    fn a_stencil_with_an_unbridged_hole_is_rejected_and_a_bridged_one_passes() {
        assert_eq!(rejected(RING_SVG, Style::Stencil, Detail::Medium), ["1 enclosed hole would fall out of the stencil"]);
        assert!(check(RING_SVG, Style::Silhouette, Detail::Medium).is_ok(), "only a stencil minds the hole");
        let piece = check(BRIDGED_SVG, Style::Stencil, Detail::Medium).unwrap();
        assert_eq!(contours(&piece).len(), 1);
    }

    #[test]
    fn every_applicable_reason_is_collected() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><text>Hi</text><image href="a.png"/><script/></svg>"#;
        assert_eq!(rejected(svg, Style::Silhouette, Detail::Medium), [TEXT, IMAGE, EXTERNAL, SCRIPT]);
    }

    #[test]
    fn overlapping_paths_weld_into_one_piece_and_even_odd_subpaths_cut_holes() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 10">
            <path d="M0 0 H20 V10 H0 Z M5 3 H8 V7 H5 Z"/>
            <path d="M15 0 H30 V10 H15 Z"/>
        </svg>"#;
        let piece = check(svg, Style::Silhouette, Detail::Medium).unwrap();
        assert_eq!(contours(&piece).len(), 2, "one welded outline and one hole");
    }
}

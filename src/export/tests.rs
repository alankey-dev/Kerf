use egui::Color32;

use super::*;
use crate::design::{Shape, ShapeKind};
use crate::edit::{Edit, Subject};
use crate::machine::MachineFamily;
use crate::shape_library::{BuiltinId, Import, import_svg};
use crate::text::FontRef;

pub(crate) const TOL: f32 = 0.001;

pub(crate) fn mat() -> MatSize {
    MachineFamily::StudioPro.preferred_mat()
}

pub(crate) fn add(d: &mut Design, kind: ShapeKind, op: Operation, fill: Color32, at: Pt) -> u64 {
    let id = d.add_shape(kind);
    let s = d.shape_mut(id).unwrap();
    (s.x_in, s.y_in, s.operation, s.fill) = (at.0, at.1, op, fill);
    id
}

/// A square, a heart, a ring and a text object, the heart turned so placement is more than a shift.
pub(crate) fn sampler() -> Design {
    let mut d = Design::default();
    add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    let heart = add(&mut d, ShapeKind::Heart, Operation::BasicCut, Color32::RED, (4.0, 1.5));
    d.shape_mut(heart).unwrap().rotation_deg = 30.0;
    add(&mut d, ShapeKind::Builtin(BuiltinId::named("Ring").unwrap()), Operation::BasicCut, Color32::BLUE, (1.0, 4.0));
    d.add_text("Kerf".into(), FontRef::Inter, 1.0, Color32::BLACK, 4.0, 4.5);
    d
}

pub(crate) fn request(format: Format, scope: Scope, units: Units) -> Request {
    Request { format, scope, style: Style::Filled, units, mirror: false, raster: crate::export::raster::RasterOptions::default() }
}

fn points(c: &Contour) -> Vec<Pt> {
    let mut pts = vec![c.start];
    for seg in &c.segs {
        match *seg {
            Seg::Line(p) => pts.push(p),
            Seg::Cubic(a, b, p) => pts.extend([a, b, p]),
        }
    }
    pts
}

fn near(a: Pt, b: Pt) -> bool {
    (a.0 - b.0).abs() <= TOL && (a.1 - b.1).abs() <= TOL
}

/// The import's contours back in sheet inches.
fn placed(import: &Import) -> Vec<Vec<Pt>> {
    import.contours.iter().map(|c| points(c).into_iter().map(|(x, y)| (x + import.x_in, y + import.y_in)).collect()).collect()
}

fn bounds(contours: &[Vec<Pt>]) -> (f32, f32, f32, f32) {
    contours.iter().flatten().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
}

fn assert_same(got: &[Vec<Pt>], want: &[Vec<Pt>], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: contour count");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g.len(), w.len(), "{what}: contour {i} point count");
        for (a, b) in g.iter().zip(w) {
            assert!(near(*a, *b), "{what}: contour {i} has {a:?} for {b:?}");
        }
    }
    let (g, w) = (bounds(got), bounds(want));
    assert!(near((g.0, g.1), (w.0, w.1)) && near((g.2, g.3), (w.2, w.3)), "{what}: bounds {g:?} for {w:?}");
}

fn round_trip(units: Units) {
    let d = sampler();
    let [file] = &plan(&d, &request(Format::Svg, Scope::Design, units), "Untitled-1", mat())[..] else { panic!("one file") };
    let imports = import_svg(&file.bytes(&Default::default())).expect("the export parses");
    assert_eq!(imports.len(), d.shapes.len(), "one path per shape");
    let origin = d.shapes.iter().map(Shape::aabb).fold(Aabb::EMPTY, Aabb::union);
    let shift = |(x, y): Pt| (x - origin.min_x + DESIGN_PAD_IN, y - origin.min_y + DESIGN_PAD_IN);
    for (import, shape) in imports.iter().zip(&d.shapes) {
        let want: Vec<Vec<Pt>> = shape.world_contours().iter().map(|c| points(c).into_iter().map(shift).collect()).collect();
        assert_same(&placed(import), &want, shape.kind.label());
    }
}

#[test]
fn an_svg_in_inches_reimports_to_the_source_geometry() {
    round_trip(Units::Imperial);
}

#[test]
fn an_svg_in_millimetres_reimports_to_the_same_geometry() {
    round_trip(Units::Metric);
}

#[test]
fn a_per_mat_svg_reimports_at_the_composed_offsets() {
    let mut d = Design::default();
    add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::RED, (1.0, 1.0));
    add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLUE, (5.0, 3.0));
    let files = plan(&d, &request(Format::Svg, Scope::PerMat, Units::Imperial), "Untitled-1", mat());
    let mats = composition::compose(&d, mat(), 1);
    assert_eq!((files.len(), mats.len()), (2, 2));
    for (file, mat) in files.iter().zip(&mats) {
        let [import] = &import_svg(&file.bytes(&Default::default())).expect("parses")[..] else { panic!("one shape per mat") };
        let [item] = &mat.items[..] else { panic!("one item") };
        assert!(near((import.x_in, import.y_in), item.offset_in), "{} at {:?}, composed at {:?}", file.name, (import.x_in, import.y_in), item.offset_in);
    }
}

#[test]
fn layers_are_named_by_operation_and_shape_and_repeats_are_numbered() {
    let mut d = Design::default();
    let pink = Color32::from_rgb(0xe5, 0x48, 0x4d);
    add(&mut d, ShapeKind::Heart, Operation::Score, pink, (1.0, 1.0));
    add(&mut d, ShapeKind::Heart, Operation::Score, pink, (4.0, 1.0));
    let [file] = &plan(&d, &request(Format::Svg, Scope::Design, Units::Imperial), "p", mat())[..] else { panic!("one file") };
    let names: Vec<&str> = file.sheets()[0].layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Score - Heart", "Score - Heart 2"]);
    let svg = String::from_utf8(file.bytes(&Default::default())).unwrap();
    let at = svg.find("inkscape:label=\"Score - Heart\"").expect("a labelled layer");
    let open = svg[..at].rfind('<').unwrap();
    let tag = &svg[open..open + svg[open..].find('>').unwrap()];
    assert!(tag.starts_with("<g ") && tag.contains("inkscape:groupmode=\"layer\""), "{tag}");
    let body = &svg[at..svg[at..].find("</g>").unwrap() + at];
    assert!(body.contains("stroke=\"#e5484d\""), "the heart's colour: {body}");
    assert!(svg.contains("inkscape:label=\"Score - Heart 2\""));
}

#[test]
fn an_attached_pen_star_and_cut_square_share_a_group() {
    let mut d = Design::default();
    let square = add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    let star = add(&mut d, ShapeKind::Star, Operation::Pen, Color32::BLACK, (1.5, 1.5));
    crate::edit::run(&mut d, &mut vec![square, star], &Edit::Attach, Subject::Selection).expect("attaches");
    let sheet = &sheets(&d, Scope::Design, &MatSetup::preferred(MachineFamily::StudioPro))[0];
    let groups: Vec<(&str, &[String])> = sheet.layers.iter().map(|l| (l.name.as_str(), &l.group[..])).collect();
    let attached = ["Attached - Square".to_owned()];
    assert_eq!(groups, [("Basic Cut - Square", &attached[..]), ("Pen - Star", &attached[..])]);
    let svg = svg::document(sheet, Units::Imperial, Style::Filled, &Default::default());
    let group = svg.find("inkscape:label=\"Attached - Square\"").expect("the link's group");
    assert!(svg[group..].find("Basic Cut - Square").is_some() && svg[group..].find("Pen - Star").is_some());
    assert_eq!(svg.matches("<g ").count(), 3, "{svg}");
}

#[test]
fn renamed_layers_and_links_export_under_their_names() {
    let mut d = Design::default();
    let square = add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    let star = add(&mut d, ShapeKind::Star, Operation::Pen, Color32::BLACK, (1.5, 1.5));
    let mut held = vec![square, star];
    crate::edit::run(&mut d, &mut held, &Edit::Attach, Subject::Selection).expect("attaches");
    crate::edit::run(&mut d, &mut held, &Edit::Rename(Some("Badge".into())), Subject::Selection).expect("names the link");
    crate::edit::run(&mut d, &mut held, &Edit::Rename(Some("Speech Bubble".into())), Subject::Ids(&[star])).expect("names the star");
    let sheet = &sheets(&d, Scope::Design, &MatSetup::preferred(MachineFamily::StudioPro))[0];
    let groups: Vec<(&str, &[String])> = sheet.layers.iter().map(|l| (l.name.as_str(), &l.group[..])).collect();
    let badge = ["Badge".to_owned()];
    assert_eq!(groups, [("Basic Cut - Square", &badge[..]), ("Pen - Speech Bubble", &badge[..])]);
}

#[test]
fn a_container_groups_its_children_under_its_name() {
    let mut d = Design::default();
    let a = add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    let b = add(&mut d, ShapeKind::Circle, Operation::BasicCut, Color32::BLACK, (4.0, 1.0));
    d.contain(&[a, b]).expect("contains");
    let sheet = &sheets(&d, Scope::Design, &MatSetup::preferred(MachineFamily::StudioPro))[0];
    assert!(sheet.layers.iter().all(|l| l.group == ["Rectangle"]), "{:?}", sheet.layers.iter().map(|l| &l.group).collect::<Vec<_>>());
}

#[test]
fn an_old_project_exports_neither_its_hidden_shape_nor_its_guide() {
    let shape = |id: u32, kind: &str, op: &str, hidden: bool| {
        format!(
            r#"{{"id":{id},"kind":"{kind}","x_in":{id}.0,"y_in":1.0,"width_in":1.0,"height_in":1.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"{op}","hidden":{hidden},"contour_hidden":[]}}"#
        )
    };
    let shapes = [shape(1, "Square", "BasicCut", false), shape(3, "Heart", "BasicCut", true), shape(5, "Circle", "Guide", false)].join(",");
    let json = format!(r#"{{"id":"old","name":"Old","design":{{"shapes":[{shapes}]}},"created":1,"modified":1}}"#);
    let (project, _) = crate::project::parse(&json).expect("format 1 loads");
    for scope in [Scope::Design, Scope::PerMat] {
        let layers: Vec<String> =
            sheets(&project.design, scope, &MatSetup::preferred(MachineFamily::StudioPro)).into_iter().flat_map(|s| s.layers).map(|l| l.name).collect();
        assert_eq!(layers, ["Basic Cut - Square"], "{scope:?}");
    }
}

#[test]
fn prepare_for_design_space_writes_one_svg_per_mat_in_inches_and_the_guide() {
    let mut d = Design::default();
    add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    add(&mut d, ShapeKind::Heart, Operation::Score, Color32::from_rgb(0xd0, 0x20, 0x2e), (4.0, 1.0));
    let files = design_space(&d, "Untitled-1", mat());
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Untitled-1 - Mat 1 - Black - Basic Cut.svg", "Untitled-1 - Mat 2 - Red - Score.svg", DESIGN_SPACE_GUIDE]);
    let svg = String::from_utf8(files[0].bytes(&Default::default())).unwrap();
    let attr = |name: &str| {
        let at = svg.find(&format!(" {name}=\"")).expect(name) + name.len() + 3;
        svg[at..at + svg[at..].find('"').unwrap()].to_owned()
    };
    let view: Vec<f32> = attr("viewBox").split(' ').map(|v| v.parse().unwrap()).collect();
    for (side, view_pt) in [("width", view[2]), ("height", view[3])] {
        let value = attr(side);
        let inches: f32 = value.strip_suffix(Units::Imperial.file_unit()).unwrap_or_else(|| panic!("{side} {value} states inches")).parse().unwrap();
        assert!((inches * POINTS_PER_INCH - view_pt).abs() < 1e-3, "{side} {inches} against a viewBox of {view_pt} points");
    }
    assert!(!svg.contains("clipPath") && !svg.contains("<pattern"), "Design Space rejects both");
    assert!(svg.contains("fill=\"#000000\""), "one solid fill per path");
    let guide = String::from_utf8(files[2].bytes(&Default::default())).unwrap();
    assert!(guide.contains("Untitled-1 - Mat 2 - Red - Score.svg\n    Score - Heart, #d0202e"), "{guide}");
    assert!(guide.contains("mat's ruler"), "asks for a size check before Make");
}

#[test]
fn nothing_visible_plans_no_files() {
    let mut d = Design::default();
    let id = add(&mut d, ShapeKind::Square, Operation::Guide, Color32::BLACK, (1.0, 1.0));
    assert!(!has_output(&d));
    d.shape_mut(id).unwrap().operation = Operation::BasicCut;
    assert!(has_output(&d));
    d.shape_mut(id).unwrap().hidden = true;
    for format in [Format::Svg, Format::Dxf, Format::Pdf] {
        assert!(plan(&d, &request(format, Scope::PerMat, Units::Imperial), "p", mat()).is_empty(), "{format:?}");
    }
    assert!(design_space(&d, "p", mat()).is_empty());
}

#[test]
fn mirror_flips_a_whole_design_sheet_left_to_right() {
    let mut d = Design::default();
    add(&mut d, ShapeKind::Square, Operation::BasicCut, Color32::BLACK, (1.0, 1.0));
    add(&mut d, ShapeKind::Circle, Operation::BasicCut, Color32::BLACK, (5.0, 1.0));
    let setup = |mirror| MatSetup { mirror, ..MatSetup::preferred(MachineFamily::StudioPro) };
    let (plain, mirrored) = (&sheets(&d, Scope::Design, &setup(false))[0], &sheets(&d, Scope::Design, &setup(true))[0]);
    let x = |s: &Sheet| s.layers[0].contours[0].start.0;
    assert!((x(plain) + x(mirrored) - plain.size_in.0).abs() < 1e-4);
}

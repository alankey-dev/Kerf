//! The heavy-design benchmark (spec 38). [`build`] makes one deterministic design from a seed, the single
//! scenario that the timing kittest and `kerf --bench` load; [`write`]
//! saves it as a Kerf project, a whole-design SVG and Prepare for Design Space files. `report` reads the
//! numbers back: frame-time statistics, process samples and the printed tables.

pub mod report;

use std::io;
use std::path::{Path, PathBuf};

use egui::Color32;
use image::RgbaImage;

use crate::assets::{self, AssetId};
use crate::containers::{Axis, Cross, Flow, Sizing};
use crate::design::{Combine, Design, Operation, Pt, Shape, ShapeKind, UnitKey, units_by};
use crate::edit::{Edit, Placement};
use crate::effects::{Corners, Direction, Effect, Offset, Reach, Sticker};
use crate::export::{self, Format, Request, Scope, Style};
use crate::geometry::Join;
use crate::machine::{MachineFamily, MatSize};
use crate::patterns::{PatternFill, PatternMode};
use crate::stroke_font::StrokeFontId;
use crate::text::FontRef;
use crate::warp::{Warp, WarpStyle};

pub const DEFAULT_SEED: u64 = 38;
pub const DEFAULT_OBJECTS: usize = 150;

/// The artefact names [`write`] uses inside its folder.
pub const PROJECT_FILE: &str = "heavy.kerf.json";
pub const SVG_FILE: &str = "heavy.svg";
pub const MATS_DIR: &str = "heavy-mats";
const TITLE: &str = "heavy";

/// The Print Then Cut image the bitmaps and pattern fills use: a disc on transparency.
const IMAGE_PNG: &[u8] = include_bytes!("../tests/fixtures/trace/disc.png");

const COLOURS: [Color32; 8] = [
    Color32::from_rgb(0xd6, 0x3a, 0x3a),
    Color32::from_rgb(0x2f, 0x6f, 0xd8),
    Color32::from_rgb(0x2c, 0x9a, 0x52),
    Color32::from_rgb(0xe8, 0xa3, 0x1c),
    Color32::from_rgb(0x7b, 0x3f, 0xc4),
    Color32::from_rgb(0x1e, 0x1e, 0x1e),
    Color32::from_rgb(0xe0, 0x5a, 0xa8),
    Color32::from_rgb(0x1f, 0xa5, 0xa5),
];

/// The library shapes cycle through these.
const OPERATIONS: [Operation; 5] = [Operation::BasicCut, Operation::Pen, Operation::Score, Operation::Perforate, Operation::PrintThenCut];

const WORDS: [&str; 10] = ["Kerf", "Bench", "Heavy", "Layers", "Cut", "Score", "Hello", "Paper", "Vinyl", "Mats"];

/// One grid cell per top-level object, row by row across a 12 in mat and on down past it.
const CELL_IN: f32 = 0.9;
const COLUMNS: usize = 13;
const MARGIN_IN: f32 = 0.25;

/// The image, decoded.
pub fn image() -> RgbaImage {
    assets::decode(IMAGE_PNG).expect("the bundled benchmark image decodes")
}

/// SplitMix64: a few lines, no dependency, and the same sequence on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn colour(&mut self) -> Color32 {
        COLOURS[self.below(COLOURS.len())]
    }
}

/// The design being built and the next free grid cell.
struct Builder {
    design: Design,
    rng: Rng,
    cell: usize,
}

impl Builder {
    /// The top-left of the next cell.
    fn cell(&mut self) -> Pt {
        let (col, row) = (self.cell % COLUMNS, self.cell / COLUMNS);
        self.cell += 1;
        (MARGIN_IN + col as f32 * CELL_IN, MARGIN_IN + row as f32 * CELL_IN)
    }

    fn push(&mut self, kind: ShapeKind, (x_in, y_in): Pt, (width_in, height_in): Pt, fill: Color32) -> u64 {
        let id = self.design.next_id();
        let pen = crate::pens::Pen::nearest(fill, Default::default());
        self.design.shapes.push(Shape { x_in, y_in, width_in, height_in, fill, pen, ..Shape::new(id, kind) });
        id
    }

    fn shape(&mut self, id: u64) -> &mut Shape {
        self.design.shape_mut(id).expect("a shape this builder made")
    }

    fn apply(&mut self, edit: &Edit, on: &[u64]) -> Vec<u64> {
        self.design.apply(edit, on).unwrap_or_else(|blocked| panic!("the scenario's {edit:?} is blocked: {blocked:?}"))
    }

    /// A closed library shape: any free shape but the open Line.
    fn library_kind(&mut self) -> ShapeKind {
        let kinds: Vec<ShapeKind> = ShapeKind::free().into_iter().filter(|k| *k != ShapeKind::Line).collect();
        kinds[self.rng.below(kinds.len())].clone()
    }

    /// `n` small closed shapes spread across one cell, for a group, an attach set or a container.
    fn cluster(&mut self, n: usize) -> Vec<u64> {
        let (x, y) = self.cell();
        let size = CELL_IN / n as f32;
        (0..n)
            .map(|i| {
                let kind = self.library_kind();
                let fill = self.rng.colour();
                self.push(kind, (x + i as f32 * size, y + (i % 2) as f32 * size * 0.5), (size * 0.9, size * 0.9), fill)
            })
            .collect()
    }

    fn library_shapes(&mut self, n: usize) {
        for i in 0..n {
            let kind = self.library_kind();
            let fill = self.rng.colour();
            let at = self.cell();
            let id = self.push(kind, at, (0.7, 0.7), fill);
            let turn = (i % 5 == 1).then(|| self.rng.below(12) as f32 * 30.0);
            let s = self.shape(id);
            s.operation = OPERATIONS[i % OPERATIONS.len()];
            s.rotation_deg = turn.unwrap_or(0.0);
            s.flip_x = i % 7 == 3;
            s.flip_y = i % 11 == 5;
        }
    }

    /// Outline fonts (bundled, and a system family that falls back where it is missing) and one writing
    /// font; every fifth text curves and the last is three lines.
    fn texts(&mut self, n: usize) {
        let fonts = [FontRef::Inter, FontRef::SpaceGrotesk, FontRef::System("Helvetica".into()), FontRef::Stroke(StrokeFontId("hershey-script-simplex".into()))];
        for i in 0..n {
            let content = if i + 1 == n { "Heavy\nbench\ndesign".to_owned() } else { WORDS[self.rng.below(WORDS.len())].to_owned() };
            let font = fonts[i % fonts.len()].clone();
            let writing = matches!(font, FontRef::Stroke(_));
            let (x, y) = self.cell();
            let fill = self.rng.colour();
            let id = self.design.add_text(content, font, 0.25, fill, x, y);
            if !writing {
                self.shape(id).operation = if i % 2 == 0 { Operation::BasicCut } else { Operation::PrintThenCut };
            }
            if i % 5 == 0 && i + 1 != n {
                self.apply(&Edit::Curve(2.5), &[id]);
            }
        }
    }

    fn links(&mut self, groups: usize, attaches: usize) {
        for i in 0..groups + attaches {
            let members = self.cluster(3 + i % 3);
            self.apply(if i < groups { &Edit::Group } else { &Edit::Attach }, &members);
        }
    }

    /// Five children each; the first lays out in a Row and the second in a three-column Grid.
    fn containers(&mut self, n: usize) {
        for i in 0..n {
            let children = self.cluster(5);
            let held = self.apply(&Edit::Contain, &children);
            let axis = match i {
                0 => Some(Axis::Row),
                1 => Some(Axis::Grid { columns: std::num::NonZeroU16::new(3).expect("3 is not zero") }),
                _ => None,
            };
            let before = self.design.clone();
            let role = self.shape(held[0]).container.as_mut().expect("Contain made a container");
            role.layout = axis.map(|axis| Flow { axis, gap_in: 0.05, cross: Cross::Centre, sizing: Sizing::Hug });
            self.design.settle(Some(&before));
        }
    }

    /// Offset out, Offset in, Sticker and Warp in turn.
    fn effects(&mut self, n: usize) {
        for i in 0..n {
            let kind = self.library_kind();
            let fill = self.rng.colour();
            let at = self.cell();
            let id = self.push(kind, at, (0.6, 0.6), fill);
            let offset = |direction| Effect::Offset(Offset { distance_in: 0.05, direction, join: Join::Round, style: None });
            let effect = match i % 4 {
                0 => offset(Direction::Outset),
                1 => offset(Direction::Inset),
                2 => Effect::Sticker(Sticker { border_in: 0.06, corners: Corners::Round }),
                _ => Effect::Warp(Warp { style: WarpStyle::Arch, bend: 0.4, vertical: false }),
            };
            self.apply(&Edit::AddEffect { effect, reach: Reach::EachShape }, &[id]);
        }
    }

    /// Weld, Subtract and Slice in turn over two or three overlapping shapes; Slice takes exactly two.
    fn booleans(&mut self, n: usize) {
        for i in 0..n {
            let op = [Combine::Weld, Combine::Subtract, Combine::Slice][i % 3];
            let count = if op == Combine::Slice { 2 } else { 2 + i % 2 };
            let (x, y) = self.cell();
            let fill = self.rng.colour();
            let inputs: Vec<u64> = (0..count).map(|k| self.push(ShapeKind::Circle, (x + k as f32 * 0.2, y + k as f32 * 0.1), (0.5, 0.5), fill)).collect();
            self.apply(&Edit::Combine(op), &inputs);
        }
    }

    /// Each master is a ring of `parts` shapes; every third instance overrides one part's fill.
    fn components(&mut self, masters: usize, instances: usize, parts: usize) {
        for m in 0..masters {
            let (x, y) = self.cell();
            let shapes: Vec<u64> = (0..parts)
                .map(|k| {
                    let a = k as f32 / parts as f32 * std::f32::consts::TAU;
                    let fill = self.rng.colour();
                    let kind = if (k + m) % 2 == 0 { ShapeKind::Heart } else { ShapeKind::Star };
                    self.push(kind, (x + 0.3 + 0.25 * a.cos(), y + 0.3 + 0.25 * a.sin()), (0.2, 0.2), fill)
                })
                .collect();
            let first = self.apply(&Edit::MakeComponent, &shapes)[0];
            let component = self.design.components.last().expect("MakeComponent made a component").id;
            let part = self.design.components.last().expect("the component").shapes[0].id;
            let mut placed = vec![first];
            for _ in 1..instances {
                let (x, y) = self.cell();
                placed.extend(self.apply(&Edit::PlaceInstance { component, at: Some((x + 0.4, y + 0.4)) }, &[]));
            }
            for (i, &instance) in placed.iter().enumerate().filter(|(i, _)| i % 3 == 1) {
                let fill = COLOURS[i % COLOURS.len()];
                self.apply(&Edit::Override { part, change: crate::components::PartChange::Fill(fill) }, &[instance]);
            }
        }
    }

    /// Traced Print Then Cut images: each cuts along the silhouette of the disc.
    fn bitmaps(&mut self, n: usize, image: &RgbaImage, asset: &AssetId) {
        let clip = crate::trace::silhouette(image, &Default::default(), &crate::trace::Cancel::new(Default::default(), 0)).expect("never cancelled");
        let px = (image.width(), image.height());
        for _ in 0..n {
            let (x_in, y_in) = self.cell();
            let shape = Shape {
                x_in,
                y_in,
                width_in: 0.8,
                height_in: 0.8,
                operation: Operation::PrintThenCut,
                ..Shape::new(0, ShapeKind::Bitmap { asset: asset.clone(), px, clip: clip.clone() })
            };
            self.apply(&Edit::Place(Placement { shapes: vec![shape], group: false }), &[]);
        }
    }

    fn patterns(&mut self, n: usize, image: &RgbaImage, asset: &AssetId) {
        for &fill in COLOURS.iter().cycle().take(n) {
            let at = self.cell();
            let id = self.push(ShapeKind::RoundedSquare, at, (0.8, 0.8), fill);
            self.shape(id).operation = Operation::PrintThenCut;
            let pattern = PatternFill { scale: 0.3, ..PatternFill::new(asset.clone(), image.width() as f32 / image.height() as f32, PatternMode::Tile) };
            self.apply(&Edit::Pattern(Some(pattern)), &[id]);
        }
    }
}

/// The heavy design for `seed`: every feature in spec 38's scenario, each count scaled in proportion to
/// `objects` (the counts at [`DEFAULT_OBJECTS`] are the spec's), then plain library shapes until it holds
/// `objects` top-level objects. The same seed always builds an equal design.
pub fn build(seed: u64, objects: usize) -> Design {
    let image = image();
    let asset = assets::id_of(&image);
    let mut b = Builder { design: Design::default(), rng: Rng(seed), cell: 0 };
    let scaled = |base: usize| (base * objects + DEFAULT_OBJECTS / 2) / DEFAULT_OBJECTS;
    b.library_shapes(scaled(60));
    b.texts(scaled(20));
    b.links(scaled(10), scaled(6));
    b.containers(scaled(4));
    b.effects(scaled(15));
    b.booleans(scaled(6));
    b.components(scaled(2), 8, 12);
    b.bitmaps(scaled(2), &image, &asset);
    b.patterns(scaled(2), &image, &asset);
    let missing = objects.saturating_sub(counts(&b.design).objects);
    b.library_shapes(missing);
    b.design
}

/// What the scenario holds, as the canvas and the Layers panel count it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Counts {
    /// Top-level objects: a lone shape, a whole group or attach set, a container or a live group.
    pub objects: usize,
    /// Layers panel rows: every shape, every group or attach header, and every part of every instance.
    pub layers: usize,
}

pub fn counts(design: &Design) -> Counts {
    let objects = units_by(design, design.shapes.iter().filter(|s| s.parent.is_none()), UnitKey::AnyLink).len();
    let mut links: Vec<u64> = design.shapes.iter().filter_map(|s| s.link.map(|l| l.id)).collect();
    links.sort_unstable();
    links.dedup();
    let parts: usize = design.shapes.iter().filter_map(Shape::instance_of).map(|c| design.components.iter().find(|m| m.id == c).map_or(0, |m| m.shapes.len())).sum();
    Counts { objects, layers: design.shapes.len() + links.len() + parts }
}

/// Evaluates every shape's geometry, as the first frame that draws them all does.
pub fn evaluate(design: &Design) {
    for s in &design.shapes {
        design.geometry(s);
    }
}

/// The mat Kerf's canvas starts on, which Prepare for Design Space sizes its files for.
fn mat() -> MatSize {
    MachineFamily::StudioPro.preferred_mat()
}

/// The whole design as one filled SVG in inches.
pub fn svg(design: &Design, image: &RgbaImage) -> Vec<u8> {
    let request = Request {
        format: Format::Svg,
        scope: Scope::Design,
        style: Style::Filled,
        units: export::DESIGN_SPACE_UNITS,
        mirror: false,
        raster: export::raster::RasterOptions::default(),
    };
    let file = export::plan(design, &request, TITLE, mat()).into_iter().next().expect("a design with shapes makes one SVG");
    file.bytes(&pixels(image))
}

fn pixels(image: &RgbaImage) -> assets::Pixels {
    [(assets::id_of(image), std::sync::Arc::new(image.clone()))].into()
}

/// Where [`write`] put each artefact.
pub struct Written {
    pub project: PathBuf,
    pub svg: PathBuf,
    pub mats: Vec<PathBuf>,
}

/// Writes the Kerf project, the whole-design SVG and the Prepare for Design Space files into `dir`.
pub fn write(dir: &Path, design: &Design) -> io::Result<Written> {
    let image = image();
    std::fs::create_dir_all(dir.join(MATS_DIR))?;
    let project = crate::project::Project { id: TITLE.into(), name: "Heavy benchmark".into(), design: design.clone(), created: 0, modified: 0 };
    let written = Written { project: dir.join(PROJECT_FILE), svg: dir.join(SVG_FILE), mats: Vec::new() };
    std::fs::write(&written.project, crate::project::to_json(&project)?)?;
    std::fs::write(&written.svg, svg(design, &image))?;
    let pixels = pixels(&image);
    let mats = export::design_space(design, TITLE, mat())
        .into_iter()
        .map(|file| {
            let path = dir.join(MATS_DIR).join(&file.name);
            std::fs::write(&path, file.bytes(&pixels)).map(|()| path)
        })
        .collect::<io::Result<_>>()?;
    Ok(Written { mats, ..written })
}

/// Puts the image in the asset store under `projects_dir`, so the canvas can draw the bitmaps and patterns.
pub fn install_image(projects_dir: &Path) -> io::Result<AssetId> {
    assets::store(&assets::dir(projects_dir), &image())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heavy() -> Design {
        crate::system_fonts::FontCatalogue::fixture().make_current();
        build(DEFAULT_SEED, DEFAULT_OBJECTS)
    }

    #[test]
    fn the_scenario_holds_150_objects_and_400_layers_and_the_same_seed_builds_the_same_design() {
        let design = heavy();
        let c = counts(&design);
        assert!(c.objects >= 150, "{c:?}");
        assert!(c.layers >= 400, "{c:?}");
        assert_eq!(design, heavy());
        assert_ne!(design, build(DEFAULT_SEED + 1, DEFAULT_OBJECTS), "the seed changes the design");
        assert_eq!(design.check(&Edit::SelectAll, &[]), Ok(()));
    }

    #[test]
    fn the_scenario_scales_with_its_size_and_keeps_every_feature() {
        crate::system_fonts::FontCatalogue::fixture().make_current();
        let mut last = Counts { objects: 0, layers: 0 };
        for size in [150, 500, 1000, 2000] {
            let d = build(DEFAULT_SEED, size);
            let c = counts(&d);
            assert!(c.objects >= size && c.objects <= size + size / 50, "{size}: {c:?}");
            assert!(c.layers > last.layers, "{size}: {c:?} after {last:?}");
            last = c;
            let scaled = |base: usize| (base * size + 75) / 150;
            let n = |f: &dyn Fn(&Shape) -> bool| d.shapes.iter().filter(|s| f(s)).count();
            assert_eq!(n(&|s| matches!(s.kind, ShapeKind::Text { .. })), scaled(20), "{size}: texts");
            let links = |kind| {
                let mut ids: Vec<u64> = d.shapes.iter().filter_map(|s| s.link).filter(|l| l.kind == kind).map(|l| l.id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len()
            };
            assert_eq!((links(crate::links::LinkKind::Group), links(crate::links::LinkKind::Attach)), (scaled(10), scaled(6)), "{size}: links");
            assert_eq!(n(&|s| s.container.is_some()), scaled(4), "{size}: containers");
            assert_eq!(d.components.len(), scaled(2), "{size}: components");
            let booleans = |combine| n(&|s| s.kind == ShapeKind::Compound(crate::design::CompoundOp::Boolean(combine)));
            assert_eq!(booleans(Combine::Weld) + booleans(Combine::Subtract) + booleans(Combine::Slice), scaled(6), "{size}: booleans");
            assert!(n(&|s| s.effects.warp().is_some()) > 0 && n(&|s| s.effects.entries().iter().any(|e| matches!(e.effect, Effect::Sticker(_)))) > 0, "{size}: effects");
            assert_eq!(n(&|s| matches!(&s.kind, ShapeKind::Bitmap { clip, .. } if !clip.is_empty())), scaled(2), "{size}: bitmaps");
            assert_eq!(n(&|s| s.pattern.is_some()), scaled(2), "{size}: patterns");
            assert!(n(&|s| matches!(&s.kind, ShapeKind::Instance { overrides, .. } if !overrides.is_empty())) > 0, "{size}: overrides");
            for op in OPERATIONS {
                assert!(n(&|s| s.parent.is_none() && s.operation == op && !matches!(s.kind, ShapeKind::Text { .. })) > 0, "{size}: {op:?}");
            }
            assert_eq!(d.check(&Edit::SelectAll, &[]), Ok(()));
        }
    }

    #[test]
    fn every_feature_of_the_scenario_is_present() {
        let d = heavy();
        let count = |f: &dyn Fn(&Shape) -> bool| d.shapes.iter().filter(|s| f(s)).count();
        for op in OPERATIONS {
            assert!(count(&|s| s.parent.is_none() && s.operation == op && !matches!(s.kind, ShapeKind::Text { .. })) > 0, "{op:?}");
        }
        assert!(count(&|s| s.rotation_deg != 0.0) > 0 && count(&|s| s.flip_x) > 0, "rotated and mirrored shapes");
        let text = |f: &dyn Fn(&FontRef, &str) -> bool| count(&|s| matches!(&s.kind, ShapeKind::Text { font, content, .. } if f(font, content)));
        assert_eq!(text(&|_, _| true), 20);
        assert!(text(&|f, _| matches!(f, FontRef::Stroke(_))) > 0, "a writing font");
        assert!(text(&|f, _| matches!(f, FontRef::System(_))) > 0, "a system font");
        assert!(text(&|_, c| c.lines().count() == 3) == 1, "one multi-line text");
        assert!(count(&|s| s.kind.active_curve().is_some()) >= 3, "curved text");
        let links = |kind| {
            let mut ids: Vec<u64> = d.shapes.iter().filter_map(|s| s.link).filter(|l| l.kind == kind).map(|l| l.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len()
        };
        assert_eq!((links(crate::links::LinkKind::Group), links(crate::links::LinkKind::Attach)), (10, 6));
        let containers: Vec<&Shape> = d.shapes.iter().filter(|s| s.container.is_some()).collect();
        assert_eq!(containers.len(), 4);
        assert!(containers.iter().all(|c| d.children(Some(c.id)).count() == 5));
        assert_eq!(containers.iter().filter(|c| c.container.is_some_and(|r| r.layout.is_some())).count(), 2);
        let effect = |f: fn(&Effect) -> bool| count(&|s| s.effects.entries().iter().any(|e| f(&e.effect)));
        assert!(effect(|e| matches!(e, Effect::Offset(o) if o.direction == Direction::Outset)) > 0);
        assert!(effect(|e| matches!(e, Effect::Offset(o) if o.direction == Direction::Inset)) > 0);
        assert!(effect(|e| matches!(e, Effect::Sticker(_))) > 0);
        assert!(count(&|s| s.effects.warp().is_some()) > 0, "a warp");
        for op in [Combine::Weld, Combine::Subtract, Combine::Slice] {
            assert_eq!(count(&|s| s.kind == ShapeKind::Compound(crate::design::CompoundOp::Boolean(op))), 2, "{op:?}");
        }
        assert_eq!(d.components.len(), 2);
        for c in &d.components {
            assert_eq!(count(&|s| s.instance_of() == Some(c.id)), 8);
        }
        assert!(count(&|s| matches!(&s.kind, ShapeKind::Instance { overrides, .. } if !overrides.is_empty())) > 0, "overrides");
        assert_eq!(count(&|s| matches!(&s.kind, ShapeKind::Bitmap { clip, .. } if !clip.is_empty()) && s.operation == Operation::PrintThenCut), 2);
        assert_eq!(count(&|s| s.pattern.is_some()), 2);
    }

    #[test]
    fn the_scenario_round_trips_through_save_and_load_and_its_svg_reimports_path_for_path() {
        let design = heavy();
        let dir = tempfile::tempdir().unwrap();
        let written = write(dir.path(), &design).unwrap();
        let (project, repairs) = crate::project::parse(&std::fs::read_to_string(&written.project).unwrap()).unwrap();
        assert!(repairs.is_empty(), "{repairs}");
        assert!(project.design.eq_closed(&design), "the design loads back unchanged");
        assert!(!written.mats.is_empty() && written.mats.iter().all(|p| p.exists()));
        let svg = std::fs::read(&written.svg).unwrap();
        let text = String::from_utf8_lossy(&svg);
        let paths = text.matches("<path").count() - text.matches("<clipPath").count();
        assert!(paths > 150, "{paths} paths");
        assert_eq!(crate::shape_library::import_svg(&svg).unwrap().len(), paths);
    }
}

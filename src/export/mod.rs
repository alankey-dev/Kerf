//! Export (spec 25). One intermediate shape, `Sheet`, feeds the SVG, DXF and PDF writers, so names, colours
//! and geometry cannot drift between formats. `sheets` decides everything: which shapes leave, their layer
//! and group names, colours and placement. Each writer only turns a `Sheet` into bytes in its own space.
//! `plan` names the files a request writes; the dialog lists them and the app writes their bytes.

pub mod biarc;
pub mod dxf;
pub mod pdf;
pub mod picture;
pub mod raster;
pub mod svg;

use std::collections::HashMap;

use egui::Color32;

use crate::assets::{AssetId, Pixels};
use crate::bridge;
use crate::composition::{self, Mat};
use crate::design::{Contour, Design, Operation, POINTS_PER_INCH, Pt, Seg, Shape};
use crate::effects::{self, Geometry, Ink, Paint, image_corners};
use crate::links::LinkKind;
use crate::machine::{LoadType, MatSize};
use crate::make_flow::MatSetup;
use crate::pens::{Pen, PenSize};
use crate::snap::Aabb;
use crate::units::Units;
pub use picture::Picture;

/// What exported files name as their maker.
pub const APP_NAME: &str = "Kerf";

/// Design Space's upload sends the SVG unchanged to a server converter, so the only safe size is one a
/// unit-honouring reader and a 72 dpi reader agree on: `width`/`height` in inches over a viewBox in points.
/// See `docs/evidence-webapp.md`, "SVG upload".
pub const DESIGN_SPACE_UNITS: Units = Units::Imperial;

pub const DESIGN_SPACE_GUIDE: &str = "README - Design Space.txt";

/// The margin a whole-design sheet keeps around the design's bounds.
const DESIGN_PAD_IN: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Design,
    PerMat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Svg,
    Dxf,
    Pdf,
    Png,
    Jpeg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Dxf => "dxf",
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    /// Whether the format rasterises the design rather than writing vectors.
    pub fn is_raster(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg)
    }
}

/// Filled suits Design Space; Outlines (hairline strokes) suits laser software.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Filled,
    Outlines,
}

/// One file, or one PDF page.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// "Mat 1 - Black - Basic Cut".
    pub name: String,
    pub size_in: Pt,
    pub layers: Vec<Layer>,
}

/// One (operation, shape) pair, in z order. Layers sharing a group prefix are adjacent, so a writer can
/// open and close groups as the prefix changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// "Score - Heart", unique within the sheet.
    pub name: String,
    /// The enclosing group names, outermost first: containers, then an "Attached - Square" link.
    pub group: Vec<String>,
    pub operation: Operation,
    pub colour: Color32,
    /// Inches on the sheet, origin top left, Y down.
    pub contours: Vec<Contour>,
    /// An image's pixels, clipped to `contours`, in place of the colour when Filled.
    pub picture: Option<Picture>,
}

impl Layer {
    /// Operations that draw or press a line rather than cut out an area; Filled style strokes them.
    pub fn draws_lines(&self) -> bool {
        matches!(self.operation, Operation::Pen | Operation::Score | Operation::Deboss | Operation::Engrave | Operation::Foil(_))
    }

    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.colour.r(), self.colour.g(), self.colour.b())
    }
}

/// The pieces of `s` that leave the canvas. Hidden shapes, hidden contours and Guide pieces stay, as
/// `compose` leaves them out.
fn exported<'g>(s: &Shape, geometry: &'g Geometry) -> impl Iterator<Item = &'g effects::Piece> {
    geometry.pieces().iter().filter(move |p| !s.hidden && p.style.operation != Operation::Guide)
}

/// Whether an export would write anything.
pub fn has_output(design: &Design) -> bool {
    design.drawn().any(|s| exported(s, &design.geometry(s)).next().is_some())
}

pub fn sheets(design: &Design, scope: Scope, setup: &MatSetup) -> Vec<Sheet> {
    match scope {
        Scope::Design => design_sheet(design, setup.mirror).into_iter().collect(),
        Scope::PerMat => composition::compose(design, setup.mat_size, setup.copies).iter().enumerate().map(|(i, mat)| mat_sheet(design, i, mat, setup)).collect(),
    }
}

struct Piece<'a> {
    shape: &'a Shape,
    copy: u32,
    style: effects::Style,
    contours: Vec<Contour>,
    picture: Option<Picture>,
}

/// The one image a piece paints, carried from its shape's local frame through `place`: a Bitmap's box, or
/// a pattern fill or flattened image whose print is a single image. A tile repeats across the box around
/// `contours`, the piece's outline already placed.
fn picture_of(shape: &Shape, piece: &effects::Piece, place: impl Fn(Pt) -> Pt, contours: &[Contour]) -> Option<Picture> {
    match &piece.paint {
        Paint::Image(asset) => Some(Picture::once(asset.clone(), image_corners((shape.width_in, shape.height_in), place), 0.0)),
        Paint::Print(layers) => {
            let [(_, Ink::Image { asset, corners, repeat })] = &layers[..] else { return None };
            let local = effects::Placement::of(shape);
            let corners = corners.map(|p| place(local.to_local(p)));
            match repeat {
                true => Picture::tiled(asset.clone(), corners, contours),
                false => Some(Picture::once(asset.clone(), corners, 0.0)),
            }
        }
        Paint::Fill => None,
    }
}

pub(crate) fn map_contour(c: &Contour, place: impl Fn(Pt) -> Pt) -> Contour {
    let segs = c
        .segs
        .iter()
        .map(|s| match *s {
            Seg::Line(p) => Seg::Line(place(p)),
            Seg::Cubic(a, b, p) => Seg::Cubic(place(a), place(b), place(p)),
        })
        .collect();
    Contour { start: place(c.start), segs, closed: c.closed }
}

fn design_sheet(design: &Design, mirror: bool) -> Option<Sheet> {
    let mut shapes: Vec<(&Shape, effects::Style, Vec<Contour>, Option<Picture>)> = Vec::new();
    let mut bounds = Aabb::EMPTY;
    for s in design.drawn() {
        let geometry = design.geometry(s);
        for piece in exported(s, &geometry) {
            let contours = geometry.world_contours(piece);
            let picture = picture_of(s, piece, effects::Placement::of(s).placer(), &contours);
            shapes.push((s, piece.style, contours, picture));
            bounds = bounds.union(piece.bounds);
        }
    }
    if shapes.is_empty() {
        return None;
    }
    let size_in = (bounds.width() + 2.0 * DESIGN_PAD_IN, bounds.height() + 2.0 * DESIGN_PAD_IN);
    let place = |(x, y): Pt| {
        let x = x - bounds.min_x + DESIGN_PAD_IN;
        (if mirror { size_in.0 - x } else { x }, y - bounds.min_y + DESIGN_PAD_IN)
    };
    let pieces = shapes
        .into_iter()
        .map(|(shape, style, contours, picture)| Piece {
            shape,
            copy: 0,
            style,
            contours: contours.iter().map(|c| map_contour(c, place)).collect(),
            picture: picture.map(|p| p.mapped(place)),
        })
        .collect();
    Some(Sheet { name: "Design".into(), size_in, layers: layers(design, pieces) })
}

fn mat_sheet(design: &Design, index: usize, mat: &Mat, setup: &MatSetup) -> Sheet {
    let z = |id: u64| design.shapes.iter().position(|s| s.id == id);
    let mut placements = bridge::mat_placements(design, mat, setup);
    placements.sort_by_key(|p| (p.copy, z(p.piece().id.owner)));
    let pieces = placements
        .iter()
        .map(|p| {
            let place = |(x, y): Pt| {
                let (x, y) = p.transform.apply((x * POINTS_PER_INCH, y * POINTS_PER_INCH));
                (x / POINTS_PER_INCH, y / POINTS_PER_INCH)
            };
            let contours: Vec<Contour> = p.piece().contours.iter().map(|c| map_contour(c, place)).collect();
            let picture = picture_of(p.shape, p.piece(), place, &contours);
            Piece { shape: p.shape, copy: p.copy, style: p.piece().style, contours, picture }
        })
        .collect();
    Sheet { name: mat_name(index, mat), size_in: (setup.mat_size.width_in, setup.mat_size.height_in), layers: layers(design, pieces) }
}

/// "Mat 1 - Black - Score, Basic Cut": the mat's number, its material colour named from the pen palette,
/// and its operations in plot order.
fn mat_name(index: usize, mat: &Mat) -> String {
    format!("Mat {} - {} - {}", index + 1, Pen::nearest(mat.colour, PenSize::Fine).colour_name(), mat.label())
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GroupKey {
    Container(u64, u32),
    Attach(u64, u32),
}

/// Hands out names unique within one sheet: a repeat gets " 2", " 3".
#[derive(Default)]
struct Names(HashMap<String, u32>);

impl Names {
    fn unique(&mut self, base: String) -> String {
        let n = self.0.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 { base } else { format!("{base} {n}") }
    }
}

fn layers(design: &Design, pieces: Vec<Piece>) -> Vec<Layer> {
    let mut names = Names::default();
    let mut groups: HashMap<GroupKey, String> = HashMap::new();
    let mut out = Vec::new();
    for p in pieces.into_iter().filter(|p| !p.contours.is_empty()) {
        let mut group = Vec::new();
        for (key, label) in group_path(design, p.shape, p.copy) {
            group.push(groups.entry(key).or_insert_with(|| names.unique(label)).clone());
        }
        let style = p.style;
        let colour = if style.operation == Operation::Pen { style.pen.rgb() } else { style.fill };
        let name = names.unique(format!("{} - {}", style.operation.label(), design.layer_name(p.shape)));
        out.push(Layer { name, group, operation: style.operation, colour, contours: p.contours, picture: p.picture });
    }
    cluster(out, 0)
}

/// The groups `s` sits in, outermost first: each container ancestor, then its Attach link.
fn group_path(design: &Design, s: &Shape, copy: u32) -> Vec<(GroupKey, String)> {
    let find = |id: u64| design.shapes.iter().find(|c| c.id == id);
    let mut path = Vec::new();
    let mut at = s.parent.and_then(find);
    while let Some(c) = at {
        path.push((GroupKey::Container(c.id, copy), design.layer_name(c)));
        at = c.parent.and_then(find);
    }
    path.reverse();
    if let Some(link) = s.link.filter(|l| l.kind == LinkKind::Attach) {
        let leaving = |m: &Shape| exported(m, &design.geometry(m)).map(|p| p.style.operation).collect::<Vec<_>>();
        let members: Vec<(&Shape, Vec<Operation>)> =
            design.shapes.iter().filter(|m| m.link.is_some_and(|l| l.id == link.id)).map(|m| (m, leaving(m))).filter(|(_, ops)| !ops.is_empty()).collect();
        let cut = members.iter().find(|(_, ops)| ops.iter().any(|&op| composition::is_cut(op))).or(members.first()).map_or(s, |(m, _)| m);
        let name = design.link_names.get(&link.id).cloned().unwrap_or_else(|| format!("Attached - {}", design.layer_name(cut)));
        path.push((GroupKey::Attach(link.id, copy), name));
    }
    path
}

/// Pulls each group's later members up to its first, keeping z order otherwise, so groups are contiguous.
fn cluster(layers: Vec<Layer>, depth: usize) -> Vec<Layer> {
    let mut rest: Vec<Option<Layer>> = layers.into_iter().map(Some).collect();
    let mut out = Vec::new();
    for i in 0..rest.len() {
        let Some(layer) = rest[i].take() else { continue };
        let Some(group) = layer.group.get(depth).cloned() else {
            out.push(layer);
            continue;
        };
        let mut members = vec![layer];
        for later in &mut rest[i + 1..] {
            if later.as_ref().is_some_and(|l| l.group.get(depth) == Some(&group)) {
                members.extend(later.take());
            }
        }
        out.extend(cluster(members, depth + 1));
    }
    out
}

/// What the export dialog edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub format: Format,
    pub scope: Scope,
    pub style: Style,
    pub units: Units,
    pub mirror: bool,
    pub raster: raster::RasterOptions,
}

impl Request {
    pub fn new(units: Units) -> Self {
        Self { format: Format::Svg, scope: Scope::Design, style: Style::Filled, units, mirror: false, raster: raster::RasterOptions::default() }
    }

    /// PDF always writes one page per mat.
    pub fn scope(&self) -> Scope {
        match self.format {
            Format::Pdf => Scope::PerMat,
            _ => self.scope,
        }
    }
}

/// One file an export will write.
pub struct Planned {
    pub name: String,
    body: Body,
}

enum Body {
    Svg(Sheet, Units, Style),
    Dxf(Sheet, Units),
    Pdf(Vec<Sheet>, Style),
    Raster { sheet: Sheet, opts: raster::RasterOptions, jpeg: bool },
    Text(String),
}

impl Planned {
    pub fn pdf(name: String, sheets: Vec<Sheet>, style: Style) -> Self {
        Self { name, body: Body::Pdf(sheets, style) }
    }

    /// The file's bytes, drawing images from `pixels`.
    pub fn bytes(&self, pixels: &Pixels) -> Vec<u8> {
        match &self.body {
            Body::Svg(sheet, units, style) => svg::document(sheet, *units, *style, pixels).into_bytes(),
            Body::Dxf(sheet, units) => dxf::document(sheet, *units).into_bytes(),
            Body::Pdf(sheets, style) => {
                let mut pdf = pdf::PdfBuilder::new();
                for sheet in sheets {
                    pdf::page(&mut pdf, sheet, *style, pixels);
                }
                pdf.finish()
            }
            Body::Raster { sheet, opts, jpeg } => raster::encode(&raster::render(sheet, opts, pixels), opts, *jpeg),
            Body::Text(text) => text.clone().into_bytes(),
        }
    }

    /// The pixel size a raster file writes, for the file list and the dialog's size check.
    pub fn raster_size(&self) -> Option<(u32, u32)> {
        match &self.body {
            Body::Raster { sheet, opts, .. } => Some(raster::dimensions(sheet.size_in, opts)),
            _ => None,
        }
    }

    /// A message when a raster file would be over the megapixel limit, else `None`.
    pub fn oversize(&self) -> Option<String> {
        match &self.body {
            Body::Raster { sheet, opts, .. } => raster::oversize(sheet.size_in, opts),
            _ => None,
        }
    }

    /// A rough byte estimate for the file list, for a raster file.
    pub fn estimated_bytes(&self) -> Option<u64> {
        match &self.body {
            Body::Raster { sheet, opts, jpeg } => {
                let (w, h) = raster::dimensions(sheet.size_in, opts);
                Some(raster::estimate_bytes(w, h, *jpeg, opts.quality))
            }
            _ => None,
        }
    }

    /// The images the file draws: what the caller decodes for [`Planned::bytes`]. DXF draws none, and
    /// Outlines style draws only cut lines.
    pub fn assets(&self) -> impl Iterator<Item = &AssetId> {
        let drawn = match &self.body {
            Body::Svg(_, _, Style::Filled) | Body::Pdf(_, Style::Filled) => self.sheets(),
            Body::Raster { opts, .. } if !opts.as_made => self.sheets(),
            _ => &[],
        };
        drawn.iter().flat_map(|s| &s.layers).filter_map(|l| l.picture.as_ref().map(|p| &p.asset))
    }

    /// The sheets this file holds: one, a page per mat for a PDF, none for the guide.
    pub fn sheets(&self) -> &[Sheet] {
        match &self.body {
            Body::Svg(sheet, ..) | Body::Dxf(sheet, _) | Body::Raster { sheet, .. } => std::slice::from_ref(sheet),
            Body::Pdf(sheets, _) => sheets,
            Body::Text(_) => &[],
        }
    }
}

/// The files `request` writes for `design` on mats of `mat`, named after `project`.
pub fn plan(design: &Design, request: &Request, project: &str, mat: MatSize) -> Vec<Planned> {
    let setup = MatSetup { load_type: LoadType::OnMat, mat_size: mat, mirror: request.mirror, copies: 1 };
    let (scope, ext) = (request.scope(), request.format.extension());
    let sheets = sheets(design, scope, &setup);
    let project = file_stem(project);
    if request.format == Format::Pdf {
        let pdf = Planned { name: format!("{project}.{ext}"), body: Body::Pdf(sheets, request.style) };
        return if pdf.sheets().is_empty() { Vec::new() } else { vec![pdf] };
    }
    let name = |sheet: &Sheet| match scope {
        Scope::Design => format!("{project}.{ext}"),
        Scope::PerMat => format!("{project} - {}.{ext}", sheet.name),
    };
    let body = |sheet: Sheet| match request.format {
        Format::Dxf => Body::Dxf(sheet, request.units),
        Format::Png => Body::Raster { sheet, opts: request.raster, jpeg: false },
        Format::Jpeg => Body::Raster { sheet, opts: request.raster, jpeg: true },
        _ => Body::Svg(sheet, request.units, request.style),
    };
    sheets.into_iter().map(|sheet| Planned { name: name(&sheet), body: body(sheet) }).collect()
}

/// The pixel size the first file a raster `request` would write, for the dialog's live readout. `None`
/// when nothing would export.
pub fn raster_size(design: &Design, request: &Request, mat: MatSize) -> Option<(u32, u32)> {
    let setup = MatSetup { load_type: LoadType::OnMat, mat_size: mat, mirror: request.mirror, copies: 1 };
    sheets(design, request.scope(), &setup).first().map(|sheet| raster::dimensions(sheet.size_in, &request.raster))
}

/// Prepare for Design Space: one filled SVG per mat, sized the way the upload reads it, and a guide to
/// setting each layer's operation back after upload.
pub fn design_space(design: &Design, project: &str, mat: MatSize) -> Vec<Planned> {
    let request =
        Request { format: Format::Svg, scope: Scope::PerMat, style: Style::Filled, units: DESIGN_SPACE_UNITS, mirror: false, raster: raster::RasterOptions::default() };
    let mut files = plan(design, &request, project, mat);
    if !files.is_empty() {
        let guide = guide(&file_stem(project), &files);
        files.push(Planned { name: DESIGN_SPACE_GUIDE.into(), body: Body::Text(guide) });
    }
    files
}

fn guide(project: &str, files: &[Planned]) -> String {
    let mut listing = String::new();
    for file in files {
        listing += &format!("{}\n", file.name);
        for layer in file.sheets().iter().flat_map(|s| &s.layers) {
            let path: String = layer.group.iter().map(|g| format!("{g} / ")).collect();
            listing += &format!("    {path}{}, {}\n", layer.name, layer.hex());
        }
        listing.push('\n');
    }
    include_str!("design_space_guide.txt").replace("{project}", project).replace("{files}", listing.trim_end())
}

/// The project title made safe as a file name.
pub(crate) fn file_stem(project: &str) -> String {
    let stem: String = project.trim().chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    if stem.is_empty() { "Untitled".into() } else { stem }
}

#[cfg(test)]
pub(crate) mod tests;

//! Self-contained clipboard packages. The UI owns clipboard I/O; this module closes, validates and
//! remaps the model fragment without touching the filesystem or platform clipboard.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::assets::{self, AssetId, Pixels};
use crate::components::Component;
use crate::design::{Design, Operation, Pt, Shape, ShapeKind};
use crate::sanitise::Repairs;
use crate::snap::Aabb;

const PACKAGE_SCHEMA: u16 = 1;
const MAX_PNG_BYTES: usize = 256 * 1024 * 1024;

/// The tab which produced a package. This identity is deliberately ephemeral.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Source {
    pub(crate) document_id: String,
    pub(crate) mat_in: Pt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Placement {
    InPlace,
    Offset,
    At(Pt),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PasteStyle {
    pub(crate) operation: Operation,
    pub(crate) fill: egui::Color32,
    pub(crate) pen: crate::pens::Pen,
    pub(crate) pattern: Option<crate::patterns::PatternFill>,
}

impl PasteStyle {
    /// The operation, fill, pen and pattern of one shape, for the eyedropper to lift (spec 56).
    pub(crate) fn from_shape(shape: &Shape) -> Self {
        Self { operation: shape.operation, fill: shape.fill, pen: shape.pen, pattern: shape.pattern.clone() }
    }
}

/// The selected roots after paste, and any repairs made only to the incoming fragment.
#[derive(Debug)]
pub(crate) struct PasteResult {
    pub(crate) roots: Vec<u64>,
    pub(crate) repairs: Repairs,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Bounds {
    min: Pt,
    max: Pt,
}

impl Bounds {
    fn from_aabb(bounds: Aabb) -> Self {
        Self { min: (bounds.min_x, bounds.min_y), max: (bounds.max_x, bounds.max_y) }
    }

    fn centre(self) -> Pt {
        ((self.min.0 + self.max.0) / 2.0, (self.min.1 + self.max.1) / 2.0)
    }

    fn is_sound(self) -> bool {
        self.min.0.is_finite() && self.min.1.is_finite() && self.max.0.is_finite() && self.max.1.is_finite() && self.min.0 <= self.max.0 && self.min.1 <= self.max.1
    }
}

/// A closed, serialisable selection fragment. Shape positions are stored relative to `origin_in`;
/// component masters remain in their own frames.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Package {
    schema: u16,
    project_format_version: u64,
    source: Source,
    origin_in: Pt,
    bounds_in: Bounds,
    roots: Vec<u64>,
    shapes: Vec<Shape>,
    components: Vec<Component>,
    link_names: BTreeMap<u64, String>,
    assets: Vec<AssetBlob>,
}

impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Package").field("shapes", &self.shapes.len()).field("components", &self.components.len()).field("assets", &self.assets.len()).finish()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetBlob {
    id: AssetId,
    #[serde(with = "base64_bytes")]
    png: Vec<u8>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ValidatedPackage {
    package: Arc<Package>,
    assets: Arc<Vec<(AssetId, Arc<image::RgbaImage>)>>,
    repairs: Repairs,
}

impl fmt::Debug for ValidatedPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedPackage").field("package", &self.package).field("assets", &self.assets.len()).field("repairs", &self.repairs).finish()
    }
}

mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        base64::engine::general_purpose::STANDARD.encode(bytes).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let max_encoded = super::MAX_PNG_BYTES.div_ceil(3) * 4;
        if encoded.len() > max_encoded {
            return Err(serde::de::Error::custom("clipboard PNG is oversized"));
        }
        base64::engine::general_purpose::STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ClipboardError {
    EmptySelection,
    MissingShape(u64),
    MissingComponent(u64),
    MissingAsset(AssetId),
    InvalidAsset { id: AssetId, reason: String },
    InvalidPackage(&'static str),
    DuplicateId(u64),
    ComponentCycle,
    IdExhausted,
    InvalidPlacement,
    ImageEncode(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => f.write_str("cannot capture an empty selection"),
            Self::MissingShape(id) => write!(f, "shape {id} is missing"),
            Self::MissingComponent(id) => write!(f, "component {id} is missing"),
            Self::MissingAsset(id) => write!(f, "asset {id} is missing"),
            Self::InvalidAsset { id, reason } => write!(f, "asset {id} is invalid: {reason}"),
            Self::InvalidPackage(reason) => write!(f, "clipboard package is invalid: {reason}"),
            Self::DuplicateId(id) => write!(f, "id {id} is duplicated or ambiguous"),
            Self::ComponentCycle => f.write_str("clipboard components contain a cycle"),
            Self::IdExhausted => f.write_str("the design has no unused ids left"),
            Self::InvalidPlacement => f.write_str("clipboard placement is not finite"),
            Self::ImageEncode(reason) => write!(f, "could not encode clipboard image: {reason}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

impl Package {
    /// Captures selected shapes and their descendants in source z-order, plus every recursively used
    /// component and asset. Linked siblings outside that closure are deliberately not pulled in.
    pub(crate) fn capture(design: &Design, selected: &[u64], source: Source, pixels: &Pixels) -> Result<Self, ClipboardError> {
        if selected.is_empty() {
            return Err(ClipboardError::EmptySelection);
        }
        validate_source_namespace(design)?;
        validate_source_metadata(&source)?;

        let mut selected_set = HashSet::new();
        for id in selected {
            if !selected_set.insert(*id) {
                return Err(ClipboardError::DuplicateId(*id));
            }
            if !design.shapes.iter().any(|shape| shape.id == *id) {
                return Err(ClipboardError::MissingShape(*id));
            }
        }

        let active_by_id: HashMap<u64, &Shape> = design.shapes.iter().map(|shape| (shape.id, shape)).collect();
        let mut closure = selected_set.clone();
        loop {
            let new_children: Vec<u64> = design
                .shapes
                .iter()
                .filter(|shape| !closure.contains(&shape.id) && shape.parent.is_some_and(|parent| closure.contains(&parent)))
                .map(|shape| shape.id)
                .collect();
            if new_children.is_empty() {
                break;
            }
            closure.extend(new_children);
        }

        let mut shapes: Vec<Shape> = design.shapes.iter().filter(|shape| closure.contains(&shape.id)).cloned().collect();
        for shape in &shapes {
            validate_parent_chain(shape, &active_by_id)?;
        }
        for shape in &mut shapes {
            if shape.parent.is_some_and(|parent| !closure.contains(&parent)) {
                shape.parent = None;
            }
        }
        let roots: Vec<u64> =
            shapes.iter().filter(|shape| selected_set.contains(&shape.id) && !has_selected_ancestor(shape, &selected_set, &active_by_id)).map(|shape| shape.id).collect();
        if roots.is_empty() {
            return Err(ClipboardError::InvalidPackage("selection has no roots"));
        }

        let components = capture_components(design, &shapes)?;
        let mut captured_shape_ids: HashSet<u64> = shapes.iter().map(|shape| shape.id).collect();
        for component in &components {
            captured_shape_ids.extend(component.shapes.iter().map(|shape| shape.id));
        }
        close_links(design, &mut shapes, &captured_shape_ids);
        let mut components = components;
        close_component_links(design, &mut components, &captured_shape_ids);

        let captured_link_ids: HashSet<u64> =
            shapes.iter().chain(components.iter().flat_map(|component| &component.shapes)).filter_map(|shape| shape.link.map(|link| link.id)).collect();
        let link_names = design.link_names.iter().filter(|(id, _)| captured_link_ids.contains(id)).map(|(id, name)| (*id, name.clone())).collect();

        let bounds = capture_bounds(design, &shapes)?;
        let origin_in = bounds.min;
        for shape in &mut shapes {
            shape.x_in -= origin_in.0;
            shape.y_in -= origin_in.1;
        }

        let asset_ids = referenced_assets(&shapes, &components);
        let mut assets = Vec::with_capacity(asset_ids.len());
        for id in asset_ids {
            let image = pixels.get(&id).ok_or_else(|| ClipboardError::MissingAsset(id.clone()))?;
            if assets::id_of(image) != id {
                return Err(ClipboardError::InvalidAsset { id, reason: "decoded pixels do not match their content id".into() });
            }
            let mut png = Vec::new();
            image.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).map_err(|error| ClipboardError::ImageEncode(error.to_string()))?;
            if png.len() > MAX_PNG_BYTES {
                return Err(ClipboardError::InvalidAsset { id, reason: "PNG exceeds the clipboard asset limit".into() });
            }
            assets.push(AssetBlob { id, png });
        }

        let package = Self {
            schema: PACKAGE_SCHEMA,
            project_format_version: crate::project::FORMAT_VERSION,
            source,
            origin_in,
            bounds_in: bounds,
            roots,
            shapes,
            components,
            link_names,
            assets,
        };
        package.validate_structure()?;
        Ok(package)
    }

    /// Validates a wire package and keeps its decoded assets for the paste operation.
    pub(crate) fn validate(self) -> Result<ValidatedPackage, ClipboardError> {
        self.validate_structure()?;
        let assets = self.decode_assets_validated()?.into_iter().map(|(id, image)| (id, Arc::new(image))).collect();
        let repairs = self.repairs_validated();
        Ok(ValidatedPackage { package: Arc::new(self), assets: Arc::new(assets), repairs })
    }

    /// Validates and decodes embedded PNGs for SVG export or package preparation.
    pub(crate) fn decode_assets(&self) -> Result<Vec<(AssetId, image::RgbaImage)>, ClipboardError> {
        self.validate_structure()?;
        self.decode_assets_validated()
    }

    fn decode_assets_validated(&self) -> Result<Vec<(AssetId, image::RgbaImage)>, ClipboardError> {
        let required: HashSet<AssetId> = referenced_assets(&self.shapes, &self.components).into_iter().collect();
        let mut found = HashSet::new();
        let mut decoded = Vec::with_capacity(self.assets.len());
        for blob in &self.assets {
            if !found.insert(blob.id.clone()) {
                return Err(ClipboardError::InvalidPackage("an asset id appears more than once"));
            }
            if blob.png.is_empty() || blob.png.len() > MAX_PNG_BYTES {
                return Err(ClipboardError::InvalidAsset { id: blob.id.clone(), reason: "PNG is empty or oversized".into() });
            }
            if !blob.png.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(ClipboardError::InvalidAsset { id: blob.id.clone(), reason: "asset data is not a PNG".into() });
            }
            let image = assets::decode(&blob.png).map_err(|error| ClipboardError::InvalidAsset { id: blob.id.clone(), reason: error.to_string() })?;
            if assets::id_of(&image) != blob.id {
                return Err(ClipboardError::InvalidAsset { id: blob.id.clone(), reason: "PNG pixels do not match their content id".into() });
            }
            decoded.push((blob.id.clone(), image));
        }
        if found != required {
            if let Some(missing) = required.difference(&found).next() {
                return Err(ClipboardError::MissingAsset(missing.clone()));
            }
            return Err(ClipboardError::InvalidPackage("package includes an unreferenced asset"));
        }
        Ok(decoded)
    }

    pub(crate) fn paste_style(&self) -> Result<PasteStyle, ClipboardError> {
        let first = self.shapes.first().ok_or(ClipboardError::InvalidPackage("package has no shapes"))?;
        Ok(PasteStyle { operation: first.operation, fill: first.fill, pen: first.pen, pattern: first.pattern.clone() })
    }

    fn repairs_validated(&self) -> Repairs {
        let mut incoming = Design { shapes: self.shapes.clone(), components: self.components.clone(), link_names: self.link_names.clone(), ..Design::default() };
        let repairs = incoming.sanitise();
        cleanup_sanitised_fragment(&mut incoming);
        repairs
    }

    /// The source tab and mat size, used by the UI to choose a default placement.
    pub(crate) fn source(&self) -> &Source {
        &self.source
    }

    pub(crate) fn project_format_version(&self) -> u64 {
        self.project_format_version
    }

    /// Rebuilds the captured source design for its editable SVG fallback.
    pub(crate) fn export_design(&self) -> Design {
        let mut shapes = self.shapes.clone();
        for shape in &mut shapes {
            shape.x_in += self.origin_in.0;
            shape.y_in += self.origin_in.1;
        }
        Design { shapes, components: self.components.clone(), link_names: self.link_names.clone(), ..Design::default() }
    }

    /// Stages the incoming fragment, remaps every live id and commits only after validation, sanitising,
    /// component reuse and container dropping have all succeeded.
    fn place(&self, design: &mut Design, placement: Placement, drop_at: Pt) -> Result<PasteResult, ClipboardError> {
        if !drop_at.0.is_finite() || !drop_at.1.is_finite() {
            return Err(ClipboardError::InvalidPlacement);
        }
        if let Placement::At(point) = placement
            && (!point.0.is_finite() || !point.1.is_finite())
        {
            return Err(ClipboardError::InvalidPlacement);
        }

        let mut incoming = Design { shapes: self.shapes.clone(), components: self.components.clone(), link_names: self.link_names.clone(), ..Design::default() };
        let mut repairs = incoming.sanitise();
        cleanup_sanitised_fragment(&mut incoming);

        let translation = match placement {
            Placement::InPlace => self.origin_in,
            Placement::Offset => (self.origin_in.0 + 0.25, self.origin_in.1 + 0.25),
            Placement::At(point) => {
                if incoming.shapes.is_empty() {
                    (0.0, 0.0)
                } else {
                    let bounds = design_bounds(&incoming, &incoming.shapes)?;
                    let centre = bounds.centre();
                    (point.0 - centre.0, point.1 - centre.1)
                }
            }
        };
        for shape in &mut incoming.shapes {
            shape.x_in += translation.0;
            shape.y_in += translation.1;
        }
        merge_repairs(&mut repairs, incoming.sanitise());
        cleanup_sanitised_fragment(&mut incoming);

        validate_live_namespace(design)?;

        let mut staged = design.clone();
        let source_component_order = component_order(&incoming.components)?;
        let source_fingerprints = canonical_fingerprints(&incoming.components, &incoming.link_names, None)?;
        if source_fingerprints.len() != incoming.components.len() {
            return Err(ClipboardError::InvalidPackage("component closure cannot be fingerprinted"));
        }
        let destination_fingerprints = canonical_fingerprints(&staged.components, &staged.link_names, staged.editing.map(|id| (id, staged.shapes.as_slice())))?;

        let mut allocator = IdAllocator::for_design(&staged)?;
        let mut maps = IdMaps::default();
        let mut available: Vec<(u64, [u8; 32], CanonicalComponent)> = Vec::new();
        for (id, canonical) in destination_fingerprints {
            if let Ok(hash) = canonical_hash(&canonical) {
                available.push((id, hash, canonical));
            }
        }
        let source_component_by_id: HashMap<u64, &Component> = incoming.components.iter().map(|component| (component.id, component)).collect();
        let destination_component_by_id: HashMap<u64, &Component> = staged.components.iter().map(|component| (component.id, component)).collect();
        let mut additions: Vec<Component> = Vec::new();

        for source_id in source_component_order {
            let source_component = source_component_by_id.get(&source_id).ok_or(ClipboardError::MissingComponent(source_id))?;
            let canonical = source_fingerprints.get(&source_id).ok_or(ClipboardError::MissingComponent(source_id))?;
            let hash = canonical_hash(canonical)?;
            let existing = available.iter().find_map(|(id, candidate_hash, candidate)| (*candidate_hash == hash && candidate == canonical).then_some(*id));
            if let Some(destination_id) = existing {
                let destination_component = destination_component_by_id
                    .get(&destination_id)
                    .copied()
                    .or_else(|| additions.iter().find(|component| component.id == destination_id))
                    .ok_or(ClipboardError::MissingComponent(destination_id))?;
                maps.components.insert(source_id, destination_id);
                let destination_shapes = if staged.editing == Some(destination_id) { staged.shapes.as_slice() } else { destination_component.shapes.as_slice() };
                map_corresponding_shapes(&source_component.shapes, destination_shapes, &mut maps)?;
                continue;
            }

            let destination_id = allocator.allocate()?;
            maps.components.insert(source_id, destination_id);
            for shape in &source_component.shapes {
                maps.shapes.insert(shape.id, allocator.allocate()?);
            }
            allocate_links(&source_component.shapes, &mut maps, &mut allocator)?;
            let mut component = (**source_component).clone();
            component.id = destination_id;
            component.shapes = component.shapes.iter().map(|shape| remap_shape(shape, true, &maps)).collect::<Result<_, _>>()?;
            additions.push(component);
            available.push((destination_id, hash, canonical.clone()));
        }

        for shape in &incoming.shapes {
            maps.shapes.insert(shape.id, allocator.allocate()?);
        }
        allocate_links(&incoming.shapes, &mut maps, &mut allocator)?;

        for component in additions {
            staged.components.push(component);
        }
        for (old_link, name) in &incoming.link_names {
            let new_link = maps.links.get(old_link).copied().ok_or(ClipboardError::InvalidPackage("link name has no remapped link"))?;
            if staged.link_names.get(&new_link).is_some_and(|old_name| old_name != name) {
                return Err(ClipboardError::InvalidPackage("component link names disagree after reuse"));
            }
            staged.link_names.entry(new_link).or_insert_with(|| name.clone());
        }

        let mut pasted_shapes = incoming.shapes.iter().map(|shape| remap_shape(shape, true, &maps)).collect::<Result<Vec<_>, _>>()?;
        let live_ids: HashSet<u64> = incoming.shapes.iter().map(|shape| shape.id).collect();
        let mut pasted_roots = Vec::new();
        for root in &self.roots {
            if live_ids.contains(root) {
                pasted_roots.push(*maps.shapes.get(root).ok_or(ClipboardError::MissingShape(*root))?);
            }
        }
        if staged.editing.is_some_and(|host| would_create_component_cycle(&staged, host, &pasted_shapes)) {
            return Err(ClipboardError::ComponentCycle);
        }

        staged.shapes.append(&mut pasted_shapes);
        staged.drop_at(&pasted_roots, drop_at);
        *design = staged;
        Ok(PasteResult { roots: pasted_roots, repairs })
    }

    fn validate_structure(&self) -> Result<(), ClipboardError> {
        if self.schema != PACKAGE_SCHEMA {
            return Err(ClipboardError::InvalidPackage("unsupported clipboard schema"));
        }
        validate_source_metadata(&self.source)?;
        if !self.origin_in.0.is_finite() || !self.origin_in.1.is_finite() || !self.bounds_in.is_sound() {
            return Err(ClipboardError::InvalidPackage("package origin or bounds are invalid"));
        }
        if self.shapes.is_empty() || self.roots.is_empty() {
            return Err(ClipboardError::InvalidPackage("package has no selected shapes"));
        }

        let component_by_id: HashMap<u64, &Component> = self.components.iter().map(|component| (component.id, component)).collect();
        if component_by_id.len() != self.components.len() {
            return Err(ClipboardError::InvalidPackage("component ids are duplicated"));
        }
        let shape_lists = std::iter::once(self.shapes.as_slice()).chain(self.components.iter().map(|component| component.shapes.as_slice())).collect::<Vec<_>>();
        validate_live_namespace_parts(&shape_lists, &self.components, &self.link_names)?;
        validate_shape_list(&self.shapes, &component_by_id)?;
        for component in &self.components {
            if !component.size_in.0.is_finite() || !component.size_in.1.is_finite() {
                return Err(ClipboardError::InvalidPackage("component size is not finite"));
            }
            validate_shape_list(&component.shapes, &component_by_id)?;
        }

        let roots: HashSet<u64> = self.roots.iter().copied().collect();
        if roots.len() != self.roots.len() || self.roots.iter().any(|id| !self.shapes.iter().any(|shape| shape.id == *id && shape.parent.is_none())) {
            return Err(ClipboardError::InvalidPackage("root ids are duplicated, missing or not roots"));
        }
        let top_level: HashSet<u64> = self.shapes.iter().filter(|shape| shape.parent.is_none()).map(|shape| shape.id).collect();
        if roots != top_level {
            return Err(ClipboardError::InvalidPackage("package roots do not match the closed shape forest"));
        }

        let mut reachable = HashSet::new();
        let mut visiting = HashSet::new();
        for shape in &self.shapes {
            visit_shape_components(shape, &component_by_id, &mut reachable, &mut visiting)?;
        }
        if reachable.len() != self.components.len() || self.components.iter().any(|component| !reachable.contains(&component.id)) {
            return Err(ClipboardError::InvalidPackage("package contains an unreferenced component"));
        }
        validate_links(
            &[self.shapes.as_slice()].into_iter().chain(self.components.iter().map(|component| component.shapes.as_slice())).collect::<Vec<_>>(),
            &self.link_names,
        )?;
        Ok(())
    }
}

impl ValidatedPackage {
    #[cfg(test)]
    pub(crate) fn package(&self) -> &Package {
        &self.package
    }

    pub(crate) fn assets(&self) -> &[(AssetId, Arc<image::RgbaImage>)] {
        &self.assets
    }

    pub(crate) fn repairs(&self) -> &Repairs {
        &self.repairs
    }

    pub(crate) fn source(&self) -> &Source {
        self.package.source()
    }

    pub(crate) fn paste_style(&self) -> Result<PasteStyle, ClipboardError> {
        self.package.paste_style()
    }

    pub(crate) fn place(&self, design: &mut Design, placement: Placement, drop_at: Pt) -> Result<PasteResult, ClipboardError> {
        self.package.place(design, placement, drop_at)
    }
}

#[derive(Default)]
struct IdMaps {
    shapes: HashMap<u64, u64>,
    components: HashMap<u64, u64>,
    links: HashMap<u64, u64>,
}

struct IdAllocator {
    next: u64,
}

impl IdAllocator {
    fn for_design(design: &Design) -> Result<Self, ClipboardError> {
        let used = namespace_ids(&design.shapes, &design.components, &design.link_names)?;
        let next = used.iter().copied().max().unwrap_or(0).checked_add(1).ok_or(ClipboardError::IdExhausted)?;
        Ok(Self { next })
    }

    fn allocate(&mut self) -> Result<u64, ClipboardError> {
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or(ClipboardError::IdExhausted)?;
        Ok(id)
    }
}

fn validate_source_metadata(source: &Source) -> Result<(), ClipboardError> {
    if source.document_id.is_empty() || !source.mat_in.0.is_finite() || !source.mat_in.1.is_finite() || source.mat_in.0 <= 0.0 || source.mat_in.1 <= 0.0 {
        return Err(ClipboardError::InvalidPackage("source document metadata is invalid"));
    }
    Ok(())
}

fn validate_source_namespace(design: &Design) -> Result<(), ClipboardError> {
    let shape_lists = std::iter::once(design.shapes.as_slice()).chain(design.components.iter().map(|component| component.shapes.as_slice())).collect::<Vec<_>>();
    validate_live_namespace_parts(&shape_lists, &design.components, &design.link_names)
}

fn validate_live_namespace(design: &Design) -> Result<(), ClipboardError> {
    validate_source_namespace(design)
}

fn validate_live_namespace_parts(shape_lists: &[&[Shape]], components: &[Component], link_names: &BTreeMap<u64, String>) -> Result<(), ClipboardError> {
    let mut namespace: HashMap<u64, &'static str> = HashMap::new();
    for shapes in shape_lists {
        for shape in *shapes {
            reserve_id(&mut namespace, shape.id, "shape", false)?;
            if let Some(link) = shape.link {
                reserve_id(&mut namespace, link.id, "link", true)?;
            }
        }
    }
    for component in components {
        reserve_id(&mut namespace, component.id, "component", false)?;
    }
    for id in link_names.keys() {
        reserve_id(&mut namespace, *id, "link", true)?;
    }
    Ok(())
}

fn reserve_id(namespace: &mut HashMap<u64, &'static str>, id: u64, kind: &'static str, repeated_link: bool) -> Result<(), ClipboardError> {
    match namespace.get(&id) {
        None => {
            namespace.insert(id, kind);
            Ok(())
        }
        Some(existing) if repeated_link && *existing == "link" => Ok(()),
        Some(_) => Err(ClipboardError::DuplicateId(id)),
    }
}

fn namespace_ids(shapes: &[Shape], components: &[Component], link_names: &BTreeMap<u64, String>) -> Result<HashSet<u64>, ClipboardError> {
    let lists = std::iter::once(shapes).chain(components.iter().map(|component| component.shapes.as_slice())).collect::<Vec<_>>();
    validate_live_namespace_parts(&lists, components, link_names)?;
    let mut ids: HashSet<u64> = link_names.keys().copied().collect();
    for shape_list in lists {
        for shape in shape_list {
            ids.insert(shape.id);
            if let Some(link) = shape.link {
                ids.insert(link.id);
            }
        }
    }
    ids.extend(components.iter().map(|component| component.id));
    Ok(ids)
}

fn validate_parent_chain(shape: &Shape, shapes: &HashMap<u64, &Shape>) -> Result<(), ClipboardError> {
    let mut seen = HashSet::new();
    let mut parent = shape.parent;
    while let Some(id) = parent {
        if !seen.insert(id) || id == shape.id {
            return Err(ClipboardError::InvalidPackage("shape parent chain contains a cycle"));
        }
        let Some(parent_shape) = shapes.get(&id) else { return Err(ClipboardError::InvalidPackage("shape has a dangling parent")) };
        if !parent_shape.holds_children() {
            return Err(ClipboardError::InvalidPackage("shape parent cannot hold children"));
        }
        parent = parent_shape.parent;
    }
    Ok(())
}

fn has_selected_ancestor(shape: &Shape, selected: &HashSet<u64>, shapes: &HashMap<u64, &Shape>) -> bool {
    let mut parent = shape.parent;
    while let Some(id) = parent {
        if selected.contains(&id) {
            return true;
        }
        parent = shapes.get(&id).and_then(|shape| shape.parent);
    }
    false
}

fn master_shapes(design: &Design, component_id: u64) -> Option<&[Shape]> {
    if design.editing == Some(component_id) {
        return Some(&design.shapes);
    }
    design.components.iter().find(|component| component.id == component_id).map(|component| component.shapes.as_slice())
}

fn capture_components(design: &Design, roots: &[Shape]) -> Result<Vec<Component>, ClipboardError> {
    fn visit(design: &Design, id: u64, visiting: &mut HashSet<u64>, done: &mut HashSet<u64>, out: &mut Vec<Component>) -> Result<(), ClipboardError> {
        if done.contains(&id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(ClipboardError::ComponentCycle);
        }
        let Some(component) = design.components.iter().find(|component| component.id == id) else {
            return Err(ClipboardError::MissingComponent(id));
        };
        let shapes = master_shapes(design, id).ok_or(ClipboardError::MissingComponent(id))?;
        validate_shape_list(shapes, &design.components.iter().map(|component| (component.id, component)).collect())?;
        for dependency in referenced_components(shapes) {
            visit(design, dependency, visiting, done, out)?;
        }
        visiting.remove(&id);
        done.insert(id);
        out.push(Component { id, name: component.name.clone(), shapes: shapes.to_vec(), size_in: component.size_in });
        Ok(())
    }

    let component_by_id: HashMap<u64, &Component> = design.components.iter().map(|component| (component.id, component)).collect();
    for shape in roots {
        validate_shape_payload(shape, &component_by_id)?;
    }
    let mut out = Vec::new();
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for id in referenced_components(roots) {
        visit(design, id, &mut visiting, &mut done, &mut out)?;
    }
    Ok(out)
}

fn close_links(design: &Design, shapes: &mut [Shape], included_shape_ids: &HashSet<u64>) {
    let complete = complete_links(design, included_shape_ids);
    for shape in shapes {
        if shape.link.is_some_and(|link| !complete.contains(&link.id)) {
            shape.link = None;
        }
    }
}

fn close_component_links(design: &Design, components: &mut [Component], included_shape_ids: &HashSet<u64>) {
    let complete = complete_links(design, included_shape_ids);
    for shape in components.iter_mut().flat_map(|component| &mut component.shapes) {
        if shape.link.is_some_and(|link| !complete.contains(&link.id)) {
            shape.link = None;
        }
    }
}

fn complete_links(design: &Design, included_shape_ids: &HashSet<u64>) -> HashSet<u64> {
    let mut members: HashMap<u64, Vec<u64>> = HashMap::new();
    for shape in design.shapes.iter().chain(design.components.iter().flat_map(|component| &component.shapes)) {
        if let Some(link) = shape.link {
            members.entry(link.id).or_default().push(shape.id);
        }
    }
    members.into_iter().filter_map(|(id, members)| (!members.is_empty() && members.iter().all(|member| included_shape_ids.contains(member))).then_some(id)).collect()
}

fn capture_bounds(design: &Design, shapes: &[Shape]) -> Result<Bounds, ClipboardError> {
    let mut bounds = Aabb::EMPTY;
    for shape in shapes {
        let candidate = design.geometry(shape).bounds();
        if !candidate.is_empty() {
            bounds = bounds.union(candidate);
        }
    }
    if bounds.is_empty() {
        for shape in shapes {
            let candidate = Aabb {
                min_x: shape.x_in.min(shape.x_in + shape.width_in),
                min_y: shape.y_in.min(shape.y_in + shape.height_in),
                max_x: shape.x_in.max(shape.x_in + shape.width_in),
                max_y: shape.y_in.max(shape.y_in + shape.height_in),
            };
            bounds = bounds.union(candidate);
        }
    }
    let bounds = Bounds::from_aabb(bounds);
    if !bounds.is_sound() {
        return Err(ClipboardError::InvalidPackage("selection bounds are invalid"));
    }
    Ok(bounds)
}

fn design_bounds(design: &Design, shapes: &[Shape]) -> Result<Bounds, ClipboardError> {
    let mut bounds = Aabb::EMPTY;
    for shape in shapes {
        let candidate = design.geometry(shape).bounds();
        if !candidate.is_empty() {
            bounds = bounds.union(candidate);
        }
    }
    if bounds.is_empty() {
        for shape in shapes {
            bounds = bounds.union(Aabb {
                min_x: shape.x_in.min(shape.x_in + shape.width_in),
                min_y: shape.y_in.min(shape.y_in + shape.height_in),
                max_x: shape.x_in.max(shape.x_in + shape.width_in),
                max_y: shape.y_in.max(shape.y_in + shape.height_in),
            });
        }
    }
    let bounds = Bounds::from_aabb(bounds);
    if !bounds.is_sound() {
        return Err(ClipboardError::InvalidPackage("incoming bounds are invalid"));
    }
    Ok(bounds)
}

fn referenced_assets(shapes: &[Shape], components: &[Component]) -> Vec<AssetId> {
    fn visit(shape: &Shape, out: &mut HashSet<AssetId>) {
        if let Some(pattern) = &shape.pattern {
            out.insert(pattern.asset.clone());
        }
        match &shape.kind {
            ShapeKind::Bitmap { asset, .. } => {
                out.insert(asset.clone());
            }
            ShapeKind::Flattened { parts, .. } => {
                for part in parts {
                    visit(part, out);
                }
            }
            ShapeKind::Instance { overrides, .. } => {
                out.extend(overrides.values().filter_map(|override_| override_.pattern.as_ref().and_then(Option::as_ref).map(|pattern| pattern.asset.clone())));
            }
            _ => {}
        }
    }

    let mut ids = HashSet::new();
    for shape in shapes.iter().chain(components.iter().flat_map(|component| &component.shapes)) {
        visit(shape, &mut ids);
    }
    let mut ids: Vec<AssetId> = ids.into_iter().collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

fn referenced_components(shapes: &[Shape]) -> Vec<u64> {
    fn visit(shape: &Shape, out: &mut BTreeSet<u64>) {
        match &shape.kind {
            ShapeKind::Instance { component, .. } => {
                out.insert(*component);
            }
            ShapeKind::Flattened { parts, .. } => {
                for part in parts {
                    visit(part, out);
                }
            }
            _ => {}
        }
    }
    let mut ids = BTreeSet::new();
    for shape in shapes {
        visit(shape, &mut ids);
    }
    ids.into_iter().collect()
}

fn validate_shape_list(shapes: &[Shape], components: &HashMap<u64, &Component>) -> Result<(), ClipboardError> {
    let by_id: HashMap<u64, &Shape> = shapes.iter().map(|shape| (shape.id, shape)).collect();
    if by_id.len() != shapes.len() {
        return Err(ClipboardError::InvalidPackage("shape ids are duplicated within a shape list"));
    }
    for shape in shapes {
        if let Some(parent_id) = shape.parent {
            let Some(parent) = by_id.get(&parent_id) else { return Err(ClipboardError::InvalidPackage("shape has a dangling parent")) };
            if !parent.holds_children() {
                return Err(ClipboardError::InvalidPackage("shape parent cannot hold children"));
            }
        }
        let mut ancestors = HashSet::new();
        let mut at = shape.parent;
        while let Some(parent_id) = at {
            if !ancestors.insert(parent_id) || parent_id == shape.id {
                return Err(ClipboardError::InvalidPackage("shape parents contain a cycle"));
            }
            at = by_id.get(&parent_id).and_then(|parent| parent.parent);
        }
        validate_shape_payload(shape, components)?;
    }
    Ok(())
}

fn validate_shape_payload(shape: &Shape, components: &HashMap<u64, &Component>) -> Result<(), ClipboardError> {
    match &shape.kind {
        ShapeKind::Instance { component, overrides } => {
            let Some(target) = components.get(component) else { return Err(ClipboardError::MissingComponent(*component)) };
            let master_ids: HashSet<u64> = target.shapes.iter().map(|part| part.id).collect();
            if overrides.keys().any(|id| !master_ids.contains(id)) {
                return Err(ClipboardError::InvalidPackage("instance override names a missing master shape"));
            }
        }
        ShapeKind::Flattened { parts, .. } => {
            validate_shape_list(parts, components)?;
        }
        _ => {}
    }
    Ok(())
}

fn visit_shape_components(shape: &Shape, components: &HashMap<u64, &Component>, reachable: &mut HashSet<u64>, visiting: &mut HashSet<u64>) -> Result<(), ClipboardError> {
    match &shape.kind {
        ShapeKind::Instance { component, .. } => {
            if !components.contains_key(component) {
                return Err(ClipboardError::MissingComponent(*component));
            }
            if visiting.contains(component) {
                return Err(ClipboardError::ComponentCycle);
            }
            if !reachable.insert(*component) {
                return Ok(());
            }
            if !visiting.insert(*component) {
                return Err(ClipboardError::ComponentCycle);
            }
            let target = components.get(component).ok_or(ClipboardError::MissingComponent(*component))?;
            for master_shape in &target.shapes {
                visit_shape_components(master_shape, components, reachable, visiting)?;
            }
            visiting.remove(component);
        }
        ShapeKind::Flattened { parts, .. } => {
            for part in parts {
                visit_shape_components(part, components, reachable, visiting)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_links(shape_lists: &[&[Shape]], names: &BTreeMap<u64, String>) -> Result<(), ClipboardError> {
    let mut links: HashMap<u64, (crate::links::LinkKind, usize)> = HashMap::new();
    for shape in shape_lists.iter().flat_map(|shapes| shapes.iter()) {
        if let Some(link) = shape.link {
            let entry = links.entry(link.id).or_insert((link.kind, 0));
            if entry.0 != link.kind {
                return Err(ClipboardError::InvalidPackage("link members disagree about their kind"));
            }
            entry.1 += 1;
        }
    }
    if links.values().any(|(_, count)| *count < 2) {
        return Err(ClipboardError::InvalidPackage("a copied link does not include all of its members"));
    }
    if names.keys().any(|id| !links.contains_key(id)) {
        return Err(ClipboardError::InvalidPackage("link name does not refer to an included link"));
    }
    Ok(())
}

fn would_create_component_cycle(design: &Design, host: u64, pasted_shapes: &[Shape]) -> bool {
    fn reaches_host(design: &Design, component: u64, host: u64, visited: &mut HashSet<u64>) -> bool {
        if component == host {
            return true;
        }
        if !visited.insert(component) {
            return false;
        }
        master_shapes(design, component).is_some_and(|shapes| referenced_components(shapes).into_iter().any(|dependency| reaches_host(design, dependency, host, visited)))
    }

    referenced_components(pasted_shapes).into_iter().any(|component| reaches_host(design, component, host, &mut HashSet::new()))
}

fn cleanup_sanitised_fragment(design: &mut Design) {
    fn clean_parents(shapes: &mut [Shape]) {
        let parents: HashMap<u64, bool> = shapes.iter().map(|shape| (shape.id, shape.holds_children())).collect();
        for shape in shapes {
            if shape.parent.is_some_and(|parent| !parents.get(&parent).copied().unwrap_or(false)) {
                shape.parent = None;
            }
        }
    }
    clean_parents(&mut design.shapes);
    for component in &mut design.components {
        clean_parents(&mut component.shapes);
    }
    design.prune_overrides();

    let mut reachable = HashSet::new();
    let mut visiting = HashSet::new();
    let map: HashMap<u64, &Component> = design.components.iter().map(|component| (component.id, component)).collect();
    for shape in &design.shapes {
        let _ = visit_shape_components(shape, &map, &mut reachable, &mut visiting);
    }
    design.components.retain(|component| reachable.contains(&component.id));

    let mut membership: HashMap<u64, Vec<(u64, crate::links::LinkKind)>> = HashMap::new();
    for shape in design.shapes.iter().chain(design.components.iter().flat_map(|component| &component.shapes)) {
        if let Some(link) = shape.link {
            membership.entry(link.id).or_default().push((shape.id, link.kind));
        }
    }
    let complete: HashSet<u64> = membership
        .into_iter()
        .filter_map(|(id, members)| {
            let consistent = members.first().is_some_and(|(_, kind)| members.iter().all(|(_, other)| other == kind));
            (consistent && members.len() >= 2).then_some(id)
        })
        .collect();
    for shape in design.shapes.iter_mut().chain(design.components.iter_mut().flat_map(|component| &mut component.shapes)) {
        if shape.link.is_some_and(|link| !complete.contains(&link.id)) {
            shape.link = None;
        }
    }
    design.link_names.retain(|id, _| complete.contains(id));
}

fn component_order(components: &[Component]) -> Result<Vec<u64>, ClipboardError> {
    fn visit(id: u64, by_id: &HashMap<u64, &Component>, visiting: &mut HashSet<u64>, done: &mut HashSet<u64>, out: &mut Vec<u64>) -> Result<(), ClipboardError> {
        if done.contains(&id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(ClipboardError::ComponentCycle);
        }
        let component = by_id.get(&id).ok_or(ClipboardError::MissingComponent(id))?;
        for dependency in referenced_components(&component.shapes) {
            visit(dependency, by_id, visiting, done, out)?;
        }
        visiting.remove(&id);
        done.insert(id);
        out.push(id);
        Ok(())
    }
    let by_id: HashMap<u64, &Component> = components.iter().map(|component| (component.id, component)).collect();
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    let mut out = Vec::new();
    for component in components {
        visit(component.id, &by_id, &mut visiting, &mut done, &mut out)?;
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CanonicalComponent {
    name: String,
    size_in: Pt,
    shapes: Vec<CanonicalShape>,
    link_names: Vec<(u64, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CanonicalShape {
    shape: Shape,
    flattened_parts: Vec<CanonicalShape>,
    nested_component: Option<Box<CanonicalComponent>>,
}

type CanonicalLinks = Vec<(u64, u64)>;

fn canonical_fingerprints(
    components: &[Component],
    link_names: &BTreeMap<u64, String>,
    editing: Option<(u64, &[Shape])>,
) -> Result<HashMap<u64, CanonicalComponent>, ClipboardError> {
    let mut context = CanonicalContext { components, link_names, editing, cache: HashMap::new(), visiting: HashSet::new() };
    let mut result = HashMap::new();
    for component in components {
        if let Ok(canonical) = context.component(component.id) {
            result.insert(component.id, canonical);
        }
    }
    Ok(result)
}

struct CanonicalContext<'a> {
    components: &'a [Component],
    link_names: &'a BTreeMap<u64, String>,
    editing: Option<(u64, &'a [Shape])>,
    cache: HashMap<u64, CanonicalComponent>,
    visiting: HashSet<u64>,
}

impl CanonicalContext<'_> {
    fn component(&mut self, id: u64) -> Result<CanonicalComponent, ClipboardError> {
        if let Some(cached) = self.cache.get(&id) {
            return Ok(cached.clone());
        }
        if !self.visiting.insert(id) {
            return Err(ClipboardError::ComponentCycle);
        }
        let result = self.component_contents(id);
        self.visiting.remove(&id);
        let canonical = result?;
        self.cache.insert(id, canonical.clone());
        Ok(canonical)
    }

    fn component_contents(&mut self, id: u64) -> Result<CanonicalComponent, ClipboardError> {
        let source = self.components.iter().find(|component| component.id == id).ok_or(ClipboardError::MissingComponent(id))?;
        let name = source.name.clone();
        let size_in = source.size_in;
        let shapes = self.editing.filter(|(editing_id, _)| *editing_id == id).map_or(source.shapes.as_slice(), |(_, shapes)| shapes).to_vec();
        let (canonical_shapes, link_order) = self.shapes(&shapes)?;
        let link_names = link_order.into_iter().filter_map(|(old, normalized)| self.link_names.get(&old).map(|name| (normalized, name.clone()))).collect();
        Ok(CanonicalComponent { name, size_in, shapes: canonical_shapes, link_names })
    }

    fn shapes(&mut self, shapes: &[Shape]) -> Result<(Vec<CanonicalShape>, CanonicalLinks), ClipboardError> {
        let indices: HashMap<u64, u64> = shapes.iter().enumerate().map(|(index, shape)| (shape.id, index as u64)).collect();
        if indices.len() != shapes.len() {
            return Err(ClipboardError::InvalidPackage("component shape ids are duplicated"));
        }
        let mut link_indices = HashMap::new();
        let mut link_order = Vec::new();
        for shape in shapes {
            if let Some(link) = shape.link
                && !link_indices.contains_key(&link.id)
            {
                let normalized = link_indices.len() as u64;
                link_indices.insert(link.id, normalized);
                link_order.push((link.id, normalized));
            }
        }

        let mut canonical = Vec::with_capacity(shapes.len());
        for source in shapes {
            let mut shape = source.clone();
            shape.id = *indices.get(&source.id).ok_or(ClipboardError::InvalidPackage("shape id cannot be canonicalised"))?;
            shape.parent = source.parent.map(|id| indices.get(&id).copied().ok_or(ClipboardError::InvalidPackage("parent cannot be canonicalised"))).transpose()?;
            if let Some(link) = &mut shape.link {
                link.id = *link_indices.get(&link.id).ok_or(ClipboardError::InvalidPackage("link cannot be canonicalised"))?;
            }
            let mut flattened_parts = Vec::new();
            let mut nested_component = None;
            match &source.kind {
                ShapeKind::Flattened { parts, view_box } => {
                    let (parts, _) = self.shapes(parts)?;
                    flattened_parts = parts;
                    shape.kind = ShapeKind::Flattened { parts: Vec::new(), view_box: *view_box };
                }
                ShapeKind::Instance { component, overrides } => {
                    let target = self.components.iter().find(|candidate| candidate.id == *component).ok_or(ClipboardError::MissingComponent(*component))?;
                    let master = self.editing.filter(|(editing_id, _)| *editing_id == *component).map_or(target.shapes.as_slice(), |(_, shapes)| shapes);
                    let master_indices: HashMap<u64, u64> = master.iter().enumerate().map(|(index, shape)| (shape.id, index as u64)).collect();
                    let mut normalized_overrides = BTreeMap::new();
                    for (id, value) in overrides {
                        let normalized = master_indices.get(id).copied().ok_or(ClipboardError::InvalidPackage("override cannot be canonicalised"))?;
                        normalized_overrides.insert(normalized, value.clone());
                    }
                    shape.kind = ShapeKind::Instance { component: 0, overrides: normalized_overrides };
                    nested_component = Some(Box::new(self.component(*component)?));
                }
                _ => {}
            }
            canonical.push(CanonicalShape { shape, flattened_parts, nested_component });
        }
        Ok((canonical, link_order))
    }
}

fn canonical_hash(component: &CanonicalComponent) -> Result<[u8; 32], ClipboardError> {
    let bytes = serde_json::to_vec(component).map_err(|_| ClipboardError::InvalidPackage("component cannot be fingerprinted"))?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn map_corresponding_shapes(source: &[Shape], destination: &[Shape], maps: &mut IdMaps) -> Result<(), ClipboardError> {
    if source.len() != destination.len() {
        return Err(ClipboardError::InvalidPackage("equal components have different master sizes"));
    }
    for (source, destination) in source.iter().zip(destination) {
        maps.shapes.insert(source.id, destination.id);
    }
    let source_links = link_ordinals(source);
    let destination_links = link_ordinals(destination);
    if source_links.len() != destination_links.len() {
        return Err(ClipboardError::InvalidPackage("equal components have different links"));
    }
    for (source_id, ordinal) in source_links {
        let destination_id = destination_links
            .iter()
            .find_map(|(id, other_ordinal)| (*other_ordinal == ordinal).then_some(*id))
            .ok_or(ClipboardError::InvalidPackage("equal components have different links"))?;
        maps.links.insert(source_id, destination_id);
    }
    Ok(())
}

fn link_ordinals(shapes: &[Shape]) -> HashMap<u64, u64> {
    let mut out = HashMap::new();
    for shape in shapes {
        if let Some(link) = shape.link {
            let next = out.len() as u64;
            out.entry(link.id).or_insert(next);
        }
    }
    out
}

fn allocate_links(shapes: &[Shape], maps: &mut IdMaps, allocator: &mut IdAllocator) -> Result<(), ClipboardError> {
    let mut links = BTreeSet::new();
    for shape in shapes {
        if let Some(link) = shape.link {
            links.insert(link.id);
        }
    }
    for id in links {
        if let std::collections::hash_map::Entry::Vacant(entry) = maps.links.entry(id) {
            entry.insert(allocator.allocate()?);
        }
    }
    Ok(())
}

fn remap_shape(shape: &Shape, live: bool, maps: &IdMaps) -> Result<Shape, ClipboardError> {
    let mut remapped = shape.clone();
    if live {
        remapped.id = maps.shapes.get(&shape.id).copied().ok_or(ClipboardError::InvalidPackage("shape has no destination id"))?;
        remapped.parent =
            shape.parent.map(|parent| maps.shapes.get(&parent).copied().ok_or(ClipboardError::InvalidPackage("parent has no destination id"))).transpose()?;
        remapped.link = shape
            .link
            .map(|link| {
                maps.links.get(&link.id).copied().map(|id| crate::links::Link { id, kind: link.kind }).ok_or(ClipboardError::InvalidPackage("link has no destination id"))
            })
            .transpose()?;
    }
    remap_nested_references(&mut remapped, maps)?;
    Ok(remapped)
}

fn remap_nested_references(shape: &mut Shape, maps: &IdMaps) -> Result<(), ClipboardError> {
    match &mut shape.kind {
        ShapeKind::Instance { component, overrides } => {
            *component = maps.components.get(component).copied().ok_or(ClipboardError::MissingComponent(*component))?;
            let mut remapped = BTreeMap::new();
            for (id, override_) in overrides.iter() {
                let new_id = maps.shapes.get(id).copied().ok_or(ClipboardError::InvalidPackage("instance override has no destination master shape"))?;
                remapped.insert(new_id, override_.clone());
            }
            *overrides = remapped;
        }
        ShapeKind::Flattened { parts, .. } => {
            for part in parts {
                remap_nested_references(part, maps)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn merge_repairs(into: &mut Repairs, more: Repairs) {
    into.dropped.extend(more.dropped);
    into.clamped.extend(more.clamped);
    into.dropped.sort_unstable();
    into.dropped.dedup();
    into.clamped.sort_unstable();
    into.clamped.dedup();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use egui::Color32;

    use super::*;
    use crate::components::{Component, PartOverride};
    use crate::containers::ContainerRole;
    use crate::design::ShapeKind;
    use crate::links::{Link, LinkKind};
    use crate::patterns::{PatternFill, PatternMode};

    fn source() -> Source {
        Source { document_id: "tab-a".into(), mat_in: (12.0, 12.0) }
    }

    fn package(design: &Design, ids: &[u64]) -> Package {
        Package::capture(design, ids, source(), &Pixels::new()).expect("capture package")
    }

    fn square(id: u64, x: f32, y: f32) -> Shape {
        Shape { x_in: x, y_in: y, width_in: 1.0, height_in: 1.0, ..Shape::new(id, ShapeKind::Square) }
    }

    #[test]
    fn container_capture_closes_over_children_and_drop_reparents_only_the_new_root() {
        let mut source_design = Design::default();
        source_design.shapes.push(Shape {
            x_in: 1.0,
            y_in: 1.0,
            width_in: 4.0,
            height_in: 4.0,
            container: Some(ContainerRole { padding_in: 0.0, layout: None }),
            ..Shape::new(1, ShapeKind::Rectangle)
        });
        source_design.shapes.push(Shape { parent: Some(1), hidden: true, ..square(2, 2.0, 2.0) });
        source_design.shapes.push(square(3, 8.0, 8.0));
        let captured = package(&source_design, &[1]);
        assert_eq!(captured.shapes.iter().map(|shape| shape.id).collect::<Vec<_>>(), [1, 2], "hidden child included, unrelated shape excluded");
        let captured = captured.validate().unwrap();

        let mut destination = Design::default();
        destination.shapes.push(Shape {
            x_in: 0.0,
            y_in: 0.0,
            width_in: 10.0,
            height_in: 10.0,
            container: Some(ContainerRole { padding_in: 0.0, layout: None }),
            ..Shape::new(20, ShapeKind::Rectangle)
        });
        let result = captured.place(&mut destination, Placement::InPlace, (2.0, 2.0)).expect("paste into container");
        let pasted_root = result.roots[0];
        let root_parent = destination.shapes.iter().find(|shape| shape.id == pasted_root).and_then(|shape| shape.parent);
        let child_parent = destination.shapes.iter().find(|shape| shape.parent == Some(pasted_root)).and_then(|shape| shape.parent);
        assert_eq!(root_parent, Some(20));
        assert_eq!(child_parent, Some(pasted_root));

        let child_only = package(&source_design, &[2]);
        assert_eq!(child_only.shapes.len(), 1);
        assert_eq!(child_only.shapes[0].parent, None, "copying a child clears its external parent");
        let child_only = child_only.validate().unwrap();
        let mut empty = Design::default();
        let pasted = child_only.place(&mut empty, Placement::InPlace, (20.0, 20.0)).unwrap();
        assert_eq!(empty.shapes[0].id, pasted.roots[0]);
        assert_eq!(empty.shapes[0].parent, None);
        assert_eq!((empty.shapes[0].x_in, empty.shapes[0].y_in), (2.0, 2.0));
    }

    #[test]
    fn copying_an_instance_reuses_the_component_and_maps_override_keys_to_its_master() {
        let mut source_design = Design::default();
        let master = square(101, 0.0, 0.0);
        source_design.components.push(Component { id: 100, name: "Badge".into(), shapes: vec![master], size_in: (1.0, 1.0) });
        source_design.shapes.push(Shape {
            x_in: 3.0,
            y_in: 4.0,
            kind: ShapeKind::Instance { component: 100, overrides: BTreeMap::from([(101, PartOverride { fill: Some(Color32::RED), ..Default::default() })]) },
            ..square(1, 3.0, 4.0)
        });
        let captured = package(&source_design, &[1]).validate().unwrap();

        let mut destination = Design::default();
        let first = captured.place(&mut destination, Placement::InPlace, (40.0, 40.0)).unwrap();
        assert_eq!(destination.components.len(), 1);
        let first_component_id = destination.components[0].id;
        let first_master_id = destination.components[0].shapes[0].id;
        let ShapeKind::Instance { component, overrides } = &destination.shapes[0].kind else { panic!("pasted instance") };
        assert_eq!(*component, first_component_id);
        assert!(overrides.contains_key(&first_master_id));

        let second = captured.place(&mut destination, Placement::Offset, (40.0, 40.0)).unwrap();
        assert_eq!(destination.components.len(), 1, "identical component reused");
        let ShapeKind::Instance { component, overrides } = &destination.shapes[1].kind else { panic!("second pasted instance") };
        assert_eq!(*component, first_component_id);
        assert!(overrides.contains_key(&first_master_id));
        assert_ne!(first.roots, second.roots);

        let mut different_source_ids = Design::default();
        different_source_ids.components.push(Component { id: 900, name: "Badge".into(), shapes: vec![square(901, 0.0, 0.0)], size_in: (1.0, 1.0) });
        different_source_ids.shapes.push(Shape {
            kind: ShapeKind::Instance { component: 900, overrides: BTreeMap::from([(901, PartOverride { fill: Some(Color32::RED), ..Default::default() })]) },
            ..square(902, 3.0, 4.0)
        });
        let equivalent = package(&different_source_ids, &[902]).validate().unwrap();
        equivalent.place(&mut destination, Placement::InPlace, (40.0, 40.0)).unwrap();
        assert_eq!(destination.components.len(), 1, "canonical equality ignores source ids");
        let ShapeKind::Instance { component, overrides } = &destination.shapes[2].kind else { panic!("equivalent pasted instance") };
        assert_eq!(*component, first_component_id);
        assert!(overrides.contains_key(&first_master_id));
    }

    #[test]
    fn paste_style_uses_the_first_shape_operation_and_appearance() {
        let mut design = Design::default();
        let mut first = square(1, 1.0, 2.0);
        first.operation = Operation::PrintThenCut;
        first.fill = Color32::from_rgb(10, 20, 30);
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([4, 5, 6, 255]));
        let asset = crate::assets::id_of(&image);
        first.pattern = Some(PatternFill::new(asset.clone(), 1.0, PatternMode::Tile));
        let before_geometry = (first.x_in, first.y_in, first.width_in, first.height_in);
        design.shapes.push(first.clone());
        design.shapes.push(square(2, 8.0, 8.0));
        let pixels = Pixels::from([(asset, Arc::new(image))]);
        let style = Package::capture(&design, &[1, 2], source(), &pixels).unwrap().paste_style().unwrap();
        assert_eq!(style.operation, first.operation);
        assert_eq!(style.fill, first.fill);
        assert_eq!(style.pen, first.pen);
        assert_eq!(style.pattern, first.pattern);
        assert_eq!((first.x_in, first.y_in, first.width_in, first.height_in), before_geometry);
    }

    #[test]
    fn paste_style_applies_to_three_shapes_without_changing_their_geometry() {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 30, 50, 255]));
        let asset = assets::id_of(&image);
        let mut design = Design::default();
        let first = Shape {
            operation: Operation::PrintThenCut,
            fill: Color32::from_rgb(90, 40, 170),
            pen: crate::pens::Pen { size: crate::pens::PenSize::Bold, colour: 14 },
            pattern: Some(crate::patterns::PatternFill::new(asset.clone(), 1.0, crate::patterns::PatternMode::Tile)),
            ..square(1, 0.0, 0.0)
        };
        design.shapes.push(first.clone());
        design.shapes.extend([square(2, 2.0, 3.0), square(3, 5.0, 6.0), square(4, 8.0, 9.0)]);
        let geometry: Vec<_> = design.shapes[1..].iter().map(|shape| (shape.x_in, shape.y_in, shape.width_in, shape.height_in)).collect();
        let pixels = Pixels::from([(asset, Arc::new(image))]);
        let package = Package::capture(&design, &[1, 2, 3, 4], source(), &pixels).unwrap();
        let style = package.paste_style().unwrap();

        design.apply(&crate::edit::Edit::PasteStyle(style), &[2, 3, 4]).unwrap();
        for (shape, before) in design.shapes[1..].iter().zip(geometry) {
            assert_eq!((shape.operation, shape.fill, shape.pen, shape.pattern.as_ref()), (first.operation, first.fill, first.pen, first.pattern.as_ref()));
            assert_eq!((shape.x_in, shape.y_in, shape.width_in, shape.height_in), before);
        }
    }

    #[test]
    fn oversized_incoming_shape_is_clamped_and_reported_without_sanitising_destination() {
        let mut source_design = Design::default();
        source_design.shapes.push(Shape { width_in: 400.0, ..square(1, 2.0, 3.0) });
        let captured = package(&source_design, &[1]).validate().unwrap();
        let mut destination = Design::default();
        destination.shapes.push(Shape { width_in: 400.0, ..square(40, 1.0, 1.0) });
        let before = destination.shapes[0].clone();

        let result = captured.place(&mut destination, Placement::InPlace, (30.0, 30.0)).unwrap();
        assert_eq!(result.repairs.clamped.len(), 1);
        assert_eq!(destination.shapes[1].width_in, crate::transform::MAX_SIZE_IN);
        assert_eq!(destination.shapes[0], before, "existing oversized shapes are untouched");
    }

    #[test]
    fn asset_references_are_embedded_verified_and_required() {
        let pixels_value = image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]));
        let id = assets::id_of(&pixels_value);
        let mut source_design = Design::default();
        source_design.shapes.push(Shape { kind: ShapeKind::Bitmap { asset: id.clone(), px: (2, 1), clip: Vec::new() }, ..square(1, 0.0, 0.0) });
        let pixels = Pixels::from([(id.clone(), Arc::new(pixels_value))]);
        let mut captured = Package::capture(&source_design, &[1], source(), &pixels).unwrap();
        assert_eq!(captured.decode_assets().unwrap()[0].0, id);

        let mut without_pixels = source_design.clone();
        without_pixels.shapes[0].pattern = None;
        assert!(matches!(Package::capture(&without_pixels, &[1], source(), &Pixels::new()), Err(ClipboardError::MissingAsset(_))));

        captured.assets[0].png = b"not a png".to_vec();
        assert!(captured.validate().is_err(), "invalid assets fail before a package can reach Design::apply");
    }

    #[test]
    fn paste_allocates_fresh_ids_even_when_the_source_id_exists_in_the_destination() {
        let mut source_design = Design::default();
        source_design.shapes.push(square(7, 1.0, 1.0));
        let captured = package(&source_design, &[7]).validate().unwrap();

        let mut destination = Design::default();
        destination.shapes.push(square(7, 5.0, 5.0));
        let result = captured.place(&mut destination, Placement::InPlace, (50.0, 50.0)).unwrap();
        assert_ne!(result.roots[0], 7);
        assert_eq!(destination.shapes.iter().map(|shape| shape.id).collect::<HashSet<_>>().len(), 2);
        assert_eq!(destination.shapes[0].id, 7);
    }

    #[test]
    fn link_closure_does_not_import_an_unselected_link_sibling() {
        let mut design = Design::default();
        let link = Link { id: 5, kind: LinkKind::Group };
        design.shapes.push(Shape { link: Some(link), ..square(1, 1.0, 1.0) });
        design.shapes.push(Shape { link: Some(link), ..square(2, 3.0, 1.0) });
        design.link_names.insert(5, "pair".into());
        let captured = package(&design, &[1]);
        assert_eq!(captured.shapes.len(), 1);
        assert_eq!(captured.shapes[0].link, None);
        assert!(captured.link_names.is_empty());
    }

    #[test]
    fn a_complete_link_keeps_its_name_under_a_fresh_id() {
        let mut design = Design::default();
        let link = Link { id: 5, kind: LinkKind::Group };
        design.shapes.push(Shape { link: Some(link), ..square(1, 1.0, 1.0) });
        design.shapes.push(Shape { link: Some(link), ..square(2, 3.0, 1.0) });
        design.link_names.insert(5, "Pair".into());
        let captured = package(&design, &[1, 2]).validate().unwrap();
        let mut destination = Design::default();

        let result = captured.place(&mut destination, Placement::Offset, (8.0, 8.0)).unwrap();
        let ids: HashSet<u64> = result.roots.iter().copied().collect();
        let copied: Vec<_> = destination.shapes.iter().filter(|shape| ids.contains(&shape.id)).collect();
        let copied_link = copied[0].link.expect("linked copy").id;
        assert_ne!(copied_link, 5);
        assert!(copied.iter().all(|shape| shape.link.is_some_and(|link| link.id == copied_link)));
        assert_eq!(destination.link_names.get(&copied_link).map(String::as_str), Some("Pair"));
    }
}

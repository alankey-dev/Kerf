//! Clipboard I/O at the egui boundary. Clipboard contents remain in the system clipboard; only a
//! short-lived menu paste intent is kept in egui context data.

use std::sync::Arc;
use std::time::Duration;

use egui::{Context, Event, Id, Modifiers, ViewportCommand};
use roxmltree::Document;

use crate::clipboard::{Package, ValidatedPackage};
use crate::shape_library::Import;

const METADATA_ID: &str = "kerf-clipboard";
const MAX_CLIPBOARD_BYTES: usize = 384 * 1024 * 1024;
const PASTE_INTENT_TTL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    Cut,
    Copy,
    CopyAsImage,
    Paste,
    PasteInPlace,
    PasteStyle,
}

/// Copy as PNG renders the selection at this resolution with a transparent background.
const COPY_IMAGE_DPI: u32 = 300;

pub(crate) enum Payload {
    Package(crate::clipboard::ValidatedPackage),
    Svg(Vec<Import>),
    Text(String),
}

#[derive(Clone, Copy)]
struct PendingIntent {
    action: Action,
    drop_at: Option<crate::design::Pt>,
    expires_at: f64,
}

fn intent_id() -> Id {
    Id::new("clipboard-paste-intent")
}

pub(crate) fn copy(ctx: &Context, svg: String) {
    ctx.copy_text(svg);
}

/// Requests a system clipboard paste with a menu-selected placement or style action.
pub(crate) fn request_paste(ctx: &Context, action: Action, drop_at: Option<crate::design::Pt>) {
    if !matches!(action, Action::Paste | Action::PasteInPlace | Action::PasteStyle) {
        return;
    }

    let now = ctx.input(|input| input.time);
    ctx.data_mut(|data| {
        data.insert_temp(intent_id(), Some(PendingIntent { action, drop_at, expires_at: now + PASTE_INTENT_TTL.as_secs_f64() }));
    });
    ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
    ctx.request_repaint_after(PASTE_INTENT_TTL);
}

/// Consumes one paste event when the caller and egui both leave keyboard input to clipboard commands.
/// A pending menu intent takes priority over modifiers on keyboard paste events.
pub(crate) fn take_paste(ctx: &Context, clipboard_commands_may_run: bool) -> Option<(Action, String, Option<crate::design::Pt>)> {
    let now = ctx.input(|input| input.time);
    clear_expired_intent(ctx, now);

    if !clipboard_commands_may_run || ctx.wants_keyboard_input() {
        return None;
    }

    let (text, modifiers) = ctx.input_mut(|input| {
        let index = input.events.iter().position(|event| matches!(event, Event::Paste(_)))?;
        let Event::Paste(text) = input.events.remove(index) else { unreachable!() };
        Some((text, input.modifiers))
    })?;

    let pending = ctx.data_mut(|data| data.remove_temp::<Option<PendingIntent>>(intent_id()).flatten());
    let (action, drop_at) = pending.map_or_else(|| (action_from_modifiers(modifiers), None), |intent| (intent.action, intent.drop_at));
    Some((action, text, drop_at))
}

fn clear_expired_intent(ctx: &Context, now: f64) {
    ctx.data_mut(|data| {
        let expired = data.get_temp::<Option<PendingIntent>>(intent_id()).flatten().is_some_and(|intent| !now.is_finite() || now > intent.expires_at);
        if expired {
            data.remove_temp::<Option<PendingIntent>>(intent_id());
        }
    });
}

fn action_from_modifiers(modifiers: Modifiers) -> Action {
    let command = modifiers.command || modifiers.ctrl;
    if command && modifiers.alt {
        Action::PasteStyle
    } else if command && modifiers.shift {
        Action::PasteInPlace
    } else {
        Action::Paste
    }
}

/// Decodes a system clipboard string without treating malformed SVG as plain text.
pub(crate) fn parse(text: &str) -> Result<Payload, String> {
    check_size(text.len())?;

    let document = match Document::parse(text) {
        Ok(document) => document,
        Err(error) if looks_like_svg(text) => return Err(format!("invalid SVG: {error}")),
        Err(_) => return Ok(Payload::Text(text.to_owned())),
    };

    if document.root_element().tag_name().name() != "svg" {
        return Ok(Payload::Text(text.to_owned()));
    }

    if let Some(json) = metadata_json(&document)? {
        let package: Package = serde_json::from_str(&json).map_err(|error| format!("invalid clipboard metadata JSON: {error}"))?;
        if package.project_format_version() > crate::project::FORMAT_VERSION {
            return Err(format!("clipboard package uses a newer project format ({})", package.project_format_version()));
        }
        let package = package.validate().map_err(|error| error.to_string())?;
        return Ok(Payload::Package(package));
    }

    let imports = crate::shape_library::import_svg(text.as_bytes())?;
    Ok(Payload::Svg(imports))
}

fn check_size(bytes: usize) -> Result<(), String> {
    if bytes > MAX_CLIPBOARD_BYTES { Err(format!("clipboard content exceeds the {MAX_CLIPBOARD_BYTES}-byte limit")) } else { Ok(()) }
}

fn looks_like_svg(text: &str) -> bool {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("<?xml") {
        return true;
    }
    let Some(rest) = text.strip_prefix("<svg") else { return false };
    rest.is_empty() || rest.chars().next().is_some_and(|character| matches!(character, '>' | '/' | ' ' | '\t' | '\r' | '\n'))
}

/// Extracts the one Kerf metadata element, preserving its text after XML entity unescaping.
fn metadata_json(document: &Document<'_>) -> Result<Option<String>, String> {
    let root = document.root_element();
    let metadata: Vec<_> =
        document.descendants().filter(|node| node.is_element() && node.tag_name().name() == "metadata" && node.attribute("id") == Some(METADATA_ID)).collect();

    if metadata.len() > 1 {
        return Err("SVG contains duplicate Kerf clipboard metadata".into());
    }
    let Some(element) = metadata.first().copied() else { return Ok(None) };
    if element.parent() != Some(root) {
        return Err("Kerf clipboard metadata must be a child of the SVG root".into());
    }

    let mut json = String::new();
    for child in element.children() {
        if !child.is_text() {
            return Err("Kerf clipboard metadata must contain text only".into());
        }
        json.push_str(child.text().unwrap_or_default());
    }
    Ok(Some(json))
}

/// Adds the native package to an SVG without changing ordinary SVG export output.
pub(crate) fn wrap_svg(svg: &str, package: &Package) -> Result<String, String> {
    check_size(svg.len())?;
    let document = Document::parse(svg).map_err(|error| format!("invalid SVG: {error}"))?;
    if document.root_element().tag_name().name() != "svg" {
        return Err("clipboard wrapper requires an SVG root element".into());
    }
    if metadata_json(&document)?.is_some() {
        return Err("SVG already contains Kerf clipboard metadata".into());
    }

    let json = serde_json::to_string(package).map_err(|error| format!("could not serialise clipboard package: {error}"))?;
    let escaped = xml_escape(&json);
    let root_start = document.root_element().range().start;
    let (open_end, self_closing, name, slash) = opening_tag(svg, root_start)?;
    let metadata = format!("<metadata id=\"{METADATA_ID}\">{escaped}</metadata>");

    if self_closing {
        let slash = slash.ok_or_else(|| "could not locate the SVG root tag".to_owned())?;
        let mut wrapped = String::with_capacity(svg.len() + metadata.len() + name.len() + 3);
        wrapped.push_str(&svg[..slash]);
        wrapped.push_str(&svg[slash + 1..open_end]);
        wrapped.push('>');
        wrapped.push_str(&metadata);
        wrapped.push_str("</");
        wrapped.push_str(name);
        wrapped.push('>');
        wrapped.push_str(&svg[open_end + 1..]);
        check_size(wrapped.len())?;
        return Ok(wrapped);
    }

    let mut wrapped = String::with_capacity(svg.len() + metadata.len());
    wrapped.push_str(&svg[..open_end + 1]);
    wrapped.push_str(&metadata);
    wrapped.push_str(&svg[open_end + 1..]);
    check_size(wrapped.len())?;
    Ok(wrapped)
}

fn opening_tag(svg: &str, start: usize) -> Result<(usize, bool, &str, Option<usize>), String> {
    let bytes = svg.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return Err("could not locate the SVG root tag".into());
    }

    let name_start = start + 1;
    let mut cursor = name_start;
    while let Some(byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>') {
            break;
        }
        cursor += 1;
    }
    let name = svg.get(name_start..cursor).ok_or_else(|| "could not read the SVG root name".to_owned())?;
    let mut quote = None;
    while let Some(&byte) = bytes.get(cursor) {
        match (quote, byte) {
            (Some(active), value) if active == value => quote = None,
            (None, b'\'' | b'\"') => quote = Some(byte),
            (None, b'>') => {
                let slash = bytes[..cursor].iter().rposition(|byte| !byte.is_ascii_whitespace()).filter(|index| bytes[*index] == b'/');
                return Ok((cursor, slash.is_some(), name, slash));
            }
            _ => {}
        }
        cursor += 1;
    }
    Err("SVG root opening tag is incomplete".into())
}

fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

impl super::Kerf {
    pub(super) fn run_clipboard(&mut self, action: Action, ctx: &Context) {
        tracing::info!(target: "cmd", action = ?action, sel = %crate::debug_log::ids(&self.doc.selection), "clipboard");
        match action {
            Action::Copy => {
                if !self.doc.selection.is_empty() {
                    match capture_clip(self, &self.doc.selection).and_then(|package| write_clip(&package, ctx)) {
                        Ok(()) => {}
                        Err(error) => self.doc.view.notice = Some(super::notice::Notice::new(error, false)),
                    }
                }
            }
            Action::CopyAsImage => {
                if !self.doc.selection.is_empty()
                    && let Err(error) = copy_as_image(self, ctx)
                {
                    self.doc.view.notice = Some(super::notice::Notice::new(error, false));
                }
            }
            Action::Cut => cut_selection(self, ctx),
            Action::Paste | Action::PasteInPlace | Action::PasteStyle => request_paste(ctx, action, self.doc.view.clipboard_target_in),
        }
    }

    pub(super) fn clipboard_input(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        if take_input_event(ctx, |event| matches!(event, Event::Copy)) {
            if self.doc.flow.is_none()
                && self.doc.trace.is_none()
                && self.doc.cut_preview.is_none()
                && self.doc.export.is_none()
                && self.doc.autosave.preview().is_none()
                && self.doc.view.nodes.is_none()
                && self.doc.view.text_edit.is_none()
                && let Some(measurement) = self.doc.view.measure.measurement
            {
                ctx.copy_text(measurement.label(self.units));
            } else if clipboard_edits_live(self) && !self.doc.selection.is_empty() {
                self.run_clipboard(Action::Copy, ctx);
            }
        }
        if clipboard_edits_live(self) && take_input_event(ctx, |event| matches!(event, Event::Cut)) {
            self.run_clipboard(Action::Cut, ctx);
        }
        let live = clipboard_edits_live(self);
        if let Some((action, text, drop_at)) = take_paste(ctx, live) {
            paste_clip(self, action, &text, drop_at, ctx);
        }
    }
}

fn cut_selection(app: &mut super::Kerf, ctx: &Context) {
    if app.doc.selection.is_empty() {
        return;
    }
    let expanded = app.doc.design.expand(&app.doc.selection);
    let roots = app.doc.design.roots(&expanded);
    let root_count = roots.len();
    let deletable: Vec<u64> = roots
        .into_iter()
        .filter(|&root| {
            app.doc.design.with_descendants(&[root]).iter().all(|id| app.doc.design.shapes.iter().find(|shape| shape.id == *id).is_some_and(|shape| !shape.locked))
        })
        .collect();
    if deletable.is_empty() {
        app.doc.view.notice = Some(super::notice::Notice::new("Locked layers stay", false));
        return;
    }
    let skipped = deletable.len() != root_count;
    if let Err(error) = capture_clip(app, &deletable).and_then(|package| write_clip(&package, ctx)) {
        app.doc.view.notice = Some(super::notice::Notice::new(error, false));
        return;
    }
    if let Err(error) = crate::edit::run(&mut app.doc.design, &mut app.doc.selection, &crate::edit::Edit::Delete, crate::edit::Subject::Ids(&deletable)) {
        app.doc.view.notice = Some(super::notice::Notice::new(error.reason(), false));
    } else if skipped {
        app.doc.view.notice = Some(super::notice::Notice::new("Locked layers stay", false));
    }
}

fn capture_clip(app: &super::Kerf, selection: &[u64]) -> Result<Package, String> {
    let asset_dir = app.projects_dir.as_deref().map(crate::assets::dir);
    let ids = crate::assets::ids(&app.doc.design);
    let pixels = crate::assets::pixels(asset_dir.as_deref(), ids.iter());
    Package::capture(
        &app.doc.design,
        selection,
        crate::clipboard::Source { document_id: app.doc.autosave.id.clone(), mat_in: (app.doc.mat.width_in, app.doc.mat.height_in) },
        &pixels,
    )
    .map_err(|error| error.to_string())
}

fn write_clip(package: &Package, ctx: &Context) -> Result<(), String> {
    let design = package.export_design();
    let decoded = package.decode_assets().map_err(|error| error.to_string())?;
    let pixels: crate::assets::Pixels = decoded.into_iter().map(|(id, image)| (id, Arc::new(image))).collect();
    let setup = crate::make_flow::MatSetup::preferred(crate::machine::MachineFamily::StudioPro);
    let sheets = crate::export::sheets(&design, crate::export::Scope::Design, &setup);
    let svg = match sheets.first() {
        Some(sheet) => crate::export::svg::document(sheet, crate::units::Units::Imperial, crate::export::Style::Filled, &pixels),
        None => r#"<svg xmlns="http://www.w3.org/2000/svg" width="1in" height="1in" viewBox="0 0 72 72"></svg>"#.into(),
    };
    let svg = wrap_svg(&svg, package)?;
    copy(ctx, svg);
    Ok(())
}

/// Copy as PNG (spec 54): the vector clip on the clipboard as usual, and the selection rendered to a
/// transparent PNG beside it, where the platform allows an image. A paste into a chat gets the picture,
/// a paste into Kerf gets the shapes.
fn copy_as_image(app: &super::Kerf, ctx: &Context) -> Result<(), String> {
    let package = capture_clip(app, &app.doc.selection)?;
    write_clip(&package, ctx)?;
    let design = package.export_design();
    let decoded = package.decode_assets().map_err(|error| error.to_string())?;
    let pixels: crate::assets::Pixels = decoded.into_iter().map(|(id, image)| (id, Arc::new(image))).collect();
    let setup = crate::make_flow::MatSetup::preferred(crate::machine::MachineFamily::StudioPro);
    let sheets = crate::export::sheets(&design, crate::export::Scope::Design, &setup);
    let Some(sheet) = sheets.first() else { return Ok(()) };
    let opts =
        crate::export::raster::RasterOptions { dpi: COPY_IMAGE_DPI, background: crate::export::raster::Background::Transparent, margin_in: 0.0, ..Default::default() };
    if crate::export::raster::oversize(sheet.size_in, &opts).is_some() {
        return Err("This selection is too large to copy as an image. Export it as a PNG instead.".into());
    }
    let image = crate::export::raster::render(sheet, &opts, &pixels);
    let (w, h) = image.dimensions();
    ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.as_raw()));
    Ok(())
}

fn install_clip_assets(app: &super::Kerf, package: &ValidatedPackage, ctx: &Context) -> Result<(), String> {
    let assets = package.assets();
    if assets.is_empty() {
        return Ok(());
    }
    let projects = app.projects_dir.as_deref().ok_or_else(|| "Kerf can't store images without a projects folder.".to_owned())?;
    let dir = crate::assets::dir(projects);
    let textures = crate::assets::Textures::of(ctx);
    for (id, image) in assets {
        crate::assets::store(&dir, image).map_err(|error| format!("could not store clipboard image: {error}"))?;
        if let Some(textures) = &textures {
            let color = egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
            textures.insert(ctx, id, color.clone(), crate::assets::Wrap::Clamp);
            textures.insert(ctx, id, color, crate::assets::Wrap::Repeat);
        }
    }
    Ok(())
}

fn paste_clip(app: &mut super::Kerf, action: Action, text: &str, drop_at: Option<crate::design::Pt>, ctx: &Context) {
    let paste_error = |app: &mut super::Kerf, error: String| app.doc.view.notice = Some(super::notice::Notice::new(error, false));
    let payload = match parse(text) {
        Ok(payload) => payload,
        Err(error) => return paste_error(app, error),
    };
    let drop_at = drop_at.unwrap_or_else(|| app.doc.view.centre_in());
    match payload {
        Payload::Package(package) => {
            if action == Action::PasteStyle {
                if let Err(error) = install_clip_assets(app, &package, ctx) {
                    return paste_error(app, error);
                }
                let style = match package.paste_style() {
                    Ok(style) => style,
                    Err(error) => return paste_error(app, error.to_string()),
                };
                match crate::edit::run(&mut app.doc.design, &mut app.doc.selection, &crate::edit::Edit::PasteStyle(style), crate::edit::Subject::Selection) {
                    Ok(()) => {}
                    Err(error) => app.doc.view.notice = Some(super::notice::Notice::new(error.reason(), false)),
                }
                return;
            }
            if let Err(error) = install_clip_assets(app, &package, ctx) {
                return paste_error(app, error);
            }
            let repairs = package.repairs().clone();
            let source = package.source();
            let mat = (app.doc.mat.width_in, app.doc.mat.height_in);
            let placement = if action == Action::PasteInPlace {
                crate::clipboard::Placement::InPlace
            } else if source.document_id == app.doc.autosave.id {
                crate::clipboard::Placement::Offset
            } else if source.mat_in == mat {
                crate::clipboard::Placement::InPlace
            } else {
                crate::clipboard::Placement::At(app.doc.view.centre_in())
            };
            let edit = crate::edit::Edit::Paste { package: Box::new(package), placement, drop_at };
            match crate::edit::run(&mut app.doc.design, &mut app.doc.selection, &edit, crate::edit::Subject::Selection) {
                Ok(()) if !repairs.is_empty() => app.doc.view.notice = Some(super::notice::Notice::new(format!("Pasted content: {repairs}"), false)),
                Ok(()) => {}
                Err(error) => app.doc.view.notice = Some(super::notice::Notice::new(error.reason(), false)),
            }
        }
        Payload::Svg(imports) => {
            if action == Action::PasteStyle {
                return paste_error(app, "Paste Style needs a Kerf clipboard item.".into());
            }
            let mut design = crate::design::Design::default();
            let roots = design.add_imported(imports);
            let package = match Package::capture(
                &design,
                &roots,
                crate::clipboard::Source { document_id: "external-svg".into(), mat_in: (app.doc.mat.width_in, app.doc.mat.height_in) },
                &crate::assets::Pixels::new(),
            )
            .and_then(Package::validate)
            {
                Ok(package) => package,
                Err(error) => return paste_error(app, error.to_string()),
            };
            let edit = crate::edit::Edit::Paste { package: Box::new(package), placement: crate::clipboard::Placement::At(app.doc.view.centre_in()), drop_at };
            if let Err(error) = crate::edit::run(&mut app.doc.design, &mut app.doc.selection, &edit, crate::edit::Subject::Selection) {
                app.doc.view.notice = Some(super::notice::Notice::new(error.reason(), false));
            }
        }
        Payload::Text(text) => {
            if action == Action::PasteStyle {
                return paste_error(app, "Paste Style needs a Kerf clipboard item.".into());
            }
            let mut design = crate::design::Design::default();
            let id = design.add_text(text, crate::text::FontRef::Inter, 1.0, egui::Color32::BLACK, 0.0, 0.0);
            let centre = app.doc.view.centre_in();
            if let Some(shape) = design.shape_mut(id) {
                shape.x_in = centre.0 - shape.width_in / 2.0;
                shape.y_in = centre.1 - shape.height_in / 2.0;
            }
            let package = match Package::capture(
                &design,
                &[id],
                crate::clipboard::Source { document_id: "external-text".into(), mat_in: (app.doc.mat.width_in, app.doc.mat.height_in) },
                &crate::assets::Pixels::new(),
            )
            .and_then(Package::validate)
            {
                Ok(package) => package,
                Err(error) => return paste_error(app, error.to_string()),
            };
            let edit = crate::edit::Edit::Paste { package: Box::new(package), placement: crate::clipboard::Placement::InPlace, drop_at };
            if let Err(error) = crate::edit::run(&mut app.doc.design, &mut app.doc.selection, &edit, crate::edit::Subject::Selection) {
                app.doc.view.notice = Some(super::notice::Notice::new(error.reason(), false));
            }
        }
    }
}

fn clipboard_edits_live(app: &super::Kerf) -> bool {
    app.doc.flow.is_none()
        && app.doc.trace.is_none()
        && app.doc.cut_preview.is_none()
        && app.doc.export.is_none()
        && app.doc.tool == super::canvas::Tool::Select
        && app.doc.view.nodes.is_none()
        && app.doc.view.text_edit.is_none()
        && app.doc.autosave.preview().is_none()
}

fn take_input_event(ctx: &Context, matches: impl Fn(&Event) -> bool) -> bool {
    ctx.input_mut(|input| {
        let Some(index) = input.events.iter().position(matches) else { return false };
        input.events.remove(index);
        true
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::assets::Pixels;
    use crate::clipboard::Source;
    use crate::design::{Design, Shape, ShapeKind};

    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><path d="M0 0 L10 0 L10 10 Z"/></svg>"#;

    fn package() -> Package {
        let mut design = Design::default();
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([8, 16, 32, 255]));
        let asset = crate::assets::id_of(&image);
        design.shapes.push(Shape {
            x_in: 1.0,
            y_in: 2.0,
            width_in: 1.0,
            height_in: 1.0,
            pattern: Some(crate::patterns::PatternFill::new(asset.clone(), 1.0, crate::patterns::PatternMode::Tile)),
            ..Shape::new(1, ShapeKind::Square)
        });
        let pixels = Pixels::from([(asset, Arc::new(image))]);
        Package::capture(&design, &[1], Source { document_id: "tab<&\"'".into(), mat_in: (12.0, 12.0) }, &pixels).expect("capture package")
    }

    #[test]
    fn wrapped_package_round_trips_with_exact_xml_unescaped_json() {
        let package = package();
        let json = serde_json::to_string(&package).expect("serialise package");
        let wrapped = wrap_svg(SVG, &package).expect("wrap SVG");
        let document = Document::parse(&wrapped).expect("valid wrapped SVG");

        assert_eq!(metadata_json(&document).expect("metadata parses").as_deref(), Some(json.as_str()));
        let Payload::Package(parsed) = parse(&wrapped).expect("parse wrapped package") else { panic!("expected native package") };
        assert_eq!(parsed.package(), &package);
    }

    #[test]
    fn malformed_or_duplicate_metadata_is_rejected() {
        let malformed = format!(r#"<svg><metadata id="{METADATA_ID}">{{not json}}</metadata></svg>"#);
        assert!(parse(&malformed).err().expect("malformed metadata is rejected").contains("metadata JSON"));

        let duplicate = format!(r#"<svg><metadata id="{METADATA_ID}">{{}}</metadata><metadata id="{METADATA_ID}">{{}}</metadata></svg>"#);
        assert!(parse(&duplicate).err().expect("duplicate metadata is rejected").contains("duplicate"));
        assert!(parse("<svg><path></svg>").err().expect("malformed SVG is rejected").contains("invalid SVG"));
        assert!(parse("<svg").err().expect("incomplete SVG root is rejected").contains("invalid SVG"));
    }

    #[test]
    fn future_project_format_metadata_is_rejected() {
        let future_version = crate::project::FORMAT_VERSION + 1;
        let current_version = format!("\"project_format_version\":{}", crate::project::FORMAT_VERSION);
        let newer_version = format!("\"project_format_version\":{future_version}");
        let json = serde_json::to_string(&package()).expect("serialise package").replace(current_version.as_str(), newer_version.as_str());
        let svg = SVG.replace("</svg>", &format!("<metadata id=\"{METADATA_ID}\">{}</metadata></svg>", xml_escape(&json)));
        assert!(parse(&svg).err().expect("future package is rejected").contains("newer project format"));
    }

    #[test]
    fn external_svg_imports_as_paths_and_other_input_remains_text() {
        let Payload::Svg(imports) = parse(SVG).expect("external SVG imports") else { panic!("expected SVG imports") };
        assert_eq!(imports.len(), 1);
        assert!(!imports[0].contours.is_empty());

        let Payload::Text(text) = parse("A label from the clipboard").expect("plain text parses") else { panic!("expected plain text") };
        assert_eq!(text, "A label from the clipboard");
    }

    #[test]
    fn package_size_limit_is_checked_before_parsing() {
        assert!(check_size(MAX_CLIPBOARD_BYTES).is_ok());
        assert!(check_size(MAX_CLIPBOARD_BYTES + 1).is_err());
    }

    #[test]
    fn menu_intent_wins_and_expires_after_one_second() {
        let ctx = Context::default();
        let mut taken = None;
        let _ = ctx.run(egui::RawInput { time: Some(0.0), ..Default::default() }, |ctx| request_paste(ctx, Action::PasteStyle, Some((2.0, 3.0))));
        let _ = ctx.run(
            egui::RawInput { time: Some(0.5), modifiers: Modifiers::COMMAND.plus(Modifiers::SHIFT), events: vec![Event::Paste("first".into())], ..Default::default() },
            |ctx| taken = take_paste(ctx, true),
        );
        assert_eq!(taken, Some((Action::PasteStyle, "first".into(), Some((2.0, 3.0)))));

        let _ = ctx.run(egui::RawInput { time: Some(2.0), ..Default::default() }, |ctx| request_paste(ctx, Action::PasteStyle, None));
        let _ = ctx.run(
            egui::RawInput { time: Some(3.5), modifiers: Modifiers::COMMAND.plus(Modifiers::SHIFT), events: vec![Event::Paste("second".into())], ..Default::default() },
            |ctx| taken = take_paste(ctx, true),
        );
        assert_eq!(taken, Some((Action::PasteInPlace, "second".into(), None)));
    }

    #[test]
    fn paste_takes_only_one_matching_event_and_obeys_the_caller_guard() {
        let ctx = Context::default();
        let mut results = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                modifiers: Modifiers::COMMAND,
                events: vec![Event::Text("keep".into()), Event::Paste("one".into()), Event::Paste("two".into())],
                ..Default::default()
            },
            |ctx| {
                assert_eq!(take_paste(ctx, false), None);
                results.push(take_paste(ctx, true));
                results.push(take_paste(ctx, true));
                assert_eq!(take_paste(ctx, true), None);
            },
        );
        assert_eq!(results, [Some((Action::Paste, "one".into(), None)), Some((Action::Paste, "two".into(), None))]);
        assert!(ctx.input(|input| input.events.iter().any(|event| matches!(event, Event::Text(text) if text == "keep"))));
    }
}

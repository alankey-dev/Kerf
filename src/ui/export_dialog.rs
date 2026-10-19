//! File > Export (spec 25): a modal that edits an `export::Request`, lists the files `export::plan` will
//! write, and hands them back on Export. Writing them, after asking where, happens here at the boundary;
//! Prepare for Design Space writes its plan the same way.

use std::io;
use std::path::{Path, PathBuf};

use egui::{Context, Id, Ui};

use crate::assets::Pixels;
use crate::design::{Design, ShapeKind};
use crate::export::raster::Background;
use crate::export::{self, Format, Planned, Request, Scope, Style};
use crate::machine::MatSize;
use crate::units::{self, Units};

use super::Kerf;
use super::flow_kit::{self as kit, body_text, heading, section_header};

const WIDTH: f32 = 440.0;

pub struct Dialog {
    pub request: Request,
}

impl Dialog {
    pub fn new(units: Units) -> Self {
        Self { request: Request::new(units) }
    }
}

pub enum Outcome {
    Cancel,
    Export(Vec<Planned>),
}

/// What happens to images: Filled SVG and PDF embed their pixels inside the cut line; Outlines and DXF
/// write the cut line alone.
pub const IMAGES_NOTE: &str = "Images export as their cut line only.";
pub const IMAGES_PICTURE_NOTE: &str = "Images export as pictures inside their cut line.";

fn images_note(request: &Request) -> &'static str {
    if request.format.is_raster() {
        return if request.raster.as_made { IMAGES_NOTE } else { IMAGES_PICTURE_NOTE };
    }
    match (request.format, request.style) {
        (Format::Svg | Format::Pdf, Style::Filled) => IMAGES_PICTURE_NOTE,
        _ => IMAGES_NOTE,
    }
}

pub fn show(ctx: &Context, dialog: &mut Dialog, design: &Design, project: &str, mat: MatSize) -> Option<Outcome> {
    let mut outcome = None;
    let modal = egui::Modal::new(Id::new("export-dialog")).frame(kit::card_frame()).show(ctx, |ui| {
        kit::themed(ui, |ui| {
            ui.set_width(WIDTH);
            heading(ui, "Export design");
            ui.add_space(16.0);
            options(ui, &mut dialog.request, design, mat);
            let files = export::plan(design, &dialog.request, project, mat);
            let oversize = files.iter().find_map(Planned::oversize);
            ui.add_space(8.0);
            section_header(ui, "Files");
            file_list(ui, &files, dialog.request.units);
            if design.shapes.iter().any(|s| !s.hidden && matches!(s.kind, ShapeKind::Bitmap { .. })) {
                ui.add_space(4.0);
                body_text(ui, images_note(&dialog.request), true);
            }
            if let Some(message) = &oversize {
                ui.add_space(4.0);
                body_text(ui, message.as_str(), true);
            }
            ui.add_space(16.0);
            let (mut export, mut cancel) = (false, false);
            kit::actions(ui, |ui| {
                export = kit::primary_button(ui, "Export", !files.is_empty() && oversize.is_none()).clicked();
                cancel = kit::secondary_button(ui, "Cancel", true).clicked();
            });
            if export {
                outcome = Some(Outcome::Export(files));
            } else if cancel {
                outcome = Some(Outcome::Cancel);
            }
        });
    });
    if modal.should_close() && outcome.is_none() {
        outcome = Some(Outcome::Cancel);
    }
    outcome
}

fn options(ui: &mut Ui, request: &mut Request, design: &Design, mat: MatSize) {
    row(ui, "Format", |ui| {
        let formats = &[(Format::Svg, "SVG"), (Format::Dxf, "DXF"), (Format::Pdf, "PDF"), (Format::Png, "PNG"), (Format::Jpeg, "JPEG")];
        if let Some(f) = kit::segmented(ui, request.format, formats) {
            request.format = f;
        }
    });
    row(ui, "Scope", |ui| {
        ui.add_enabled_ui(request.format != Format::Pdf, |ui| {
            if let Some(s) = kit::segmented(ui, request.scope(), &[(Scope::Design, "Whole design"), (Scope::PerMat, "One per mat")]) {
                request.scope = s;
            }
        });
    });
    // Raster export always draws as the canvas does, so Style and file units do not apply.
    if !request.format.is_raster() {
        row(ui, "Style", |ui| {
            ui.add_enabled_ui(request.format != Format::Dxf, |ui| {
                if let Some(s) = kit::segmented(ui, request.style, &[(Style::Filled, "Filled"), (Style::Outlines, "Outlines")]) {
                    request.style = s;
                }
            });
        });
        row(ui, "Units", |ui| {
            if let Some(u) = kit::segmented(ui, request.units, &[(Units::Imperial, "Inches"), (Units::Metric, "Millimetres")]) {
                request.units = u;
            }
        });
    }
    kit::toggle_row(ui, "Mirror (for Iron-On)", &mut request.mirror);
    if request.format.is_raster() {
        raster_options(ui, request, design, mat);
        return;
    }
    let note = match (request.format, request.units) {
        (Format::Pdf, _) => Some("A PDF has one page per mat, sized to the mat."),
        (Format::Dxf, Units::Imperial) => Some("DXF coordinates are in inches. Strict R12 readers ignore the file's unit, so choose inches when you import."),
        (Format::Dxf, Units::Metric) => Some("DXF coordinates are in millimetres. Strict R12 readers ignore the file's unit, so choose millimetres when you import."),
        _ => None,
    };
    if let Some(note) = note {
        ui.add_space(4.0);
        body_text(ui, note, true);
    }
}

/// PNG and JPEG options: resolution with a live pixel size, background, margin, JPEG quality and Show
/// as made. JPEG has no transparent background.
fn raster_options(ui: &mut Ui, request: &mut Request, design: &Design, mat: MatSize) {
    if request.format == Format::Jpeg && request.raster.background == Background::Transparent {
        request.raster.background = Background::White;
    }
    row(ui, "Resolution", |ui| {
        let steps = &[(72u32, "72"), (150, "150"), (300, "300"), (600, "600")];
        if let Some(dpi) = kit::segmented(ui, request.raster.dpi, steps) {
            request.raster.dpi = dpi;
        }
    });
    if let Some((w, h)) = export::raster_size(design, request, mat) {
        ui.add_space(2.0);
        body_text(ui, format!("{w} x {h} px"), true);
    }
    row(ui, "Background", |ui| {
        let all = [(Background::Transparent, "Transparent"), (Background::White, "White"), (Background::Mat, "Mat")];
        let options: Vec<(Background, &str)> = all.into_iter().filter(|(b, _)| *b != Background::Transparent || request.format == Format::Png).collect();
        if let Some(b) = kit::segmented(ui, request.raster.background, &options) {
            request.raster.background = b;
        }
    });
    row(ui, "Margin", |ui| {
        if let Some(typed) = units::length_field(ui, "Margin", Some(request.raster.margin_in), request.units, units::FIELD_W) {
            request.raster.margin_in = typed.apply(request.raster.margin_in).max(0.0);
        }
    });
    if request.format == Format::Jpeg {
        row(ui, "Quality", |ui| {
            if let Some(typed) = units::number_field(ui, "Quality", Some(f32::from(request.raster.quality)), "", units::FIELD_W) {
                request.raster.quality = typed.apply(f32::from(request.raster.quality)).clamp(60.0, 100.0).round() as u8;
            }
        });
    }
    kit::toggle_row(ui, "Show as made", &mut request.raster.as_made);
}

fn row(ui: &mut Ui, label: &str, control: impl FnOnce(&mut Ui)) {
    kit::controls(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 20.0), egui::Sense::hover());
        ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, label, super::fonts::medium(13.0), super::tokens::TEXT_PRIMARY);
        control(ui);
    });
}

fn file_list(ui: &mut Ui, files: &[Planned], units: Units) {
    if files.is_empty() {
        body_text(ui, "Nothing to export: every object is hidden or a Guide.", true);
        return;
    }
    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
        for file in files {
            let detail = match file.raster_size() {
                Some((w, h)) => format!("{w} x {h} px, {}", human_bytes(file.estimated_bytes().unwrap_or(0))),
                None => match file.sheets() {
                    [sheet] => units.size(sheet.size_in.0, sheet.size_in.1),
                    sheets => format!("{} pages", sheets.len()),
                },
            };
            ui.horizontal(|ui| {
                body_text(ui, &file.name, false);
                body_text(ui, detail, true);
            });
        }
    });
}

/// A rough byte count for the file list: "~1.4 MB", "~230 KB".
fn human_bytes(n: u64) -> String {
    if n >= 1_000_000 {
        format!("~{:.1} MB", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("~{} KB", n / 1_000)
    } else {
        format!("~{n} B")
    }
}

/// Where the files go: a folder for several, or the file name the user typed for one.
enum Target {
    Folder(PathBuf),
    File(PathBuf),
}

/// Asks where to write `files`, unless `injected` names a folder already. `None` when the user cancels.
fn ask(files: &[Planned], injected: Option<&Path>, always_folder: bool) -> Option<Target> {
    if let Some(dir) = injected {
        return Some(Target::Folder(dir.to_owned()));
    }
    match files {
        [one] if !always_folder => {
            let ext = Path::new(&one.name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_owned();
            rfd::FileDialog::new().add_filter(ext.to_uppercase(), &[ext.as_str()]).set_file_name(&one.name).save_file().map(Target::File)
        }
        _ => rfd::FileDialog::new().set_title("Choose a folder for the exported files").pick_folder().map(Target::Folder),
    }
}

fn write(files: &[Planned], target: &Target, pixels: &Pixels) -> io::Result<PathBuf> {
    match target {
        Target::File(path) => {
            std::fs::write(path, files[0].bytes(pixels))?;
            Ok(path.clone())
        }
        Target::Folder(dir) => {
            for file in files {
                std::fs::write(dir.join(&file.name), file.bytes(pixels))?;
            }
            Ok(dir.clone())
        }
    }
}

impl Kerf {
    pub(super) fn export_dialog(&mut self, ctx: &Context) {
        let Some(dialog) = self.doc.export.as_mut() else { return };
        match show(ctx, dialog, &self.doc.design, &self.doc.title, self.doc.mat) {
            Some(Outcome::Cancel) => self.doc.export = None,
            Some(Outcome::Export(files)) => {
                self.doc.export = None;
                self.write_export(&files, false);
            }
            None => {}
        }
    }

    /// Prepare for Design Space: every mat's SVG and the guide, into one folder.
    pub(super) fn prepare_for_design_space(&mut self) {
        let files = export::design_space(&self.doc.design, &self.doc.title, self.doc.mat);
        self.write_export(&files, true);
    }

    pub(super) fn write_export(&mut self, files: &[Planned], always_folder: bool) {
        let Some(target) = ask(files, self.export_dir.as_deref(), always_folder) else { return };
        let pixels = crate::assets::pixels(self.projects_dir.as_deref().map(crate::assets::dir).as_deref(), files.iter().flat_map(Planned::assets));
        match write(files, &target, &pixels) {
            Ok(at) => tracing::info!(target: "save", files = files.len(), at = ?at.display().to_string(), "export"),
            Err(e) => {
                tracing::error!(target: "error", error = %e, "export failed");
                self.report(format!("Couldn't export: {e}"));
            }
        }
    }
}

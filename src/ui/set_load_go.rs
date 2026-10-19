use egui::{RichText, Ui};

use crate::bridge::ArtType;
use crate::device::DeviceState;
use crate::make_flow::{DonePhase, MakeFlow, MakeStep, Outcome};
use crate::material::{self, PressureAdjust};

use super::flow_kit::{self as kit, Notice, StepState, body_text, card, heading, primary_button, secondary_button, step_card};
use super::fonts;
use super::tokens::TEXT_PRIMARY;
use super::{Nav, Outbox};

const WARNING: &str = "Important: to prevent damage to the machine, mat, or blade, ensure the material selected above matches the material on your mat";
const LASER: &str = "Laser: wear eye protection rated for this wavelength and never leave the machine running unattended.";
const HPGL_PAUSE: &str = "The cutter finishes the moves it has already received.";
const COLUMN_W: f32 = 720.0;

pub fn show(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) -> Nav {
    kit::themed(ui, |ui| {
        egui::ScrollArea::vertical()
            .show(ui, |ui| {
                let w = ui.available_width().min(COLUMN_W);
                let gutter = (ui.available_width() - w) / 2.0;
                ui.add_space(24.0);
                let nav = ui
                    .horizontal_top(|ui| {
                        ui.add_space(gutter);
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            ui.spacing_mut().item_spacing.y = 12.0;
                            heading(ui, &format!("Mat {} of {}", flow.current + 1, flow.mats.len()));
                            screen(ui, flow, out)
                        })
                        .inner
                    })
                    .inner;
                ui.add_space(24.0);
                nav
            })
            .inner
    })
}

fn screen(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) -> Nav {
    match flow.step {
        MakeStep::Cutting => {
            card(ui, |ui| cutting(ui, flow, out));
            Nav::Stay
        }
        MakeStep::Done { outcome, phase } => card(ui, |ui| done(ui, flow, outcome, phase, out)),
        MakeStep::Lost => card(ui, |ui| lost(ui, flow)),
        MakeStep::Print => super::print_step::show(ui, flow, out),
        _ => {
            steps(ui, flow, out);
            Nav::Stay
        }
    }
}

fn steps(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    let current = flow.step.slg_number().unwrap_or(0);
    step_card(ui, 1, "Set Base Material", StepState::of(1, current), |ui| set_material(ui, flow, out));
    let load_title = if current > 2 { "Tools and material loaded" } else { "Load tools and material" };
    step_card(ui, 2, load_title, StepState::of(2, current), |ui| load(ui, flow, out));
    step_card(ui, 3, "Press Go", StepState::of(3, current), |ui| go(ui, flow, out));
}

fn set_material(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    ui.spacing_mut().item_spacing.y = 8.0;
    kit::disclosure(ui, "Browse All Materials", |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        kit::text_field(ui, &mut flow.material_query, "Search All Materials");
        ui.add_space(6.0);
        for m in material::search(&flow.material_query) {
            let detail = format!("({}, {})", m.tool.display_name(), m.clamp.label());
            if kit::select_row(ui, flow.material == Some(m), m.name, &detail).clicked() {
                flow.material = Some(m);
            }
        }
    });
    match (flow.material, flow.profile()) {
        (Some(m), Some(p)) => _ = body_text(ui, format!("Selected: {}  {}", m.name, p.drive(m, flow.pressure).summary()), true),
        (Some(m), None) => _ = body_text(ui, format!("Selected: {}  pressure {}", m.name, m.adjusted_pressure(flow.pressure)), true),
        (None, _) => {}
    }
    kit::controls(ui, |ui| {
        body_text(ui, "Pressure", false);
        let options = PressureAdjust::ALL.map(|p| (p, p.label()));
        if let Some(p) = kit::segmented(ui, flow.pressure, &options) {
            flow.pressure = p;
        }
    });
    ui.add_space(4.0);
    kit::controls(ui, |ui| {
        let ready = flow.material.is_some() && flow.device_state == DeviceState::WaitingOnMaterial;
        if primary_button(ui, "Next", ready).clicked() {
            out.extend(flow.confirm_material());
        }
        if flow.device_state != DeviceState::WaitingOnMaterial && flow.step == MakeStep::Set {
            body_text(ui, "Waiting for machine...", true);
        }
    });
}

fn load(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    ui.spacing_mut().item_spacing.y = 8.0;
    if let Some(p) = flow.profile() {
        let done = flow.step.slg_number().is_some_and(|n| n > 2);
        kit::check_item(ui, done, p.tool.load_line());
        kit::check_item(ui, done, &format!("Place the material at the origin, {}", p.origin.label()));
        ui.add_space(4.0);
        if primary_button(ui, "Continue", flow.device_state == DeviceState::WaitingOnLoad).clicked() {
            out.extend(flow.press_load_unload());
        }
        return;
    }
    for load in flow.tool_loads() {
        kit::check_item(ui, flow.tools_loaded(), &load.label());
    }
    let loaded = !matches!(flow.device_state, DeviceState::WaitingOnLoad) && flow.tools_loaded();
    kit::check_item(ui, loaded, "Load mat and press Load/Unload button");
    ui.add_space(4.0);
    if primary_button(ui, "Load/Unload", flow.device_state == DeviceState::WaitingOnLoad).clicked() {
        out.extend(flow.press_load_unload());
    }
}

fn go(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let open = flow.profile().map(|p| (p.is_laser(), p.name.clone()));
    match &open {
        Some((_, name)) => body_text(ui, format!("Press Go to send the mat to {name}."), false),
        None => body_text(ui, "Press flashing Go button.", false),
    };
    if let Some((true, _)) = open {
        kit::notice(ui, Notice::Warning, LASER);
    }
    kit::notice(ui, Notice::Warning, WARNING);
    ui.add_space(4.0);
    kit::controls(ui, |ui| {
        let ready = flow.device_state == DeviceState::ReadyToGo;
        if primary_button(ui, "Go", ready).clicked() {
            out.extend(flow.press_go());
        }
        if open.is_some() && secondary_button(ui, "Save to file", ready).clicked() {
            tracing::info!(target: "cmd", mat = flow.current + 1, "save to file");
            out.extend(flow.save_to_file());
        }
        pause_and_cancel(ui, flow, out);
    });
}

fn pause_and_cancel(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    let paused = flow.device_state == DeviceState::Paused;
    let can_pause = paused || flow.device_state == DeviceState::GoPressed;
    if secondary_button(ui, if paused { "Resume" } else { "Pause" }, can_pause).clicked() {
        out.extend(flow.press_pause());
    }
    if secondary_button(ui, "Cancel", true).clicked() {
        out.extend(flow.cancel());
    }
}

fn cutting(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    ui.spacing_mut().item_spacing.y = 12.0;
    if let Some(p) = flow.profile() {
        let (sent, total) = flow.sent.unwrap_or((0, 0));
        heading(ui, if matches!(p.tool, crate::output::ToolKind::DragKnife { .. }) { "Cutting" } else { "Drawing" });
        kit::progress(ui, if total == 0 { 0.0 } else { sent as f32 / total as f32 });
        body_text(ui, format!("Sent {sent} of {total} lines"), true);
        let paused = flow.device_state == DeviceState::Paused;
        match &flow.tool_change {
            Some(words) => kit::notice(ui, Notice::Warning, words),
            None if paused && p.protocol == crate::output::Protocol::Hpgl => kit::notice(ui, Notice::Warning, HPGL_PAUSE),
            None => kit::notice(ui, Notice::Warning, "Keep hands clear of the head while it moves."),
        }
        kit::controls(ui, |ui| pause_and_cancel(ui, flow, out));
        return;
    }
    let progress = flow.progress.as_ref();
    heading(ui, progress.map_or("Preparing", |p| operation_label(p.current_operation)));
    kit::progress(ui, progress.map_or(0.0, |p| p.current_percentage as f32 / 100.0));
    if let Some(p) = progress {
        let remaining = p.estimated_seconds_left.round();
        body_text(ui, format!("{} of {} passes, {remaining} s remaining", p.cut.current_pass, p.cut.total_num_passes), true);
    }
    kit::notice(ui, Notice::Warning, "IMPORTANT: Do NOT unload the mat.");
    kit::controls(ui, |ui| pause_and_cancel(ui, flow, out));
}

fn operation_label(art: ArtType) -> &'static str {
    match art {
        ArtType::Cut | ArtType::Wave | ArtType::Perforate => "Cutting",
        ArtType::Draw => "Drawing",
        ArtType::Score => "Scoring",
        ArtType::Deboss => "Debossing",
        ArtType::Engrave => "Engraving",
        ArtType::MidasFine | ArtType::MidasRegular | ArtType::MidasBold => "Foiling",
        ArtType::None => "Preparing",
    }
}

fn done(ui: &mut Ui, flow: &mut MakeFlow, outcome: Outcome, phase: DonePhase, out: &mut Outbox) -> Nav {
    ui.spacing_mut().item_spacing.y = 12.0;
    heading(
        ui,
        match outcome {
            Outcome::Complete => "Complete",
            Outcome::Cancelled => "Cancelled",
        },
    );
    match phase {
        DonePhase::Checking => check_your_cut(ui, flow, out),
        DonePhase::Unloading => unload(ui, flow, out),
        DonePhase::Unloaded => return unloaded(ui, flow, out),
    }
    Nav::Stay
}

fn check_your_cut(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    ui.label(RichText::new("Check your cut").font(fonts::semibold(14.0)).color(TEXT_PRIMARY));
    body_text(ui, "Before unloading, check that the cut went all the way through.", true);
    kit::controls(ui, |ui| {
        if primary_button(ui, "Looks good", true).clicked() {
            flow.looks_good();
        }
        if secondary_button(ui, "Cut more", true).clicked() {
            out.extend(flow.cut_more());
        }
    });
}

fn unload(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) {
    let label = if flow.profile().is_some() { "Material removed" } else { "Unload mat" };
    if primary_button(ui, label, flow.device_state == DeviceState::WaitingOnUnload).clicked() {
        out.extend(flow.press_load_unload());
    }
}

fn lost(ui: &mut Ui, flow: &MakeFlow) -> Nav {
    ui.spacing_mut().item_spacing.y = 12.0;
    match &flow.failure {
        Some(why) => {
            heading(ui, "Device stopped");
            kit::notice(ui, Notice::Warning, &format!("{why} This mat cannot continue. Check the device, then start Make again."));
        }
        None => {
            heading(ui, "Machine disconnected");
            kit::notice(ui, Notice::Warning, "The machine stopped responding, so this mat cannot continue. Check that it is on and connected, then start Make again.");
        }
    }
    let mut nav = Nav::Stay;
    kit::controls(ui, |ui| {
        if primary_button(ui, "Back to canvas", true).clicked() {
            nav = Nav::Close;
        }
    });
    nav
}

fn unloaded(ui: &mut Ui, flow: &mut MakeFlow, out: &mut Outbox) -> Nav {
    body_text(ui, "Mat unloaded.", true);
    let mut nav = Nav::Stay;
    kit::controls(ui, |ui| {
        if flow.has_next_mat() {
            if primary_button(ui, "Next Mat", true).clicked() {
                out.extend(flow.next_mat());
            }
        } else if primary_button(ui, "Back to canvas", true).clicked() {
            nav = Nav::Close;
        }
        if secondary_button(ui, "Rerun", true).clicked() {
            out.extend(flow.rerun());
        }
    });
    nav
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, Operation, ShapeKind};
    use crate::pens::{Pen, PenSize};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn the_load_step_lists_one_line_per_pen_in_plot_order() {
        let mut design = Design::default();
        for name in ["Black", "Wine", "Black"] {
            let id = design.add_shape(ShapeKind::Square);
            let shape = design.shape_mut(id).unwrap();
            shape.set_operation(Operation::Pen);
            shape.set_pen(Pen::named(name, PenSize::Fine));
        }
        let ids: Vec<u64> = design.shapes.iter().map(|s| s.id).collect();
        design.attach(&ids);
        let mut flow = MakeFlow::new(design, None);
        flow.material = Some(&material::MATERIALS[0]);
        flow.step = MakeStep::Load;
        let mut harness = Harness::builder().with_size(egui::vec2(520.0, 420.0)).wgpu().build_ui_state(
            |ui, flow: &mut MakeFlow| {
                ui.ctx().set_visuals(egui::Visuals::light());
                // The app installs its theme before the first frame; a bare harness can only do so in it.
                if ui.ctx().cumulative_pass_nr() == 0 {
                    super::super::install_theme(ui.ctx());
                    return;
                }
                show(ui, flow, &mut Vec::new());
            },
            flow,
        );
        harness.run();
        let lines: Vec<String> = harness.query_all_by_label_contains("Pen in Clamp A").map(|n| n.value().unwrap_or_default()).collect();
        assert_eq!(lines, ["Load Black Fine Point Pen in Clamp A", "Load Wine Fine Point Pen in Clamp A"]);
    }
}

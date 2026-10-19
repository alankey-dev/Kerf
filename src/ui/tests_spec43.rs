//! Spec 43: Create AI. Every test runs through `FakeProvider` and an in-memory key store, so none
//! touches the network or the real keychain.

use egui_kittest::kittest::Queryable;

use super::Kerf;
use super::commands::Command;
use super::test_harness::{Fixture, launch, open_blank_tab, save_snapshot, snapshot_harness};
use super::tests_spec28::wait_for;
use crate::ai::{AiError, Candidate};
use crate::edit::AppCommand;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 860.0);
const PROMPT: &str = "a dinosaur with a unicorn horn riding a skateboard";
const KEY: &str = "sk-ant-api03-LEAKCHECK-7f3a9c";
const OPENAI_KEY: &str = "sk-proj-LEAKCHECK-openai-0d1e2f";

fn click(fixture: &mut Fixture, label: &str) {
    fixture.get_by_label(label).click();
    fixture.step();
    fixture.step();
}

fn open_panel(fixture: &mut Fixture) {
    click(fixture, "Create AI");
    fixture.run();
}

/// A blank canvas with the panel open and a key saved.
fn ready() -> Fixture {
    let mut fixture = snapshot_harness(SIZE);
    fixture.state_mut().create_ai.save_key_for_test("sk-ant-test");
    open_panel(&mut fixture);
    fixture
}

fn type_prompt(fixture: &mut Fixture, prompt: &str) {
    fixture.get_by_label("Prompt").click();
    fixture.run();
    fixture.get_by_label("Prompt").type_text(prompt);
    fixture.run();
}

/// Opens the settings window from the panel, by its setup button or its footer link.
fn open_settings(fixture: &mut Fixture) {
    let way_in = if fixture.query_by_label("Set up Create AI").is_some() { "Set up Create AI" } else { "Create AI settings" };
    click(fixture, way_in);
    fixture.run();
    fixture.get_by_label("Close Create AI settings");
}

/// Types `text` into the field labelled `label`.
fn type_into(fixture: &mut Fixture, label: &str, text: &str) {
    fixture.get_by_label(label).click();
    fixture.run();
    fixture.get_by_label(label).type_text(text);
    fixture.run();
}

fn close_settings(fixture: &mut Fixture) {
    click(fixture, "Done");
    fixture.run();
    assert!(fixture.query_by_label("Close Create AI settings").is_none());
}

/// Picks the provider `label` in the settings window, opening and closing it around the pick.
fn pick_service(fixture: &mut Fixture, label: &str) {
    open_settings(fixture);
    click(fixture, label);
    fixture.run();
    close_settings(fixture);
}

fn finished(app: &Kerf) -> bool {
    !app.create_ai.running()
}

fn generate(fixture: &mut Fixture) {
    click(fixture, "Generate");
    wait_for(fixture, "the generation finishes", finished);
}

#[test]
fn with_no_key_the_panel_says_what_is_sent_and_the_settings_window_takes_a_key() {
    let mut fixture = snapshot_harness(SIZE);
    open_panel(&mut fixture);
    assert!(fixture.query_by_label("Prompt").is_none(), "no prompt until there is a key");
    save_snapshot(&mut fixture, "43-no-key.png");

    open_settings(&mut fixture);
    fixture.get_by_label("Get a key from the Anthropic Console");
    fixture.get_by_label("No key yet. Paste yours here; Kerf keeps it in your system keychain, never in its settings or your projects.");
    type_into(&mut fixture, "Anthropic API key", "sk-ant-pasted");
    save_snapshot(&mut fixture, "43-settings.png");
    click(&mut fixture, "Save key");
    fixture.run();
    assert!(fixture.state_mut().create_ai.has_key());
    fixture.get_by_label("A key is saved in your system keychain.");
    close_settings(&mut fixture);
    fixture.get_by_label("Prompt");
    fixture.get_by_label("Anthropic (Claude), claude-opus-5-5");

    open_settings(&mut fixture);
    click(&mut fixture, "Remove key");
    fixture.run();
    close_settings(&mut fixture);
    fixture.get_by_label("Set up Create AI");
}

#[test]
fn the_command_palette_opens_the_settings_window_and_escape_closes_it() {
    let mut fixture = snapshot_harness(SIZE);
    let ctx = fixture.ctx.clone();
    fixture.state_mut().run(Command::App(AppCommand::AiSettings), &ctx);
    fixture.run();
    fixture.get_by_label("Close Create AI settings");
    super::test_harness::key(&mut fixture, egui::Key::Escape, egui::Modifiers::NONE);
    fixture.run();
    assert!(fixture.query_by_label("Close Create AI settings").is_none());
}

#[test]
fn load_models_lists_the_providers_models_and_a_click_picks_one() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    open_settings(&mut fixture);
    click(&mut fixture, "Load models");
    wait_for(&mut fixture, "the model list arrives", |app| !app.create_ai.listing());
    fixture.get_by_label("2 of 2 models. Click one to use it.");
    click(&mut fixture, "fake-small");
    fixture.run();
    assert_eq!(fixture.state().create_ai.ai.model(crate::ai::Service::Anthropic), "fake-small");
    assert_eq!(crate::settings::Settings::load(&fixture.projects).ai.model(crate::ai::Service::Anthropic), "fake-small", "saved at once");
    click(&mut fixture, "Use the default");
    fixture.run();
    assert_eq!(fixture.state().create_ai.ai.model(crate::ai::Service::Anthropic), "claude-opus-5-5");

    *fake.models.lock().unwrap() = Some(Err(AiError::KeyRejected));
    click(&mut fixture, "Load models");
    wait_for(&mut fixture, "the failed list arrives", |app| !app.create_ai.listing());
    fixture.get_by_label("Your API key was rejected.");
}

#[test]
fn generate_shows_four_tiles_and_insert_adds_one_named_shape_as_one_undo_step() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    type_prompt(&mut fixture, PROMPT);
    click(&mut fixture, "Line art");
    generate(&mut fixture);
    for i in 1..=4 {
        fixture.get_by_label(&format!("Design {i}"));
    }
    assert_eq!(fixture.state().create_ai.ready_tiles(), 4);
    let asked = fake.calls();
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].prompt.as_str(), asked[0].style), (PROMPT, crate::ai::Style::LineArt));
    save_snapshot(&mut fixture, "43-results.png");

    let before = fixture.state().design.shapes.len();
    click(&mut fixture, "Design 1");
    click(&mut fixture, "Insert");
    fixture.run();
    let design = &fixture.state().design;
    assert_eq!(design.shapes.len(), before + 1, "one shape");
    let shape = design.shapes.last().unwrap();
    assert_eq!(shape.name.as_deref(), Some("Dinosaur skateboard"), "named from the prompt's design");
    assert_eq!(shape.operation, crate::design::Operation::BasicCut);
    assert_eq!(shape.fill, egui::Color32::BLACK);
    assert!((shape.width_in - 4.0).abs() < 1e-3, "4 in wide: {}", shape.width_in);
    let (cx, cy) = fixture.state().view.centre_in();
    assert!((shape.x_in + shape.width_in / 2.0 - cx).abs() < 1e-3 && (shape.y_in + shape.height_in / 2.0 - cy).abs() < 1e-3, "at the centre of the view");

    let ctx = fixture.ctx.clone();
    fixture.state_mut().run(Command::App(AppCommand::Undo), &ctx);
    fixture.run();
    assert_eq!(fixture.state().design.shapes.len(), before, "one Undo removes it");
}

#[test]
fn more_like_this_asks_for_variations_of_the_selected_tile_and_regenerate_asks_again() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    type_prompt(&mut fixture, PROMPT);
    generate(&mut fixture);
    click(&mut fixture, "Design 2");
    click(&mut fixture, "More like this");
    wait_for(&mut fixture, "the variations finish", finished);
    click(&mut fixture, "Regenerate");
    wait_for(&mut fixture, "the regeneration finishes", finished);
    let calls = fake.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].variations_of.as_ref().map(|c| c.title.as_str()), Some("Star skateboard"));
    assert!(calls[2].variations_of.is_none() && calls[2].prompt == PROMPT);
}

#[test]
fn a_refusal_shows_its_explanation_and_keeps_the_prompt() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    fake.then(Err(AiError::Refused { explanation: Some("it asks for a trademarked character.".into()) }));
    type_prompt(&mut fixture, "a famous cartoon mouse");
    generate(&mut fixture);
    fixture.get_by_label("Kerf couldn't make this: it asks for a trademarked character.");
    assert_eq!(fixture.state().create_ai.prompt(), "a famous cartoon mouse", "the prompt stays to rephrase");
    save_snapshot(&mut fixture, "43-refusal.png");
}

#[test]
fn a_rejected_key_says_so_and_keeps_the_prompt() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    fake.then(Err(AiError::KeyRejected));
    fake.then(Err(AiError::Busy { retry_after_s: Some(12) }));
    type_prompt(&mut fixture, PROMPT);
    generate(&mut fixture);
    fixture.get_by_label("Your API key was rejected.");
    assert_eq!(fixture.state().create_ai.prompt(), PROMPT);
    generate(&mut fixture);
    fixture.get_by_label("Anthropic is busy. Try again in 12 seconds.");
}

#[test]
fn a_failed_tile_says_why_and_the_others_still_insert() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    let text = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><text x="10" y="50">Hi</text></svg>"#;
    let mut first = crate::ai::FakeProvider::clean();
    first[0] = Candidate::drawn("Words", text);
    fake.then(Ok(first));
    fake.then(Ok(vec![Candidate::drawn("Still words", text)]));
    type_prompt(&mut fixture, PROMPT);
    generate(&mut fixture);
    assert_eq!(fixture.state().create_ai.ready_tiles(), 3);
    click(&mut fixture, "Insert");
    fixture.run();
    assert_eq!(fixture.state().design.shapes.last().and_then(|s| s.name.as_deref()), Some("Star skateboard"), "the first ready tile is chosen");
}

#[test]
fn cancel_returns_the_panel_to_idle_and_a_late_result_is_dropped() {
    let mut fixture = ready();
    let fake = fixture.state_mut().create_ai.use_fake();
    let release = fake.hold();
    type_prompt(&mut fixture, PROMPT);
    click(&mut fixture, "Generate");
    fixture.get_by_label("Cancel");
    assert_eq!(fixture.query_all_by_label("Drawing").count(), 4, "four placeholder tiles");
    assert!(fixture.state().create_ai.running());
    click(&mut fixture, "Cancel");
    assert!(!fixture.state().create_ai.running());
    fixture.run();
    fixture.get_by_label("Generate");
    assert!(fixture.query_by_label("Cancel").is_none());

    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while fake.calls().is_empty() && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    fixture.run_steps(5);
    assert_eq!(fixture.state().create_ai.ready_tiles(), 0, "the late answer is dropped");
    assert!(fixture.query_by_label("Design 1").is_none());
}

#[test]
fn history_survives_a_restart_and_inserts_offline() {
    let mut fixture = ready();
    type_prompt(&mut fixture, PROMPT);
    generate(&mut fixture);
    let saved = crate::ai::history::load(&fixture.projects);
    assert_eq!(saved.len(), 4, "every cut-ready design is kept");
    assert!(saved.iter().all(|e| e.prompt == PROMPT));

    let root = std::mem::replace(&mut fixture.root, tempfile::tempdir().unwrap());
    drop(fixture);
    let mut relaunched = open_blank_tab(launch(root, SIZE));
    let fake = relaunched.state_mut().create_ai.use_fake();
    for _ in 0..4 {
        fake.then(Err(AiError::Network));
    }
    open_panel(&mut relaunched);
    click(&mut relaunched, "History");
    relaunched.run();
    click(&mut relaunched, "Insert Cat skateboard");
    relaunched.run();
    assert_eq!(relaunched.state().design.shapes.last().and_then(|s| s.name.as_deref()), Some("Cat skateboard"));
    assert!(fake.calls().is_empty(), "nothing went to the provider");
}

/// Every file under the test's app data, recursively.
fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn the_key_never_reaches_settings_the_debug_log_or_a_project_file() {
    let log_dir = tempfile::tempdir().unwrap();
    let log = log_dir.path().join("debug.log");
    let subscriber = crate::debug_log::subscriber(&log, "trace").unwrap();
    // A second live dispatcher makes every callsite consult them all, so another test's thread cannot
    // cache the app's lines as unwanted (as `tests_spec34::pattern` explains).
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let mut fixture = tracing::subscriber::with_default(subscriber, || {
        let mut fixture = snapshot_harness(SIZE);
        open_panel(&mut fixture);
        open_settings(&mut fixture);
        type_into(&mut fixture, "Anthropic API key", KEY);
        click(&mut fixture, "Show key");
        click(&mut fixture, "Save key");
        fixture.run();
        type_into(&mut fixture, "Model name", "-test");
        click(&mut fixture, "OpenAI");
        fixture.run();
        type_into(&mut fixture, "OpenAI API key", OPENAI_KEY);
        click(&mut fixture, "Save key");
        close_settings(&mut fixture);
        type_prompt(&mut fixture, PROMPT);
        generate(&mut fixture);
        click(&mut fixture, "Insert");
        let ctx = fixture.ctx.clone();
        fixture.state_mut().run(Command::App(AppCommand::Save), &ctx);
        fixture.run();
        // Careless log lines with the keys themselves, as a request's headers might carry them.
        let provider = crate::ai::anthropic::AnthropicProvider::new(KEY.into(), "claude-opus-5-5".into());
        tracing::debug!(target: "ai", ?provider, header = KEY, "request");
        let provider = crate::ai::openai::OpenAiProvider::openai(OPENAI_KEY.into(), "gpt-6-astra".into());
        tracing::debug!(target: "ai", ?provider, authorization = %format!("Bearer {OPENAI_KEY}"), "request");
        fixture
    });
    let settings = std::fs::read_to_string(fixture.root.path().join("settings.json")).expect("the model change saved settings");
    assert!(settings.contains("claude-opus-5-5-test") && settings.contains("\"service\": \"OpenAi\""), "{settings}");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("ai: key saved") && text.contains("ai: insert") && text.contains("ai: request"), "logging was on:\n{text}");
    let mut written = vec![log.clone()];
    files(fixture.root.path(), &mut written);
    assert!(
        written.iter().any(|p| p.extension().is_some_and(|x| x == "json") && p.parent().is_some_and(|d| d.ends_with("projects"))),
        "a project was saved: {written:?}"
    );
    assert!(written.iter().any(|p| p.parent().is_some_and(|d| d.ends_with("ai"))), "history was written: {written:?}");
    for path in &written {
        let bytes = std::fs::read(path).unwrap();
        let found = bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()) || bytes.windows(9).any(|w| w == b"LEAKCHECK");
        assert!(!found, "the key leaked into {}", path.display());
    }
    assert!(fixture.state_mut().create_ai.has_key(), "the key is still there, in the key store");
}

#[test]
fn each_provider_asks_for_its_own_key_and_the_choice_is_saved() {
    let mut fixture = snapshot_harness(SIZE);
    fixture.state_mut().create_ai.save_key_for_test("sk-ant-test");
    open_panel(&mut fixture);
    fixture.get_by_label("Prompt");
    pick_service(&mut fixture, "OpenRouter");
    assert!(fixture.query_by_label("Prompt").is_none(), "OpenRouter needs its own key");
    open_settings(&mut fixture);
    fixture.get_by_label("Get a key from OpenRouter's settings");
    type_into(&mut fixture, "OpenRouter API key", "sk-or-v1-test");
    click(&mut fixture, "Save key");
    close_settings(&mut fixture);
    type_prompt(&mut fixture, PROMPT);
    assert!(fixture.get_by_label("Generate").is_disabled(), "OpenRouter has no default model");
    fixture.get_by_label("Choose a model in Create AI settings.");
    fixture.get_by_label("OpenRouter, no model chosen");
    open_settings(&mut fixture);
    type_into(&mut fixture, "Model name", "anthropic/claude-opus-5.5");
    close_settings(&mut fixture);
    generate(&mut fixture);
    assert_eq!(fixture.state().create_ai.ready_tiles(), 4);
    let settings = crate::settings::Settings::load(&fixture.projects).ai;
    assert_eq!((settings.service, settings.model(crate::ai::Service::OpenRouter)), (crate::ai::Service::OpenRouter, "anthropic/claude-opus-5.5"));
    pick_service(&mut fixture, "Anthropic (Claude)");
    fixture.get_by_label("Prompt");
    assert_eq!(fixture.state().create_ai.ai.model(crate::ai::Service::Anthropic), "claude-opus-5-5", "each service keeps its own model");
}

#[test]
fn a_local_model_needs_no_key_only_a_name_and_an_address() {
    let mut fixture = snapshot_harness(SIZE);
    open_panel(&mut fixture);
    open_settings(&mut fixture);
    click(&mut fixture, "Local model");
    fixture.run();
    assert!(fixture.query_by_label("Save key").is_none(), "no key for a local server");
    assert_eq!(fixture.get_by_label("Local server address").value().as_deref(), Some(crate::settings::DEFAULT_LOCAL_ADDRESS));
    type_into(&mut fixture, "Model name", "qwen3");
    save_snapshot(&mut fixture, "43-local.png");
    close_settings(&mut fixture);
    type_prompt(&mut fixture, PROMPT);
    let fake = fixture.state_mut().create_ai.use_fake();
    fake.then(Err(AiError::Network));
    generate(&mut fixture);
    fixture.get_by_label("Kerf couldn't reach your local model server.");
    assert_eq!(crate::settings::Settings::load(&fixture.projects).ai.model(crate::ai::Service::Local), "qwen3");
}

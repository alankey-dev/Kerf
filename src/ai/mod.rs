//! Create AI (spec 43): the user's prompt goes to a language model of their choosing ([`Service`]):
//! Claude, OpenAI or OpenRouter with their own API key, or a model served on their own machine. The
//! model writes four candidate designs as SVG, and each passes through [`validate`] before a tile shows
//! it. Kerf sends only the prompt and the chosen style and detail; never the design, a project or a file.
//!
//! Generation runs on its own thread ([`Job`]), so the panel never blocks. Dropping a job is Cancel:
//! its answer, should one still arrive, goes nowhere.

pub mod anthropic;
pub mod history;
pub mod keys;
pub mod openai;
pub mod validate;
mod wire;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use serde::{Deserialize, Serialize};

use crate::design::Shape;

/// The longest prompt the panel takes.
pub const PROMPT_MAX_CHARS: usize = 400;
/// Candidates per generation.
pub const CANDIDATES: usize = 4;
/// Every generated design is placed and validated at this width.
pub const WIDTH_IN: f32 = 4.0;

/// Who draws the designs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Service {
    /// Claude, through Anthropic's Messages API.
    #[default]
    Anthropic,
    /// OpenAI's Responses API.
    OpenAi,
    /// OpenRouter's OpenAI-compatible Chat Completions, which reaches many vendors' models.
    OpenRouter,
    /// Any OpenAI-compatible server the user runs, such as Ollama or LM Studio. No key needed.
    Local,
}

impl Service {
    pub const ALL: [Service; 4] = [Service::Anthropic, Service::OpenAi, Service::OpenRouter, Service::Local];

    /// The picker's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenAi => "OpenAI",
            Self::OpenRouter => "OpenRouter",
            Self::Local => "Local model",
        }
    }

    /// Who the request goes to, mid-sentence.
    pub fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAi => "OpenAI",
            Self::OpenRouter => "OpenRouter",
            Self::Local => "your local model server",
        }
    }

    /// [`Service::name`] at the start of a sentence.
    fn subject(self) -> &'static str {
        match self {
            Self::Local => "Your local model server",
            other => other.name(),
        }
    }

    pub fn needs_key(self) -> bool {
        self != Self::Local
    }

    /// The environment variable that overrides the saved key.
    pub fn env_var(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::OpenRouter => Some("OPENROUTER_API_KEY"),
            Self::Local => None,
        }
    }

    /// Where the user gets a key, and what one starts with.
    pub fn key_page(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Anthropic => Some(("https://console.anthropic.com/settings/keys", "the Anthropic Console")),
            Self::OpenAi => Some(("https://platform.openai.com/api-keys", "the OpenAI dashboard")),
            Self::OpenRouter => Some(("https://openrouter.ai/settings/keys", "OpenRouter's settings")),
            Self::Local => None,
        }
    }

    pub fn key_hint(self) -> &'static str {
        match self {
            Self::Anthropic => "sk-ant-...",
            Self::OpenRouter => "sk-or-...",
            Self::OpenAi | Self::Local => "sk-...",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    /// Solid, one piece.
    #[default]
    Silhouette,
    /// Outlines drawn as filled bands at least the minimum feature size wide.
    LineArt,
    /// Bridged islands for stencil film: no enclosed hole.
    Stencil,
    /// Solid, with an offset border around the whole design.
    Sticker,
}

impl Style {
    pub const ALL: [Style; 4] = [Style::Silhouette, Style::LineArt, Style::Stencil, Style::Sticker];

    pub fn label(self) -> &'static str {
        match self {
            Self::Silhouette => "Silhouette",
            Self::LineArt => "Line art",
            Self::Stencil => "Stencil",
            Self::Sticker => "Sticker",
        }
    }

    /// What the style asks of a design, as the panel's tooltip and the request to Claude say it.
    pub fn brief(self) -> &'static str {
        match self {
            Self::Silhouette => "a solid silhouette in one piece",
            Self::LineArt => "line art: outlines drawn as filled bands, never thinner than the minimum feature size",
            Self::Stencil => "a stencil: every hole bridged to the outside, so no piece of the film falls out",
            Self::Sticker => "a solid sticker with an even offset border around the whole design",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Detail {
    Simple,
    #[default]
    Medium,
    Detailed,
}

impl Detail {
    pub const ALL: [Detail; 3] = [Detail::Simple, Detail::Medium, Detail::Detailed];

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "Simple",
            Self::Medium => "Medium",
            Self::Detailed => "Detailed",
        }
    }

    /// The smallest island, across its narrower side, that still weeds cleanly at [`WIDTH_IN`] wide.
    pub fn min_feature_in(self) -> f32 {
        match self {
            Self::Simple => 0.25,
            Self::Medium => 0.15,
            Self::Detailed => 0.08,
        }
    }

    /// The most `<path>` elements a design may use.
    pub fn path_budget(self) -> usize {
        match self {
            Self::Simple => 8,
            Self::Medium => 20,
            Self::Detailed => 40,
        }
    }

    /// The most path nodes (segment ends) a design may use, across all its paths.
    pub fn node_budget(self) -> usize {
        match self {
            Self::Simple => 600,
            Self::Medium => 1500,
            Self::Detailed => 3000,
        }
    }
}

/// One design Claude drew. `piece` is the validated, cut-ready shape (at the origin, [`WIDTH_IN`]
/// wide); `problem` says why there is none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub title: String,
    pub svg: String,
    pub piece: Option<Shape>,
    pub problem: Option<String>,
}

impl Candidate {
    pub fn drawn(title: impl Into<String>, svg: impl Into<String>) -> Self {
        Self { title: title.into(), svg: svg.into(), piece: None, problem: None }
    }

    /// Runs the candidate through [`validate::check`], filling `piece` or `problem`.
    pub fn validated(mut self, style: Style, detail: Detail) -> Self {
        match validate::check(&self.svg, style, detail) {
            Ok(piece) => (self.piece, self.problem) = (Some(piece), None),
            Err(reasons) => (self.piece, self.problem) = (None, Some(reasons.join("; "))),
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenerateRequest {
    pub prompt: String,
    pub style: Style,
    pub detail: Detail,
    /// More like this: four variations of this design.
    pub variations_of: Option<Candidate>,
    /// The one retry for a candidate that failed validation; its `problem` holds the reasons quoted back.
    pub fix: Option<Candidate>,
}

impl GenerateRequest {
    pub fn new(prompt: &str, style: Style, detail: Detail) -> Self {
        Self { prompt: prompt.trim().to_owned(), style, detail, variations_of: None, fix: None }
    }

    /// How many candidates the request asks for.
    pub fn count(&self) -> usize {
        if self.fix.is_some() { 1 } else { CANDIDATES }
    }
}

/// Why a generation produced nothing. None of these clears the prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum AiError {
    /// 401: the key is wrong or revoked.
    KeyRejected,
    /// 429 or 529, with the `retry-after` seconds when the API sent them.
    Busy { retry_after_s: Option<u64> },
    /// No connection, DNS, TLS or a timeout.
    Network,
    /// `stop_reason: "refusal"`, with `stop_details.explanation` when present.
    Refused { explanation: Option<String> },
    /// The reply arrived but was not the agreed JSON, or was cut off.
    Unreadable(String),
    /// Any other HTTP error.
    Api { status: u16, message: String },
}

impl AiError {
    /// The panel's sentence for this error.
    pub fn message(&self, service: Service) -> String {
        let (name, subject) = (service.name(), service.subject());
        match self {
            Self::KeyRejected => "Your API key was rejected.".into(),
            Self::Busy { retry_after_s: Some(s) } => format!("{subject} is busy. Try again in {s} seconds."),
            Self::Busy { retry_after_s: None } => format!("{subject} is busy. Try again in a moment."),
            Self::Network => format!("Kerf couldn't reach {name}."),
            Self::Refused { explanation: Some(why) } => format!("Kerf couldn't make this: {why}"),
            Self::Refused { explanation: None } => "Kerf couldn't make this: the model declined the prompt. Try rephrasing it.".into(),
            Self::Unreadable(why) => format!("Kerf couldn't read {name}'s reply: {why}"),
            Self::Api { status, message } => format!("{subject} returned an error ({status}): {message}"),
        }
    }
}

/// Where designs come from. `AnthropicProvider` calls the Messages API; tests use `FakeProvider`,
/// so no test touches the network.
pub trait Provider: Send + Sync {
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<Candidate>, AiError>;

    /// The models the service offers for this kind of work, for the settings window's list.
    fn models(&self) -> Result<Vec<String>, AiError>;
}

/// A model list being fetched on its own thread, for the settings window. Dropping it discards the answer.
pub struct Listing {
    done: Receiver<Result<Vec<String>, AiError>>,
}

impl Listing {
    pub fn start(provider: Arc<dyn Provider>) -> Listing {
        let (send, done) = channel();
        std::thread::Builder::new().name("create-ai-models".into()).spawn(move || _ = send.send(provider.models())).expect("the model list thread starts");
        Listing { done }
    }

    /// The list, once.
    pub fn try_result(&self) -> Option<Result<Vec<String>, AiError>> {
        self.done.try_recv().ok()
    }
}

/// One generation, start to finish: ask for candidates, validate each, and ask once more, in parallel,
/// for each that failed, quoting the validator's reasons. Always [`GenerateRequest::count`] candidates.
pub fn generate(provider: &dyn Provider, request: &GenerateRequest) -> Result<Vec<Candidate>, AiError> {
    let (style, detail) = (request.style, request.detail);
    let mut drawn: Vec<Candidate> = provider.generate(request)?.into_iter().take(request.count()).map(|c| c.validated(style, detail)).collect();
    while drawn.len() < request.count() {
        drawn.push(Candidate { problem: Some("Claude sent fewer designs than asked for.".into()), ..Candidate::default() });
    }
    std::thread::scope(|scope| {
        let retries: Vec<_> = drawn
            .iter()
            .enumerate()
            .filter(|(_, c)| c.piece.is_none() && !c.svg.is_empty())
            .map(|(i, c)| {
                let fix = GenerateRequest { fix: Some(c.clone()), ..request.clone() };
                (i, scope.spawn(move || provider.generate(&fix)))
            })
            .collect();
        for (i, retry) in retries {
            let fixed = retry.join().ok().and_then(Result::ok).and_then(|c| c.into_iter().next()).map(|c| c.validated(style, detail));
            match fixed {
                Some(c) if c.piece.is_some() => drawn[i] = c,
                Some(c) => drawn[i].problem = c.problem,
                None => {}
            }
        }
    });
    tracing::info!(target: "ai", asked = request.count(), ready = drawn.iter().filter(|c| c.piece.is_some()).count(), "generated");
    Ok(drawn)
}

/// A generation running on its own thread. Dropping it is Cancel: the thread's answer is discarded.
pub struct Job {
    done: Receiver<Result<Vec<Candidate>, AiError>>,
}

impl Job {
    pub fn start(provider: Arc<dyn Provider>, request: GenerateRequest) -> Job {
        let (send, done) = channel();
        tracing::info!(target: "ai", style = ?request.style, detail = ?request.detail, variation = request.variations_of.is_some(), chars = request.prompt.chars().count(), "generate");
        std::thread::Builder::new().name("create-ai".into()).spawn(move || _ = send.send(generate(provider.as_ref(), &request))).expect("the Create AI thread starts");
        Job { done }
    }

    /// The finished generation, once.
    pub fn try_result(&self) -> Option<Result<Vec<Candidate>, AiError>> {
        self.done.try_recv().ok()
    }
}

/// A short layer name from a candidate's title, else from the prompt: printable, at most 40 characters.
pub fn layer_name(title: &str, prompt: &str) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let title = clean(title);
    let words = if title.is_empty() { clean(prompt) } else { title };
    let mut name: String = words.chars().take(40).collect();
    if let Some(first) = name.chars().next() {
        name.replace_range(..first.len_utf8(), &first.to_uppercase().to_string());
    }
    if name.is_empty() { "Generated design".into() } else { name }
}

/// Canned candidates from `src/ai/fixtures/`, for tests.
#[cfg(test)]
pub mod fixtures {
    pub const DINOSAUR: &str = include_str!("fixtures/dinosaur.svg");
    pub const STAR: &str = include_str!("fixtures/star.svg");
    pub const HEART: &str = include_str!("fixtures/heart.svg");
    pub const CAT: &str = include_str!("fixtures/cat.svg");
    /// The four clean silhouettes a fake generation returns.
    pub const CLEAN: [(&str, &str); 4] = [("Dinosaur skateboard", DINOSAUR), ("Star skateboard", STAR), ("Heart skateboard", HEART), ("Cat skateboard", CAT)];
}

/// A provider that answers from a script, for tests. With nothing scripted it returns the four clean
/// fixtures. A held provider blocks every call until [`FakeProvider::release`].
#[cfg(test)]
#[derive(Default)]
pub struct FakeProvider {
    script: std::sync::Mutex<std::collections::VecDeque<Result<Vec<Candidate>, AiError>>>,
    calls: std::sync::Mutex<Vec<GenerateRequest>>,
    gate: std::sync::Mutex<Option<Receiver<()>>>,
    /// What `models` answers; two fake model names when unset.
    pub models: std::sync::Mutex<Option<Result<Vec<String>, AiError>>>,
}

#[cfg(test)]
impl FakeProvider {
    pub fn clean() -> Vec<Candidate> {
        fixtures::CLEAN.iter().map(|(t, s)| Candidate::drawn(*t, *s)).collect()
    }

    /// The next call's answer.
    pub fn then(&self, answer: Result<Vec<Candidate>, AiError>) {
        self.script.lock().unwrap().push_back(answer);
    }

    /// Holds every call until the returned sender sends or drops.
    pub fn hold(&self) -> std::sync::mpsc::Sender<()> {
        let (send, gate) = channel();
        *self.gate.lock().unwrap() = Some(gate);
        send
    }

    pub fn calls(&self) -> Vec<GenerateRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl Provider for FakeProvider {
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<Candidate>, AiError> {
        self.calls.lock().unwrap().push(request.clone());
        if let Some(gate) = self.gate.lock().unwrap().take() {
            _ = gate.recv();
        }
        let scripted = self.script.lock().unwrap().pop_front();
        scripted.unwrap_or_else(|| Ok(Self::clean().into_iter().take(request.count()).collect()))
    }

    fn models(&self) -> Result<Vec<String>, AiError> {
        self.models.lock().unwrap().clone().unwrap_or_else(|| Ok(vec!["fake-large".into(), "fake-small".into()]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_generation_validates_all_four_without_a_retry() {
        let fake = FakeProvider::default();
        let drawn = generate(&fake, &GenerateRequest::new("a dinosaur on a skateboard", Style::Silhouette, Detail::Medium)).unwrap();
        assert_eq!(drawn.len(), 4);
        assert!(drawn.iter().all(|c| c.piece.is_some()), "{:?}", drawn.iter().map(|c| &c.problem).collect::<Vec<_>>());
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn a_failed_candidate_is_asked_for_once_more_quoting_the_reasons() {
        let fake = FakeProvider::default();
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><text x="10" y="50">Hi</text></svg>"#;
        let mut first = FakeProvider::clean();
        first[1] = Candidate::drawn("Words", text);
        first[2] = Candidate::drawn("Words", text);
        fake.then(Ok(first));
        fake.then(Ok(vec![Candidate::drawn("Fixed", fixtures::STAR)]));
        fake.then(Ok(vec![Candidate::drawn("Still words", text)]));
        let drawn = generate(&fake, &GenerateRequest::new("hello", Style::Silhouette, Detail::Medium)).unwrap();
        let calls = fake.calls();
        assert_eq!(calls.len(), 3, "one retry per failed candidate");
        let reasons = calls[1..].iter().map(|c| c.fix.as_ref().unwrap().problem.clone().unwrap()).collect::<Vec<_>>();
        assert!(reasons.iter().all(|r| r.contains("text")), "{reasons:?}");
        let ready = drawn.iter().filter(|c| c.piece.is_some()).count();
        assert_eq!(ready, 3, "one retry fixed its candidate, the other still fails");
        assert!(drawn.iter().any(|c| c.problem.as_deref().is_some_and(|p| p.contains("text"))));
    }

    #[test]
    fn an_error_stops_the_generation_and_a_short_answer_is_padded() {
        let fake = FakeProvider::default();
        fake.then(Err(AiError::KeyRejected));
        assert_eq!(generate(&fake, &GenerateRequest::new("x", Style::Stencil, Detail::Simple)), Err(AiError::KeyRejected));
        fake.then(Ok(FakeProvider::clean().into_iter().take(2).collect()));
        let drawn = generate(&fake, &GenerateRequest::new("x", Style::Silhouette, Detail::Medium)).unwrap();
        assert_eq!(drawn.len(), 4);
        assert_eq!(drawn.iter().filter(|c| c.piece.is_none()).count(), 2);
    }

    #[test]
    fn a_dropped_job_discards_its_late_answer() {
        let fake = Arc::new(FakeProvider::default());
        let release = fake.hold();
        let job = Job::start(fake.clone(), GenerateRequest::new("x", Style::Silhouette, Detail::Medium));
        assert!(job.try_result().is_none());
        drop(job);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while fake.calls().is_empty() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(fake.calls().len(), 1, "the call ran; its answer had nowhere to go");
    }

    #[test]
    fn layer_names_come_from_the_title_or_else_the_prompt() {
        assert_eq!(layer_name("Dinosaur skateboard", "anything"), "Dinosaur skateboard");
        assert_eq!(layer_name("  ", "a dinosaur\nwith a horn"), "A dinosaur with a horn");
        assert_eq!(layer_name("", ""), "Generated design");
        assert_eq!(layer_name(&"x".repeat(90), "").chars().count(), 40);
    }

    #[test]
    fn errors_read_as_the_spec_words_them() {
        let anthropic = Service::Anthropic;
        assert_eq!(AiError::KeyRejected.message(anthropic), "Your API key was rejected.");
        assert_eq!(AiError::Network.message(anthropic), "Kerf couldn't reach Anthropic.");
        assert_eq!(AiError::Busy { retry_after_s: Some(30) }.message(anthropic), "Anthropic is busy. Try again in 30 seconds.");
        assert_eq!(AiError::Refused { explanation: Some("it names a brand.".into()) }.message(anthropic), "Kerf couldn't make this: it names a brand.");
        assert_eq!(AiError::Network.message(Service::OpenAi), "Kerf couldn't reach OpenAI.");
        assert_eq!(AiError::Busy { retry_after_s: None }.message(Service::Local), "Your local model server is busy. Try again in a moment.");
    }
}

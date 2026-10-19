//! OpenAI, OpenRouter and local models, over the two OpenAI wire formats. OpenAI's own models are
//! served through the Responses API; OpenRouter and local servers (Ollama, LM Studio, llama.cpp, vLLM)
//! speak Chat Completions. Both stream, and both ask for the reply as JSON against
//! [`wire::schema`] with strict structured outputs. A server that does not enforce the schema may wrap
//! the JSON in prose; [`wire::candidates`] digs it out, and the validator judges the result as usual.
//!
//! The requests and stream parsers are pure functions, tested on canned bytes; only
//! [`OpenAiProvider::generate`] touches the network, and no test calls it.

use std::io::BufRead;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::wire::{self, Ending, Reply, SYSTEM_PROMPT};
use super::{AiError, Candidate, GenerateRequest, Provider, Service};

const OPENAI: &str = "https://api.openai.com/v1";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";
/// Connecting, and waiting for the response to start, on a hosted service.
const TIMEOUT: Duration = Duration::from_secs(120);
/// A local server may load the model into memory before its first byte.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(300);
/// The schema's name in the request, which the APIs require.
const SCHEMA_NAME: &str = "kerf_designs";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    /// `POST /responses`.
    Responses,
    /// `POST /chat/completions`.
    ChatCompletions,
}

pub struct OpenAiProvider {
    service: Service,
    base: String,
    api: Api,
    key: Option<String>,
    model: String,
    agent: ureq::Agent,
}

/// Never prints the key.
impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = self.key.as_ref().map(|_| "[redacted]");
        f.debug_struct("OpenAiProvider").field("service", &self.service).field("base", &self.base).field("model", &self.model).field("key", &key).finish()
    }
}

impl OpenAiProvider {
    pub fn openai(key: String, model: String) -> Self {
        Self { service: Service::OpenAi, base: OPENAI.into(), api: Api::Responses, key: Some(key), model, agent: wire::agent(TIMEOUT) }
    }

    pub fn openrouter(key: String, model: String) -> Self {
        Self { service: Service::OpenRouter, base: OPENROUTER.into(), api: Api::ChatCompletions, key: Some(key), model, agent: wire::agent(TIMEOUT) }
    }

    /// A server on `address`, such as Ollama's `http://localhost:11434/v1`.
    pub fn local(address: &str, model: String) -> Self {
        Self { service: Service::Local, base: base_url(address), api: Api::ChatCompletions, key: None, model, agent: wire::agent(LOCAL_TIMEOUT) }
    }

    fn endpoint(&self) -> String {
        match self.api {
            Api::Responses => format!("{}/responses", self.base),
            Api::ChatCompletions => format!("{}/chat/completions", self.base),
        }
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        self.key.iter().map(|k| ("authorization", format!("Bearer {k}"))).collect()
    }
}

impl Provider for OpenAiProvider {
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<Candidate>, AiError> {
        let started = Instant::now();
        let reply = match self.api {
            Api::Responses => wire::post(&self.agent, &self.endpoint(), &self.headers(), &responses_body(&self.model, request), |r| read_responses(r)),
            Api::ChatCompletions => {
                let body = chat_body(&self.model, request, self.service == Service::OpenRouter);
                wire::post(&self.agent, &self.endpoint(), &self.headers(), &body, |r| read_chat(r))
            }
        };
        tracing::info!(target: "ai", service = ?self.service, model = %self.model, elapsed = ?started.elapsed(), ok = reply.is_ok(), "reply");
        wire::candidates(reply?, request.count())
    }

    /// `GET /models`, kept to the ones that can draw: see [`drafts_designs`].
    fn models(&self) -> Result<Vec<String>, AiError> {
        let list = wire::get(&self.agent, &format!("{}/models", self.base), &self.headers())?;
        Ok(wire::model_ids(&list, |m| drafts_designs(self.service, m)))
    }
}

/// Whether a model-list entry can answer a design request. OpenAI's list holds embedding, speech and
/// image models too; OpenRouter says which models take a response format. A local server lists only
/// what the user has pulled, so all of it counts.
pub fn drafts_designs(service: Service, entry: &Value) -> bool {
    const NOT_TEXT: [&str; 12] = ["embedding", "tts", "whisper", "dall-e", "moderation", "transcribe", "realtime", "audio", "image", "search", "davinci", "babbage"];
    let id = entry["id"].as_str().unwrap_or_default();
    match service {
        Service::OpenAi => !NOT_TEXT.iter().any(|word| id.contains(word)),
        Service::OpenRouter => entry["supported_parameters"].as_array().is_none_or(|p| p.iter().any(|p| p == "structured_outputs" || p == "response_format")),
        Service::Anthropic | Service::Local => true,
    }
}

/// `address` as a base URL: `http://` added when it has no scheme, no trailing slash.
pub fn base_url(address: &str) -> String {
    let address = address.trim().trim_end_matches('/');
    if address.starts_with("http://") || address.starts_with("https://") { address.to_owned() } else { format!("http://{address}") }
}

fn turns(request: &GenerateRequest) -> Vec<Value> {
    wire::turns(request).into_iter().map(|(role, text)| json!({ "role": role.name(), "content": text })).collect()
}

/// The Responses API request: not stored on OpenAI's side, the reply held to the schema.
pub fn responses_body(model: &str, request: &GenerateRequest) -> Value {
    json!({
        "model": model,
        "stream": true,
        "store": false,
        "instructions": SYSTEM_PROMPT,
        "input": turns(request),
        "text": { "format": { "type": "json_schema", "name": SCHEMA_NAME, "strict": true, "schema": wire::schema() } },
    })
}

/// The Chat Completions request. On OpenRouter, `require_parameters` routes only to providers that
/// honour the schema.
pub fn chat_body(model: &str, request: &GenerateRequest, openrouter: bool) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    messages.extend(turns(request));
    let mut body = json!({
        "model": model,
        "stream": true,
        "messages": messages,
        "response_format": { "type": "json_schema", "json_schema": { "name": SCHEMA_NAME, "strict": true, "schema": wire::schema() } },
    });
    if openrouter {
        body["provider"] = json!({ "require_parameters": true });
    }
    body
}

/// The JSON in one `data:` line, if it holds any.
fn event(line: &str) -> Option<Value> {
    serde_json::from_str(line.strip_prefix("data:")?.trim()).ok()
}

/// Reads a Responses API stream to `response.completed`, `response.incomplete` or a failure.
pub fn read_responses(reader: impl BufRead) -> Result<Reply, AiError> {
    let mut reply = Reply::default();
    let mut refusal = String::new();
    for line in reader.lines() {
        let line = line.map_err(|_| AiError::Network)?;
        let Some(event) = event(&line) else { continue };
        match event["type"].as_str() {
            Some("response.output_text.delta") => reply.text.push_str(event["delta"].as_str().unwrap_or_default()),
            Some("response.refusal.delta") => refusal.push_str(event["delta"].as_str().unwrap_or_default()),
            Some("response.refusal.done") => refusal = event["refusal"].as_str().unwrap_or(&refusal).to_owned(),
            Some("response.completed") => {
                if !refusal.trim().is_empty() {
                    reply.ending = Ending::Refused(Some(refusal));
                }
                return Ok(reply);
            }
            Some("response.incomplete") => {
                reply.ending = match event["response"]["incomplete_details"]["reason"].as_str() {
                    Some("content_filter") => Ending::Refused(None),
                    _ => Ending::CutOff,
                };
                return Ok(reply);
            }
            Some("response.failed") => return Err(stream_error(&event["response"]["error"])),
            Some("error") => return Err(stream_error(&event)),
            _ => {}
        }
    }
    Err(AiError::Network)
}

/// Reads a Chat Completions stream to `data: [DONE]`, or to its end once a choice has finished.
pub fn read_chat(reader: impl BufRead) -> Result<Reply, AiError> {
    let mut reply = Reply::default();
    let mut refusal = String::new();
    let mut finished = false;
    for line in reader.lines() {
        let line = line.map_err(|_| AiError::Network)?;
        if line.strip_prefix("data:").is_some_and(|d| d.trim() == "[DONE]") {
            finished = true;
            break;
        }
        let Some(event) = event(&line) else { continue };
        if event.get("error").is_some_and(|e| !e.is_null()) {
            return Err(stream_error(&event["error"]));
        }
        let choice = &event["choices"][0];
        reply.text.push_str(choice["delta"]["content"].as_str().unwrap_or_default());
        refusal.push_str(choice["delta"]["refusal"].as_str().unwrap_or_default());
        match choice["finish_reason"].as_str() {
            Some("length") => (reply.ending, finished) = (Ending::CutOff, true),
            Some("content_filter") => (reply.ending, finished) = (Ending::Refused(None), true),
            Some(_) => finished = true,
            None => {}
        }
    }
    if !refusal.trim().is_empty() {
        reply.ending = Ending::Refused(Some(refusal));
    }
    if finished { Ok(reply) } else { Err(AiError::Network) }
}

/// An error object mid-stream: a rate limit or overload is a busy server, anything else an API error.
fn stream_error(error: &Value) -> AiError {
    let code = error["code"].as_str().or(error["type"].as_str()).unwrap_or_default();
    let busy = matches!(code, "rate_limit_exceeded" | "server_is_overloaded" | "slow_down" | "overloaded_error") || error["code"].as_u64() == Some(429);
    if busy {
        return AiError::Busy { retry_after_s: None };
    }
    let status = error["code"].as_u64().and_then(|c| u16::try_from(c).ok()).unwrap_or(0);
    AiError::Api { status, message: wire::quote(error["message"].as_str().unwrap_or("the stream stopped")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Detail, Style};

    fn request() -> GenerateRequest {
        GenerateRequest::new("a dinosaur", Style::Silhouette, Detail::Medium)
    }

    fn lines(events: &[Value]) -> Vec<u8> {
        events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>().into_bytes()
    }

    #[test]
    fn openai_asks_the_responses_api_for_strict_json_and_stores_nothing() {
        let body = responses_body("gpt-6-astra", &request());
        assert_eq!(body["model"], "gpt-6-astra");
        assert_eq!((body["stream"].clone(), body["store"].clone()), (json!(true), json!(false)));
        assert_eq!(body["instructions"], SYSTEM_PROMPT);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["text"]["format"], json!({ "type": "json_schema", "name": SCHEMA_NAME, "strict": true, "schema": wire::schema() }));
        let provider = OpenAiProvider::openai("sk-proj-abc".into(), "gpt-6-astra".into());
        assert_eq!(provider.endpoint(), "https://api.openai.com/v1/responses");
        assert_eq!(provider.headers(), [("authorization", "Bearer sk-proj-abc".to_owned())]);
    }

    #[test]
    fn openrouter_and_local_speak_chat_completions() {
        let body = chat_body("openai/gpt-6-astra", &request(), true);
        assert_eq!(body["messages"][0], json!({ "role": "system", "content": SYSTEM_PROMPT }));
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["provider"], json!({ "require_parameters": true }));
        assert!(chat_body("llama3.3", &request(), false).get("provider").is_none());
        let router = OpenAiProvider::openrouter("sk-or-v1-abc".into(), "m".into());
        assert_eq!(router.endpoint(), "https://openrouter.ai/api/v1/chat/completions");
        let local = OpenAiProvider::local("localhost:1234/v1/", "qwen".into());
        assert_eq!(local.endpoint(), "http://localhost:1234/v1/chat/completions");
        assert!(local.headers().is_empty(), "a local server gets no key");
    }

    #[test]
    fn addresses_gain_a_scheme_and_lose_a_trailing_slash() {
        assert_eq!(base_url(" http://localhost:11434/v1/ "), "http://localhost:11434/v1");
        assert_eq!(base_url("https://box.local/v1"), "https://box.local/v1");
        assert_eq!(base_url("127.0.0.1:8080/v1"), "http://127.0.0.1:8080/v1");
    }

    #[test]
    fn a_responses_stream_yields_its_text_or_its_refusal() {
        let done = json!({ "type": "response.completed", "response": { "status": "completed" } });
        let text = lines(&[
            json!({ "type": "response.created", "response": {} }),
            json!({ "type": "response.output_text.delta", "delta": "{\"candidates\":" }),
            json!({ "type": "response.output_text.delta", "delta": "[]}" }),
            done.clone(),
        ]);
        assert_eq!(read_responses(&text[..]).unwrap(), Reply { ending: Ending::Finished, text: "{\"candidates\":[]}".into() });
        let refused = lines(&[
            json!({ "type": "response.refusal.delta", "delta": "I can't help " }),
            json!({ "type": "response.refusal.done", "refusal": "I can't help with that logo." }),
            done,
        ]);
        assert_eq!(read_responses(&refused[..]).unwrap().ending, Ending::Refused(Some("I can't help with that logo.".into())));
    }

    #[test]
    fn a_responses_stream_reports_cut_offs_failures_and_drops() {
        let cut = lines(&[json!({ "type": "response.incomplete", "response": { "incomplete_details": { "reason": "max_output_tokens" } } })]);
        assert_eq!(read_responses(&cut[..]).unwrap().ending, Ending::CutOff);
        let filtered = lines(&[json!({ "type": "response.incomplete", "response": { "incomplete_details": { "reason": "content_filter" } } })]);
        assert_eq!(read_responses(&filtered[..]).unwrap().ending, Ending::Refused(None));
        let failed = lines(&[json!({ "type": "response.failed", "response": { "error": { "code": "server_error", "message": "boom" } } })]);
        assert_eq!(read_responses(&failed[..]), Err(AiError::Api { status: 0, message: "boom".into() }));
        let limited = lines(&[json!({ "type": "error", "code": "rate_limit_exceeded", "message": "slow down" })]);
        assert_eq!(read_responses(&limited[..]), Err(AiError::Busy { retry_after_s: None }));
        assert_eq!(read_responses(&lines(&[json!({ "type": "response.created" })])[..]), Err(AiError::Network));
    }

    #[test]
    fn a_chat_stream_yields_its_deltas_to_done() {
        let mut bytes = lines(&[
            json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "" } }] }),
            json!({ "choices": [{ "index": 0, "delta": { "content": "```json\n{\"candidates\":[]" } }] }),
            json!({ "choices": [{ "index": 0, "delta": { "content": "}\n```" }, "finish_reason": "stop" }] }),
        ]);
        bytes.extend_from_slice(b": OPENROUTER PROCESSING\n\ndata: [DONE]\n\n");
        let reply = read_chat(&bytes[..]).unwrap();
        assert_eq!(reply, Reply { ending: Ending::Finished, text: "```json\n{\"candidates\":[]}\n```".into() });
        let no_done = lines(&[json!({ "choices": [{ "delta": { "content": "{}" }, "finish_reason": "stop" }] })]);
        assert!(read_chat(&no_done[..]).is_ok(), "some local servers end without [DONE]");
    }

    #[test]
    fn a_chat_stream_reports_refusals_cut_offs_errors_and_drops() {
        let refused = lines(&[json!({ "choices": [{ "delta": { "refusal": "No logos." }, "finish_reason": "stop" }] })]);
        assert_eq!(read_chat(&refused[..]).unwrap().ending, Ending::Refused(Some("No logos.".into())));
        let cut = lines(&[json!({ "choices": [{ "delta": { "content": "{" }, "finish_reason": "length" }] })]);
        assert_eq!(read_chat(&cut[..]).unwrap().ending, Ending::CutOff);
        let filtered = lines(&[json!({ "choices": [{ "delta": {}, "finish_reason": "content_filter" }] })]);
        assert_eq!(read_chat(&filtered[..]).unwrap().ending, Ending::Refused(None));
        let error = lines(&[json!({ "error": { "code": 502, "message": "Provider returned error" } })]);
        assert_eq!(read_chat(&error[..]), Err(AiError::Api { status: 502, message: "Provider returned error".into() }));
        let limited = lines(&[json!({ "error": { "code": 429, "message": "Rate limited" } })]);
        assert_eq!(read_chat(&limited[..]), Err(AiError::Busy { retry_after_s: None }));
        let dropped = lines(&[json!({ "choices": [{ "delta": { "content": "{" } }] })]);
        assert_eq!(read_chat(&dropped[..]), Err(AiError::Network));
    }

    #[test]
    fn model_lists_keep_only_models_that_can_draw() {
        let openai = json!({ "data": [{ "id": "gpt-6-astra" }, { "id": "text-embedding-3-large" }, { "id": "gpt-6-luna" }, { "id": "whisper-1" }] });
        assert_eq!(wire::model_ids(&openai, |m| drafts_designs(Service::OpenAi, m)), ["gpt-6-astra", "gpt-6-luna"]);
        let router = json!({ "data": [
            { "id": "anthropic/claude-opus-5.5", "supported_parameters": ["tools", "structured_outputs"] },
            { "id": "some/old-model", "supported_parameters": ["temperature"] },
            { "id": "openai/gpt-6-astra", "supported_parameters": ["response_format"] },
            { "id": "unlabelled/model" },
        ] });
        assert_eq!(wire::model_ids(&router, |m| drafts_designs(Service::OpenRouter, m)), ["anthropic/claude-opus-5.5", "openai/gpt-6-astra", "unlabelled/model"]);
        let ollama = json!({ "object": "list", "data": [{ "id": "qwen3:14b", "object": "model" }, { "id": " " }] });
        assert_eq!(wire::model_ids(&ollama, |m| drafts_designs(Service::Local, m)), ["qwen3:14b"]);
    }

    #[test]
    fn the_provider_never_prints_its_key() {
        let shown = format!("{:?}", OpenAiProvider::openrouter("sk-or-v1-secret".into(), "m".into()));
        assert!(!shown.contains("secret") && shown.contains("[redacted]"), "{shown}");
    }
}

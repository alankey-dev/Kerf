//! Claude through the Messages API over raw HTTPS, since Rust has no official Anthropic SDK. One
//! streamed request per generation: structured outputs fix the reply's shape, adaptive thinking runs at
//! medium effort, and a refusal falls back server-side to the model Anthropic recommends for its
//! category.
//!
//! The request and the stream parser are pure functions, tested on canned bytes; only
//! [`AnthropicProvider::generate`] touches the network, and no test calls it.

use std::io::BufRead;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::wire::{self, Ending, Reply, SYSTEM_PROMPT};
use super::{AiError, Candidate, GenerateRequest, Provider};

const URL: &str = "https://api.anthropic.com/v1/messages";
const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";
const VERSION: &str = "2023-06-01";
/// Gates `fallbacks: "default"`: a declined request re-runs on the model Anthropic picks for the
/// refusal's category, inside the same call.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Room for adaptive thinking as well as four SVGs; streaming keeps a reply this long clear of timeouts.
const MAX_TOKENS: u32 = 64_000;
/// Connecting, and waiting for the response to start.
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct AnthropicProvider {
    key: String,
    model: String,
    agent: ureq::Agent,
}

/// Never prints the key.
impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider").field("model", &self.model).field("key", &"[redacted]").finish()
    }
}

impl AnthropicProvider {
    pub fn new(key: String, model: String) -> Self {
        Self { key, model, agent: wire::agent(TIMEOUT) }
    }
}

impl Provider for AnthropicProvider {
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<Candidate>, AiError> {
        let started = Instant::now();
        let headers = [("x-api-key", self.key.clone()), ("anthropic-version", VERSION.into()), ("anthropic-beta", FALLBACK_BETA.into())];
        let reply = wire::post(&self.agent, URL, &headers, &body(&self.model, request), |r| read_stream(r));
        tracing::info!(target: "ai", service = "anthropic", model = %self.model, elapsed = ?started.elapsed(), ok = reply.is_ok(), "reply");
        wire::candidates(reply?, request.count())
    }

    /// The Models API, newest first.
    fn models(&self) -> Result<Vec<String>, AiError> {
        let headers = [("x-api-key", self.key.clone()), ("anthropic-version", VERSION.into())];
        Ok(wire::model_ids(&wire::get(&self.agent, MODELS_URL, &headers)?, |_| true))
    }
}

/// The Messages API request for `request` on `model`.
pub fn body(model: &str, request: &GenerateRequest) -> Value {
    let messages: Vec<Value> = wire::turns(request).into_iter().map(|(role, text)| json!({ "role": role.name(), "content": text })).collect();
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "thinking": { "type": "adaptive" },
        "output_config": { "effort": "medium", "format": { "type": "json_schema", "schema": wire::schema() } },
        "fallbacks": "default",
        "system": [{ "type": "text", "text": SYSTEM_PROMPT, "cache_control": { "type": "ephemeral" } }],
        "messages": messages,
    })
}

/// Reads server-sent events to the end of the message. Thinking, signature and fallback blocks are
/// skipped; after a mid-stream fallback the new model continues the text, so every text block counts.
/// `stop_reason` and `stop_details` come from `message_delta`.
pub fn read_stream(reader: impl BufRead) -> Result<Reply, AiError> {
    let mut reply = Reply::default();
    let mut explanation = None;
    for line in reader.lines() {
        let line = line.map_err(|_| AiError::Network)?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else { continue };
        match event["type"].as_str() {
            Some("content_block_start") if event["content_block"]["type"] == "text" => {
                reply.text.push_str(event["content_block"]["text"].as_str().unwrap_or_default());
            }
            Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                reply.text.push_str(event["delta"]["text"].as_str().unwrap_or_default());
            }
            Some("message_delta") => {
                let details = [&event["delta"]["stop_details"], &event["stop_details"]];
                if let Some(why) = details.iter().find_map(|d| d["explanation"].as_str()) {
                    explanation = Some(why.to_owned());
                }
                match event["delta"]["stop_reason"].as_str() {
                    Some("refusal") => reply.ending = Ending::Refused(explanation.clone()),
                    Some("max_tokens") => reply.ending = Ending::CutOff,
                    _ => {}
                }
            }
            Some("error") => return Err(stream_error(&event["error"])),
            Some("message_stop") => return Ok(reply),
            _ => {}
        }
    }
    // The connection closed before the message finished.
    Err(AiError::Network)
}

/// An `error` event mid-stream: overloaded is a busy server, anything else an API error.
fn stream_error(error: &Value) -> AiError {
    match error["type"].as_str() {
        Some("overloaded_error" | "rate_limit_error") => AiError::Busy { retry_after_s: None },
        _ => AiError::Api { status: 0, message: wire::quote(error["message"].as_str().unwrap_or("the stream stopped")) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Detail, Style};

    fn sse(events: &[Value]) -> Vec<u8> {
        events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())).collect::<String>().into_bytes()
    }

    fn reply_with(text: &str, stop: Value) -> Vec<u8> {
        sse(&[
            json!({ "type": "message_start", "message": { "id": "msg_1", "model": "claude-opus-5-5", "stop_reason": null } }),
            json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "abc" } }),
            json!({ "type": "content_block_stop", "index": 0 }),
            json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": &text[..text.len() / 2] } }),
            json!({ "type": "ping" }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": &text[text.len() / 2..] } }),
            json!({ "type": "content_block_stop", "index": 1 }),
            json!({ "type": "message_delta", "delta": stop, "usage": { "output_tokens": 9 } }),
            json!({ "type": "message_stop" }),
        ])
    }

    #[test]
    fn the_request_streams_with_structured_outputs_adaptive_thinking_and_fallbacks() {
        let body = body("claude-opus-5-5", &GenerateRequest::new("a dinosaur", Style::Stencil, Detail::Simple));
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["stream"], true);
        assert_eq!(body["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["output_config"]["format"], json!({ "type": "json_schema", "schema": wire::schema() }));
        assert_eq!(body["fallbacks"], "default");
        assert!(body.get("tool_choice").is_none() && body.get("tools").is_none(), "no forced tool use");
        assert!(body.get("temperature").is_none(), "sampling parameters are rejected on this model");
        assert_eq!(body["system"][0]["text"], SYSTEM_PROMPT);
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("<prompt>a dinosaur</prompt>"));
    }

    #[test]
    fn a_stream_yields_its_text_blocks_and_skips_thinking() {
        let text = json!({ "candidates": [{ "title": "Dino", "svg": "<svg/>" }], "decline": null }).to_string();
        let reply = read_stream(&reply_with(&text, json!({ "stop_reason": "end_turn" }))[..]).unwrap();
        assert_eq!(reply, Reply { ending: Ending::Finished, text });
    }

    #[test]
    fn a_refusal_carries_its_explanation_and_a_cut_off_is_marked() {
        let stop = json!({ "stop_reason": "refusal", "stop_details": { "type": "refusal", "category": null, "explanation": "it names a brand." } });
        assert_eq!(read_stream(&reply_with("{\"candid", stop)[..]).unwrap().ending, Ending::Refused(Some("it names a brand.".into())));
        let bare = read_stream(&reply_with("", json!({ "stop_reason": "refusal", "stop_details": null }))[..]).unwrap();
        assert_eq!(bare.ending, Ending::Refused(None));
        assert_eq!(read_stream(&reply_with("{", json!({ "stop_reason": "max_tokens" }))[..]).unwrap().ending, Ending::CutOff);
    }

    #[test]
    fn a_stream_that_stops_early_or_errors_is_reported() {
        let half = sse(&[json!({ "type": "message_start", "message": {} })]);
        assert_eq!(read_stream(&half[..]), Err(AiError::Network));
        let overloaded = sse(&[json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } })]);
        assert_eq!(read_stream(&overloaded[..]), Err(AiError::Busy { retry_after_s: None }));
    }

    #[test]
    fn the_provider_never_prints_its_key() {
        let provider = AnthropicProvider::new("sk-ant-secret-1234".into(), "claude-opus-5-5".into());
        let shown = format!("{provider:?}");
        assert!(!shown.contains("sk-ant") && shown.contains("claude-opus-5-5"), "{shown}");
    }
}

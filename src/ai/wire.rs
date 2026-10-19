//! What every service's client shares: the system prompt, the reply's JSON schema, the conversation
//! for a request, and reading the reply back into candidates. Pure; the clients own the HTTP.

use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use serde_json::{Value, json};

use super::{AiError, Candidate, GenerateRequest, WIDTH_IN};

pub const SYSTEM_PROMPT: &str = include_str!("prompt.md");
/// The longest refusal or error message the panel quotes.
const QUOTE_CHARS: usize = 300;
/// The most of a reply Kerf reads.
const REPLY_LIMIT: u64 = 16 << 20;
/// The whole call, stream included, so a stalled stream cannot hold the thread for ever.
const CEILING: Duration = Duration::from_secs(600);

/// An HTTP client that waits `patience` to connect and for the response to start. Statuses come back
/// as responses, so [`post`] can read `retry-after` and the error body.
pub fn agent(patience: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(patience))
        .timeout_recv_response(Some(patience))
        .timeout_global(Some(CEILING))
        .http_status_as_error(false)
        .build();
    config.into()
}

/// POSTs `body` as JSON and reads a 200's stream with `read`; any other status becomes an [`AiError`].
pub fn post(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, String)],
    body: &Value,
    read: impl FnOnce(&mut dyn BufRead) -> Result<Reply, AiError>,
) -> Result<Reply, AiError> {
    let mut call = agent.post(url).header("content-type", "application/json");
    for (name, value) in headers {
        call = call.header(*name, value);
    }
    let mut response = call.send(&body.to_string()).map_err(|e| {
        tracing::warn!(target: "ai", error = %e, "unreachable");
        AiError::Network
    })?;
    let status = response.status().as_u16();
    let retry_after_s = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
    let mut reader = BufReader::new(response.body_mut().with_config().limit(REPLY_LIMIT).reader());
    if status == 200 { read(&mut reader) } else { Err(status_error(status, retry_after_s, reader)) }
}

/// GETs a JSON document, such as a model list; any status but 200 becomes an [`AiError`].
pub fn get(agent: &ureq::Agent, url: &str, headers: &[(&str, String)]) -> Result<Value, AiError> {
    let mut call = agent.get(url);
    for (name, value) in headers {
        call = call.header(*name, value);
    }
    let mut response = call.call().map_err(|e| {
        tracing::warn!(target: "ai", error = %e, "unreachable");
        AiError::Network
    })?;
    let status = response.status().as_u16();
    let retry_after_s = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
    let reader = response.body_mut().with_config().limit(REPLY_LIMIT).reader();
    if status != 200 {
        return Err(status_error(status, retry_after_s, reader));
    }
    serde_json::from_reader(reader).map_err(|_| AiError::Unreadable("it was not JSON.".into()))
}

/// The `id` of every entry in a model list's `data` that `keep` accepts, in the list's order: the
/// shape Anthropic, OpenAI, OpenRouter and the local servers all share.
pub fn model_ids(list: &Value, keep: impl Fn(&Value) -> bool) -> Vec<String> {
    list["data"].as_array().into_iter().flatten().filter(|m| keep(m)).filter_map(|m| Some(m["id"].as_str()?.trim().to_owned())).filter(|id| !id.is_empty()).collect()
}

/// The JSON shape every reply takes. Structured outputs take no array length limits, so the count is
/// asked for in words and enforced by [`super::generate`]. `decline` carries a model's own refusal.
pub fn schema() -> Value {
    let candidate = json!({
        "type": "object",
        "properties": { "title": { "type": "string" }, "svg": { "type": "string" } },
        "required": ["title", "svg"],
        "additionalProperties": false,
    });
    json!({
        "type": "object",
        "properties": {
            "candidates": { "type": "array", "items": candidate },
            "decline": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
        },
        "required": ["candidates", "decline"],
        "additionalProperties": false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// The conversation after the system prompt: one user turn, or for a fix the earlier design and the
/// follow-up that quotes what failed.
pub fn turns(request: &GenerateRequest) -> Vec<(Role, String)> {
    let ask = (Role::User, brief(request));
    let Some(fix) = &request.fix else { return vec![ask] };
    let drawn = json!({ "candidates": [{ "title": fix.title, "svg": fix.svg }], "decline": null });
    let problems = fix.problem.as_deref().unwrap_or("it did not pass");
    let follow_up = format!("Kerf's checks rejected that design: {problems}. Return one corrected candidate that fixes exactly these problems.");
    vec![ask, (Role::Assistant, drawn.to_string()), (Role::User, follow_up)]
}

fn brief(request: &GenerateRequest) -> String {
    let (style, detail, count) = (request.style, request.detail, request.count());
    let mut text = format!(
        "<prompt>{}</prompt>\n\nStyle: {}, {}.\nDetail: {}. At {WIDTH_IN} inches wide, every separate piece must be at least {} inches across its narrower side. Use at most {} paths and {} path nodes in total.\n\nReturn exactly {count} {}.",
        request.prompt,
        style.label(),
        style.brief(),
        detail.label(),
        detail.min_feature_in(),
        detail.path_budget(),
        detail.node_budget(),
        if count == 1 { "candidate" } else { "candidates" },
    );
    if let Some(earlier) = &request.variations_of {
        text.push_str(&format!("\n\nDraw variations of this earlier design, titled \"{}\":\n{}", earlier.title, earlier.svg));
    }
    text
}

/// How a streamed reply ended.
#[derive(Debug, Default, PartialEq)]
pub enum Ending {
    #[default]
    Finished,
    /// Declined, with the service's explanation when it gave one.
    Refused(Option<String>),
    /// Stopped at the output limit before the JSON closed.
    CutOff,
}

/// What a finished stream said: its ending, and every text fragment in order.
#[derive(Debug, Default, PartialEq)]
pub struct Reply {
    pub ending: Ending,
    pub text: String,
}

/// Checks how the reply ended before reading its content, then takes at most `count` candidates.
pub fn candidates(reply: Reply, count: usize) -> Result<Vec<Candidate>, AiError> {
    match reply.ending {
        Ending::Refused(why) => return Err(AiError::Refused { explanation: why.as_deref().map(quote) }),
        Ending::CutOff => return Err(AiError::Unreadable("it was cut off before it finished.".into())),
        Ending::Finished => {}
    }
    let parsed: Value = serde_json::from_str(json_object(&reply.text)).map_err(|_| AiError::Unreadable("it was not the agreed JSON.".into()))?;
    let drawn: Vec<Candidate> =
        parsed["candidates"].as_array().into_iter().flatten().filter_map(|c| Some(Candidate::drawn(c["title"].as_str()?, c["svg"].as_str()?))).take(count).collect();
    match parsed["decline"].as_str().map(str::trim) {
        Some(why) if drawn.is_empty() && !why.is_empty() => Err(AiError::Refused { explanation: Some(quote(why)) }),
        _ if drawn.is_empty() => Err(AiError::Unreadable("it held no designs.".into())),
        _ => Ok(drawn),
    }
}

/// The outermost `{...}` in `text`. Models without enforced structured outputs (some local ones) wrap
/// the JSON in a code fence or a sentence.
fn json_object(text: &str) -> &str {
    match (text.find('{'), text.rfind('}')) {
        (Some(open), Some(close)) if open < close => &text[open..=close],
        _ => text,
    }
}

/// A non-success HTTP status, from the status, `retry-after` and the JSON error body
/// (`{"error": {"message": ...}}` on every service Kerf talks to).
pub fn status_error(status: u16, retry_after_s: Option<u64>, body: impl Read) -> AiError {
    match status {
        401 => AiError::KeyRejected,
        429 | 503 | 529 => AiError::Busy { retry_after_s },
        _ => {
            let mut text = String::new();
            _ = body.take(64 << 10).read_to_string(&mut text);
            let message = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(quote));
            AiError::Api { status, message: message.unwrap_or_else(|| "no details".into()) }
        }
    }
}

/// Untrusted text for the panel: printable, on one line, and short.
pub fn quote(text: &str) -> String {
    let flat = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    let words = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= QUOTE_CHARS { words } else { words.chars().take(QUOTE_CHARS).collect::<String>() + "..." }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Detail, Style};

    fn request() -> GenerateRequest {
        GenerateRequest::new("a dinosaur riding a skateboard", Style::Stencil, Detail::Simple)
    }

    fn finished(text: &str) -> Reply {
        Reply { ending: Ending::Finished, text: text.into() }
    }

    #[test]
    fn the_brief_names_the_style_the_detail_and_the_count() {
        let turns = turns(&request());
        assert_eq!(turns.len(), 1);
        for words in ["<prompt>a dinosaur riding a skateboard</prompt>", "Stencil", "0.25 inches", "at most 8 paths", "exactly 4 candidates"] {
            assert!(turns[0].1.contains(words), "{words:?} missing from {}", turns[0].1);
        }
    }

    #[test]
    fn a_fix_quotes_the_failed_design_and_its_reasons_in_a_follow_up() {
        let failed = Candidate { problem: Some("it contains text".into()), ..Candidate::drawn("Words", "<svg/>") };
        let turns = turns(&GenerateRequest { fix: Some(failed), ..request() });
        assert_eq!(turns.iter().map(|t| t.0).collect::<Vec<_>>(), [Role::User, Role::Assistant, Role::User]);
        assert!(turns[0].1.contains("exactly 1 candidate."));
        assert!(turns[1].1.contains("<svg/>"));
        assert!(turns[2].1.contains("it contains text"));
    }

    #[test]
    fn variations_carry_the_earlier_design() {
        let earlier = Candidate::drawn("Dino", "<svg>dino</svg>");
        let turns = turns(&GenerateRequest { variations_of: Some(earlier), ..request() });
        assert!(turns[0].1.contains("variations of this earlier design, titled \"Dino\":\n<svg>dino</svg>"));
    }

    #[test]
    fn candidates_come_from_the_json_even_inside_a_code_fence() {
        let json = json!({ "candidates": [{ "title": "Dino", "svg": "<svg/>" }, { "title": "Star", "svg": "<svg/>" }], "decline": null }).to_string();
        let drawn = candidates(finished(&json), 4).unwrap();
        assert_eq!(drawn.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Dino", "Star"]);
        let fenced = format!("Here you go:\n```json\n{json}\n```");
        assert_eq!(candidates(finished(&fenced), 1).unwrap().len(), 1);
    }

    #[test]
    fn refusals_declines_and_cut_offs_are_read_before_the_content() {
        let refused = Reply { ending: Ending::Refused(Some("it names a\ntrademarked character.".into())), text: "{\"cand".into() };
        assert_eq!(candidates(refused, 4), Err(AiError::Refused { explanation: Some("it names a trademarked character.".into()) }));
        let declined = json!({ "candidates": [], "decline": "Logos are not something Kerf draws." }).to_string();
        assert_eq!(candidates(finished(&declined), 4), Err(AiError::Refused { explanation: Some("Logos are not something Kerf draws.".into()) }));
        assert!(matches!(candidates(Reply { ending: Ending::CutOff, text: "{".into() }, 4), Err(AiError::Unreadable(_))));
        assert!(matches!(candidates(finished("not json"), 4), Err(AiError::Unreadable(_))));
    }

    #[test]
    fn statuses_map_to_the_panel_messages() {
        assert_eq!(status_error(401, None, &b""[..]), AiError::KeyRejected);
        assert_eq!(status_error(429, Some(20), &b""[..]), AiError::Busy { retry_after_s: Some(20) });
        assert_eq!(status_error(529, None, &b""[..]), AiError::Busy { retry_after_s: None });
        assert_eq!(status_error(503, None, &b""[..]), AiError::Busy { retry_after_s: None });
        let body = br#"{"type":"error","error":{"type":"invalid_request_error","message":"model: not found"}}"#;
        assert_eq!(status_error(404, None, &body[..]), AiError::Api { status: 404, message: "model: not found".into() });
    }
}

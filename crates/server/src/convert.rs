//! Conversion of the stored transcript into provider-neutral chat messages.
//! Tool call arguments are replayed byte-identically (raw JSON incl. `purpose`) for cache hits.

use crate::toolset::PURPOSE_KEY;
use ref_llm::{AssistantBlock, ChatMessage, UserContent};
use ref_proto::{Message, MessageSource, Part, Role, ToolState};
use serde_json::{Map, Value};

const IMAGE_OMITTED: &str = "[image omitted: model does not support image input]";

/// Arguments sent for a tool call that streamed none.
const EMPTY_ARGUMENTS: &str = "{}";

/// Result sent for a tool call that produced no output.
const NO_OUTPUT: &str = "(no output)";

/// Index of the last compaction summary (0 if the session was never compacted). Messages
/// before it stay in the transcript (visible to users) but are not sent to the model.
pub(crate) fn effective_start(messages: &[Message]) -> usize {
    messages
        .iter()
        .rposition(|m| m.role == Role::User && m.source == MessageSource::Compaction)
        .unwrap_or(0)
}

/// Chat messages for a request: the effective context, i.e. the transcript from the last
/// compaction summary (inclusive) onward. The slice only grows between compactions, so the
/// request prefix stays byte-stable for prompt caching.
pub(crate) fn to_chat(messages: &[Message], images: bool) -> Vec<ChatMessage> {
    let messages = &messages[effective_start(messages)..];
    let mut out = Vec::with_capacity(messages.len() * 2);
    for m in messages {
        match m.role {
            Role::User => {
                let content = user_content(&m.parts, images);
                if !content.is_empty() {
                    out.push(ChatMessage::User(content));
                }
            }
            Role::Assistant => {
                let Some((assistant, results)) = assistant_chat(m) else {
                    continue;
                };
                out.push(assistant);
                out.extend(results);
            }
        }
    }
    out
}

/// Text and image parts of a user message; images become a note when the model has no vision.
fn user_content(parts: &[Part], images: bool) -> Vec<UserContent> {
    let mut content = Vec::new();
    for p in parts {
        match p {
            Part::Text { text, .. } => content.push(UserContent::Text(text.clone())),
            Part::Image { image, .. } if images => content.push(UserContent::Image(image.clone())),
            Part::Image { .. } => content.push(UserContent::Text(IMAGE_OMITTED.into())),
            _ => {}
        }
    }
    content
}

/// The assistant message and the tool results that follow it; `None` when nothing is left
/// to send.
fn assistant_chat(m: &Message) -> Option<(ChatMessage, Vec<ChatMessage>)> {
    let mut blocks = Vec::new();
    let mut results = Vec::new();
    for p in &m.parts {
        match p {
            Part::Text { text, .. } if !text.is_empty() => {
                blocks.push(AssistantBlock::Text(text.clone()));
            }
            Part::Reasoning {
                text,
                provider_data,
                ..
            } if !text.is_empty() || provider_data.is_some() => {
                blocks.push(AssistantBlock::Reasoning {
                    text: text.clone(),
                    provider_data: provider_data.clone(),
                });
            }
            Part::Tool {
                call_id,
                name,
                raw_input,
                provider_data,
                state,
                output,
                images,
                ..
            } => {
                let arguments = if raw_input.trim().is_empty() {
                    EMPTY_ARGUMENTS.to_string()
                } else {
                    raw_input.clone()
                };
                blocks.push(AssistantBlock::ToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments,
                    provider_data: provider_data.clone(),
                });
                results.push(ChatMessage::ToolResult {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    content: tool_output_text(output.as_deref()),
                    images: images.clone(),
                    is_error: matches!(
                        state,
                        ToolState::Error | ToolState::Denied | ToolState::Cancelled
                    ),
                });
            }
            _ => {}
        }
    }
    if blocks.is_empty() {
        return None;
    }
    let origin = m
        .model
        .as_ref()
        .map(|r| (r.provider.clone(), r.model.clone()));
    Some((ChatMessage::Assistant { blocks, origin }, results))
}

/// The tool result text; an empty output is replaced by a note.
fn tool_output_text(output: Option<&str>) -> String {
    match output {
        Some(o) if !o.is_empty() => o.to_string(),
        _ => NO_OUTPUT.to_string(),
    }
}

/// Parse streamed tool arguments: returns (input without `purpose`, purpose).
pub(crate) fn parse_tool_input(raw: &str) -> Result<(Value, String), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((Value::Object(Map::new()), String::new()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(mut o)) => {
            let purpose = match o.remove(PURPOSE_KEY) {
                Some(Value::String(s)) => s,
                Some(other) if !other.is_null() => other.to_string(),
                _ => String::new(),
            };
            Ok((Value::Object(o), purpose))
        }
        Ok(_) => Err("arguments must be a JSON object".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Extract a complete `"purpose": "..."` string from partial JSON.
pub(crate) fn partial_purpose(raw: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(&format!(r#""{PURPOSE_KEY}"\s*:\s*"((?:[^"\\]|\\.)*)""#)).expect("regex")
    });
    let c = re.captures(raw)?;
    let lit = format!("\"{}\"", c.get(1)?.as_str());
    serde_json::from_str::<String>(&lit).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_parse() {
        let (v, p) = parse_tool_input(r#"{"purpose":"look","path":"a"}"#).unwrap();
        assert_eq!(p, "look");
        assert_eq!(v, serde_json::json!({"path":"a"}));
        assert!(parse_tool_input("{bad").is_err());
        assert_eq!(
            partial_purpose(r#"{"purpose": "re\"ad", "pa"#).as_deref(),
            Some("re\"ad")
        );
        assert_eq!(partial_purpose(r#"{"purpose": "rea"#), None);
    }
}

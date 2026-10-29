//! Conversion of the stored transcript into provider-neutral chat messages.
//! Tool call arguments are replayed byte-identically (raw JSON incl. `purpose`) for cache hits.

use ref_llm::{AssistantBlock, ChatMessage, UserContent};
use ref_proto::{Message, Part, Role, ToolState};
use serde_json::{Map, Value};

pub const IMAGE_OMITTED: &str = "[image omitted: model does not support image input]";

pub fn to_chat(messages: &[Message], images: bool) -> Vec<ChatMessage> {
    let mut out = Vec::with_capacity(messages.len() * 2);
    for m in messages {
        match m.role {
            Role::User => {
                let mut content = Vec::new();
                for p in &m.parts {
                    match p {
                        Part::Text { text, .. } => content.push(UserContent::Text(text.clone())),
                        Part::Image { image, .. } if images => content.push(UserContent::Image(image.clone())),
                        Part::Image { .. } => content.push(UserContent::Text(IMAGE_OMITTED.into())),
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    out.push(ChatMessage::User(content));
                }
            }
            Role::Assistant => {
                let mut blocks = Vec::new();
                let mut results = Vec::new();
                for p in &m.parts {
                    match p {
                        Part::Text { text, .. } => {
                            if !text.is_empty() {
                                blocks.push(AssistantBlock::Text(text.clone()));
                            }
                        }
                        Part::Reasoning { text, provider_data, .. } => {
                            if !text.is_empty() || provider_data.is_some() {
                                blocks.push(AssistantBlock::Reasoning {
                                    text: text.clone(),
                                    provider_data: provider_data.clone(),
                                });
                            }
                        }
                        Part::Tool { call_id, name, raw_input, provider_data, state, output, images: imgs, .. } => {
                            let arguments = if raw_input.trim().is_empty() { "{}".to_string() } else { raw_input.clone() };
                            blocks.push(AssistantBlock::ToolCall {
                                id: call_id.clone(),
                                name: name.clone(),
                                arguments,
                                provider_data: provider_data.clone(),
                            });
                            let content = match output.as_deref() {
                                Some(o) if !o.is_empty() => o.to_string(),
                                _ => "(no output)".to_string(),
                            };
                            results.push(ChatMessage::ToolResult {
                                call_id: call_id.clone(),
                                name: name.clone(),
                                content,
                                images: imgs.clone(),
                                is_error: matches!(state, ToolState::Error | ToolState::Denied | ToolState::Cancelled),
                            });
                        }
                        Part::Image { .. } => {}
                    }
                }
                if blocks.is_empty() {
                    continue;
                }
                let origin = m.model.as_ref().map(|r| (r.provider.clone(), r.model.clone()));
                out.push(ChatMessage::Assistant { blocks, origin });
                out.extend(results);
            }
        }
    }
    out
}

/// Parse streamed tool arguments: returns (input without `purpose`, purpose).
pub fn parse_tool_input(raw: &str) -> Result<(Value, String), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok((Value::Object(Map::new()), String::new()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(mut o)) => {
            let purpose = match o.remove("purpose") {
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
pub fn partial_purpose(raw: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#""purpose"\s*:\s*"((?:[^"\\]|\\.)*)""#).expect("regex"));
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
        assert_eq!(partial_purpose(r#"{"purpose": "re\"ad", "pa"#).as_deref(), Some("re\"ad"));
        assert_eq!(partial_purpose(r#"{"purpose": "rea"#), None);
    }
}

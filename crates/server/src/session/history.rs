//! Session history -> LLM messages.

use hn_llm::{AssistantPart, Content, LlmMessage, ModelSpec};
use hn_proto::{Message, Origin, Part, Role, ToolState};

pub const IMAGE_OMITTED: &str = "[image omitted: current model does not support image input]";

/// Index of the most recent compaction checkpoint (context starts there).
pub fn context_start(msgs: &[Message]) -> usize {
    msgs.iter().rposition(|m| m.role == Role::User && m.origin == Origin::Compaction).unwrap_or(0)
}

/// Convert messages (already sliced to the context window) to LLM messages.
pub fn build(msgs: &[Message], spec: &ModelSpec) -> Vec<LlmMessage> {
    let replay_key = spec.replay_key();
    let mut out = Vec::new();
    for m in msgs {
        match m.role {
            Role::User => {
                let mut content = Vec::new();
                for p in &m.parts {
                    match p {
                        Part::Text { text, .. } => content.push(Content::Text(text.clone())),
                        Part::Image(img) => {
                            if spec.image_input {
                                content.push(Content::Image(img.clone()))
                            } else {
                                content.push(Content::Text(IMAGE_OMITTED.into()))
                            }
                        }
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    out.push(LlmMessage::User(content));
                }
            }
            Role::Assistant => {
                let replay = m.replay_key.as_deref() == Some(replay_key.as_str());
                let mut parts = Vec::new();
                let mut results = Vec::new();
                for p in &m.parts {
                    match p {
                        Part::Text { text, .. } => {
                            if !text.is_empty() {
                                parts.push(AssistantPart::Text(text.clone()))
                            }
                        }
                        Part::Reasoning { text, replay: data, .. } => {
                            parts.push(AssistantPart::Reasoning { text: text.clone(), replay: data.clone() })
                        }
                        Part::Tool(tp) => {
                            // Calls whose arguments never finished streaming were not executed.
                            if tp.state == ToolState::Streaming {
                                continue;
                            }
                            let arguments = if tp.raw_input.trim().is_empty() { "{}".to_string() } else { tp.raw_input.clone() };
                            parts.push(AssistantPart::ToolCall {
                                id: tp.call_id.clone(),
                                name: tp.name.clone(),
                                arguments,
                            });
                            let is_error = matches!(tp.state, ToolState::Error | ToolState::Denied | ToolState::Cancelled);
                            let content = if tp.state.is_final() {
                                if tp.output.is_empty() { "(no output)".to_string() } else { tp.output.clone() }
                            } else {
                                "Tool call was interrupted before it finished.".to_string()
                            };
                            let images = if spec.image_input { tp.images.clone() } else { Vec::new() };
                            results.push(LlmMessage::ToolResult {
                                call_id: tp.call_id.clone(),
                                name: tp.name.clone(),
                                content,
                                images,
                                is_error,
                            });
                        }
                        Part::Image(_) => {}
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                out.push(LlmMessage::Assistant { parts, replay });
                out.extend(results);
            }
        }
    }
    out
}

/// Rough token estimate of messages (used where no usage is known yet).
pub fn estimate(msgs: &[Message]) -> u64 {
    let mut n = 0;
    for m in msgs {
        for p in &m.parts {
            n += match p {
                Part::Text { text, .. } => hn_llm::estimate_tokens(text),
                Part::Reasoning { text, .. } => hn_llm::estimate_tokens(text),
                Part::Tool(tp) => hn_llm::estimate_tokens(&tp.raw_input) + hn_llm::estimate_tokens(&tp.output) + 10,
                Part::Image(_) => 1000,
            };
        }
        n += 4;
    }
    n
}

/// Context tokens in use: last usage within the context window + estimate of newer messages.
pub fn used_tokens(msgs: &[Message]) -> u64 {
    let start = context_start(msgs);
    let ctx = &msgs[start..];
    match ctx.iter().rposition(|m| m.usage.is_some()) {
        Some(i) => {
            let base = ctx[i].usage.as_ref().map(|u| u.context_tokens()).unwrap_or(0);
            base + estimate(&ctx[i + 1..])
        }
        None => estimate(ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hn_proto::ToolPart;

    fn msg(role: Role, origin: Origin, parts: Vec<Part>) -> Message {
        Message {
            id: crate::util::new_id("msg"),
            role,
            origin,
            time: 0,
            completed: Some(0),
            parts,
            model: None,
            agent: None,
            usage: None,
            finish: None,
            error: None,
            replay_key: None,
        }
    }

    #[test]
    fn tool_results_follow_calls_and_images_are_replaced() {
        let spec = ModelSpec::default();
        let tp = ToolPart {
            call_id: "c1".into(),
            name: "bash".into(),
            raw_input: "{\"command\":\"ls\"}".into(),
            state: ToolState::Done,
            output: "a\nb".into(),
            ..Default::default()
        };
        let streaming = ToolPart { call_id: "c2".into(), name: "bash".into(), ..Default::default() };
        let msgs = vec![
            msg(
                Role::User,
                Origin::Human,
                vec![
                    Part::Text { text: "hi".into(), time: 0 },
                    Part::Image(hn_proto::ImageData { mime: "image/png".into(), data: "AA".into() }),
                ],
            ),
            msg(Role::Assistant, Origin::Human, vec![Part::Tool(tp), Part::Tool(streaming)]),
        ];
        let out = build(&msgs, &spec);
        assert_eq!(out.len(), 3);
        match &out[0] {
            LlmMessage::User(c) => assert!(matches!(&c[1], Content::Text(t) if t == IMAGE_OMITTED)),
            _ => panic!(),
        }
        match &out[1] {
            LlmMessage::Assistant { parts, .. } => assert_eq!(parts.len(), 1),
            _ => panic!(),
        }
        assert!(matches!(&out[2], LlmMessage::ToolResult { content, .. } if content == "a\nb"));
    }

    #[test]
    fn context_starts_at_last_checkpoint() {
        let msgs = vec![
            msg(Role::User, Origin::Human, vec![]),
            msg(Role::User, Origin::Compaction, vec![]),
            msg(Role::User, Origin::Human, vec![]),
            msg(Role::User, Origin::Compaction, vec![]),
            msg(Role::User, Origin::Human, vec![]),
        ];
        assert_eq!(context_start(&msgs), 3);
        assert_eq!(context_start(&msgs[..1]), 0);
    }
}

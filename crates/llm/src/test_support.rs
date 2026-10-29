//! Fixtures shared by the unit tests.

use crate::{ApiKind, AssistantBlock, ChatMessage, Model, Provider, Request, ToolDef, UserContent};
use ref_proto::ImageData;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

pub(crate) fn provider(id: &str, api: ApiKind, base_url: &str) -> Provider {
    Provider {
        id: id.into(),
        api,
        base_url: base_url.into(),
        api_key: Some("sk-test".into()),
        headers: BTreeMap::new(),
        set_cache_key: false,
        chunk_timeout: Duration::from_secs(5),
    }
}

pub(crate) fn model(id: &str, options: Value) -> Model {
    Model {
        id: id.into(),
        context: 200_000,
        output: 8192,
        images: true,
        reasoning: true,
        options: options.as_object().cloned().unwrap_or_default(),
        temperature: None,
        top_p: None,
    }
}

pub(crate) fn image() -> ImageData {
    ImageData {
        mime: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
        ..Default::default()
    }
}

pub(crate) fn tool() -> ToolDef {
    ToolDef {
        name: "read".into(),
        description: "Read a file".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
    }
}

/// A conversation with an image, reasoning + tool call, a tool result with
/// an image, and a follow-up user message.
pub(crate) fn conversation(
    origin: (&str, &str),
    reasoning_data: Option<Value>,
    call_data: Option<Value>,
) -> Request {
    Request {
        system: vec!["You are a coding agent.".into(), "cwd: /tmp".into()],
        messages: vec![
            ChatMessage::User(vec![
                UserContent::Text("look at this".into()),
                UserContent::Image(image()),
            ]),
            ChatMessage::Assistant {
                blocks: vec![
                    AssistantBlock::Reasoning {
                        text: "Let me read the file.".into(),
                        provider_data: reasoning_data,
                    },
                    AssistantBlock::Text("Reading it.".into()),
                    AssistantBlock::ToolCall {
                        id: "call_1".into(),
                        name: "read".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                        provider_data: call_data,
                    },
                ],
                origin: Some((origin.0.into(), origin.1.into())),
            },
            ChatMessage::ToolResult {
                call_id: "call_1".into(),
                name: "read".into(),
                content: "fn main() {}".into(),
                images: vec![image()],
                is_error: false,
            },
            ChatMessage::User(vec![UserContent::Text("thanks".into())]),
        ],
        tools: vec![tool()],
        session_id: "ses_123".into(),
    }
}

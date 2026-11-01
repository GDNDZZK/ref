//! Names and pure helpers shared by the client layers and the in-process mock backend.

use ref_proto::{Delta, ModelRef, Part, SessionInfo, SessionStatus, ToolState};

/// Error text a message carries when its turn was aborted (matched case-insensitively).
pub const ABORTED_ERROR: &str = "Aborted";

/// Status strings of an MCP server as the backend reports them.
pub mod mcp_status {
    pub const CONNECTED: &str = "connected";
    pub const CONNECTING: &str = "connecting";
    pub const FAILED: &str = "failed";
    pub const DISABLED: &str = "disabled";
}

/// Slash-command names, shared by the `COMMANDS` table and everything that dispatches on them.
pub mod cmd {
    pub const NEW: &str = "new";
    pub const SESSIONS: &str = "sessions";
    pub const MODELS: &str = "models";
    pub const AGENTS: &str = "agents";
    pub const VARIANT: &str = "variant";
    pub const COMPACT: &str = "compact";
    pub const UNDO: &str = "undo";
    pub const RENAME: &str = "rename";
    pub const CD: &str = "cd";
    pub const CLOSE: &str = "close";
    pub const CLOSE_SESSION: &str = "close-session";
    pub const PEERS: &str = "peers";
    pub const PEERS_NAME: &str = "peers-name";
    pub const PEERS_INBOX: &str = "peers-inbox";
    pub const PEERS_OUTBOX: &str = "peers-outbox";
    pub const EXPORT: &str = "export";
    pub const COPY: &str = "copy";
    pub const THINKING: &str = "thinking";
    pub const DETAILS: &str = "details";
    pub const WATCH: &str = "watch";
    pub const SIDEBAR: &str = "sidebar";
    pub const MCP: &str = "mcp";
    pub const EDITOR: &str = "editor";
    pub const HELP: &str = "help";
    pub const EXIT: &str = "exit";
}

/// Session info for a session the client has not heard about yet.
pub(crate) fn placeholder_info(sid: &str, cwd: &str) -> SessionInfo {
    SessionInfo {
        id: sid.to_string(),
        name: String::new(),
        title: String::new(),
        agent: String::new(),
        model: ModelRef::default(),
        cwd: cwd.to_string(),
        created_at: 0,
        updated_at: 0,
        status: SessionStatus::Idle,
        attached: 0,
        live: true,
        parent: None,
        must_report: false,
        context: Default::default(),
        todos: Vec::new(),
        busy_since: None,
        last_error: None,
    }
}

/// The JSON key whose value `partial_purpose` extracts.
const PURPOSE_KEY: &str = "\"purpose\"";

/// Applies a streaming delta to the parts of a message, creating the part if it is new.
pub fn apply_delta(parts: &mut Vec<Part>, index: usize, delta: Delta) {
    if index >= parts.len() {
        // Delta for a part we haven't seen: create it.
        let now = ref_proto::now_ms();
        let p = match &delta {
            Delta::Text { .. } => Part::Text {
                text: String::new(),
                time: now,
            },
            Delta::Reasoning { .. } => Part::Reasoning {
                text: String::new(),
                time: now,
                time_end: None,
                provider_data: None,
            },
            Delta::ToolInput { .. } => Part::Tool {
                call_id: format!("pending-{index}"),
                name: String::new(),
                purpose: String::new(),
                input: serde_json::Value::Null,
                raw_input: String::new(),
                state: ToolState::Streaming,
                output: None,
                meta: None,
                images: Vec::new(),
                time: now,
                time_end: None,
                run_at: None,
                provider_data: None,
            },
        };
        while parts.len() < index {
            parts.push(Part::Text {
                text: String::new(),
                time: now,
            });
        }
        parts.push(p);
    }
    match (&mut parts[index], delta) {
        (Part::Text { text, .. }, Delta::Text { text: d }) => text.push_str(&d),
        (Part::Reasoning { text, .. }, Delta::Reasoning { text: d }) => text.push_str(&d),
        (
            Part::Tool {
                raw_input, purpose, ..
            },
            Delta::ToolInput { text: d },
        ) => {
            raw_input.push_str(&d);
            if purpose.is_empty()
                && let Some(p) = partial_purpose(raw_input)
            {
                *purpose = p;
            }
        }
        // Mismatched delta type: append text where it makes sense, otherwise ignore.
        (Part::Text { text, .. }, Delta::Reasoning { text: d }) => text.push_str(&d),
        (Part::Reasoning { text, .. }, Delta::Text { text: d }) => text.push_str(&d),
        _ => {}
    }
}

/// Extracts a (possibly partial) `"purpose": "..."` value from streaming tool JSON.
fn partial_purpose(raw: &str) -> Option<String> {
    let k = raw.find(PURPOSE_KEY)?;
    let rest = raw[k + PURPOSE_KEY.len()..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in rest.chars() {
        if esc {
            out.push(match c {
                'n' => ' ',
                't' => ' ',
                c => c,
            });
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_extraction() {
        assert_eq!(
            partial_purpose(r#"{"purpose":"Read \"x\" file","path":"a"}"#).as_deref(),
            Some("Read \"x\" file")
        );
        assert_eq!(partial_purpose(r#"{"path":"a"}"#), None);
        assert_eq!(partial_purpose(r#"{"purpose": "#), None);
    }
}

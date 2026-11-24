//! Names and pure helpers shared by the client layers and the in-process mock backend.

use ref_proto::{
    Delta, Message, MessageLayoutMeta, MessageSource, ModelRef, Part, PartLayoutMeta, SessionInfo,
    SessionStatus, ToolState,
};
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
    pub const RELOAD: &str = "reload";
    pub const BACKEND: &str = "backend";
    pub const DETACH: &str = "detach";
    pub const RETRY: &str = "retry";
    pub const STASH: &str = "stash";
    pub const STASHED: &str = "stashed";
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

fn source_kind_str(source: &MessageSource) -> &'static str {
    match source {
        MessageSource::Human => "human",
        MessageSource::Peer { .. } => "peer",
        MessageSource::Spawn { .. } => "spawn",
        MessageSource::Reminder => "reminder",
        MessageSource::Compaction => "compaction",
        MessageSource::Shell => "shell",
    }
}

fn count_lines_u32(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }
    s.lines().count().max(1).min(u32::MAX as usize) as u32
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('|')
        && t.contains('-')
        && t.chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
}

fn analyze_markdown(text: &str) -> (u16, u32, u16) {
    let mut code_blocks: u16 = 0;
    let mut code_lines: u32 = 0;
    let mut tables: u16 = 0;
    let mut in_fence = false;
    let mut in_table = false;
    let mut prev_has_pipe = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
            } else {
                in_fence = true;
                code_blocks = code_blocks.saturating_add(1);
            }
            in_table = false;
            prev_has_pipe = false;
            continue;
        }
        if in_fence {
            code_lines = code_lines.saturating_add(1);
            continue;
        }
        let has_pipe = line.contains('|');
        if prev_has_pipe && is_table_separator(line) {
            if !in_table {
                tables = tables.saturating_add(1);
                in_table = true;
            }
        } else if !(in_table && has_pipe) {
            in_table = false;
        }
        prev_has_pipe = has_pipe;
    }
    (code_blocks, code_lines, tables)
}

/// Computes lightweight [`MessageLayoutMeta`] from an in-memory [`Message`].
pub fn compute_layout_meta(seq: u64, msg: &Message) -> MessageLayoutMeta {
    let parts = msg
        .parts
        .iter()
        .map(|p| match p {
            Part::Text { text, .. } => {
                let (code_blocks, code_lines, tables) = analyze_markdown(text);
                PartLayoutMeta {
                    kind: "text".into(),
                    chars: text.chars().count().min(u32::MAX as usize) as u32,
                    lines: count_lines_u32(text),
                    code_blocks,
                    code_lines,
                    tables,
                    tool: None,
                    tool_lines: 0,
                    width: None,
                    height: None,
                }
            }
            Part::Reasoning { text, .. } => PartLayoutMeta {
                kind: "reasoning".into(),
                chars: text.chars().count().min(u32::MAX as usize) as u32,
                lines: count_lines_u32(text),
                code_blocks: 0,
                code_lines: 0,
                tables: 0,
                tool: None,
                tool_lines: 0,
                width: None,
                height: None,
            },
            Part::Image { .. } => PartLayoutMeta {
                kind: "image".into(),
                chars: 0,
                lines: 0,
                code_blocks: 0,
                code_lines: 0,
                tables: 0,
                tool: None,
                tool_lines: 0,
                width: None,
                height: None,
            },
            Part::Tool {
                name,
                purpose,
                input,
                output,
                meta,
                ..
            } => {
                let lines = if name == "todo" {
                    input
                        .get("todos")
                        .and_then(|v| v.as_array())
                        .map_or(0, Vec::len)
                } else if let Some(diff) = meta
                    .as_ref()
                    .and_then(|m| m.get("diff"))
                    .and_then(|v| v.as_str())
                {
                    diff.lines().count()
                } else {
                    output.as_deref().map_or(0, |o| o.lines().count())
                };
                let tool_lines = lines.min(u16::MAX as usize) as u16;
                let chars = (purpose.chars().count()
                    + output.as_deref().map_or(0, |o| o.chars().count()))
                .min(u32::MAX as usize) as u32;
                PartLayoutMeta {
                    kind: "tool".into(),
                    chars,
                    lines: tool_lines as u32,
                    code_blocks: 0,
                    code_lines: 0,
                    tables: 0,
                    tool: Some(name.clone()),
                    tool_lines,
                    width: None,
                    height: None,
                }
            }
        })
        .collect();

    MessageLayoutMeta {
        seq,
        id: msg.id.clone(),
        role: msg.role,
        source_kind: source_kind_str(&msg.source).into(),
        has_error: msg.error.is_some(),
        parts,
    }
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

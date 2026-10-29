//! Tool-call execution: purpose stripping, permission, delay, run, result recording.

use std::sync::Arc;
use std::time::Duration;

use ref_config::Action;
use ref_proto::{Part, PermissionDecision, PermissionRequest, ServerMsg, ToolPart, ToolState};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::engine::{StepEnv, ABORTED_OUTPUT};
use super::{prompt, tools, PendingPerm, SessionCore};
use crate::util;

const MCP_TIMEOUT: Duration = Duration::from_secs(600);

pub struct ToolResult {
    pub text: String,
    pub images: Vec<ref_proto::ImagePart>,
    pub state: ToolState,
}

impl ToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        ToolResult { text: text.into(), images: vec![], state: ToolState::Done }
    }
    pub fn err(text: impl Into<String>) -> Self {
        ToolResult { text: text.into(), images: vec![], state: ToolState::Error }
    }
}

/// Execute all pending tool calls of an assistant message concurrently.
pub(crate) async fn run_tools(core: &Arc<SessionCore>, tok: &CancellationToken, env: &StepEnv, msg_id: &str) {
    let calls: Vec<ToolPart> = {
        let st = core.st.lock();
        let Some(i) = st.msg_index(msg_id) else { return };
        st.messages[i]
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Tool(t) if !t.state.is_final() => Some(t.clone()),
                _ => None,
            })
            .collect()
    };
    // Consecutive read-only calls run concurrently; any other call runs alone, in the order the
    // model issued them, so e.g. `edit` then `bash cat` observes the edit.
    let mut batch: Vec<ToolPart> = Vec::new();
    for c in calls {
        if is_read_only(&c.name) {
            batch.push(c);
            continue;
        }
        if !batch.is_empty() {
            futures::future::join_all(batch.drain(..).map(|c| run_one(core, tok, env, msg_id, c))).await;
        }
        run_one(core, tok, env, msg_id, c).await;
    }
    if !batch.is_empty() {
        futures::future::join_all(batch.into_iter().map(|c| run_one(core, tok, env, msg_id, c))).await;
    }
}

fn is_read_only(name: &str) -> bool {
    matches!(
        name,
        "read" | "grep" | "find" | "context_info" | "list_sessions" | "read_messages" | "session_context" | "skill"
    )
}

/// Subject for pattern permissions: bash command / file path.
fn subject_of(name: &str, input: &Value) -> Option<String> {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(String::from);
    match name {
        "bash" => s("command"),
        "read" | "write" | "delete" | "find" | "grep" => s("path"),
        "edit" => s("path").or_else(|| {
            // Hashline patch: first `[path#TAG]` header.
            let text = s("input")?;
            let line = text.lines().find(|l| l.trim_start().starts_with('['))?;
            let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
            Some(inner.rsplit_once('#').map(|x| x.0).unwrap_or(inner).to_string())
        }),
        _ => None,
    }
}

fn set_state(core: &SessionCore, msg_id: &str, part_id: &str, f: impl FnOnce(&mut ToolPart)) {
    let mut st = core.st.lock();
    st.update_part(&core.hub, msg_id, part_id, |p| {
        if let Part::Tool(t) = p {
            f(t)
        }
    });
}

fn finish(core: &SessionCore, msg_id: &str, part_id: &str, r: ToolResult, limit: usize) {
    let text = util::truncate_output(r.text, limit);
    set_state(core, msg_id, part_id, |t| {
        t.state = r.state;
        t.output = text;
        t.images = r.images;
        t.run_at = None;
        t.time_end = Some(ref_proto::now_ms());
    });
}

async fn run_one(core: &Arc<SessionCore>, tok: &CancellationToken, env: &StepEnv, msg_id: &str, call: ToolPart) {
    let limit = env.cfg.tool_output_limit;
    let part_id = call.id.clone();
    let name = call.name.clone();
    // Arguments.
    let input = if call.input.is_object() && call.input_raw.is_empty() {
        call.input.clone()
    } else if !call.input_raw.trim().is_empty() {
        match ref_llm::parse_tool_input(&call.input_raw) {
            v @ Value::Object(_) => v,
            _ => {
                let msg = format!(
                    "Invalid tool arguments: expected a JSON object. Received: {}",
                    util::ellipsize(&call.input_raw, 500)
                );
                finish(core, msg_id, &part_id, ToolResult::err(msg), limit);
                return;
            }
        }
    } else {
        call.input.clone()
    };
    let input = if input.is_object() { input } else { Value::Object(Default::default()) };
    let (purpose, args) = prompt::split_purpose(&input);
    set_state(core, msg_id, &part_id, |t| {
        if t.purpose.is_empty() {
            t.purpose = purpose.clone();
        }
        t.input = input.clone();
    });
    let known = ref_tools::BUILTIN_TOOLS.contains(&name.as_str())
        || env.session_tools.contains(&name)
        || env.mcp_names.contains(&name);
    if !known {
        finish(core, msg_id, &part_id, ToolResult::err(format!("Unknown tool: {name}")), limit);
        return;
    }
    if tok.is_cancelled() {
        finish(core, msg_id, &part_id, cancelled(), limit);
        return;
    }

    // Permission.
    let subject = subject_of(&name, &args);
    let action = env.cfg.permission(&env.agent, &name, subject.as_deref());
    match action {
        Action::Deny => {
            let r = ToolResult { text: "Permission denied by configuration".into(), images: vec![], state: ToolState::Denied };
            finish(core, msg_id, &part_id, r, limit);
            return;
        }
        Action::Ask if !core.st.lock().allowed.contains(&name) => {
            match ask_permission(core, tok, msg_id, &call, &purpose, &input).await {
                Some(PermissionDecision::Once) | Some(PermissionDecision::Always) => {}
                Some(PermissionDecision::Deny) => {
                    let r = ToolResult { text: "The user denied this tool call".into(), images: vec![], state: ToolState::Denied };
                    finish(core, msg_id, &part_id, r, limit);
                    return;
                }
                None => {
                    finish(core, msg_id, &part_id, cancelled(), limit);
                    return;
                }
            }
        }
        _ => {}
    }

    // Delay.
    let delay = env.cfg.delay(&env.agent, &name);
    if !delay.is_zero() {
        let run_at = ref_proto::now_ms() + delay.as_millis() as i64;
        set_state(core, msg_id, &part_id, |t| {
            t.state = ToolState::Delayed;
            t.run_at = Some(run_at);
        });
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = tok.cancelled() => {
                finish(core, msg_id, &part_id, cancelled(), limit);
                return;
            }
        }
    }

    set_state(core, msg_id, &part_id, |t| {
        t.state = ToolState::Running;
        t.run_at = None;
        t.time_start = Some(ref_proto::now_ms());
    });
    let abort = tok.child_token();
    let result = if ref_tools::BUILTIN_TOOLS.contains(&name.as_str()) {
        run_builtin(core, env, msg_id, &part_id, &name, args, abort.clone()).await
    } else if env.session_tools.contains(&name) {
        tokio::select! {
            r = tools::execute(core, env, &name, args) => r,
            _ = abort.cancelled() => cancelled(),
        }
    } else {
        match &env.mcp {
            Some(m) => {
                let r = m.call(&name, args, abort.clone(), MCP_TIMEOUT).await;
                ToolResult { text: r.text, images: r.images, state: if r.is_error { ToolState::Error } else { ToolState::Done } }
            }
            None => ToolResult::err(format!("MCP tool {name} is unavailable")),
        }
    };
    let result = if tok.is_cancelled() && result.state != ToolState::Done { cancelled() } else { result };
    finish(core, msg_id, &part_id, result, limit);
}

fn cancelled() -> ToolResult {
    ToolResult { text: ABORTED_OUTPUT.into(), images: vec![], state: ToolState::Cancelled }
}

async fn run_builtin(
    core: &Arc<SessionCore>,
    env: &StepEnv,
    msg_id: &str,
    part_id: &str,
    name: &str,
    args: Value,
    abort: CancellationToken,
) -> ToolResult {
    let (ptx, mut prx) = mpsc::unbounded_channel::<String>();
    let ctx = ref_tools::ToolCtx {
        cwd: core.cwd().into(),
        abort,
        store: core.snapshots.clone(),
        image_input: env.spec.image_input,
        progress: Some(ptx),
    };
    // Forward live output, coalesced.
    let fwd = {
        let core = core.clone();
        let (sid, mid, pid) = (core.id.clone(), msg_id.to_string(), part_id.to_string());
        tokio::spawn(async move {
            while let Some(mut chunk) = prx.recv().await {
                tokio::time::sleep(Duration::from_millis(30)).await;
                while let Ok(more) = prx.try_recv() {
                    chunk.push_str(&more);
                    if chunk.len() > 64 * 1024 {
                        break;
                    }
                }
                core.hub.broadcast_session(
                    &sid,
                    &ServerMsg::ToolOutputDelta {
                        session_id: sid.clone(),
                        message_id: mid.clone(),
                        part_id: pid.clone(),
                        delta: chunk,
                    },
                );
            }
        })
    };
    let out = ref_tools::execute(name, args, &ctx).await;
    drop(ctx);
    // The forwarder ends once all senders are dropped; don't wait forever on stray clones.
    let _ = tokio::time::timeout(Duration::from_secs(1), fwd).await;
    ToolResult { text: out.text, images: out.images, state: if out.is_error { ToolState::Error } else { ToolState::Done } }
}

/// Ask the user. `None` = aborted.
async fn ask_permission(
    core: &Arc<SessionCore>,
    tok: &CancellationToken,
    msg_id: &str,
    call: &ToolPart,
    purpose: &str,
    input: &Value,
) -> Option<PermissionDecision> {
    let (tx, rx) = oneshot::channel();
    let req = PermissionRequest {
        session_id: core.id.clone(),
        message_id: msg_id.to_string(),
        part_id: call.id.clone(),
        call_id: call.call_id.clone(),
        tool: call.name.clone(),
        purpose: purpose.to_string(),
        input: input.clone(),
        time: ref_proto::now_ms(),
    };
    {
        let mut st = core.st.lock();
        if tok.is_cancelled() {
            return None;
        }
        st.update_part(&core.hub, msg_id, &call.id, |p| {
            if let Part::Tool(t) = p {
                t.state = ToolState::AwaitingPermission;
            }
        });
        st.permissions.push(PendingPerm { req: req.clone(), tx: Some(tx) });
        st.bcast(&core.hub, &ServerMsg::PermissionAsk { request: req });
        st.info_changed(&core.hub);
    }
    let r = tokio::select! {
        d = rx => d.ok(),
        _ = tok.cancelled() => None,
    };
    // Make sure the request is gone (e.g. aborted).
    let mut st = core.st.lock();
    if let Some(i) = st.permissions.iter().position(|p| p.req.call_id == call.call_id) {
        st.permissions.remove(i);
        st.bcast(&core.hub, &ServerMsg::PermissionResolved { session_id: core.id.clone(), call_id: call.call_id.clone() });
        st.info_changed(&core.hub);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subjects() {
        assert_eq!(subject_of("bash", &json!({"command": "ls -la"})).as_deref(), Some("ls -la"));
        assert_eq!(subject_of("write", &json!({"path": "a.txt"})).as_deref(), Some("a.txt"));
        assert_eq!(subject_of("edit", &json!({"input": "[src/a.rs#1A2B]\nPUT 1:\n+x"})).as_deref(), Some("src/a.rs"));
        assert_eq!(subject_of("context_info", &json!({})), None);
    }
}

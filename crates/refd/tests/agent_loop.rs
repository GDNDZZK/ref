//! Agent loop tests: multiple tool calls, tool errors, unknown tools, invalid JSON, purpose
//! injection (builtin / session / MCP), execution ordering, prompt-cache stability.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use ref_proto::{ClientMsg, ServerMsg, ToolState};
use serde_json::{json, Value};

/// A minimal stdio MCP server (python3) with two tools: `echo` and `zeta`, whose `echo`
/// records the arguments it received in `<dir>/mcp_args.jsonl`.
fn write_mcp_server(dir: &std::path::Path) -> Vec<String> {
    let script = r#"
import sys, json
log = sys.argv[1]
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    mid = msg.get("id")
    m = msg.get("method")
    if mid is None:
        continue
    if m == "initialize":
        res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "t", "version": "1"}}
    elif m == "tools/list":
        res = {"tools": [
            {"name": "zeta", "description": "Z tool", "inputSchema": {"type": "object", "properties": {}}},
            {"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}}
        ]}
    elif m == "tools/call":
        args = msg["params"].get("arguments", {})
        with open(log, "a") as f:
            f.write(json.dumps(args) + "\n")
        res = {"content": [{"type": "text", "text": "mcp says " + str(args.get("text"))}]}
    else:
        res = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": mid, "result": res}) + "\n")
    sys.stdout.flush()
"#;
    let path = dir.join("mcp_server.py");
    std::fs::write(&path, script).unwrap();
    vec!["python3".into(), path.to_string_lossy().into_owned(), dir.join("mcp_args.jsonl").to_string_lossy().into_owned()]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_tool_calls_errors_unknown_and_invalid_json() {
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            return Reply::Text("finished".into());
        }
        let mut calls: Vec<(String, String)> = Vec::new();
        // 12 parallel reads (unlimited count), each of a different file.
        for i in 0..12 {
            calls.push(("read".into(), json!({"purpose": format!("read {i}"), "path": format!("f{i}.txt")}).to_string()));
        }
        calls.push(("read".into(), json!({"purpose": "missing", "path": "does-not-exist.txt"}).to_string()));
        calls.push(("no_such_tool".into(), json!({"purpose": "x"}).to_string()));
        calls.push(("bash".into(), "{not json at all".into()));
        calls.push(("bash".into(), json!({"purpose": "fail", "command": "exit 3"}).to_string()));
        Reply::RawTools(calls)
    });
    let env = setup(script, json!({})).await;
    for i in 0..12 {
        std::fs::write(format!("{}/f{i}.txt", env.cwd), format!("content-{i}\n")).unwrap();
    }
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "go").await;
    c.until_idle_with(&sid, "finished").await;

    let tools = env.tool_parts(&sid);
    assert_eq!(tools.len(), 16);
    for (i, t) in tools.iter().take(12).enumerate() {
        assert_eq!(t.state, ToolState::Done, "{}", t.output);
        assert!(t.output.contains(&format!("content-{i}")), "{}", t.output);
        assert_eq!(t.purpose, format!("read {i}"));
    }
    assert_eq!(tools[12].state, ToolState::Error);
    assert_eq!(tools[13].state, ToolState::Error);
    assert!(tools[13].output.contains("Unknown tool: no_such_tool"), "{}", tools[13].output);
    assert_eq!(tools[14].state, ToolState::Error);
    assert!(tools[14].output.contains("Invalid tool arguments"), "{}", tools[14].output);
    assert_eq!(tools[15].state, ToolState::Error, "{}", tools[15].output);

    // All results (incl. errors) were returned to the model and the loop continued.
    let reqs = env.llm_requests();
    assert_eq!(reqs.len(), 2);
    let results = tool_result_content(&reqs[1]);
    assert_eq!(results.len(), 16);
    assert!(results[13].contains("Unknown tool"));
    assert!(results[14].contains("Invalid tool arguments"));
    // The invalid call is replayed with an object input (providers reject other JSON).
    let asst = reqs[1]["messages"].as_array().unwrap().iter().find(|m| m["role"] == "assistant").unwrap();
    let calls = asst["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 16);
    for call in calls {
        let args: Value = serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap_or(Value::Null);
        assert!(args.is_object() || call["function"]["name"] == "bash", "{call}");
    }
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn purpose_in_every_schema_and_stripped_before_execution() {
    let tmp = tempfile::tempdir().unwrap();
    let mcp_cmd = write_mcp_server(tmp.path());
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            return Reply::Text("ok".into());
        }
        Reply::Tools(vec![
            ("t_echo".into(), json!({"purpose": "call mcp", "text": "hello"})),
            ("context_info".into(), json!({"purpose": "check"})),
            ("bash".into(), json!({"purpose": "print args", "command": "echo \"$0\" ran"})),
        ])
    });
    let env = setup(script, json!({"mcp": {"t": {"type": "local", "command": mcp_cmd}}})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "go").await;
    c.until_idle_with(&sid, "ok").await;

    let reqs = env.llm_requests();
    let names = tool_names(&reqs[0]);
    for n in ["read", "bash", "context_info", "compact", "list_sessions", "send_message", "read_messages", "session_context", "create_session", "close_session", "t_echo", "t_zeta"] {
        assert!(names.contains(&n.to_string()), "{n} missing from {names:?}");
    }
    // MCP tools come last, sorted.
    let ti = names.iter().position(|n| n == "t_echo").unwrap();
    assert_eq!(&names[ti..], &["t_echo".to_string(), "t_zeta".to_string()]);
    for t in reqs[0]["tools"].as_array().unwrap() {
        let p = &t["function"]["parameters"];
        assert_eq!(p["properties"].as_object().unwrap().keys().next().unwrap(), "purpose", "{t}");
        assert_eq!(p["required"][0], "purpose", "{t}");
        assert_eq!(p["properties"]["purpose"]["type"], "string");
    }
    // The MCP server received the arguments without `purpose`.
    let logged = std::fs::read_to_string(tmp.path().join("mcp_args.jsonl")).unwrap();
    let args: Value = serde_json::from_str(logged.lines().next().unwrap()).unwrap();
    assert_eq!(args, json!({"text": "hello"}));
    let tools = env.tool_parts(&sid);
    assert_eq!(tools[0].output, "mcp says hello");
    assert_eq!(tools[0].purpose, "call mcp");
    // The stored input keeps purpose (replay is faithful).
    assert_eq!(tools[0].input["purpose"], "call mcp");
    assert_eq!(tools[2].state, ToolState::Done, "{}", tools[2].output);
    // Tool order is deterministic across steps.
    assert_eq!(reqs[0]["tools"], reqs[1]["tools"]);
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_only_tools_parallel_others_sequential() {
    let script: Script = Arc::new(|body| {
        let (role, _) = last_content(body);
        if role == "tool" {
            return Reply::Text("ok".into());
        }
        Reply::Tools(vec![
            ("write".into(), json!({"purpose": "w", "path": "a.txt", "content": "first\n"})),
            ("bash".into(), json!({"purpose": "b", "command": "sleep 0.3; cat a.txt; echo second > a.txt"})),
            ("read".into(), json!({"purpose": "r1", "path": "a.txt"})),
            ("grep".into(), json!({"purpose": "r2", "pattern": "second"})),
            ("bash".into(), json!({"purpose": "c", "command": "echo third >> a.txt; cat a.txt"})),
        ])
    });
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "go").await;
    c.until_idle_with(&sid, "ok").await;
    let t = env.tool_parts(&sid);
    assert!(t.iter().all(|t| t.state == ToolState::Done), "{:?}", t.iter().map(|t| &t.output).collect::<Vec<_>>());
    assert!(t[1].output.contains("first"), "{}", t[1].output);
    assert!(t[2].output.contains("second"), "{}", t[2].output);
    assert!(t[3].output.contains("second"), "{}", t[3].output);
    assert!(t[4].output.contains("second\nthird"), "{}", t[4].output);
    // Sequential: each side-effecting call starts after the previous finished; reads overlap.
    let s = |i: usize| t[i].time_start.unwrap();
    let e = |i: usize| t[i].time_end.unwrap();
    assert!(s(1) >= e(0));
    assert!(s(2) >= e(1) && s(3) >= e(1));
    assert!(s(4) >= e(2).max(e(3)));
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prompt_cache_prefix_stable_across_steps_and_turns() {
    let env = setup(cmd_script(), json!({"small_model": "mock/small"})).await;
    std::fs::write(format!("{}/AGENTS.md", env.cwd), "Project rule: be terse.\n").unwrap();
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    // Turn 1: tool step + final; title generation runs meanwhile. Turn 2: plain text.
    c.turn(&sid, &call(json!([["bash", {"command": "echo hi"}]]))).await;
    wait_until(Duration::from_secs(5), || env.core(&sid).name() == "Generated title").await;
    c.turn(&sid, "second turn").await;
    let reqs = env.llm_requests();
    assert_eq!(reqs.len(), 3);
    let sys0 = serde_json::to_string(&reqs[0]["messages"][0]).unwrap();
    assert!(sys0.contains("Project rule: be terse."));
    let tools0 = serde_json::to_string(&reqs[0]["tools"]).unwrap();
    for r in &reqs[1..] {
        assert_eq!(serde_json::to_string(&r["messages"][0]).unwrap(), sys0, "system prompt changed");
        assert_eq!(serde_json::to_string(&r["tools"]).unwrap(), tools0, "tools changed");
    }
    // Each request extends the previous one: earlier messages are byte-identical.
    for w in reqs.windows(2) {
        let a = w[0]["messages"].as_array().unwrap();
        let b = w[1]["messages"].as_array().unwrap();
        assert!(b.len() > a.len());
        for (x, y) in a.iter().zip(b) {
            assert_eq!(serde_json::to_string(x).unwrap(), serde_json::to_string(y).unwrap());
        }
    }
    // Title came from small_model, not the session model; system prompt unaffected (checked).
    let titles = env.title_requests();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0]["model"], "small");
    env.server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reasoning_and_delta_coalescing() {
    let long: String = "abcdefghij".repeat(200); // 2000 chars → ~667 SSE deltas
    let long2 = long.clone();
    let script: Script = Arc::new(move |_| Reply::Reasoning("thinking hard".into(), long2.clone()));
    let env = setup(script, json!({})).await;
    let mut c = Client::connect(&env).await;
    let sid = c.create(&env.cwd).await;
    c.prompt(&sid, "go").await;
    c.until_idle_with(&sid, &long).await;
    let deltas = c.log.iter().filter(|m| matches!(m, ServerMsg::PartDelta { .. })).count();
    assert!(deltas < 100, "deltas were not coalesced: {deltas}");
    let msgs = env.messages(&sid);
    assert!(matches!(&msgs[1].parts[0], ref_proto::Part::Reasoning(r) if r.text == "thinking hard"));
    assert_eq!(msgs[1].raw_text(), long);
    c.send(ClientMsg::Ping { req: 5 }).await;
    c.until(|m| matches!(m, ServerMsg::Pong { req: 5 })).await;
    env.server.shutdown();
}

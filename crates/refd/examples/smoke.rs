//! Real-model smoke test for a running refd (not run by `cargo test`).
//!
//! ```text
//! REF_DATA_DIR=/tmp/x target/debug/refd --port 0 --idle-timeout -1 &
//! REF_DATA_DIR=/tmp/x cargo run -p refd --example smoke -- [provider/model] [cwd]
//! ```
//!
//! Connects to the first live instance in the registry of `$REF_DATA_DIR`, then:
//! 1. runs a multi-step tool-using turn (write a file, read it back, answer);
//! 2. asks the model to create a background session with must_report and waits for the
//!    child's report to arrive back in the parent.
//!
//! Prints a short transcript. Never prints the token or API keys.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use ref_proto::{ClientMsg, Message, MessageOrigin, Part, ServerMsg, SessionStatus, PROTOCOL_VERSION};
use tokio_tungstenite::tungstenite::Message as WsMsg;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Conn {
    ws: Ws,
}

impl Conn {
    async fn send(&mut self, m: ClientMsg) {
        self.ws.send(WsMsg::text(serde_json::to_string(&m).unwrap())).await.unwrap();
    }

    async fn next(&mut self) -> ServerMsg {
        loop {
            let m = self.ws.next().await.expect("socket closed").expect("ws error");
            if let Ok(t) = m.to_text() {
                if !t.is_empty() {
                    return serde_json::from_str(t).expect("bad server message");
                }
            }
        }
    }
}

fn summarize(m: &Message) -> String {
    let who = match (&m.role, &m.origin) {
        (ref_proto::Role::Assistant, _) => "assistant".to_string(),
        (_, MessageOrigin::User) => "user".into(),
        (_, MessageOrigin::Peer { from_name, .. }) => format!("peer<{from_name}>"),
        (_, MessageOrigin::Spawn { from_name, .. }) => format!("spawn<{from_name}>"),
        (_, MessageOrigin::Reminder) => "reminder".into(),
        (_, MessageOrigin::Compaction) => "compaction".into(),
    };
    let mut s = format!("[{who}]");
    for p in &m.parts {
        match p {
            Part::Text(t) => s.push_str(&format!(" text={:?}", ellipsize(&t.text, 160))),
            Part::Reasoning(r) => s.push_str(&format!(" reasoning({} chars)", r.text.len())),
            Part::Tool(t) => s.push_str(&format!(
                " tool={}({:?})[{:?}] -> {:?}",
                t.name,
                t.purpose,
                t.state,
                ellipsize(&t.output, 100)
            )),
            Part::Image(_) => s.push_str(" image"),
        }
    }
    if let Some(e) = &m.error {
        s.push_str(&format!(" error={e:?}"));
    }
    s
}

fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Run until the session goes idle after having been busy; returns its messages.
async fn wait_turn(c: &mut Conn, sid: &str, timeout: Duration) -> Vec<Message> {
    let mut busy = false;
    let fut = async {
        loop {
            match c.next().await {
                ServerMsg::SessionUpdated { info } if info.id == sid => match info.status {
                    SessionStatus::Busy => busy = true,
                    SessionStatus::Retrying { attempt, error, .. } => {
                        busy = true;
                        println!("  (retrying, attempt {attempt}: {})", ellipsize(&error, 200));
                        if attempt >= 3 {
                            c.send(ClientMsg::Abort { session_id: sid.to_string() }).await;
                            panic!("model keeps failing; aborted");
                        }
                    }
                    SessionStatus::Idle if busy => break,
                    SessionStatus::Idle => {}
                },
                ServerMsg::PermissionAsk { request } => println!("  (permission asked for {})", request.tool),
                ServerMsg::Error { text, .. } => println!("  (error: {text})"),
                _ => {}
            }
        }
    };
    tokio::time::timeout(timeout, fut).await.expect("turn timed out");
    snapshot(c, sid).await
}

async fn snapshot(c: &mut Conn, sid: &str) -> Vec<Message> {
    c.send(ClientMsg::Attach { req: 99, session_id: sid.into() }).await;
    loop {
        if let ServerMsg::SessionSnapshot { req: 99, messages, .. } = c.next().await {
            return messages;
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = args.get(1).cloned().unwrap_or_else(|| "zai/glm-5.3".into());
    let cwd = args.get(2).cloned().unwrap_or_else(|| {
        let d = std::env::temp_dir().join("ref-smoke-work");
        std::fs::create_dir_all(&d).unwrap();
        d.to_string_lossy().into_owned()
    });
    // Project config: spawned sessions use their agent's default model, make it this one.
    std::fs::write(format!("{cwd}/opencode.json"), serde_json::json!({"model": model}).to_string()).unwrap();
    let data_dir = ref_config::data_dir();
    let live = refd::registry::live_instances(&data_dir).await;
    let (entry, _) = live.into_iter().next().unwrap_or_else(|| panic!("no live refd in {}", data_dir.display()));
    println!("refd pid {} port {} (data dir {})", entry.instance.pid, entry.instance.port, data_dir.display());
    let url = format!("{}?token={}", entry.url(), entry.token);
    let (ws, _) = tokio_tungstenite::connect_async(url).await.expect("connect");
    let mut c = Conn { ws };
    c.send(ClientMsg::Hello { client: "smoke".into(), cwd: cwd.clone(), protocol: PROTOCOL_VERSION }).await;
    loop {
        if let ServerMsg::Welcome { catalog, .. } = c.next().await {
            assert!(catalog.models.iter().any(|m| m.id == model), "model {model} not in catalog");
            break;
        }
    }

    // 1. Multi-step tool turn.
    c.send(ClientMsg::CreateSession { req: 1, cwd: cwd.clone(), agent: None, model: Some(model.clone()), variant: None, name: Some("smoke-main".into()) })
        .await;
    let sid = loop {
        match c.next().await {
            ServerMsg::SessionSnapshot { req: 1, info, .. } => break info.id,
            ServerMsg::Error { req: 1, text } => panic!("create failed: {text}"),
            _ => {}
        }
    };
    println!("\n== 1. multi-step tool turn in {sid}");
    c.send(ClientMsg::Prompt {
        session_id: sid.clone(),
        text: "Use the write tool to create smoke.txt containing the line `hello from ref`, then use the read tool to read it back, then tell me in one short sentence what the file contains.".into(),
        images: vec![],
    })
    .await;
    let msgs = wait_turn(&mut c, &sid, Duration::from_secs(300)).await;
    for m in &msgs {
        println!("  {}", summarize(m));
    }
    let tools: Vec<&ref_proto::ToolPart> = msgs
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| if let Part::Tool(t) = p { Some(t) } else { None })
        .collect();
    let steps = msgs.iter().filter(|m| m.role == ref_proto::Role::Assistant).count();
    println!("  -> {} assistant steps, {} tool calls", steps, tools.len());
    assert!(tools.iter().any(|t| t.name == "write"), "no write call");
    assert!(tools.iter().any(|t| t.name == "read"), "no read call");
    assert!(tools.iter().all(|t| !t.purpose.is_empty()), "missing purpose");
    let file = std::fs::read_to_string(format!("{cwd}/smoke.txt")).unwrap_or_default();
    assert!(file.contains("hello from ref"), "file content: {file:?}");

    // 2. create_session + report back via send_message.
    println!("\n== 2. create_session + send_message round trip");
    let first_len = msgs.len();
    c.send(ClientMsg::Prompt {
        session_id: sid.clone(),
        text: "Use create_session to start a background session named \"助手\" with agent \"build\" and must_report=true, with the prompt: \"Compute 17*23 and report the result to your parent with send_message.\" Then stop and wait; do not do the computation yourself.".into(),
        images: vec![],
    })
    .await;
    let _ = wait_turn(&mut c, &sid, Duration::from_secs(300)).await;
    // Wait for the child's report to arrive (it starts a new turn in the parent).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let msgs = loop {
        let msgs = snapshot(&mut c, &sid).await;
        let got = msgs.iter().any(|m| matches!(m.origin, MessageOrigin::Peer { .. }));
        let idle = msgs.last().is_some_and(|m| m.role == ref_proto::Role::Assistant && m.finish.is_some());
        if got && idle {
            break msgs;
        }
        assert!(tokio::time::Instant::now() < deadline, "no report from the child");
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    for m in &msgs[first_len..] {
        println!("  {}", summarize(m));
    }
    let report = msgs.iter().find(|m| matches!(m.origin, MessageOrigin::Peer { .. })).unwrap();
    println!("  -> report: {:?}", ellipsize(&report.raw_text(), 300));
    assert!(report.raw_text().contains("391"), "report lacks 17*23=391");
    c.send(ClientMsg::ListSessions { req: 5 }).await;
    loop {
        if let ServerMsg::SessionList { req: 5, sessions } = c.next().await {
            for s in sessions.iter().filter(|s| s.open) {
                println!("  session {:?} {} attached={} parent={:?} status={:?}", s.name, s.id, s.attached, s.parent, s.status);
                if s.parent.is_some() {
                    for m in &snapshot(&mut c, &s.id).await {
                        println!("    {}", summarize(m));
                    }
                }
            }
            break;
        }
    }
    println!("\nSMOKE OK");
}

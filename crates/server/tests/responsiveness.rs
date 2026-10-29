//! Request handling never waits for slow MCP servers: CreateSession / Attach / listings
//! answer immediately, and only a session's first LLM step waits (bounded) for MCP.
mod common;

use common::*;
use hn_proto::*;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// TCP listeners that accept connections and never answer.
async fn black_holes(n: usize) -> Vec<u16> {
    let mut ports = Vec::new();
    for _ in 0..n {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        ports.push(l.local_addr().unwrap().port());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((s, _)) = l.accept().await {
                held.push(s);
            }
        });
    }
    ports
}

async fn timed(c: &mut Client, msg: ClientMsg) -> (Duration, serde_json::Value) {
    let t = Instant::now();
    let v = c.req(msg).await.expect("request");
    (t.elapsed(), v)
}

#[tokio::test]
async fn create_session_is_immediate_with_black_hole_mcp_servers() {
    let mock = Mock::start(Arc::new(|_r: &Req| text("answer"))).await;
    let holes = black_holes(3).await;
    let mcp: serde_json::Map<String, serde_json::Value> = holes
        .iter()
        .enumerate()
        .map(|(i, p)| (format!("hole{i}"), json!({"type": "remote", "url": format!("http://127.0.0.1:{p}/mcp"), "timeout": 30000})))
        .collect();
    let env = Env::new(mock.port, json!({"mcp": mcp}));
    let srv = env.spawn(&[]);
    let mut c = Client::connect(&srv.url(), &env.work()).await;
    let create = |name: &str| ClientMsg::CreateSession { cwd: env.work(), agent: None, model: None, name: Some(name.into()) };

    // Like the TUI: listings + create + prompt right after Hello.
    let (t_models, _) = timed(&mut c, ClientMsg::ListModels).await;
    let (t_create, v) = timed(&mut c, create("first")).await;
    let first: SessionInfo = serde_json::from_value(v).unwrap();
    assert!(t_create < Duration::from_millis(500), "CreateSession took {t_create:?} (ListModels {t_models:?})");
    let t0 = Instant::now();
    c.prompt(&first.id, "hi").await;
    assert!(t0.elapsed() < Duration::from_millis(500), "Prompt took {:?}", t0.elapsed());

    // While the first step waits for MCP, other requests are not held up.
    let (t2, v) = timed(&mut c, create("second")).await;
    let second: SessionInfo = serde_json::from_value(v).unwrap();
    assert!(t2 < Duration::from_millis(500), "second CreateSession took {t2:?}");
    let (t3, _) = timed(&mut c, ClientMsg::Attach { session: first.id.clone(), foreground: true }).await;
    assert!(t3 < Duration::from_millis(500), "Attach took {t3:?}");
    let (t4, _) = timed(&mut c, ClientMsg::ListAgents).await;
    assert!(t4 < Duration::from_millis(500), "ListAgents took {t4:?}");
    let mut other = Client::connect(&srv.url(), &env.work()).await;
    let (t5, _) = timed(&mut other, create("third")).await;
    assert!(t5 < Duration::from_millis(500), "CreateSession from another client took {t5:?}");
    let (t6, _) = timed(&mut other, ClientMsg::Attach { session: second.id.clone(), foreground: false }).await;
    assert!(t6 < Duration::from_millis(500), "Attach from another client took {t6:?}");

    // The first step waits for MCP only for a bounded time (well below the 30s timeout),
    // then runs without the unavailable tools.
    c.wait_idle(&first.id, Duration::from_secs(25)).await;
    assert!(t0.elapsed() < Duration::from_secs(20), "first step waited {:?} for MCP", t0.elapsed());
    let (_, msgs) = c.snapshot(&first.id).await;
    assert_eq!(last_assistant_text(&msgs), "answer");
    assert!(srv.log_text().contains("still connecting"), "{}", srv.log_text());
}

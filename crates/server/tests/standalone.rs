//! A backend that stays up (`--idle-timeout -1`) outlives its windows, and the windows that
//! share a session see each other's prompts at once. Over real WebSocket connections.

use futures_util::{SinkExt, StreamExt};
use ref_proto::{ClientFrame, Request, Role, ServerFrame, SessionEvent};
use ref_server::mock::Mock;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message as WsMsg;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const TOKEN: &str = "token";
const KEEP_RUNNING: i64 = -1;
/// The `running` script prints for eight seconds.
const RUNNING_SCRIPT_TIME: Duration = Duration::from_secs(9);
/// A prompt typed in one window must show up in the others within this time.
const MIRROR_LIMIT: Duration = Duration::from_secs(1);
const WAIT: Duration = Duration::from_secs(20);

struct Client {
    ws: Ws,
    next_id: u64,
    /// Frames that arrived while waiting for a response.
    seen: Vec<ServerFrame>,
}

async fn connect(mock: &Mock) -> Client {
    let url = format!("ws://{}/?token={TOKEN}", mock.running.addr);
    let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut client = Client {
        ws,
        next_id: 0,
        seen: Vec::new(),
    };
    let hello = Request::Hello {
        client: "test".into(),
        version: ref_proto::PROTOCOL_VERSION,
        cwd: "/anywhere".into(),
        token: Some(TOKEN.into()),
        notify: Vec::new(),
    };
    client.call(hello).await.unwrap();
    client
}

impl Client {
    async fn send(&mut self, req: Request) -> u64 {
        self.next_id += 1;
        let frame = ClientFrame {
            id: Some(self.next_id),
            req,
        };
        let text = serde_json::to_string(&frame).unwrap();
        self.ws.send(WsMsg::text(text)).await.unwrap();
        self.next_id
    }

    /// Next server frame, if one arrives within `limit`.
    async fn next_frame(&mut self, limit: Duration) -> Option<ServerFrame> {
        let wait = async {
            loop {
                match self.ws.next().await? {
                    Ok(WsMsg::Text(t)) => return serde_json::from_str(&t).ok(),
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
        };
        tokio::time::timeout(limit, wait).await.ok().flatten()
    }

    async fn call(&mut self, req: Request) -> Result<Value, String> {
        let id = self.send(req).await;
        loop {
            match self.next_frame(WAIT).await.expect("a response") {
                ServerFrame::Response {
                    id: rid,
                    ok,
                    result,
                    error,
                } if rid == id => {
                    return if ok {
                        Ok(result)
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
                other => self.seen.push(other),
            }
        }
    }

    /// Waits for a user message with `text` in an event of `session_id`.
    async fn user_message_seen(&mut self, session_id: &str, text: &str, limit: Duration) -> bool {
        let start = Instant::now();
        let is_it = |f: &ServerFrame| {
            matches!(f, ServerFrame::Event { session_id: s, event: SessionEvent::Message { message } }
                if s == session_id && message.role == Role::User && message.raw_text() == text)
        };
        if self.seen.iter().any(is_it) {
            return true;
        }
        while let Some(left) = limit.checked_sub(start.elapsed()) {
            match self.next_frame(left).await {
                Some(f) if is_it(&f) => return true,
                Some(_) => {}
                None => return false,
            }
        }
        false
    }

    async fn open_session(&mut self, name: &str) -> String {
        let create = Request::CreateSession {
            cwd: "/anywhere".into(),
            agent: None,
            model: None,
            name: Some(name.into()),
        };
        let res = self.call(create).await.unwrap();
        let id = res["session"]["id"].as_str().unwrap().to_string();
        self.attach(&id).await;
        id
    }

    async fn attach(&mut self, session_id: &str) -> Value {
        let attach = Request::Attach {
            session_id: session_id.into(),
            foreground: true,
            limit: None,
        };
        self.call(attach).await.unwrap()["snapshot"].clone()
    }

    async fn prompt(&mut self, session_id: &str, text: &str) {
        let prompt = Request::Prompt {
            session_id: session_id.into(),
            text: text.into(),
            images: Vec::new(),
        };
        self.call(prompt).await.unwrap();
    }
}

#[tokio::test]
async fn a_turn_finishes_while_no_window_is_connected() {
    let mock = ref_server::mock::start_with(0, TOKEN.into(), KEEP_RUNNING)
        .await
        .unwrap();
    let mut window = connect(&mock).await;
    let session_id = window.open_session("long").await;
    window.prompt(&session_id, "running").await;
    // The command is running; the only window goes away.
    window.wait_for_running_command(&session_id).await;
    drop(window);

    tokio::time::sleep(RUNNING_SCRIPT_TIME).await;
    assert!(
        !mock.running.hub.shutdown.is_cancelled(),
        "the backend stays up"
    );

    let mut later = connect(&mock).await;
    let snapshot = later.attach(&session_id).await;
    assert_eq!(snapshot["info"]["status"]["state"], "idle");
    let messages = snapshot["messages"].as_array().unwrap();
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "assistant");
    assert!(
        last["parts"].to_string().contains("The command finished."),
        "{last}"
    );
}

#[tokio::test]
async fn a_prompt_typed_in_one_window_shows_up_in_the_other_at_once() {
    let mock = ref_server::mock::start_with(0, TOKEN.into(), KEEP_RUNNING)
        .await
        .unwrap();
    let mut typing = connect(&mock).await;
    let session_id = typing.open_session("shared").await;
    let mut watching = connect(&mock).await;
    watching.attach(&session_id).await;

    let sent = Instant::now();
    typing.prompt(&session_id, "hello").await;
    assert!(
        watching
            .user_message_seen(&session_id, "hello", MIRROR_LIMIT)
            .await,
        "not seen within {MIRROR_LIMIT:?}"
    );
    eprintln!("prompt mirrored after {:?}", sent.elapsed());

    // A message typed while the turn runs reaches the other window when it is delivered.
    let queued = Request::Prompt {
        session_id: session_id.clone(),
        text: "also this".into(),
        images: Vec::new(),
    };
    typing.call(queued).await.unwrap();
    assert!(
        watching
            .user_message_seen(&session_id, "also this", WAIT)
            .await
    );
}

impl Client {
    /// Reads frames until the session's bash tool is running.
    async fn wait_for_running_command(&mut self, session_id: &str) {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            let Some(frame) = self.next_frame(Duration::from_millis(200)).await else {
                continue;
            };
            let text = serde_json::to_string(&frame).unwrap();
            if text.contains(session_id)
                && text.contains("\"state\":\"running\"")
                && text.contains("\"name\":\"bash\"")
            {
                return;
            }
        }
        panic!("the command never started");
    }
}

/// `longrun` prints a line every ten seconds; wait for the next one to be sure it came.
const LONGRUN_TICK: Duration = Duration::from_millis(10_500);

#[tokio::test]
async fn output_printed_while_no_window_is_open_is_there_when_one_opens_the_session() {
    let mock = ref_server::mock::start_with(0, TOKEN.into(), KEEP_RUNNING)
        .await
        .unwrap();
    let mut window = connect(&mock).await;
    let session_id = window.open_session("long").await;
    window
        .prompt(&session_id, "longrun run every test in src/")
        .await;
    window.wait_for_running_command(&session_id).await;
    drop(window);

    tokio::time::sleep(LONGRUN_TICK).await;

    let mut later = connect(&mock).await;
    let snapshot = later.attach(&session_id).await;
    assert_eq!(snapshot["info"]["status"]["state"], "running");
    let output = snapshot["messages"].to_string();
    for step in [
        "step 1/20: tests::parser_01 ... ok",
        "step 2/20: tests::parser_02 ... ok",
    ] {
        assert!(output.contains(step), "{step} missing: {output}");
    }
    let abort = Request::Abort {
        session_id: session_id.clone(),
    };
    later.call(abort).await.unwrap();
    ref_server::ops::shutdown_all(&mock.running.hub).await;
}

//! The mock "LLM": a loopback HTTP server that speaks OpenAI Chat Completions streaming, so
//! the real `ref-llm` client, retry logic and tool loop run against scripted answers.

use super::script::{Ctx, HttpError, Pace, Round, Step, Task, analyze, select};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const CHAT_PATH: &str = "/chat/completions";
const SESSION_HEADER: &str = "x-session-id";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 64 << 20;
/// Rough size of a token in request text, to report plausible usage.
const CHARS_PER_TOKEN: usize = 4;
const MIN_PROMPT_TOKENS: u64 = 200;
/// Share of the prompt the provider reports as served from its cache.
const CACHED_SHARE: f64 = 0.7;
/// Context fill reported by `Round::fill_context`; above the automatic compaction threshold.
const FILLED_CONTEXT: f64 = 0.95;
const SUMMARY_TEXT: &str = "## Goal\n- Explore the mock backend with the user.\n\n## Accomplished\n- Ran several mock scripts and their tools in the sample workspace.\n\n## Next steps\n1. Continue with whatever the user asks next.\n\n## Relevant files\n- `src/parser.rs`: the file the demo edits.";
const SUMMARY_DELAY_MS: u64 = 2500;
const TITLE_WORDS: usize = 6;

/// Context windows of the mock models (see `config.rs`).
pub const CONTEXT_WINDOWS: [(&str, u64); 2] = [("mock-1", 200_000), ("mock-fast", 64_000)];

/// Requests seen so far, by (session, number of messages): a retry sends the same request.
type Attempts = Mutex<HashMap<(String, usize), u32>>;

pub struct LlmServer {
    pub port: u16,
}

pub async fn start() -> std::io::Result<LlmServer> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let attempts: Arc<Attempts> = Arc::default();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let attempts = attempts.clone();
            tokio::spawn(async move {
                if let Err(e) = serve(stream, &attempts).await {
                    tracing::debug!("mock llm connection ended: {e}");
                }
            });
        }
    });
    Ok(LlmServer { port })
}

struct HttpRequest {
    path: String,
    session_id: String,
    body: Value,
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let invalid = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(invalid("headers too large"));
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(invalid("connection closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let path = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    };
    let length: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return Err(invalid("body too large"));
    }
    let body_start = header_end + 4;
    while buf.len() < body_start + length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(invalid("connection closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = serde_json::from_slice(&buf[body_start..body_start + length]).unwrap_or(Value::Null);
    Ok(HttpRequest {
        path,
        session_id: header(SESSION_HEADER).unwrap_or_default(),
        body,
    })
}

async fn serve(mut stream: TcpStream, attempts: &Attempts) -> std::io::Result<()> {
    let req = read_request(&mut stream).await?;
    if !req.path.ends_with(CHAT_PATH) {
        return write_error(&mut stream, 404, "unknown path").await;
    }
    let message_count = req.body["messages"].as_array().map_or(0, Vec::len);
    let attempt = {
        let mut seen = attempts.lock().unwrap_or_else(|e| e.into_inner());
        let n = seen
            .entry((req.session_id.clone(), message_count))
            .or_insert(0);
        *n += 1;
        *n
    };
    let model = req.body["model"].as_str().unwrap_or("").to_string();
    let prompt_tokens = estimate_tokens(&req.body);
    match analyze(&req.body, &req.session_id, attempt) {
        Task::Title(text) => {
            let words: Vec<&str> = text.split_whitespace().take(TITLE_WORDS).collect();
            let round = Round::new().text(words.join(" "));
            stream_round(&mut stream, round, Pace::NORMAL, prompt_tokens).await
        }
        Task::Summary => {
            let round = Round::new().pause(SUMMARY_DELAY_MS).text(SUMMARY_TEXT);
            stream_round(&mut stream, round, Pace::MEDIUM, prompt_tokens).await
        }
        Task::Reply(ctx) => reply(&mut stream, &ctx, &model, prompt_tokens).await,
    }
}

async fn reply(
    stream: &mut TcpStream,
    ctx: &Ctx,
    model: &str,
    prompt_tokens: u64,
) -> std::io::Result<()> {
    let script = select(&ctx.prompt);
    let round = (script.play)(ctx);
    let prompt_tokens = if round.fill_context {
        context_window(model).map_or(prompt_tokens, |w| (w as f64 * FILLED_CONTEXT) as u64)
    } else {
        prompt_tokens
    };
    if let [Step::Error(HttpError { status, message })] = round.steps.as_slice() {
        return write_error(stream, *status, message).await;
    }
    stream_round(stream, round, script.pace, prompt_tokens).await
}

fn context_window(model: &str) -> Option<u64> {
    CONTEXT_WINDOWS
        .iter()
        .find(|(id, _)| *id == model)
        .map(|(_, w)| *w)
}

fn estimate_tokens(body: &Value) -> u64 {
    ((body.to_string().len() / CHARS_PER_TOKEN) as u64).max(MIN_PROMPT_TOKENS)
}

async fn write_error(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let body = json!({"error": {"message": message}}).to_string();
    let head = format!(
        "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

/// One server-sent event with a chat-completions chunk.
fn event(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn delta(delta: Value) -> String {
    event(json!({"choices": [{"index": 0, "delta": delta, "finish_reason": null}]}))
}

/// `text` in pieces of `pace.chars` characters.
fn pieces(text: &str, pace: Pace) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(pace.chars.max(1))
        .map(|c| c.iter().collect())
        .collect()
}

struct Out<'a> {
    stream: &'a mut TcpStream,
}

impl Out<'_> {
    async fn send(&mut self, sse: String) -> std::io::Result<()> {
        self.stream.write_all(sse.as_bytes()).await?;
        self.stream.flush().await
    }

    /// Streams the pieces as deltas built by `make`, pausing between them.
    async fn pieces(
        &mut self,
        text: &str,
        pace: Pace,
        make: impl Fn(&str) -> Value,
    ) -> std::io::Result<()> {
        for piece in pieces(text, pace) {
            self.send(delta(make(&piece))).await?;
            tokio::time::sleep(Duration::from_millis(pace.every_ms)).await;
        }
        Ok(())
    }
}

async fn stream_round(
    stream: &mut TcpStream,
    round: Round,
    pace: Pace,
    prompt_tokens: u64,
) -> std::io::Result<()> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream.write_all(head.as_bytes()).await?;
    let mut out = Out { stream };
    out.send(delta(json!({"role": "assistant"}))).await?;
    let mut generated = 0usize;
    let mut tool_calls = 0usize;
    for step in &round.steps {
        match step {
            Step::Reasoning(text) => {
                generated += text.len();
                let make = |p: &str| json!({"reasoning_content": p});
                out.pieces(text, pace, make).await?;
            }
            Step::Text(text) => {
                generated += text.len();
                out.pieces(text, pace, |p| json!({"content": p})).await?;
            }
            Step::Tool(call) => {
                let index = tool_calls;
                tool_calls += 1;
                let id = format!("call_{}", crate::util::random_hex(6));
                let start = json!({"tool_calls": [{"index": index, "id": id, "type": "function",
                    "function": {"name": call.name, "arguments": ""}}]});
                out.send(delta(start)).await?;
                let arguments = call.raw_arguments();
                generated += arguments.len();
                let make = |p: &str| json!({"tool_calls": [{"index": index, "function": {"arguments": p}}]});
                out.pieces(&arguments, pace, make).await?;
            }
            Step::Pause(ms) => tokio::time::sleep(Duration::from_millis(*ms)).await,
            Step::Error(_) => unreachable!("an error round has no other steps"),
        }
    }
    let reason = if tool_calls > 0 { "tool_calls" } else { "stop" };
    let finish = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]});
    out.send(event(finish)).await?;
    let cached = (prompt_tokens as f64 * CACHED_SHARE) as u64;
    let usage = json!({"choices": [], "usage": {
        "prompt_tokens": prompt_tokens,
        "completion_tokens": (generated / CHARS_PER_TOKEN) as u64 + 1,
        "prompt_tokens_details": {"cached_tokens": cached},
    }});
    out.send(event(usage)).await?;
    out.send("data: [DONE]\n\n".into()).await?;
    out.stream.shutdown().await
}

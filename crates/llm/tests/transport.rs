//! End-to-end transport tests against a minimal local HTTP server.

use ref_llm::{ApiKind, ChatMessage, Model, Provider, Request, StreamEvent, UserContent};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Serve one connection: read the request, write `head`, then each body
/// chunk with `delay` in between, then keep the socket open for `hold`.
async fn serve(
    head: String,
    chunks: Vec<String>,
    delay: Duration,
    hold: Duration,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut req = Vec::new();
        let mut buf = [0u8; 8192];
        // Read headers + body (Content-Length) of the request.
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            req.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&req).to_string();
            if let Some(pos) = text.find("\r\n\r\n") {
                let len = text[..pos]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if req.len() >= pos + 4 + len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let _ = sock.write_all(head.as_bytes()).await;
        for c in chunks {
            let _ = sock
                .write_all(format!("{:x}\r\n{c}\r\n", c.len()).as_bytes())
                .await;
            let _ = sock.flush().await;
            tokio::time::sleep(delay).await;
        }
        tokio::time::sleep(hold).await;
        let _ = sock.write_all(b"0\r\n\r\n").await;
        String::from_utf8_lossy(&req).to_string()
    });
    (format!("http://{addr}/v1/"), task)
}

fn sse_head() -> String {
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n".into()
}

fn provider(api: ApiKind, base_url: String, chunk_timeout: Duration) -> Provider {
    Provider {
        id: "test".into(),
        api,
        base_url,
        api_key: Some("sk-local".into()),
        headers: BTreeMap::new(),
        set_cache_key: false,
        chunk_timeout,
    }
}

fn model() -> Model {
    Model {
        id: "m".into(),
        context: 0,
        output: 0,
        images: false,
        reasoning: false,
        options: Default::default(),
        temperature: None,
        top_p: None,
    }
}

fn request() -> Request {
    Request {
        system: vec!["sys".into()],
        messages: vec![ChatMessage::User(vec![UserContent::Text("hi".into())])],
        tools: vec![],
        session_id: "ses_1".into(),
    }
}

#[tokio::test]
async fn chat_stream_end_to_end() {
    let chunks = vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hel\"}}]}\n\n".to_string(),
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ];
    let (url, server) = serve(sse_head(), chunks, Duration::from_millis(5), Duration::ZERO).await;
    let p = provider(ApiKind::OpenAIChat, url, Duration::from_secs(5));
    let mut events = Vec::new();
    let mut sink = |e: StreamEvent| events.push(e);
    let res = ref_llm::stream(
        &p,
        &model(),
        &request(),
        &CancellationToken::new(),
        &mut sink,
    )
    .await
    .unwrap();
    assert_eq!(res.stop_reason, "stop");
    assert_eq!(res.usage.input, 10);
    assert!(matches!(&res.blocks[0], ref_llm::AssistantBlock::Text(t) if t == "Hello"));
    let req = server.await.unwrap();
    assert!(req.starts_with("POST /v1/chat/completions "));
    let lower = req.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer sk-local"));
    assert!(lower.contains("x-session-affinity: ses_1"));
    assert!(req.contains("\"stream\":true"));
}

#[tokio::test]
async fn http_error_is_classified() {
    let body = r#"{"error":{"message":"Rate limit reached","type":"rate_limit_error"}}"#;
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\nretry-after: 3\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let (url, _server) = serve(head, vec![], Duration::ZERO, Duration::ZERO).await;
    let p = provider(ApiKind::Anthropic, url, Duration::from_secs(5));
    let mut sink = |_e: StreamEvent| {};
    let err = ref_llm::stream(
        &p,
        &model(),
        &request(),
        &CancellationToken::new(),
        &mut sink,
    )
    .await
    .unwrap_err();
    assert_eq!(err.status, Some(429));
    assert!(err.retryable);
    assert_eq!(err.retry_after, Some(Duration::from_secs(3)));
    assert!(err.message.contains("Rate limit reached"));
}

#[tokio::test]
async fn idle_timeout_is_retryable() {
    let chunks = vec!["event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n".to_string()];
    let (url, _server) = serve(sse_head(), chunks, Duration::ZERO, Duration::from_secs(5)).await;
    let p = provider(ApiKind::Anthropic, url, Duration::from_millis(200));
    let mut sink = |_e: StreamEvent| {};
    let err = ref_llm::stream(
        &p,
        &model(),
        &request(),
        &CancellationToken::new(),
        &mut sink,
    )
    .await
    .unwrap_err();
    assert!(err.retryable);
    assert!(err.message.contains("idle timeout"), "{}", err.message);
}

#[tokio::test]
async fn cancellation_aborts_stream() {
    let chunks = vec!["data: {\"type\":\"response.created\",\"response\":{}}\n\n".to_string()];
    let (url, _server) = serve(sse_head(), chunks, Duration::ZERO, Duration::from_secs(5)).await;
    let p = provider(ApiKind::OpenAIResponses, url, Duration::from_secs(30));
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        c2.cancel();
    });
    let mut sink = |_e: StreamEvent| {};
    let started = std::time::Instant::now();
    let err = ref_llm::stream(&p, &model(), &request(), &cancel, &mut sink)
        .await
        .unwrap_err();
    assert!(err.cancelled && !err.retryable);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn connection_refused_is_retryable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let p = provider(
        ApiKind::OpenAIChat,
        format!("http://{addr}/v1"),
        Duration::from_secs(5),
    );
    let mut sink = |_e: StreamEvent| {};
    let err = ref_llm::stream(
        &p,
        &model(),
        &request(),
        &CancellationToken::new(),
        &mut sink,
    )
    .await
    .unwrap_err();
    assert!(err.retryable && !err.cancelled);
}

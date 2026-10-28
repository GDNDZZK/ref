//! Live smoke test: stream "say hi" from a real endpoint.
//!
//! ```sh
//! HN_PROTOCOL=chat|responses|anthropic HN_BASE_URL=https://api.example.com/v1 \
//! HN_API_KEY=... HN_MODEL=model-id [HN_REASONING=1] [HN_OPTIONS='{"reasoningEffort":"low"}'] \
//!   cargo run -p hn-llm --example smoke
//! ```
//! The API key is never printed.

use hn_llm::*;
use std::io::Write;
use tokio_util::sync::CancellationToken;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let protocol = match env("HN_PROTOCOL").as_deref().unwrap_or("chat") {
        "chat" => Protocol::Chat,
        "responses" => Protocol::Responses,
        "anthropic" => Protocol::Anthropic,
        other => anyhow::bail!("unknown HN_PROTOCOL {other:?} (chat|responses|anthropic)"),
    };
    let base_url = env("HN_BASE_URL").ok_or_else(|| anyhow::anyhow!("HN_BASE_URL is required"))?;
    let api_model = env("HN_MODEL").ok_or_else(|| anyhow::anyhow!("HN_MODEL is required"))?;
    let options = match env("HN_OPTIONS") {
        Some(s) => serde_json::from_str(&s)?,
        None => Default::default(),
    };
    let spec = ModelSpec {
        provider_id: "smoke".into(),
        api_model,
        protocol: Some(protocol),
        base_url: base_url.trim_end_matches('/').to_string(),
        api_key: env("HN_API_KEY"),
        options,
        output: 4096,
        reasoning: env("HN_REASONING").is_some(),
        ..Default::default()
    };
    let req = ChatRequest {
        system: vec!["You are a terse assistant.".into()],
        messages: vec![LlmMessage::User(vec![Content::Text("say hi".into())])],
        session_id: "smoke-test".into(),
        ..Default::default()
    };
    eprintln!("protocol={} url={} model={}", protocol.as_str(), spec.base_url, spec.api_model);

    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        c.cancel();
    });

    let client = http_client();
    let mut in_reasoning = false;
    let mut on_event = |ev: StreamEvent| {
        let mut out = std::io::stdout().lock();
        match ev {
            StreamEvent::ReasoningDelta(d) => {
                if !in_reasoning {
                    let _ = write!(out, "\n[reasoning] ");
                    in_reasoning = true;
                }
                let _ = write!(out, "{d}");
            }
            StreamEvent::ReasoningEnd { replay } => {
                in_reasoning = false;
                let _ = writeln!(out, "\n[reasoning end, replay {} bytes]", replay.to_string().len());
            }
            StreamEvent::TextDelta(d) => {
                in_reasoning = false;
                let _ = write!(out, "{d}");
            }
            StreamEvent::Usage(u) => {
                let _ = writeln!(out, "\n[usage] {u:?}");
            }
            other => {
                let _ = writeln!(out, "\n[{other:?}]");
            }
        }
        let _ = out.flush();
    };
    match stream(&client, &spec, &req, &cancel, &mut on_event).await {
        Ok(reason) => println!("\n[finish] {}", reason.as_str()),
        Err(e) => {
            println!("\n[error] kind={:?} status={:?} retry_after={:?} partial={} {}", e.kind, e.status, e.retry_after, e.partial, e.message);
            std::process::exit(1);
        }
    }
    Ok(())
}

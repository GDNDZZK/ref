//! Live protocol check against a real endpoint, running scripted scenarios.
//!
//! ```sh
//! HN_PROTOCOL=chat|responses|anthropic HN_BASE_URL=https://api.example.com/v1 \
//! HN_API_KEY=... HN_MODEL=model-id HN_SCENARIOS=text,reasoning,tool,cache,image \
//! [HN_OPTIONS='{"reasoningEffort":"low"}'] [HN_REASONING=1] [HN_MAX_TOKENS=2000] \
//! [HN_INTERLEAVED=reasoning_content] [HN_IMAGE_B64=... HN_IMAGE_MIME=image/png] \
//!   cargo run -p hn-llm --example live
//! ```
//! Set `RUST_LOG=hn_llm::wire=trace` to dump request bodies and raw SSE events (no headers,
//! so the API key never appears). The key itself is never printed.

use hn_llm::*;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn short(s: &str, max: usize) -> String {
    let flat = s.replace('\n', "⏎");
    let n = flat.chars().count();
    if n <= max {
        flat
    } else {
        format!("{}…(+{} chars)", flat.chars().take(max).collect::<String>(), n - max)
    }
}

/// Compact description of an opaque replay value (lengths instead of long strings).
fn describe_replay(v: &Value) -> String {
    match v {
        Value::Object(o) => {
            let parts: Vec<String> = o
                .iter()
                .map(|(k, v)| match v {
                    Value::String(s) if s.len() > 40 => format!("{k}=<{} bytes>", s.len()),
                    Value::Array(a) => format!("{k}=[{} items]", a.len()),
                    other => format!("{k}={}", short(&other.to_string(), 40)),
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        other => short(&other.to_string(), 80),
    }
}

/// Result of one streamed turn.
struct Turn {
    parts: Vec<AssistantPart>,
    finish: Result<FinishReason, LlmError>,
    usage: Option<Usage>,
}

async fn turn(client: &reqwest::Client, spec: &ModelSpec, req: &ChatRequest, label: &str) -> Turn {
    println!("── {label} ──");
    let cancel = CancellationToken::new();
    let mut parts: Vec<AssistantPart> = Vec::new();
    let mut usage = None;
    let mut counts = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut on_event = |ev: StreamEvent| {
        let name = match &ev {
            StreamEvent::TextDelta(_) => "text",
            StreamEvent::ReasoningDelta(_) => "reasoning",
            StreamEvent::ReasoningEnd { .. } => "reasoning_end",
            StreamEvent::ToolCallStart { .. } => "tool_start",
            StreamEvent::ToolCallDelta { .. } => "tool_delta",
            StreamEvent::ToolCallEnd { .. } => "tool_end",
            StreamEvent::Usage(_) => "usage",
        };
        *counts.entry(name).or_default() += 1;
        match ev {
            StreamEvent::TextDelta(d) => match parts.last_mut() {
                Some(AssistantPart::Text(t)) => t.push_str(&d),
                _ => parts.push(AssistantPart::Text(d)),
            },
            StreamEvent::ReasoningDelta(d) => match parts.last_mut() {
                Some(AssistantPart::Reasoning { text, replay }) if replay.is_null() => text.push_str(&d),
                _ => parts.push(AssistantPart::Reasoning { text: d, replay: Value::Null }),
            },
            StreamEvent::ReasoningEnd { replay: r } => match parts.last_mut() {
                Some(AssistantPart::Reasoning { replay, .. }) if replay.is_null() => *replay = r,
                _ => parts.push(AssistantPart::Reasoning { text: String::new(), replay: r }),
            },
            StreamEvent::ToolCallStart { id, name } => {
                parts.push(AssistantPart::ToolCall { id, name, arguments: String::new() })
            }
            StreamEvent::ToolCallDelta { .. } => {}
            StreamEvent::ToolCallEnd { id, arguments } => {
                for p in parts.iter_mut() {
                    if let AssistantPart::ToolCall { id: pid, arguments: a, .. } = p {
                        if *pid == id {
                            *a = arguments.clone();
                        }
                    }
                }
            }
            StreamEvent::Usage(u) => usage = Some(u),
        }
    };
    let started = std::time::Instant::now();
    let finish = stream(client, spec, req, &cancel, &mut on_event).await;
    println!("  events: {counts:?} ({:.1}s)", started.elapsed().as_secs_f32());
    for p in &parts {
        match p {
            AssistantPart::Text(t) => println!("  text: {}", short(t, 200)),
            AssistantPart::Reasoning { text, replay } => {
                println!("  reasoning ({} chars): {}", text.chars().count(), short(text, 120));
                println!("    replay: {}", describe_replay(replay));
            }
            AssistantPart::ToolCall { id, name, arguments } => println!("  tool_call id={id} name={name} args={}", short(arguments, 200)),
        }
    }
    if let Some(u) = &usage {
        println!(
            "  usage: input={} cache_read={} cache_write={} output={} reasoning={}",
            u.input, u.cache_read, u.cache_write, u.output, u.reasoning
        );
    } else {
        println!("  usage: (none)");
    }
    match &finish {
        Ok(r) => println!("  finish: {}", r.as_str()),
        Err(e) => println!("  ERROR kind={:?} status={:?} partial={} {}", e.kind, e.status, e.partial, short(&e.message, 600)),
    }
    Turn { parts, finish, usage }
}

fn user(text: &str) -> LlmMessage {
    LlmMessage::User(vec![Content::Text(text.into())])
}

fn weather_tool() -> ToolDef {
    ToolDef {
        name: "get_weather".into(),
        description: "Get the current weather for a city.".into(),
        parameters: json!({
            "type": "object",
            "properties": {"city": {"type": "string", "description": "City name"}},
            "required": ["city"],
        }),
    }
}

/// Deterministic ~6k-token system prompt for cache tests.
fn long_system() -> String {
    let mut s = String::from("You are a terse assistant. The following is reference material; ignore it unless asked.\n");
    for i in 0..400 {
        s.push_str(&format!(
            "Rule {i}: when handling item number {i}, prefer clarity over cleverness, keep functions small, and document invariants.\n"
        ));
    }
    s
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let protocol = match env("HN_PROTOCOL").as_deref().unwrap_or("chat") {
        "chat" => Protocol::Chat,
        "responses" => Protocol::Responses,
        "anthropic" => Protocol::Anthropic,
        other => anyhow::bail!("unknown HN_PROTOCOL {other:?}"),
    };
    let base_url = env("HN_BASE_URL").ok_or_else(|| anyhow::anyhow!("HN_BASE_URL is required"))?;
    let api_model = env("HN_MODEL").ok_or_else(|| anyhow::anyhow!("HN_MODEL is required"))?;
    let options = match env("HN_OPTIONS") {
        Some(s) => serde_json::from_str(&s)?,
        None => Default::default(),
    };
    let spec = ModelSpec {
        provider_id: env("HN_PROVIDER").unwrap_or_else(|| "live".into()),
        api_model,
        protocol: Some(protocol),
        base_url: base_url.trim_end_matches('/').to_string(),
        api_key: env("HN_API_KEY"),
        options,
        output: env("HN_OUTPUT").and_then(|s| s.parse().ok()).unwrap_or(4096),
        reasoning: env("HN_REASONING").is_some(),
        interleaved_field: env("HN_INTERLEAVED"),
        ..Default::default()
    };
    let max_tokens = env("HN_MAX_TOKENS").and_then(|s| s.parse().ok());
    let scenarios = env("HN_SCENARIOS").unwrap_or_else(|| "text".into());
    println!(
        "protocol={} url={} model={} options={}",
        protocol.as_str(),
        spec.base_url,
        spec.api_model,
        Value::Object(spec.options.clone())
    );
    let client = http_client();
    let base = ChatRequest {
        system: vec!["You are a terse assistant.".into()],
        session_id: "hn-live-test".into(),
        max_tokens,
        ..Default::default()
    };
    let mut failures = 0;

    for sc in scenarios.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match sc {
            "text" | "reasoning" => {
                let prompt = if sc == "text" { "Say hi in three words." } else { "What is 17 * 23? Think it through, then answer with just the number." };
                let req = ChatRequest { messages: vec![user(prompt)], ..base.clone() };
                let t = turn(&client, &spec, &req, sc).await;
                failures += t.finish.is_err() as u32;
            }
            "tool" => {
                let prompt = env("HN_TOOL_PROMPT").unwrap_or_else(|| {
                    "What's the weather in Paris right now? Use the get_weather tool, then answer in one sentence.".into()
                });
                let mut messages = vec![user(&prompt)];
                let req = ChatRequest { messages: messages.clone(), tools: vec![weather_tool()], ..base.clone() };
                let t1 = turn(&client, &spec, &req, "tool turn 1").await;
                let calls: Vec<(String, String, String)> = t1
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        AssistantPart::ToolCall { id, name, arguments } => Some((id.clone(), name.clone(), arguments.clone())),
                        _ => None,
                    })
                    .collect();
                if t1.finish.is_err() || calls.is_empty() {
                    println!("  !! no tool call in turn 1");
                    failures += 1;
                    continue;
                }
                // HN_REPLAY=0 simulates a model switch: reasoning replay data must then be dropped.
                let replay = env("HN_REPLAY").as_deref() != Some("0");
                messages.push(LlmMessage::Assistant { parts: t1.parts, replay });
                for (id, name, args) in calls {
                    let city = serde_json::from_str::<Value>(&args)
                        .ok()
                        .and_then(|v| v.get("city").and_then(Value::as_str).map(str::to_string))
                        .unwrap_or_else(|| "unknown".into());
                    messages.push(LlmMessage::ToolResult {
                        call_id: id,
                        name,
                        content: json!({"city": city, "temp_c": 18, "conditions": "sunny"}).to_string(),
                        images: vec![],
                        is_error: false,
                    });
                }
                let req = ChatRequest { messages, tools: vec![weather_tool()], ..base.clone() };
                let t2 = turn(&client, &spec, &req, "tool turn 2 (with replay)").await;
                failures += t2.finish.is_err() as u32;
            }
            "cache" => {
                let req = ChatRequest {
                    system: vec![long_system()],
                    // Multi-turn: some gateways only cache once a conversation has history.
                    messages: vec![
                        user("Hi"),
                        LlmMessage::Assistant { parts: vec![AssistantPart::Text("Hello.".into())], replay: true },
                        user("Reply with the single word: ok"),
                    ],
                    ..base.clone()
                };
                let a = turn(&client, &spec, &req, "cache call 1").await;
                let b = turn(&client, &spec, &req, "cache call 2").await;
                failures += (a.finish.is_err() || b.finish.is_err()) as u32;
                if let (Some(a), Some(b)) = (a.usage, b.usage) {
                    println!(
                        "  cache summary: call1 total_in={} (write {}), call2 cache_read={} of total_in={}",
                        a.input + a.cache_read + a.cache_write,
                        a.cache_write,
                        b.cache_read,
                        b.input + b.cache_read + b.cache_write
                    );
                }
            }
            "image" => {
                let (Some(data), mime) = (env("HN_IMAGE_B64"), env("HN_IMAGE_MIME").unwrap_or_else(|| "image/png".into())) else {
                    println!("── image ── skipped (HN_IMAGE_B64 unset)");
                    continue;
                };
                let req = ChatRequest {
                    messages: vec![LlmMessage::User(vec![
                        Content::Text("What is the dominant color of this image? One word.".into()),
                        Content::Image(ImageData { mime, data }),
                    ])],
                    ..base.clone()
                };
                let t = turn(&client, &spec, &req, "image").await;
                failures += t.finish.is_err() as u32;
            }
            // History with a finished tool call, but no tools offered now (compaction/summary
            // requests), optionally with an image inside the tool result.
            "notools" | "toolimage" => {
                let image = env("HN_IMAGE_B64").filter(|_| sc == "toolimage").map(|data| ImageData { mime: env("HN_IMAGE_MIME").unwrap_or_else(|| "image/png".into()), data });
                if sc == "toolimage" && image.is_none() {
                    println!("── {sc} ── skipped (HN_IMAGE_B64 unset)");
                    continue;
                }
                let call_id = if protocol == Protocol::Anthropic { "toolu_01A2b3C4d5E6f7G8h9I0j1K2" } else { "call_abc123" };
                let (tool_name, args, content, question) = if sc == "toolimage" {
                    ("screenshot", "{}", "Screenshot attached.", "What is the dominant color of the screenshot? One word.")
                } else {
                    ("get_weather", "{\"city\":\"Paris\"}", "{\"city\":\"Paris\",\"temp_c\":18,\"conditions\":\"sunny\"}", "Summarize this conversation in one sentence.")
                };
                let messages = vec![
                    user(if sc == "toolimage" { "Take a screenshot." } else { "What's the weather in Paris?" }),
                    LlmMessage::Assistant {
                        parts: vec![AssistantPart::ToolCall { id: call_id.into(), name: tool_name.into(), arguments: args.into() }],
                        replay: false,
                    },
                    LlmMessage::ToolResult {
                        call_id: call_id.into(),
                        name: tool_name.into(),
                        content: content.into(),
                        images: image.into_iter().collect(),
                        is_error: false,
                    },
                    LlmMessage::Assistant { parts: vec![AssistantPart::Text("Done.".into())], replay: false },
                    user(question),
                ];
                let tools = if sc == "toolimage" {
                    vec![ToolDef { name: "screenshot".into(), description: "Take a screenshot.".into(), parameters: json!({"type":"object","properties":{}}) }]
                } else {
                    vec![]
                };
                let req = ChatRequest { messages, tools, ..base.clone() };
                let t = turn(&client, &spec, &req, sc).await;
                failures += t.finish.is_err() as u32;
            }
            other => println!("unknown scenario {other:?}"),
        }
    }
    println!("== failures: {failures}");
    std::process::exit(if failures == 0 { 0 } else { 1 });
}

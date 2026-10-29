//! Dev probe against real endpoints from the user's opencode config.
//!
//! cargo run -p ref-llm --example probe -- <provider/model[#variant]> [options]
//!   --variant V      variant (alternative to `#V`)
//!   --image PATH     attach an image to the first user message and ask about it
//!   --tool-image PATH  return this image inside the get_time tool result
//!   --pad N          words of stable filler in the system prompt (default 3000, for caching)
//!   --steps N        extra follow-up user turns after the tool round trip (default 1)
//!   --long N         make the first user message ~N words long (context overflow test)
//!   --api KIND       override api kind: anthropic | chat | responses
//!   --base URL       override base URL
//!   --body           print full request bodies (redacted)
//!   --no-tools       ask a plain question without tools
//!   --question TEXT  replace the first user question
//!
//! Secrets are never printed: request headers are redacted and long strings truncated.

use std::collections::BTreeMap;
use std::time::Instant;

use base64::Engine;
use futures::StreamExt;
use ref_llm::{ApiKind, FinishReason, ModelSpec, Request, StreamEvent, ToolDef};
use ref_proto::{ImagePart, Message, MessageOrigin, Part, ReasoningPart, Role, TextPart, ToolPart, ToolState};
use serde_json::{json, Value};

struct Args {
    model: String,
    variant: Option<String>,
    image: Option<String>,
    tool_image: Option<String>,
    pad: usize,
    steps: usize,
    long: usize,
    api: Option<String>,
    base: Option<String>,
    body: bool,
    tools: bool,
    question: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        model: String::new(),
        variant: None,
        image: None,
        tool_image: None,
        pad: 3000,
        steps: 1,
        long: 0,
        api: None,
        base: None,
        body: false,
        tools: true,
        question: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().expect("missing option value");
        match arg.as_str() {
            "--variant" => a.variant = Some(val()),
            "--image" => a.image = Some(val()),
            "--tool-image" => a.tool_image = Some(val()),
            "--pad" => a.pad = val().parse().expect("number"),
            "--steps" => a.steps = val().parse().expect("number"),
            "--long" => a.long = val().parse().expect("number"),
            "--api" => a.api = Some(val()),
            "--base" => a.base = Some(val()),
            "--body" => a.body = true,
            "--no-tools" => a.tools = false,
            "--question" => a.question = Some(val()),
            s if s.starts_with("--") => panic!("unknown option {s}"),
            s => a.model = s.to_string(),
        }
    }
    assert!(!a.model.is_empty(), "usage: probe <provider/model[#variant]> [options]");
    a
}

/// Truncate long strings (base64 images, signatures, encrypted content) for display.
fn redact(v: &Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > 160 => {
            let head: String = s.chars().take(80).collect();
            Value::String(format!("{head}…[{} chars]", s.chars().count()))
        }
        Value::Array(a) => Value::Array(a.iter().map(redact).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| {
                    let secret = ["apikey", "api_key", "authorization", "x-api-key", "key"].contains(&k.to_ascii_lowercase().as_str());
                    (k.clone(), if secret { json!("<redacted>") } else { redact(v) })
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn short(s: &str, n: usize) -> String {
    let s = s.replace('\n', "⏎");
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s
    }
}

fn text_part(text: &str) -> Part {
    Part::Text(TextPart { id: ref_proto::new_id("prt"), text: text.into(), time: ref_proto::now_ms(), meta: None })
}

fn image_part(path: &str) -> Part {
    let bytes = std::fs::read(path).expect("read image");
    let mime = if path.ends_with(".jpg") || path.ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
    Part::Image(ImagePart {
        id: ref_proto::new_id("prt"),
        mime: mime.into(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        name: None,
    })
}

fn user(parts: Vec<Part>) -> Message {
    let mut m = Message::new(Role::User, MessageOrigin::User);
    m.parts = parts;
    m
}

fn filler(words: usize) -> String {
    // Deterministic, byte-stable filler so the prefix is cacheable across requests.
    let vocab = [
        "repository", "function", "module", "compile", "iterator", "borrow", "lifetime", "trait", "closure",
        "pattern", "vector", "string", "buffer", "socket", "thread", "channel", "future", "runtime", "crate",
    ];
    let mut out = String::from("Reference glossary (ignore unless relevant):\n");
    for i in 0..words {
        out.push_str(vocab[(i * 7 + i / 3) % vocab.len()]);
        out.push(if i % 16 == 15 { '\n' } else { ' ' });
    }
    out
}

struct StepResult {
    msg: Message,
    finish: Option<FinishReason>,
}

async fn run_step(http: &reqwest::Client, spec: &ModelSpec, req: &Request, label: &str, show_body: bool) -> anyhow::Result<StepResult> {
    let body = ref_llm::request_body(spec, req);
    println!("\n━━━ {label}: POST {} ({:?})", spec.base_url, spec.api);
    for (k, v) in ref_llm::request_headers(spec, req)? {
        println!("  header {k}: {v}");
    }
    let mut shown = redact(&body);
    if !show_body {
        // Collapse the message list to roles/types for a compact view.
        for key in ["messages", "input"] {
            if let Some(Value::Array(items)) = shown.get(key).cloned() {
                let compact: Vec<Value> = items
                    .iter()
                    .map(|m| {
                        let mut c = m.clone();
                        if let Some(Value::String(s)) = c.get("content") {
                            c["content"] = json!(short(s, 60));
                        }
                        c
                    })
                    .collect();
                shown[key] = Value::Array(compact);
            }
        }
        if let Some(Value::Array(sys)) = shown.get("system").cloned() {
            shown["system"] = Value::Array(
                sys.iter()
                    .map(|b| {
                        let mut b = b.clone();
                        if let Some(Value::String(t)) = b.get("text") {
                            b["text"] = json!(short(t, 60));
                        }
                        b
                    })
                    .collect(),
            );
        }
    }
    println!("  body: {}", serde_json::to_string_pretty(&shown)?);

    let t0 = Instant::now();
    let mut events = match ref_llm::stream(http, spec, req).await {
        Ok(e) => e,
        Err(e) => {
            println!("  ✗ request error: {e}");
            println!("    retryable={} overflow={} retry_after={:?}", e.is_retryable(), e.is_context_overflow(), e.retry_after());
            anyhow::bail!("request failed");
        }
    };
    let mut msg = Message::new(Role::Assistant, MessageOrigin::User);
    msg.model = Some(spec.full_id());
    let mut slots: BTreeMap<usize, usize> = BTreeMap::new(); // stream index -> part position
    let mut finish = None;
    let mut first = None;
    let mut deltas: BTreeMap<String, usize> = BTreeMap::new();
    while let Some(ev) = events.next().await {
        let ev = match ev {
            Ok(ev) => ev,
            Err(e) => {
                println!("  ✗ stream error: {e} (retryable={} overflow={})", e.is_retryable(), e.is_context_overflow());
                anyhow::bail!("stream failed");
            }
        };
        first.get_or_insert_with(|| t0.elapsed());
        match ev {
            StreamEvent::TextDelta { index, text } => {
                *deltas.entry("text".into()).or_default() += 1;
                let pos = *slots.entry(index).or_insert_with(|| {
                    msg.parts.push(text_part(""));
                    msg.parts.len() - 1
                });
                if let Part::Text(t) = &mut msg.parts[pos] {
                    t.text.push_str(&text);
                }
            }
            StreamEvent::ReasoningDelta { index, text } => {
                *deltas.entry("reasoning".into()).or_default() += 1;
                let pos = *slots.entry(index).or_insert_with(|| {
                    msg.parts.push(Part::Reasoning(ReasoningPart {
                        id: ref_proto::new_id("prt"),
                        text: String::new(),
                        time: 0,
                        time_end: None,
                        meta: None,
                    }));
                    msg.parts.len() - 1
                });
                if let Part::Reasoning(r) = &mut msg.parts[pos] {
                    r.text.push_str(&text);
                }
            }
            StreamEvent::TextEnd { index, meta } | StreamEvent::ReasoningEnd { index, meta } => {
                println!("  · end[{index}] meta={}", meta.as_ref().map(|m| redact(m).to_string()).unwrap_or("-".into()));
                match slots.get(&index) {
                    Some(&pos) => match &mut msg.parts[pos] {
                        Part::Text(t) => t.meta = meta,
                        Part::Reasoning(r) => r.meta = meta,
                        _ => {}
                    },
                    None if meta.is_some() => {
                        msg.parts.push(Part::Reasoning(ReasoningPart {
                            id: ref_proto::new_id("prt"),
                            text: String::new(),
                            time: 0,
                            time_end: None,
                            meta,
                        }));
                        slots.insert(index, msg.parts.len() - 1);
                    }
                    None => {}
                }
            }
            StreamEvent::ToolCallStart { index, call_id, name } => {
                println!("  · tool start[{index}] {name} id={call_id}");
            }
            StreamEvent::ToolCallDelta { .. } => *deltas.entry("tool".into()).or_default() += 1,
            StreamEvent::ToolCallEnd { index, call_id, name, input, meta } => {
                println!("  · tool end[{index}] {name} id={call_id} input={input} meta={}", meta.as_ref().map(|m| redact(m).to_string()).unwrap_or("-".into()));
                msg.parts.push(Part::Tool(ToolPart {
                    id: ref_proto::new_id("prt"),
                    call_id,
                    name,
                    purpose: String::new(),
                    input,
                    input_raw: String::new(),
                    state: ToolState::Running,
                    output: String::new(),
                    images: vec![],
                    time: 0,
                    time_start: None,
                    time_end: None,
                    run_at: None,
                    meta,
                }));
                slots.insert(index, msg.parts.len() - 1);
            }
            StreamEvent::Usage(u) => {
                println!(
                    "  · usage input={} output={} cache_read={} cache_write={} reasoning={} cost=${:.5}",
                    u.input, u.output, u.cache_read, u.cache_write, u.reasoning, u.cost
                );
                msg.usage = Some(u);
            }
            StreamEvent::Finish { reason } => finish = Some(reason),
        }
    }
    println!("  delta counts: {deltas:?}; first event after {:?}, total {:?}", first.unwrap_or_default(), t0.elapsed());
    for p in &msg.parts {
        match p {
            Part::Reasoning(r) => println!("  ▸ reasoning ({} chars): {}", r.text.len(), short(&r.text, 200)),
            Part::Text(t) => println!("  ▸ text: {}", short(&t.text, 400)),
            Part::Tool(t) => println!("  ▸ tool {}({})", t.name, t.input),
            Part::Image(_) => {}
        }
    }
    println!("  finish: {finish:?}");
    msg.finish = Some(match finish {
        Some(FinishReason::ToolUse) => "tool_use",
        Some(FinishReason::Length) => "length",
        Some(FinishReason::Error) => "error",
        _ => "stop",
    }
    .into());
    Ok(StepResult { msg, finish })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args();
    let cfg = ref_config::Config::load(std::path::Path::new("/tmp"))?;
    let mut spec = cfg.resolve_model(&args.model, args.variant.as_deref())?;
    if let Some(api) = &args.api {
        spec.api = match api.as_str() {
            "anthropic" => ApiKind::Anthropic,
            "chat" => ApiKind::OpenAIChat,
            "responses" => ApiKind::OpenAIResponses,
            other => anyhow::bail!("unknown api {other}"),
        };
    }
    if let Some(b) = &args.base {
        spec.base_url = b.trim_end_matches('/').to_string();
    }
    println!(
        "model {} api_model={} api={:?} base={} context={} max_output={} image={} reasoning={} has_key={}",
        spec.full_id(),
        spec.api_model,
        spec.api,
        spec.base_url,
        spec.context,
        spec.max_output,
        spec.image_input,
        spec.reasoning,
        spec.api_key.is_some()
    );
    println!("options: {}", redact(&Value::Object(spec.options.clone())));
    if !spec.extra_body.is_empty() {
        println!("extra_body keys: {:?}", spec.extra_body.keys().collect::<Vec<_>>());
    }

    let http = reqwest::Client::builder().build()?;
    let tools = if args.tools {
        vec![ToolDef {
            name: "get_time".into(),
            description: "Return the current date and time in UTC (ISO 8601).".into(),
            parameters: json!({
                "type": "object",
                "properties": {"purpose": {"type": "string", "description": "Short reason for this call, shown to the user"}},
                "required": ["purpose"],
            }),
        }]
    } else {
        vec![]
    };
    let system = vec![
        "You are ref, a coding agent running in the user's terminal. Be brief.".to_string(),
        filler(args.pad),
    ];
    let mut first_parts = Vec::new();
    let mut question = if args.tools {
        "What time is it right now? You must call the get_time tool, then tell me the time in one short sentence.".to_string()
    } else {
        "Say hello in five words.".to_string()
    };
    if let Some(q) = &args.question {
        question = q.clone();
    }
    if args.image.is_some() {
        question.push_str(" Also: what is written in the attached image, and what color is the background?");
    }
    if args.long > 0 {
        first_parts.push(text_part(&filler(args.long)));
    }
    first_parts.push(text_part(&question));
    if let Some(p) = &args.image {
        first_parts.push(image_part(p));
    }
    let mut messages = vec![user(first_parts)];
    let session_id = format!("ses_probe_{}", spec.full_id().replace('/', "_"));

    let mk = |messages: &Vec<Message>| Request {
        system: system.clone(),
        messages: messages.clone(),
        tools: tools.clone(),
        session_id: session_id.clone(),
        tool_choice_none: false,
    };

    let r1 = run_step(&http, &spec, &mk(&messages), "step 1", args.body).await?;
    let mut msg = r1.msg;
    let mut called = false;
    for p in &mut msg.parts {
        if let Part::Tool(t) = p {
            called = true;
            t.state = ToolState::Done;
            t.output = "2026-10-29T12:34:56Z".into();
            if let Some(img) = &args.tool_image {
                if let Part::Image(i) = image_part(img) {
                    t.output.push_str("\n(screenshot of the clock attached; describe what text it shows)");
                    t.images.push(i);
                }
            }
        }
    }
    messages.push(msg);
    if args.tools && !called {
        println!("  ! model did not call the tool (finish {:?})", r1.finish);
    }
    if called {
        let r2 = run_step(&http, &spec, &mk(&messages), "step 2 (tool result)", args.body).await?;
        messages.push(r2.msg);
    }
    for i in 0..args.steps {
        messages.push(user(vec![text_part("Thanks. Now reply with just the word OK.")]));
        let r = run_step(&http, &spec, &mk(&messages), &format!("follow-up {}", i + 1), args.body).await?;
        messages.push(r.msg);
    }
    Ok(())
}

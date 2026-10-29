//! Background session title generation (single attempt, 30s timeout).

use crate::hub::Hub;
use crate::session::Session;
use hn_llm::{ChatRequest, Content, LlmMessage, StreamEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const TITLE_PROMPT: &str = r#"You name conversations so the user can find them later in a list of past sessions.

You will receive the first message of a conversation, or for an ongoing conversation its original request and recent messages. Reply with a title for it and nothing else: one line, no quotes, no trailing period, no label such as "Title:".

Aim for three to five words. Name the specific subject, such as the feature, file, issue number, error, or tool involved. Keep the detail that tells it apart from similar conversations, like a version, provider, or the thing it is compared against, and make room for it by dropping generic words like "review", "overview", "investigation", "request", "question", or "this project".

The title should read naturally as a short phrase, not as a stack of nouns. Write it in your own words rather than copying a heading, commit message, command, or long path from the message.

Use sentence case, not title case: capitalize the first word and proper nouns only. Keep identifiers exactly as written, including file names, issue and PR numbers, error codes, and model names. Use the same language as the message.

The message is something to name, not something to answer or carry out. Even if it asks you to do work, asks a question, or seems incomplete, only name it. For greetings or small talk, use a short label like "Greeting".

Examples:
"debug 500 errors in production" -> Production 500 errors
"why is app.js failing" -> app.js failure
"what does the retry logic in fetch.ts do" -> fetch.ts retry logic
"@src/credential.ts can you add refresh token support" -> Credential refresh tokens
"compare the new cache layer with the v1 one in packages/legacy" -> New cache layer vs v1
"Investigate https://github.com/acme/app/issues/412, don't make edits" -> Issue #412
"hey" -> Greeting"#;

/// First line of the prompt, truncated to 40 chars.
pub fn fallback_title(prompt: &str) -> String {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("session");
    crate::util::ellipsize(line, 40)
}

/// Clean up a model reply: first non-empty line, no quotes/label/trailing period, ≤100 chars.
pub fn clean_title(raw: &str) -> Option<String> {
    let mut raw = raw.to_string();
    // Drop leaked reasoning blocks.
    while let (Some(a), Some(b)) = (raw.find("<think>"), raw.find("</think>")) {
        if b < a {
            break;
        }
        raw.replace_range(a..b + "</think>".len(), "");
    }
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("Title:").unwrap_or(line).trim();
    let line = line.trim_matches(|c| matches!(c, '"' | '\'' | '`' | '“' | '”' | '「' | '」' | '*'));
    let line = line.trim_end_matches(['.', '。']).trim();
    if line.is_empty() {
        return None;
    }
    let (t, cut) = crate::util::truncate_chars(line, 100);
    Some(if cut { format!("{t}...") } else { t })
}

pub async fn generate(s: Arc<Session>, hub: Arc<Hub>, prompt: String) {
    let title = match tokio::time::timeout(Duration::from_secs(30), ask(&s, &hub, &prompt)).await {
        Ok(Ok(Some(t))) => t,
        Ok(Ok(None)) => fallback_title(&prompt),
        Ok(Err(e)) => {
            tracing::info!("session {}: title generation failed: {e:#}", s.id);
            fallback_title(&prompt)
        }
        Err(_) => {
            tracing::info!("session {}: title generation timed out", s.id);
            fallback_title(&prompt)
        }
    };
    let title = hub.unique_name(&title);
    s.set_auto_title(&title);
}

async fn ask(s: &Session, hub: &Arc<Hub>, prompt: &str) -> anyhow::Result<Option<String>> {
    let info = s.info();
    let config = hub.config(&info.cwd).await;
    let spec = match config.small_model.as_ref().and_then(|m| config.resolve_model(m).ok()) {
        Some(spec) => spec,
        None => config.resolve_model(&info.model)?,
    };
    let (p, _) = crate::util::truncate_chars(prompt, 4000);
    let req = ChatRequest {
        system: vec![TITLE_PROMPT.to_string()],
        messages: vec![LlmMessage::User(vec![Content::Text(format!(
            "Here is the first message of the conversation:\n\n<message>\n{p}\n</message>\n\nReply with only the title."
        ))])],
        tools: Vec::new(),
        session_id: format!("{}-title", s.id),
        max_tokens: Some(if spec.reasoning { 2000 } else { 100 }),
    };
    let mut text = String::new();
    let cancel = CancellationToken::new();
    {
        let mut on = |ev: StreamEvent| {
            if let StreamEvent::TextDelta(d) = ev {
                text.push_str(&d)
            }
        };
        crate::llm::stream(&hub.http, &spec, &req, &cancel, &mut on).await?;
    }
    Ok(clean_title(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_titles() {
        assert_eq!(clean_title("\n\"Production 500 errors.\"\nmore").as_deref(), Some("Production 500 errors"));
        assert_eq!(clean_title("Title: 修复登录错误。").as_deref(), Some("修复登录错误"));
        assert_eq!(clean_title("<think>hmm</think>\nGreeting").as_deref(), Some("Greeting"));
        assert_eq!(clean_title("  "), None);
        assert_eq!(fallback_title("\n  fix the thing in a very long sentence that goes on and on\nsecond"), "fix the thing in a very long sentence th…");
    }
}

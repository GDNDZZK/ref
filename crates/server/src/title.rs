//! Asynchronous session title generation (one attempt, failures ignored).

use crate::hub::Hub;
use crate::session::Session;
use ref_llm::{AssistantBlock, ChatMessage, UserContent};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const TITLE_PROMPT: &str =
    "Generate a short title (max 8 words, same language as the user) for this conversation. Reply with the title only.";

/// Clean a model reply into a title: first non-empty line, quotes / label stripped, <= 100 chars.
pub fn clean_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("Title:").unwrap_or(line).trim();
    let line = line.trim_matches(|c| matches!(c, '"' | '\'' | '`' | '*' | '#')).trim();
    let line = line.trim_end_matches('.').trim();
    if line.is_empty() {
        return None;
    }
    Some(crate::util::truncate_chars(line, 100))
}

pub async fn generate(hub: Arc<Hub>, s: Arc<Session>, first_text: String) {
    let (cfg, agent, model_ref) = {
        let st = s.lock();
        (st.cfg.clone(), st.info.agent.clone(), st.info.model.clone())
    };
    let small = cfg.small_model.clone().filter(|m| cfg.model(m).is_some());
    let mref = small.unwrap_or(model_ref);
    let Ok((provider, model)) = crate::models::resolve(&cfg, &mref, &agent) else { return };
    let text = crate::util::truncate_chars(&first_text, 2000);
    let req = ref_llm::Request {
        system: vec![TITLE_PROMPT.into()],
        messages: vec![ChatMessage::User(vec![UserContent::Text(format!("{TITLE_PROMPT}\n\n{text}"))])],
        tools: vec![],
        session_id: format!("{}-title", s.id),
    };
    let cancel = CancellationToken::new();
    let shutdown = hub.shutdown.clone();
    let mut sink = |_e: ref_llm::StreamEvent| {};
    let res = tokio::select! {
        r = ref_llm::stream(&provider, &model, &req, &cancel, &mut sink) => r,
        _ = shutdown.cancelled() => return,
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => return,
    };
    let Ok(resp) = res else {
        tracing::debug!(session = %s.id, "title generation failed");
        return;
    };
    let raw: String = resp
        .blocks
        .iter()
        .filter_map(|b| if let AssistantBlock::Text(t) = b { Some(t.as_str()) } else { None })
        .collect();
    let Some(title) = clean_title(&raw) else { return };
    {
        let mut st = s.lock();
        if st.closed {
            return;
        }
        st.info.title = title;
        hub.emit_info(&st);
    }
    crate::ops::persist(&hub, &s);
}

#[cfg(test)]
mod tests {
    #[test]
    fn clean() {
        assert_eq!(super::clean_title("\n\"Fix the build.\"\nmore").as_deref(), Some("Fix the build"));
        assert_eq!(super::clean_title("Title: 修复构建").as_deref(), Some("修复构建"));
        assert_eq!(super::clean_title("  "), None);
    }
}

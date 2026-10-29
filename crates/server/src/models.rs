//! Resolve a `ModelRef` from the config into `ref_llm` Provider / Model values.

use ref_config::Config;
use ref_llm::{ApiKind, Model, Provider};
use ref_proto::{ContextUsage, ModelRef};
use serde_json::{Map, Value};
use std::time::Duration;

pub const DEFAULT_CONTEXT: u64 = 128_000;

/// Recursive object merge: objects merged key by key, everything else replaced.
pub fn deep_merge(base: &mut Map<String, Value>, overlay: &Map<String, Value>) {
    for (k, v) in overlay {
        match (base.get_mut(k), v) {
            (Some(Value::Object(b)), Value::Object(o)) => deep_merge(b, o),
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

fn api_kind(api: Option<&str>, npm: Option<&str>) -> ApiKind {
    match api.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("chat" | "openai-chat" | "openai-compatible" | "completions" | "chat-completions") => ApiKind::OpenAIChat,
        Some("responses" | "openai-responses" | "openai") => ApiKind::OpenAIResponses,
        Some("anthropic" | "messages" | "anthropic-messages") => ApiKind::Anthropic,
        _ => ApiKind::from_npm(npm),
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// Build Provider + Model for a session request.
pub fn resolve(cfg: &Config, r: &ModelRef, agent: &str) -> Result<(Provider, Model), String> {
    if r.provider.is_empty() || r.model.is_empty() {
        return Err("No model configured. Add a provider with models to your config (opencode.json / ref.json).".into());
    }
    let (p, m) = cfg.model(r).ok_or_else(|| format!("Model {} is not configured.", r.key()))?;
    let api = api_kind(p.api.as_deref(), p.npm.as_deref());
    let base_url = p.base_url.clone().unwrap_or_default();
    let mut headers = p.headers.clone();
    headers.extend(m.headers.clone());
    let set_cache_key = p.options.get("setCacheKey").and_then(Value::as_bool).unwrap_or_else(|| {
        api == ApiKind::OpenAIResponses || host_of(&base_url) == "api.openai.com"
    });
    let chunk_timeout = p
        .options
        .get("chunkTimeout")
        .and_then(Value::as_u64)
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(300));
    let provider = Provider {
        id: p.id.clone(),
        api,
        base_url,
        api_key: p.api_key.clone(),
        headers,
        set_cache_key,
        chunk_timeout,
    };
    let ag = cfg.agents.get(agent);
    let mut options = m.options.clone();
    if let Some(a) = ag {
        deep_merge(&mut options, &a.options);
    }
    if let Some(v) = r.variant.as_ref().and_then(|v| m.variants.get(v)) {
        deep_merge(&mut options, v);
    }
    let model = Model {
        id: m.id.clone(),
        context: if m.context > 0 { m.context } else { DEFAULT_CONTEXT },
        output: m.output,
        images: m.images,
        reasoning: m.reasoning,
        options,
        temperature: ag.and_then(|a| a.temperature).or(m.temperature),
        top_p: ag.and_then(|a| a.top_p).or(m.top_p),
    };
    Ok((provider, model))
}

/// Context window thresholds for a model (used = 0).
pub fn context_for(cfg: &Config, r: &ModelRef) -> ContextUsage {
    let (max, compact_at) = match cfg.model(r) {
        Some((_, m)) => (if m.context > 0 { m.context } else { DEFAULT_CONTEXT }, m.compact_at),
        None => (DEFAULT_CONTEXT, None),
    };
    ContextUsage {
        used: 0,
        max,
        recommend: cfg.compaction.recommend_tokens(max, compact_at),
        auto: cfg.compaction.auto_tokens(max),
    }
}

/// Variant names of a model, sorted.
pub fn variants(cfg: &Config, r: &ModelRef) -> Vec<String> {
    cfg.model(r).map(|(_, m)| m.variants.keys().cloned().collect()).unwrap_or_default()
}

/// Next variant in the cycle `None -> v0 -> v1 -> ... -> None`.
pub fn next_variant(list: &[String], cur: Option<&str>) -> Option<String> {
    match cur.and_then(|c| list.iter().position(|v| v == c)) {
        None => list.first().cloned(),
        Some(i) => list.get(i + 1).cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge() {
        let mut a = json!({"a": 1, "o": {"x": 1, "y": 2}}).as_object().cloned().unwrap();
        let b = json!({"b": 2, "o": {"y": 3}}).as_object().cloned().unwrap();
        deep_merge(&mut a, &b);
        assert_eq!(Value::Object(a), json!({"a": 1, "o": {"x": 1, "y": 3}, "b": 2}));
    }

    #[test]
    fn variant_cycle() {
        let l = vec!["high".to_string(), "low".to_string()];
        assert_eq!(next_variant(&l, None).as_deref(), Some("high"));
        assert_eq!(next_variant(&l, Some("high")).as_deref(), Some("low"));
        assert_eq!(next_variant(&l, Some("low")), None);
        assert_eq!(next_variant(&l, Some("gone")).as_deref(), Some("high"));
        assert_eq!(next_variant(&[], None), None);
    }

    #[test]
    fn api_kinds() {
        assert_eq!(api_kind(Some("chat"), Some("@ai-sdk/anthropic")), ApiKind::OpenAIChat);
        assert_eq!(api_kind(None, Some("@ai-sdk/anthropic")), ApiKind::Anthropic);
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
    }
}

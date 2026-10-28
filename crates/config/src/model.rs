//! Model resolution: config + models.dev catalog -> [`ModelSpec`].

use crate::jsonc::merge_maps;
use crate::*;
use hn_llm::Protocol;
use serde_json::Map;
use std::collections::BTreeMap;

pub(crate) const DEFAULT_CONTEXT: u64 = 128_000;
pub(crate) const DEFAULT_OUTPUT: u64 = 32_000;

/// Provider-level option keys that are not passed through as request options.
const PROVIDER_ONLY: &[&str] = &["apiKey", "baseURL", "headers", "body", "extraBody"];
/// Variant keys that are not request options.
const VARIANT_META: &[&str] = &["disabled"];

/// models.dev catalog (`~/.cache/opencode/models.json`), parsed into the config shape.
#[derive(Clone, Default)]
pub struct Catalog(pub IndexMap<String, ProviderConfig>);

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Catalog({} providers)", self.0.len())
    }
}

impl Catalog {
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.0.get(id)
    }
    pub fn model(&self, provider: &str, model: &str) -> Option<&ModelConfig> {
        self.0.get(provider)?.models.get(model)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn default_base_url(provider: &str, protocol: Protocol) -> Option<&'static str> {
    Some(match provider {
        "openai" => "https://api.openai.com/v1",
        "anthropic" => "https://api.anthropic.com/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        _ => match protocol {
            Protocol::Anthropic => "https://api.anthropic.com/v1",
            Protocol::Responses => "https://api.openai.com/v1",
            Protocol::Chat => return None,
        },
    })
}

fn deep_merge_into(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    merge_maps(dst, src.clone());
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn opt_str<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

/// Does a provider have a usable API key (options.apiKey or an env var set)?
fn has_key(p: &ProviderConfig, env: &dyn Fn(&str) -> Option<String>) -> bool {
    opt_str(&p.options, "apiKey").is_some() || p.env.iter().any(|e| env(e).is_some_and(|v| !v.is_empty()))
}

fn variant_has_reasoning(v: &Map<String, Value>) -> bool {
    const KEYS: &[&str] =
        &["reasoningEffort", "reasoning_effort", "reasoning", "thinking", "reasoningSummary", "budgetTokens", "effort"];
    v.keys().any(|k| KEYS.contains(&k.as_str()))
}

fn interleaved_field(v: &Value) -> Option<String> {
    match v {
        Value::Bool(true) => Some("reasoning_content".into()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(m) => m.get("field").and_then(|f| f.as_str()).map(str::to_string),
        _ => None,
    }
}

fn image_input(m: &ModelConfig) -> Option<bool> {
    let modal = m.modalities.as_ref().map(|md| md.input.iter().any(|i| i == "image"));
    match (m.attachment, modal) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (None, None) => None,
        _ => Some(false),
    }
}

/// Effective context window: limit.context, capped by limit.input when that is smaller.
fn limit_context(l: &Limit) -> Option<u64> {
    match (l.context.filter(|c| *c > 0), l.input.filter(|c| *c > 0)) {
        (Some(c), Some(i)) => Some(c.min(i)),
        (c, i) => c.or(i),
    }
}

pub(crate) fn resolve(c: &Config, r: &ModelRef) -> anyhow::Result<ModelSpec> {
    let env = |k: &str| std::env::var(k).ok();
    resolve_with(c, r, &env)
}

pub(crate) fn resolve_with(
    c: &Config,
    r: &ModelRef,
    env: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<ModelSpec> {
    let cat_p = c.catalog.provider(&r.provider);
    let cfg_p = c.providers.get(&r.provider);
    let provider: &ProviderConfig = match (cfg_p, cat_p) {
        (Some(p), _) => p,
        (None, Some(p)) if has_key(p, env) => p,
        (None, Some(p)) => anyhow::bail!(
            "provider {:?} is not configured and no API key env var is set ({})",
            r.provider,
            p.env.join(", ")
        ),
        (None, None) => anyhow::bail!("unknown provider {:?}", r.provider),
    };
    let cat_m = c.catalog.model(&r.provider, &r.model);
    let cfg_m = cfg_p.and_then(|p| p.models.get(&r.model));
    let empty = ModelConfig::default();
    let model: &ModelConfig = match (cfg_m, cat_m) {
        (Some(m), _) => m,
        (None, Some(m)) => m,
        (None, None) => {
            anyhow::bail!("unknown model {:?} for provider {:?}", r.model, r.provider)
        }
    };
    // Catalog entry used to fill gaps of a config model (matching provider + model id first,
    // then the real API id).
    let api_model = nonempty(&model.id).unwrap_or(&r.model).to_string();
    let fill = cat_m.or_else(|| c.catalog.model(&r.provider, &api_model)).unwrap_or(&empty);

    // Protocol.
    let npm = model
        .provider
        .as_ref()
        .and_then(|o| nonempty(&o.npm))
        .or_else(|| nonempty(&provider.npm))
        .or_else(|| cat_p.and_then(|p| nonempty(&p.npm)));
    let protocol = npm.map(Protocol::from_npm).unwrap_or(Protocol::Chat);

    // Base URL.
    let base_url = model
        .provider
        .as_ref()
        .and_then(|o| nonempty(&o.api))
        .or_else(|| opt_str(&provider.options, "baseURL"))
        .or_else(|| nonempty(&provider.api))
        .or_else(|| cat_p.and_then(|p| nonempty(&p.api)))
        .map(str::to_string)
        .or_else(|| default_base_url(&r.provider, protocol).map(str::to_string))
        .ok_or_else(|| anyhow::anyhow!("provider {:?} has no base URL (set options.baseURL)", r.provider))?;
    let base_url = base_url.trim_end_matches('/').to_string();

    // API key.
    let api_key = opt_str(&provider.options, "apiKey").map(str::to_string).or_else(|| {
        provider
            .env
            .iter()
            .chain(cat_p.map(|p| p.env.iter()).into_iter().flatten())
            .find_map(|e| env(e).filter(|v| !v.is_empty()))
    });

    // Headers.
    let mut headers = BTreeMap::new();
    if let Some(Value::Object(h)) = provider.options.get("headers") {
        for (k, v) in h {
            if let Some(s) = v.as_str() {
                headers.insert(k.clone(), s.to_string());
            }
        }
    }
    for (k, v) in &model.headers {
        headers.insert(k.clone(), v.clone());
    }

    // Body overlay.
    let mut body = Map::new();
    for key in ["body", "extraBody"] {
        for src in [&provider.options, &model.options] {
            if let Some(Value::Object(b)) = src.get(key) {
                deep_merge_into(&mut body, b);
            }
        }
    }

    // Options: provider (minus transport keys) <- model <- variant.
    let mut options: Map<String, Value> =
        provider.options.iter().filter(|(k, _)| !PROVIDER_ONLY.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    let model_opts: Map<String, Value> =
        model.options.iter().filter(|(k, _)| !matches!(k.as_str(), "body" | "extraBody")).map(|(k, v)| (k.clone(), v.clone())).collect();
    deep_merge_into(&mut options, &model_opts);
    if let Some(vname) = r.variant.as_deref() {
        match model.variants.get(vname).or_else(|| fill.variants.get(vname)) {
            Some(v) if v.get("disabled").and_then(|d| d.as_bool()) == Some(true) => {
                anyhow::bail!("variant {vname:?} of {} is disabled", r.key())
            }
            Some(v) => {
                let v: Map<String, Value> =
                    v.iter().filter(|(k, _)| !VARIANT_META.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
                deep_merge_into(&mut options, &v);
            }
            None => anyhow::bail!("unknown variant {vname:?} for {}", r.key()),
        }
    }
    let chunk_timeout = options
        .get("chunkTimeout")
        .and_then(|v| v.as_u64())
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis);

    // Limits.
    let lim = |m: &ModelConfig| m.limit.as_ref().and_then(limit_context);
    let out = |m: &ModelConfig| m.limit.as_ref().and_then(|l| l.output).filter(|o| *o > 0);
    let context = lim(model).or_else(|| lim(fill)).unwrap_or(DEFAULT_CONTEXT);
    let output = out(model).or_else(|| out(fill)).unwrap_or(DEFAULT_OUTPUT);

    let reasoning = model.reasoning.or(model.thinking).unwrap_or(false)
        || (model.reasoning.is_none() && model.thinking.is_none() && fill.reasoning.unwrap_or(false))
        || model.variants.values().any(variant_has_reasoning);
    let image_input = image_input(model).or_else(|| image_input(fill)).unwrap_or(false);
    let interleaved_field = model.interleaved.as_ref().or(fill.interleaved.as_ref()).and_then(interleaved_field);

    Ok(ModelSpec {
        provider_id: r.provider.clone(),
        api_model,
        protocol: Some(protocol),
        base_url,
        api_key,
        headers,
        options,
        body,
        context,
        output,
        reasoning,
        image_input,
        interleaved_field,
        chunk_timeout,
    })
}

/// Non-disabled variant names in config order.
fn variant_names(m: &ModelConfig) -> Vec<String> {
    m.variants
        .iter()
        .filter(|(_, v)| v.get("disabled").and_then(|d| d.as_bool()) != Some(true))
        .map(|(k, _)| k.clone())
        .collect()
}

fn listed(p: &ProviderConfig, id: &str) -> bool {
    if let Some(w) = &p.whitelist {
        if !w.iter().any(|x| x == id) {
            return false;
        }
    }
    !p.blacklist.as_ref().is_some_and(|b| b.iter().any(|x| x == id))
}

pub(crate) fn list(c: &Config) -> Vec<ModelInfo> {
    let mut out = Vec::new();
    for (pid, p) in &c.providers {
        let provider_name = nonempty(&p.name)
            .or_else(|| c.catalog.provider(pid).and_then(|cp| nonempty(&cp.name)))
            .unwrap_or(pid)
            .to_string();
        for (mid, m) in &p.models {
            if !listed(p, mid) {
                continue;
            }
            let r = ModelRef { provider: pid.clone(), model: mid.clone(), variant: None };
            let empty = ModelConfig::default();
            let fill = c.catalog.model(pid, mid).unwrap_or(&empty);
            let (context, output, reasoning, image_input) = match resolve_with(c, &r, &|_| None) {
                Ok(s) => (s.context, s.output, s.reasoning, s.image_input),
                Err(_) => (
                    m.limit.as_ref().and_then(limit_context).unwrap_or(DEFAULT_CONTEXT),
                    m.limit.as_ref().and_then(|l| l.output).unwrap_or(DEFAULT_OUTPUT),
                    m.reasoning.or(m.thinking).unwrap_or(false),
                    image_input(m).unwrap_or(false),
                ),
            };
            out.push(ModelInfo {
                provider: pid.clone(),
                provider_name: provider_name.clone(),
                model: mid.clone(),
                name: nonempty(&m.name).or_else(|| nonempty(&fill.name)).unwrap_or(mid).to_string(),
                context,
                output,
                reasoning,
                image_input,
                variants: variant_names(m),
            });
        }
    }
    out
}

pub(crate) fn default_for(c: &Config, agent: &str) -> Option<ModelRef> {
    if let Some(a) = c.agents.get(agent) {
        if let Some(mut r) = a.model.as_deref().and_then(ModelRef::parse) {
            if r.variant.is_none() {
                r.variant = a.variant.clone().filter(|v| !v.is_empty());
            }
            return Some(r);
        }
    }
    if let Some(m) = &c.model {
        return Some(m.clone());
    }
    c.providers.iter().find_map(|(pid, p)| {
        p.models
            .keys()
            .find(|m| listed(p, m))
            .map(|m| ModelRef { provider: pid.clone(), model: m.clone(), variant: None })
    })
}

pub(crate) fn context_limits(c: &Config, spec: &ModelSpec, m: &ModelRef) -> (u64, u64) {
    let max = if spec.context > 0 { spec.context } else { DEFAULT_CONTEXT };
    let per_model = c
        .providers
        .get(&m.provider)
        .and_then(|p| p.models.get(&m.model))
        .and_then(|mc| mc.limit.as_ref())
        .and_then(|l| l.compact)
        .filter(|n| *n > 0);
    let ratio = if c.compaction.ratio.is_finite() && c.compaction.ratio > 0.0 { c.compaction.ratio.min(1.0) } else { 0.8 };
    let compact_at = per_model.unwrap_or((max as f64 * ratio) as u64).min(max);
    (max, compact_at)
}

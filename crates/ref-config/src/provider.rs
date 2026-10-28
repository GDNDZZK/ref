//! Providers: config `provider` map, models.dev catalog cache, auth.json, model resolution.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use anyhow::bail;
use ref_proto::spec::{ApiKind, Cost, ModelSpec};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::jsonc::merge_map;

/// Deserialize a field leniently: type mismatches become `None` instead of failing the document.
fn lenient<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct LimitDef {
    #[serde(default, deserialize_with = "lenient")]
    pub context: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub input: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub output: Option<f64>,
    /// ref extension: recommended compaction point (absolute tokens).
    #[serde(default, deserialize_with = "lenient")]
    pub compact: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ModalitiesDef {
    #[serde(default, deserialize_with = "lenient")]
    pub input: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient")]
    pub output: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct CostDef {
    #[serde(default, deserialize_with = "lenient")]
    pub input: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub output: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub cache_read: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ProvOverride {
    #[serde(default, deserialize_with = "lenient")]
    pub npm: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub api: Option<String>,
}

/// One model definition (config or catalog). All fields optional; config overrides catalog.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ModelDef {
    #[serde(default, deserialize_with = "lenient")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub family: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub attachment: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub reasoning: Option<bool>,
    /// Non-standard flag used by some configs instead of `reasoning`.
    #[serde(default, deserialize_with = "lenient")]
    pub thinking: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub temperature: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub tool_call: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub limit: Option<LimitDef>,
    #[serde(default, deserialize_with = "lenient")]
    pub modalities: Option<ModalitiesDef>,
    #[serde(default, deserialize_with = "lenient")]
    pub cost: Option<CostDef>,
    #[serde(default, deserialize_with = "lenient")]
    pub options: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "lenient")]
    pub headers: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "lenient")]
    pub variants: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "lenient")]
    pub provider: Option<ProvOverride>,
    #[serde(default, deserialize_with = "lenient")]
    pub api_format: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub status: Option<String>,
}

fn or<T: Clone>(a: &Option<T>, b: &Option<T>) -> Option<T> {
    a.clone().or_else(|| b.clone())
}

impl ModelDef {
    /// `self` (config) over `cat` (catalog defaults).
    pub fn over(&self, cat: &ModelDef) -> ModelDef {
        let limit = match (&self.limit, &cat.limit) {
            (Some(a), Some(b)) => Some(LimitDef {
                context: a.context.or(b.context),
                input: a.input.or(b.input),
                output: a.output.or(b.output),
                compact: a.compact.or(b.compact),
            }),
            (a, b) => or(a, b),
        };
        let modalities = match (&self.modalities, &cat.modalities) {
            (Some(a), Some(b)) => Some(ModalitiesDef {
                input: or(&a.input, &b.input),
                output: or(&a.output, &b.output),
            }),
            (a, b) => or(a, b),
        };
        let provider = match (&self.provider, &cat.provider) {
            (Some(a), Some(b)) => Some(ProvOverride {
                npm: or(&a.npm, &b.npm),
                api: or(&a.api, &b.api),
            }),
            (a, b) => or(a, b),
        };
        ModelDef {
            id: or(&self.id, &cat.id),
            name: or(&self.name, &cat.name),
            family: or(&self.family, &cat.family),
            attachment: self.attachment.or(cat.attachment),
            reasoning: self.reasoning.or(cat.reasoning),
            thinking: self.thinking.or(cat.thinking),
            temperature: self.temperature.or(cat.temperature),
            tool_call: self.tool_call.or(cat.tool_call),
            limit,
            modalities,
            cost: or(&self.cost, &cat.cost),
            options: or(&self.options, &cat.options),
            headers: or(&self.headers, &cat.headers),
            variants: or(&self.variants, &cat.variants),
            provider,
            api_format: or(&self.api_format, &cat.api_format),
            status: or(&self.status, &cat.status),
        }
    }

    pub fn is_reasoning(&self) -> bool {
        self.reasoning.unwrap_or(false) || self.thinking.unwrap_or(false)
    }

    pub fn image_input(&self) -> bool {
        let modal = self
            .modalities
            .as_ref()
            .and_then(|m| m.input.as_ref())
            .is_some_and(|i| i.iter().any(|x| x == "image"));
        modal || self.attachment.unwrap_or(false)
    }

    pub fn context(&self) -> u64 {
        self.limit
            .as_ref()
            .and_then(|l| l.context)
            .filter(|c| *c > 0.0)
            .map(|c| c as u64)
            .unwrap_or(DEFAULT_CONTEXT)
    }

    pub fn max_output(&self) -> u64 {
        let context = self.context();
        let out = self
            .limit
            .as_ref()
            .and_then(|l| l.output)
            .filter(|o| *o > 0.0)
            .map(|o| o as u64)
            .unwrap_or(DEFAULT_OUTPUT);
        if out > context {
            context / 4
        } else {
            out
        }
    }
}

pub(crate) const DEFAULT_CONTEXT: u64 = 128_000;
pub(crate) const DEFAULT_OUTPUT: u64 = 32_000;

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct CatProvider {
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub env: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient")]
    pub npm: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub api: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub models: Option<BTreeMap<String, ModelDef>>,
}

#[derive(Debug, Default)]
pub(crate) struct Catalog {
    pub providers: BTreeMap<String, CatProvider>,
}

impl Catalog {
    pub fn model(&self, provider: &str, key: &str) -> Option<&ModelDef> {
        self.providers.get(provider)?.models.as_ref()?.get(key)
    }
}

/// Which catalog providers need their models parsed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CatalogFilter {
    /// Always keep (configured providers, providers with stored credentials).
    pub keep: std::collections::BTreeSet<String>,
    /// Non-empty env var names: keep providers whose `env` lists one of them.
    pub env: std::collections::BTreeSet<String>,
}

impl CatalogFilter {
    fn wants(&self, id: &str, env: Option<&[String]>) -> bool {
        self.keep.contains(id) || env.unwrap_or_default().iter().any(|e| self.env.contains(e))
    }
}

/// Streaming catalog deserializer: providers not wanted by the filter keep their metadata but
/// their (large) `models` map is skipped without allocation.
struct CatalogSeed<'a>(&'a CatalogFilter);

impl<'de> serde::de::DeserializeSeed<'de> for CatalogSeed<'_> {
    type Value = BTreeMap<String, CatProvider>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> serde::de::Visitor<'de> for CatalogSeed<'_> {
    type Value = BTreeMap<String, CatProvider>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("models.dev catalog object")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut out = BTreeMap::new();
        while let Some(id) = map.next_key::<String>()? {
            let p = map.next_value_seed(ProviderSeed {
                filter: self.0,
                id: &id,
            })?;
            if let Some(p) = p {
                out.insert(id, p);
            }
        }
        Ok(out)
    }
}

struct ProviderSeed<'a> {
    filter: &'a CatalogFilter,
    id: &'a str,
}

impl<'de> serde::de::DeserializeSeed<'de> for ProviderSeed<'_> {
    type Value = Option<CatProvider>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for ProviderSeed<'_> {
    type Value = Option<CatProvider>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("provider object")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        use serde::de::IgnoredAny;
        let mut p = CatProvider::default();
        while let Some(k) = map.next_key::<String>()? {
            match k.as_str() {
                "name" => p.name = serde_json::from_value(map.next_value::<Value>()?).ok(),
                "env" => p.env = serde_json::from_value(map.next_value::<Value>()?).ok(),
                "npm" => p.npm = serde_json::from_value(map.next_value::<Value>()?).ok(),
                "api" => p.api = serde_json::from_value(map.next_value::<Value>()?).ok(),
                // `env` precedes `models` in models.dev output.
                "models" if self.filter.wants(self.id, p.env.as_deref()) => {
                    let v = map.next_value::<BTreeMap<String, Value>>()?;
                    let models = v
                        .into_iter()
                        .filter_map(|(k, v)| {
                            serde_json::from_value::<ModelDef>(v).ok().map(|m| (k, m))
                        })
                        .collect();
                    p.models = Some(models);
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Some(p))
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
        Ok(None)
    }
    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(None)
    }
}

struct CacheEntry {
    mtime: Option<SystemTime>,
    len: u64,
    filter: CatalogFilter,
    catalog: Arc<Catalog>,
}

/// Load the models.dev catalog, parsing models only for providers wanted by `filter`.
/// Cached process-wide per path (reused while file mtime/size and filter are unchanged).
pub(crate) fn load_catalog(path: &Path, filter: &CatalogFilter) -> Option<Arc<Catalog>> {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, CacheEntry>>> = OnceLock::new();
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok();
    let len = meta.len();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(e) = cache.lock().ok()?.get(path) {
        if e.mtime == mtime && e.len == len && &e.filter == filter {
            return Some(e.catalog.clone());
        }
    }
    let text = std::fs::read(path).ok()?;
    let mut de = serde_json::Deserializer::from_slice(&text);
    let providers = match serde::de::DeserializeSeed::deserialize(CatalogSeed(filter), &mut de) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("models catalog {}: {e}", path.display());
            return None;
        }
    };
    let catalog = Arc::new(Catalog { providers });
    if let Ok(mut c) = cache.lock() {
        c.insert(
            path.to_path_buf(),
            CacheEntry {
                mtime,
                len,
                filter: filter.clone(),
                catalog: catalog.clone(),
            },
        );
    }
    Some(catalog)
}

/// `auth.json`: `{providerId: {type: "api", key}}` → providerId → key.
pub(crate) fn load_auth(path: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text) else {
        tracing::warn!("invalid auth file {}", path.display());
        return out;
    };
    for (id, v) in m {
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("api");
        if ty != "api" && ty != "wellknown" {
            continue;
        }
        if let Some(k) = v.get("key").and_then(|k| k.as_str()) {
            out.insert(id, k.to_string());
        }
    }
    out
}

/// Config `provider.<id>` entry.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderDef {
    pub id: String,
    pub name: Option<String>,
    pub npm: Option<String>,
    pub api: Option<String>,
    pub env: Option<Vec<String>>,
    pub options: Map<String, Value>,
    pub models: Vec<(String, ModelDef)>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
    pub api_format: Option<String>,
}

fn str_list(v: Option<&Value>) -> Option<Vec<String>> {
    Some(
        v?.as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
    )
}

pub(crate) fn parse_providers(v: Option<&Value>) -> Vec<ProviderDef> {
    let mut out = Vec::new();
    let Some(Value::Object(map)) = v else {
        return out;
    };
    for (id, p) in map {
        let Value::Object(p) = p else {
            tracing::warn!("provider {id}: not an object");
            continue;
        };
        let s = |k: &str| p.get(k).and_then(|x| x.as_str()).map(String::from);
        let mut models = Vec::new();
        if let Some(Value::Object(ms)) = p.get("models") {
            for (key, mv) in ms {
                match serde_json::from_value::<ModelDef>(mv.clone()) {
                    Ok(m) => models.push((key.clone(), m)),
                    Err(e) => tracing::warn!("provider {id} model {key}: {e}"),
                }
            }
        }
        out.push(ProviderDef {
            id: id.clone(),
            name: s("name"),
            npm: s("npm"),
            api: s("api"),
            env: str_list(p.get("env")),
            options: p
                .get("options")
                .and_then(|o| o.as_object())
                .cloned()
                .unwrap_or_default(),
            models,
            whitelist: str_list(p.get("whitelist")),
            blacklist: str_list(p.get("blacklist")),
            api_format: s("api_format"),
        });
    }
    out
}

/// A provider usable by ref: configured (and not disabled) or catalog provider with credentials.
#[derive(Clone)]
pub(crate) struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub npm: Option<String>,
    /// Config `api` / options.baseURL / catalog api / well-known default (provider level).
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub options: Map<String, Value>,
    pub api_format: Option<String>,
    /// Listed models (merged config over catalog), config order then catalog order.
    pub models: Vec<(String, ModelDef)>,
    pub whitelist: Option<Vec<String>>,
    pub blacklist: Option<Vec<String>>,
}

impl std::fmt::Debug for ProviderInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print api_key / options (may contain secrets).
        f.debug_struct("Provider")
            .field("id", &self.id)
            .field("npm", &self.npm)
            .field("base_url", &self.base_url)
            .field("has_key", &self.api_key.is_some())
            .field(
                "models",
                &self.models.iter().map(|m| &m.0).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ProviderInfo {
    fn allowed(&self, key: &str) -> bool {
        if let Some(w) = &self.whitelist {
            if !w.iter().any(|x| x == key) {
                return false;
            }
        }
        if let Some(b) = &self.blacklist {
            if b.iter().any(|x| x == key) {
                return false;
            }
        }
        true
    }
}

pub(crate) fn default_base_url(npm: &str) -> Option<&'static str> {
    Some(match npm {
        "@ai-sdk/anthropic" => "https://api.anthropic.com/v1",
        "@ai-sdk/openai" => "https://api.openai.com/v1",
        "@ai-sdk/google" => "https://generativelanguage.googleapis.com/v1beta/openai",
        "@ai-sdk/mistral" => "https://api.mistral.ai/v1",
        "@ai-sdk/xai" => "https://api.x.ai/v1",
        "@ai-sdk/groq" => "https://api.groq.com/openai/v1",
        "@ai-sdk/cerebras" => "https://api.cerebras.ai/v1",
        "@ai-sdk/deepinfra" => "https://api.deepinfra.com/v1/openai",
        "@ai-sdk/togetherai" => "https://api.together.xyz/v1",
        "@ai-sdk/perplexity" => "https://api.perplexity.ai",
        _ => return None,
    })
}

pub(crate) fn api_kind_from_npm(npm: Option<&str>) -> ApiKind {
    match npm {
        Some("@ai-sdk/anthropic") => ApiKind::Anthropic,
        Some("@ai-sdk/openai") => ApiKind::OpenAIResponses,
        _ => ApiKind::OpenAIChat,
    }
}

pub(crate) fn api_kind_from_format(f: &str) -> Option<ApiKind> {
    match f.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "messages" => Some(ApiKind::Anthropic),
        "chat" | "openai-chat" | "openai_chat" | "openai-compatible" | "completions" => {
            Some(ApiKind::OpenAIChat)
        }
        "responses" | "openai-responses" | "openai_responses" | "openai" => {
            Some(ApiKind::OpenAIResponses)
        }
        _ => {
            tracing::warn!("unknown api_format {f:?}");
            None
        }
    }
}

/// Transport-level option keys removed from the model `options`.
const TRANSPORT_KEYS: &[&str] = &[
    "apiKey",
    "baseURL",
    "headers",
    "timeout",
    "headerTimeout",
    "chunkTimeout",
    "body",
    "extraBody",
    "setCacheKey",
    "enterpriseUrl",
];

pub(crate) struct ProviderInputs<'a> {
    pub defs: &'a [ProviderDef],
    pub catalog: Option<&'a Catalog>,
    pub auth: &'a BTreeMap<String, String>,
    pub env: &'a BTreeMap<String, String>,
    pub disabled: &'a [String],
    pub enabled: Option<&'a [String]>,
}

fn trim_url(s: &str) -> String {
    s.trim().trim_end_matches('/').to_string()
}

/// Build the provider list: configured providers (config order) then catalog providers with
/// credentials (sorted by id).
pub(crate) fn build_providers(inp: &ProviderInputs) -> Vec<ProviderInfo> {
    let is_enabled = |id: &str| {
        !inp.disabled.iter().any(|d| d == id)
            && inp.enabled.is_none_or(|e| e.iter().any(|x| x == id))
    };
    let env_key = |names: &[String]| {
        names
            .iter()
            .find_map(|n| inp.env.get(n).filter(|v| !v.is_empty()).cloned())
    };
    let mut out = Vec::new();
    for def in inp.defs {
        if !is_enabled(&def.id) {
            continue;
        }
        let cat = inp.catalog.and_then(|c| c.providers.get(&def.id));
        let npm = def.npm.clone().or_else(|| cat.and_then(|c| c.npm.clone()));
        let base_url = def
            .options
            .get("baseURL")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| def.api.clone())
            .or_else(|| cat.and_then(|c| c.api.clone()))
            .or_else(|| npm.as_deref().and_then(default_base_url).map(String::from))
            .map(|s| trim_url(&s));
        let env_names = def
            .env
            .clone()
            .or_else(|| cat.and_then(|c| c.env.clone()))
            .unwrap_or_default();
        let api_key = def
            .options
            .get("apiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| inp.auth.get(&def.id).cloned())
            .or_else(|| env_key(&env_names));
        let cat_models = cat.and_then(|c| c.models.as_ref());
        let mut models: Vec<(String, ModelDef)> = Vec::new();
        if def.models.is_empty() {
            if let Some(cm) = cat_models {
                for (k, m) in cm {
                    models.push((k.clone(), m.clone()));
                }
            }
        } else {
            for (k, m) in &def.models {
                let cm = cat_models.and_then(|cm| {
                    cm.get(k)
                        .or_else(|| m.id.as_ref().and_then(|id| cm.get(id)))
                });
                let merged = match cm {
                    Some(c) => m.over(c),
                    None => m.clone(),
                };
                models.push((k.clone(), merged));
            }
        }
        let name = def
            .name
            .clone()
            .or_else(|| cat.and_then(|c| c.name.clone()))
            .unwrap_or_else(|| def.id.clone());
        out.push(ProviderInfo {
            id: def.id.clone(),
            name,
            npm,
            base_url,
            api_key,
            options: def.options.clone(),
            api_format: def.api_format.clone(),
            models,
            whitelist: def.whitelist.clone(),
            blacklist: def.blacklist.clone(),
        });
    }
    if let Some(catalog) = inp.catalog {
        for (id, cat) in &catalog.providers {
            if inp.defs.iter().any(|d| &d.id == id) || !is_enabled(id) {
                continue;
            }
            let api_key = inp
                .auth
                .get(id)
                .cloned()
                .or_else(|| env_key(cat.env.as_deref().unwrap_or_default()));
            let Some(api_key) = api_key else { continue };
            let base_url = cat
                .api
                .clone()
                .or_else(|| {
                    cat.npm
                        .as_deref()
                        .and_then(default_base_url)
                        .map(String::from)
                })
                .map(|s| trim_url(&s));
            if base_url.is_none() {
                continue;
            }
            let models = cat
                .models
                .as_ref()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            out.push(ProviderInfo {
                id: id.clone(),
                name: cat.name.clone().unwrap_or_else(|| id.clone()),
                npm: cat.npm.clone(),
                base_url,
                api_key: Some(api_key),
                options: Map::new(),
                api_format: None,
                models,
                whitelist: None,
                blacklist: None,
            });
        }
    }
    out
}

/// Configured variant names (not disabled) or derived defaults for reasoning models.
pub(crate) fn variant_names(m: &ModelDef, api: ApiKind) -> Vec<String> {
    if let Some(vs) = &m.variants {
        return vs
            .iter()
            .filter(|(_, v)| v.get("disabled").and_then(|d| d.as_bool()) != Some(true))
            .map(|(k, _)| k.clone())
            .collect();
    }
    if !m.is_reasoning() {
        return vec![];
    }
    let names: &[&str] = match api {
        ApiKind::Anthropic => &["low", "medium", "high", "max"],
        ApiKind::OpenAIResponses => &["low", "medium", "high", "xhigh"],
        ApiKind::OpenAIChat => &["low", "medium", "high"],
    };
    names.iter().map(|s| s.to_string()).collect()
}

/// Options for a variant (configured, or derived default). `None` if unknown.
pub(crate) fn variant_options(
    m: &ModelDef,
    api: ApiKind,
    name: &str,
) -> Option<Map<String, Value>> {
    if let Some(vs) = &m.variants {
        let v = vs.get(name)?;
        if v.get("disabled").and_then(|d| d.as_bool()) == Some(true) {
            return None;
        }
        let mut o = v.as_object().cloned().unwrap_or_default();
        o.remove("disabled");
        return Some(o);
    }
    if !variant_names(m, api).iter().any(|n| n == name) {
        return None;
    }
    let mut o = Map::new();
    match api {
        ApiKind::Anthropic => {
            o.insert("effort".into(), Value::String(name.into()));
        }
        ApiKind::OpenAIResponses => {
            o.insert("reasoningEffort".into(), Value::String(name.into()));
            o.insert("reasoningSummary".into(), Value::String("auto".into()));
        }
        ApiKind::OpenAIChat => {
            o.insert("reasoningEffort".into(), Value::String(name.into()));
        }
    }
    Some(o)
}

pub(crate) fn model_api_kind(p: &ProviderInfo, m: &ModelDef) -> ApiKind {
    if let Some(k) = m.api_format.as_deref().and_then(api_kind_from_format) {
        return k;
    }
    let model_npm = m.provider.as_ref().and_then(|o| o.npm.as_deref());
    if model_npm.is_none() {
        if let Some(k) = p.api_format.as_deref().and_then(api_kind_from_format) {
            return k;
        }
    }
    api_kind_from_npm(model_npm.or(p.npm.as_deref()))
}

fn ms_timeout(v: Option<&Value>) -> Option<Option<Duration>> {
    match v? {
        Value::Bool(false) => Some(None),
        Value::Number(n) => n
            .as_f64()
            .filter(|f| *f > 0.0)
            .map(|f| Some(Duration::from_millis(f as u64))),
        _ => None,
    }
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const NO_TIMEOUT: Duration = Duration::from_secs(3600);

fn headers_into(dst: &mut BTreeMap<String, String>, src: Option<&Value>) {
    if let Some(Value::Object(h)) = src {
        for (k, v) in h {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Null => continue,
                other => other.to_string(),
            };
            dst.insert(k.clone(), s);
        }
    }
}

/// Build a ModelSpec for a provider model with an optional variant.
pub(crate) fn build_spec(
    p: &ProviderInfo,
    key: &str,
    m: &ModelDef,
    variant: Option<&str>,
) -> anyhow::Result<ModelSpec> {
    let api = model_api_kind(p, m);
    // Merge provider + model options; extract transport keys afterwards.
    let mut all = p.options.clone();
    if let Some(mo) = &m.options {
        merge_map(&mut all, mo);
    }
    if let Some(v) = variant {
        match variant_options(m, api, v) {
            Some(vo) => merge_map(&mut all, &vo),
            None => {
                let names = variant_names(m, api);
                if names.is_empty() {
                    bail!("model {}/{key} has no variants (requested {v:?})", p.id);
                }
                bail!(
                    "unknown variant {v:?} for {}/{key}; available: {}",
                    p.id,
                    names.join(", ")
                );
            }
        }
    }

    let model_npm = m.provider.as_ref().and_then(|o| o.npm.clone());
    let base_url = m
        .provider
        .as_ref()
        .and_then(|o| o.api.clone())
        .or_else(|| {
            m.options
                .as_ref()
                .and_then(|o| o.get("baseURL"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .or_else(|| {
            // A model-level package override changes the default endpoint only if the provider
            // has no explicit one.
            p.base_url.clone()
        })
        .or_else(|| {
            model_npm
                .as_deref()
                .and_then(default_base_url)
                .map(String::from)
        })
        .map(|s| trim_url(&s));
    let Some(base_url) = base_url else {
        bail!(
            "provider {} has no base URL (set provider.{}.options.baseURL)",
            p.id,
            p.id
        );
    };
    let api_key = all
        .get("apiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| p.api_key.clone());

    let mut headers = BTreeMap::new();
    headers_into(&mut headers, p.options.get("headers"));
    headers_into(
        &mut headers,
        m.options.as_ref().and_then(|o| o.get("headers")),
    );
    if let Some(h) = &m.headers {
        headers_into(&mut headers, Some(&Value::Object(h.clone())));
    }

    let mut extra_body = Map::new();
    for src in [&p.options, m.options.as_ref().unwrap_or(&Map::new())] {
        for k in ["body", "extraBody"] {
            if let Some(Value::Object(b)) = src.get(k) {
                merge_map(&mut extra_body, b);
            }
        }
    }

    let total = ms_timeout(all.get("timeout"));
    let header_timeout = match ms_timeout(all.get("headerTimeout")) {
        Some(Some(d)) => d,
        Some(None) => NO_TIMEOUT,
        None => match total {
            Some(Some(d)) => d.min(DEFAULT_TIMEOUT).max(Duration::from_secs(1)),
            Some(None) | None => DEFAULT_TIMEOUT,
        },
    };
    let chunk_timeout = match ms_timeout(all.get("chunkTimeout")) {
        Some(Some(d)) => d,
        Some(None) => NO_TIMEOUT,
        None => DEFAULT_TIMEOUT,
    };

    let mut options = all;
    for k in TRANSPORT_KEYS {
        options.remove(*k);
    }

    let c = m.cost.clone().unwrap_or_default();
    Ok(ModelSpec {
        provider_id: p.id.clone(),
        model_key: key.to_string(),
        api_model: m.id.clone().unwrap_or_else(|| key.to_string()),
        api,
        base_url,
        api_key,
        headers,
        options,
        extra_body,
        context: m.context(),
        max_output: m.max_output(),
        image_input: m.image_input(),
        reasoning: m.is_reasoning(),
        temperature: None,
        top_p: None,
        cost: Cost {
            input: c.input.unwrap_or(0.0),
            output: c.output.unwrap_or(0.0),
            cache_read: c.cache_read.unwrap_or(0.0),
            cache_write: c.cache_write.unwrap_or(0.0),
        },
        header_timeout,
        chunk_timeout,
    })
}

/// Find a model in a provider: listed models, then the catalog (if not filtered).
pub(crate) fn find_model(
    p: &ProviderInfo,
    key: &str,
    catalog: Option<&Catalog>,
) -> Option<ModelDef> {
    if !p.allowed(key) {
        return None;
    }
    if let Some((_, m)) = p.models.iter().find(|(k, _)| k == key) {
        return Some(m.clone());
    }
    catalog.and_then(|c| c.model(&p.id, key)).cloned()
}

pub(crate) fn listed_models(p: &ProviderInfo) -> impl Iterator<Item = &(String, ModelDef)> {
    p.models
        .iter()
        .filter(|(k, m)| p.allowed(k) && m.status.as_deref() != Some("deprecated"))
}

/// Simple edit distance for "did you mean" suggestions.
pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

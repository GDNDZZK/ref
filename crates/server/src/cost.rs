//! Cost accounting from `models.<id>.cost` ($ per 1M tokens).

use hn_config::Config;
use hn_proto::{ModelRef, Usage};
use serde_json::Value;

fn rate(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).filter(|x| x.is_finite() && *x >= 0.0).unwrap_or(0.0)
}

/// Dollar cost of one response, or 0 when the model has no cost info.
pub fn cost(config: &Config, model: &ModelRef, u: &Usage) -> f64 {
    let cfg = config
        .providers
        .get(&model.provider)
        .and_then(|p| p.models.get(&model.model))
        .and_then(|m| m.cost.clone())
        .or_else(|| config.catalog.model(&model.provider, &model.model).and_then(|m| m.cost.clone()));
    let Some(c) = cfg else { return 0.0 };
    let total_in = u.input + u.cache_read + u.cache_write;
    // Long-context pricing tier when present.
    let c = match c.get("context_over_200k") {
        Some(tier) if total_in > 200_000 && tier.is_object() => tier.clone(),
        _ => c,
    };
    let input = rate(&c, "input");
    let per = |n: u64, r: f64| n as f64 * r / 1_000_000.0;
    let cache_read = c.get("cache_read").map(|_| rate(&c, "cache_read")).unwrap_or(input);
    let cache_write = c.get("cache_write").map(|_| rate(&c, "cache_write")).unwrap_or(input);
    per(u.input, input) + per(u.output, rate(&c, "output")) + per(u.cache_read, cache_read) + per(u.cache_write, cache_write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_cost() {
        let mut c = Config::default();
        let mut p = hn_config::ProviderConfig::default();
        p.models.insert(
            "m".into(),
            hn_config::ModelConfig {
                cost: Some(serde_json::json!({"input": 2.0, "output": 10.0, "cache_read": 0.5})),
                ..Default::default()
            },
        );
        c.providers.insert("p".into(), p);
        let r = ModelRef { provider: "p".into(), model: "m".into(), variant: None };
        let u = Usage { input: 1_000_000, output: 100_000, cache_read: 2_000_000, cache_write: 0, reasoning: 0 };
        assert!((cost(&c, &r, &u) - (2.0 + 1.0 + 1.0)).abs() < 1e-9);
        let none = ModelRef { provider: "x".into(), model: "y".into(), variant: None };
        assert_eq!(cost(&c, &none, &u), 0.0);
    }
}

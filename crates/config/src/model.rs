use crate::*;
pub(crate) fn resolve(_c: &Config, _r: &ModelRef) -> anyhow::Result<ModelSpec> { todo!() }
pub(crate) fn list(_c: &Config) -> Vec<ModelInfo> { todo!() }
pub(crate) fn default_for(_c: &Config, _agent: &str) -> Option<ModelRef> { todo!() }
pub(crate) fn context_limits(_c: &Config, _s: &ModelSpec, _m: &ModelRef) -> (u64, u64) { todo!() }

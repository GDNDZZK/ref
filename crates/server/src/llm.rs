//! Thin wrapper around [`hn_llm::stream`] (single place to add logging / instrumentation).

use hn_llm::{ChatRequest, FinishReason, LlmError, ModelSpec, StreamEvent};
use tokio_util::sync::CancellationToken;

pub async fn stream(
    http: &reqwest::Client,
    spec: &ModelSpec,
    req: &ChatRequest,
    cancel: &CancellationToken,
    on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    let started = std::time::Instant::now();
    let r = hn_llm::stream(http, spec, req, cancel, on_event).await;
    tracing::debug!(
        model = %spec.api_model,
        elapsed_ms = started.elapsed().as_millis() as u64,
        ok = r.is_ok(),
        "llm stream finished"
    );
    r
}

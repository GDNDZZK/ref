use crate::*;
use tokio_util::sync::CancellationToken;

pub(crate) async fn stream(
    _client: &reqwest::Client,
    _spec: &ModelSpec,
    _req: &ChatRequest,
    _cancel: &CancellationToken,
    _on_event: &mut (dyn FnMut(StreamEvent) + Send),
) -> Result<FinishReason, LlmError> {
    todo!()
}

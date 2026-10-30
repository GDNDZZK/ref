//! Claude's per-request image limits, enforced on the outgoing request like oh-my-pi does
//! (stored messages keep their images). See
//! <https://platform.claude.com/docs/en/build-with-claude/vision>: at most 8000px per side,
//! 10MB of base64 per image, and at most 2000px per side once a request has more than 20 images.
//! These catch what `images.auto_resize` does not cover: MCP images, the setting turned off,
//! and many-image requests.

use ref_llm::{ApiKind, ChatMessage, Model, Provider, Request, UserContent};
use ref_proto::ImageData;

const MAX_DIMENSION: u32 = 8000;
const MANY_IMAGE_THRESHOLD: usize = 20;
const MANY_IMAGE_MAX_DIMENSION: u32 = 2000;
/// 10MB of base64, less 10% headroom for the surrounding JSON.
const MAX_IMAGE_BASE64: usize = 10 * 1024 * 1024 / 10 * 9;

/// True if `model` is a Claude model: the Anthropic API, or a model id naming Claude behind
/// another API (proxies forward to the same limits).
fn is_claude(provider: &Provider, model: &Model) -> bool {
    provider.api == ApiKind::Anthropic || model.id.to_ascii_lowercase().contains("claude")
}

fn images_mut(messages: &mut [ChatMessage]) -> impl Iterator<Item = &mut ImageData> {
    messages.iter_mut().flat_map(|m| -> Box<dyn Iterator<Item = &mut ImageData> + '_> {
        match m {
            ChatMessage::User(parts) => Box::new(parts.iter_mut().filter_map(|p| match p {
                UserContent::Image(i) => Some(i),
                UserContent::Text(_) => None,
            })),
            ChatMessage::ToolResult { images, .. } => Box::new(images.iter_mut()),
            ChatMessage::Assistant { .. } => Box::new(std::iter::empty()),
        }
    })
}

/// Downscales the images of `req` that break Claude's limits. Other models: unchanged.
pub async fn clamp_request(mut req: Request, provider: &Provider, model: &Model) -> Request {
    if !model.images || !is_claude(provider, model) {
        return req;
    }
    let count = images_mut(&mut req.messages).count();
    if count == 0 {
        return req;
    }
    let max_dim = if count > MANY_IMAGE_THRESHOLD { MANY_IMAGE_MAX_DIMENSION } else { MAX_DIMENSION };
    let mut messages = std::mem::take(&mut req.messages);
    // `clamp_image_data` catches decoder panics, so the task always hands the messages back.
    req.messages = tokio::task::spawn_blocking(move || {
        for img in images_mut(&mut messages) {
            if let Some(small) = ref_tools::imgfit::clamp_image_data(img, max_dim, MAX_IMAGE_BASE64) {
                *img = small;
            }
        }
        messages
    })
    .await
    .expect("image clamp task does not panic");
    req
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn png(w: u32, h: u32) -> ImageData {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        ImageData::new("image/png", base64::engine::general_purpose::STANDARD.encode(out))
    }

    fn dims(i: &ImageData) -> (u32, u32) {
        let b = base64::engine::general_purpose::STANDARD.decode(&i.data).unwrap();
        let img = image::load_from_memory(&b).unwrap();
        (img.width(), img.height())
    }

    fn setup(api: ApiKind, id: &str) -> (Provider, Model) {
        let provider = Provider {
            id: "p".into(),
            api,
            base_url: String::new(),
            api_key: None,
            headers: Default::default(),
            set_cache_key: false,
            chunk_timeout: std::time::Duration::from_secs(1),
        };
        let model = Model { id: id.into(), context: 0, output: 0, images: true, reasoning: false, options: Default::default(), temperature: None, top_p: None };
        (provider, model)
    }

    fn request(images: Vec<ImageData>) -> Request {
        let tool = ChatMessage::ToolResult { call_id: "c".into(), name: "read".into(), content: String::new(), images: images[1..].to_vec(), is_error: false };
        Request { system: vec![], messages: vec![ChatMessage::User(vec![UserContent::Image(images[0].clone())]), tool], tools: vec![], session_id: "s".into() }
    }

    fn sizes(req: &mut Request) -> Vec<(u32, u32)> {
        images_mut(&mut req.messages).map(|i| dims(i)).collect()
    }

    #[tokio::test]
    async fn many_images_get_the_tighter_cap_and_only_for_claude() {
        let big = png(2400, 600);
        let small = png(300, 300);
        let few = vec![big.clone(), small.clone()];
        let many: Vec<_> = std::iter::once(big.clone()).chain(std::iter::repeat_n(small.clone(), 20)).collect();
        let (p, m) = setup(ApiKind::Anthropic, "claude-x");
        // 2 images: 2400px is within the 8000px limit.
        let mut r = clamp_request(request(few), &p, &m).await;
        assert_eq!(sizes(&mut r), [(2400, 600), (300, 300)]);
        // 21 images: everything must fit 2000px.
        let mut r = clamp_request(request(many.clone()), &p, &m).await;
        let s = sizes(&mut r);
        assert_eq!(s.len(), 21);
        assert_eq!(s[0], (2000, 500));
        assert!(s[1..].iter().all(|d| *d == (300, 300)));
        // A Claude id behind another API counts; other models are left alone.
        let (p, m) = setup(ApiKind::OpenAIChat, "anthropic/Claude-Opus");
        assert_eq!(sizes(&mut clamp_request(request(many.clone()), &p, &m).await)[0], (2000, 500));
        let (p, m) = setup(ApiKind::OpenAIChat, "gpt-x");
        assert_eq!(sizes(&mut clamp_request(request(many), &p, &m).await)[0], (2400, 600));
    }
}

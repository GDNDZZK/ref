//! Per-request image limits, enforced on the outgoing request like oh-my-pi does (stored
//! messages keep their images). Claude's are known up front
//! (<https://platform.claude.com/docs/en/build-with-claude/vision>): at most 8000px per side,
//! 10MB of base64 per image, and at most 2000px per side once a request has more than 20 images.
//! Stricter limits that a provider states in an error (Bedrock allows 5MB, proxies have their
//! own) are remembered per provider for the life of the server and the request is retried.
//! This catches what `images.auto_resize` does not cover: MCP images, the setting turned off,
//! and many-image requests.

use ref_llm::{ApiKind, ChatMessage, Model, Provider, Request, UserContent};
use ref_proto::ImageData;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const MANY_IMAGE_THRESHOLD: usize = 20;

/// Image limits for one provider. `u32::MAX` / `usize::MAX`: no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Caps {
    max_dim: u32,
    /// Applies to every image once a request has more than [`MANY_IMAGE_THRESHOLD`].
    many_dim: u32,
    /// Base64 bytes per image, already less the headroom.
    max_b64: usize,
}

const NO_CAPS: Caps = Caps { max_dim: u32::MAX, many_dim: u32::MAX, max_b64: usize::MAX };
/// 10MB of base64, less 10% headroom for the surrounding JSON.
const CLAUDE_CAPS: Caps = Caps { max_dim: 8000, many_dim: 2000, max_b64: 10 * 1024 * 1024 / 10 * 9 };

impl Caps {
    fn min(self, o: Caps) -> Caps {
        Caps { max_dim: self.max_dim.min(o.max_dim), many_dim: self.many_dim.min(o.many_dim), max_b64: self.max_b64.min(o.max_b64) }
    }
}

/// Limits learned from provider errors, by provider id.
static LEARNED: LazyLock<Mutex<HashMap<String, Caps>>> = LazyLock::new(Default::default);

fn learned(provider: &str) -> Caps {
    LEARNED.lock().unwrap_or_else(|e| e.into_inner()).get(provider).copied().unwrap_or(NO_CAPS)
}

/// The image limit stated in a provider error, as caps. Anthropic / Bedrock word them as
/// "image exceeds 5 MB maximum: 7009648 bytes > 5242880 bytes", "At least one of the image
/// dimensions exceed max allowed size: 8000 pixels" and "... max allowed size for many-image
/// requests: 2000 pixels".
fn caps_from_error(msg: &str) -> Option<Caps> {
    static BYTES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)image\b[^\n]*?exceeds?\b[^\n]*?\b\d+ bytes > (\d+) bytes").expect("regex"));
    static MB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)image\b[^\n]*?exceeds? (?:the )?(\d+(?:\.\d+)?) ?MB (?:maximum|limit)").expect("regex"));
    static DIM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)image dimensions? exceeds? (?:the )?max(?:imum)? allowed size( for many-image requests)?:? (\d+) ?(?:pixels|px)").expect("regex"));
    let bytes = BYTES.captures(msg).and_then(|c| c[1].parse::<usize>().ok()).or_else(|| MB.captures(msg).and_then(|c| c[1].parse::<f64>().ok()).map(|mb| (mb * 1024.0 * 1024.0) as usize));
    if let Some(b) = bytes.filter(|b| *b > 0) {
        return Some(Caps { max_b64: b / 10 * 9, ..NO_CAPS });
    }
    let c = DIM.captures(msg)?;
    let px = c[2].parse::<u32>().ok().filter(|p| *p > 0)?;
    Some(if c.get(1).is_some() { Caps { many_dim: px, ..NO_CAPS } } else { Caps { max_dim: px, ..NO_CAPS } })
}

/// Records the image limit stated in `error` for the provider. True if that is stricter than
/// what [`clamp_request`] applied, i.e. rebuilding the request and sending it again may succeed.
pub fn learn(provider: &Provider, model: &Model, error: &str) -> bool {
    let Some(stated) = caps_from_error(error) else { return false };
    let mut map = LEARNED.lock().unwrap_or_else(|e| e.into_inner());
    let known = map.get(&provider.id).copied().unwrap_or(NO_CAPS);
    let applied = known.min(base_caps(provider, model));
    if applied.min(stated) == applied {
        return false;
    }
    let new = known.min(stated);
    tracing::info!(provider = %provider.id, ?new, "provider stated a stricter image limit; retrying with smaller images");
    map.insert(provider.id.clone(), new);
    true
}

/// Limits known up front: Claude's for the Anthropic API or a model id naming Claude behind
/// another API (proxies forward to the same limits), none otherwise.
fn base_caps(provider: &Provider, model: &Model) -> Caps {
    if provider.api == ApiKind::Anthropic || model.id.to_ascii_lowercase().contains("claude") { CLAUDE_CAPS } else { NO_CAPS }
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

/// Downscales the images of `req` that break the limits known for this provider and model
/// (Claude's, plus any learned from errors). Without known limits: unchanged.
pub async fn clamp_request(mut req: Request, provider: &Provider, model: &Model) -> Request {
    let caps = learned(&provider.id).min(base_caps(provider, model));
    if !model.images || caps == NO_CAPS {
        return req;
    }
    let count = images_mut(&mut req.messages).count();
    if count == 0 {
        return req;
    }
    let max_dim = if count > MANY_IMAGE_THRESHOLD { caps.max_dim.min(caps.many_dim) } else { caps.max_dim };
    let max_b64 = caps.max_b64;
    let mut messages = std::mem::take(&mut req.messages);
    // `clamp_image_data` catches decoder panics, so the task always hands the messages back.
    req.messages = tokio::task::spawn_blocking(move || {
        for img in images_mut(&mut messages) {
            if let Some(small) = ref_tools::imgfit::clamp_image_data(img, max_dim, max_b64) {
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

    #[test]
    fn limits_are_read_from_provider_errors() {
        let c = caps_from_error("messages.0.content.1.image.source.base64: image exceeds 5 MB maximum: 7009648 bytes > 5242880 bytes").unwrap();
        assert_eq!(c, Caps { max_b64: 5242880 / 10 * 9, ..NO_CAPS });
        assert_eq!(caps_from_error("Image exceeds 3.5 MB limit").unwrap().max_b64, (3.5 * 1048576.0) as usize / 10 * 9);
        let c = caps_from_error("At least one of the image dimensions exceed max allowed size: 8000 pixels").unwrap();
        assert_eq!(c, Caps { max_dim: 8000, ..NO_CAPS });
        let c = caps_from_error("At least one of the image dimensions exceed max allowed size for many-image requests: 2000 pixels").unwrap();
        assert_eq!(c, Caps { many_dim: 2000, ..NO_CAPS });
        assert!(caps_from_error("Could not process image").is_none());
        assert!(caps_from_error("prompt is too long: 210000 tokens > 200000 maximum").is_none());
    }

    #[test]
    fn learning_only_reports_stricter_limits() {
        let (p, m) = setup(ApiKind::Anthropic, "claude-x");
        let p = Provider { id: "learn-unit".into(), ..p };
        // Claude's own 8000px limit is already applied: nothing to learn.
        assert!(!learn(&p, &m, "image dimensions exceed max allowed size: 8000 pixels"));
        // Bedrock's 5MB is stricter than the 10MB assumed.
        let e = "image exceeds 5 MB maximum: 7009648 bytes > 5242880 bytes";
        assert!(learn(&p, &m, e));
        assert!(!learn(&p, &m, e), "already known");
        assert_eq!(learned("learn-unit").max_b64, 5242880 / 10 * 9);
        assert!(!learn(&p, &m, "bad request"));
    }
}

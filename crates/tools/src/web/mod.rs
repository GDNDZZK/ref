//! `webfetch`: fetch a URL and return it as markdown (default), text or HTML.

mod feed;
mod fetch;
mod html;
mod sites;
#[cfg(test)]
mod tests;

use crate::util;
use crate::{ToolCtx, ToolOutput, ToolSpec};
use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use ref_proto::ImageData;
use reqwest::Url;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Text,
    Html,
}

pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
pub const MAX_TIMEOUT_SECS: u64 = 120;
/// Images larger than this are not attached (model APIs reject them).
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Target size of a fetched image after downscaling (the value oh-my-pi uses).
const FETCH_IMAGE_BYTES: usize = 300 * 1024;
/// Body shown with an HTTP error status.
const ERROR_BODY_CHARS: usize = 4000;

pub fn spec() -> ToolSpec {
    ToolSpec {
        name: "webfetch",
        description: "Fetch a URL (http/https) and return its content. Read-only; no JavaScript is run.\n\
- HTML becomes markdown (default), plain text (`format: \"text\"`) or stays HTML (`format: \"html\"`). For markdown and text, the main content is extracted when the page reads like an article (the header says so); pass `full_page: true` for everything (navigation, sidebars, comments).\n\
- Markdown / plain text / JSON (pretty-printed) / XML are returned as text, RSS and Atom feeds as a list of entries, images (png, jpeg, gif, webp up to 5MB) as an image you can see. Other binary files (PDFs included) are not shown: download them with bash (`curl -L -o`).\n\
- GitHub file pages return the raw file; GitHub issues and pull requests return the conversation (from the API).\n\
- Output starts with a header (URL, Content-Type, Title, notes) and `---`. Content over 50KB or 2000 lines is cut; the full content is saved to a file you can `read` with offset/limit or `grep`.\n\
- `timeout` in seconds (default 30, max 120). Responses over 20MB are refused. HTTP errors return the status and the start of the error page."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "URL to fetch (http or https; `https://` is assumed when missing)."},
                "format": {"type": "string", "enum": ["markdown", "text", "html"], "description": "Output format for HTML pages (default markdown)."},
                "full_page": {"type": "boolean", "description": "Convert the whole page instead of only its main content (default false)."},
                "timeout": {"type": "integer", "minimum": 1, "maximum": 120, "description": "Timeout in seconds (default 30, max 120)."}
            },
            "required": ["url"]
        }),
    }
}

fn parse_url(raw: &str) -> Result<Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("`url` must not be empty");
    }
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{}", raw.trim_start_matches('/')) };
    let url = Url::parse(&with_scheme).map_err(|e| anyhow!("Invalid URL {raw}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("Only http and https URLs can be fetched (got {}:).", url.scheme());
    }
    if url.host_str().is_none_or(str::is_empty) {
        bail!("Invalid URL {raw}: no host");
    }
    Ok(url)
}

/// Result being assembled.
struct Out {
    requested: Url,
    url: Url,
    status: Option<reqwest::StatusCode>,
    content_type: String,
    title: Option<String>,
    notes: Vec<String>,
    body: String,
    images: Vec<ImageData>,
    error: bool,
    /// Size of the response body as received.
    raw_bytes: usize,
    /// Extension of the file the full content is saved to when cut.
    ext: &'static str,
}

pub async fn run(input: Value, ctx: &ToolCtx) -> Result<ToolOutput> {
    let url = parse_url(&util::req_str(&input, "url")?)?;
    let format = match util::opt_str(&input, "format")?.as_deref().map(str::trim) {
        None | Some("") | Some("markdown") | Some("md") => Format::Markdown,
        Some("text") | Some("txt") => Format::Text,
        Some("html") => Format::Html,
        Some(f) => bail!("`format` must be markdown, text or html (got {f:?})"),
    };
    let full_page = util::opt_bool(&input, "full_page")?.unwrap_or(false);
    let timeout = util::opt_u64(&input, "timeout")?.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let started = std::time::Instant::now();
    let mut notes = Vec::new();
    let mut target = url.clone();
    if format != Format::Html {
        match sites::special(&url) {
            Some(sites::Special::Rewrite { url: u, note }) => {
                target = u;
                notes.push(note);
            }
            Some(sites::Special::GithubIssue { owner, repo, number }) => match github_issue(&owner, &repo, number, deadline, ctx).await {
                Ok((title, body)) => {
                    let out = Out { requested: url.clone(), url, status: None, content_type: "application/json (GitHub API)".into(), title: Some(title), notes: vec!["Read from the GitHub API.".into()], body, images: vec![], error: false, raw_bytes: 0, ext: "md" };
                    return Ok(finish(out, started));
                }
                Err(e) => notes.push(format!("GitHub API not usable ({e:#}); fetched the web page instead.")),
            },
            None => {}
        }
    }
    let accept = match format {
        Format::Markdown => "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1",
        Format::Text => "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1",
        Format::Html => "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, */*;q=0.1",
    };
    let f = fetch::get(&target, accept, &HeaderMap::new(), deadline, &ctx.cancel).await?;
    let mut out = Out {
        requested: target,
        url: f.url.clone(),
        status: Some(f.status),
        content_type: f.content_type.clone(),
        title: None,
        notes,
        body: String::new(),
        images: vec![],
        error: !f.status.is_success(),
        raw_bytes: f.body.len(),
        ext: "txt",
    };
    if out.error && (f.status.as_u16() == 403 || f.status.as_u16() == 503) && String::from_utf8_lossy(&f.body[..f.body.len().min(64 * 1024)]).contains("challenge-platform") {
        out.notes.push("Blocked by a Cloudflare bot check that needs a browser running JavaScript; the page cannot be fetched this way. Look for an API, a raw source or a mirror.".into());
    }
    convert(&mut out, f, format, full_page, deadline, ctx).await?;
    Ok(finish(out, started))
}

enum Kind {
    Html,
    Text,
    Json,
    Image(&'static str),
    Binary,
}

fn classify(mime: &str, body: &[u8]) -> Kind {
    let start = String::from_utf8_lossy(&body[..body.len().min(512)]).trim_start().to_ascii_lowercase();
    let sniff_html = start.starts_with("<!doctype html") || start.starts_with("<html");
    if let Some(m) = util::image_mime(body) {
        return Kind::Image(m);
    }
    if mime == "application/pdf" || body.starts_with(b"%PDF-") {
        return Kind::Binary;
    }
    if mime == "text/html" || mime == "application/xhtml+xml" || ((mime.is_empty() || mime == "application/octet-stream") && sniff_html) {
        return Kind::Html;
    }
    if mime == "application/json" || mime == "text/json" || mime.ends_with("+json") {
        return Kind::Json;
    }
    let texty = mime.starts_with("text/")
        || mime.contains("xml")
        || mime.contains("javascript")
        || mime.contains("ecmascript")
        || ["yaml", "toml", "csv", "x-sh", "x-python", "x-rust", "markdown", "graphql", "x-www-form-urlencoded", "sql"].iter().any(|t| mime.contains(t));
    if texty || (!body.is_empty() && !util::looks_binary(body)) {
        return Kind::Text;
    }
    Kind::Binary
}

async fn convert(out: &mut Out, f: fetch::Fetched, format: Format, full_page: bool, deadline: Instant, ctx: &ToolCtx) -> Result<()> {
    let mime = f.mime();
    let host = f.url.host_str().map(str::to_string);
    match classify(&mime, &f.body) {
        Kind::Html => {
            let text = fetch::decode(&f.body, &f.content_type, true, host.as_deref());
            if format == Format::Html {
                out.body = text;
                out.ext = "html";
                return Ok(());
            }
            if !full_page
                && !out.error
                && let Some(alt) = html::markdown_alternate(&text, &f.url)
                && let Ok(m) = fetch::get(&alt, "text/markdown, text/plain;q=0.9", &HeaderMap::new(), deadline, &ctx.cancel).await
                && m.status.is_success()
                && matches!(m.mime().as_str(), "text/markdown" | "text/x-markdown" | "text/plain")
            {
                out.body = fetch::decode(&m.body, &m.content_type, false, host.as_deref());
                out.notes.push(format!("The page links a markdown version; this is it: {}", m.url));
                out.ext = "md";
                return Ok(());
            }
            let url = f.url.clone();
            let page = tokio::task::spawn_blocking(move || html::convert(&text, &url, format, full_page)).await.map_err(|_| anyhow!("HTML conversion failed"))?;
            out.title = page.title;
            out.notes.extend(page.note);
            out.body = page.body;
            out.ext = if format == Format::Markdown { "md" } else { "txt" };
            if out.body.trim().is_empty() && !out.error {
                out.notes.push("The page has no text without JavaScript (probably rendered in the browser). Try format: \"html\" to see the raw page, or look for an API / raw source.".into());
            }
        }
        Kind::Json => {
            let text = fetch::decode(&f.body, &f.content_type, false, host.as_deref());
            out.body = match serde_json::from_str::<Value>(&text) {
                Ok(v) if format != Format::Html => serde_json::to_string_pretty(&v).unwrap_or(text),
                _ => text,
            };
            out.ext = "json";
        }
        Kind::Text => {
            let text = fetch::decode(&f.body, &f.content_type, mime.contains("xml"), host.as_deref());
            if format != Format::Html && feed::is_feed(&mime, &text)
                && let Some((title, md)) = feed::to_markdown(&text) {
                    out.title = title;
                    out.notes.push("RSS / Atom feed shown as a list of entries; format: \"html\" returns the raw XML.".into());
                    out.body = md;
                    out.ext = "md";
                    return Ok(());
                }
            out.ext = if mime.contains("markdown") { "md" } else { "txt" };
            out.body = text;
        }
        Kind::Image(m) => {
            let size = util::human_bytes(f.body.len() as u64);
            if f.body.len() > MAX_IMAGE_BYTES {
                out.body = format!("Image ({m}, {size}) is larger than {} and was not attached. Download it with bash (curl -L -o) to inspect it.", util::human_bytes(MAX_IMAGE_BYTES as u64));
                return Ok(());
            }
            let mut sent = (m, f.body);
            out.body = format!("Image ({m}, {size}), attached.");
            if ctx.resize_images {
                let bytes = sent.1;
                let limits = crate::imgfit::Limits { max_bytes: FETCH_IMAGE_BYTES, ..Default::default() };
                let (bytes, fitted) = tokio::task::spawn_blocking(move || {
                    let r = crate::imgfit::fit(&bytes, &limits);
                    (bytes, r)
                })
                .await
                .map_err(|_| anyhow!("image conversion failed"))?;
                match fitted {
                    Err(e) => {
                        out.body = format!("The content looks like {m} ({size}) but could not be decoded ({e}); not attached.");
                        return Ok(());
                    }
                    Ok(fitted) => {
                        let note = fitted.note();
                        sent = match fitted.data {
                            Some(d) => {
                                out.body = format!("Image ({m}, {size}), attached as {} {}x{} ({}).", fitted.mime, fitted.size.0, fitted.size.1, util::human_bytes(d.len() as u64));
                                (fitted.mime, d)
                            }
                            None => (m, bytes),
                        };
                        if let Some(n) = note {
                            out.body.push('\n');
                            out.body.push_str(&n);
                        }
                    }
                }
            }
            out.images.push(ImageData::new(sent.0, base64::engine::general_purpose::STANDARD.encode(&sent.1)));
        }
        Kind::Binary => {
            let what = if mime.is_empty() { "unknown type".to_string() } else { mime.clone() };
            out.body = format!("Binary content ({what}, {}) is not shown. Download it with bash, e.g. curl -L -o FILE '{}'.", util::human_bytes(f.body.len() as u64), f.url);
        }
    }
    Ok(())
}

async fn github_issue(owner: &str, repo: &str, number: u64, deadline: Instant, ctx: &ToolCtx) -> Result<(String, String)> {
    let mut h = HeaderMap::new();
    h.insert("X-GitHub-Api-Version", HeaderValue::from_static("2022-11-28"));
    if let Some(t) = sites::github_token().and_then(|t| HeaderValue::from_str(&format!("Bearer {}", t.trim())).ok()) {
        h.insert(AUTHORIZATION, t);
    }
    let get = |u: String| {
        let h = h.clone();
        async move {
            let url = Url::parse(&u)?;
            let f = fetch::get(&url, "application/vnd.github+json", &h, deadline, &ctx.cancel).await?;
            if !f.status.is_success() {
                bail!("HTTP {}", f.status);
            }
            serde_json::from_slice::<Value>(&f.body).map_err(|e| anyhow!("bad JSON: {e}"))
        }
    };
    let issue = get(format!("https://api.github.com/repos/{owner}/{repo}/issues/{number}")).await?;
    let comments = match issue.get("comments").and_then(Value::as_u64) {
        Some(n) if n > 0 => get(format!("https://api.github.com/repos/{owner}/{repo}/issues/{number}/comments?per_page=100")).await?.as_array().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    Ok(sites::github_issue_markdown(&issue, &comments))
}

/// Header, body (cut to the inline budget, the full text saved to a file) and metadata.
fn finish(mut out: Out, started: std::time::Instant) -> ToolOutput {
    let total_bytes = out.body.len();
    let total_lines = out.body.lines().count();
    if out.error && out.body.chars().count() > ERROR_BODY_CHARS {
        let (cut, _) = util::truncate_chars(&out.body, ERROR_BODY_CHARS);
        out.body = format!("{cut}\n[… error page cut …]");
    }
    let mut saved = None;
    let cut = !out.error && (total_bytes > crate::bash::MAX_BYTES || total_lines > crate::bash::MAX_LINES);
    if cut {
        let path = util::spill_path(out.ext);
        let ok = path.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&path, &out.body));
        match ok {
            Ok(_) => saved = Some(path),
            Err(e) => tracing::warn!("webfetch: cannot save full content to {}: {e}", path.display()),
        }
        let mut end = util::floor_char_boundary(&out.body, crate::bash::MAX_BYTES);
        if let Some((i, _)) = out.body[..end].match_indices('\n').nth(crate::bash::MAX_LINES - 1) {
            end = i;
        } else if let Some(i) = out.body[..end].rfind('\n') {
            end = i;
        }
        out.body.truncate(end);
    }
    let mut text = String::new();
    if out.url == out.requested {
        text.push_str(&format!("URL: {}\n", out.url));
    } else {
        text.push_str(&format!("URL: {} (redirected from {})\n", out.url, out.requested));
    }
    if let Some(s) = out.status.filter(|s| !s.is_success()) {
        text.push_str(&format!("Status: {s}\n"));
    }
    text.push_str(&format!("Content-Type: {}\n", if out.content_type.is_empty() { "(none)" } else { &out.content_type }));
    if let Some(t) = &out.title {
        text.push_str(&format!("Title: {t}\n"));
    }
    for n in &out.notes {
        text.push_str(&format!("Note: {n}\n"));
    }
    text.push_str("---\n");
    text.push_str(if out.body.trim().is_empty() { "(no content)" } else { out.body.trim_end() });
    if cut {
        let shown = out.body.lines().count();
        text.push_str(&format!("\n\n[Content cut: showing {shown} of {total_lines} lines ({} of {}).", util::human_bytes(out.body.len() as u64), util::human_bytes(total_bytes as u64)));
        match &saved {
            Some(p) => text.push_str(&format!(" Full content saved to {}; use `read` with offset/limit or `grep` on it.]", p.display())),
            None => text.push(']'),
        }
    }
    ToolOutput {
        text,
        images: out.images,
        meta: Some(json!({
            "url": out.url.as_str(),
            "status": out.status.map(|s| s.as_u16()),
            "content_type": out.content_type,
            "title": out.title,
            "bytes": out.raw_bytes,
            "truncated": cut,
            "output_path": saved.map(|p| p.to_string_lossy().into_owned()),
            "duration_ms": started.elapsed().as_millis() as u64,
        })),
        is_error: out.error,
    }
}

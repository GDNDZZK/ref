//! HTTP for `webfetch`: clients (environment proxies; direct for local hosts), browser-like
//! request headers, size-capped body reading and charset decoding.

use anyhow::{Result, anyhow, bail};
use encoding_rs::Encoding;
use regex::Regex;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, USER_AGENT};
use reqwest::{Client, StatusCode, Url};
use std::net::IpAddr;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Largest response body read.
pub const MAX_BODY: usize = 20 * 1024 * 1024;

pub struct Fetched {
    /// After redirects.
    pub url: Url,
    pub status: StatusCode,
    /// `Content-Type` header value ("" if missing).
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Fetched {
    /// Media type without parameters, lower case.
    pub fn mime(&self) -> String {
        self.content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase()
    }
}

/// A browser user agent: some sites serve bots an error page or a challenge.
fn browser_ua() -> &'static str {
    if cfg!(windows) {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"
    } else if cfg!(target_os = "macos") {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"
    } else {
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36"
    }
}

/// Honest user agent, used after a Cloudflare bot challenge (they often let declared bots through).
fn honest_ua() -> String {
    format!("ref/{} (coding agent; +https://github.com/GDNDZZK/ref)", env!("CARGO_PKG_VERSION"))
}

/// `Accept-Language` from the locale (`LC_ALL` / `LC_MESSAGES` / `LANG`), English as fallback.
pub fn accept_language() -> String {
    let loc = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty()).unwrap_or_default();
    accept_language_for(&loc)
}

/// `zh_CN.UTF-8` → `zh-CN,zh;q=0.9,en;q=0.8`.
pub fn accept_language_for(loc: &str) -> String {
    let tag = loc.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let lang = tag.split('-').next().unwrap_or("").to_ascii_lowercase();
    if lang.len() < 2 || lang == "c" || lang == "posix" || lang == "en" {
        return "en-US,en;q=0.9".into();
    }
    if tag.contains('-') {
        format!("{tag},{lang};q=0.9,en;q=0.8")
    } else {
        format!("{lang},en;q=0.8")
    }
}

fn build(direct: bool) -> Result<Client, String> {
    let mut b = Client::builder().redirect(reqwest::redirect::Policy::limited(10)).connect_timeout(Duration::from_secs(15)).pool_idle_timeout(Duration::from_secs(60));
    if direct {
        b = b.no_proxy();
    }
    b.build().map_err(|e| format!("cannot create HTTP client: {e}"))
}

fn client(direct: bool) -> Result<&'static Client> {
    static ENV: OnceLock<Result<Client, String>> = OnceLock::new();
    static DIRECT: OnceLock<Result<Client, String>> = OnceLock::new();
    let c = if direct { DIRECT.get_or_init(|| build(true)) } else { ENV.get_or_init(|| build(false).or_else(|e| {
        tracing::warn!("webfetch: {e}; proxies from the environment are not used");
        build(true)
    })) };
    c.as_ref().map_err(|e| anyhow!("{e}"))
}

/// Local and private hosts are fetched without a proxy, as are hosts matching a wildcard
/// `NO_PROXY` entry (`127.*`, `*.corp`): reqwest only understands suffixes and CIDRs.
pub fn bypass_proxy(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        let local = match ip {
            IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified(),
            IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.is_unicast_link_local() || v.is_unspecified() || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback() || m.is_private()),
        };
        if local {
            return true;
        }
    }
    let list = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")).unwrap_or_default();
    list.split(',').map(str::trim).filter(|e| e.contains('*')).any(|e| glob(&e.to_ascii_lowercase(), &h))
}

/// `*` matches any run of characters.
fn glob(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s.ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

/// GET `url`. Follows redirects; gives up at `deadline` or on cancel. A Cloudflare bot
/// challenge (403 + `cf-mitigated: challenge`) is retried once with an honest user agent.
pub async fn get(url: &Url, accept: &str, extra: &HeaderMap, deadline: Instant, cancel: &CancellationToken) -> Result<Fetched> {
    let direct = url.host_str().is_some_and(bypass_proxy);
    let c = client(direct)?;
    let send = |ua: String| {
        let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        c.get(url.clone())
            .header(USER_AGENT, ua)
            .header(ACCEPT, accept)
            .header(ACCEPT_LANGUAGE, accept_language())
            .headers(extra.clone())
            .timeout(left)
            .send()
    };
    let mut resp = cancellable(send(browser_ua().to_string()), cancel).await?.map_err(|e| request_error(e, url))?;
    if resp.status() == StatusCode::FORBIDDEN && resp.headers().get("cf-mitigated").is_some_and(|v| v == "challenge") {
        resp = cancellable(send(honest_ua()), cancel).await?.map_err(|e| request_error(e, url))?;
    }
    let status = resp.status();
    let final_url = resp.url().clone();
    let content_type = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let declared = resp.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_BODY as u64) {
        bail!("Response too large: {} (limit {}). Download it with bash (curl -o) instead.", crate::util::human_bytes(declared.unwrap_or(0)), crate::util::human_bytes(MAX_BODY as u64));
    }
    let mut body = Vec::with_capacity(declared.unwrap_or(0).min(MAX_BODY as u64) as usize);
    while let Some(chunk) = cancellable(resp.chunk(), cancel).await?.map_err(|e| request_error(e, url))? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            bail!("Response too large: over {}. Download it with bash (curl -o) instead.", crate::util::human_bytes(MAX_BODY as u64));
        }
    }
    Ok(Fetched { url: final_url, status, content_type, body })
}

async fn cancellable<T>(f: impl Future<Output = T>, cancel: &CancellationToken) -> Result<T> {
    tokio::select! {
        r = f => Ok(r),
        _ = cancel.cancelled() => Err(anyhow!("Request cancelled")),
    }
}

fn request_error(e: reqwest::Error, url: &Url) -> anyhow::Error {
    if e.is_timeout() {
        return anyhow!("Request timed out: {url}");
    }
    // The innermost cause says what went wrong (DNS, refused, TLS, …).
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        let t = s.to_string();
        if !msg.contains(&t) {
            msg.push_str(": ");
            msg.push_str(&t);
        }
        src = s.source();
    }
    anyhow!("{msg}")
}

/// Decodes text: BOM, then the `Content-Type` charset, then (HTML / XML) a `<meta charset>` or
/// XML declaration, then UTF-8 if valid, else a guess from the bytes (and the host's TLD).
pub fn decode(bytes: &[u8], content_type: &str, markup: bool, host: Option<&str>) -> String {
    if let Some((enc, n)) = Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[n..]).0.into_owned();
    }
    let declared = charset_param(content_type).or_else(|| markup.then(|| meta_charset(bytes)).flatten());
    if let Some(enc) = declared.and_then(|l| Encoding::for_label(l.trim().as_bytes())) {
        // A page cannot really be UTF-16 if its declaration was readable as ASCII.
        let enc = if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE { encoding_rs::UTF_8 } else { enc };
        return enc.decode_without_bom_handling(bytes).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(bytes, true);
    let tld = host.and_then(|h| h.rsplit('.').next()).map(str::as_bytes);
    det.guess(tld, chardetng::Utf8Detection::Allow).decode_without_bom_handling(bytes).0.into_owned()
}

fn charset_param(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("charset").then(|| v.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn meta_charset(bytes: &[u8]) -> Option<String> {
    static META: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<meta[^>]*?charset\s*=\s*["']?\s*([A-Za-z0-9_:.\-]+)"#).unwrap());
    static XML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<\?xml[^>]*encoding\s*=\s*["']([A-Za-z0-9_:.\-]+)"#).unwrap());
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    XML.captures(&head).or_else(|| META.captures(&head)).map(|c| c[1].to_string())
}

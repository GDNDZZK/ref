use super::*;
use crate::util::test_support::{ctx, temp_dir};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Req {
    path: String,
    headers: HashMap<String, String>,
}

#[derive(Default)]
struct Resp {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    delay_ms: u64,
}

fn ok(ct: &str, body: impl Into<Vec<u8>>) -> Resp {
    Resp { status: 200, headers: vec![("Content-Type", ct.to_string())], body: body.into(), delay_ms: 0 }
}

type Handler = Arc<dyn Fn(&Req) -> Resp + Send + Sync>;

/// Minimal HTTP/1.1 server on 127.0.0.1 (one request per connection). Returns the base URL.
async fn serve(h: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> String {
    let h: Handler = Arc::new(h);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            let h = h.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let mut lines = head.split("\r\n");
                let path = lines.next().unwrap_or("").split(' ').nth(1).unwrap_or("/").to_string();
                let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
                let r = h(&Req { path, headers });
                if r.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(r.delay_ms)).await;
                }
                let reason = match r.status {
                    200 => "OK",
                    302 => "Found",
                    403 => "Forbidden",
                    404 => "Not Found",
                    _ => "Status",
                };
                let mut out = format!("HTTP/1.1 {} {reason}\r\nConnection: close\r\n", r.status);
                if !r.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length")) {
                    out.push_str(&format!("Content-Length: {}\r\n", r.body.len()));
                }
                for (k, v) in &r.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = s.write_all(out.as_bytes()).await;
                let _ = s.write_all(&r.body).await;
                let _ = s.shutdown().await;
            });
        }
    });
    base
}

async fn fetch(v: Value) -> ToolOutput {
    let d = temp_dir("webfetch");
    crate::execute("webfetch", v, &ctx(&d)).await
}

/// Body after the `---` header line.
fn body(o: &ToolOutput) -> &str {
    o.text.split_once("\n---\n").map_or("", |(_, b)| b)
}

const ARTICLE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Guide to things</title></head><body>
<nav><a href="/">Home</a> <a href="/about">About us</a> NAVTEXT</nav>
<main><article><h1>Guide to things</h1>
<p>This guide explains things in depth. It has enough text to count as an article for the extractor, so here is more of it. And more prose follows, explaining each detail with care and patience for the reader.</p>
<p>See <a href="/docs/page?a=1&amp;b=2">the docs</a> or write to <a href="/cdn-cgi/l/email-protection#5a333b371a3f223b372a363f74393537" class="__cf_email__" data-cfemail="5a333b371a3f223b372a363f74393537">[email&#160;protected]</a>.</p>
<p>Another paragraph with plenty of words so the readability scoring is satisfied. Another paragraph with plenty of words so the readability scoring is satisfied.</p>
<div class="highlight highlight-source-python"><pre>def f():
    return 1</pre></div>
<p><img src="data:image/gif;base64,R0lGOD" data-src="/img/pic.png" alt="Picture"></p>
<details><summary>Spoiler</summary>Hidden detail</details>
<div style="display: none">INVISIBLE</div><div hidden>ALSOHIDDEN</div>
<p>Final paragraph with more words to make the article long enough to be considered readable content by the scoring rules.</p>
</article></main>
<footer>FOOTERTEXT</footer><script>var SCRIPTTEXT = 1;</script>
</body></html>"#;

#[tokio::test]
async fn html_article_full_page_text_and_html() {
    let base = serve(|_| ok("text/html", ARTICLE)).await;
    let o = fetch(json!({"url": format!("{base}/guide")})).await;
    assert!(!o.is_error, "{}", o.text);
    assert!(o.text.starts_with(&format!("URL: {base}/guide\nContent-Type: text/html\nTitle: Guide to things\nNote: Main content only")), "{}", o.text);
    let b = body(&o);
    assert!(b.contains(&format!("[the docs]({base}/docs/page?a=1&b=2)")), "{b}");
    assert!(b.contains("[iam@example.com](mailto:iam@example.com)"), "{b}");
    assert!(b.contains("```python\ndef f():\n    return 1\n```"), "{b}");
    assert!(b.contains(&format!("![Picture]({base}/img/pic.png)")), "{b}");
    assert!(b.contains("**Spoiler**") && b.contains("Hidden detail"), "{b}");
    for junk in ["NAVTEXT", "FOOTERTEXT", "SCRIPTTEXT", "INVISIBLE", "ALSOHIDDEN", "data:image"] {
        assert!(!b.contains(junk), "{junk} in {b}");
    }
    let m = o.meta.as_ref().unwrap();
    assert_eq!(m["status"], 200);
    assert_eq!(m["title"], "Guide to things");
    // Whole page: navigation and footer too, still without scripts / hidden nodes.
    let o = fetch(json!({"url": format!("{base}/guide"), "full_page": true})).await;
    let b = body(&o);
    assert!(b.contains("NAVTEXT") && b.contains("FOOTERTEXT") && b.contains("# Guide to things"), "{b}");
    assert!(!b.contains("SCRIPTTEXT") && !b.contains("INVISIBLE") && !o.text.contains("Note: Main content"));
    // Text: no markdown syntax.
    let o = fetch(json!({"url": format!("{base}/guide"), "format": "text"})).await;
    let b = body(&o);
    assert!(b.contains("This guide explains things") && !b.contains("](") && !b.contains("```"), "{b}");
    // HTML: the page as received.
    let o = fetch(json!({"url": format!("{base}/guide"), "format": "html"})).await;
    assert_eq!(body(&o), ARTICLE.trim_end());
    // Bad arguments.
    assert!(fetch(json!({"url": format!("{base}/guide"), "format": "pdf"})).await.is_error);
    assert!(fetch(json!({"url": "ftp://example.com/x"})).await.text.contains("Only http and https"));
    assert!(fetch(json!({"url": "  "})).await.is_error);
}

#[tokio::test]
async fn non_article_pages_convert_whole_and_js_pages_say_so() {
    let page = "<html><head><title>Portal</title></head><body><ul><li><a href='/a'>Alpha news</a></li><li><a href='/b'>Beta news</a></li><li><a href='/c'>Gamma</a></li></ul></body></html>";
    let spa = "<html><head><title>App</title></head><body><div id='root'></div><script src='/app.js'></script></body></html>";
    let base = serve(move |r| if r.path == "/spa" { ok("text/html", spa) } else { ok("text/html; charset=utf-8", page) }).await;
    let o = fetch(json!({"url": format!("{base}/")})).await;
    assert!(!o.text.contains("Note: Main content"), "{}", o.text);
    assert!(body(&o).contains(&format!("- [Alpha news]({base}/a)")), "{}", o.text);
    let o = fetch(json!({"url": format!("{base}/spa")})).await;
    assert!(o.text.contains("no text without JavaScript") && body(&o) == "(no content)", "{}", o.text);
}

#[tokio::test]
async fn charsets_from_header_meta_and_bom() {
    let (gbk, _, _) = encoding_rs::GBK.encode("<html><head><meta charset=\"gbk\"><title>中文标题</title></head><body><p>你好，世界</p></body></html>");
    let gbk = gbk.into_owned();
    let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("日本語のテキスト");
    let sjis = sjis.into_owned();
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice("BOM text é".as_bytes());
    let base = serve(move |r| match r.path.as_str() {
        "/meta" => ok("text/html", gbk.clone()),
        "/header" => ok("text/plain; charset=Shift_JIS", sjis.clone()),
        _ => ok("text/plain; charset=iso-8859-1", bom.clone()),
    })
    .await;
    let o = fetch(json!({"url": format!("{base}/meta"), "full_page": true})).await;
    assert!(o.text.contains("Title: 中文标题") && body(&o).contains("你好，世界"), "{}", o.text);
    assert_eq!(body(&fetch(json!({"url": format!("{base}/header")})).await), "日本語のテキスト");
    // A BOM wins over the declared charset.
    assert_eq!(body(&fetch(json!({"url": format!("{base}/bom")})).await), "BOM text é");
}

#[tokio::test]
async fn markdown_by_negotiation_or_alternate_link() {
    let base = serve(|r| match r.path.as_str() {
        "/neg" if r.headers.get("accept").is_some_and(|a| a.starts_with("text/markdown")) => ok("text/markdown; charset=utf-8", "# Served as markdown\n"),
        "/neg" => ok("text/html", "<html><body>html version</body></html>"),
        "/alt" => ok("text/html", "<html><head><link rel=\"alternate\" type=\"text/markdown\" href=\"/alt.md\"></head><body><p>html body</p></body></html>"),
        "/alt.md" => ok("text/markdown", "# Alternate markdown\n"),
        _ => Resp { status: 404, ..Default::default() },
    })
    .await;
    let o = fetch(json!({"url": format!("{base}/neg")})).await;
    assert!(o.text.contains("Content-Type: text/markdown") && body(&o) == "# Served as markdown", "{}", o.text);
    // format html asks for HTML first.
    assert!(body(&fetch(json!({"url": format!("{base}/neg"), "format": "html"})).await).contains("html version"));
    let o = fetch(json!({"url": format!("{base}/alt")})).await;
    assert_eq!(body(&o), "# Alternate markdown");
    assert!(o.text.contains(&format!("links a markdown version; this is it: {base}/alt.md")), "{}", o.text);
    // full_page converts the HTML itself.
    assert!(body(&fetch(json!({"url": format!("{base}/alt"), "full_page": true})).await).contains("html body"));
}

const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Feed &amp; Co</title><link>https://ex.com/</link><description>News</description>
<item><title>First post</title><link>https://ex.com/1</link><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate><description>&lt;p&gt;Hello &lt;b&gt;world&lt;/b&gt;&lt;/p&gt;</description></item>
<item><title><![CDATA[Second <post>]]></title><link>https://ex.com/2</link></item></channel></rss>"#;
const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?><feed xmlns="http://www.w3.org/2005/Atom"><title>Atom feed</title><link href="https://ex.org/"/>
<entry><title>Entry one</title><link rel="alternate" href="https://ex.org/e1"/><published>2024-02-02T00:00:00Z</published><author><name>Ann</name></author><summary>Short summary</summary></entry></feed>"#;

#[tokio::test]
async fn json_feeds_images_svg_and_binary() {
    let png = crate::imgfit::test_png(2000, 500, true);
    let base = serve(move |r| match r.path.as_str() {
        "/j" => ok("application/json", r#"{"a":[1,2],"b":{"c":"d"}}"#),
        "/rss" => ok("application/rss+xml", RSS),
        "/atom" => ok("application/xml", ATOM),
        "/img" => ok("image/png", png.clone()),
        "/fake" => ok("image/png", b"\x89PNG\r\n\x1a\n0000IHDR fake png".to_vec()),
        "/svg" => ok("image/svg+xml", "<svg xmlns='http://www.w3.org/2000/svg'><circle r='1'/></svg>"),
        _ => ok("application/octet-stream", vec![0u8, 1, 2, 3, 0, 0, 255, 254]),
    })
    .await;
    assert_eq!(body(&fetch(json!({"url": format!("{base}/j")})).await), "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {\n    \"c\": \"d\"\n  }\n}");
    assert_eq!(body(&fetch(json!({"url": format!("{base}/j"), "format": "html"})).await), r#"{"a":[1,2],"b":{"c":"d"}}"#);
    let o = fetch(json!({"url": format!("{base}/rss")})).await;
    let b = body(&o);
    assert!(o.text.contains("Title: Feed & Co"), "{}", o.text);
    assert!(b.contains("2 entries") && b.contains("## [First post](https://ex.com/1)\nMon, 01 Jan 2024 00:00:00 GMT\n\nHello world"), "{b}");
    assert!(b.contains("## [Second <post>](https://ex.com/2)"), "{b}");
    assert!(body(&fetch(json!({"url": format!("{base}/rss"), "format": "html"})).await).starts_with("<?xml"));
    let b = body(&fetch(json!({"url": format!("{base}/atom")})).await).to_string();
    assert!(b.contains("# Atom feed") && b.contains("## [Entry one](https://ex.org/e1)\n2024-02-02T00:00:00Z · Ann\n\nShort summary"), "{b}");
    let o = fetch(json!({"url": format!("{base}/img")})).await;
    assert_eq!(o.images.len(), 1);
    assert_eq!(o.images[0].mime, "image/jpeg");
    assert!(body(&o).starts_with("Image (image/png, ") && body(&o).contains("attached as image/jpeg 1568x392 (") && body(&o).contains("[Image: original 2000x500, displayed at 1568x392."), "{}", o.text);
    let o = fetch(json!({"url": format!("{base}/fake")})).await;
    assert!(o.images.is_empty() && !o.is_error && body(&o).starts_with("The content looks like image/png (25 B) but could not be decoded"), "{}", o.text);
    assert!(body(&fetch(json!({"url": format!("{base}/svg")})).await).starts_with("<svg"));
    let o = fetch(json!({"url": format!("{base}/bin")})).await;
    assert!(!o.is_error && body(&o).starts_with("Binary content (application/octet-stream, 8 B) is not shown"), "{}", o.text);
}

#[tokio::test]
async fn pdf_is_binary() {
    let base = serve(|_| ok("application/pdf", "%PDF-1.4 plain ascii")).await;
    let o = fetch(json!({"url": format!("{base}/a.pdf")})).await;
    assert!(!o.is_error && body(&o).starts_with("Binary content (application/pdf"), "{}", o.text);
}

#[tokio::test]
async fn errors_redirects_challenge_gzip_and_limits() {
    use std::io::Write as _;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(b"compressed body").unwrap();
    let gz = gz.finish().unwrap();
    let base = serve(move |r| {
        let ua = r.headers.get("user-agent").cloned().unwrap_or_default();
        match r.path.as_str() {
            "/missing" => Resp { status: 404, headers: vec![("Content-Type", "text/html".into())], body: format!("<html><body><p>No such page</p>{}</body></html>", "x".repeat(9000)).into(), delay_ms: 0 },
            "/old" => Resp { status: 302, headers: vec![("Location", "/new".into())], ..Default::default() },
            "/new" => ok("text/plain", "moved here"),
            "/cf" if !ua.starts_with("ref/") => Resp { status: 403, headers: vec![("cf-mitigated", "challenge".into()), ("Content-Type", "text/html".into())], body: b"<html>Just a moment</html>".to_vec(), delay_ms: 0 },
            "/cf" => ok("text/plain", format!("hello {}", ua.split('/').next().unwrap_or(""))),
            "/gz" => Resp { status: 200, headers: vec![("Content-Type", "text/plain".into()), ("Content-Encoding", "gzip".into())], body: gz.clone(), delay_ms: 0 },
            "/huge" => Resp { status: 200, headers: vec![("Content-Type", "text/plain".into()), ("Content-Length", "999999999".into())], ..Default::default() },
            "/slow" => Resp { delay_ms: 5000, ..ok("text/plain", "late") },
            _ => ok("text/plain", format!("lang={}", r.headers.get("accept-language").cloned().unwrap_or_default())),
        }
    })
    .await;
    let o = fetch(json!({"url": format!("{base}/missing")})).await;
    assert!(o.is_error && o.text.contains("\nStatus: 404 Not Found\n") && body(&o).starts_with("No such page"), "{}", o.text);
    assert!(o.text.contains("[… error page cut …]") && o.text.len() < 5000);
    let o = fetch(json!({"url": format!("{base}/old")})).await;
    assert!(o.text.starts_with(&format!("URL: {base}/new (redirected from {base}/old)")) && body(&o) == "moved here", "{}", o.text);
    assert_eq!(body(&fetch(json!({"url": format!("{base}/cf")})).await), "hello ref");
    assert_eq!(body(&fetch(json!({"url": format!("{base}/gz")})).await), "compressed body");
    let o = fetch(json!({"url": format!("{base}/huge")})).await;
    assert!(o.is_error && o.text.contains("Response too large"), "{}", o.text);
    let t = std::time::Instant::now();
    let o = fetch(json!({"url": format!("{base}/slow"), "timeout": 1})).await;
    assert!(o.is_error && o.text.contains("timed out") && t.elapsed() < Duration::from_secs(4), "{}", o.text);
    // Cancel.
    let d = temp_dir("webfetch-cancel");
    let c = ctx(&d);
    let token = c.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
    });
    let o = crate::execute("webfetch", json!({"url": format!("{base}/slow")}), &c).await;
    assert!(o.is_error && o.text.contains("cancelled") && t.elapsed() < Duration::from_secs(6), "{}", o.text);
    // Scheme-less URLs are https; the Accept-Language header is sent.
    assert!(body(&fetch(json!({"url": format!("{base}/lang")})).await).starts_with("lang=") );
    assert_eq!(parse_url("example.com/a").unwrap().as_str(), "https://example.com/a");
}

#[tokio::test]
async fn long_content_is_cut_and_saved() {
    let text: String = (1..=5000).map(|i| format!("line {i}\n")).collect();
    let t2 = text.clone();
    let base = serve(move |_| ok("text/plain", t2.clone())).await;
    let o = fetch(json!({"url": format!("{base}/long")})).await;
    let m = o.meta.as_ref().unwrap();
    assert_eq!(m["truncated"], true);
    assert!(body(&o).starts_with("line 1\nline 2\n") && o.text.contains("[Content cut: showing 2000 of 5000 lines"), "{}", &o.text[o.text.len() - 300..]);
    let path = m["output_path"].as_str().unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn helpers() {
    assert_eq!(html::cf_email("5a333b371a3f223b372a363f74393537").as_deref(), Some("iam@example.com"));
    assert_eq!(html::cf_email("zz"), None);
    for h in ["localhost", "127.0.0.1", "[::1]", "10.1.2.3", "192.168.0.9", "172.16.0.1", "fe80::1", "box.local"] {
        assert!(fetch::bypass_proxy(h), "{h}");
    }
    assert!(!fetch::bypass_proxy("8.8.8.8") || std::env::var("NO_PROXY").is_ok_and(|v| v.contains('*')));
    assert_eq!(fetch::accept_language_for("zh_CN.UTF-8"), "zh-CN,zh;q=0.9,en;q=0.8");
    assert_eq!(fetch::accept_language_for("de"), "de,en;q=0.8");
    assert_eq!(fetch::accept_language_for("C.UTF-8"), "en-US,en;q=0.9");
    assert_eq!(fetch::accept_language_for(""), "en-US,en;q=0.9");
    let u = |s: &str| Url::parse(s).unwrap();
    match sites::special(&u("https://github.com/o/r/blob/main/src/a b.rs")) {
        Some(sites::Special::Rewrite { url, .. }) => assert_eq!(url.as_str(), "https://raw.githubusercontent.com/o/r/main/src/a%20b.rs"),
        _ => panic!("blob rewrite"),
    }
    assert!(matches!(sites::special(&u("https://github.com/o/r/pull/12")), Some(sites::Special::GithubIssue { number: 12, .. })));
    assert!(sites::special(&u("https://github.com/o/r")).is_none());
    let issue = json!({"number": 7, "title": "Crash", "state": "closed", "state_reason": "completed", "user": {"login": "ann"}, "created_at": "2024-03-01T10:00:00Z", "labels": [{"name": "bug"}], "comments": 1, "body": "It crashes."});
    let (t, md) = sites::github_issue_markdown(&issue, &[json!({"user": {"login": "bob"}, "created_at": "2024-03-02T00:00:00Z", "body": "Fixed."})]);
    assert_eq!(t, "Crash");
    assert_eq!(md, "# Crash (#7)\n\nIssue, closed (completed) · by ann · opened 2024-03-01 · labels: bug · 1 comments\n\nIt crashes.\n\n---\n\n**bob** · 2024-03-02\n\nFixed.\n");
}

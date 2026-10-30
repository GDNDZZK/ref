//! HTML → markdown / text for `webfetch`: cleanup (scripts, hidden nodes, lazy images,
//! relative URLs, Cloudflare-obfuscated e-mails, code languages), main-content extraction
//! (readability) with a whole-page fallback.

use super::Format;
use dom_query::{Document, NodeRef};
use dom_smoothie::{Config, Readability, TextMode};
use regex::Regex;
use reqwest::Url;
use std::sync::LazyLock;

pub struct Page {
    pub title: Option<String>,
    pub body: String,
    pub note: Option<String>,
}

/// Extracted main content under a quarter of the page text must be at least this long (chars),
/// else the whole page is converted.
const MIN_ARTICLE_CHARS: usize = 3000;

/// `<link rel="alternate" type="text/markdown" href=…>`: a markdown version of the page.
pub fn markdown_alternate(html: &str, page: &Url) -> Option<Url> {
    static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link\b[^>]*>").unwrap());
    static ATTR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)\b(rel|type|href)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap()
    });
    // Only the head matters; avoid scanning megabytes of body.
    let head = &html[..crate::util::floor_char_boundary(
        html,
        html.find("</head").unwrap_or(html.len()).min(256 * 1024),
    )];
    for tag in LINK.find_iter(head) {
        let (mut rel, mut ty, mut href) = (String::new(), String::new(), None);
        for c in ATTR.captures_iter(tag.as_str()) {
            let v = c
                .get(2)
                .or(c.get(3))
                .or(c.get(4))
                .map_or("", |m| m.as_str());
            match c[1].to_ascii_lowercase().as_str() {
                "rel" => rel = v.to_ascii_lowercase(),
                "type" => ty = v.trim().to_ascii_lowercase(),
                _ => href = Some(v.trim().to_string()),
            }
        }
        if rel.split_whitespace().any(|r| r == "alternate")
            && (ty == "text/markdown" || ty == "text/x-markdown")
            && let Some(u) = href
                .and_then(|h| page.join(&h).ok())
                .filter(|u| matches!(u.scheme(), "http" | "https") && u != page)
        {
            return Some(u);
        }
    }
    None
}

/// Converts a page. `full_page = false` extracts the main content when the page reads like an
/// article and falls back to the whole page otherwise.
pub fn convert(html: &str, url: &Url, format: Format, full_page: bool) -> Page {
    let doc = Document::from(html);
    let prep = prepare(&doc, url);
    let full = render(&doc, format);
    if full_page || format == Format::Html {
        return Page {
            title: prep.title,
            body: full,
            note: None,
        };
    }
    let doc2 = Document::from(html);
    let prep2 = prepare(&doc2, url);
    let cfg = Config {
        text_mode: if format == Format::Text {
            TextMode::Formatted
        } else {
            TextMode::Markdown
        },
        classes_to_preserve: prep2.langs,
        ..Default::default()
    };
    let article = Readability::with_document(doc2, Some(url.as_str()), Some(cfg))
        .ok()
        .and_then(|mut r| {
            let readable = r.is_probably_readable();
            r.parse().ok().filter(|_| readable)
        });
    let Some(a) = article else {
        return Page {
            title: prep.title,
            body: full,
            note: None,
        };
    };
    let body = tidy(&a.text_content);
    let (n, total) = (body.chars().count(), full.chars().count().max(1));
    // A small piece of a big page (portal, listing) is the wrong "main content".
    if n * 4 < total && n < MIN_ARTICLE_CHARS {
        return Page {
            title: prep.title,
            body: full,
            note: None,
        };
    }
    let title = Some(a.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .or(prep.title);
    let note = (n * 10 < total * 9).then(|| {
        format!("Main content only ({}% of the page text). Pass full_page: true for the whole page (navigation, sidebars, comments).", (n * 100 / total).max(1))
    });
    Page { title, body, note }
}

fn render(doc: &Document, format: Format) -> String {
    match format {
        Format::Markdown => tidy(
            &doc.select("body")
                .nodes()
                .first()
                .map_or_else(|| doc.md(None), |b| b.md(None)),
        )
        .trim()
        .to_string(),
        Format::Text => tidy(&doc.select("body").formatted_text())
            .trim()
            .to_string(),
        Format::Html => doc.html().to_string(),
    }
}

/// Link / image destinations come out HTML-escaped (`?a=1&amp;b=2`, valid CommonMark); a model
/// copying the URL needs the real one.
fn unescape_urls(s: &str) -> String {
    static DEST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\]\(([^\s()]*(?:\([^\s()]*\)[^\s()]*)*)").unwrap());
    DEST.replace_all(s, |c: &regex::Captures| {
        let d = c[1]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&");
        format!("]({d}")
    })
    .into_owned()
}

/// Trailing spaces off, at most one blank line in a row (outside code fences).
fn tidy(s: &str) -> String {
    let s = &unescape_urls(s);
    let mut out = String::with_capacity(s.len());
    let (mut blank, mut fence) = (0, false);
    for line in s.lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        if line.is_empty() && !fence {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

struct Prep {
    title: Option<String>,
    /// Classes (`language-x`) put on code blocks, to survive readability's class stripping.
    langs: Vec<String>,
}

fn prepare(doc: &Document, page: &Url) -> Prep {
    let title = Some(
        doc.select("head title")
            .first()
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
    .filter(|t| !t.is_empty());
    let base = doc
        .select("base[href]")
        .attr("href")
        .and_then(|h| page.join(h.trim()).ok())
        .unwrap_or_else(|| page.clone());
    doc.select("script, style, noscript, template, iframe, frame, object, embed, canvas, svg, math annotation, dialog, [hidden], [aria-hidden='true']").remove();
    for n in doc.select("[style]").nodes() {
        let st: String = n
            .attr("style")
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        if st.contains("display:none") || st.contains("visibility:hidden") {
            n.remove_from_parent();
        }
    }
    for n in doc.select("[data-cfemail]").nodes() {
        if let Some(mail) = n.attr("data-cfemail").and_then(|h| cf_email(&h)) {
            n.set_text(mail.as_str());
            n.remove_attr("data-cfemail");
        }
    }
    for n in doc.select("a[href]").nodes() {
        let href = n.attr("href").unwrap_or_default().trim().to_string();
        if let Some(mail) = href
            .split_once("/cdn-cgi/l/email-protection#")
            .and_then(|(_, h)| cf_email(h))
        {
            n.set_attr("href", &format!("mailto:{mail}"));
            if n.text().contains("email") && n.text().contains("protected") {
                n.set_text(mail.as_str());
            }
        } else if href.to_ascii_lowercase().starts_with("javascript:") || href.is_empty() {
            n.remove_attr("href");
        } else if let Ok(u) = base.join(&href) {
            n.set_attr("href", u.as_str());
        }
    }
    for n in doc.select("img").nodes() {
        fix_image(n, &base);
    }
    let mut langs = Vec::new();
    for pre in doc.select("pre").nodes() {
        if let Some(l) = code_lang(pre) {
            let class = format!("language-{l}");
            // Readability keeps listed classes; the markdown writer reads `language-*` on <code>.
            match pre.children().into_iter().find(|c| c.has_name("code")) {
                Some(code) => code.add_class(&class),
                None => pre.set_html(format!(
                    "<code class=\"{class}\">{}</code>",
                    pre.inner_html()
                )),
            }
            if !langs.contains(&class) {
                langs.push(class);
            }
        }
    }
    for s in doc.select("summary").nodes() {
        s.set_html(format!("<strong>{}</strong>", s.inner_html()));
        s.rename("p");
    }
    doc.select("details").rename("div");
    Prep { title, langs }
}

/// Lazy-loading placeholders and `data:` URIs replaced by the real source (or dropped).
fn fix_image(n: &NodeRef, base: &Url) {
    let usable = |s: &str| !s.trim().is_empty() && !s.trim_start().starts_with("data:");
    let src = n
        .attr("src")
        .map(|s| s.to_string())
        .filter(|s| usable(s))
        .or_else(|| {
            [
                "data-src",
                "data-original",
                "data-lazy-src",
                "data-url",
                "data-srcset",
                "srcset",
            ]
            .iter()
            .filter_map(|a| n.attr(a))
            .map(|v| {
                v.split(',')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .find(|s| usable(s))
        });
    match src.and_then(|s| base.join(s.trim()).ok()) {
        Some(u) => {
            n.set_attr("src", u.as_str());
            n.remove_attrs(&["srcset", "data-src", "data-srcset"]);
        }
        None => {
            let alt = n
                .attr("alt")
                .map(|a| a.trim().to_string())
                .unwrap_or_default();
            if alt.is_empty() {
                n.remove_from_parent();
            } else {
                n.replace_with_html(format!("<span>[image: {}]</span>", html_escape(&alt)));
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Cloudflare e-mail obfuscation: hex, first byte is the XOR key.
pub fn cf_email(hex: &str) -> Option<String> {
    let hex = hex.trim();
    if hex.len() < 4 || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<_>>()?;
    let key = bytes[0];
    let out = String::from_utf8(bytes[1..].iter().map(|b| b ^ key).collect()).ok()?;
    out.contains('@').then_some(out)
}

/// Language of a code block from the usual conventions: `language-x` / `lang-x` classes,
/// GitHub `highlight-source-x`, `highlight x`, pandoc `sourceCode x`, SyntaxHighlighter
/// `brush: x`, `data-lang` attributes — on the block, its `<code>` or a close ancestor.
fn code_lang(pre: &NodeRef) -> Option<String> {
    const SKIP: [&str; 12] = [
        "highlight",
        "sourcecode",
        "source",
        "code",
        "hljs",
        "prettyprint",
        "chroma",
        "notranslate",
        "shiki",
        "line-numbers",
        "codehilite",
        "plaintext",
    ];
    let mut nodes: Vec<NodeRef> = pre
        .children()
        .into_iter()
        .filter(|c| c.has_name("code"))
        .collect();
    nodes.insert(0, *pre);
    nodes.extend(pre.ancestors(Some(2)));
    for n in &nodes {
        for a in ["data-lang", "data-language", "lang"] {
            if let Some(v) = n.attr(a).map(|v| clean_lang(&v)).filter(|v| !v.is_empty()) {
                return Some(v);
            }
        }
        let class = n.class().unwrap_or_default().to_string();
        if let Some(b) = class
            .split_once("brush:")
            .map(|(_, b)| clean_lang(b.split(';').next().unwrap_or("")))
            .filter(|b| !b.is_empty())
        {
            return Some(b);
        }
        for c in class.split_whitespace() {
            let lc = c.to_ascii_lowercase();
            for p in [
                "language-",
                "lang-",
                "highlight-source-",
                "highlight-",
                "source-",
            ] {
                if let Some(v) = lc
                    .strip_prefix(p)
                    .map(clean_lang)
                    .filter(|v| !v.is_empty() && !SKIP.contains(&v.as_str()))
                {
                    return Some(v);
                }
            }
        }
        // `highlight python`, `sourceCode rust`: a bare word next to a marker class.
        let words: Vec<String> = class
            .split_whitespace()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if words.iter().any(|w| w == "highlight" || w == "sourcecode")
            && let Some(w) = words.iter().find(|w| {
                !SKIP.contains(&w.as_str())
                    && w.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+#-".contains(c))
                    && w.len() <= 20
            })
        {
            return Some(w.clone());
        }
    }
    None
}

fn clean_lang(s: &str) -> String {
    let t = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    t.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.' | '#'))
        .collect()
}

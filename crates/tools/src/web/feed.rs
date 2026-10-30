//! RSS (0.9x / 1.0 / 2.0) and Atom feeds → a markdown list of entries.

use quick_xml::Reader;
use quick_xml::events::Event;

/// Entry summaries longer than this (chars) are cut.
const SUMMARY_CHARS: usize = 600;

/// The document's root element is `rss`, `feed` (Atom) or `rdf:RDF` (RSS 1.0).
pub fn is_feed(mime: &str, text: &str) -> bool {
    if matches!(
        mime,
        "application/rss+xml" | "application/atom+xml" | "application/rdf+xml"
    ) {
        return true;
    }
    if !mime.contains("xml") {
        return false;
    }
    let mut r = Reader::from_str(text);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                return matches!(e.local_name().as_ref(), "rss" | "feed" | "RDF");
            }
            Ok(Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    date: String,
    summary: String,
    author: String,
}

/// `(feed title, markdown)`; `None` if the XML is not a readable feed.
pub fn to_markdown(xml: &str) -> Option<(Option<String>, String)> {
    let mut r = Reader::from_str(xml);
    let (mut feed, mut entries) = (Entry::default(), Vec::<Entry>::new());
    let mut cur: Option<Entry> = None;
    // Element path (local names) and the text collected for the innermost element.
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut saw_root = false;
    loop {
        let ev = r.read_event().ok()?;
        let empty = matches!(ev, Event::Empty(_));
        match ev {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name().as_ref().to_string();
                saw_root |= matches!(name.as_str(), "rss" | "feed" | "RDF");
                if matches!(name.as_str(), "item" | "entry") {
                    cur = Some(Entry::default());
                }
                // Atom: <link rel="alternate" href="…"/> (no rel = alternate).
                if name == "link" {
                    let mut href = None;
                    let mut rel = String::from("alternate");
                    for a in e.attributes().flatten() {
                        let v = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        match a.key.local_name().as_ref() {
                            "href" => href = Some(v),
                            "rel" => rel = v,
                            _ => {}
                        }
                    }
                    if let Some(h) = href.filter(|_| rel == "alternate") {
                        let target = cur.as_mut().unwrap_or(&mut feed);
                        if target.link.is_empty() {
                            target.link = h;
                        }
                    }
                }
                if !empty {
                    path.push(name);
                    text.clear();
                }
            }
            Event::Text(t) => text.push_str(&t.html_content()),
            Event::CData(c) => text.push_str(&c),
            Event::GeneralRef(g) => match g.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => {
                    let n = g.to_string();
                    let s = match n.as_str() {
                        "lt" => "<".to_string(),
                        "gt" => ">".to_string(),
                        "amp" => "&".to_string(),
                        "quot" => "\"".to_string(),
                        "apos" => "'".to_string(),
                        // HTML entities in summaries are decoded with the HTML later.
                        _ => format!("&{n};"),
                    };
                    text.push_str(&s);
                }
            },
            Event::End(_) => {
                let Some(name) = path.pop() else { continue };
                if matches!(name.as_str(), "item" | "entry") {
                    if let Some(e) = cur.take() {
                        entries.push(e);
                    }
                    continue;
                }
                let value = std::mem::take(&mut text).trim().to_string();
                if value.is_empty() {
                    continue;
                }
                let target = cur.as_mut().unwrap_or(&mut feed);
                let slot = match name.as_str() {
                    "title" => &mut target.title,
                    "link" => &mut target.link,
                    "pubDate" | "published" | "updated" | "date" | "lastBuildDate" => {
                        &mut target.date
                    }
                    "description" | "summary" | "subtitle" | "encoded" | "content" => {
                        &mut target.summary
                    }
                    "name" | "creator" | "author" => &mut target.author,
                    _ => continue,
                };
                // First value wins (`published` before `updated`, `description` before `content`).
                if slot.is_empty() {
                    *slot = value;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return None;
    }
    let mut md = String::new();
    let title = Some(plain(&feed.title)).filter(|t| !t.is_empty());
    if let Some(t) = &title {
        md.push_str(&format!("# {t}\n\n"));
    }
    if !feed.summary.is_empty() {
        md.push_str(&format!("{}\n\n", plain(&feed.summary)));
    }
    if !feed.link.is_empty() {
        md.push_str(&format!("Site: {}\n\n", feed.link.trim()));
    }
    md.push_str(&format!("{} entries\n", entries.len()));
    for e in &entries {
        let t = Some(plain(&e.title))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "(untitled)".into());
        md.push_str("\n## ");
        if e.link.is_empty() {
            md.push_str(&t);
        } else {
            md.push_str(&format!("[{t}]({})", e.link.trim()));
        }
        md.push('\n');
        let meta: Vec<&str> = [e.date.as_str(), e.author.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        if !meta.is_empty() {
            md.push_str(&format!("{}\n", meta.join(" · ")));
        }
        let s = plain(&e.summary);
        if !s.is_empty() {
            let (cut, more) = crate::util::truncate_chars(&s, SUMMARY_CHARS);
            md.push_str(&format!("\n{cut}{}\n", if more { "…" } else { "" }));
        }
    }
    Some((title, md))
}

/// Feed text fields often hold (escaped) HTML: reduce to plain text on one paragraph. Text
/// without real tags or entities (`Second <post>`) is kept as it is.
fn plain(s: &str) -> String {
    static HTML: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)</?(p|br|b|i|em|strong|a|div|span|img|ul|ol|li|code|pre|h[1-6]|blockquote|table|tr|td|figure|hr)\b|&(#\d+|#x[0-9a-f]+|[a-z]+);").unwrap()
    });
    let t = if HTML.is_match(s) {
        dom_query::Document::from(format!("<body>{s}</body>").as_str())
            .select("body")
            .text()
            .to_string()
    } else {
        s.to_string()
    };
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

//! Incremental Server-Sent Events parser.
//!
//! Implements the WHATWG event-stream framing rules that matter for LLM APIs:
//! `\n`, `\r\n` and lone `\r` line endings (also when split across chunks),
//! multi-line `data:` fields, `event:` names, `:` comments, and dispatch on a
//! blank line. Bytes are buffered per line so UTF-8 sequences split across
//! network chunks are decoded correctly.

use serde_json::Value;

/// One dispatched SSE event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// Value of the `event:` field (empty when absent).
    pub event: String,
    /// `data:` lines joined with `\n`.
    pub data: String,
}

#[derive(Debug, Default)]
pub(crate) struct SseParser {
    line: Vec<u8>,
    /// Last byte seen was `\r`; swallow a directly following `\n`.
    skip_lf: bool,
    /// Whether the very first line has been processed (BOM stripping).
    started: bool,
    event: String,
    data: String,
    has_data: bool,
}

impl SseParser {
    /// Feed raw bytes; returns every event completed by this chunk.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        for &b in chunk {
            if self.skip_lf {
                self.skip_lf = false;
                if b == b'\n' {
                    continue;
                }
            }
            match b {
                b'\n' => self.end_line(&mut out),
                b'\r' => {
                    self.end_line(&mut out);
                    self.skip_lf = true;
                }
                _ => self.line.push(b),
            }
        }
        out
    }

    /// Flush at end of stream: a final event without a trailing blank line is
    /// still dispatched.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.line.is_empty() {
            self.end_line(&mut out);
        }
        self.dispatch(&mut out);
        out
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) {
        let raw = std::mem::take(&mut self.line);
        let mut line = String::from_utf8_lossy(&raw).into_owned();
        if !self.started {
            self.started = true;
            if let Some(rest) = line.strip_prefix('\u{feff}') {
                line = rest.to_string();
            }
        }
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line.starts_with(':') {
            return; // comment / keep-alive
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_str(), ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.event = value.to_string(),
            _ => {} // id, retry and unknown fields are irrelevant here
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        if self.has_data {
            out.push(SseEvent {
                event: std::mem::take(&mut self.event),
                data: std::mem::take(&mut self.data),
            });
        } else {
            self.event.clear();
        }
        self.has_data = false;
    }
}

/// Parse the JSON payload(s) of an event.
///
/// Normally `data` is one JSON document. Some non-compliant proxies omit the
/// blank line between events, which makes consecutive `data:` lines merge; in
/// that case each line is parsed separately. Unparseable lines and `[DONE]`
/// markers are skipped.
pub(crate) fn data_values(data: &str) -> Vec<Value> {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Vec::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return vec![v];
    }
    data.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "[DONE]")
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Whether the event payload contains the OpenAI `[DONE]` terminator.
pub(crate) fn is_done(data: &str) -> bool {
    data.lines().any(|l| l.trim() == "[DONE]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(event: &str, data: &str) -> SseEvent {
        SseEvent {
            event: event.into(),
            data: data.into(),
        }
    }

    #[test]
    fn basic_lf() {
        let mut p = SseParser::default();
        let out = p.push(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(out, vec![ev("", "{\"a\":1}"), ev("", "{\"b\":2}")]);
    }

    #[test]
    fn crlf_and_event_names() {
        let mut p = SseParser::default();
        let out = p.push(b"event: message_start\r\ndata: {}\r\n\r\nevent: ping\r\ndata: x\r\n\r\n");
        assert_eq!(out, vec![ev("message_start", "{}"), ev("ping", "x")]);
    }

    #[test]
    fn lone_cr_split_across_chunks() {
        let mut p = SseParser::default();
        let mut out = p.push(b"data: a\r");
        out.extend(p.push(b"\ndata: b\r"));
        out.extend(p.push(b"\r"));
        assert_eq!(out, vec![ev("", "a\nb")]);
    }

    #[test]
    fn multiline_data_comments_and_no_space() {
        let mut p = SseParser::default();
        let out = p.push(b": keep-alive\ndata:line1\ndata: line2\nid: 7\nretry: 100\n\n");
        assert_eq!(out, vec![ev("", "line1\nline2")]);
    }

    #[test]
    fn split_utf8_and_partial_lines() {
        let text = "data: {\"t\":\"héllo 世界\"}\n\n".as_bytes();
        let mut p = SseParser::default();
        let mut out = Vec::new();
        for b in text {
            out.extend(p.push(std::slice::from_ref(b)));
        }
        assert_eq!(out, vec![ev("", "{\"t\":\"héllo 世界\"}")]);
    }

    #[test]
    fn event_without_data_is_ignored_and_finish_flushes() {
        let mut p = SseParser::default();
        let mut out = p.push(b"event: lonely\n\nevent: last\ndata: [DONE]");
        assert!(out.is_empty());
        out.extend(p.finish());
        assert_eq!(out, vec![ev("last", "[DONE]")]);
        assert!(is_done(&out[0].data));
    }

    #[test]
    fn bom_is_stripped() {
        let mut p = SseParser::default();
        let out = p.push("\u{feff}data: 1\n\n".as_bytes());
        assert_eq!(out, vec![ev("", "1")]);
    }

    #[test]
    fn data_values_fallback_per_line() {
        let vals = data_values("{\"a\":1}\n{\"b\":2}\n[DONE]");
        assert_eq!(vals.len(), 2);
        assert!(data_values("[DONE]").is_empty());
        assert_eq!(data_values("{\"x\":\n1}").len(), 1);
    }
}

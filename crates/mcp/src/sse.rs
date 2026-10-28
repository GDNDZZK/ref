//! Minimal incremental Server-Sent Events parser.

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SseEvent {
    /// `event:` field; empty means the default ("message").
    pub event: String,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Default)]
pub(crate) struct SseParser {
    buf: Vec<u8>,
    event: String,
    data: Vec<String>,
    id: Option<String>,
    has_data: bool,
}

impl SseParser {
    /// Feed a chunk; returns every event completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut start = 0;
        while let Some(pos) = self.buf[start..].iter().position(|&b| b == b'\n') {
            let end = start + pos;
            let mut line = &self.buf[start..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            let line = String::from_utf8_lossy(line).into_owned();
            start = end + 1;
            if let Some(ev) = self.line(&line) {
                out.push(ev);
            }
        }
        self.buf.drain(..start);
        out
    }

    /// Flush at end of stream (a trailing event without a blank line).
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_string();
            if let Some(ev) = self.line(&line) {
                return Some(ev);
            }
        }
        self.line("")
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if !self.has_data {
                self.event.clear();
                return None;
            }
            let ev = SseEvent {
                event: std::mem::take(&mut self.event),
                data: std::mem::take(&mut self.data).join("\n"),
                id: self.id.clone(),
            };
            self.has_data = false;
            return Some(ev);
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                self.data.push(value.to_string());
                self.has_data = true;
            }
            "id" => self.id = Some(value.to_string()),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_chunks_and_crlf() {
        let mut p = SseParser::default();
        let mut evs = p.push(b"event: endpoint\r\ndata: /msg?x=1\r");
        assert!(evs.is_empty());
        evs.extend(p.push(b"\n\r\n: comment\n\ndata: {\"a\":"));
        evs.extend(p.push(b"1}\nid: 7\n\ndata: a\ndata: b\n\n"));
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0].event, "endpoint");
        assert_eq!(evs[0].data, "/msg?x=1");
        assert_eq!(evs[1].event, "");
        assert_eq!(evs[1].data, "{\"a\":1}");
        assert_eq!(evs[1].id.as_deref(), Some("7"));
        assert_eq!(evs[2].data, "a\nb");
    }

    #[test]
    fn finish_flushes_trailing_event() {
        let mut p = SseParser::default();
        assert!(p.push(b"data: x").is_empty());
        assert_eq!(p.finish().map(|e| e.data), Some("x".into()));
        assert_eq!(p.finish(), None);
    }
}

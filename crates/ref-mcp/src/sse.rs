//! Minimal incremental Server-Sent Events parser.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// Event type (`message` when not specified).
    pub event: String,
    pub data: String,
}

#[derive(Default)]
pub(crate) struct SseParser {
    buf: Vec<u8>,
    /// Bytes of `buf` already scanned for a newline.
    scanned: usize,
    event: String,
    data: String,
    has_data: bool,
}

impl SseParser {
    /// Feed a chunk of bytes, returning all events completed by it.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut start = 0;
        let mut i = self.scanned;
        while i < self.buf.len() {
            if self.buf[i] == b'\n' {
                let mut end = i;
                if end > start && self.buf[end - 1] == b'\r' {
                    end -= 1;
                }
                let line = String::from_utf8_lossy(&self.buf[start..end]).into_owned();
                self.line(&line, &mut out);
                start = i + 1;
            }
            i += 1;
        }
        self.buf.drain(..start);
        self.scanned = self.buf.len();
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if self.has_data {
                let event = if self.event.is_empty() { "message".to_string() } else { std::mem::take(&mut self.event) };
                out.push(SseEvent { event, data: std::mem::take(&mut self.data) });
            }
            self.event.clear();
            self.data.clear();
            self.has_data = false;
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = match line.find(':') {
            Some(idx) => {
                let v = &line[idx + 1..];
                (&line[..idx], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        match field {
            "event" => self.event = value.to_string(),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_events() {
        let mut p = SseParser::default();
        assert!(p.feed(b"event: endpoint\r\nda").is_empty());
        let ev = p.feed(b"ta: /msg?x=1\r\n\r\n: comment\ndata: a\ndata: b\n\n");
        assert_eq!(
            ev,
            vec![
                SseEvent { event: "endpoint".into(), data: "/msg?x=1".into() },
                SseEvent { event: "message".into(), data: "a\nb".into() },
            ]
        );
        assert!(p.feed(b"event: x\n\n").is_empty());
    }
}

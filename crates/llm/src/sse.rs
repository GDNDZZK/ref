//! Incremental Server-Sent Events decoder.
//!
//! Follows the WHATWG event-stream parsing rules closely enough for LLM APIs:
//! lines end in `\n`, `\r\n` or `\r` (also when split across chunks); `:` lines are comments;
//! `data:` lines are joined with `\n`; a blank line dispatches the event. Bytes are buffered
//! until a full line is available, so UTF-8 sequences split across network chunks decode
//! correctly. Invalid UTF-8 is replaced (never panics).

/// One dispatched SSE event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// Value of the last `event:` line, if any.
    pub event: Option<String>,
    /// All `data:` lines joined with `\n`.
    pub data: String,
}

/// Stateful decoder; feed it raw body chunks with [`SseDecoder::push`].
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// Bytes of the current, incomplete line.
    line: Vec<u8>,
    /// The previous chunk ended in `\r`; swallow a leading `\n` in the next chunk.
    pending_cr: bool,
    event: Option<String>,
    data: String,
    has_data: bool,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk; returns every event completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut out = Vec::new();
        let mut i = 0;
        if self.pending_cr {
            self.pending_cr = false;
            if chunk.first() == Some(&b'\n') {
                i = 1;
            }
        }
        while i < chunk.len() {
            // Find the next line terminator.
            match chunk[i..].iter().position(|&b| b == b'\n' || b == b'\r') {
                None => {
                    self.line.extend_from_slice(&chunk[i..]);
                    break;
                }
                Some(off) => {
                    let end = i + off;
                    self.line.extend_from_slice(&chunk[i..end]);
                    let term = chunk[end];
                    i = end + 1;
                    if term == b'\r' {
                        if i < chunk.len() {
                            if chunk[i] == b'\n' {
                                i += 1;
                            }
                        } else {
                            self.pending_cr = true;
                        }
                    }
                    let line = std::mem::take(&mut self.line);
                    if let Some(ev) = self.process_line(&line) {
                        out.push(ev);
                    }
                }
            }
        }
        out
    }

    /// Flush at end of stream: a trailing line and an event that lacked the final blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        let line = std::mem::take(&mut self.line);
        if !line.is_empty() {
            if let Some(ev) = self.process_line(&line) {
                return Some(ev);
            }
        }
        self.dispatch()
    }

    fn process_line(&mut self, line: &[u8]) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line[0] == b':' {
            return None; // comment / keep-alive
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = match text.find(':') {
            Some(p) => {
                let v = &text[p + 1..];
                (&text[..p], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (&text[..], ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.event = Some(value.to_string()),
            _ => {} // id, retry and unknown fields are irrelevant here
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = self.event.take();
        if !self.has_data {
            return None;
        }
        self.has_data = false;
        Some(SseEvent { event, data: std::mem::take(&mut self.data) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut d = SseDecoder::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(d.push(c));
        }
        out.extend(d.finish());
        out
    }

    #[test]
    fn basic_events() {
        let evs = decode_all(&[b"event: a\ndata: {\"x\":1}\n\ndata: second\n\n"]);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("a"));
        assert_eq!(evs[0].data, "{\"x\":1}");
        assert_eq!(evs[1].event, None);
        assert_eq!(evs[1].data, "second");
    }

    #[test]
    fn multiline_comments_crlf() {
        let evs = decode_all(&[b": keepalive\r\ndata: a\r\ndata:b\r\n\r\n: x\r\r"]);
        assert_eq!(evs, vec![SseEvent { event: None, data: "a\nb".into() }]);
    }

    #[test]
    fn split_everywhere_including_utf8_and_crlf() {
        let src = "event: message\r\ndata: héllo 世界 🎉\r\n\r\ndata: [DONE]\r\n\r\n".as_bytes();
        let whole = decode_all(&[src]);
        for size in 1..8 {
            let chunks: Vec<&[u8]> = src.chunks(size).collect();
            assert_eq!(decode_all(&chunks), whole, "chunk size {size}");
        }
        assert_eq!(whole[0].data, "héllo 世界 🎉");
        assert_eq!(whole[0].event.as_deref(), Some("message"));
        assert_eq!(whole[1].data, "[DONE]");
    }

    #[test]
    fn event_without_data_is_dropped_and_trailing_flushed() {
        let evs = decode_all(&[b"event: ping\n\ndata: tail"]);
        assert_eq!(evs, vec![SseEvent { event: None, data: "tail".into() }]);
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        let evs = decode_all(&[b"data: \xff\xfe\n\n"]);
        assert_eq!(evs.len(), 1);
    }
}

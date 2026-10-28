//! Minimal, robust Server-Sent Events parser (WHATWG event-stream format).
//!
//! Bytes are fed incrementally with [`SseParser::push`]; complete events are returned as soon as
//! their terminating blank line arrives. Handles `\n`, `\r\n` and lone `\r` line endings (also
//! when split across chunks), comment lines, multi-line `data:` fields and a final event that is
//! not followed by a blank line ([`SseParser::finish`]).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Value of the `event:` field, if any.
    pub event: Option<String>,
    /// `data:` lines joined with `\n`.
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    line: Vec<u8>,
    /// The previous chunk ended with `\r`; a leading `\n` in the next chunk belongs to it.
    pending_cr: bool,
    event: Option<String>,
    data: String,
    has_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes; returns all events completed by it.
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
            match memchr_newline(&chunk[i..]) {
                Some(off) => {
                    let end = i + off;
                    self.line.extend_from_slice(&chunk[i..end]);
                    let line = std::mem::take(&mut self.line);
                    self.process_line(&line, &mut out);
                    if chunk[end] == b'\r' {
                        if end + 1 < chunk.len() {
                            if chunk[end + 1] == b'\n' {
                                i = end + 2;
                                continue;
                            }
                        } else {
                            self.pending_cr = true;
                        }
                    }
                    i = end + 1;
                }
                None => {
                    self.line.extend_from_slice(&chunk[i..]);
                    break;
                }
            }
        }
        out
    }

    /// End of input: flush a trailing unterminated line / event.
    pub fn finish(&mut self) -> Option<SseEvent> {
        let mut out = Vec::new();
        if !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.process_line(&line, &mut out);
        }
        self.dispatch(&mut out);
        out.pop()
    }

    fn process_line(&mut self, line: &[u8], out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            self.dispatch(out);
            return;
        }
        if line[0] == b':' {
            return; // comment / keep-alive
        }
        let (field, value) = match line.iter().position(|&b| b == b':') {
            Some(p) => {
                let v = &line[p + 1..];
                (&line[..p], v.strip_prefix(b" ").unwrap_or(v))
            }
            None => (line, &b""[..]),
        };
        match field {
            b"data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(&String::from_utf8_lossy(value));
                self.has_data = true;
            }
            b"event" => self.event = Some(String::from_utf8_lossy(value).into_owned()),
            _ => {} // id, retry, unknown fields
        }
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let event = self.event.take();
        if !self.has_data {
            return;
        }
        self.has_data = false;
        out.push(SseEvent { event, data: std::mem::take(&mut self.data) });
    }
}

fn memchr_newline(s: &[u8]) -> Option<usize> {
    s.iter().position(|&b| b == b'\n' || b == b'\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.push(c));
        }
        out.extend(p.finish());
        out
    }

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent { event: event.map(str::to_string), data: data.to_string() }
    }

    #[test]
    fn basic_and_multiline() {
        let evs = all(&[b"event: a\ndata: {\"x\":1}\n\ndata: l1\ndata: l2\n\n"]);
        assert_eq!(evs, vec![ev(Some("a"), "{\"x\":1}"), ev(None, "l1\nl2")]);
    }

    #[test]
    fn crlf_split_across_chunks_and_comments() {
        let evs = all(&[b": keep-alive\r", b"\nevent: b\r\ndata:x", b"y\r", b"\n\r", b"\ndata: [DONE]\r\r"]);
        assert_eq!(evs, vec![ev(Some("b"), "xy"), ev(None, "[DONE]")]);
    }

    #[test]
    fn byte_by_byte_utf8() {
        let src = "data: 你好 🌍\n\ndata: second\n\n".as_bytes();
        let mut p = SseParser::new();
        let mut out = Vec::new();
        for b in src {
            out.extend(p.push(std::slice::from_ref(b)));
        }
        assert_eq!(out, vec![ev(None, "你好 🌍"), ev(None, "second")]);
    }

    #[test]
    fn trailing_event_without_blank_line() {
        let evs = all(&[b"data: last"]);
        assert_eq!(evs, vec![ev(None, "last")]);
    }

    #[test]
    fn event_without_data_is_ignored() {
        let evs = all(&[b"event: ping\n\nid: 3\nretry: 10\ndata\n\n"]);
        assert_eq!(evs, vec![ev(None, "")]);
    }
}

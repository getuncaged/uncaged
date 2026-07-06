//! A minimal Server-Sent Events line decoder.
//!
//! Both the Anthropic and OpenAI streaming APIs send newline-delimited frames
//! where the JSON payload is on a `data:` line. We don't need full SSE
//! semantics (event types, multi-line data, retry) — just the `data:` payloads.

/// Accumulates bytes and yields complete `data:` payloads as they arrive.
#[derive(Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes; returns any newly-complete `data:` payloads.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = line.strip_prefix("data:") {
                out.push(rest.trim().to_string());
            }
        }
        out
    }
}

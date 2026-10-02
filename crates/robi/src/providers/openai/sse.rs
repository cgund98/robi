//! Server-sent events, decoded from a byte stream.
//!
//! Hand-rolled because the protocol is small and the two properties that matter
//! are hidden by a wrapper: the inter-chunk timeout, and cancellation. The decoder
//! holds bytes rather than lines so a multi-byte character split across two chunks
//! is reassembled instead of corrupted.
//!
//! The spec allows more than OpenAI uses, and a proxy in front of many models
//! emits it: multi-line `data:` fields join with a newline, both CRLF and LF end a
//! line, and `:` comments (frequently keepalives) are ignored.

/// Accumulates chunks and yields the payload of each complete event.
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// Bytes not yet terminated by a newline.
    buffer: Vec<u8>,
    /// The `data:` values of the event being accumulated.
    data: Vec<String>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk, returning the payload of every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            // Drop the newline, then the carriage return of a CRLF ending.
            let line = &line[..line.len() - 1];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            self.consume(line, &mut events);
        }

        events
    }

    /// Flush an event whose last line arrived without a terminating newline.
    ///
    /// A stream that ends without a blank line still carried its data, and dropping
    /// it would lose the final chunk of a message.
    pub fn finish(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.consume(&line, &mut events);
        }
        self.dispatch(&mut events);
        events
    }

    fn consume(&mut self, line: &[u8], events: &mut Vec<String>) {
        // A blank line ends the event.
        if line.is_empty() {
            self.dispatch(events);
            return;
        }

        // A comment, which is what a keepalive looks like.
        if line[0] == b':' {
            return;
        }

        let Ok(text) = std::str::from_utf8(line) else {
            // One unreadable line must not end a turn.
            tracing::debug!("skipping a non-UTF-8 SSE line");
            return;
        };

        let Some((field, value)) = text.split_once(':') else {
            // A field name with no colon is valid and carries an empty value.
            return;
        };

        // `event:`, `id:`, and `retry:` carry nothing this protocol uses.
        if field != "data" {
            return;
        }

        // One optional leading space is part of the framing, not the value.
        self.data
            .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
    }

    fn dispatch(&mut self, events: &mut Vec<String>) {
        if self.data.is_empty() {
            return;
        }
        events.push(self.data.join("\n"));
        self.data.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payloads(chunks: &[&[u8]]) -> Vec<String> {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(decoder.push(chunk));
        }
        events.extend(decoder.finish());
        events
    }

    #[test]
    fn a_single_event_yields_its_payload() {
        assert_eq!(payloads(&[b"data: {\"a\":1}\n\n"]), vec!["{\"a\":1}"]);
    }

    #[test]
    fn events_are_dispatched_only_on_a_blank_line() {
        // No blank line yet, so nothing is dispatched even though a line arrived.
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(b"data: one\n").is_empty());
        assert_eq!(decoder.push(b"\n"), vec!["one".to_owned()]);
    }

    #[test]
    fn crlf_endings_work() {
        assert_eq!(payloads(&[b"data: one\r\n\r\n"]), vec!["one"]);
    }

    #[test]
    fn a_payload_split_across_chunks_is_reassembled() {
        assert_eq!(
            payloads(&[b"data: {\"cho", b"ices\":[]}\n\n"]),
            vec!["{\"choices\":[]}"]
        );
    }

    #[test]
    fn a_split_multibyte_character_survives() {
        let payload = "data: é\n\n".as_bytes();
        let (head, tail) = payload.split_at(7); // splits the two-byte `é`
        assert_eq!(payloads(&[head, tail]), vec!["é"]);
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(
            payloads(&[b": keepalive\n\n", b"data: two\n\n"]),
            vec!["two"]
        );
    }

    #[test]
    fn multi_line_data_joins_with_a_newline() {
        assert_eq!(payloads(&[b"data: one\ndata: two\n\n"]), vec!["one\ntwo"]);
    }

    #[test]
    fn other_fields_are_ignored() {
        assert_eq!(
            payloads(&[b"event: message\nid: 7\nretry: 100\ndata: x\n\n"]),
            vec!["x"]
        );
    }

    #[test]
    fn a_final_event_without_a_blank_line_is_still_dispatched() {
        assert_eq!(payloads(&[b"data: done"]), vec!["done"]);
    }

    #[test]
    fn the_done_sentinel_passes_through_as_a_payload() {
        // The adapter decides what `[DONE]` means; the decoder only frames it.
        assert_eq!(payloads(&[b"data: [DONE]\n\n"]), vec!["[DONE]"]);
    }

    #[test]
    fn a_blank_line_with_no_data_dispatches_nothing() {
        assert!(payloads(&[b"\n\n\n"]).is_empty());
    }
}

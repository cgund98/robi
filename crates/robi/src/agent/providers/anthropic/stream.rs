//! The response side: Anthropic's typed SSE events become deltas, then one
//! assembled assistant message.
//!
//! Anthropic's stream is a typed sequence, not a stream of `choices[].delta`
//! chunks. Each event carries a `type` in its JSON body that mirrors the SSE
//! `event:` name, so the shared [`SseDecoder`](crate::agent::providers::sse) —
//! which keeps only the `data:` payload — is enough; this module reads the `type`.
//!
//! The assembler owns the state the caller must not reason about: the partial
//! text, the thinking trace with its signature (which a tool continuation must
//! echo back, A3), and the tool calls keyed by their content-block index. The
//! usage in `message_delta` is **cumulative**, so it replaces the running counts
//! rather than adding (A8).

use std::collections::BTreeMap;

use robi_core::ids::ToolCallId;
use robi_core::message::{Message, ReasoningTrace, ToolCall, Usage};
use robi_core::model::Delta;
use serde::Deserialize;

use crate::agent::providers::error::ProviderError;

/// One streamed event. The `type` tag in the body selects the variant; the SSE
/// `event:` name is redundant and ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    MessageStart {
        message: MessageStart,
    },
    ContentBlockStart {
        index: usize,
        content_block: BlockStart,
    },
    ContentBlockDelta {
        index: usize,
        delta: BlockDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        #[serde(default)]
        delta: MessageDeltaBody,
        #[serde(default)]
        usage: Option<UsageBody>,
    },
    MessageStop,
    Ping,
    Error {
        error: ErrorBody,
    },
    /// An event type this client does not model; ignored (versioning).
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageStart {
    #[serde(default)]
    pub usage: Option<UsageBody>,
}

/// The opening of a content block.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockStart {
    Text {
        #[serde(default)]
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    ToolUse {
        #[serde(default)]
        id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    /// A block this client does not model; ignored.
    #[serde(other)]
    Other,
}

/// A content-block delta.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockDelta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    SignatureDelta {
        signature: String,
    },
    /// A delta this client does not model; ignored.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageDeltaBody {
    #[serde(default)]
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageBody {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl ErrorBody {
    fn describe(&self) -> String {
        let kind = self.r#type.as_deref().unwrap_or("error");
        let message = self.message.as_deref().unwrap_or("no message");
        format!("{kind}: {message}")
    }
}

/// A tool call being assembled, keyed by its content-block index.
#[derive(Debug, Clone)]
struct PartialCall {
    id: ToolCallId,
    provider_id: Option<String>,
    name: String,
    arguments: String,
}

impl PartialCall {
    fn new() -> Self {
        Self {
            id: ToolCallId::new(),
            provider_id: None,
            name: String::new(),
            arguments: String::new(),
        }
    }
}

/// Turns event payloads into deltas, and the deltas into one message.
#[derive(Debug, Default)]
pub struct Assembler {
    text: String,
    thinking: String,
    signature: Option<String>,
    calls: BTreeMap<usize, PartialCall>,
    /// The open tool-use block index, so `content_block_stop` knows what to close.
    open_call: Option<usize>,
    usage: Usage,
    has_usage: bool,
    stop_reason: Option<String>,
    completed: bool,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The stop reason the provider reported, if any.
    pub fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    /// Read one event payload.
    ///
    /// Any payload after `message_stop` is ignored, so a trailing event does not
    /// produce a second message.
    pub fn on_payload(&mut self, payload: &str) -> Result<Vec<Delta>, ProviderError> {
        if self.completed {
            return Ok(Vec::new());
        }

        let event: Event = serde_json::from_str(payload).map_err(|error| {
            ProviderError::Malformed(format!("{error} in {}", preview(payload)))
        })?;

        let mut deltas = Vec::new();
        match event {
            Event::MessageStart { message } => {
                if let Some(usage) = &message.usage {
                    self.usage.input = usage.input_tokens;
                    self.usage.cached = usage.cache_read_input_tokens;
                    self.usage.output = usage.output_tokens;
                    self.has_usage = true;
                }
            }
            Event::ContentBlockStart {
                index,
                content_block,
            } => {
                if let BlockStart::ToolUse { id, name, .. } = content_block {
                    if !name.is_empty() {
                        let entry = self.calls.entry(index).or_insert_with(PartialCall::new);
                        if !id.is_empty() {
                            entry.provider_id = Some(id);
                        }
                        entry.name = name;
                        self.open_call = Some(index);
                        deltas.push(Delta::ToolCallStart {
                            index,
                            id: entry.id,
                            name: entry.name.clone(),
                        });
                    }
                }
            }
            Event::ContentBlockDelta { index, delta } => {
                self.absorb_delta(index, delta, &mut deltas);
            }
            Event::ContentBlockStop { index } => {
                if self.open_call == Some(index) {
                    self.open_call = None;
                    deltas.push(Delta::ToolCallEnd { index });
                }
            }
            Event::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason {
                    self.stop_reason = Some(reason);
                }
                if let Some(usage) = usage {
                    // Cumulative (A8): replace, never add.
                    self.usage.input = self.usage.input.max(usage.input_tokens);
                    self.usage.cached = self.usage.cached.max(usage.cache_read_input_tokens);
                    self.usage.output = usage.output_tokens;
                    self.has_usage = true;
                    deltas.push(Delta::Usage(self.usage));
                }
            }
            Event::MessageStop => {
                self.close_open_call(&mut deltas);
                self.completed = true;
                deltas.push(Delta::Finished(self.assemble()));
            }
            Event::Ping => {}
            Event::Other => {}
            Event::Error { error } => {
                return Err(ProviderError::Reported(error.describe()));
            }
        }

        Ok(deltas)
    }

    /// The stream ended. Finish the message only if `message_stop` arrived or a
    /// stop reason was recorded; otherwise the message is truncated.
    pub fn on_eof(&mut self) -> Result<Vec<Delta>, ProviderError> {
        if self.completed {
            return Ok(Vec::new());
        }
        if self.stop_reason.is_none() {
            return Err(ProviderError::StreamClosed);
        }
        let mut deltas = Vec::new();
        self.close_open_call(&mut deltas);
        self.completed = true;
        deltas.push(Delta::Finished(self.assemble()));
        Ok(deltas)
    }

    fn absorb_delta(&mut self, index: usize, delta: BlockDelta, deltas: &mut Vec<Delta>) {
        match delta {
            BlockDelta::TextDelta { text } => {
                if !text.is_empty() {
                    self.text.push_str(&text);
                    deltas.push(Delta::Text(text));
                }
            }
            BlockDelta::ThinkingDelta { thinking } => {
                if !thinking.is_empty() {
                    if self.thinking.is_empty() {
                        tracing::info!("model started reasoning");
                    }
                    self.thinking.push_str(&thinking);
                    deltas.push(Delta::Reasoning(thinking));
                }
            }
            BlockDelta::InputJsonDelta { partial_json } => {
                if !partial_json.is_empty() {
                    let entry = self.calls.entry(index).or_insert_with(PartialCall::new);
                    entry.arguments.push_str(&partial_json);
                    deltas.push(Delta::ToolCallArgs {
                        index,
                        fragment: partial_json,
                    });
                }
            }
            BlockDelta::SignatureDelta { signature } => {
                self.signature = Some(signature);
            }
            BlockDelta::Other => {}
        }
    }

    fn close_open_call(&mut self, deltas: &mut Vec<Delta>) {
        if let Some(index) = self.open_call.take() {
            deltas.push(Delta::ToolCallEnd { index });
        }
    }

    /// Build the assistant message the loop appends.
    fn assemble(&mut self) -> Message {
        let calls: Vec<ToolCall> = self
            .calls
            .values()
            .map(|partial| {
                let mut call = match parse_arguments(&partial.arguments) {
                    Ok(args) => ToolCall::new(partial.name.clone(), args),
                    Err(error) => ToolCall::with_args_error(partial.name.clone(), error),
                };
                call.id = partial.id;
                call.provider_call_id = partial.provider_id.clone();
                call
            })
            .collect();

        let mut message = Message::assistant_with_tool_calls(std::mem::take(&mut self.text), calls);
        if self.has_usage {
            message = message.with_usage(self.usage);
        }
        if !self.thinking.is_empty() || self.signature.is_some() {
            message = message.with_reasoning(ReasoningTrace {
                text: std::mem::take(&mut self.thinking),
                signature: self.signature.take(),
            });
        }
        message
    }
}

/// Parse assembled tool arguments.
///
/// Empty arguments mean the model sent none, which is `{}`. Anything else must
/// parse: defaulting a malformed argument set to `{}` would run a tool on values
/// the model never chose.
fn parse_arguments(raw: &str) -> Result<serde_json::Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(trimmed)
        .map_err(|error| format!("could not parse tool arguments: {error}"))
}

/// The first part of a payload, for an error message.
fn preview(payload: &str) -> String {
    const LIMIT: usize = 200;
    if payload.len() <= LIMIT {
        return payload.to_owned();
    }
    let mut cut = LIMIT;
    while cut > 0 && !payload.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &payload[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::message::Role;

    fn run(payloads: &[&str]) -> (Vec<Delta>, Message) {
        let mut assembler = Assembler::new();
        let mut deltas = Vec::new();
        for payload in payloads {
            deltas.extend(assembler.on_payload(payload).expect("payload parses"));
        }
        let finished = deltas
            .iter()
            .find_map(|delta| match delta {
                Delta::Finished(message) => Some(message.clone()),
                _ => None,
            })
            .expect("a finished message");
        (deltas, finished)
    }

    #[test]
    fn text_deltas_become_text_and_one_message() {
        let (deltas, message) = run(&[
            r#"{"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#,
            r#"{"type":"message_stop"}"#,
        ]);
        assert!(deltas.contains(&Delta::Text("Hel".to_owned())));
        assert!(deltas.contains(&Delta::Text("lo".to_owned())));
        assert_eq!(message.role, Role::Assistant);
        assert_eq!(message.content, "Hello");
        assert!(!message.has_tool_calls());
        assert_eq!(
            message.usage,
            Some(Usage {
                input: 25,
                output: 15,
                cached: 0
            })
        );
    }

    #[test]
    fn thinking_deltas_reach_the_delta_and_round_trip_with_a_signature() {
        // A3: the thinking trace must be recoverable, with its signature, so the
        // next request can echo it back.
        let (deltas, message) = run(&[
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"considering"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-abc"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            r#"{"type":"message_stop"}"#,
        ]);
        assert!(deltas.contains(&Delta::Reasoning("considering".to_owned())));
        assert_eq!(message.content, "answer");
        let trace = message.reasoning.expect("a reasoning trace is kept");
        assert_eq!(trace.text, "considering");
        assert_eq!(trace.signature.as_deref(), Some("sig-abc"));
    }

    #[test]
    fn tool_use_blocks_assemble_with_matching_ids() {
        let (deltas, message) = run(&[
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"a.rs\"}"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":89}}"#,
            r#"{"type":"message_stop"}"#,
        ]);

        let (id, name) = deltas
            .iter()
            .find_map(|delta| match delta {
                Delta::ToolCallStart { id, name, .. } => Some((*id, name.clone())),
                _ => None,
            })
            .expect("a start was emitted");
        assert_eq!(name, "read_file");

        assert_eq!(message.tool_calls.len(), 1);
        let call = &message.tool_calls[0];
        assert_eq!(call.id, id, "the delta and the transcript name one call");
        assert_eq!(call.args["path"], "a.rs");
        assert_eq!(call.provider_call_id.as_deref(), Some("toolu_1"));
        assert!(call.args_error.is_none());

        // The arguments arrive before the closing marker.
        let end = deltas
            .iter()
            .position(|delta| matches!(delta, Delta::ToolCallEnd { .. }))
            .expect("the call was closed");
        let last_args = deltas
            .iter()
            .rposition(|delta| matches!(delta, Delta::ToolCallArgs { .. }))
            .expect("arguments streamed");
        assert!(last_args < end);
    }

    #[test]
    fn cumulative_usage_replaces_rather_than_adds() {
        // A8: message_delta usage is cumulative.
        let (_, message) = run(&[
            r#"{"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":30}}"#,
            r#"{"type":"message_stop"}"#,
        ]);
        assert_eq!(
            message.usage.expect("usage arrived").output,
            30,
            "the second report replaces the first, it does not add to it"
        );
    }

    #[test]
    fn an_error_event_is_reported() {
        let mut assembler = Assembler::new();
        let error = assembler
            .on_payload(
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            )
            .expect_err("an error event is refused");
        assert!(matches!(error, ProviderError::Reported(_)), "{error:?}");
        assert!(error.to_string().contains("Overloaded"));
    }

    #[test]
    fn a_ping_is_ignored() {
        let mut assembler = Assembler::new();
        assert!(assembler
            .on_payload(r#"{"type":"ping"}"#)
            .expect("a ping is fine")
            .is_empty());
    }

    #[test]
    fn a_payload_after_message_stop_is_ignored() {
        let mut assembler = Assembler::new();
        assembler
            .on_payload(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#)
            .expect("content is fine");
        assembler
            .on_payload(r#"{"type":"message_stop"}"#)
            .expect("stop is fine");
        assert!(assembler
            .on_payload(r#"{"type":"ping"}"#)
            .expect("ignored")
            .is_empty());
    }

    #[test]
    fn a_stream_that_stops_without_a_stop_reason_is_not_settled() {
        let mut assembler = Assembler::new();
        assembler
            .on_payload(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}"#)
            .expect("content is fine");
        let error = assembler.on_eof().expect_err("no stop reason");
        assert_eq!(error, ProviderError::StreamClosed);
    }

    #[test]
    fn a_stream_that_stops_after_a_stop_reason_is_settled() {
        let mut assembler = Assembler::new();
        assembler
            .on_payload(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"complete"}}"#)
            .expect("content is fine");
        assembler
            .on_payload(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#)
            .expect("stop reason is fine");
        let deltas = assembler.on_eof().expect("the message is settled");
        assert!(matches!(deltas.as_slice(), [Delta::Finished(_)]));
    }

    #[test]
    fn malformed_arguments_fail_the_call_rather_than_defaulting() {
        let (_, message) = run(&[
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"{"type":"message_stop"}"#,
        ]);
        let call = &message.tool_calls[0];
        assert!(call.args_error.is_some());
        assert_eq!(call.args, serde_json::Value::Null);
    }

    #[test]
    fn an_unknown_event_type_is_skipped() {
        // Versioning: new event types may be added, and an unknown one must not
        // kill a turn.
        let mut assembler = Assembler::new();
        assert!(assembler
            .on_payload(r#"{"type":"something_new","value":1}"#)
            .expect("an unknown type is ignored")
            .is_empty());
    }

    #[test]
    fn a_malformed_payload_is_reported_with_a_preview() {
        let mut assembler = Assembler::new();
        let error = assembler
            .on_payload("not json at all")
            .expect_err("a malformed payload is refused");
        assert!(matches!(error, ProviderError::Malformed(_)), "{error:?}");
    }
}

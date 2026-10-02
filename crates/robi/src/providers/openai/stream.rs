//! The response side: a stream of chunks becomes a stream of deltas, then one
//! assembled assistant message.
//!
//! The assembler owns three pieces of state that the caller must not have to
//! reason about: the partial text, the reasoning trace, and the tool calls keyed by
//! their stream-local index. Tool calls are held by index because the provider
//! sends a call's name and its argument fragments across several chunks, and the
//! arguments are not valid JSON until the last fragment lands.

use std::collections::BTreeMap;

use robi_core::ids::ToolCallId;
use robi_core::message::{Message, ToolCall, Usage};
use robi_core::model::Delta;
use serde::Deserialize;

use crate::providers::error::ProviderError;

/// The sentinel that ends a stream.
const DONE: &str = "[DONE]";

/// One chunk of a streamed completion.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default)]
    pub usage: Option<UsageChunk>,
    #[serde(default)]
    pub error: Option<ErrorChunk>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChunkChoice {
    #[serde(default)]
    pub delta: ChunkDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
    /// The spelling most providers use.
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// The other spelling. Which one arrives depends on the model.
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ChunkToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChunkToolCall {
    /// Absent on some OpenAI-compatible servers. Assumed to be `0` then.
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<ChunkFunction>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChunkFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageChunk {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

/// An error object, which a provider can send inside a 200 stream.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorChunk {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
}

impl ErrorChunk {
    fn describe(&self) -> String {
        let kind = self.r#type.as_deref().unwrap_or("error");
        let message = self.message.as_deref().unwrap_or("no message");
        match &self.code {
            Some(code) if !code.is_null() => format!("{kind} ({code}): {message}"),
            _ => format!("{kind}: {message}"),
        }
    }
}

/// A tool call being assembled.
#[derive(Debug, Clone)]
struct PartialCall {
    /// Minted when the call first appears, so the delta events and the finished
    /// message carry the same id.
    id: ToolCallId,
    /// The id the provider issued, echoed on the next request.
    provider_id: Option<String>,
    name: String,
    arguments: String,
    /// Whether `ToolCallStart` has been emitted for this call.
    started: bool,
}

impl PartialCall {
    /// A fresh call, with its id minted on first sight so the delta that announces
    /// it and the transcript entry agree.
    fn new() -> Self {
        Self {
            id: ToolCallId::new(),
            provider_id: None,
            name: String::new(),
            arguments: String::new(),
            started: false,
        }
    }
}

/// Turns payloads into deltas, and the deltas into one message.
#[derive(Debug, Default)]
pub struct Assembler {
    text: String,
    reasoning: String,
    calls: BTreeMap<usize, PartialCall>,
    usage: Option<Usage>,
    /// The index whose `ToolCallStart` is still open, so `ToolCallEnd` knows what
    /// to close.
    open_index: Option<usize>,
    finish_reason: Option<String>,
    completed: bool,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The finish reason the provider reported, if any.
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    /// The reasoning trace, which never enters the transcript.
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Read one payload.
    ///
    /// Any payload after the message finished is ignored, so a `[DONE]` following
    /// a `finish_reason` does not produce a second message.
    pub fn on_payload(&mut self, payload: &str) -> Result<Vec<Delta>, ProviderError> {
        if self.completed {
            return Ok(Vec::new());
        }

        if payload == DONE {
            let mut deltas = Vec::new();
            self.close_open_call(&mut deltas);
            self.completed = true;
            deltas.push(Delta::Finished(self.assemble()));
            return Ok(deltas);
        }

        let chunk: ChatChunk = serde_json::from_str(payload).map_err(|error| {
            ProviderError::Malformed(format!("{error} in {}", preview(payload)))
        })?;

        if let Some(error) = chunk.error {
            return Err(ProviderError::Reported(error.describe()));
        }

        let mut deltas = Vec::new();
        for choice in &chunk.choices {
            self.absorb_delta(&choice.delta, &mut deltas)?;
            if let Some(reason) = &choice.finish_reason {
                // Recorded, not acted on: a usage report can still arrive after it.
                self.finish_reason = Some(reason.clone());
                self.close_open_call(&mut deltas);
            }
        }

        if let Some(usage) = &chunk.usage {
            let usage = Usage {
                input: usage.prompt_tokens,
                output: usage.completion_tokens,
                cached: usage
                    .prompt_tokens_details
                    .as_ref()
                    .map(|details| details.cached_tokens)
                    .unwrap_or(0),
            };
            self.usage = Some(usage);
            deltas.push(Delta::Usage(usage));
        }

        Ok(deltas)
    }

    /// The stream ended. Finish the message only if the provider said it had.
    ///
    /// A stream that stops with neither `[DONE]` nor a finish reason did not
    /// complete, and treating it as complete would append a truncated message.
    pub fn on_eof(&mut self) -> Result<Vec<Delta>, ProviderError> {
        if self.completed {
            return Ok(Vec::new());
        }
        if self.finish_reason.is_none() {
            return Err(ProviderError::StreamClosed);
        }
        let mut deltas = Vec::new();
        self.close_open_call(&mut deltas);
        self.completed = true;
        deltas.push(Delta::Finished(self.assemble()));
        Ok(deltas)
    }

    fn absorb_delta(
        &mut self,
        delta: &ChunkDelta,
        deltas: &mut Vec<Delta>,
    ) -> Result<(), ProviderError> {
        if let Some(content) = &delta.content {
            if !content.is_empty() {
                self.text.push_str(content);
                deltas.push(Delta::Text(content.clone()));
            }
        }

        // Read both spellings; which one a model uses is not something this client
        // gets to assume.
        let reasoning = delta
            .reasoning_content
            .as_deref()
            .or(delta.reasoning.as_deref());
        if let Some(reasoning) = reasoning {
            if !reasoning.is_empty() {
                self.reasoning.push_str(reasoning);
                deltas.push(Delta::Reasoning(reasoning.to_owned()));
            }
        }

        for tool_call in &delta.tool_calls {
            let index = tool_call.index.unwrap_or(0);

            // A payload that moves to a new index ends the previous call.
            if let Some(open) = self.open_index {
                if open != index {
                    deltas.push(Delta::ToolCallEnd { index: open });
                }
            }

            // Record the provider's id, refusing a second distinct call at one
            // index: that is what a server omitting `index` looks like, and
            // silently merging two calls would lose one.
            if let Some(provider_id) = &tool_call.id {
                let entry = self.calls.entry(index).or_insert_with(PartialCall::new);
                match &entry.provider_id {
                    Some(existing) if existing != provider_id => {
                        return Err(ProviderError::Malformed(format!(
                            "two tool calls reported at index {index}: '{existing}' and \
                             '{provider_id}'"
                        )));
                    }
                    _ => entry.provider_id = Some(provider_id.clone()),
                }
            }

            // Emit the start as soon as the call has a name, before its arguments,
            // so a UI can show what is about to happen.
            let started = {
                let entry = self.calls.entry(index).or_insert_with(PartialCall::new);
                if let Some(name) = tool_call.function.as_ref().and_then(|f| f.name.as_ref()) {
                    if !name.is_empty() {
                        entry.name = name.clone();
                    }
                }
                if !entry.started && !entry.name.is_empty() {
                    entry.started = true;
                    Some((entry.id, entry.name.clone()))
                } else {
                    None
                }
            };
            if let Some((id, name)) = started {
                deltas.push(Delta::ToolCallStart { index, id, name });
            }

            if let Some(fragment) = tool_call
                .function
                .as_ref()
                .and_then(|function| function.arguments.as_ref())
            {
                if !fragment.is_empty() {
                    self.calls
                        .entry(index)
                        .or_insert_with(PartialCall::new)
                        .arguments
                        .push_str(fragment);
                    deltas.push(Delta::ToolCallArgs {
                        index,
                        fragment: fragment.clone(),
                    });
                }
            }

            self.open_index = Some(index);
        }

        Ok(())
    }

    fn close_open_call(&mut self, deltas: &mut Vec<Delta>) {
        if let Some(index) = self.open_index.take() {
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
                // Keep the id the `ToolCallStart` delta announced, so an event and
                // the transcript name the same call.
                call.id = partial.id;
                call.provider_call_id = partial.provider_id.clone();
                call
            })
            .collect();

        let mut message = Message::assistant_with_tool_calls(std::mem::take(&mut self.text), calls);
        if let Some(usage) = self.usage {
            message = message.with_usage(usage);
        }
        message
    }
}

/// Parse assembled tool arguments.
///
/// Empty arguments mean the model sent none, which is `{}`. Anything else must
/// parse: defaulting a malformed argument set to `{}` would run a tool on values
/// the model never chose, which for `read_file` or `grep` is a silently wrong call.
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

    /// Feed payloads, returning the deltas and the assembled message.
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

    fn text_delta(content: &str) -> String {
        format!(r#"{{"choices":[{{"delta":{{"content":"{content}"}}}}]}}"#)
    }

    #[test]
    fn text_becomes_text_deltas_and_one_message() {
        let hel = text_delta("Hel");
        let lo = text_delta("lo");
        let payloads = [
            hel.as_str(),
            lo.as_str(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ];
        let (deltas, message) = run(&payloads);

        assert!(deltas.contains(&Delta::Text("Hel".to_owned())));
        assert!(deltas.contains(&Delta::Text("lo".to_owned())));
        assert_eq!(message.role, Role::Assistant);
        assert_eq!(message.content, "Hello");
        assert!(!message.has_tool_calls());
    }

    #[test]
    fn both_reasoning_spellings_are_read() {
        let (deltas, _) = run(&[
            r#"{"choices":[{"delta":{"reasoning_content":"thinking"}}]}"#,
            r#"{"choices":[{"delta":{"reasoning":" harder"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ]);
        assert!(deltas.contains(&Delta::Reasoning("thinking".to_owned())));
        assert!(deltas.contains(&Delta::Reasoning(" harder".to_owned())));
    }

    #[test]
    fn reasoning_does_not_enter_the_message() {
        let (_, message) = run(&[
            r#"{"choices":[{"delta":{"reasoning_content":"secret"}}]}"#,
            r#"{"choices":[{"delta":{"content":"answer"}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            "[DONE]",
        ]);
        assert_eq!(message.content, "answer");
        assert!(!message.content.contains("secret"));
    }

    #[test]
    fn tool_call_fragments_assemble_in_order_with_matching_ids() {
        let (deltas, message) = run(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.rs\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);

        let started = deltas.iter().find_map(|delta| match delta {
            Delta::ToolCallStart { id, name, .. } => Some((*id, name.clone())),
            _ => None,
        });
        let (id, name) = started.expect("a start was emitted");
        assert_eq!(name, "read_file");

        assert_eq!(message.tool_calls.len(), 1);
        let call = &message.tool_calls[0];
        assert_eq!(call.id, id, "the delta and the transcript name one call");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.args["path"], "a.rs");
        assert_eq!(call.provider_call_id.as_deref(), Some("call_1"));
        assert!(call.args_error.is_none());

        // The argument fragments arrived before the end marker.
        let end = deltas
            .iter()
            .position(|delta| matches!(delta, Delta::ToolCallEnd { .. }))
            .expect("the call was closed");
        let last_args = deltas
            .iter()
            .rposition(|delta| matches!(delta, Delta::ToolCallArgs { .. }))
            .expect("arguments streamed");
        assert!(last_args < end, "the call closes after its arguments");
    }

    #[test]
    fn a_missing_index_is_treated_as_one_call() {
        let (_, message) = run(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"read_file","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        assert_eq!(message.tool_calls.len(), 1);
        assert_eq!(message.tool_calls[0].name, "read_file");
    }

    #[test]
    fn two_distinct_calls_at_one_index_are_refused() {
        // A server that omits `index` and sends two calls would otherwise merge
        // them and silently drop one.
        let mut assembler = Assembler::new();
        assembler
            .on_payload(
                r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"a","arguments":"{}"}}]}}]}"#,
            )
            .expect("the first call is fine");
        let error = assembler
            .on_payload(
                r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_2","function":{"name":"b","arguments":"{}"}}]}}]}"#,
            )
            .expect_err("a second call at the same index is refused");
        assert!(matches!(error, ProviderError::Malformed(_)), "{error:?}");
    }

    #[test]
    fn malformed_arguments_fail_the_call_rather_than_defaulting() {
        let (_, message) = run(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let call = &message.tool_calls[0];
        assert!(
            call.args_error.is_some(),
            "a truncated argument string must not become an empty object"
        );
        assert_eq!(call.args, serde_json::Value::Null);
    }

    #[test]
    fn a_usage_chunk_reaches_the_delta_and_the_message() {
        let hi = text_delta("hi");
        let (deltas, message) = run(&[
            hi.as_str(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":4}}}"#,
            "[DONE]",
        ]);

        let expected = Usage {
            input: 10,
            output: 2,
            cached: 4,
        };
        assert!(deltas.contains(&Delta::Usage(expected)));
        assert_eq!(message.usage, Some(expected));
    }

    #[test]
    fn a_usage_chunk_after_the_finish_reason_is_not_lost() {
        // The order is finish_reason, then usage, then [DONE]. Finishing early
        // would drop the accounting the context meter depends on.
        let hi = text_delta("hi");
        let (_, message) = run(&[
            hi.as_str(),
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":1}}"#,
            "[DONE]",
        ]);
        assert_eq!(message.usage.expect("usage arrived").input, 7);
    }

    #[test]
    fn an_error_object_mid_stream_is_reported() {
        let mut assembler = Assembler::new();
        let error = assembler
            .on_payload(r#"{"error":{"message":"upstream failed","type":"server_error"}}"#)
            .expect_err("an error object is refused");
        assert!(matches!(error, ProviderError::Reported(_)), "{error:?}");
        assert!(error.to_string().contains("upstream failed"));
    }

    #[test]
    fn a_payload_after_done_is_ignored() {
        let mut assembler = Assembler::new();
        assembler
            .on_payload(&text_delta("hi"))
            .expect("content is fine");
        let finished = assembler.on_payload("[DONE]").expect("done");
        assert_eq!(finished.len(), 1);
        assert!(assembler.on_payload("[DONE]").expect("ignored").is_empty());
    }

    #[test]
    fn a_stream_that_stops_without_a_finish_reason_is_not_settled() {
        let mut assembler = Assembler::new();
        assembler
            .on_payload(&text_delta("partial"))
            .expect("content is fine");
        let error = assembler.on_eof().expect_err("no finish reason");
        assert_eq!(error, ProviderError::StreamClosed);
    }

    #[test]
    fn a_stream_that_stops_after_a_finish_reason_is_settled() {
        // Some servers omit `[DONE]`. A recorded finish reason is enough.
        let mut assembler = Assembler::new();
        assembler
            .on_payload(&text_delta("complete"))
            .expect("content is fine");
        assembler
            .on_payload(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
            .expect("finish reason is fine");
        let deltas = assembler.on_eof().expect("the message is settled");
        assert!(matches!(deltas.as_slice(), [Delta::Finished(_)]));
    }

    #[test]
    fn a_malformed_payload_is_reported_with_a_preview() {
        let mut assembler = Assembler::new();
        let error = assembler
            .on_payload("not json at all")
            .expect_err("a malformed payload is refused");
        assert!(matches!(error, ProviderError::Malformed(_)), "{error:?}");
        assert!(error.to_string().contains("not json at all"));
    }

    #[test]
    fn empty_arguments_become_an_empty_object() {
        // A tool that takes no arguments is called with none, not with an error.
        let (_, message) = run(&[
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"status"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]);
        let call = &message.tool_calls[0];
        assert!(call.args_error.is_none());
        assert_eq!(call.args, serde_json::json!({}));
    }
}

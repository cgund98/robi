//! The request side: a transcript becomes an Anthropic Messages body.
//!
//! Rules this module owns, all asserted below:
//!
//! - **The system prompt is a top-level `system` field**, not a message (A1). The
//!   transcript has no `system` role, so it never appears in a session record.
//! - **`max_tokens` is required** by the Messages API and comes from the catalog's
//!   `max_output` (A5).
//! - **Tool results are `tool_result` blocks in a `user` message**, keyed by the
//!   `tool_use_id` the assistant turn issued. A `Role::Tool` message becomes one such
//!   user message (A4).
//! - **A thinking block is echoed** on the assistant turn whose tool results are
//!   still pending, with its signature, because a tool continuation requires it (A3).
//! - **The model id on the wire is the bare id** with the `ant_` prefix removed
//!   (A11).
//! - **`output_config.effort` is sent only when the catalog says the model accepts
//!   it** (A2). Haiku 4.5 rejects the field outright.

use std::collections::HashMap;
use std::sync::Arc;

use robi_core::ids::ToolCallId;
use robi_core::message::{Message, ReasoningTrace, Role, ToolCall};
use robi_core::tool::Tool;
use serde::Serialize;

use crate::agent::providers::config::ProviderSettings;
use crate::agent::providers::error::ProviderError;

/// The body of `POST /v1/messages`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    pub stream: bool,
    /// `{ "effort": "high" }`, only when the model supports it (A2/A12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputConfig {
    pub effort: String,
}

/// One message in the request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WireMessage {
    pub role: &'static str,
    pub content: WireContent,
}

/// The `content` of one message.
///
/// A plain string for the common text-only case, an array of blocks otherwise.
/// Anthropic accepts both spellings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WireContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// One element of a content-block array.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    Image {
        source: ImageSource,
    },
}

/// The base64 source of an image block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub media_type: String,
    pub data: String,
}

/// A tool definition. Anthropic uses `input_schema` at the top level, with no
/// `function` wrapper.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// The id of an attachment whose bytes the caller already resolved, so
/// `build_request` stays pure and needs no I/O (D11). Keyed by attachment id,
/// values are `(media_type, bytes)`.
pub type ResolvedImages = HashMap<String, (String, Vec<u8>)>;

/// The wire id of a tool call: the provider's own `tool_use_id` when stored, else
/// the local `ToolCallId`.
fn wire_call_id(call: &ToolCall) -> String {
    call.provider_call_id
        .clone()
        .unwrap_or_else(|| call.id.to_string())
}

/// The id sent on the wire for each call the transcript mentions.
pub fn wire_call_ids(transcript: &[Message]) -> HashMap<ToolCallId, String> {
    let mut ids = HashMap::new();
    for message in transcript {
        for call in &message.tool_calls {
            ids.insert(call.id, wire_call_id(call));
        }
    }
    ids
}

/// The text content of a user message, including any `@id` skill bodies and
/// attached file blocks.
fn user_text(message: &Message) -> String {
    if message.skills.is_empty() && message.files.is_empty() {
        return message.content.clone();
    }
    let mut content = message.content.clone();
    for skill in &message.skills {
        content.push_str(&crate::agent::skills::skill_block(skill));
    }
    for file in &message.files {
        content.push_str(&crate::agent::files::file_block(file));
    }
    content
}

/// The `input` object for one `tool_use` block.
fn tool_input(call: &ToolCall) -> serde_json::Value {
    match &call.args {
        serde_json::Value::Null => serde_json::json!({}),
        value => value.clone(),
    }
}

/// A user message: a plain string, or blocks when it carries images.
fn user_content(message: &Message, images: &ResolvedImages) -> Result<WireContent, MissingImage> {
    if message.images.is_empty() {
        return Ok(WireContent::Text(user_text(message)));
    }
    let mut blocks = vec![ContentBlock::Text {
        text: user_text(message),
    }];
    for attachment in &message.images {
        let (media_type, bytes) = images.get(&attachment.id).ok_or_else(|| MissingImage {
            id: attachment.id.clone(),
        })?;
        blocks.push(ContentBlock::Image {
            source: ImageSource {
                kind: "base64",
                media_type: media_type.clone(),
                data: base64_standard(bytes),
            },
        });
    }
    Ok(WireContent::Blocks(blocks))
}

/// An assistant turn: plain text, or blocks when it made a tool call or has a
/// thinking trace to preserve.
fn assistant_content(message: &Message) -> WireContent {
    let echo_thinking = message
        .reasoning
        .as_ref()
        .and_then(echoable_thinking)
        .filter(|_| !message.tool_calls.is_empty());

    if message.tool_calls.is_empty() && echo_thinking.is_none() {
        return WireContent::Text(message.content.clone());
    }

    let mut blocks = Vec::new();
    if let Some((thinking, signature)) = echo_thinking {
        blocks.push(ContentBlock::Thinking {
            thinking,
            signature,
        });
    }
    if !message.content.is_empty() {
        blocks.push(ContentBlock::Text {
            text: message.content.clone(),
        });
    }
    for call in &message.tool_calls {
        blocks.push(ContentBlock::ToolUse {
            id: wire_call_id(call),
            name: call.name.clone(),
            input: tool_input(call),
        });
    }
    WireContent::Blocks(blocks)
}

/// A thinking trace that can be echoed: it needs a signature to be accepted.
fn echoable_thinking(trace: &ReasoningTrace) -> Option<(String, String)> {
    trace
        .signature
        .as_ref()
        .map(|signature| (trace.text.clone(), signature.clone()))
}

/// A tool result: one `tool_result` block in a user message.
fn tool_result_content(message: &Message, wire_ids: &HashMap<ToolCallId, String>) -> WireContent {
    let tool_use_id = message
        .tool_call_id
        .map(|id| wire_ids.get(&id).cloned().unwrap_or_else(|| id.to_string()))
        .unwrap_or_default();
    WireContent::Blocks(vec![ContentBlock::ToolResult {
        tool_use_id,
        content: message.content.clone(),
    }])
}

/// Build the request for one model turn.
///
/// `images` is the caller's resolution of every attachment id in `transcript`
/// (D11); `build_request` stays pure. `supports_effort` comes from the catalog and
/// gates `output_config` (A2).
#[allow(clippy::too_many_arguments)]
pub fn build_request(
    settings: &ProviderSettings,
    tools: &[Arc<dyn Tool>],
    transcript: &[Message],
    images: &ResolvedImages,
    supports_effort: bool,
) -> Result<MessagesRequest, MissingImage> {
    let wire_ids = wire_call_ids(transcript);
    let mut messages = Vec::with_capacity(transcript.len());

    for message in transcript {
        messages.push(match message.role {
            Role::User => WireMessage {
                role: "user",
                content: user_content(message, images)?,
            },
            Role::Assistant => WireMessage {
                role: "assistant",
                content: assistant_content(message),
            },
            Role::Tool => WireMessage {
                role: "user",
                content: tool_result_content(message, &wire_ids),
            },
        });
    }

    let definitions: Vec<ToolDefinition> = tools
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.name().to_owned(),
            description: tool.description().to_owned(),
            input_schema: tool.parameters(),
        })
        .collect();

    let output_config = if supports_effort {
        settings.reasoning_effort.map(|effort| OutputConfig {
            effort: effort.as_str().to_owned(),
        })
    } else {
        None
    };

    Ok(MessagesRequest {
        model: settings.model.wire_id().to_owned(),
        // The API requires max_tokens; fall back to the catalog window when unset,
        // which only happens in a test that skipped the catalog wiring.
        max_tokens: settings.max_tokens.unwrap_or(8_192),
        system: (!settings.system_prompt.is_empty()).then(|| settings.system_prompt.clone()),
        messages,
        tools: (!definitions.is_empty()).then_some(definitions),
        stream: true,
        output_config,
    })
}

/// `base64` of the bytes, standard alphabet.
fn base64_standard(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A reference to an image the transcript lists but the caller did not resolve.
#[derive(Debug)]
pub struct MissingImage {
    pub id: String,
}

impl From<MissingImage> for ProviderError {
    fn from(error: MissingImage) -> Self {
        ProviderError::MissingImage { id: error.id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::providers::config::{ApiKey, ModelId, ReasoningEffort};
    use robi_core::message::{ImageAttachment, ReasoningTrace};

    fn settings() -> ProviderSettings {
        let mut settings =
            ProviderSettings::anthropic(ApiKey::new("k"), ModelId::new("ant_claude-sonnet-5-5"));
        settings.system_prompt = "You are Robi.".to_owned();
        settings.max_tokens = Some(64_000);
        settings
    }

    fn req(settings: &ProviderSettings, transcript: &[Message]) -> MessagesRequest {
        build_request(settings, &[], transcript, &ResolvedImages::default(), true)
            .expect("no test references a missing image")
    }

    #[test]
    fn the_system_prompt_is_top_level_and_the_model_is_bare() {
        let request = req(&settings(), &[Message::user("hi")]);
        assert_eq!(
            request.model, "claude-sonnet-5-5",
            "the ant_ prefix is stripped"
        );
        assert_eq!(request.system.as_deref(), Some("You are Robi."));
        assert_eq!(request.max_tokens, 64_000);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
        assert_eq!(
            request.messages[0].content,
            WireContent::Text("hi".to_owned())
        );
    }

    #[test]
    fn an_empty_system_prompt_is_omitted() {
        let mut settings = settings();
        settings.system_prompt = String::new();
        let request = req(&settings, &[Message::user("hi")]);
        assert!(request.system.is_none());
    }

    #[test]
    fn a_tool_call_becomes_a_tool_use_block_and_the_result_a_tool_result_block() {
        let call = ToolCall::new("read_file", serde_json::json!({"path": "a.rs"}))
            .with_provider_call_id("toolu_abc123");
        let assistant = Message::assistant_with_tool_calls("", vec![call.clone()]);
        let result = Message::tool_result(call.id, "contents");
        let request = req(&settings(), &[assistant, result]);

        let WireContent::Blocks(blocks) = &request.messages[0].content else {
            panic!("an assistant with a tool call uses blocks");
        };
        let ContentBlock::ToolUse { id, name, input } = &blocks[0] else {
            panic!("the first block is the tool use");
        };
        assert_eq!(id, "toolu_abc123");
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "a.rs");

        assert_eq!(
            request.messages[1].role, "user",
            "tool results are user messages"
        );
        let WireContent::Blocks(blocks) = &request.messages[1].content else {
            panic!("a tool result uses blocks");
        };
        let ContentBlock::ToolResult {
            tool_use_id,
            content,
        } = &blocks[0]
        else {
            panic!("the block is a tool result");
        };
        assert_eq!(
            tool_use_id, "toolu_abc123",
            "both halves agree on the provider id"
        );
        assert_eq!(content, "contents");
    }

    #[test]
    fn a_thinking_trace_is_echoed_on_the_tool_turn_with_its_signature() {
        // A3: a tool continuation requires the thinking block back, unmodified.
        let call =
            ToolCall::new("read_file", serde_json::json!({})).with_provider_call_id("toolu_1");
        let assistant = Message::assistant_with_tool_calls("", vec![call.clone()]).with_reasoning(
            ReasoningTrace {
                text: "let me read".to_owned(),
                signature: Some("sig-xyz".to_owned()),
            },
        );
        let result = Message::tool_result(call.id, "done");
        let request = req(&settings(), &[assistant, result]);

        let WireContent::Blocks(blocks) = &request.messages[0].content else {
            panic!("blocks");
        };
        let ContentBlock::Thinking {
            thinking,
            signature,
        } = &blocks[0]
        else {
            panic!("the thinking block comes first");
        };
        assert_eq!(thinking, "let me read");
        assert_eq!(signature, "sig-xyz");
    }

    #[test]
    fn a_thinking_trace_without_a_signature_is_not_echoed() {
        let call = ToolCall::new("read_file", serde_json::json!({}));
        let assistant =
            Message::assistant_with_tool_calls("", vec![call]).with_reasoning(ReasoningTrace {
                text: "no signature".to_owned(),
                signature: None,
            });
        let request = req(&settings(), &[assistant]);
        let WireContent::Blocks(blocks) = &request.messages[0].content else {
            panic!("blocks");
        };
        assert!(
            !blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::Thinking { .. })),
            "a thinking block with no signature cannot be sent back"
        );
    }

    #[test]
    fn a_text_only_assistant_turn_stays_a_string() {
        let request = req(&settings(), &[Message::assistant("answer")]);
        assert_eq!(
            request.messages[0].content,
            WireContent::Text("answer".to_owned())
        );
    }

    #[test]
    fn a_file_attachment_becomes_a_file_block_in_the_text() {
        let message =
            Message::user("please").with_files(vec![robi_core::message::FileAttachment {
                name: "error.rs".to_owned(),
                path: Some("src/error.rs".to_owned()),
                start_line: Some(29),
                end_line: Some(34),
                text: "boom".to_owned(),
            }]);
        let request = req(&settings(), &[message]);
        let WireContent::Text(text) = &request.messages[0].content else {
            panic!("a message with no images stays a string");
        };
        assert!(
            text.contains("<file name=\"error.rs\" path=\"src/error.rs\" lines=\"29-34\">"),
            "the file block carries its attributes: {text}"
        );
        assert!(text.contains("boom"), "the attached text is present");
    }

    #[test]
    fn images_become_base64_image_blocks() {
        let mut resolved = ResolvedImages::default();
        resolved.insert("img".to_owned(), ("image/png".to_owned(), b"PNG".to_vec()));
        let message = Message::user_with_images(
            "look",
            vec![ImageAttachment {
                id: "img".into(),
                media_type: "image/png".into(),
            }],
        );
        let request =
            build_request(&settings(), &[], &[message], &resolved, true).expect("resolves");
        let WireContent::Blocks(blocks) = &request.messages[0].content else {
            panic!("an image message uses blocks");
        };
        let ContentBlock::Image { source } = &blocks[1] else {
            panic!("the second block is the image");
        };
        assert_eq!(source.kind, "base64");
        assert_eq!(source.media_type, "image/png");
        assert_eq!(source.data, "UE5H");
    }

    #[test]
    fn a_missing_image_fails_the_build() {
        let message = Message::user_with_images(
            "look",
            vec![ImageAttachment {
                id: "ghost".into(),
                media_type: "image/png".into(),
            }],
        );
        let error = build_request(
            &settings(),
            &[],
            &[message],
            &ResolvedImages::default(),
            true,
        )
        .expect_err("the ghost id is not resolved");
        assert_eq!(error.id, "ghost");
    }

    #[test]
    fn effort_is_sent_when_supported_and_omitted_when_not() {
        // A2/A12: Haiku rejects output_config, so supports_effort gates it.
        let mut settings = settings();
        settings.reasoning_effort = Some(ReasoningEffort::High);

        let supported = build_request(
            &settings,
            &[],
            &[Message::user("hi")],
            &ResolvedImages::default(),
            true,
        )
        .expect("builds");
        assert_eq!(
            supported.output_config,
            Some(OutputConfig {
                effort: "high".to_owned()
            })
        );

        let unsupported = build_request(
            &settings,
            &[],
            &[Message::user("hi")],
            &ResolvedImages::default(),
            false,
        )
        .expect("builds");
        assert!(unsupported.output_config.is_none());
    }

    #[test]
    fn no_effort_means_no_output_config_even_when_supported() {
        let request = req(&settings(), &[Message::user("hi")]);
        assert!(request.output_config.is_none());
    }

    #[test]
    fn tool_definitions_use_input_schema_not_a_function_wrapper() {
        use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};

        struct Stub;
        #[async_trait::async_trait]
        impl Tool for Stub {
            fn name(&self) -> &str {
                "read_file"
            }
            fn description(&self) -> &str {
                "read a file"
            }
            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({"type": "object"})
            }
            fn concurrency(&self) -> Concurrency {
                Concurrency::Concurrent
            }
            async fn requires_approval(&self, _args: &serde_json::Value) -> ApprovalDecision {
                ApprovalDecision::AllowImmediately
            }
            async fn execute(
                &self,
                _args: serde_json::Value,
                _run: ToolRun,
            ) -> Result<serde_json::Value, robi_core::error::ToolError> {
                Ok(serde_json::Value::Null)
            }
        }

        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(Stub)];
        let request = build_request(
            &settings(),
            &tools,
            &[Message::user("hi")],
            &ResolvedImages::default(),
            true,
        )
        .expect("builds");
        let value = serde_json::to_value(&request).expect("serializes");
        let tool = &value["tools"][0];
        assert_eq!(tool["name"], "read_file");
        assert_eq!(tool["description"], "read a file");
        assert_eq!(tool["input_schema"]["type"], "object");
        assert!(tool.get("function").is_none(), "no function wrapper");
        assert_eq!(value["stream"], true);
    }
}

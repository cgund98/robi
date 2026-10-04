//! The request side: a transcript becomes a chat-completions body.
//!
//! Three rules are load-bearing and all are asserted in tests below:
//!
//! - **The system prompt is injected here.** The transcript has no `system` role,
//!   so it never appears in a session record.
//! - **The wire id of a tool call is the provider's own id when we have it, and the
//!   local `ToolCallId` otherwise.** The assistant entry and the tool result must
//!   agree, so both resolve through one map.
//! - **A user message with images becomes content parts (D12).** The text part
//!   ships first, then one `image_url` part per attachment, each a `data:` URI
//!   built from bytes the caller resolved. A text-only user message stays a plain
//!   string, so a body with no images is byte-identical to before.

use std::collections::HashMap;
use std::sync::Arc;

use robi_core::ids::ToolCallId;
use robi_core::message::{Message, Role, ToolCall};
use robi_core::tool::Tool;
use serde::Serialize;

use crate::agent::providers::config::ProviderSettings;
use crate::agent::providers::error::ProviderError;

/// The body of `POST /chat/completions`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// Ask for a usage report, which otherwise arrives only on the final chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// One message in the request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: WireContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OutgoingToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// The `content` of one chat message.
///
/// `Text` is a plain string, identical to what this adapter always sent — a
/// body without images serializes byte-for-byte the same as before, which keeps
/// prompt-cache prefixes and golden tests stable. `Parts` is the array shape
/// vision models need: the text part first, then one `image_url` part per
/// attachment. Only a `Role::User` message with images ever uses `Parts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WireContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// One element of a parts array.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

/// The `data:` URI carrying an image's bytes to the model.
///
/// `url` is `data:<media_type>;base64,<b64>`. We never send a remote `https`
/// URL: the desktop app has local files, not URLs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

/// The id of an attachment whose bytes the caller already resolved, so
/// `build_request` stays pure and needs no I/O (D11). Keyed by attachment id,
/// values are `(media_type, bytes)`.
pub type ResolvedImages = HashMap<String, (String, Vec<u8>)>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: OutgoingFunctionCall,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutgoingFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The id sent on the wire for each call the transcript mentions.
///
/// A tool-result message carries only the local `ToolCallId`, so this is the only
/// way to recover the id the provider will recognize. The lookup is needed whether
/// or not the provider issued its own id.
pub fn wire_call_ids(transcript: &[Message]) -> HashMap<ToolCallId, String> {
    let mut ids = HashMap::new();
    for message in transcript {
        for call in &message.tool_calls {
            ids.insert(call.id, wire_call_id(call));
        }
    }
    ids
}

/// The wire id for one call: the provider's own when we stored one, else the local
/// id.
fn wire_call_id(call: &ToolCall) -> String {
    call.provider_call_id
        .clone()
        .unwrap_or_else(|| call.id.to_string())
}

fn user_content(message: &Message) -> String {
    if message.skills.is_empty() {
        return message.content.clone();
    }
    let mut content = message.content.clone();
    for skill in &message.skills {
        content.push_str(&crate::agent::skills::skill_block(skill));
    }
    content
}

/// The `arguments` string for one call.
///
/// A call whose arguments never assembled sends `{}` rather than its error text:
/// the loop fails that call before execution, so the value exists only for the
/// provider to accept the transcript.
fn arguments_for(call: &ToolCall) -> String {
    match &call.args {
        serde_json::Value::Null => "{}".to_owned(),
        value => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()),
    }
}

/// The image content for a user message, or text as before when it has none.
///
/// A user message with attachments emits a parts array: the text first (its
/// content, even when empty), then one `image_url` part per attachment in attach
/// order. A message without images stays a plain string, byte-identical to what
/// this adapter always sent. Calls that return `Ok(None)` have no images.
fn user_wire_content(
    message: &Message,
    images: &ResolvedImages,
) -> Result<WireContent, MissingImage> {
    if message.images.is_empty() {
        return Ok(WireContent::Text(user_content(message)));
    }
    let mut parts = vec![ContentPart::Text {
        text: user_content(message),
    }];
    for attachment in &message.images {
        let (media_type, bytes) = images.get(&attachment.id).ok_or_else(|| MissingImage {
            id: attachment.id.clone(),
        })?;
        parts.push(ContentPart::ImageUrl {
            image_url: ImageUrl {
                url: data_uri(media_type, bytes),
            },
        });
    }
    Ok(WireContent::Parts(parts))
}

/// `data:<media_type>;base64,<b64>`.
fn data_uri(media_type: &str, bytes: &[u8]) -> String {
    // `base64_engine` with the standard alphabet; the engine is the stable API
    // since base64 0.22. The engine instance is cheap and stateless.
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{media_type};base64,{b64}")
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

/// Build the request for one model turn.
///
/// `images` is the caller's resolution of every attachment id in `transcript`
/// (D11); `build_request` stays pure. An id with no entry fails the turn — a
/// missing image is a corrupt store, never a silently dropped part.
pub fn build_request(
    settings: &ProviderSettings,
    tools: &[Arc<dyn Tool>],
    transcript: &[Message],
    images: &ResolvedImages,
) -> Result<ChatRequest, MissingImage> {
    let wire_ids = wire_call_ids(transcript);
    let mut messages = Vec::with_capacity(transcript.len() + 1);

    if !settings.system_prompt.is_empty() {
        messages.push(ChatMessage {
            role: "system",
            content: WireContent::Text(settings.system_prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in transcript {
        messages.push(match message.role {
            Role::User => ChatMessage {
                role: "user",
                content: user_wire_content(message, images)?,
                tool_calls: None,
                tool_call_id: None,
            },
            Role::Assistant => ChatMessage {
                role: "assistant",
                content: WireContent::Text(message.content.clone()),
                tool_calls: (!message.tool_calls.is_empty()).then(|| {
                    message
                        .tool_calls
                        .iter()
                        .map(|call| OutgoingToolCall {
                            id: wire_call_id(call),
                            kind: "function",
                            function: OutgoingFunctionCall {
                                name: call.name.clone(),
                                arguments: arguments_for(call),
                            },
                        })
                        .collect()
                }),
                tool_call_id: None,
            },
            Role::Tool => ChatMessage {
                role: "tool",
                content: WireContent::Text(message.content.clone()),
                tool_calls: None,
                tool_call_id: message
                    .tool_call_id
                    .map(|id| wire_ids.get(&id).cloned().unwrap_or_else(|| id.to_string())),
            },
        });
    }

    // `ToolRegistry::tools()` already sorts by name, so this array is byte-stable
    // across runs. An unstable one breaks prompt caching and makes golden tests
    // noisy.
    let definitions: Vec<ToolDefinition> = tools
        .iter()
        .map(|tool| ToolDefinition {
            kind: "function",
            function: FunctionDefinition {
                name: tool.name().to_owned(),
                description: tool.description().to_owned(),
                parameters: tool.parameters(),
            },
        })
        .collect();

    Ok(ChatRequest {
        model: settings.model.as_str().to_owned(),
        messages,
        stream: true,
        stream_options: StreamOptions {
            include_usage: true,
        },
        tools: (!definitions.is_empty()).then_some(definitions),
        reasoning_effort: settings
            .reasoning_effort
            .map(|effort| effort.as_str().to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::providers::config::{ApiKey, ModelId};
    use robi_core::message::ImageAttachment;

    fn settings() -> ProviderSettings {
        ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new("glm-5.3"))
            .with_system_prompt("You are Robi.")
    }

    /// `build_request` with no images and the result unwrapped. A test that
    /// exercises images calls `build_request` directly and handles the `Result`.
    fn req(
        settings: &ProviderSettings,
        tools: &[Arc<dyn Tool>],
        transcript: &[Message],
    ) -> ChatRequest {
        build_request(settings, tools, transcript, &ResolvedImages::default())
            .expect("no test references a missing image")
    }

    fn call_with_result(provider_id: Option<&str>) -> Vec<Message> {
        let mut call = ToolCall::new("read_file", serde_json::json!({"path": "a.rs"}));
        if let Some(id) = provider_id {
            call = call.with_provider_call_id(id);
        }
        let result = Message::tool_result(call.id, "contents");
        vec![Message::assistant_with_tool_calls("", vec![call]), result]
    }

    #[test]
    fn the_nothing_case_is_a_system_message_and_nothing_else() {
        let request = req(&settings(), &[], &[Message::user("hi")]);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(
            request.messages[0].content,
            WireContent::Text("You are Robi.".to_owned())
        );
        assert_eq!(request.messages[1].role, "user");
        assert!(request.tools.is_none(), "no tools means no tools array");
        assert!(request.reasoning_effort.is_none());
    }

    #[test]
    fn an_empty_system_prompt_is_omitted() {
        let mut settings = settings();
        settings.system_prompt = String::new();
        let request = req(&settings, &[], &[Message::user("hi")]);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
    }

    #[test]
    fn the_providers_own_id_is_used_when_we_have_one() {
        let transcript = call_with_result(Some("call_abc123"));
        let request = req(&settings(), &[], &transcript);

        let assistant = &request.messages[1];
        let tool = &request.messages[2];
        assert_eq!(
            assistant.tool_calls.as_ref().expect("calls are sent")[0].id,
            "call_abc123"
        );
        assert_eq!(
            tool.tool_call_id.as_deref(),
            Some("call_abc123"),
            "both halves of the pair must agree"
        );
    }

    #[test]
    fn the_local_id_is_used_when_the_provider_gave_none() {
        let transcript = call_with_result(None);
        let local = transcript[0].tool_calls[0].id.to_string();
        let request = req(&settings(), &[], &transcript);

        assert_eq!(
            request.messages[1].tool_calls.as_ref().expect("calls")[0].id,
            local
        );
        assert_eq!(request.messages[2].tool_call_id.as_deref(), Some(&*local));
    }

    #[test]
    fn tool_arguments_serialize_rather_than_defaulting() {
        let call = ToolCall::new("read_file", serde_json::json!({"path": "a.rs", "limit": 5}));
        let request = req(
            &settings(),
            &[],
            &[Message::assistant_with_tool_calls("", vec![call])],
        );
        let arguments = &request.messages[1].tool_calls.as_ref().expect("calls")[0]
            .function
            .arguments;
        let parsed: serde_json::Value = serde_json::from_str(arguments).expect("valid JSON");
        assert_eq!(parsed["limit"], 5);
    }

    #[test]
    fn a_call_whose_arguments_never_assembled_sends_an_empty_object() {
        // `args_error` means the loop fails this call before execution, so the body
        // only has to be something the provider accepts.
        let call = ToolCall::with_args_error("read_file", "unexpected end of JSON");
        let request = req(
            &settings(),
            &[],
            &[Message::assistant_with_tool_calls("", vec![call])],
        );
        assert_eq!(
            request.messages[1].tool_calls.as_ref().expect("calls")[0]
                .function
                .arguments,
            "{}"
        );
    }

    #[test]
    fn the_serialized_body_carries_the_stream_and_usage_flags() {
        let request = req(&settings(), &[], &[Message::user("hi")]);
        let value = serde_json::to_value(&request).expect("serializes");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["model"], "glm-5.3");
    }

    #[test]
    fn a_text_only_message_serializes_identically_with_and_without_resolved_images() {
        // D12: a body without images must be byte-identical to before, so prompt
        // caching and golden tests are unchanged.
        let text_only = req(&settings(), &[], &[Message::user("hi")]);
        let with_empty_map = build_request(
            &settings(),
            &[],
            &[Message::user("hi")],
            &ResolvedImages::default(),
        )
        .expect("a text-only message needs no resolution");
        assert_eq!(text_only, with_empty_map);

        let bytes = serde_json::to_vec(&text_only).expect("serializes");
        let again = serde_json::to_vec(&with_empty_map).expect("serializes");
        assert_eq!(bytes, again);
    }

    #[test]
    fn images_emit_parts_with_text_first_and_a_data_uri_each() {
        // D12: a user message with images becomes a parts array — the text first,
        // then one `image_url` part per attachment, each a `data:` URI built from
        // the resolved bytes.
        let mut resolved = ResolvedImages::default();
        resolved.insert(
            "img_1".to_owned(),
            ("image/png".to_owned(), b"\x89PNGABCD".to_vec()),
        );
        resolved.insert(
            "img_2".to_owned(),
            ("image/jpeg".to_owned(), b"\xff\xd8\xffE".to_vec()),
        );

        let mut transcript = Message::user_with_images(
            "look",
            vec![
                ImageAttachment {
                    id: "img_1".into(),
                    media_type: "image/png".into(),
                },
                ImageAttachment {
                    id: "img_2".into(),
                    media_type: "image/jpeg".into(),
                },
            ],
        );
        transcript.skills = Vec::new();

        let request =
            build_request(&settings(), &[], &[transcript], &resolved).expect("both images resolve");

        let message = &request.messages[1]; // after the injected system prompt
        let WireContent::Parts(parts) = &message.content else {
            panic!("an image-bearing user message must use content parts");
        };
        assert_eq!(
            parts.len(),
            3,
            "text part plus one part per image, in attach order"
        );

        let ContentPart::Text { text } = &parts[0] else {
            panic!("first part is the text");
        };
        assert_eq!(text, "look");

        let ContentPart::ImageUrl { image_url } = &parts[1] else {
            panic!("second part is the first image");
        };
        assert!(
            image_url.url.starts_with("data:image/png;base64,"),
            "the URI is a PNG data URI: {}",
            image_url.url
        );
        // The resolved bytes, base64-encoded.
        assert_eq!(image_url.url, "data:image/png;base64,iVBOR0FCQ0Q=");

        let ContentPart::ImageUrl { image_url } = &parts[2] else {
            panic!("third part is the second image");
        };
        assert!(image_url.url.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn a_missing_image_reference_fails_the_build_rather_than_dropping_the_part() {
        // D11: a transcript that references an id the caller did not resolve is a
        // corrupt store, not a silently skipped image.
        let transcript = Message::user_with_images(
            "describe",
            vec![ImageAttachment {
                id: "ghost".into(),
                media_type: "image/png".into(),
            }],
        );
        let error = build_request(&settings(), &[], &[transcript], &ResolvedImages::default())
            .expect_err("the ghost id is not resolved");
        assert_eq!(error.id, "ghost");
    }

    #[test]
    fn an_image_message_can_have_no_text_and_still_ship_a_text_part_first() {
        // The text part is always first, even when the content is empty.
        let mut resolved = ResolvedImages::default();
        resolved.insert("img".to_owned(), ("image/png".to_owned(), b"PNG".to_vec()));
        let transcript = Message::user_with_images(
            "",
            vec![ImageAttachment {
                id: "img".into(),
                media_type: "image/png".into(),
            }],
        );
        let request = build_request(&settings(), &[], &[transcript], &resolved).expect("resolves");
        let WireContent::Parts(parts) = &request.messages[1].content else {
            panic!("parts");
        };
        assert_eq!(parts.len(), 2);
        let ContentPart::Text { text } = &parts[0] else {
            panic!("text first");
        };
        assert_eq!(text, "");
    }
}

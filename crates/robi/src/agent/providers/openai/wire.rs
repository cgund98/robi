//! The request side: a transcript becomes a chat-completions body.
//!
//! Two rules are load-bearing and both are asserted in tests below:
//!
//! - **The system prompt is injected here.** The transcript has no `system` role,
//!   so it never appears in a session record.
//! - **The wire id of a tool call is the provider's own id when we have it, and the
//!   local `ToolCallId` otherwise.** The assistant entry and the tool result must
//!   agree, so both resolve through one map.

use std::collections::HashMap;
use std::sync::Arc;

use robi_core::ids::ToolCallId;
use robi_core::message::{Message, Role, ToolCall};
use robi_core::tool::Tool;
use serde::Serialize;

use crate::agent::providers::config::ProviderSettings;

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
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OutgoingToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

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

/// Build the request for one model turn.
pub fn build_request(
    settings: &ProviderSettings,
    tools: &[Arc<dyn Tool>],
    transcript: &[Message],
) -> ChatRequest {
    let wire_ids = wire_call_ids(transcript);
    let mut messages = Vec::with_capacity(transcript.len() + 1);

    if !settings.system_prompt.is_empty() {
        messages.push(ChatMessage {
            role: "system",
            content: settings.system_prompt.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for message in transcript {
        messages.push(match message.role {
            Role::User => ChatMessage {
                role: "user",
                content: user_content(message),
                tool_calls: None,
                tool_call_id: None,
            },
            Role::Assistant => ChatMessage {
                role: "assistant",
                content: message.content.clone(),
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
                content: message.content.clone(),
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

    ChatRequest {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::providers::config::{ApiKey, ModelId};

    fn settings() -> ProviderSettings {
        ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new("glm-5.3"))
            .with_system_prompt("You are Robi.")
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
        let request = build_request(&settings(), &[], &[Message::user("hi")]);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert_eq!(request.messages[0].content, "You are Robi.");
        assert_eq!(request.messages[1].role, "user");
        assert!(request.tools.is_none(), "no tools means no tools array");
        assert!(request.reasoning_effort.is_none());
    }

    #[test]
    fn an_empty_system_prompt_is_omitted() {
        let mut settings = settings();
        settings.system_prompt = String::new();
        let request = build_request(&settings, &[], &[Message::user("hi")]);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
    }

    #[test]
    fn the_providers_own_id_is_used_when_we_have_one() {
        let transcript = call_with_result(Some("call_abc123"));
        let request = build_request(&settings(), &[], &transcript);

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
        let request = build_request(&settings(), &[], &transcript);

        assert_eq!(
            request.messages[1].tool_calls.as_ref().expect("calls")[0].id,
            local
        );
        assert_eq!(request.messages[2].tool_call_id.as_deref(), Some(&*local));
    }

    #[test]
    fn tool_arguments_serialize_rather_than_defaulting() {
        let call = ToolCall::new("read_file", serde_json::json!({"path": "a.rs", "limit": 5}));
        let request = build_request(
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
        let request = build_request(
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
        let request = build_request(&settings(), &[], &[Message::user("hi")]);
        let value = serde_json::to_value(&request).expect("serializes");
        assert_eq!(value["stream"], true);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["model"], "glm-5.3");
    }
}

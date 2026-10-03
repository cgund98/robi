//! The transcript: messages, tool calls, and the status enums the loop branches on.
//!
//! The loop derives whether it is paused, runnable, or done from these values, so
//! a restart needs no separate state to reconstruct.

use serde::{Deserialize, Serialize};

use crate::ids::{MessageId, ToolCallId};

/// Who authored a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

/// Whether a tool call has been decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

impl ApprovalStatus {
    /// A call the user has decided. A `Rejected` call is settled without running.
    pub fn is_settled(self) -> bool {
        !matches!(self, ApprovalStatus::Pending)
    }
}

/// How far a tool call has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    NotStarted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl ExecutionStatus {
    /// A call that needs no further work.
    pub fn is_terminal(self) -> bool {
        !matches!(self, ExecutionStatus::NotStarted | ExecutionStatus::Running)
    }
}

/// Who truncated a tool result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Truncator {
    /// The tool bounded its own output. Expected.
    Tool,
    /// The core's backstop fired, which means a tool is missing its own bound.
    Core,
}

/// A record that a tool result was cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    pub by: Truncator,
    pub limit_bytes: usize,
}

/// Token accounting for one model turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

/// Which child a `delegate` call is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMode {
    Explore,
    General,
}

/// How far one child tool call has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStepStatus {
    Running,
    Ok,
    Denied,
    Failed,
}

/// One child tool call, as the parent card shows it.
///
/// `target` is the path, pattern, or command. The file body and the command
/// output stay in the child transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentStep {
    pub name: String,
    pub target: String,
    pub status: SubagentStepStatus,
}

/// The child run attached to a parent tool call.
///
/// This is what the UI renders. It is not the tool result the model reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentSnapshot {
    pub mode: SubagentMode,
    pub description: String,
    /// Unix time in milliseconds when the child started.
    pub started_ms: u64,
    #[serde(default)]
    pub steps: Vec<SubagentStep>,
}

/// One call the model asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
    /// Set by the provider adapter when the arguments could not be parsed.
    ///
    /// The call fails with a parse error rather than running on defaults. An
    /// empty object is a valid argument set for many tools, so defaulting would
    /// silently execute the wrong thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_error: Option<String>,
    pub approval_status: ApprovalStatus,
    pub execution_status: ExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
    /// The id the provider issued for this call, when it issued one.
    ///
    /// The wire id: a stateless request body must repeat what the provider handed
    /// out, so a later request echoes this. Not identity — the loop keys on `id`,
    /// so a provider id that is reused, absent, or malformed cannot confuse
    /// approval or lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_call_id: Option<String>,
    /// Child tool calls while a `delegate` run is in progress, and after it
    /// finishes. Absent on every other tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<SubagentSnapshot>,
}

impl ToolCall {
    /// A call awaiting a decision, which is how a model's request arrives.
    pub fn new(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            id: ToolCallId::new(),
            name: name.into(),
            args,
            args_error: None,
            approval_status: ApprovalStatus::Pending,
            execution_status: ExecutionStatus::NotStarted,
            result: None,
            error: None,
            truncation: None,
            provider_call_id: None,
            subagent: None,
        }
    }

    /// A call that could not be parsed and will fail without running.
    pub fn with_args_error(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            args_error: Some(error.into()),
            ..Self::new(name, serde_json::Value::Null)
        }
    }

    /// A call the user has already approved, as a test or a re-run supplies it.
    pub fn approved(mut self) -> Self {
        self.approval_status = ApprovalStatus::Approved;
        self
    }

    /// Record the id the provider issued, so a later request can echo it.
    pub fn with_provider_call_id(mut self, provider_call_id: impl Into<String>) -> Self {
        self.provider_call_id = Some(provider_call_id.into());
        self
    }

    pub fn is_pending_approval(&self) -> bool {
        self.approval_status == ApprovalStatus::Pending
    }

    pub fn needs_execution(&self) -> bool {
        !self.execution_status.is_terminal()
    }
}

/// One message in a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Set on a `Role::Tool` message, naming the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            usage: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            usage: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            tool_calls,
            ..Self::assistant(content)
        }
    }

    pub fn tool_result(tool_call_id: ToolCallId, content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id),
            usage: None,
        }
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// The message describes work that has not been answered yet.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Every call is approved, and none has been attempted.
    pub fn can_execute_tools(&self) -> bool {
        self.has_tool_calls()
            && self
                .tool_calls
                .iter()
                .all(|c| c.approval_status == ApprovalStatus::Approved)
            && self
                .tool_calls
                .iter()
                .all(|c| c.execution_status == ExecutionStatus::NotStarted)
    }

    /// No call is waiting on a decision.
    pub fn all_tool_calls_approval_settled(&self) -> bool {
        self.tool_calls
            .iter()
            .all(|c| c.approval_status.is_settled())
    }

    /// At least one call has unfinished work.
    pub fn has_unfinished_tool_calls(&self) -> bool {
        self.tool_calls.iter().any(ToolCall::needs_execution)
    }

    pub fn pending_approval_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.tool_calls.iter().filter(|c| c.is_pending_approval())
    }

    pub fn call_mut(&mut self, call_id: ToolCallId) -> Option<&mut ToolCall> {
        self.tool_calls.iter_mut().find(|c| c.id == call_id)
    }
}

/// The newest assistant message that still has unfinished tool work.
///
/// A turn is unresolved while any of its calls is not terminal. Scanning
/// backwards finds the newest such turn, which is the only one that can be
/// outstanding: an earlier turn is closed by a later user message.
pub fn unresolved_turn(messages: &[Message]) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.role == Role::Assistant && m.has_unfinished_tool_calls())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_has_no_provider_id_until_the_adapter_sets_one() {
        let call = ToolCall::new("read_file", serde_json::json!({}));
        assert_eq!(call.provider_call_id, None);
        assert_eq!(
            call.with_provider_call_id("call_abc123").provider_call_id,
            Some("call_abc123".to_owned())
        );
    }

    #[test]
    fn an_absent_provider_id_is_not_serialized() {
        let call = ToolCall::new("read_file", serde_json::json!({}));
        let json = serde_json::to_value(&call).expect("a call serializes");
        assert!(
            json.get("provider_call_id").is_none(),
            "a call with no provider id omits the field entirely"
        );
    }

    #[test]
    fn a_transcript_written_before_the_field_existed_still_deserializes() {
        // The shape `ToolCall` had before `provider_call_id` existed.
        let old = serde_json::json!({
            "id": MessageId::new(),
            "role": "assistant",
            "content": "reading",
            "tool_calls": [{
                "id": ToolCallId::new(),
                "name": "read_file",
                "args": {"path": "a.rs"},
                "approval_status": "approved",
                "execution_status": "not_started"
            }]
        });

        let message: Message = serde_json::from_value(old).expect("an older transcript loads");
        assert_eq!(message.tool_calls[0].provider_call_id, None);
        assert_eq!(message.tool_calls[0].subagent, None);
        assert_eq!(message.tool_calls[0].name, "read_file");
    }

    #[test]
    fn a_provider_id_does_not_change_identity() {
        // Two calls that share a provider id stay distinct, because the loop keys
        // on `id`. A provider that reused an id must not merge them.
        let a = ToolCall::new("read_file", serde_json::json!({}))
            .with_provider_call_id("call_duplicate");
        let b = ToolCall::new("read_file", serde_json::json!({}))
            .with_provider_call_id("call_duplicate");

        assert_ne!(a.id, b.id);
        let mut message = Message::assistant_with_tool_calls("", vec![a.clone(), b.clone()]);
        assert!(message.call_mut(a.id).is_some());
        assert!(message.call_mut(b.id).is_some());
        assert_eq!(message.tool_calls.len(), 2);
    }
}

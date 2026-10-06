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
    /// The originals-table id when compression replaced this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_id: Option<String>,
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
            original_id: None,
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

/// An image attached to a user message.
///
/// The transcript holds the reference; the bytes live in the store
/// (the session blob file), and the provider adapter resolves them when it builds a
/// request. A `data:` URI or the raw bytes never enter the transcript: they
/// would bloat the persisted `chat_messages.body`, the event stream, and every
/// `GET /messages`. The wire needs the data URI; the transcript does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// The image id in the session blob file.
    pub id: String,
    /// `image/png`, `image/jpeg`, `image/webp`, or `image/gif`.
    pub media_type: String,
}

/// A text file the user attached to a user message.
///
/// The client reads the bytes and sends them; the server never opens `path`.
/// That is what makes an attachment outside the workspace safe with no grant —
/// there is no server-side read to confine.
///
/// `path` is the **workspace-relative** path (`src/error.rs`) when the file is
/// inside the workspace, so the model can re-read it with `read_file`. It is
/// `None` when the file is outside the workspace (an upload, or a file with no
/// workspace location); such an attachment carries only its name, and the
/// provider block notes that it came from outside the workspace.
///
/// `start_line` and `end_line` are the 1-based inclusive range the slice came
/// from in the source, so the chip can read `filename (1-10)`. Both are `None`
/// for a whole-file attach or an upload. The transcript holds the text, not a
/// pointer, so a later request rebuilds the same prompt after the file on disk
/// has changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAttachment {
    /// Display name, e.g. `error.rs`.
    pub name: String,
    /// Workspace-relative path when the file is inside the workspace, so the
    /// model can re-fetch it. `None` when the file is outside the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based first line of the slice, when the attach was a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    /// 1-based last line of the slice, inclusive, when the attach was a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// The attached slice: the whole file, or the selected range.
    pub text: String,
}

/// The provider's reasoning trace for one assistant turn.
///
/// Most providers drop their reasoning and the loop never sees it again. Anthropic
/// is the exception: a tool continuation must echo the `thinking` block back,
/// unmodified, with its signature. The adapter sets this on the `Finished` message
/// when the provider requires it, and the request builder echoes it for the turn
/// whose tool results are still pending. Absent for every other message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningTrace {
    pub text: String,
    /// The integrity signature Anthropic returns with a thinking block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// A skill the host loaded because the user wrote `@id`.
///
/// The typed text stays in [`Message::content`]. The provider appends one
/// block per load. An old row has no field and deserializes as an empty list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLoad {
    pub id: String,
    pub description: String,
    pub directory: String,
    pub files: Vec<String>,
    pub body: String,
}

/// One message in a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<SkillLoad>,
    /// Images the user attached to this message. Ids, never bytes; the adapter
    /// resolves them when it builds a request. Only `Role::User` messages carry
    /// them. Old rows have no field and deserialize as an empty list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageAttachment>,
    /// Text files the user attached to this message. The slice's bytes are held
    /// here, not referenced by path, so a later request rebuilds the same
    /// prompt. Only `Role::User` messages carry them. Old rows have no field and
    /// deserialize as an empty list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Set on a `Role::Tool` message, naming the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<ToolCallId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// The provider's reasoning trace, when a later turn must echo it.
    ///
    /// Set on an assistant message the Anthropic adapter assembled. Old rows
    /// have no field and deserialize as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningTrace>,
    /// This message is a compaction summary, not something a person typed.
    ///
    /// The transcript paints a **Context compacted** divider on it. A later
    /// compact may include it in a new prefix; the flag does not protect it.
    /// Old rows have no field and deserialize as `false`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub compaction: bool,
}

/// Serde helper: omit `compaction` when it is `false`.
fn is_false(value: &bool) -> bool {
    !*value
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::User,
            content: content.into(),
            skills: Vec::new(),
            images: Vec::new(),
            files: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            usage: None,
            reasoning: None,
            compaction: false,
        }
    }

    /// A compaction summary: a `user` message the transcript marks as generated.
    ///
    /// A `user` role keeps the tail able to start with the user's real turn.
    pub fn summary(content: impl Into<String>) -> Self {
        Self {
            compaction: true,
            ..Self::user(content)
        }
    }

    /// A user message carrying images. The text part still ships first on the
    /// wire, so it reads like what the user said followed by what they showed.
    pub fn user_with_images(content: impl Into<String>, images: Vec<ImageAttachment>) -> Self {
        let user = Self::user(content);
        Self { images, ..user }
    }

    pub fn with_skills(mut self, skills: Vec<SkillLoad>) -> Self {
        self.skills = skills;
        self
    }

    /// Attach text files the user picked. Only meaningful on a user message.
    pub fn with_files(mut self, files: Vec<FileAttachment>) -> Self {
        self.files = files;
        self
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::Assistant,
            content: content.into(),
            skills: Vec::new(),
            images: Vec::new(),
            files: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            usage: None,
            reasoning: None,
            compaction: false,
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
            skills: Vec::new(),
            images: Vec::new(),
            files: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id),
            usage: None,
            reasoning: None,
            compaction: false,
        }
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Attach the provider's reasoning trace, so a tool continuation echoes it.
    pub fn with_reasoning(mut self, reasoning: ReasoningTrace) -> Self {
        self.reasoning = Some(reasoning);
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

    #[test]
    fn images_round_trip_with_the_message() {
        let images = vec![
            ImageAttachment {
                id: "img_1".to_owned(),
                media_type: "image/png".to_owned(),
            },
            ImageAttachment {
                id: "img_2".to_owned(),
                media_type: "image/jpeg".to_owned(),
            },
        ];
        let message = Message::user_with_images("look at these", images.clone());
        let json = serde_json::to_value(&message).expect("a message with images serializes");
        assert_eq!(
            json.get("images"),
            Some(&serde_json::json!([
                {"id": "img_1", "media_type": "image/png"},
                {"id": "img_2", "media_type": "image/jpeg"},
            ])),
            "the images reference both parts of the attachment"
        );

        let back: Message = serde_json::from_value(json).expect("it deserializes");
        assert_eq!(back.images, images);
        assert_eq!(back.content, "look at these");
    }

    #[test]
    fn a_message_without_images_omits_the_field() {
        let json = serde_json::to_value(Message::user("plain")).expect("it serializes");
        assert!(
            json.get("images").is_none(),
            "no images means no images field on the wire"
        );
    }

    #[test]
    fn files_round_trip_with_the_message() {
        let files = vec![
            FileAttachment {
                name: "error.rs".to_owned(),
                path: Some("src/error.rs".to_owned()),
                start_line: Some(29),
                end_line: Some(34),
                text: "the slice".to_owned(),
            },
            FileAttachment {
                name: "notes.txt".to_owned(),
                path: None,
                start_line: None,
                end_line: None,
                text: "uploaded bytes".to_owned(),
            },
        ];
        let message = Message::user("look at these").with_files(files.clone());
        let json = serde_json::to_value(&message).expect("a message with files serializes");
        assert_eq!(
            json.get("files"),
            Some(&serde_json::json!([
                {
                    "name": "error.rs",
                    "path": "src/error.rs",
                    "start_line": 29,
                    "end_line": 34,
                    "text": "the slice"
                },
                {"name": "notes.txt", "text": "uploaded bytes"}
            ])),
            "an upload omits path and range; a ranged attach keeps them"
        );

        let back: Message = serde_json::from_value(json).expect("it deserializes");
        assert_eq!(back.files, files);
    }

    #[test]
    fn a_message_without_files_omits_the_field() {
        let json = serde_json::to_value(Message::user("plain")).expect("it serializes");
        assert!(
            json.get("files").is_none(),
            "no files means no files field on the wire"
        );
    }

    #[test]
    fn a_transcript_written_before_files_existed_still_deserializes() {
        // The shape `Message` had before `files` existed.
        let old = serde_json::json!({
            "id": MessageId::new(),
            "role": "user",
            "content": "hi",
            "skills": [],
            "images": [],
        });

        let message: Message = serde_json::from_value(old).expect("an older transcript loads");
        assert!(message.files.is_empty());
        assert_eq!(message.content, "hi");
    }

    #[test]
    fn a_transcript_written_before_images_existed_still_deserializes() {
        // The shape `Message` had before `images` existed, plus the newer fields.
        let old = serde_json::json!({
            "id": MessageId::new(),
            "role": "user",
            "content": "hi",
            "skills": [],
        });

        let message: Message = serde_json::from_value(old).expect("an older transcript loads");
        assert!(message.images.is_empty());
        assert_eq!(message.content, "hi");
    }

    #[test]
    fn a_transcript_written_before_compaction_existed_still_deserializes() {
        // The shape `Message` had before `compaction` existed.
        let old = serde_json::json!({
            "id": MessageId::new(),
            "role": "user",
            "content": "hi",
        });

        let message: Message = serde_json::from_value(old).expect("an older transcript loads");
        assert!(!message.compaction, "an absent flag reads as false");
    }

    #[test]
    fn a_summary_is_a_user_message_marked_as_compaction() {
        let summary = Message::summary("decisions so far");
        assert_eq!(summary.role, Role::User);
        assert!(summary.compaction);
        let json = serde_json::to_value(&summary).expect("a summary serializes");
        assert_eq!(json["compaction"], serde_json::json!(true));
    }

    #[test]
    fn an_ordinary_message_omits_the_compaction_flag() {
        let json = serde_json::to_value(Message::user("plain")).expect("it serializes");
        assert!(
            json.get("compaction").is_none(),
            "a message that is not a summary omits the field"
        );
    }
}

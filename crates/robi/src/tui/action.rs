//! The effects `update` asks for, and what came back.
//!
//! `update` never performs I/O. Each `Action` names one service call;
//! `effects::run` is the only place a `.await` exists. An outcome comes back as
//! `Msg::Done`, so the loop stays one state machine.
//!
//! # Generations
//!
//! Two refetches of one thing can be in flight together, so the ones that can
//! land out of order carry the generation they were issued at:
//!
//! - [`Action::Reload`] reads the whole transcript. It bumps the generation and
//!   applies only while that number is still current, so an older full read
//!   never overwrites a newer one.
//! - [`Action::LoadMessage`] reads one row. It carries the generation that was
//!   current when it was issued and applies only while it still matches, so a
//!   full reload supersedes it. Two per-row reads of the same id are harmless
//!   either way: applying a message is an upsert by id.
//! - The three list reads (workspaces, sessions, MCP marks) and the path
//!   completion carry their own counter, so a renamed session or a typed path
//!   cannot be answered by a read issued before it.
//!
//! A `Done` with a stale generation is dropped, never merged.

use robi_core::ids::{MessageId, SessionId, ToolCallId, WorkspaceId};
use robi_core::message::{FileAttachment, Message};

use crate::adapters::model_source::ModeDefaults;
use crate::agent::mcp::McpServerStatus;
use crate::agent::providers::ModelInfo;
use crate::domain::chat_message::runtime::SubmitOutcome;
use crate::domain::chat_session::model::{AgentMode, ChatSession, ModelConfig};
use crate::domain::workspace::model::Workspace;
use crate::tui::persist::ViewState;
use crate::tui::state::approval::Decision;
use crate::tui::state::completion::SkillEntry;
use crate::tui::state::models::Pick;

/// One effect `update` asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// List workspaces, to pick the active one.
    LoadWorkspaces { generation: u64 },
    /// List this workspace's sessions, newest first, with the live actors.
    LoadSessions {
        workspace: WorkspaceId,
        generation: u64,
    },
    /// Refetch the transcript of the open session.
    ///
    /// `generation` is the value the transcript held when the action was
    /// issued. Two reloads of one session can be in flight together, and the
    /// older one landing last must not win.
    Reload { session: SessionId, generation: u64 },
    /// Hand one instruction to the session's actor.
    ///
    /// T1 sends text only; T6 adds `files`, and images arrive with it. The
    /// vectors are here now so the ask does not change shape when they do.
    Submit {
        session: SessionId,
        text: String,
        files: Vec<FileAttachment>,
    },
    /// Approve or refuse one paused call, then let the turn resume.
    ///
    /// One call per action, because that is what the runtime settles: the next
    /// pause arrives as another `awaiting_approval` frame.
    Decide {
        session: SessionId,
        call: ToolCallId,
        decision: Decision,
    },
    /// Fetch the uncapped body behind a compressed result.
    ///
    /// Only an opened shell or MCP card asks for this, and only once: the answer
    /// is kept on the transcript for as long as the process lives.
    LoadOriginal {
        session: SessionId,
        call: ToolCallId,
        original_id: String,
    },
    /// Cancel the running turn.
    Stop { session: SessionId },
    /// Refetch one stored message, after a frame said that row changed.
    LoadMessage {
        session: SessionId,
        message: MessageId,
        generation: u64,
    },
    /// Open (or reuse) the workspace at `root`.
    ///
    /// `root` is what was typed; the effect expands `~` and relative paths
    /// against the working directory before the service sees it, so the typed
    /// text and the stored root are different things.
    CreateWorkspace { root: String },
    /// Remove a workspace and everything under it.
    DeleteWorkspace { workspace: WorkspaceId },
    /// Create a session in this workspace **and send the first message to it**.
    ///
    /// One action, because the two are one gesture: `Ctrl-N` creates nothing, so
    /// the row exists only once there is a message to put in it. A create that
    /// succeeded and a send that failed still leaves a session, which is why the
    /// outcome carries both.
    StartSession {
        workspace: WorkspaceId,
        text: String,
        files: Vec<FileAttachment>,
        /// The mode the chat is created in, and its choice.
        mode: AgentMode,
        model_config: ModelConfig,
    },
    /// Rename a session.
    RenameSession { session: SessionId, title: String },
    /// The models a turn can drive, and each mode's inheritable choice.
    LoadModels { generation: u64 },
    /// This workspace's user-invocable skills, for the `@` menu.
    LoadSkills {
        workspace: WorkspaceId,
        generation: u64,
    },
    /// Switch the session's mode.
    SetSessionMode { session: SessionId, mode: AgentMode },
    /// Set or clear one key of one mode's override.
    SetSessionChoice {
        session: SessionId,
        mode: AgentMode,
        pick: Pick,
    },
    /// Summarize an older prefix.
    Compact { session: SessionId },
    /// Stop the session's turn if one is running, then remove the row.
    DeleteSession { session: SessionId },
    /// This workspace's MCP servers, with connection status.
    LoadMcp {
        workspace: WorkspaceId,
        generation: u64,
    },
    /// Open this workspace's MCP servers and close the others, as selecting a
    /// workspace does in the GUI.
    FocusMcp { workspace: WorkspaceId },
    /// Ask the store whether it answers. The status strip's health mark.
    Probe,
    /// List the directories a typed path could complete to.
    CompletePath { text: String, generation: u64 },
    /// Persist the view state: on a change, and before the process exits.
    SaveView { view: ViewState },
}

/// What an effect produced.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    Workspaces {
        generation: u64,
        workspaces: Vec<Workspace>,
    },
    Sessions {
        workspace: WorkspaceId,
        generation: u64,
        sessions: Vec<ChatSession>,
        /// Sessions with a live actor. Not persisted, so it comes from the
        /// service rather than from the rows.
        running: Vec<SessionId>,
    },
    Transcript {
        session: SessionId,
        generation: u64,
        messages: Vec<Message>,
    },
    /// The actor took the instruction. `AwaitingApproval` is an outcome, not a
    /// failure: the turn is paused on a decision, so nothing was queued.
    Submitted {
        session: SessionId,
        outcome: SubmitOutcome,
    },
    /// The instruction was refused before the actor took it.
    SubmitFailed {
        session: SessionId,
        message: String,
    },
    /// The turn was cancelled, and the actor has exited.
    Stopped {
        session: SessionId,
    },
    /// One decision was accepted, and the turn is running again.
    Decided {
        session: SessionId,
        call: ToolCallId,
    },
    /// The decision was refused, so the call is still pending.
    ///
    /// A `Conflict` here is not an error the user caused: it means the runtime
    /// started the turn before the decision arrived, and the next
    /// `awaiting_approval` frame will list the call again.
    DecideFailed {
        session: SessionId,
        call: ToolCallId,
        message: String,
    },
    /// The models a turn can drive, and each mode's inheritable choice.
    Models {
        generation: u64,
        models: Vec<ModelInfo>,
        defaults: ModeDefaults,
    },
    /// This workspace's user-invocable skills, in menu order.
    Skills {
        workspace: WorkspaceId,
        generation: u64,
        skills: Vec<SkillEntry>,
    },
    /// A session's mode or choice was written, and this is the row as stored.
    SessionUpdated(Box<ChatSession>),
    /// The compaction was accepted. The rewrite announces itself on the bus.
    Compacted {
        session: SessionId,
    },
    /// The uncapped body behind a compressed result.
    Original {
        session: SessionId,
        call: ToolCallId,
        text: String,
    },
    /// The uncapped body could not be read, so a later attempt may retry.
    OriginalFailed {
        session: SessionId,
        call: ToolCallId,
        message: String,
    },
    /// One stored row, refetched after a frame said it changed.
    Message {
        session: SessionId,
        generation: u64,
        message: Box<Message>,
    },
    WorkspaceOpened {
        workspace: Box<Workspace>,
        created: bool,
    },
    WorkspaceDeleted {
        workspace: WorkspaceId,
    },
    /// The landing page's first message: the session exists, and this says
    /// whether the instruction was taken into it.
    ///
    /// `outcome` is `None` when the send failed; the row is still there, the text
    /// is still in the composer, and `error` is what to say about it.
    SessionStarted {
        session: Box<ChatSession>,
        outcome: Option<SubmitOutcome>,
        error: Option<String>,
    },
    SessionRenamed(Box<ChatSession>),
    SessionDeleted {
        session: SessionId,
    },
    Mcp {
        workspace: WorkspaceId,
        generation: u64,
        servers: Vec<McpServerStatus>,
    },
    /// The servers were told to start. Nothing to draw: a status frame follows.
    McpFocused {
        workspace: WorkspaceId,
    },
    /// The health probe finished. `error` is `None` when the store answered.
    Probed {
        error: Option<String>,
    },
    /// Path completion for `text`. A later keystroke has issued its own read.
    Paths {
        generation: u64,
        text: String,
        candidates: Vec<String>,
    },
    /// The path could not be opened, so the typed text stays where it is.
    CreateWorkspaceFailed {
        message: String,
    },
    /// The rename was refused, so the field stays open with the typed title.
    RenameFailed {
        message: String,
    },
    DeleteFailed {
        message: String,
    },
    /// The view state was written.
    Saved,
    /// An effect failed. `generation` is set by the effect that can go stale.
    Failed {
        generation: Option<u64>,
        message: String,
    },
}

impl ActionOutcome {
    /// A failure that no reload can make stale.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            generation: None,
            message: message.into(),
        }
    }
}

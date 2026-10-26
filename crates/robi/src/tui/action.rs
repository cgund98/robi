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
use crate::tui::state::composer::{Pending, PendingImage};
use crate::tui::state::models::Pick;
use crate::tui::state::settings::Loaded;

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
    /// `images` are the pending images' bytes: the store is keyed by session, so
    /// the actor's session has to exist before they can be written. `effects`
    /// stores them, then submits the references.
    Submit {
        session: SessionId,
        text: String,
        files: Vec<FileAttachment>,
        images: Vec<PendingImage>,
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
        images: Vec<PendingImage>,
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
    /// Every setting the surface shows, with each secret reduced to one bit.
    LoadSettings { generation: u64 },
    /// Write one setting, or clear it when `value` is `None`.
    ///
    /// `key` is `'static` because it comes from the surface's own table: a write
    /// names a row the reader selected, never a name a field produced.
    SaveSetting {
        key: &'static str,
        value: Option<String>,
    },
    /// The user and project MCP config paths, and whether the project file is
    /// trusted. The file text is not read: the terminal never becomes an editor.
    LoadMcpConfig { workspace: WorkspaceId },
    /// Read one file the reader named, and hand it back as an attachment.
    ///
    /// The path is resolved, read, and classified in the effect: `App` does no
    /// I/O, and the caps belong in one place.
    Attach {
        workspace: WorkspaceId,
        source: String,
        /// The session the composer is showing when the read is issued. A read
        /// that lands after a switch is dropped rather than putting a chip under
        /// another chat — the same generation rule, keyed by session.
        session: Option<SessionId>,
    },
    /// Open one workspace file with the configured `file_open_command`.
    OpenFile {
        workspace: WorkspaceId,
        path: String,
        /// 1-based line to land on. `/open` has none; a card's path could.
        line: Option<u32>,
    },
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
    /// The surface's values. A secret arrives as `configured` and nothing else.
    Settings {
        generation: u64,
        values: std::collections::BTreeMap<&'static str, Loaded>,
    },
    /// One setting was written or cleared.
    SettingSaved {
        key: &'static str,
        label: &'static str,
    },
    /// The write was refused, so the row goes back to what is stored.
    SettingFailed {
        key: &'static str,
        message: String,
    },
    /// This workspace's MCP config paths.
    McpConfig(crate::tui::state::settings::McpPaths),
    /// One file is read and ready to attach. `session` is the composer's session
    /// at the moment `Attach` was issued, so a read that lands after a switch
    /// cannot put a chip under another chat.
    Attached {
        session: Option<SessionId>,
        source: String,
        pending: Pending,
    },
    /// The file was not attached, and the composer is unchanged.
    AttachFailed {
        message: String,
    },
    /// The configured program was started.
    Opened {
        path: String,
    },
    /// No command is configured, or the program refused.
    OpenFailed {
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

impl Action {
    /// What the audit table calls this call.
    ///
    /// Exhaustive on purpose: a new action is a new name here before it can
    /// dispatch, so no call reaches the table as "unknown".
    pub fn label(&self) -> &'static str {
        match self {
            Action::LoadWorkspaces { .. } => "workspaces",
            Action::LoadSessions { .. } => "sessions",
            Action::Reload { .. } => "transcript",
            Action::Submit { .. } => "submit",
            Action::Decide { .. } => "decide",
            Action::LoadOriginal { .. } => "original",
            Action::Stop { .. } => "stop",
            Action::LoadMessage { .. } => "message",
            Action::CreateWorkspace { .. } => "create workspace",
            Action::DeleteWorkspace { .. } => "delete workspace",
            Action::StartSession { .. } => "start session",
            Action::RenameSession { .. } => "rename session",
            Action::LoadModels { .. } => "models",
            Action::LoadSkills { .. } => "skills",
            Action::SetSessionMode { .. } => "session mode",
            Action::SetSessionChoice { .. } => "session choice",
            Action::Compact { .. } => "compact",
            Action::DeleteSession { .. } => "delete session",
            Action::LoadMcp { .. } => "mcp",
            Action::FocusMcp { .. } => "mcp focus",
            Action::Probe => "probe",
            Action::CompletePath { .. } => "complete path",
            Action::SaveView { .. } => "view state",
            Action::LoadSettings { .. } => "settings",
            Action::SaveSetting { .. } => "save setting",
            Action::LoadMcpConfig { .. } => "mcp config",
            Action::Attach { .. } => "attach",
            Action::OpenFile { .. } => "open file",
        }
    }

    /// The calls the audit table leaves out.
    ///
    /// The webview's middleware skips the health check, and these are its peer:
    /// the probe is a heartbeat the reader did not ask for, path completion runs
    /// once per keystroke, and the view state is written while the process is
    /// leaving. A table of those would be noise around the calls that matter.
    ///
    /// `/attach` and `/open` are **not** here: one reads a file the reader named
    /// and one starts a program, and both are exactly what an audit table is for.
    pub fn skips_audit(&self) -> bool {
        matches!(
            self,
            Action::Probe | Action::CompletePath { .. } | Action::SaveView { .. }
        )
    }
}

impl ActionOutcome {
    /// One line for the audit table's expanded body.
    ///
    /// Exhaustive for the same reason [`Action::label`] is: a new outcome states
    /// what it is worth saying about itself.
    pub fn summary(&self) -> String {
        match self {
            ActionOutcome::Workspaces { workspaces, .. } => {
                format!("{} workspace(s)", workspaces.len())
            }
            ActionOutcome::Sessions {
                sessions, running, ..
            } => {
                format!("{} session(s), {} running", sessions.len(), running.len())
            }
            ActionOutcome::Transcript { messages, .. } => {
                format!("{} message(s)", messages.len())
            }
            ActionOutcome::Submitted { outcome, .. } => format!("{outcome:?}"),
            ActionOutcome::SubmitFailed { message, .. } => message.clone(),
            ActionOutcome::Stopped { .. } => "stopped".to_owned(),
            ActionOutcome::Decided { call, .. } => format!("decided {call}"),
            ActionOutcome::DecideFailed { call, message, .. } => {
                format!("{call} refused: {message}")
            }
            ActionOutcome::Models { models, .. } => format!("{} model(s)", models.len()),
            ActionOutcome::Skills { skills, .. } => format!("{} skill(s)", skills.len()),
            ActionOutcome::SessionUpdated(session) => {
                format!(
                    "{} \u{b7} {:?}",
                    session.title.as_deref().unwrap_or("untitled"),
                    session.mode
                )
            }
            ActionOutcome::Compacted { .. } => "accepted".to_owned(),
            ActionOutcome::Original { text, .. } => format!("{} byte(s)", text.len()),
            ActionOutcome::OriginalFailed { message, .. } => message.clone(),
            ActionOutcome::Message { message, .. } => format!("{} row", message.id),
            ActionOutcome::WorkspaceOpened { workspace, created } => {
                format!(
                    "{} ({})",
                    workspace.root,
                    if *created { "created" } else { "opened" }
                )
            }
            ActionOutcome::WorkspaceDeleted { workspace } => format!("removed {workspace}"),
            ActionOutcome::SessionStarted { error, .. } => error
                .clone()
                .unwrap_or_else(|| "session started".to_owned()),
            ActionOutcome::SessionRenamed(session) => {
                format!(
                    "renamed to {}",
                    session.title.as_deref().unwrap_or("untitled")
                )
            }
            ActionOutcome::SessionDeleted { session } => format!("removed {session}"),
            ActionOutcome::Mcp { servers, .. } => format!("{} server(s)", servers.len()),
            ActionOutcome::McpFocused { .. } => "focusing".to_owned(),
            ActionOutcome::Probed { error } => {
                error.clone().unwrap_or_else(|| "answered".to_owned())
            }
            ActionOutcome::Paths { candidates, .. } => format!("{} candidate(s)", candidates.len()),
            ActionOutcome::CreateWorkspaceFailed { message } => message.clone(),
            ActionOutcome::RenameFailed { message } => message.clone(),
            ActionOutcome::DeleteFailed { message } => message.clone(),
            ActionOutcome::Settings { values, .. } => format!("{} setting(s)", values.len()),
            ActionOutcome::SettingSaved { key, .. } => format!("wrote {key}"),
            ActionOutcome::SettingFailed { key, message } => format!("{key} refused: {message}"),
            ActionOutcome::McpConfig(paths) => paths.project_path.clone(),
            ActionOutcome::Attached {
                source, pending, ..
            } => {
                format!("{source} ({} byte(s))", pending.text_len())
            }
            ActionOutcome::AttachFailed { message } => message.clone(),
            ActionOutcome::Opened { path } => format!("opened {path}"),
            ActionOutcome::OpenFailed { message } => message.clone(),
            ActionOutcome::Saved => "written".to_owned(),
            ActionOutcome::Failed { message, .. } => message.clone(),
        }
    }

    /// Whether this outcome is a refusal the reader has to be told about.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            ActionOutcome::Failed { .. }
                | ActionOutcome::SubmitFailed { .. }
                | ActionOutcome::DecideFailed { .. }
                | ActionOutcome::OriginalFailed { .. }
                | ActionOutcome::CreateWorkspaceFailed { .. }
                | ActionOutcome::RenameFailed { .. }
                | ActionOutcome::DeleteFailed { .. }
                | ActionOutcome::SettingFailed { .. }
                | ActionOutcome::AttachFailed { .. }
                | ActionOutcome::OpenFailed { .. }
        )
    }

    /// A failure that no reload can make stale.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            generation: None,
            message: message.into(),
        }
    }
}

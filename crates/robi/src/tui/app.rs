//! `App`, `Msg`, and `update`.
//!
//! `update` is a pure function of state and a message: no I/O, no `.await`, no
//! services. It records [`Action`]s that `effects` runs on the runtime, and the
//! result comes back as [`Msg::Done`]. That is what makes the loop testable
//! without a terminal, a database, or a network — the same discipline as
//! `robi-core`.
//!
//! `App` holds no `AppState`: the pump owns the services and passes them to
//! `effects`, so a view snapshot can be built in a unit test.
//!
//! # Stored rows, never deltas
//!
//! T0 reloaded the whole transcript on any frame. T1 narrows that to one row
//! per frame: `message_added`, `message_updated`, and `tool_call_updated` each
//! name a `message_id`, and the view asks for exactly that row. A
//! `message_delta` only moves the phase. That is the rule the web shell already
//! follows, and here it is load-bearing: the bus drops a queued delta when a
//! subscriber falls behind (`make_room` in `domain/events`), so a transcript
//! rendered from deltas would silently lose tokens.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui_textarea::CursorMove;

use robi_core::ids::{MessageId, SessionId, ToolCallId, WorkspaceId};
use robi_core::message::{Message, ToolCall};

use crate::adapters::model_source::{EffectiveChoice, ModeDefaults};
use crate::agent::providers::ModelInfo;
use crate::domain::chat_message::attachment::{MAX_ATTACHMENTS, MAX_TEXT_TOTAL_BYTES};
use crate::domain::chat_message::runtime::SubmitOutcome;
use std::sync::Arc;

use crate::domain::chat_session::lease::{LeaseGuard, Locks, Resource, Scope};
use crate::domain::chat_session::model::{
    AgentMode, ChatSession, ChoiceKey, ModeOverride, ModelConfig,
};
use crate::domain::events::{
    EventEnvelope, APP_ERROR, AWAITING_APPROVAL, MCP_STATUS, MESSAGE_ADDED, MESSAGE_DELTA,
    MESSAGE_UPDATED, SESSION_CREATED, SESSION_DELETED, SESSION_UPDATED, TOOL_CALL_UPDATED,
    TRANSCRIPT_COMPACTED, TURN_FINISHED, TURN_STARTED,
};
use crate::domain::workspace::model::Workspace;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::command::{self, Verdict};
use crate::tui::keys::{self, Command};
use crate::tui::lifecycle;
use crate::tui::persist::ViewState;
use crate::tui::state::activity::{Activity, DeltaKind};
use crate::tui::state::approval::{Approval, Decision};
use crate::tui::state::audit::AuditLog;
use crate::tui::state::completion::{Completion, SkillEntry};
use crate::tui::state::composer::Attachment;
use crate::tui::state::composer::Composer;
use crate::tui::state::confirm::{Confirm, Target};
use crate::tui::state::detail::Detail;
use crate::tui::state::facts::Facts;
use crate::tui::state::help::HelpView;
use crate::tui::state::landing::Greeting;
use crate::tui::state::manage::ManageView;
use crate::tui::state::models::Picker;
use crate::tui::state::prompt::{Prompt, PromptKind};
use crate::tui::state::sessions::{self, Mark, SessionsView};
use crate::tui::state::settings::{self, Kind, McpPaths, Section, SettingsRow, SettingsView};
use crate::tui::state::status::Status;
use crate::tui::state::transcript::{pending_calls, Transcript};
use crate::tui::state::workspaces::{self, Screen, WorkspacesView, RECENT_LIMIT};
use crate::tui::state::{Focus, Mode, Notice, Phase};
use crate::tui::view;

/// Rows one `PageUp`/`PageDown` moves an overlay's selection by.
const LIST_PAGE: usize = 10;

/// Pump ticks per animation step: one dot change a second, on a 250 ms tick.
///
/// A pause is a question, not a spinner. A quarter-second flicker reads as
/// machinery waiting on itself; a slow, even beat reads as a prompt.
const TICKS_PER_DOT: u64 = 4;

/// How far one scrolling key moves, so the same command serves the transcript and
/// the detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scroll {
    Line,
    Half,
    Page,
}

/// What the pump hands to `update`.
#[derive(Debug, PartialEq)]
pub enum Msg {
    /// A keystroke. Releases are filtered in `input`, and ignored here too.
    Key(KeyEvent),
    /// A bracketed paste: the block, as text.
    ///
    /// It goes to the field on top and never to the key map, so nothing in it
    /// can submit, stop a turn, or open a surface.
    Paste(String),
    /// One frame from the bus.
    Agent(EventEnvelope),
    /// 250 ms: elapsed time, the dots, and the catch-up reload.
    Tick,
    /// An effect finished.
    Done(ActionOutcome),
    /// One call's audit row. Sent before the outcome it belongs to, and dropped
    /// by the view entirely when nothing is reading the table.
    Audited(crate::tui::state::audit::Entry),
    /// The mouse wheel turned, in rows: negative is up. It goes through the
    /// same scroll path as `PageUp`/`PageDown`, so the follow lock behaves
    /// identically however the reader scrolled.
    Scroll(isize),
    /// The terminal changed size.
    Resize { width: u16, height: u16 },
    /// The terminal window gained or lost focus.
    ///
    /// Only used to decide whether a pause rings the bell: a terminal that never
    /// reports focus simply stays "focused", and no bell is rung.
    Focus(bool),
    /// `SIGTERM`.
    Terminate,
}

/// The whole view state.
pub struct App {
    view: ViewState,
    /// The database this session opened, shown when there is nothing to show.
    database_url: String,
    /// The `Open` lease on the session on screen, when this process has it.
    ///
    /// Held as long as the session is displayed, so a second process sees the
    /// session as owned and renders its results as unavailable rather than
    /// racing this one for the blob file.
    open_lease: Option<LeaseGuard>,
    /// The pid holding the session on screen, when this process does not.
    ///
    /// Set from the refusal, so the transcript can say why a result is missing
    /// and the footer can name it.
    held_by: Option<u32>,
    /// The session `open_lease` was taken for, so a re-render does not refcount.
    lease_session: Option<SessionId>,
    /// Cross-process ownership, shared with the services.
    locks: Arc<dyn Locks>,
    /// Calls whose `locked` failure has already been announced.
    ///
    /// A refusal arrives once per store row and again on every reload. The strip
    /// overwrites, so repeating it is not noise, but a reload of fifty cards would
    /// end on whichever came last. This keeps it to the calls that are new.
    locked_announced: BTreeSet<String>,
    workspaces: Vec<Workspace>,
    sessions: Vec<ChatSession>,
    transcript: Transcript,
    activity: Activity,
    composer: Composer,
    focus: Focus,
    notice: Option<Notice>,
    /// The mode stack. The last item owns the keyboard; `Mode::Chat` is the base
    /// and is never popped.
    modes: Vec<Mode>,
    /// The paused turn, when there is one.
    ///
    /// State, not a mode: the approvals are answered on the selected card, so
    /// nothing needs the keyboard to itself while a turn is paused.
    approval: Option<Approval>,
    /// The reason field, when it is open over a card.
    prompt: Option<Prompt>,
    /// The card being read in full, when the detail view is open.
    detail: Option<Detail>,
    /// Whether the terminal window is in front. Assumed, until it says otherwise.
    focused: bool,
    /// How many ticks have arrived. The only clock the view has.
    ticks: u64,
    /// A pause arrived while the window was not in front.
    bell: bool,
    /// Originals already asked for, so a second `Enter` does not refetch.
    loading_originals: BTreeSet<ToolCallId>,
    /// The sessions overlay's filter and selection.
    sessions_view: SessionsView,
    /// The workspace switcher's screen, field, and selection.
    workspaces_view: WorkspacesView,
    /// The manage view's filter and selection.
    manage: ManageView,
    /// The path the process was started on, canonicalised.
    ///
    /// The CLI's argument, or the working directory. It picks the workspace at
    /// startup and it is what the missing-workspace view offers to create.
    target_workspace: PathBuf,
    /// Whether the target path's workspace has already been looked for.
    ///
    /// Set after the first workspace list lands: one prompt per process, not one
    /// per list refresh.
    target_checked: bool,
    /// The greeting the landing page shows, read once at startup.
    ///
    /// Carried as state rather than read in `update` or `view`, so a frame is a
    /// pure function of the state and a test gets the same word every run.
    greeting: Greeting,
    /// Whether the first session listing has landed.
    ///
    /// The first one always opens a session and reads its transcript. Later ones
    /// are refreshes, and a refresh must not reread the transcript under the
    /// reader.
    sessions_loaded: bool,
    /// The confirmation under `Mode::Confirm`, when one is up.
    confirm: Option<Confirm>,
    /// The app-level error, the health probe, and the MCP marks.
    status: Status,
    /// Sessions with a live actor, as the last list read reported them.
    running: BTreeSet<SessionId>,
    /// A rename the service has not answered yet, so a refusal can reopen the
    /// field with the title that was typed.
    pending_rename: Option<(SessionId, String)>,
    /// Counters for the reads that can land out of order.
    workspaces_generation: u64,
    sessions_generation: u64,
    mcp_generation: u64,
    paths_generation: u64,
    /// The workspace whose servers were last told to start.
    focused_mcp: Option<WorkspaceId>,
    /// The terminal's columns. The detail view wraps to
    /// `view::content_width` of them, so it needs the number, not just the
    /// transcript's already-drunk width.
    width: u16,
    height: u16,
    /// A stop was issued and has not come back yet.
    stopping: bool,
    quit: bool,
    actions: Vec<Action>,
    /// The mode a chat with no session row will be created in.
    ///
    /// The webview's `draftMode`. Not persisted: a mode is a fact about a session
    /// once the session exists, and a draft is view state that dies with the
    /// process, like the composer's text.
    draft_mode: AgentMode,
    /// The model this chat will be created with. `None` inherits the mode's
    /// setting, which is what `Use default` means.
    draft_model: Option<String>,
    /// The reasoning effort this chat will be created with.
    draft_effort: Option<String>,
    /// The models a turn can drive, as the provider filter allows.
    models: Vec<ModelInfo>,
    /// Each mode's inheritable choice, read with the list.
    defaults: Option<ModeDefaults>,
    models_generation: u64,
    /// This workspace's user-invocable skills, for the `@` menu.
    skills: Vec<SkillEntry>,
    /// The workspace the skill list belongs to, so a switch can clear it.
    skills_workspace: Option<WorkspaceId>,
    skills_generation: u64,
    /// The model and effort picker, when it is open.
    picker: Option<Picker>,
    /// The `/` or `@` menu over the composer, when it is open.
    completion: Option<Completion>,
    /// The help page's scroll.
    help: HelpView,
    /// The settings surface: the section, the row, and the values it shows.
    settings: SettingsView,
    /// One row per effect, for the settings surface's audit section.
    audit: AuditLog,
    settings_generation: u64,
    /// The MCP config paths, read when the MCP section is first opened.
    mcp_config: Option<McpPaths>,
}

impl App {
    pub fn new(
        view: ViewState,
        database_url: impl Into<String>,
        target_workspace: PathBuf,
        greeting: Greeting,
        locks: Arc<dyn Locks>,
    ) -> Self {
        let mut transcript = Transcript::new();
        // The persisted expansion set is the transcript's, and the transcript's is
        // the persisted one: one set, mirrored on every change.
        transcript.set_expanded(view.expanded.clone());
        Self {
            view,
            database_url: database_url.into(),
            open_lease: None,
            held_by: None,
            lease_session: None,
            locks,
            locked_announced: BTreeSet::new(),
            workspaces: Vec::new(),
            sessions: Vec::new(),
            transcript,
            activity: Activity::default(),
            composer: Composer::new(),
            focus: Focus::default(),
            notice: None,
            modes: vec![Mode::Chat],
            approval: None,
            prompt: None,
            detail: None,
            focused: true,
            ticks: 0,
            bell: false,
            loading_originals: BTreeSet::new(),
            sessions_view: SessionsView::new(),
            workspaces_view: WorkspacesView::new(),
            manage: ManageView::new(),
            target_workspace,
            greeting,
            target_checked: false,
            sessions_loaded: false,
            confirm: None,
            status: Status::new(),
            running: BTreeSet::new(),
            pending_rename: None,
            workspaces_generation: 0,
            sessions_generation: 0,
            mcp_generation: 0,
            paths_generation: 0,
            focused_mcp: None,
            width: 0,
            height: 0,
            stopping: false,
            quit: false,
            actions: Vec::new(),
            draft_mode: AgentMode::Agent,
            draft_model: None,
            draft_effort: None,
            models: Vec::new(),
            defaults: None,
            models_generation: 0,
            skills: Vec::new(),
            skills_workspace: None,
            skills_generation: 0,
            picker: None,
            completion: None,
            help: HelpView::default(),
            settings: SettingsView::new(),
            audit: AuditLog::new(),
            settings_generation: 0,
            mcp_config: None,
        }
    }

    /// The first action: list workspaces, so the active one can be picked.
    ///
    /// Everything after this chains through [`Msg::Done`].
    pub fn boot(&mut self) -> Vec<Action> {
        self.load_workspaces();
        // The picker's offer and the facts row's model name both need this, and it
        // does not depend on a workspace, so it goes with the first action.
        self.load_models();
        self.take_actions()
    }

    /// The database this session opened.
    ///
    /// The default is relative to the working directory, so this is the one
    /// fact that explains an empty workspace list.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    /// What to persist.
    ///
    /// The expansion set is the transcript's, because that is where it is read;
    /// `self.view.expanded` is only the copy the file was loaded from.
    pub fn snapshot(&self) -> ViewState {
        let mut view = self.view.clone();
        view.expanded = self.transcript.render().expanded.clone();
        view
    }

    /// The mode that owns the keyboard.
    pub fn mode(&self) -> Mode {
        self.modes.last().copied().unwrap_or(Mode::Chat)
    }

    /// Push a mode. Approval and its reason field ride on top of `chat`.
    fn enter(&mut self, mode: Mode) {
        if self.mode() != mode {
            self.modes.push(mode);
        }
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The agent mode in force: the open session's, or the draft's on the landing
    /// page.
    ///
    /// Read by the rails, the facts row and `Ctrl-T`, so one resolution serves all
    /// three. Not to be confused with [`App::mode`], which is which surface owns
    /// the keyboard.
    pub fn agent_mode(&self) -> AgentMode {
        match self.open_session() {
            Some(session) => session.mode,
            None => self.draft_mode,
        }
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn sessions(&self) -> &[ChatSession] {
        &self.sessions
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn activity(&self) -> &Activity {
        &self.activity
    }

    pub fn composer(&self) -> &Composer {
        &self.composer
    }

    /// The paused turn, if the modal is up.
    pub fn approval(&self) -> Option<&Approval> {
        self.approval.as_ref()
    }

    /// The reason field, if it is open.
    pub fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// The card being read in full, if the detail view is open.
    pub fn detail(&self) -> Option<&Detail> {
        self.detail.as_ref()
    }

    /// The confirmation under `Mode::Confirm`, if one is up.
    pub fn confirm(&self) -> Option<&Confirm> {
        self.confirm.as_ref()
    }

    /// The process facts the frame shows: the error, the probe, and the marks.
    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn sessions_view(&self) -> &SessionsView {
        &self.sessions_view
    }

    pub fn workspaces_view(&self) -> &WorkspacesView {
        &self.workspaces_view
    }

    /// Whether the sessions overlay is on the stack.
    ///
    /// A prompt or a confirmation sits above it and draws over the same frame,
    /// so the overlay is asked of the stack rather than of the top mode.
    pub fn sessions_open(&self) -> bool {
        self.modes.contains(&Mode::Sessions)
    }

    pub fn workspaces_open(&self) -> bool {
        self.modes.contains(&Mode::Workspaces)
    }

    /// The sessions the overlay lists, with their marks.
    pub fn session_rows(&self) -> Vec<(&ChatSession, Mark)> {
        sessions::visible(&self.sessions, &self.sessions_view.query())
            .into_iter()
            .map(|session| (session, self.mark_of(session)))
            .collect()
    }

    /// A row's mark, with the open session's own turn taken from the phase
    /// rather than from the last list read: the mark must not wait a query.
    pub fn mark_of(&self, session: &ChatSession) -> Mark {
        let mark = sessions::mark(session, &self.running);
        if mark == Mark::Idle && Some(session.id) == self.view.open_session_id && self.is_busy() {
            return Mark::Running;
        }
        mark
    }

    /// The workspaces the switcher lists: the eight most recent, in recent order.
    ///
    /// The view adds the two action rows after them, which is why the selection
    /// is an index rather than an id.
    pub fn workspace_rows(&self) -> Vec<&Workspace> {
        workspaces::order_recent(&self.workspaces, &self.view.recent_workspace_ids)
            .into_iter()
            .take(RECENT_LIMIT)
            .collect()
    }

    /// Every workspace, filtered by the manage view's query.
    pub fn manage_rows(&self) -> Vec<&Workspace> {
        workspaces::filtered(&self.workspaces, &self.manage.query())
    }

    pub fn manage(&self) -> &ManageView {
        &self.manage
    }

    /// The workspace the manage view would open, or remove.
    pub fn manage_selected(&self) -> Option<&Workspace> {
        self.manage_rows().get(self.manage.selected).copied()
    }

    /// The path the process was started on, as the reader typed it.
    ///
    /// The missing-workspace view names it: "no workspace at this path" is only
    /// useful with the path in it.
    pub fn target_workspace(&self) -> std::borrow::Cow<'_, str> {
        self.target_workspace.to_string_lossy()
    }

    /// The startup question owns the keyboard: `y` and `n` are its answers.
    pub fn workspace_missing(&self) -> bool {
        self.mode() == Mode::WorkspaceMissing
    }

    /// The landing page: a workspace is open and no session is.
    ///
    /// This is what `Ctrl-N` shows, and what a workspace with no sessions shows.
    /// No session row exists until the first message is sent, so opening a
    /// workspace and leaving never leaves an empty chat behind.
    pub fn landing(&self) -> bool {
        self.view.active_workspace_id.is_some() && self.view.open_session_id.is_none()
    }

    pub fn greeting(&self) -> Greeting {
        self.greeting
    }

    /// The startup question is the page, whether or not an overlay is over it.
    ///
    /// The view asks this rather than [`Self::workspace_missing`]: a switcher
    /// opened from the question draws *over the question*, and closing it has to
    /// come back here instead of to a chat frame with nothing open behind it.
    pub fn missing_workspace_page(&self) -> bool {
        self.modes.contains(&Mode::WorkspaceMissing)
    }

    /// The session the sessions overlay's cursor is on.
    ///
    /// Row zero is the new-session action, so it names no session at all.
    pub fn selected_session(&self) -> Option<&ChatSession> {
        let rows = self.session_ids();
        let id = sessions::session_at(&rows, self.sessions_view.selected())?;
        self.sessions.iter().find(|session| session.id == id)
    }

    /// Whether the cursor is on the overlay's action row.
    pub fn session_action_selected(&self) -> bool {
        self.sessions_view.selected() == 0
    }

    pub fn selected_workspace(&self) -> Option<&Workspace> {
        let index = self.workspaces_view.selected;
        self.workspace_rows().get(index).copied()
    }

    /// Whether this session's actor is live, including a turn this process just
    /// started and no list read has reported yet.
    fn is_running(&self, id: SessionId) -> bool {
        self.running.contains(&id) || (Some(id) == self.view.open_session_id && self.is_busy())
    }

    /// One call from the open transcript, for the modal's detail.
    pub fn call(&self, id: ToolCallId) -> Option<&ToolCall> {
        self.transcript
            .messages()
            .iter()
            .flat_map(|message| message.tool_calls.iter())
            .find(|call| call.id == id)
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Take the pending bell, if a pause asked for one.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// What the window title should say.
    pub fn title(&self) -> &'static str {
        if self.approval.is_some() {
            "robi \u{2014} approval needed"
        } else {
            "robi"
        }
    }

    /// Whether a turn is running.
    pub fn is_busy(&self) -> bool {
        self.activity.is_busy() || self.composer.sending()
    }

    /// Whether a stop is in flight.
    pub fn is_stopping(&self) -> bool {
        self.stopping
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// How many ticks have arrived. The only clock the view has, so spinners and
    /// the waiting marker move without keeping a timer of their own.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The generation the next workspace list read belongs to.
    ///
    /// Public so the tests can land an outcome the app will accept; the app
    /// itself is the only caller.
    pub fn workspaces_generation(&self) -> u64 {
        self.workspaces_generation
    }

    pub fn sessions_generation(&self) -> u64 {
        self.sessions_generation
    }

    pub fn mcp_generation(&self) -> u64 {
        self.mcp_generation
    }

    pub fn models_generation(&self) -> u64 {
        self.models_generation
    }

    pub fn skills_generation(&self) -> u64 {
        self.skills_generation
    }

    pub fn settings_generation(&self) -> u64 {
        self.settings_generation
    }

    /// The skills the `@` menu offers.
    pub fn skills(&self) -> &[SkillEntry] {
        &self.skills
    }

    pub fn active_workspace_id(&self) -> Option<WorkspaceId> {
        self.view.active_workspace_id
    }

    pub fn open_session_id(&self) -> Option<SessionId> {
        self.view.open_session_id
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let id = self.view.active_workspace_id?;
        self.workspaces.iter().find(|workspace| workspace.id == id)
    }

    pub fn open_session(&self) -> Option<&ChatSession> {
        let id = self.view.open_session_id?;
        self.sessions.iter().find(|session| session.id == id)
    }

    /// The header's session title.
    ///
    /// The landing page reads `New chat`, the web shell's name for a chat with no
    /// row yet (`sessionDisplayTitle(undefined)`); `no session` is left for the
    /// frame with no workspace open at all, where there is nothing to name.
    pub fn session_title(&self) -> &str {
        if self.landing() {
            return "New chat";
        }
        match self.open_session() {
            Some(session) => session.title.as_deref().unwrap_or(sessions::UNTITLED),
            None => "no session",
        }
    }

    /// How many rows the transcript area has: the frame minus the blank row above
    /// the header, the header and the rule under it, the gap above the composer,
    /// the composer band, and the blank row, hint, and blank row beneath it.
    pub fn body_height(&self) -> usize {
        // Three rows above the transcript: the frame's top pad, the header, and
        // the rule under it. Three below the composer: the gap above the hint, the
        // hint, and the frame's bottom pad.
        let used = (3 + 3_u16)
            .saturating_add(view::COMPOSER_GAP)
            .saturating_add(self.status_rows())
            .saturating_add(self.strip_gap())
            .saturating_add(self.composer.height())
            .saturating_add(self.prompt_rows());
        usize::from(self.height.saturating_sub(used)).max(1)
    }

    /// Rows the status strip takes above the composer: one when there is
    /// something to say, none when there is not.
    ///
    /// The view's `strip::content` decides what it says; the two agree because
    /// both ask these five questions, in this order.
    pub fn status_rows(&self) -> u16 {
        let anything = self.status.error().is_some()
            || self.status.health().is_some()
            || self.is_stopping()
            || self.composer.sending()
            || self.activity.label().is_some();
        u16::from(anything)
    }

    /// Rows of canvas between the strip and the composer band: one while the
    /// strip is drawn, none while it is not.
    ///
    /// The view's layout asks this too, so the activity line never sits flush
    /// against the band's top row and the transcript's height stays right.
    pub fn strip_gap(&self) -> u16 {
        if self.status_rows() > 0 {
            view::STRIP_GAP
        } else {
            0
        }
    }

    /// Rows the one-line prompt takes between the composer and the footer.
    ///
    /// The confirmation is not here: it is a panel over the frame
    /// (`view::confirm`), so it takes no layout row and the transcript keeps the
    /// one it used to give up.
    pub fn prompt_rows(&self) -> u16 {
        u16::from(self.prompt.is_some())
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Take the effects `update` recorded.
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    /// Set the terminal size. The pump calls this once, then `Msg::Resize`
    /// keeps it current.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        // The transcript wraps to the content width, not the terminal's: the
        // frame keeps a gutter, and a band must end where the header does.
        self.transcript.set_width(view::content_width(width));
        // The detail view's body is the same width, so it re-wraps with it: a
        // command that fitted on one row may not, and rows the reader never saw
        // as two would otherwise be cut at the frame's edge.
        if let Some(detail) = self.detail.as_mut() {
            detail.set_width(usize::from(view::content_width(width)));
        }
        self.sync_viewport();
    }

    /// Apply the transcript's share of the frame.
    ///
    /// Called after every message, not only on a resize: the composer grows and
    /// shrinks with its text, and the follow lock has to know.
    pub fn sync_viewport(&mut self) {
        self.transcript.set_viewport(self.body_height());
    }

    pub fn view(&self, frame: &mut Frame) {
        view::draw(frame, self);
    }

    pub fn update(&mut self, message: Msg) {
        match message {
            Msg::Key(key) => self.on_key(key),
            Msg::Paste(text) => self.on_paste(&text),
            Msg::Agent(envelope) => self.on_agent(envelope),
            Msg::Tick => self.on_tick(),
            Msg::Done(outcome) => self.on_done(outcome),
            Msg::Audited(entry) => self.audit.record(entry),
            Msg::Scroll(rows) => self.transcript.scroll_by(rows),
            Msg::Resize { width, height } => self.resize(width, height),
            Msg::Focus(focused) => self.focused = focused,
            Msg::Terminate => self.quit = true,
        }
        self.sync_viewport();
    }

    /// A paste. Text, never a keystroke.
    ///
    /// The terminal types a pasted block at the field if paste marking is off,
    /// and a newline among those keystrokes is `Enter`: the first line of a
    /// pasted message was sent as a message of its own. Here the block only ever
    /// reaches a field, and no character in it is a binding — a paste cannot
    /// send, stop, or quit whatever it contains.
    ///
    /// The field is the same one a keystroke would reach, so a paste is never
    /// silently dropped from a surface that accepts typing.
    fn on_paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.mode() {
            Mode::Prompt => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.insert_text(&one_line(text));
                }
            }
            Mode::Sessions => {
                self.sessions_view.insert_text(&one_line(text));
                self.reconcile_sessions_view();
            }
            Mode::Workspaces => {
                if self.workspaces_view.screen == Screen::New {
                    self.workspaces_view.insert_path(&one_line(text));
                    self.request_candidates();
                }
            }
            Mode::Manage => {
                self.manage.insert_text(&one_line(text));
                // A filter that changed can shrink the list under the selection.
                self.manage.selected = 0;
                self.reconcile_manage();
            }
            Mode::Settings => self.settings.editor_insert(&one_line(text)),
            Mode::Chat if self.focus.is_composer() && !self.composer.sending() => {
                self.composer.insert_text(&pasted_lines(text));
                self.refresh_completion();
            }
            Mode::Complete => {
                self.composer.insert_text(&pasted_lines(text));
                self.refresh_completion();
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        // An open editor band owns the keyboard, and `keys::command` cannot see
        // it: the map is a pure function of `(Mode, Focus, KeyEvent)`. So the one
        // exception lives here, where the state is. It has to: a path list is
        // full of `j`s, `k`s, and `h`s, and a map that read them as navigation
        // would make a path impossible to type.
        if self.mode() == Mode::Settings && self.settings.editing().is_some() {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            // `Ctrl-C` keeps its global meaning, so a band cannot trap the one
            // key that stops a turn.
            if control && key.code == KeyCode::Char('c') {
                self.stop_or_quit();
                return;
            }
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            match key.code {
                KeyCode::Esc => self.settings.cancel(),
                KeyCode::Enter if !alt => self.settings_commit(),
                // `Alt-Enter`'s fallback, as in the composer: a terminal that
                // delivers it as `Esc`, `Enter` leaves `Ctrl-J` as the newline.
                KeyCode::Char('j') if control => self
                    .settings
                    .editor_input(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
                _ => self.settings.editor_input(&key),
            }
            return;
        }
        match keys::command(self.mode(), self.focus, &key) {
            Some(Command::Quit) => self.quit_or_refuse(),
            Some(Command::StopOrQuit) => self.stop_or_quit(),
            Some(Command::Interrupt) => self.interrupt(),
            Some(Command::Submit) => self.submit(),
            Some(Command::DetachLast) => self.detach_last(&key),
            Some(Command::Newline) => self.composer.insert_newline(),
            Some(Command::FocusNext) | Some(Command::FocusPrev) => {
                self.focus = self.focus.other();
            }
            // The scroll commands mean the detail view's scroll while it is open.
            Some(Command::PageUp) => self.scroll(-1, Scroll::Page),
            Some(Command::PageDown) => self.scroll(1, Scroll::Page),
            Some(Command::HalfPageUp) => self.scroll(-1, Scroll::Half),
            Some(Command::HalfPageDown) => self.scroll(1, Scroll::Half),
            Some(Command::LineUp) => self.scroll(-1, Scroll::Line),
            Some(Command::LineDown) => self.scroll(1, Scroll::Line),
            Some(Command::Top) => self.scroll_to_end(false),
            Some(Command::Bottom) => self.scroll_to_end(true),
            Some(Command::Approve) => self.approve(),
            Some(Command::ApproveAll) => self.approve_all(),
            Some(Command::OpenReason) => self.open_reason(),
            Some(Command::PromptSubmit) => self.submit_prompt(),
            Some(Command::CancelPrompt) => self.cancel_prompt(),
            // The startup prompt asks its two answers with the same keys a
            // destructive confirmation does, and only `update` knows which
            // question is on screen.
            Some(Command::ConfirmYes) if self.workspace_missing() => self.create_target_workspace(),
            Some(Command::ConfirmNo) if self.workspace_missing() => self.choose_another_workspace(),
            Some(Command::ConfirmYes) => self.confirm_yes(),
            Some(Command::ConfirmNo) | Some(Command::CancelConfirm) => self.cancel_confirm(),
            Some(Command::SelectNext) => self.select(1),
            Some(Command::SelectPrev) => self.select(-1),
            Some(Command::OpenDetail) => self.open_detail(),
            Some(Command::CloseDetail) => self.close_detail(),
            Some(Command::OpenSessions) => self.open_sessions(),
            Some(Command::OpenWorkspaces) => self.open_workspaces(),
            Some(Command::NewSession) => self.new_session(),
            Some(Command::NewWorkspace) => self.new_workspace(),
            Some(Command::RenameSession) => self.open_rename(),
            Some(Command::DeleteSelected) => self.delete_selected(),
            Some(Command::ListNext) => self.list_move(1),
            Some(Command::ListPrev) => self.list_move(-1),
            Some(Command::ListPageDown) => self.list_page(1),
            Some(Command::ListPageUp) => self.list_page(-1),
            Some(Command::OverlayOpen) => self.overlay_open(),
            Some(Command::CloseOverlay) => self.close_overlay(),
            Some(Command::CloseManage) => self.close_manage(),
            Some(Command::AcceptCandidate) => self.accept_candidate(),
            Some(Command::CycleMode) => self.cycle_mode(),
            Some(Command::OpenModels) => self.open_models(),
            Some(Command::Compact) => self.open_compact(),
            Some(Command::OpenSettings) => self.open_settings(),
            Some(Command::CloseSettings) => self.close_settings(),
            Some(Command::SettingsApply) => self.settings_apply(),
            Some(Command::SettingsSectionNext) => self.settings_step_section(1),
            Some(Command::SettingsSectionPrev) => self.settings_step_section(-1),
            Some(Command::OpenHelp) => self.open_help(),
            Some(Command::CloseHelp) => self.close_help(),
            Some(Command::CompleteAccept) => self.accept_completion(),
            Some(Command::CompleteDismiss) => self.dismiss_completion(),
            None => self.unclaimed(key),
        }
    }

    /// A scroll key, routed to whichever surface is on top.
    ///
    /// The detail view has its own offset and no follow lock; the transcript has
    /// both. One command set, so the keys are the same in both.
    fn scroll(&mut self, direction: isize, amount: Scroll) {
        let viewport = self.detail_height();
        if self.mode() == Mode::Help {
            let rows = crate::tui::state::help::page()
                .iter()
                .map(|(_, rows)| rows.len() + 2)
                .sum();
            match (amount, direction.is_negative()) {
                (Scroll::Page, true) => self.help.page_up(rows, viewport),
                (Scroll::Page, false) => self.help.page_down(rows, viewport),
                (Scroll::Half, true) => {
                    self.help
                        .scroll_by(-((viewport / 2) as isize), rows, viewport)
                }
                (Scroll::Half, false) => {
                    self.help.scroll_by((viewport / 2) as isize, rows, viewport)
                }
                (Scroll::Line, true) => self.help.scroll_by(-1, rows, viewport),
                (Scroll::Line, false) => self.help.scroll_by(1, rows, viewport),
            }
            return;
        }
        if let Some(detail) = self.detail.as_mut() {
            match (amount, direction.is_negative()) {
                (Scroll::Page, true) => detail.page_up(viewport),
                (Scroll::Page, false) => detail.page_down(viewport),
                (Scroll::Half, true) => {
                    detail.scroll_by(-((viewport / 2) as isize), viewport);
                }
                (Scroll::Half, false) => {
                    detail.scroll_by((viewport / 2) as isize, viewport);
                }
                (Scroll::Line, true) => detail.scroll_by(-1, viewport),
                (Scroll::Line, false) => detail.scroll_by(1, viewport),
            }
            return;
        }
        match (amount, direction.is_negative()) {
            (Scroll::Page, true) => self.transcript.page_up(),
            (Scroll::Page, false) => self.transcript.page_down(),
            (Scroll::Half, true) => self.transcript.half_page_up(),
            (Scroll::Half, false) => self.transcript.half_page_down(),
            (Scroll::Line, true) => self.transcript.scroll_by(-1),
            (Scroll::Line, false) => self.transcript.scroll_by(1),
        }
    }

    /// `Home` / `End`, on whichever surface is on top.
    fn scroll_to_end(&mut self, bottom: bool) {
        let viewport = self.detail_height();
        if self.mode() == Mode::Help {
            if bottom {
                let rows = crate::tui::state::help::page()
                    .iter()
                    .map(|(_, rows)| rows.len() + 2)
                    .sum();
                self.help.bottom(rows, viewport);
            } else {
                self.help.top();
            }
            return;
        }
        if let Some(detail) = self.detail.as_mut() {
            if bottom {
                detail.bottom(viewport);
            } else {
                detail.top();
            }
            return;
        }
        if bottom {
            self.transcript.bottom();
        } else {
            self.transcript.top();
        }
    }

    /// A key no binding claimed.
    ///
    /// Only the field on top may have it. This is the rule the modal depends on:
    /// while a turn is paused the composer is not the top field, so a keystroke
    /// cannot reach a message box whose only possible answer is
    /// `AwaitingApproval`.
    fn unclaimed(&mut self, key: KeyEvent) {
        match self.mode() {
            Mode::Prompt => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.input(&key);
                }
            }
            Mode::Sessions => {
                self.sessions_view.input(&key);
                self.reconcile_sessions_view();
            }
            Mode::Workspaces => match self.workspaces_view.screen {
                Screen::New => {
                    self.workspaces_view.input_path(&key);
                    // One read per keystroke, at the generation this keystroke
                    // is; an answer for the previous text is dropped.
                    self.request_candidates();
                }
                Screen::Recent => {}
            },
            Mode::Manage => {
                self.manage.input(&key);
                // A filter that changed can shrink the list under the selection.
                self.manage.selected = 0;
                self.reconcile_manage();
            }
            Mode::Settings => self.settings_input(&key),
            Mode::Chat if self.focus.is_composer() && !self.composer.sending() => {
                self.composer.input(&key);
                // Every character that reaches the field can open, narrow, or
                // close the completion menu.
                self.refresh_completion();
            }
            Mode::Complete => {
                // The menu takes the arrows and `Enter`; everything else is the
                // field's, and the query is recomputed after it lands.
                self.composer.input(&key);
                self.refresh_completion();
            }
            _ => {}
        }
    }

    /// Hand the composer's text to the session's actor.
    ///
    /// Three refusals, each with a line, because a keystroke that does nothing
    /// is indistinguishable from a keystroke that was lost:
    ///
    /// - nothing to send,
    /// - nothing open to send it to,
    /// - a turn already running.
    ///
    /// The third is deliberate. The desktop shell lets a send interrupt and
    /// queue; in a terminal `Enter` mid-stream more often means "newline", and
    /// `Esc` (or `Ctrl-C`) is the documented interrupt. So this refuses and
    /// says so.
    fn submit(&mut self) {
        if self.composer.is_blank() {
            self.notice = Some(Notice::info("nothing to send"));
            return;
        }
        // The sigil rule, before anything else: a leading `/` or `@` that names
        // nothing is refused, and the text stays. A refusal changes nothing — no
        // submit, no clear, no session — so the reader can fix the word instead of
        // retyping the message. This runs before the landing-page branch, because a
        // command has to work with no session open: `/sessions` should not create
        // one on its way to opening an overlay.
        match command::verdict(&self.composer.text(), &self.skills) {
            Verdict::Send => {}
            Verdict::Refuse(message) => {
                self.notice = Some(Notice::error(message));
                return;
            }
            Verdict::Run(run) => {
                self.composer.clear();
                self.notice = None;
                self.run_command(run);
                return;
            }
        }
        // The landing page: the session does not exist until this message does.
        // One action creates the row and sends into it, which is what keeps an
        // abandoned new chat from leaving an empty session behind.
        if self.landing() {
            let Some(workspace) = self.view.active_workspace_id else {
                self.notice = Some(Notice::error("no workspace is open"));
                return;
            };
            let text = self.composer.text();
            self.composer.set_sending(true);
            self.notice = None;
            self.close_approval();
            self.actions.push(Action::StartSession {
                workspace,
                text,
                files: self.composer.pending_files(),
                images: self.composer.pending_images(),
                mode: self.draft_mode,
                model_config: self.draft_config(),
            });
            return;
        }
        let Some(session) = self.view.open_session_id else {
            self.notice = Some(Notice::error("no session is open"));
            return;
        };
        // Two refusals, both about the message itself. A running turn and a
        // paused one are **not** refusals: the desktop shell interrupts and
        // queues on a send, and the loop rejects a paused turn's waiting calls
        // with "the user sent a new message". `ChatRuntime::submit` does both, so
        // refusing here would be the terminal disagreeing with the product.
        let text = self.composer.text();
        self.composer.set_sending(true);
        self.activity.start(Phase::Thinking);
        self.transcript.set_phase(Phase::Thinking);
        self.notice = None;
        // The waiting calls are about to be rejected by the loop, so the pause is
        // over as soon as the send is taken. Clearing it here keeps `Esc` and the
        // approval keys from naming calls that are already settled.
        self.close_approval();
        self.actions.push(Action::Submit {
            session,
            text,
            files: self.composer.pending_files(),
            images: self.composer.pending_images(),
        });
    }

    /// Run a `/` command.
    ///
    /// A command that has a key dispatches to the same handler the key reaches, so
    /// it cannot become a second interface. The three that take a path have no key
    /// — that is the one difference the table records — and they are the only arms
    /// that read `run.argument` for anything but a word.
    fn run_command(&mut self, run: crate::tui::command::Run) {
        use crate::tui::command::{Arg, BuiltIn};
        match run.built_in {
            BuiltIn::Models => self.open_models(),
            BuiltIn::Sessions => self.open_sessions(),
            BuiltIn::Workspaces => self.open_workspaces(),
            BuiltIn::New => self.new_session(),
            BuiltIn::Help => self.open_help(),
            BuiltIn::Settings => self.open_settings(),
            BuiltIn::Compact => self.open_compact(),
            BuiltIn::Rename => match run.argument {
                Some(title) => self.rename_to(title),
                // With no argument it is the prompt, which is what `Ctrl-R` opens.
                None => self.open_rename(),
            },
            BuiltIn::Mode => match run.argument.as_deref() {
                None => self.cycle_mode(),
                Some(name) => match AgentMode::parse(name.trim()) {
                    Ok(mode) => {
                        self.set_mode(mode);
                        self.notice = Some(Notice::info(format!("{name} mode")));
                    }
                    Err(error) => self.notice = Some(Notice::error(error)),
                },
            },
            BuiltIn::Attach => self.attach(run.argument),
            BuiltIn::Detach => self.detach(run.argument),
            BuiltIn::Open => self.open_file(run.argument),
        }
        // `verdict` enforces `Arg::Required` and rejects a surplus word on
        // `Arg::None`; the arms above take the argument `None` or `Some`.
        let _ = Arg::Required;
    }

    /// `/attach <path>`: read a file into the composer.
    ///
    /// The three refusals here are the ones that do not need a file: no workspace
    /// to resolve against, no path at all, and a path split over two lines. The
    /// rest — a missing file, a binary one, one over the cap — come back from the
    /// effect as `AttachFailed`, because only it may touch the disk.
    fn attach(&mut self, argument: Option<String>) {
        let Some(workspace) = self.view.active_workspace_id else {
            self.notice = Some(Notice::error("no workspace is open"));
            return;
        };
        let Some(source) = argument else {
            self.notice = Some(Notice::error("/attach needs an argument"));
            return;
        };
        let source = source.trim().to_owned();
        if source.is_empty() {
            self.notice = Some(Notice::error("/attach needs an argument"));
            return;
        }
        // `verdict` takes the argument from the whole message, so a second line
        // would arrive here as part of the path.
        if source.contains('\n') {
            self.notice = Some(Notice::error("/attach takes one path on one line"));
            return;
        }
        if self.composer.is_attached(&source) {
            self.notice = Some(Notice::info(format!("{source} is already attached")));
            return;
        }
        self.notice = None;
        self.actions.push(Action::Attach {
            workspace,
            source,
            session: self.view.open_session_id,
        });
    }

    /// `/detach`: no argument takes everything off, a name takes one.
    ///
    /// Composer state only, so this issues no effect. Every refusal is a sentence
    /// about what is attached, which the composer knows.
    fn detach(&mut self, argument: Option<String>) {
        let Some(name) = argument else {
            let removed = self.composer.detach_all();
            self.notice = Some(if removed == 0 {
                Notice::info("nothing is attached")
            } else if removed == 1 {
                Notice::info("1 attachment removed")
            } else {
                Notice::info(format!("{removed} attachments removed"))
            });
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            let removed = self.composer.detach_all();
            self.notice = Some(Notice::info(format!("{removed} attachments removed")));
            return;
        }
        match self.composer.detach(name) {
            Ok(attachment) => {
                self.notice = Some(Notice::info(format!(
                    "{} detached",
                    attachment.pending.name()
                )));
            }
            Err(error) => self.notice = Some(Notice::error(error.message())),
        }
    }

    /// `/open <path>`: hand a workspace file to the configured program.
    ///
    /// Four refusals, all of them about what may be named: a path is *in the
    /// workspace*, so an absolute one and a `~` one are refused here rather than in
    /// the opener (which resolves what it is given), and an unset command is the
    /// effect's refusal because reading the setting is I/O.
    fn open_file(&mut self, argument: Option<String>) {
        let Some(workspace) = self.view.active_workspace_id else {
            self.notice = Some(Notice::error("no workspace is open"));
            return;
        };
        let Some(path) = argument else {
            self.notice = Some(Notice::error("/open needs an argument"));
            return;
        };
        let path = path.trim().to_owned();
        if path.is_empty() {
            self.notice = Some(Notice::error("/open needs an argument"));
            return;
        }
        if path.contains('\n') {
            self.notice = Some(Notice::error("/open takes one path on one line"));
            return;
        }
        if path.starts_with('~') {
            self.notice = Some(Notice::error(
                "/open takes a path inside this workspace, not ~",
            ));
            return;
        }
        if std::path::Path::new(&path).is_absolute() {
            self.notice = Some(Notice::error(
                "/open takes a path inside this workspace, not an absolute one",
            ));
            return;
        }
        self.notice = None;
        self.actions.push(Action::OpenFile {
            workspace,
            path,
            line: None,
        });
    }

    /// `Backspace` in the composer: take the last attachment off, or delete a
    /// character.
    ///
    /// The key is claimed by the map, so this is where it is handed back. An
    /// empty field has nothing to delete, which is what makes the attachment the
    /// thing the keystroke means; with text it belongs to the field, and refusing
    /// it there would be a key that silently stopped deleting.
    fn detach_last(&mut self, key: &ratatui::crossterm::event::KeyEvent) {
        if self.composer.text().is_empty() {
            match self.composer.detach_last() {
                Some(attachment) => {
                    self.notice = Some(Notice::info(format!(
                        "{} detached",
                        attachment.pending.name()
                    )));
                    return;
                }
                None => return,
            }
        }
        self.unclaimed(*key);
        self.refresh_completion();
    }

    /// Rename the open session to `title`, without the prompt.
    fn rename_to(&mut self, title: String) {
        let Some(session) = self.open_session().map(|session| session.id) else {
            self.notice = Some(Notice::info("no session is open"));
            return;
        };
        if title.trim().is_empty() {
            self.notice = Some(Notice::info("a title is required"));
            return;
        }
        self.actions.push(Action::RenameSession { session, title });
    }

    /// `Ctrl-T`: ask, then plan, then agent.
    ///
    /// A write to the session row, or to the draft when there is no row yet. The
    /// running actor keeps the model it was given; the next one reads the row, so
    /// there is nothing to restart and no reason to refuse mid-turn.
    fn cycle_mode(&mut self) {
        let next = match self.agent_mode() {
            AgentMode::Agent => AgentMode::Ask,
            AgentMode::Ask => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Agent,
        };
        self.set_mode(next);
        self.notice = Some(Notice::info(format!(
            "{} mode",
            crate::tui::state::facts::mode_label(next).to_lowercase()
        )));
    }

    /// Set the mode: the session's row, or the draft.
    ///
    /// The row is updated locally as well as written, so the rail and the facts row
    /// move on the keystroke rather than a round trip later; the outcome replaces it
    /// with what the service stored, which is the same value or a refusal.
    fn set_mode(&mut self, mode: AgentMode) {
        match self.open_session().map(|session| session.id) {
            Some(session) => {
                if let Some(slot) = self.sessions.iter_mut().find(|held| held.id == session) {
                    slot.mode = mode;
                }
                self.actions.push(Action::SetSessionMode { session, mode });
            }
            None => self.draft_mode = mode,
        }
    }

    /// `Ctrl-M`: the model and effort picker.
    fn open_models(&mut self) {
        self.confirm = None;
        self.completion = None;
        // Before the settings read lands there is no effective choice to show, so
        // the picker opens on what is stored rather than waiting: `Use default`
        // still means what it says.
        let effective = self.effective_choice().unwrap_or_else(|| EffectiveChoice {
            model: self.model_label(
                &self
                    .current_override()
                    .model
                    .unwrap_or_else(|| crate::domain::settings::keys::DEFAULT_MODEL.to_owned()),
            ),
            effort: self.current_override().reasoning_effort,
        });
        let override_ = self.current_override();
        self.picker = Some(Picker::new(&self.models, &override_, &effective));
        self.enter(Mode::Models);
    }

    fn close_models(&mut self) {
        self.picker = None;
        self.leave_mode(Mode::Models);
    }

    /// Write the picked row, then close.
    fn commit_pick(&mut self) {
        let Some(pick) = self.picker.as_ref().and_then(Picker::pick).cloned() else {
            return;
        };
        self.close_models();
        let mode = self.agent_mode();
        match self.open_session().map(|session| session.id) {
            Some(session) => self.actions.push(Action::SetSessionChoice {
                session,
                mode,
                pick,
            }),
            None => match pick.key() {
                ChoiceKey::Model => self.draft_model = pick.value(),
                ChoiceKey::Effort => self.draft_effort = pick.value(),
            },
        }
    }

    /// `Ctrl-L`: summarize older messages, behind the same confirmation a delete
    /// uses.
    ///
    /// The guard runs first: with nothing to compact or a turn in flight, asking a
    /// question would be noise, so the refusal is the answer.
    fn open_compact(&mut self) {
        if self.is_busy() {
            self.notice = Some(Notice::info(
                "a turn is running \u{2014} Esc stops it first",
            ));
            return;
        }
        let Some(session) = self.open_session().map(|session| session.id) else {
            self.notice = Some(Notice::info("no session is open"));
            return;
        };
        let title = self
            .open_session()
            .and_then(|session| session.title.clone())
            .unwrap_or_else(|| sessions::UNTITLED.to_owned());
        self.confirm = Some(Confirm::compact(session, &title));
        self.enter(Mode::Confirm);
    }

    /// `?` from the transcript, or `/help`.
    fn open_help(&mut self) {
        self.help.open();
        self.enter(Mode::Help);
    }

    fn close_help(&mut self) {
        self.leave_mode(Mode::Help);
    }

    // -- The settings surface ----------------------------------------------

    pub fn settings(&self) -> &SettingsView {
        &self.settings
    }

    pub fn audit(&self) -> &AuditLog {
        &self.audit
    }

    pub fn mcp_config(&self) -> Option<&McpPaths> {
        self.mcp_config.as_ref()
    }

    /// The rows the surface draws for the open section.
    ///
    /// Read-only rows are built here rather than in the view because they are
    /// app state — the workspace's name, the servers' status, the audit table —
    /// and because `Enter` has to know which rows write and which do not.
    pub fn settings_rows(&self) -> Vec<SettingsRow> {
        let section = self.settings.section();
        let mut rows = Vec::new();
        if section.leads_with_workspace() {
            let value = self
                .active_workspace()
                .map(|workspace| workspace.name.clone())
                .unwrap_or_else(|| "\u{2014}".to_owned());
            rows.push(SettingsRow::read_only("Workspace", value));
        }
        for field in section.fields() {
            let value = if self.settings.editing() == Some(field.key) {
                "editing\u{2026}".to_owned()
            } else {
                settings::value_text(field, self.settings.value(field.key))
            };
            rows.push(SettingsRow {
                field: Some(field),
                label: field.label.to_owned(),
                value,
            });
        }
        match section {
            Section::Mcp => self.mcp_rows(&mut rows),
            Section::Audit => self.audit_rows(&mut rows),
            _ => {}
        }
        rows
    }

    /// The MCP section: one row per server, then the two config files.
    fn mcp_rows(&self, rows: &mut Vec<SettingsRow>) {
        for server in self.status.servers() {
            let title = server.title.clone().unwrap_or_else(|| server.id.clone());
            rows.push(SettingsRow::read_only(
                title,
                format!("{} \u{b7} {} tool(s)", server.status, server.tool_count),
            ));
        }
        match &self.mcp_config {
            Some(paths) => {
                rows.push(SettingsRow::read_only(
                    "Your config",
                    paths.user_path.clone(),
                ));
                // A project file only applies once its hash is trusted, which is
                // the one fact about it a reader cannot see in the path.
                let value = if paths.project_enabled {
                    paths.project_path.clone()
                } else {
                    format!("{} \u{b7} not trusted", paths.project_path)
                };
                rows.push(SettingsRow::read_only("Project config", value));
                rows.push(SettingsRow::read_only(
                    "Logs",
                    format!("{}/<server>/", paths.log_root),
                ));
            }
            None => rows.push(SettingsRow::read_only("Config files", "reading\u{2026}")),
        }
    }

    /// The audit section: one row per call, newest first.
    fn audit_rows(&self, rows: &mut Vec<SettingsRow>) {
        if self.audit.is_empty() {
            rows.push(SettingsRow::read_only(
                "No calls yet",
                "this process has asked a service for nothing",
            ));
            return;
        }
        for entry in self.audit.entries() {
            rows.push(SettingsRow::read_only(
                format!("{} \u{b7} {}", entry.at.format("%H:%M:%S"), entry.call),
                format!(
                    "{} \u{b7} {} ms",
                    if entry.ok { "ok" } else { "failed" },
                    entry.duration_ms
                ),
            ));
        }
    }

    fn open_settings(&mut self) {
        self.confirm = None;
        self.completion = None;
        self.settings.open();
        self.request_settings();
        self.enter(Mode::Settings);
    }

    /// Leave the surface, or close the editor band first.
    ///
    /// `Esc` unwinds one level everywhere, and a batch of typed text is a level:
    /// `Ctrl-,` therefore takes two presses while a band is open, which is the
    /// same rule the transcript's selection follows.
    fn close_settings(&mut self) {
        if self.settings.editing().is_some() {
            self.settings.cancel();
            return;
        }
        self.leave_mode(Mode::Settings);
        self.focus = Focus::Composer;
    }

    /// Read every key the surface shows, and the MCP paths when they are wanted.
    fn request_settings(&mut self) {
        self.settings_generation += 1;
        self.actions.push(Action::LoadSettings {
            generation: self.settings_generation,
        });
        if self.settings.section() == Section::Mcp && self.mcp_config.is_none() {
            self.request_mcp_config();
        }
    }

    fn request_mcp_config(&mut self) {
        let Some(workspace) = self.view.active_workspace_id else {
            return;
        };
        self.actions.push(Action::LoadMcpConfig { workspace });
    }

    fn settings_step_section(&mut self, delta: isize) {
        self.settings.step_section(delta);
        if self.settings.section() == Section::Mcp && self.mcp_config.is_none() {
            self.request_mcp_config();
        }
        self.reconcile_settings();
    }

    /// Keep the row cursor inside the open section.
    fn reconcile_settings(&mut self) {
        let rows = self.settings_rows().len();
        self.settings.reconcile(rows);
    }

    /// `Enter` on the surface: open a band, flip a switch, or advance a choice.
    fn settings_apply(&mut self) {
        if self.settings.editing().is_some() {
            self.settings_commit();
            return;
        }
        let rows = self.settings_rows();
        let Some(row) = rows.get(self.settings.selected) else {
            return;
        };
        let Some(field) = row.field else {
            // A read-only row: only the audit table has anything to open.
            if self.settings.section() == Section::Audit {
                let index = self.settings.selected;
                self.audit.toggle(index);
            }
            return;
        };
        self.settings.clear_note();
        match field.kind {
            // The two kinds a terminal cannot guess at wait for the band.
            Kind::Text | Kind::Secret => {
                self.settings.begin(field);
            }
            Kind::Toggle => {
                let next = settings::toggled(self.settings.plain(field.key)).to_owned();
                self.settings.hold(field, Some(next.clone()));
                self.actions.push(Action::SaveSetting {
                    key: field.key,
                    value: Some(next),
                });
            }
            Kind::Choice(choice) => {
                let next = choice.next(self.settings.plain(field.key));
                self.settings.hold(field, next.clone());
                self.actions.push(Action::SaveSetting {
                    key: field.key,
                    value: next,
                });
            }
        }
    }

    /// `Enter` in the band: send the buffer, or clear the key when it is empty.
    ///
    /// An empty buffer on a secret sends nothing. `SettingsService::remove`
    /// refuses a secret, and the GUI's own `save()` returns early for the same
    /// reason; the alternative would be a write the store answers with a `400`.
    fn settings_commit(&mut self) {
        let Some(field) = self.settings.editing_field() else {
            return;
        };
        let text = self.settings.editor_text();
        let value = if text.is_empty() { None } else { Some(text) };
        if field.kind == Kind::Secret && value.is_none() {
            self.settings.cancel();
            return;
        }
        self.settings.hold(field, value.clone());
        self.actions.push(Action::SaveSetting {
            key: field.key,
            value,
        });
    }

    /// The surface's field, for a keystroke no binding claimed.
    fn settings_input(&mut self, key: &KeyEvent) {
        if self.settings.editing().is_some() {
            self.settings.editor_input(key);
        }
    }

    /// The draft's choice as the create command wants it.
    ///
    /// `chatStore.ts::draftConfig`, one for one: the mode, then that mode's
    /// override, and nothing at all when there is no override to write.
    fn draft_config(&self) -> ModelConfig {
        let mut config = ModelConfig::default();
        let override_ = config.for_mode_mut(self.draft_mode);
        override_.model = self.draft_model.clone();
        override_.reasoning_effort = self.draft_effort.clone();
        config
    }

    /// The session's override for the current mode, or the draft's.
    fn current_override(&self) -> ModeOverride {
        match self.open_session() {
            Some(session) => session.model_config.for_mode(session.mode).clone(),
            None => ModeOverride {
                model: self.draft_model.clone(),
                reasoning_effort: self.draft_effort.clone(),
            },
        }
    }

    /// What the next turn will use, resolved the way the runtime resolves it.
    ///
    /// Before the settings read lands, the caller's `None` is the honest answer: the
    /// band says the mode and nothing else rather than guessing a model id.
    pub fn effective_choice(&self) -> Option<EffectiveChoice> {
        let defaults = self.defaults.as_ref()?;
        let mode = self.agent_mode();
        let inherited = defaults.for_mode(mode);
        let override_ = self.current_override();
        Some(EffectiveChoice {
            model: override_
                .model
                .clone()
                .unwrap_or_else(|| inherited.model.clone()),
            effort: override_
                .reasoning_effort
                .clone()
                .or_else(|| inherited.effort.clone()),
        })
    }

    /// The models the picker offers.
    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    pub fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }

    pub fn help(&self) -> &HelpView {
        &self.help
    }

    /// The composer band's facts row.
    pub fn facts(&self) -> Facts {
        let effective = self.effective_choice();
        let usage = crate::tui::state::context::estimate(
            self.transcript.messages(),
            &self.composer.text(),
            self.effective_window(),
        );
        Facts::build(self.agent_mode(), effective.as_ref(), usage, |model| {
            self.model_label(model)
        })
    }

    /// The menu label for one model id.
    fn model_label(&self, model: &str) -> String {
        match self.models.iter().find(|info| info.id.as_str() == model) {
            Some(info) => crate::agent::providers::catalog::menu_label(info),
            None => model.to_owned(),
        }
    }

    /// The effective model's window, for the meter.
    fn effective_window(&self) -> Option<u64> {
        let effective = self.effective_choice()?;
        self.models
            .iter()
            .find(|info| info.id.as_str() == effective.model)
            .map(|info| info.context_window)
    }

    /// Recompute the completion menu from the field, or close it.
    ///
    /// Called after every key that reaches the field, so a typed character narrows
    /// the menu and a deleted one closes it.
    fn refresh_completion(&mut self) {
        if let Some(open) = self.completion.as_mut() {
            if open.refresh(self.composer.textarea(), &self.skills) {
                return;
            }
            self.completion = None;
            self.leave_mode(Mode::Complete);
            return;
        }
        self.completion = Completion::open(self.composer.textarea(), &self.skills);
        if self.completion.is_some() {
            self.enter(Mode::Complete);
        }
    }

    /// Accept the highlighted row: rewrite the token and close the menu.
    ///
    /// `delete_str` deletes forward from the caret, so the cursor goes to the
    /// token's first character first, then the token is replaced by the full name
    /// and a space. Submitting is a second `Enter`: a picked command takes one more
    /// keystroke, which is what stops a typo from running something.
    fn accept_completion(&mut self) {
        let Some(open) = self.completion.clone() else {
            return;
        };
        let Some((row, column, length, insert)) = open.rewrite(self.composer.textarea()) else {
            self.completion = None;
            self.leave_mode(Mode::Complete);
            return;
        };
        let field = self.composer.textarea_mut();
        field.move_cursor(CursorMove::Jump(u16::try_from(row).unwrap_or(0), column));
        field.delete_str(length);
        field.insert_str(insert);
        self.completion = None;
        self.leave_mode(Mode::Complete);
    }

    fn dismiss_completion(&mut self) {
        self.completion = None;
        self.leave_mode(Mode::Complete);
    }

    fn list_move_models(&mut self, delta: isize) {
        if let Some(picker) = self.picker.as_mut() {
            picker.move_by(delta);
        }
    }

    /// `q`: leave, but never mid-turn.
    ///
    /// The design's failure mode is explicit — quit with a turn running
    /// confirms, then stops before exiting — and T1 has no confirm dialog, so
    /// the smaller guarantee is taken: refuse, and name the interrupt. Dropping
    /// the process mid-turn is the one thing the transcript may not survive.
    fn quit_or_refuse(&mut self) {
        if self.is_busy() {
            self.notice = Some(Notice::info(
                "a turn is running \u{2014} Esc stops it, then q leaves",
            ));
            return;
        }
        self.quit = true;
    }

    /// `Ctrl-C`: refuse a paused batch, stop a running turn, or leave when idle.
    ///
    /// It keeps its T1 meanings. The new one comes first: a paused turn has no
    /// running actor, so `Stop` would be a no-op there, and refusing the calls is
    /// the only thing that resolves the pause.
    fn stop_or_quit(&mut self) {
        // Paused, it still declines the batch; running, it still stops the turn.
        if self.approval.is_some() {
            self.decline_all();
            return;
        }
        // A confirmation is one thing to get out of, and not worth a stopped
        // turn: `n` and `Esc` are the answers, this is the third way out.
        if self.confirm.is_some() {
            self.cancel_confirm();
            return;
        }
        if self.is_busy() || self.stopping {
            self.stop_turn();
            return;
        }
        // An overlay or a list view is idle configuration: closing it is what
        // the key means here, so browsing a list can never end the process.
        if self.mode() == Mode::Manage {
            self.close_manage();
            return;
        }
        // The three views that own the frame close the way their own `Esc` does.
        // Before this branch they fell through to `quit`, which the keyboard
        // table never promised: `Ctrl-C` closes an overlay, and reading the help
        // page is not a reason to lose the chat.
        if self.mode() == Mode::Settings {
            self.close_settings();
            return;
        }
        if self.mode() == Mode::Help {
            self.close_help();
            return;
        }
        if self.mode() == Mode::Detail {
            self.close_detail();
            return;
        }
        if self.sessions_open() || self.workspaces_open() {
            self.close_overlay();
            return;
        }
        self.quit = true;
    }

    /// `Esc`: stop the running turn, and otherwise do nothing permanent.
    ///
    /// Never quits. `Esc` is the key people reach for to call off what is
    /// happening; leaving is `q` (transcript focus, idle) or `Ctrl-C` (idle).
    /// Idle, it drops the card cursor and an informational notice, in that order,
    /// which is the "unwind one thing" the design gives it everywhere else.
    fn interrupt(&mut self) {
        // A selection is one level to unwind, and a running turn is another: with
        // something selected, the first `Esc` clears it and the second stops the
        // turn. That is the mode stack's rule applied to state that is not a mode.
        //
        // Clearing a selection hands the keyboard back to the composer. The keys
        // that were acting on the block stop meaning anything the moment it is
        // gone, and typing is what a chat is for, so the reader lands where they
        // would next want to be rather than on a transcript that no longer takes
        // their letters.
        if self.transcript.selection().is_some() {
            self.transcript.clear_selection();
            self.focus = Focus::Composer;
            return;
        }
        if !self.is_busy() && !self.stopping {
            if self
                .notice
                .as_ref()
                .is_some_and(|notice| !notice.is_error())
            {
                self.notice = None;
            }
            return;
        }
        self.stop_turn();
    }

    /// Ask the actor to cancel the turn. One in flight at a time.
    fn stop_turn(&mut self) {
        let Some(session) = self.view.open_session_id else {
            return;
        };
        if self.stopping {
            return;
        }
        self.stopping = true;
        self.notice = Some(Notice::info("stopping\u{2026}"));
        self.actions.push(Action::Stop { session });
    }

    // -- The pause ---------------------------------------------------------

    /// A `robi.agent.v1.awaiting_approval` frame for the open session.
    ///
    /// The frame names one call. The first one opens the modal and the reason
    /// field; a later one only extends the list, because a field the user chose
    /// to close must not reopen under their hands.
    fn on_awaiting_approval(&mut self, envelope: &EventEnvelope) {
        let Some(call) = tool_call_id(envelope) else {
            // Unreachable with the payload `envelope::from_core_event` builds, and
            // loud rather than silent when it stops being true: a pause that
            // cannot be read cannot be answered, and saying nothing would leave
            // the screen looking merely stale.
            tracing::error!(data = %envelope.data, "approval frame without a call id");
            self.status
                .set_error("a tool call is waiting for approval but the request could not be read");
            return;
        };
        self.activity.idle();
        self.composer.set_sending(false);
        self.transcript.set_phase(Phase::Idle);
        if let Some(session) = self.view.open_session_id {
            self.reload(session);
        }

        let queued = {
            let approval = self.approval.get_or_insert_with(Approval::new);
            approval.push(call);
            approval.take_queued(call)
        };
        // Take the keyboard and the view to the card that needs an answer. Without
        // this a pause arriving mid-transcript is invisible — there is no modal to
        // cover the frame, so the selection *is* the announcement — and the keys
        // that answer it would be unreachable without a `Tab`.
        //
        // `focus_pending` leaves a draft alone, which is what keeps a second call
        // in the same batch from taking the keyboard back mid-word.
        self.focus_pending(call);
        if !self.focused {
            // The title changes either way; the bell only when nobody is looking.
            self.bell = true;
        }
        // A queued decision is released here and nowhere else: this frame is the
        // only one that means the runtime is idle again.
        if let Some(decision) = queued {
            self.decide(call, decision);
        }
    }

    /// The call the transcript's selection names, when it is a card.
    fn selected_call(&self) -> Option<ToolCallId> {
        self.transcript.selected_call()
    }

    /// Select the card for `call`, wherever it sits in the transcript.
    ///
    /// The block is found from the messages rather than from the frame, so a
    /// pause whose row has not loaded yet simply selects nothing and the reload
    /// picks it up.
    /// Put the keyboard on the card for `call` and move focus to the transcript.
    ///
    /// A pause is a question the app asks, not one the user went looking for, so
    /// it takes the keyboard: leaving focus in the composer means the first `a`
    /// is a character in a message nobody meant to write, and the card the keys
    /// act on is one `Tab` away from being discoverable. Only from `chat` — a
    /// detail view or a reason field the user opened is not interrupted.
    fn focus_pending(&mut self, call: ToolCallId) {
        self.select_call(call);
        if self.mode() != Mode::Chat || self.transcript.selection().is_none() {
            return;
        }
        // Only from an empty composer. A draft is the reader's work in progress,
        // and an arriving pause is not a reason to take the caret out of it — the
        // orange rail and the selection still say something is waiting, and the
        // keyboard is one `Tab` away. An empty field has nothing to protect, and
        // its arrows select anyway, so the pause takes it.
        if self.composer.is_blank() {
            self.focus = Focus::Transcript;
        }
    }

    fn select_call(&mut self, call: ToolCallId) {
        let block = self.transcript.messages().iter().find_map(|message| {
            message
                .tool_calls
                .iter()
                .any(|holding| holding.id == call)
                .then_some(crate::tui::state::transcript::BlockRef::call(
                    message.id, call,
                ))
        });
        if let Some(block) = block {
            self.transcript.select_block(block);
        }
    }

    /// The selected call, if it is one the loop is still waiting on.
    ///
    /// The approval keys act only here: `a` on a card that already ran, or on a
    /// paragraph, is a keystroke that would otherwise approve something the user
    /// did not mean.
    fn selected_pending(&self) -> Option<ToolCallId> {
        let call = self.selected_call()?;
        let pending = self
            .approval
            .as_ref()
            .is_some_and(|approval| approval.pending().contains(&call));
        pending.then_some(call)
    }

    /// Move the selection, and keep the reason field's label in step.
    ///
    /// `Up` in an empty composer reaches the transcript and takes the keyboard
    /// with it: the field has no caret to move, and selecting is what the reader
    /// is asking for. With text in the field the arrows stay the caret's.
    fn select(&mut self, delta: isize) {
        if self.mode() == Mode::Chat && self.focus.is_composer() {
            if !self.composer.is_blank() {
                return;
            }
            self.focus = Focus::Transcript;
        }
        self.transcript.select(delta);
        self.sync_reason_label();
    }

    fn approve(&mut self) {
        match self.selected_pending() {
            Some(call) => self.decide(call, Decision::Approve),
            None => self.not_pending(),
        }
    }

    fn approve_all(&mut self) {
        let Some(call) = self.selected_pending() else {
            self.not_pending();
            return;
        };
        if let Some(approval) = self.approval.as_mut() {
            approval.queue_for_all(Decision::Approve);
        }
        self.decide(call, Decision::Approve);
    }

    /// The one-line answer when an approval key is pressed off-target.
    fn not_pending(&mut self) {
        self.notice = Some(Notice::info(match self.transcript.selection() {
            Some(_) => "that call is not waiting for approval",
            None => "nothing is selected \u{2014} \u{2191}\u{2193} picks a card",
        }));
    }

    /// Refuse every waiting call, which is what `Ctrl-C` means while paused.
    fn decline_all(&mut self) {
        let Some(call) = self.selected_pending().or_else(|| {
            self.approval
                .as_ref()
                .and_then(|approval| approval.pending().first().copied())
        }) else {
            return;
        };
        if let Some(approval) = self.approval.as_mut() {
            approval.queue_for_all(Decision::Reject { reason: None });
        }
        self.decide(call, Decision::Reject { reason: None });
    }

    /// Open the reason field with whatever was last typed into it.
    fn open_reason(&mut self) {
        let Some(call) = self.selected_pending() else {
            self.not_pending();
            return;
        };
        let _ = call;
        let Some(reason) = self
            .approval
            .as_ref()
            .map(|approval| approval.reason.clone())
        else {
            return;
        };
        let mut prompt = Prompt::new(PromptKind::RejectReason);
        prompt.field = reason;
        self.prompt = Some(prompt);
        self.enter(Mode::Prompt);
        self.sync_reason_label();
    }

    /// `Esc` in the field: put the text back and hand the keyboard to the card.
    fn close_reason(&mut self) {
        if let (Some(prompt), Some(approval)) = (self.prompt.take(), self.approval.as_mut()) {
            approval.reason = prompt.field;
        }
        self.leave_mode(Mode::Prompt);
    }

    /// `Enter` in the field: refuse the selected call, with the typed reason.
    fn reject_with_reason(&mut self) {
        // Close first: the field's live text is copied back into
        // `approval.reason` on the way out, so reading before that would send
        // whatever was in the buffer when the field opened.
        self.close_reason();
        let reason = self
            .approval
            .as_ref()
            .and_then(|approval| approval.reason_text());
        if let Some(call) = self.selected_pending() {
            self.decide(call, Decision::Reject { reason });
        }
    }

    /// The label over the field names the selected call.
    fn sync_reason_label(&mut self) {
        let label = {
            let call = self.selected_call().and_then(|id| self.call(id));
            self.approval
                .as_ref()
                .map(|approval| approval.reason_label(call))
                .unwrap_or_default()
        };
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.label = label;
        }
    }

    /// Issue one decision. One in flight at a time, which is what the runtime
    /// can take: it refuses a second decide while the turn is running.
    fn decide(&mut self, call: ToolCallId, decision: Decision) {
        let Some(session) = self.view.open_session_id else {
            return;
        };
        let Some(approval) = self.approval.as_mut() else {
            return;
        };
        if approval.busy() {
            return;
        }
        approval.set_busy(true);
        self.actions.push(Action::Decide {
            session,
            call,
            decision,
        });
    }

    /// `Enter` on a card: read the whole body in a view of its own.
    ///
    /// On a paragraph it does nothing, which is the answer the design gives for
    /// now: a message has no second view to open.
    fn open_detail(&mut self) {
        let Some(call_id) = self.selected_call() else {
            return;
        };
        let Some(call) = self.call(call_id).cloned() else {
            return;
        };
        let original = self.transcript.original(call_id).map(str::to_owned);
        let phase = self.transcript.phase();
        self.detail = Some(Detail::new(
            &call,
            phase,
            original.as_deref(),
            usize::from(view::content_width(self.width)),
        ));
        self.enter(Mode::Detail);
        // A compressed result keeps the uncapped body elsewhere; opening the view
        // is the one moment it is worth asking for.
        if let (Some(original_id), Some(session)) =
            (call.original_id.clone(), self.view.open_session_id)
        {
            if !self.transcript.render().originals.contains_key(&call_id) {
                self.actions.push(Action::LoadOriginal {
                    session,
                    call: call_id,
                    original_id,
                });
            }
        }
    }

    /// `Esc` in the detail view: back to the transcript, selection intact.
    fn close_detail(&mut self) {
        self.detail = None;
        self.leave_mode(Mode::Detail);
    }

    // -- The overlays ------------------------------------------------------

    /// `Ctrl-S`: the sessions overlay, on the open session.
    fn open_sessions(&mut self) {
        self.confirm = None;
        if !self.sessions_open() {
            let rows = self.session_ids();
            self.sessions_view.open();
            // Start on the session that is on screen, so `Enter` keeps the
            // reader where they were; the action row is one `Up` away.
            self.sessions_view
                .set_selected(self.view.open_session_id, &rows);
            // One surface at a time: `Esc` must close this one, not step back to
            // whichever list was opened before it.
            self.leave_mode(Mode::Workspaces);
            self.leave_mode(Mode::Manage);
            self.enter(Mode::Sessions);
        }
        self.reconcile_sessions_view();
    }

    /// `Ctrl-W`: the workspace overlay, on the recent screen.
    fn open_workspaces(&mut self) {
        self.confirm = None;
        if !self.workspaces_open() {
            let recent: Vec<&Workspace> =
                workspaces::order_recent(&self.workspaces, &self.view.recent_workspace_ids)
                    .into_iter()
                    .take(RECENT_LIMIT)
                    .collect();
            self.workspaces_view
                .open(self.view.active_workspace_id, &recent);
            self.leave_mode(Mode::Sessions);
            self.leave_mode(Mode::Manage);
            self.enter(Mode::Workspaces);
        }
    }

    /// `Ctrl-N` from `chat`: the landing page, and **no session yet**.
    ///
    /// Creating the row here is what left an empty chat behind whenever a reader
    /// opened a workspace, looked around, and left. The row appears when the first
    /// message is sent (`Action::StartSession`), so an abandoned new chat costs
    /// nothing.
    fn new_session(&mut self) {
        let Some(_workspace) = self.view.active_workspace_id else {
            self.notice = Some(Notice::info("open a workspace before starting a chat"));
            return;
        };
        // The overlay's action row and `Ctrl-N` mean the same thing: both land on
        // the greeting with the composer focused.
        self.leave_mode(Mode::Sessions);
        self.select_session(None);
        self.focus = Focus::Composer;
    }

    /// `Ctrl-N` in the workspace overlay, or in the manage view: the create
    /// screen, empty.
    fn new_workspace(&mut self) {
        if self.mode() == Mode::Manage {
            self.leave_mode(Mode::Manage);
            self.enter(Mode::Workspaces);
        }
        self.workspaces_view.screen = Screen::New;
        self.workspaces_view.selected = 0;
        self.workspaces_view.reset_path();
        self.request_candidates();
    }

    /// Ask the effect for the directories the path field could complete to.
    fn request_candidates(&mut self) {
        self.paths_generation += 1;
        self.actions.push(Action::CompletePath {
            text: self.workspaces_view.path(),
            generation: self.paths_generation,
        });
    }

    fn accept_candidate(&mut self) {
        if self.mode() != Mode::Workspaces || self.workspaces_view.screen != Screen::New {
            return;
        }
        if self.workspaces_view.accept_candidate() {
            self.request_candidates();
        }
    }

    /// `Ctrl-R`: rename the selected session, or the open one from `chat`.
    fn open_rename(&mut self) {
        let session = match self.mode() {
            Mode::Sessions => self.selected_session().cloned(),
            _ => self.open_session().cloned(),
        };
        let Some(session) = session else {
            self.notice = Some(Notice::info("no session to rename"));
            return;
        };
        let title = sessions::display_title(&session).to_owned();
        self.open_rename_prompt(session.id, &title);
    }

    /// Open the one-line field on a title, pre-filled and at its end.
    fn open_rename_prompt(&mut self, session: SessionId, title: &str) {
        let mut prompt = Prompt::new(PromptKind::RenameSession(session));
        prompt.label = "rename".to_owned();
        prompt.field.insert_str(title);
        prompt.field.move_cursor(CursorMove::End);
        self.prompt = Some(prompt);
        self.enter(Mode::Prompt);
    }

    /// `Ctrl-D` or `Del`: ask before removing the selected row.
    fn delete_selected(&mut self) {
        match self.mode() {
            Mode::Sessions => {
                let Some(session) = self.selected_session() else {
                    return;
                };
                let title = sessions::display_title(session).to_owned();
                let id = session.id;
                let running = self.is_running(id);
                self.confirm = Some(Confirm::delete_session(id, &title, running));
                self.enter(Mode::Confirm);
            }
            Mode::Manage => {
                let Some(workspace) = self.manage_selected() else {
                    return;
                };
                let id = workspace.id;
                let name = workspace.name.clone();
                self.confirm = Some(Confirm::delete_workspace(id, &name));
                self.enter(Mode::Confirm);
            }
            _ => {}
        }
    }

    fn confirm_yes(&mut self) {
        let Some(confirm) = self.confirm.take() else {
            return;
        };
        self.leave_mode(Mode::Confirm);
        match confirm.target {
            Target::DeleteSession(session) => {
                self.actions.push(Action::DeleteSession { session });
            }
            Target::DeleteWorkspace(workspace) => {
                self.actions.push(Action::DeleteWorkspace { workspace });
            }
            Target::Compact(session) => {
                self.actions.push(Action::Compact { session });
            }
        }
    }

    fn cancel_confirm(&mut self) {
        self.confirm = None;
        self.leave_mode(Mode::Confirm);
    }

    /// `Enter` on the row or the screen under the cursor.
    fn overlay_open(&mut self) {
        // The picker's `Enter` is a write and a close, and nothing below applies.
        if self.mode() == Mode::Models {
            self.commit_pick();
            return;
        }
        match self.mode() {
            Mode::Sessions => {
                let rows = self.session_ids();
                let Some(id) = sessions::session_at(&rows, self.sessions_view.selected()) else {
                    // The action row: a fresh chat in the workspace that is
                    // already active, so the overlay closes on the field.
                    self.leave_mode(Mode::Sessions);
                    self.new_session();
                    return;
                };
                self.leave_mode(Mode::Sessions);
                self.open_from_list(id);
                self.focus = Focus::Composer;
            }
            Mode::Workspaces => match self.workspaces_view.screen {
                Screen::New => self.commit_workspace(),
                Screen::Recent => {
                    // The two action rows follow the recent list.
                    let len = self.workspace_rows().len();
                    let selected = self.workspaces_view.selected;
                    if selected == len {
                        self.open_manage();
                    } else if selected > len {
                        self.new_workspace();
                    } else {
                        let id = self.workspace_rows().get(selected).map(|w| w.id);
                        if let Some(id) = id {
                            self.leave_mode(Mode::Workspaces);
                            self.select_workspace(Some(id));
                            self.focus = Focus::Composer;
                        }
                    }
                }
            },
            Mode::Manage => {
                let id = self.manage_rows().get(self.manage.selected).map(|w| w.id);
                if let Some(id) = id {
                    self.leave_mode(Mode::Manage);
                    self.select_workspace(Some(id));
                    self.focus = Focus::Composer;
                }
            }
            _ => {}
        }
    }

    /// Open the dedicated manage view on the active workspace.
    ///
    /// It replaces the switcher rather than stacking on it: managing is what the
    /// reader is doing now, and one `Esc` should take them back to the chat, not
    /// to a menu they have finished with.
    fn open_manage(&mut self) {
        let active_index = self.view.active_workspace_id.and_then(|id| {
            let rows = workspaces::filtered(&self.workspaces, "");
            rows.iter().position(|workspace| workspace.id == id)
        });
        self.manage.open(active_index);
        self.leave_mode(Mode::Workspaces);
        self.enter(Mode::Manage);
    }

    fn close_manage(&mut self) {
        self.confirm = None;
        self.leave_mode(Mode::Manage);
        self.focus = Focus::Composer;
    }

    /// Keep the manage view's selection inside the filtered list.
    fn reconcile_manage(&mut self) {
        let rows = self.manage_rows().len();
        self.manage.reconcile(rows);
    }

    /// `Esc` in an overlay: step back a screen, then close.
    fn close_overlay(&mut self) {
        match self.mode() {
            Mode::Workspaces if self.workspaces_view.screen != Screen::Recent => {
                self.workspaces_view.screen = Screen::Recent;
                let active = self.view.active_workspace_id;
                let position = self
                    .workspace_rows()
                    .iter()
                    .position(|workspace| Some(workspace.id) == active);
                self.workspaces_view.selected = position.unwrap_or(0);
            }
            Mode::Sessions => self.leave_mode(Mode::Sessions),
            Mode::Workspaces => self.leave_mode(Mode::Workspaces),
            Mode::Models => {
                self.picker = None;
                self.leave_mode(Mode::Models);
            }
            _ => {}
        }
        self.confirm = None;
    }

    /// `Enter` on the create screen: open (or reuse) the typed path.
    fn commit_workspace(&mut self) {
        let root = self.workspaces_view.path();
        if root.trim().is_empty() {
            self.notice = Some(Notice::error("a path is required"));
            return;
        }
        self.actions.push(Action::CreateWorkspace { root });
    }

    fn list_move(&mut self, delta: isize) {
        match self.mode() {
            Mode::Sessions => {
                let rows = self.session_ids();
                self.sessions_view.select(delta, &rows);
            }
            Mode::Workspaces => match self.workspaces_view.screen {
                Screen::New => self.workspaces_view.select_candidate(delta),
                // The recent screen has the two action rows after its list.
                Screen::Recent => {
                    let rows = self.workspace_rows().len() + 2;
                    self.workspaces_view.select(delta, rows);
                }
            },
            Mode::Manage => {
                let rows = self.manage_rows().len();
                self.manage.select(delta, rows);
            }
            Mode::Models => self.list_move_models(delta),
            Mode::Settings => {
                let rows = self.settings_rows().len();
                self.settings.select(delta, rows);
            }
            Mode::Complete => {
                if let Some(open) = self.completion.as_mut() {
                    open.move_by(delta);
                }
            }
            _ => {}
        }
    }

    fn list_page(&mut self, delta: isize) {
        match self.mode() {
            Mode::Sessions => {
                let rows = self.session_ids();
                self.sessions_view.page(delta, &rows, LIST_PAGE);
            }
            // A page of candidates is the whole list; a page of workspaces is
            // `LIST_PAGE` rows.
            Mode::Workspaces if self.workspaces_view.screen == Screen::New => {
                self.workspaces_view.select_candidate(delta);
            }
            Mode::Workspaces => {
                let extra = usize::from(self.workspaces_view.screen == Screen::Recent) * 2;
                let rows = self.workspace_rows().len() + extra;
                let delta = delta * LIST_PAGE as isize;
                self.workspaces_view.select(delta, rows);
            }
            Mode::Manage => {
                let rows = self.manage_rows().len();
                self.manage.select(delta * LIST_PAGE as isize, rows);
            }
            Mode::Models => self.list_move_models(delta * LIST_PAGE as isize),
            Mode::Settings => {
                let rows = self.settings_rows().len();
                self.settings.page(delta, rows, LIST_PAGE);
            }
            Mode::Complete => {
                if let Some(open) = self.completion.as_mut() {
                    open.move_by(delta * LIST_PAGE as isize);
                }
            }
            _ => {}
        }
    }

    /// Keep the sessions overlay's selection on a row that still exists.
    fn reconcile_sessions_view(&mut self) {
        let rows = self.session_ids();
        self.sessions_view.reconcile(&rows);
    }

    /// Open `id` from the list, whether or not it is the session already open.
    ///
    /// "Open" always means "show me this transcript", so a row that is already
    /// the open one still issues a read. The earlier version did nothing in that
    /// case, which left an empty frame whenever the open session had never been
    /// read — the first row after a restart, and the reason `Enter` on it looked
    /// broken until another session had been opened and come back from.
    fn open_from_list(&mut self, id: SessionId) {
        if Some(id) == self.view.open_session_id {
            self.reload(id);
            return;
        }
        self.select_session(Some(id));
    }

    fn session_ids(&self) -> Vec<SessionId> {
        self.session_rows()
            .into_iter()
            .map(|(session, _)| session.id)
            .collect()
    }

    /// `Enter` in a one-line prompt: the action its kind names.
    fn submit_prompt(&mut self) {
        let Some(kind) = self.prompt.as_ref().map(|prompt| prompt.kind) else {
            return;
        };
        match kind {
            PromptKind::RejectReason => self.reject_with_reason(),
            PromptKind::RenameSession(session) => self.commit_rename(session),
        }
    }

    /// `Esc` in a prompt: the reason keeps its text, a rename is dropped.
    fn cancel_prompt(&mut self) {
        match self.prompt.as_ref().map(|prompt| prompt.kind) {
            Some(PromptKind::RejectReason) => self.close_reason(),
            _ => {
                self.prompt = None;
                self.leave_mode(Mode::Prompt);
            }
        }
    }

    /// `Enter` in the rename field.
    ///
    /// An empty title is refused here rather than by the service, which would
    /// return a `BadRequest` for a keystroke the field can answer itself. The
    /// field stays open so the title can be typed.
    fn commit_rename(&mut self, session: SessionId) {
        let Some(title) = self
            .prompt
            .as_ref()
            .map(|prompt| prompt.text().trim().to_owned())
        else {
            return;
        };
        if title.is_empty() {
            self.notice = Some(Notice::error("a title is required"));
            return;
        }
        let unchanged = self
            .sessions
            .iter()
            .find(|held| held.id == session)
            .is_some_and(|held| held.title.as_deref() == Some(title.as_str()));
        self.prompt = None;
        self.leave_mode(Mode::Prompt);
        if unchanged {
            return;
        }
        self.pending_rename = Some((session, title.clone()));
        self.notice = None;
        self.actions.push(Action::RenameSession { session, title });
    }

    /// How many dots to draw after a waiting card's label: 1, 2, 3, 1, …
    ///
    /// Counted in ticks, not wall clock, so the pump stays a pure function of the
    /// messages it has seen. Four ticks of 250 ms is one dot per second: fast
    /// enough to read as motion, slow enough not to pull the eye off the text.
    pub fn pending_dots(&self) -> usize {
        ((self.ticks / TICKS_PER_DOT) % 3) as usize + 1
    }

    /// Rows the detail view's body has: the frame minus its title and hint rows.
    ///
    /// `view::detail` lays the frame out as one row, `Min(1)`, one row, so this
    /// has to agree with it or the position it prints is a lie.
    pub fn detail_height(&self) -> usize {
        usize::from(self.height.saturating_sub(2)).max(1)
    }

    /// Drop the pause and every mode that rode on it.
    ///
    /// The selection is left alone: it is the reader's place in the transcript,
    /// not part of the pause.
    fn close_approval(&mut self) {
        self.approval = None;
        self.prompt = None;
        self.detail = None;
        while self.mode() != Mode::Chat {
            self.modes.pop();
        }
    }

    /// Pop `mode` and anything above it.
    fn leave_mode(&mut self, mode: Mode) {
        if let Some(index) = self.modes.iter().position(|held| *held == mode) {
            self.modes.truncate(index);
        }
        if self.modes.is_empty() {
            self.modes.push(Mode::Chat);
        }
    }

    /// One 250 ms tick: advance the elapsed time, and reload if the frames have
    /// gone quiet.
    ///
    /// The catch-up exists because the bus drops frames under backpressure. A
    /// row can be stored while the event that would have painted it was thrown
    /// away, so a turn that stops hearing anything assumes the worst and
    /// refetches.
    fn on_tick(&mut self) {
        // The view's only clock. Counted before anything can return early, so the
        // dots advance whether or not a turn is running.
        self.ticks = self.ticks.wrapping_add(1);
        if self.status.due() {
            self.actions.push(Action::Probe);
        }
        let quiet = self.activity.tick();
        self.transcript.set_phase(self.activity.phase());
        if !quiet {
            return;
        }
        self.activity.caught_up();
        if let Some(session) = self.view.open_session_id {
            self.reload(session);
        }
    }

    /// One frame from the bus.
    fn on_agent(&mut self, envelope: EventEnvelope) {
        // The list, the header title, and the marks come from the session rows,
        // so any front end's change refetches them. The transcript is untouched:
        // a rename in another window is not a reason to reread the messages.
        match envelope.event_type.as_str() {
            SESSION_CREATED | SESSION_UPDATED | SESSION_DELETED => {
                self.load_sessions();
                return;
            }
            MCP_STATUS => {
                if envelope.subject == self.active_workspace_key() {
                    self.load_mcp();
                }
                return;
            }
            _ => {}
        }
        if envelope.event_type == APP_ERROR {
            // An app-level failure is a fact about the process, so it belongs in
            // the status strip rather than in the footer's last-answer slot.
            let text = envelope
                .data
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("an error was reported")
                .to_owned();
            self.status.set_error(text);
            return;
        }

        let Some(open) = self.view.open_session_id else {
            return;
        };
        // A turn that starts or ends in another session is what makes an
        // overlay's mark live; while the overlay is up, one read follows it.
        if self.mode() == Mode::Sessions
            && matches!(
                envelope.event_type.as_str(),
                TURN_STARTED | TURN_FINISHED | AWAITING_APPROVAL
            )
        {
            self.load_sessions();
        }
        if envelope.subject != open.to_string() {
            // Another session's turn. T3 owns the list, but a pause it cannot
            // answer is the one frame worth reporting from here: the turn is
            // stopped until someone decides it.
            if envelope.event_type == AWAITING_APPROVAL {
                self.bell |= !self.focused;
                self.notice = Some(Notice::info("another session is waiting for approval"));
            }
            // Whether that actor is live is what the phase is rebuilt from when
            // the reader switches to it, so the two frames that change it are
            // recorded rather than waited for: a list read is a round trip, and
            // the switch can land first.
            if let Some(other) = session_id(&envelope) {
                match envelope.event_type.as_str() {
                    TURN_STARTED => {
                        self.running.insert(other);
                    }
                    TURN_FINISHED => {
                        self.running.remove(&other);
                    }
                    _ => {}
                }
            }
            return;
        }

        match envelope.event_type.as_str() {
            TURN_STARTED => {
                self.activity.start(Phase::Thinking);
                // The overlay's mark must not wait for a list read.
                self.running.insert(open);
            }
            MESSAGE_DELTA => {
                let kind = envelope
                    .data
                    .get("delta")
                    .and_then(|delta| delta.get("kind"))
                    .and_then(|kind| kind.as_str());
                self.activity.note(DeltaKind::from_name(kind));
            }
            MESSAGE_ADDED | MESSAGE_UPDATED | TOOL_CALL_UPDATED => {
                // A stored row: refetch that row, never render from the frame.
                self.activity.frame();
                if let Some(message) = message_id(&envelope) {
                    self.load_message(open, message);
                }
            }
            AWAITING_APPROVAL => self.on_awaiting_approval(&envelope),
            TURN_FINISHED => {
                self.activity.idle();
                self.composer.set_sending(false);
                self.stopping = false;
                self.running.remove(&open);
                if turn_paused(&envelope) {
                    // The actor exited on purpose: the turn is waiting for a
                    // decision, not over. Closing the modal here is what made a
                    // live pause look like one from an earlier run, so this
                    // frame only refreshes the transcript under it.
                    self.reload(open);
                    self.sync_reason_label();
                } else {
                    self.close_approval();
                    if let Some(message) = failed_turn(&envelope) {
                        self.status.set_error(message);
                    }
                    self.reload(open);
                }
            }
            TRANSCRIPT_COMPACTED => self.reload(open),
            _ => {}
        }
        self.transcript.set_phase(self.activity.phase());
    }

    /// Issue a full reload and record the generation it belongs to.
    fn reload(&mut self, session: SessionId) {
        let generation = self.transcript.next_generation();
        self.actions.push(Action::Reload {
            session,
            generation,
        });
    }

    /// Refetch one row, at the generation that is current now.
    ///
    /// The generation only has to make a full reload supersede this read; two
    /// reads of one id are upserts and cannot conflict.
    fn load_message(&mut self, session: SessionId, message: MessageId) {
        let generation = self.transcript.generation();
        self.actions.push(Action::LoadMessage {
            session,
            message,
            generation,
        });
    }

    fn on_done(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Models {
                generation,
                models,
                defaults,
            } => {
                if generation != self.models_generation {
                    return;
                }
                self.models = models;
                self.defaults = Some(defaults);
                self.status.clear_error();
            }
            ActionOutcome::Skills {
                workspace,
                generation,
                skills,
            } => {
                // A list for a workspace the reader has left is dropped, the way a
                // stale session read is.
                if generation != self.skills_generation
                    || self.view.active_workspace_id != Some(workspace)
                {
                    return;
                }
                self.skills = skills;
                self.status.clear_error();
            }
            ActionOutcome::SessionUpdated(session) => {
                let session = *session;
                self.status.clear_error();
                if let Some(slot) = self.sessions.iter_mut().find(|held| held.id == session.id) {
                    *slot = session.clone();
                }
                // Nothing to rebuild: the rail's fallback is read from the session
                // at draw time, so an unstamped row follows the write on the next
                // frame without the transcript knowing a mode exists.
            }
            ActionOutcome::Compacted { session } => {
                self.status.clear_error();
                if self.view.open_session_id == Some(session) {
                    // The rewrite is announced on the bus; this only says the job
                    // was accepted, so a reader is not left wondering.
                    self.notice = Some(Notice::info("compacting\u{2026}"));
                }
            }
            ActionOutcome::Workspaces {
                generation,
                workspaces,
            } => {
                if generation != self.workspaces_generation {
                    return;
                }
                self.workspaces = workspaces;
                self.status.clear_error();
                if !self.target_checked {
                    // The first listing decides the workspace, from the path the
                    // process was started on and nothing else.
                    self.target_checked = true;
                    match self.pick_startup_workspace() {
                        Some(id) => {
                            self.leave_mode(Mode::WorkspaceMissing);
                            self.select_workspace(Some(id));
                        }
                        None => {
                            // The path has no workspace. Nothing else is opened:
                            // a different workspace on screen would answer a
                            // question the reader did not ask.
                            self.select_workspace(None);
                            self.enter(Mode::WorkspaceMissing);
                        }
                    }
                } else if let Some(active) = self.view.active_workspace_id {
                    // A refresh keeps the reader where they are unless the row
                    // they are on has gone.
                    if !self
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.id == active)
                    {
                        let next = self.workspaces.first().map(|workspace| workspace.id);
                        self.select_workspace(next);
                    }
                }
                // The row selection indexes a list that just changed.
                if self.workspaces_view.selected >= self.workspace_rows().len() + 2 {
                    self.workspaces_view.selected = 0;
                }
                self.reconcile_manage();
            }
            ActionOutcome::Sessions {
                workspace,
                generation,
                sessions,
                running,
            } => {
                if Some(workspace) != self.view.active_workspace_id {
                    // A listing for a workspace that is no longer active.
                    return;
                }
                if generation != self.sessions_generation {
                    return;
                }
                self.sessions = sessions;
                self.running = running.into_iter().collect();
                self.status.clear_error();
                // The first listing opens a session and **reads it**. A
                // remembered `open_session_id` is already in the list, so the
                // "does the open session still exist" check below would skip the
                // reload — and the process starts with an empty transcript it
                // never asked for. One read per process is the fix; the rows are
                // what tell us this is the first.
                let first = !self.sessions_loaded;
                self.sessions_loaded = true;
                let open = self.view.open_session_id;
                let listed = open.is_some_and(|id| self.sessions.iter().any(|s| s.id == id));
                if first || !listed {
                    let picked = lifecycle::pick_session(&self.sessions, workspace, open);
                    if picked.is_some() || open.is_some() {
                        self.select_session(picked);
                        // Unless the selection itself had something to say: a
                        // session another process holds sets the footer notice,
                        // and clearing it here would hide the one refusal the
                        // reader cannot get anywhere else.
                        if self.held_by.is_none() {
                            self.notice = None;
                        }
                    }
                }
                let rows = self.session_ids();
                self.sessions_view.reconcile(&rows);
            }
            ActionOutcome::Transcript {
                session,
                generation,
                messages,
            } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                if generation != self.transcript.generation() {
                    // A newer reload of this session is in flight or landed.
                    return;
                }
                // A paused turn is answerable from a transcript that was just
                // loaded, not only from a frame that arrived live: reopening a
                // session paused by an earlier run has to reach the card the
                // same way, or the pause is on screen with no way to answer it.
                let pending = pending_calls(&messages);
                self.transcript.set_messages(messages);
                if self.held_by.is_none() {
                    self.notice = None;
                }
                self.status.clear_error();
                self.note_locked_results();
                self.open_approval_for(pending);
                // The pause labels its field before the reload lands, so the
                // call it names may only have arrived now.
                self.sync_reason_label();
            }
            ActionOutcome::Submitted { session, outcome } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                self.composer.set_sending(false);
                match outcome {
                    SubmitOutcome::Accepted => {
                        // Only a send that was taken clears the buffer; a
                        // refused one has to leave it exactly as it was.
                        self.composer.take_sent(Some(session));
                        self.activity.start(Phase::Thinking);
                        self.running.insert(session);
                        self.notice = None;
                        // The loop rejects a paused turn's waiting calls when a
                        // new instruction arrives, so the pause is over here too.
                        self.close_approval();
                    }
                    SubmitOutcome::AwaitingApproval => {
                        self.activity.idle();
                        self.notice = Some(Notice::info(
                            "nothing was sent \u{2014} the turn is waiting for approval",
                        ));
                    }
                }
            }
            ActionOutcome::Stopped { session } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                self.stopping = false;
                self.activity.idle();
                self.composer.set_sending(false);
                self.running.remove(&session);
                self.notice = Some(Notice::info("stopped"));
                // A cancelled turn still has to render as it ended, even when
                // the `turn_finished` frame was dropped.
                self.reload(session);
            }
            ActionOutcome::Decided { session, call } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                if let Some(approval) = self.approval.as_mut() {
                    approval.set_busy(false);
                    approval.remove(call);
                }
                // The runtime resumed the turn, so the phase moves until the
                // next frame says otherwise.
                self.activity.start(Phase::Thinking);
                self.transcript.set_phase(Phase::Thinking);
                self.running.insert(session);
                self.notice = if self
                    .approval
                    .as_ref()
                    .is_none_or(|approval| approval.is_empty())
                {
                    self.close_approval();
                    None
                } else {
                    Some(Notice::info(
                        "one call decided \u{2014} the next one is waiting",
                    ))
                };
                // The next decision is released by `awaiting_approval`, not from
                // here: the actor is running now, and `decide` would be refused.
            }
            ActionOutcome::DecideFailed {
                session,
                call,
                message,
            } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                if let Some(approval) = self.approval.as_mut() {
                    approval.set_busy(false);
                    // The call stays listed: a `Conflict` means the frame that
                    // lists it again is already on its way.
                    approval.push(call);
                }
                // The footer answers the keystroke; the strip keeps it. A refusal
                // because another process owns the chat lands here.
                self.status.set_error(message.clone());
                self.notice = Some(Notice::error(message));
            }
            ActionOutcome::Original {
                session,
                call,
                text,
            } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                self.loading_originals.remove(&call);
                self.transcript.set_original(call, text);
            }
            ActionOutcome::OriginalFailed {
                session,
                call,
                message,
            } => {
                // Forget the attempt, so closing and reopening the card retries
                // rather than showing the compressed body forever.
                self.loading_originals.remove(&call);
                if Some(session) == self.view.open_session_id {
                    self.status.set_error(message);
                }
            }
            ActionOutcome::Message {
                session,
                generation,
                message,
            } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                if generation != self.transcript.generation() {
                    // A full reload supersedes a per-row read.
                    return;
                }
                self.transcript.upsert(*message);
            }
            ActionOutcome::WorkspaceOpened { workspace, created } => {
                let workspace = *workspace;
                let id = workspace.id;
                let name = workspace.name.clone();
                self.status.clear_error();
                self.workspaces.retain(|held| held.id != id);
                self.workspaces.insert(0, workspace);
                self.notice = Some(Notice::info(if created {
                    format!("opened workspace {name}")
                } else {
                    format!("workspace {name} was already open")
                }));
                // The create screen and the missing-workspace prompt both go
                // away, and the new workspace is on screen.
                self.workspaces_view.screen = Screen::Recent;
                self.leave_mode(Mode::Workspaces);
                self.select_workspace(Some(id));
                self.focus = Focus::Composer;
                // The list is ordered by creation, so the service's order wins.
                self.load_workspaces();
            }
            ActionOutcome::WorkspaceDeleted { workspace } => {
                self.status.clear_error();
                self.workspaces.retain(|held| held.id != workspace);
                if self.view.active_workspace_id == Some(workspace) {
                    let next = self.workspaces.first().map(|held| held.id);
                    self.select_workspace(next);
                }
                if self.workspaces_view.selected >= self.workspace_rows().len().max(1) {
                    self.workspaces_view.selected = 0;
                }
                self.reconcile_manage();
                self.load_workspaces();
            }
            ActionOutcome::SessionStarted {
                session,
                outcome,
                error,
            } => {
                let session = *session;
                self.status.clear_error();
                // The row exists now, so it goes in the list and on screen whether
                // or not the send was taken: a failed send is retried from the
                // session it was made in.
                self.sessions.retain(|held| held.id != session.id);
                self.sessions.insert(0, session.clone());
                let rows = self.session_ids();
                self.sessions_view.set_selected(Some(session.id), &rows);
                let id = session.id;
                self.select_session(Some(id));
                self.focus = Focus::Composer;
                self.composer.set_sending(false);
                match outcome {
                    Some(SubmitOutcome::Accepted) => {
                        // The landing page's own key: the row that was just
                        // created is already on screen, so `switch_to` above
                        // parked this message under `None`, not under `id`.
                        self.composer.take_sent(None);
                        self.activity.start(Phase::Thinking);
                        self.running.insert(id);
                        self.notice = None;
                    }
                    Some(SubmitOutcome::AwaitingApproval) => {
                        self.activity.idle();
                        self.notice = Some(Notice::info(
                            "nothing was sent \u{2014} the turn is waiting for approval",
                        ));
                    }
                    None => {
                        self.activity.idle();
                        // The send was refused, so the text goes back into the
                        // field: it is parked under the landing page's draft key,
                        // and the session's own draft is empty. Swapping brings
                        // the typed message back, ready for one more `Enter`.
                        self.composer.switch_to(Some(id), None);
                        if let Some(message) = error {
                            self.status.set_error(message);
                        }
                    }
                }
            }
            ActionOutcome::SessionRenamed(session) => {
                let session = *session;
                self.pending_rename = None;
                self.status.clear_error();
                if let Some(slot) = self.sessions.iter_mut().find(|held| held.id == session.id) {
                    *slot = session;
                }
                self.notice = None;
            }
            ActionOutcome::SessionDeleted { session } => {
                self.status.clear_error();
                self.sessions.retain(|held| held.id != session);
                if self.view.open_session_id == Some(session) {
                    let next = self.sessions.first().map(|held| held.id);
                    self.select_session(next);
                }
                self.reconcile_sessions_view();
            }
            ActionOutcome::Mcp {
                workspace,
                generation,
                servers,
            } => {
                if Some(workspace) != self.view.active_workspace_id {
                    return;
                }
                if generation != self.mcp_generation {
                    return;
                }
                self.status.set_servers(workspace, servers);
            }
            ActionOutcome::McpFocused { workspace } => {
                if Some(workspace) == self.view.active_workspace_id {
                    self.focused_mcp = Some(workspace);
                }
            }
            ActionOutcome::Probed { error } => self.status.probed(error),
            ActionOutcome::Paths {
                generation,
                text,
                candidates,
            } => {
                if generation != self.paths_generation {
                    return;
                }
                self.workspaces_view.set_candidates(&text, candidates);
            }
            ActionOutcome::CreateWorkspaceFailed { message } => {
                // The typed path stays: the failure is usually a typo in it.
                self.status.set_error(message);
            }
            ActionOutcome::RenameFailed { message } => {
                if let Some((session, title)) = self.pending_rename.take() {
                    // Reopen the field with what was typed, so the title can be
                    // fixed rather than retyped.
                    self.open_rename_prompt(session, &title);
                }
                self.status.set_error(message);
            }
            ActionOutcome::DeleteFailed { message } => {
                self.status.set_error(message.clone());
                self.notice = Some(Notice::error(message));
            }
            ActionOutcome::SubmitFailed { session, message } => {
                if Some(session) == self.view.open_session_id {
                    self.composer.set_sending(false);
                    self.activity.idle();
                }
                // Both surfaces: the footer answers the keystroke that failed, and
                // the strip keeps it visible after the next keystroke clears the
                // footer. A refusal for a held session is the case that matters —
                // the draft survives and the reader needs to know why it did not
                // send.
                self.status.set_error(message.clone());
                self.notice = Some(Notice::error(message));
            }
            ActionOutcome::Attached {
                session,
                source,
                pending,
            } => {
                // A read issued for one chat must not put a chip under another.
                if session != self.view.open_session_id {
                    return;
                }
                // The two caps that depend on what is already attached. The
                // per-file one is the effect's, because it needs the bytes.
                if self.composer.attachment_count() >= MAX_ATTACHMENTS {
                    self.notice = Some(Notice::error(format!(
                        "a message carries at most {MAX_ATTACHMENTS} attachments"
                    )));
                    return;
                }
                let total = self.composer.text_total() + pending.text_len();
                if total > MAX_TEXT_TOTAL_BYTES {
                    self.notice = Some(Notice::error(format!(
                        "the attached files total more than {MAX_TEXT_TOTAL_BYTES} bytes"
                    )));
                    return;
                }
                let label = pending.name().to_owned();
                self.composer.attach(Attachment { source, pending });
                self.status.clear_error();
                self.notice = Some(Notice::info(format!("{label} attached")));
            }
            ActionOutcome::AttachFailed { message } => {
                // An answer to the command the reader just ran, so it goes in the
                // footer rather than on the strip.
                self.notice = Some(Notice::error(message));
            }
            ActionOutcome::Opened { path } => {
                self.status.clear_error();
                self.notice = Some(Notice::info(format!("opened {path}")));
            }
            ActionOutcome::OpenFailed { message } => {
                self.notice = Some(Notice::error(message));
            }
            ActionOutcome::Saved => {}
            ActionOutcome::Settings { generation, values } => {
                // A read issued before a write landed would show the old value
                // with the new one's note beside it.
                if generation != self.settings_generation {
                    return;
                }
                self.settings.set_values(values);
                self.status.clear_error();
                self.reconcile_settings();
            }
            ActionOutcome::SettingSaved { key, label } => {
                let _ = key;
                self.status.clear_error();
                self.settings.saved(label);
                // The facts row, the picker, and the surface all read the store,
                // so a write is followed by a read rather than by a local patch:
                // one copy, and a default that is stale for its provider shows as
                // stale exactly as `Ctrl-M`'s row does.
                self.load_models();
                self.request_settings();
            }
            ActionOutcome::SettingFailed { key, message } => {
                let _ = key;
                self.settings.failed(message);
                // The row goes back to what is stored. The note says why, and
                // the surface's own line is the only place it can: the strip is
                // behind this view.
                self.request_settings();
            }
            ActionOutcome::McpConfig(paths) => {
                self.mcp_config = Some(paths);
                self.status.clear_error();
            }
            ActionOutcome::Failed {
                generation,
                message,
            } => {
                if generation.is_some_and(|stale| stale != self.transcript.generation()) {
                    return;
                }
                self.status.set_error(message);
            }
        }
    }

    /// The workspace to open at startup.
    ///
    /// The path the process was started on wins over the remembered workspace:
    /// `robi ~/code/pocket` means that directory or a prompt, never some
    /// other workspace an earlier run happened to leave open. The two are
    /// compared canonically, because one directory has many spellings; the store
    /// keeps the canonical root.
    fn pick_startup_workspace(&self) -> Option<WorkspaceId> {
        let target = canonical(&self.target_workspace);
        self.workspaces
            .iter()
            .find(|workspace| canonical(std::path::Path::new(&workspace.root)) == target)
            .map(|workspace| workspace.id)
    }

    /// Accept the missing-workspace prompt: open the typed path as a workspace.
    fn create_target_workspace(&mut self) {
        let root = self.target_workspace.to_string_lossy().into_owned();
        self.actions.push(Action::CreateWorkspace { root });
    }

    /// Decline it: open the switcher **over the question**, not instead of it.
    ///
    /// The question stays on the stack, so closing the overlay without choosing
    /// anything returns to it. Leaving it first put the reader back in a chat
    /// frame with no workspace and no way to answer — the state this replaces.
    fn choose_another_workspace(&mut self) {
        self.open_workspaces();
    }

    /// Make `id` the active workspace, saving on a change.
    ///
    /// The session listing is issued on every load, including the first,
    /// because the remembered workspace is already active at startup and a
    /// "no change" check would never list its sessions.
    fn select_workspace(&mut self, id: Option<WorkspaceId>) {
        let changed = self.view.active_workspace_id != id;
        if changed {
            let previous_session = self.view.open_session_id;
            self.view.active_workspace_id = id;
            // Everything below belonged to the other workspace: its list, its
            // marks, its session, and the transcript that session had.
            self.sessions.clear();
            self.running.clear();
            self.sessions_view.set_selected(None, &[]);
            self.status.clear_servers();
            self.focused_mcp = None;
            self.view.open_session_id = None;
            self.transcript.clear();
            self.close_approval();
            self.composer.switch_to(previous_session, None);
            self.composer.set_sending(false);
            self.activity.idle();
            // The new workspace's own first listing opens its most recent
            // session, exactly as the process's first listing did.
            self.sessions_loaded = false;
        }
        // A workspace answers the startup question, so the page goes with it —
        // and so does a switcher opened over that page.
        if id.is_some() {
            self.leave_mode(Mode::WorkspaceMissing);
        }
        let promoted = id.is_some_and(|workspace| self.promote_workspace(workspace));
        if changed || promoted {
            self.actions.push(Action::SaveView {
                view: self.snapshot(),
            });
        }
        if let Some(workspace) = id {
            self.load_sessions();
            self.load_mcp();
            // Skills are per workspace: the list for the old one would offer ids
            // this workspace may not have, so it is cleared and re-read.
            self.skills.clear();
            self.completion = None;
            self.load_skills();
            if self.focused_mcp != Some(workspace) {
                self.actions.push(Action::FocusMcp { workspace });
            }
        }
    }

    /// Make `id` the open session, saving on a change and reloading always.
    /// Take `Open` on the session about to be displayed, or note who has it.
    ///
    /// `Open` never refuses a transcript. It decides whether this process may open
    /// the session's blob file, and a refusal means tool results render as a line
    /// naming the other pid while everything in the chat database still shows.
    ///
    /// Refcounted inside the adapter, but re-taking on every render would leak
    /// references, so the session id is remembered.
    fn take_open_lease(&mut self, id: Option<SessionId>) {
        if self.lease_session == id {
            return;
        }
        self.open_lease = None;
        self.held_by = None;
        self.lease_session = id;
        self.transcript.set_result_holder(None);
        let Some(id) = id else {
            return;
        };
        match self.locks.try_acquire(&Resource::Session(id), Scope::Open) {
            Ok(guard) => self.open_lease = Some(guard),
            Err(error) => {
                let pid = error.holder().map(|holder| holder.pid);
                self.held_by = pid;
                self.transcript.set_result_holder(pid);
                if let Some(pid) = pid {
                    self.notice = Some(Notice::info(format!(
                        "tool results are hidden: another robi has this chat open (pid {pid})"
                    )));
                }
            }
        }
    }

    /// Set the degraded-render pid, for a test that has no second process.
    #[cfg(test)]
    pub fn set_result_holder_for_test(&mut self, pid: Option<u32>) {
        self.transcript.set_result_holder(pid);
    }

    /// The pid holding the session on screen, when it is not this one.
    pub fn held_by(&self) -> Option<u32> {
        self.held_by
    }

    /// Put a tool call's lock refusal in the status strip, once.
    ///
    /// The tool payload carries a stable `kind`, so this reads the same field the
    /// model does rather than matching a sentence. `Locked` is not a failure of
    /// the tool: another process held the file for longer than the wait.
    fn note_locked_results(&mut self) {
        let mut announced = None;
        for message in self.transcript.messages() {
            for call in &message.tool_calls {
                let Some(result) = call.result.as_ref() else {
                    continue;
                };
                if result.get("kind").and_then(|kind| kind.as_str()) != Some("locked") {
                    continue;
                }
                if self.locked_announced.contains(&call.id.to_string()) {
                    continue;
                }
                let detail = result
                    .get("error")
                    .and_then(|error| error.as_str())
                    .unwrap_or("another robi is editing that file");
                announced = Some(detail.to_owned());
                self.locked_announced.insert(call.id.to_string());
            }
        }
        if let Some(detail) = announced {
            self.status.set_error(detail);
        }
    }

    fn select_session(&mut self, id: Option<SessionId>) {
        let previous = self.view.open_session_id;
        if previous != id {
            self.view.open_session_id = id;
            // The rows belonged to the previous session.
            self.transcript.clear();
            self.actions.push(Action::SaveView {
                view: self.snapshot(),
            });
            self.take_open_lease(id);
        }
        // The pause belongs to the session that was paused, not to the one on
        // screen now. The new session's own pause arrives with its transcript.
        self.close_approval();
        // The composer keeps one draft per session, so a switch parks the text
        // being typed and brings back the other session's.
        self.composer.switch_to(previous, id);
        self.composer.set_sending(false);
        // A session that is still running keeps its busy phase across the
        // switch: the frames that would have set it were another session's and
        // were dropped, and an idle phase is what makes a call the loop has not
        // started yet read as one waiting on the reader.
        if id.is_some_and(|id| self.running.contains(&id)) {
            self.activity.start(Phase::Thinking);
        } else {
            self.activity.idle();
        }
        self.stopping = false;
        self.transcript.set_phase(self.activity.phase());
        let rows = self.session_ids();
        self.sessions_view.set_selected(id, &rows);
        if let Some(session) = id {
            self.reload(session);
        }
    }

    /// Put `workspace` at the front of the persisted recents.
    ///
    /// Returns whether the order changed, so a selection of the same workspace
    /// does not write the file again. Unbounded, like the GUI's list: the recent
    /// screen takes the first eight and a stale id is skipped when it is read.
    fn promote_workspace(&mut self, workspace: WorkspaceId) -> bool {
        if self.view.recent_workspace_ids.first() == Some(&workspace) {
            return false;
        }
        self.view
            .recent_workspace_ids
            .retain(|held| *held != workspace);
        self.view.recent_workspace_ids.insert(0, workspace);
        true
    }

    /// Issue a workspace list read and record its generation.
    fn load_workspaces(&mut self) {
        self.workspaces_generation += 1;
        self.actions.push(Action::LoadWorkspaces {
            generation: self.workspaces_generation,
        });
    }

    /// Issue a session list read for the active workspace.
    fn load_sessions(&mut self) {
        let Some(workspace) = self.view.active_workspace_id else {
            return;
        };
        self.sessions_generation += 1;
        self.actions.push(Action::LoadSessions {
            workspace,
            generation: self.sessions_generation,
        });
    }

    /// Issue an MCP status read for the active workspace.
    fn load_mcp(&mut self) {
        let Some(workspace) = self.view.active_workspace_id else {
            return;
        };
        self.mcp_generation += 1;
        self.actions.push(Action::LoadMcp {
            workspace,
            generation: self.mcp_generation,
        });
    }

    /// Issue the model list and each mode's inheritable choice.
    fn load_models(&mut self) {
        self.models_generation += 1;
        self.actions.push(Action::LoadModels {
            generation: self.models_generation,
        });
    }

    /// Issue this workspace's skill list. A switch clears the old one first.
    fn load_skills(&mut self) {
        let Some(workspace) = self.view.active_workspace_id else {
            return;
        };
        self.skills_generation += 1;
        self.skills_workspace = Some(workspace);
        self.actions.push(Action::LoadSkills {
            workspace,
            generation: self.skills_generation,
        });
    }

    /// The active workspace's id as a frame's `subject` spells it.
    fn active_workspace_key(&self) -> String {
        self.view
            .active_workspace_id
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

impl App {
    /// A paused call that this process cannot decide, because the actor that
    /// paused it has exited.
    /// Show the modal for every call a loaded transcript is still waiting on.
    ///
    /// This is what makes a pause survive a restart, a session switch, or a reload
    /// that arrived after the `awaiting_approval` frame was dropped. Idle only: a
    /// running turn is not paused whatever a stale transcript says, and after a
    /// decision the phase is `Thinking`, which is what stops an in-flight reload
    /// from re-listing a call that was just settled.
    fn open_approval_for(&mut self, pending: Vec<ToolCallId>) {
        if pending.is_empty() || self.is_busy() {
            return;
        }
        let mut opened = false;
        match self.approval.as_mut() {
            Some(approval) => {
                // Extend what is already on screen rather than replace it: the
                // typed reason belongs to this pause.
                for call in pending.iter().copied() {
                    approval.push(call);
                }
            }
            None => {
                let mut approval = Approval::new();
                for call in pending.iter().copied() {
                    approval.push(call);
                }
                self.approval = Some(approval);
                opened = true;
            }
        }
        // The selection is the announcement: there is no modal to cover the frame,
        // so take the reader to a card that is waiting. Only when the selection is
        // not already on one — a reload must not drag the reader off a card they
        // chose.
        let already_selected = self
            .selected_pending()
            .is_some_and(|call| pending.contains(&call));
        if !already_selected {
            if let Some(first) = pending.first() {
                self.focus_pending(*first);
            }
        }
        if opened {
            self.bell |= !self.focused;
        }
    }
}

/// The `tool_call_id` an `awaiting_approval` frame names.
/// The path a comparison should use: resolved when it exists, unchanged when it
/// does not.
///
/// Two spellings of one directory are one workspace. A path that does not exist
/// yet cannot be canonicalised, and the service refuses it with its own message
/// rather than this helper guessing.
pub fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A pasted block with its line breaks in the one shape a field stores.
///
/// A terminal may send `\r\n` or a bare `\r` for the breaks in a paste. A text
/// area holds `\n`, and a stray carriage return inside a message is a character
/// the reader cannot see and cannot delete on purpose.
fn pasted_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// A pasted block as one line, for a field that holds one.
///
/// A path, a title, a filter: a break in any of them is a mistake, and a field
/// that cannot hold one would otherwise show the first line and hide the rest.
fn one_line(text: &str) -> String {
    pasted_lines(text).replace('\n', " ").trim().to_owned()
}

/// The session a frame's `subject` names.
fn session_id(envelope: &EventEnvelope) -> Option<SessionId> {
    uuid::Uuid::parse_str(&envelope.subject)
        .ok()
        .map(SessionId::from_uuid)
}

fn tool_call_id(envelope: &EventEnvelope) -> Option<ToolCallId> {
    let raw = envelope.data.get("tool_call_id")?.as_str()?;
    uuid::Uuid::parse_str(raw).ok().map(ToolCallId::from_uuid)
}

/// The `message_id` a stored-row frame names.
///
/// Every one of them carries it in `data` (`message_ref` in
/// `domain/events::envelope`). A frame without one is ignored rather than
/// turning into a full reload: a reload per frame is what T1 set out to stop.
fn message_id(envelope: &EventEnvelope) -> Option<MessageId> {
    let raw = envelope.data.get("message_id")?.as_str()?;
    uuid::Uuid::parse_str(raw).ok().map(MessageId::from_uuid)
}

/// The failure a `turn_finished` frame reported, if it failed.
/// Whether a `turn_finished` frame is the loop pausing for a decision.
///
/// A pause emits `awaiting_approval` and then `turn_finished { kind: "paused" }`,
/// because the actor really does exit. The turn is not over, though: the
/// transcript is parked until a call is settled.
fn turn_paused(envelope: &EventEnvelope) -> bool {
    envelope
        .data
        .get("outcome")
        .and_then(|outcome| outcome.get("kind"))
        .and_then(|kind| kind.as_str())
        == Some("paused")
}

fn failed_turn(envelope: &EventEnvelope) -> Option<String> {
    let outcome = envelope.data.get("outcome")?;
    if outcome.get("kind")?.as_str()? != "failed" {
        return None;
    }
    Some(
        outcome
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("the turn failed")
            .to_owned(),
    )
}

/// Whether `message` is the row a stored-row frame named.
///
/// Kept beside `message_id` so a test can assert the pairing without parsing
/// the envelope twice.
pub fn is_same_row(envelope: &EventEnvelope, message: &Message) -> bool {
    message_id(envelope) == Some(message.id)
}

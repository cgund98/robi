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

use ratatui::crossterm::event::{KeyEvent, KeyEventKind};
use ratatui::Frame;

use robi_core::ids::SessionId;

use crate::domain::chat_session::model::ChatSession;
use crate::domain::events::{
    EventEnvelope, APP_ERROR, MESSAGE_ADDED, MESSAGE_UPDATED, TOOL_CALL_UPDATED,
    TRANSCRIPT_COMPACTED, TURN_FINISHED,
};
use crate::domain::workspace::model::Workspace;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::keys::{self, Command};
use crate::tui::lifecycle;
use crate::tui::persist::ViewState;
use crate::tui::state::transcript::Transcript;
use crate::tui::state::{Mode, Notice};
use crate::tui::view;

/// What the pump hands to `update`.
#[derive(Debug)]
pub enum Msg {
    /// A keystroke. Releases are filtered in `input`, and ignored here too.
    Key(KeyEvent),
    /// One frame from the bus.
    Agent(EventEnvelope),
    /// 250 ms: spinners, elapsed time, and the catch-up reload from T1.
    Tick,
    /// An effect finished.
    Done(ActionOutcome),
    /// The terminal changed size.
    Resize { width: u16, height: u16 },
    /// `SIGTERM`.
    Terminate,
}

/// The whole view state.
pub struct App {
    view: ViewState,
    /// The database this session opened, shown when there is nothing to show.
    database_url: String,
    workspaces: Vec<Workspace>,
    sessions: Vec<ChatSession>,
    transcript: Transcript,
    notice: Option<Notice>,
    mode: Mode,
    height: u16,
    quit: bool,
    actions: Vec<Action>,
}

impl App {
    pub fn new(view: ViewState, database_url: impl Into<String>) -> Self {
        Self {
            view,
            database_url: database_url.into(),
            workspaces: Vec::new(),
            sessions: Vec::new(),
            transcript: Transcript::new(),
            notice: None,
            mode: Mode::Chat,
            height: 0,
            quit: false,
            actions: Vec::new(),
        }
    }

    /// The first action: list workspaces, so the active one can be picked.
    ///
    /// Everything after this chains through [`Msg::Done`].
    pub fn boot(&mut self) -> Vec<Action> {
        self.actions.push(Action::LoadWorkspaces);
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
    pub fn snapshot(&self) -> ViewState {
        self.view.clone()
    }

    pub fn mode(&self) -> Mode {
        self.mode
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

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn active_workspace_id(&self) -> Option<robi_core::ids::WorkspaceId> {
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
    pub fn session_title(&self) -> &str {
        match self.open_session() {
            Some(session) => session.title.as_deref().unwrap_or("Untitled session"),
            None => "no session",
        }
    }

    /// How many rows the transcript area has.
    pub fn body_height(&self) -> usize {
        usize::from(self.height.saturating_sub(2)).max(1)
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
        self.height = height;
        // The transcript wraps to the content width, not the terminal's: the
        // frame keeps a gutter, and a band must end where the header does.
        self.transcript.set_width(view::content_width(width));
        self.transcript.set_viewport(self.body_height());
    }

    pub fn view(&self, frame: &mut Frame) {
        view::draw(frame, self);
    }

    pub fn update(&mut self, message: Msg) {
        match message {
            Msg::Key(key) => self.on_key(key),
            Msg::Agent(envelope) => self.on_agent(envelope),
            Msg::Tick => {}
            Msg::Done(outcome) => self.on_done(outcome),
            Msg::Resize { width, height } => self.resize(width, height),
            Msg::Terminate => self.quit = true,
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match keys::command(self.mode, &key) {
            Some(Command::Quit) => self.quit = true,
            Some(Command::PageUp) => self.transcript.page_up(),
            Some(Command::PageDown) => self.transcript.page_down(),
            Some(Command::HalfPageUp) => self.transcript.half_page_up(),
            Some(Command::HalfPageDown) => self.transcript.half_page_down(),
            Some(Command::Top) => self.transcript.top(),
            Some(Command::Bottom) => self.transcript.bottom(),
            None => {}
        }
    }

    /// The thin T0 event handling: a stored row means refetch, and a delta
    /// means nothing yet.
    ///
    /// The bus drops a queued `message_delta` when a subscriber falls behind
    /// (`make_room` in `domain/events`), so a transcript rendered from deltas
    /// would silently lose text. Deltas move the phase from T1; they never
    /// write text.
    fn on_agent(&mut self, envelope: EventEnvelope) {
        if envelope.event_type == APP_ERROR {
            let text = envelope
                .data
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("an error was reported")
                .to_owned();
            self.notice = Some(Notice::error(text));
            return;
        }

        let Some(open) = self.view.open_session_id else {
            return;
        };
        if envelope.subject != open.to_string() {
            return;
        }
        match envelope.event_type.as_str() {
            MESSAGE_ADDED | MESSAGE_UPDATED | TOOL_CALL_UPDATED | TURN_FINISHED
            | TRANSCRIPT_COMPACTED => self.reload(open),
            _ => {}
        }
    }

    /// Issue a reload and record the generation it belongs to.
    fn reload(&mut self, session: SessionId) {
        let generation = self.transcript.next_generation();
        self.actions.push(Action::Reload {
            session,
            generation,
        });
    }

    fn on_done(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Workspaces(workspaces) => {
                self.workspaces = workspaces;
                let picked =
                    lifecycle::pick_workspace(&self.workspaces, self.view.active_workspace_id);
                self.select_workspace(picked);
            }
            ActionOutcome::Sessions {
                workspace,
                sessions,
            } => {
                if Some(workspace) != self.view.active_workspace_id {
                    // A listing for a workspace that is no longer active.
                    return;
                }
                self.sessions = sessions;
                let picked =
                    lifecycle::pick_session(&self.sessions, workspace, self.view.open_session_id);
                self.select_session(picked);
                self.notice = None;
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
                self.transcript.set_messages(messages);
                self.notice = None;
            }
            ActionOutcome::Saved => {}
            ActionOutcome::Failed {
                generation,
                message,
            } => {
                if generation.is_some_and(|stale| stale != self.transcript.generation()) {
                    return;
                }
                self.notice = Some(Notice::error(message));
            }
        }
    }

    /// Make `id` the active workspace, saving only on a change.
    ///
    /// The session listing is issued on every load, including the first,
    /// because the remembered workspace is already active at startup and a
    /// "no change" check would never list its sessions.
    fn select_workspace(&mut self, id: Option<robi_core::ids::WorkspaceId>) {
        if self.view.active_workspace_id != id {
            self.view.active_workspace_id = id;
            self.sessions.clear();
            self.actions.push(Action::SaveView {
                view: self.snapshot(),
            });
        }
        if let Some(workspace) = id {
            self.actions.push(Action::LoadSessions { workspace });
        }
    }

    /// Make `id` the open session, saving on a change and reloading always.
    fn select_session(&mut self, id: Option<SessionId>) {
        if self.view.open_session_id != id {
            self.view.open_session_id = id;
            // The rows belonged to the previous session.
            self.transcript.clear();
            self.actions.push(Action::SaveView {
                view: self.snapshot(),
            });
        }
        if let Some(session) = id {
            self.reload(session);
        }
    }
}

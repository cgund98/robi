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

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;

use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::Message;

use crate::domain::chat_message::runtime::SubmitOutcome;
use crate::domain::chat_session::model::ChatSession;
use crate::domain::events::{
    EventEnvelope, APP_ERROR, AWAITING_APPROVAL, MESSAGE_ADDED, MESSAGE_DELTA, MESSAGE_UPDATED,
    TOOL_CALL_UPDATED, TRANSCRIPT_COMPACTED, TURN_FINISHED, TURN_STARTED,
};
use crate::domain::workspace::model::Workspace;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::keys::{self, Command};
use crate::tui::lifecycle;
use crate::tui::persist::ViewState;
use crate::tui::state::activity::{Activity, DeltaKind};
use crate::tui::state::composer::Composer;
use crate::tui::state::transcript::Transcript;
use crate::tui::state::{Focus, Mode, Notice, Phase};
use crate::tui::view;

/// What the pump hands to `update`.
#[derive(Debug)]
pub enum Msg {
    /// A keystroke. Releases are filtered in `input`, and ignored here too.
    Key(KeyEvent),
    /// One frame from the bus.
    Agent(EventEnvelope),
    /// 250 ms: elapsed time, the dots, and the catch-up reload.
    Tick,
    /// An effect finished.
    Done(ActionOutcome),
    /// The mouse wheel turned, in rows: negative is up. It goes through the
    /// same scroll path as `PageUp`/`PageDown`, so the follow lock behaves
    /// identically however the reader scrolled.
    Scroll(isize),
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
    activity: Activity,
    composer: Composer,
    focus: Focus,
    notice: Option<Notice>,
    mode: Mode,
    height: u16,
    /// A stop was issued and has not come back yet.
    stopping: bool,
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
            activity: Activity::default(),
            composer: Composer::new(),
            focus: Focus::default(),
            notice: None,
            mode: Mode::Chat,
            height: 0,
            stopping: false,
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

    pub fn focus(&self) -> Focus {
        self.focus
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
    pub fn session_title(&self) -> &str {
        match self.open_session() {
            Some(session) => session.title.as_deref().unwrap_or("Untitled session"),
            None => "no session",
        }
    }

    /// How many rows the transcript area has: the frame minus the header, the
    /// gap above the composer, the composer band, and the footer.
    pub fn body_height(&self) -> usize {
        let used = 1u16
            .saturating_add(view::COMPOSER_GAP)
            .saturating_add(self.composer.height())
            .saturating_add(1);
        usize::from(self.height.saturating_sub(used)).max(1)
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
            Msg::Agent(envelope) => self.on_agent(envelope),
            Msg::Tick => self.on_tick(),
            Msg::Done(outcome) => self.on_done(outcome),
            Msg::Scroll(rows) => self.transcript.scroll_by(rows),
            Msg::Resize { width, height } => self.resize(width, height),
            Msg::Terminate => self.quit = true,
        }
        self.sync_viewport();
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match keys::command(self.mode, self.focus, &key) {
            Some(Command::Quit) => self.quit_or_refuse(),
            Some(Command::StopOrQuit) => self.stop_or_quit(),
            Some(Command::Submit) => self.submit(),
            Some(Command::Newline) => self.composer.insert_newline(),
            Some(Command::FocusNext) | Some(Command::FocusPrev) => {
                self.focus = self.focus.other();
            }
            Some(Command::PageUp) => self.transcript.page_up(),
            Some(Command::PageDown) => self.transcript.page_down(),
            Some(Command::HalfPageUp) => self.transcript.half_page_up(),
            Some(Command::HalfPageDown) => self.transcript.half_page_down(),
            Some(Command::LineUp) | Some(Command::LineDown) => self.arrow(key),
            Some(Command::Top) => self.transcript.top(),
            Some(Command::Bottom) => self.transcript.bottom(),
            None => {
                // Anything unclaimed belongs to the field, which is the only
                // thing on screen that takes text.
                if self.focus.is_composer() && !self.composer.sending() {
                    self.composer.input(&key);
                }
            }
        }
    }

    /// An `Up`/`Down` key: scroll one row, or move the caret.
    ///
    /// The map gives both focuses the scroll command, and this is the one place
    /// that decides. A composer holding text needs its arrows — a multi-line
    /// draft is navigated with them, and taking the caret away to scroll is the
    /// trade the earlier design made. An empty field has no caret to move, so
    /// there the arrows scroll the transcript, which is what a reader reaches
    /// for first.
    fn arrow(&mut self, key: KeyEvent) {
        let field_wants_it = self.focus.is_composer() && !self.composer.is_blank();
        if field_wants_it {
            if !self.composer.sending() {
                self.composer.input(&key);
            }
            return;
        }
        let rows = if key.code == KeyCode::Up { -1 } else { 1 };
        self.transcript.scroll_by(rows);
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
    /// `Ctrl-C` is the documented interrupt. So this refuses and says so.
    fn submit(&mut self) {
        if self.composer.is_blank() {
            self.notice = Some(Notice::info("nothing to send"));
            return;
        }
        let Some(session) = self.view.open_session_id else {
            self.notice = Some(Notice::error("no session is open"));
            return;
        };
        // A send is in flight, or a turn is running. Both refuse, and both name
        // the interrupt, so the answer is not "nothing happened".
        if self.composer.sending() || self.activity.is_busy() {
            self.notice = Some(Notice::info("a turn is running \u{2014} Ctrl-C stops it"));
            return;
        }

        let text = self.composer.text();
        self.composer.set_sending(true);
        self.activity.start(Phase::Thinking);
        self.transcript.set_phase(Phase::Thinking);
        self.notice = None;
        self.actions.push(Action::Submit {
            session,
            text,
            files: Vec::new(),
        });
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
                "a turn is running \u{2014} Ctrl-C stops it, then q leaves",
            ));
            return;
        }
        self.quit = true;
    }

    /// `Ctrl-C`: stop the turn, or leave when nothing is running.
    fn stop_or_quit(&mut self) {
        if !self.activity.is_busy() && !self.composer.sending() {
            self.quit = true;
            return;
        }
        let Some(session) = self.view.open_session_id else {
            self.quit = true;
            return;
        };
        if self.stopping {
            return;
        }
        self.stopping = true;
        self.notice = Some(Notice::info("stopping\u{2026}"));
        self.actions.push(Action::Stop { session });
    }

    /// One 250 ms tick: advance the elapsed time, and reload if the frames have
    /// gone quiet.
    ///
    /// The catch-up exists because the bus drops frames under backpressure. A
    /// row can be stored while the event that would have painted it was thrown
    /// away, so a turn that stops hearing anything assumes the worst and
    /// refetches.
    fn on_tick(&mut self) {
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
            // Another session's turn. T3 owns the list.
            return;
        }

        match envelope.event_type.as_str() {
            TURN_STARTED => {
                self.activity.start(Phase::Thinking);
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
            AWAITING_APPROVAL => {
                // T2 turns this into the modal. Until then the turn is paused
                // with no way to answer it from here, so say so rather than
                // sitting on a spinner that will not move.
                self.activity.idle();
                self.composer.set_sending(false);
                self.notice = Some(Notice::info(
                    "the turn is waiting for approval \u{2014} answer it in the desktop app",
                ));
            }
            TURN_FINISHED => {
                self.activity.idle();
                self.composer.set_sending(false);
                self.stopping = false;
                if let Some(message) = failed_turn(&envelope) {
                    self.notice = Some(Notice::error(message));
                }
                self.reload(open);
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
            ActionOutcome::Submitted { session, outcome } => {
                if Some(session) != self.view.open_session_id {
                    return;
                }
                self.composer.set_sending(false);
                match outcome {
                    SubmitOutcome::Accepted => {
                        // Only a send that was taken clears the buffer; a
                        // refused one has to leave it exactly as it was.
                        self.composer.take_sent();
                        self.activity.start(Phase::Thinking);
                        self.notice = None;
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
                self.notice = Some(Notice::info("stopped"));
                // A cancelled turn still has to render as it ended, even when
                // the `turn_finished` frame was dropped.
                self.reload(session);
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
            ActionOutcome::SubmitFailed { session, message } => {
                if Some(session) == self.view.open_session_id {
                    self.composer.set_sending(false);
                    self.activity.idle();
                }
                self.notice = Some(Notice::error(message));
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
    fn select_workspace(&mut self, id: Option<WorkspaceId>) {
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
        let previous = self.view.open_session_id;
        if previous != id {
            self.view.open_session_id = id;
            // The rows belonged to the previous session.
            self.transcript.clear();
            self.actions.push(Action::SaveView {
                view: self.snapshot(),
            });
        }
        // The composer keeps one draft per session, so a switch parks the text
        // being typed and brings back the other session's.
        self.composer.switch_to(previous, id);
        self.composer.set_sending(false);
        self.activity.idle();
        self.stopping = false;
        self.transcript.set_phase(Phase::Idle);
        if let Some(session) = id {
            self.reload(session);
        }
    }
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

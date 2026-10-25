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

use ratatui::crossterm::event::{KeyEvent, KeyEventKind};
use ratatui::Frame;

use robi_core::ids::{MessageId, SessionId, ToolCallId, WorkspaceId};
use robi_core::message::{Message, ToolCall};

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
use crate::tui::state::approval::{Approval, Decision};
use crate::tui::state::composer::Composer;
use crate::tui::state::detail::Detail;
use crate::tui::state::prompt::{Prompt, PromptKind};
use crate::tui::state::transcript::Transcript;
use crate::tui::state::{Focus, Mode, Notice, Phase};
use crate::tui::view;

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
    height: u16,
    /// A stop was issued and has not come back yet.
    stopping: bool,
    quit: bool,
    actions: Vec<Action>,
}

impl App {
    pub fn new(view: ViewState, database_url: impl Into<String>) -> Self {
        let mut transcript = Transcript::new();
        // The persisted expansion set is the transcript's, and the transcript's is
        // the persisted one: one set, mirrored on every change.
        transcript.set_expanded(view.expanded.clone());
        Self {
            view,
            database_url: database_url.into(),
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
            Msg::Focus(focused) => self.focused = focused,
            Msg::Terminate => self.quit = true,
        }
        self.sync_viewport();
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        match keys::command(self.mode(), self.focus, &key) {
            Some(Command::Quit) => self.quit_or_refuse(),
            Some(Command::StopOrQuit) => self.stop_or_quit(),
            Some(Command::Interrupt) => self.interrupt(),
            Some(Command::Submit) => self.submit(),
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
            Some(Command::RejectWithReason) => self.reject_with_reason(),
            Some(Command::SelectNext) => self.select(1),
            Some(Command::SelectPrev) => self.select(-1),
            Some(Command::OpenDetail) => self.open_detail(),
            Some(Command::CloseDetail) => self.close_detail(),
            Some(Command::ClosePrompt) => self.close_reason(),
            None => self.unclaimed(key),
        }
    }

    /// A scroll key, routed to whichever surface is on top.
    ///
    /// The detail view has its own offset and no follow lock; the transcript has
    /// both. One command set, so the keys are the same in both.
    fn scroll(&mut self, direction: isize, amount: Scroll) {
        let viewport = self.detail_height();
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
            Mode::Chat if self.focus.is_composer() && !self.composer.sending() => {
                self.composer.input(&key);
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
        if self.approval.is_some() {
            self.decline_all();
            return;
        }
        if !self.is_busy() {
            self.quit = true;
            return;
        }
        self.stop_turn();
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
            self.notice = Some(Notice::error(
                "a tool call is waiting for approval but the request could not be read",
            ));
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
        self.detail = Some(Detail::new(&call, original.as_deref()));
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
            // Another session's turn. T3 owns the list, but a pause it cannot
            // answer is the one frame worth reporting from here: the turn is
            // stopped until someone decides it.
            if envelope.event_type == AWAITING_APPROVAL {
                self.bell |= !self.focused;
                self.notice = Some(Notice::info("another session is waiting for approval"));
            }
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
            AWAITING_APPROVAL => self.on_awaiting_approval(&envelope),
            TURN_FINISHED => {
                self.activity.idle();
                self.composer.set_sending(false);
                self.stopping = false;
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
                        self.notice = Some(Notice::error(message));
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
                // A paused turn is answerable from a transcript that was just
                // loaded, not only from a frame that arrived live: reopening a
                // session paused by an earlier run has to reach the modal the
                // same way, or the pause is on screen with no way to answer it.
                let pending = pending_calls(&messages);
                self.transcript.set_messages(messages);
                self.notice = None;
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
                    self.notice = Some(Notice::error(message));
                }
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
        // The modal belongs to the session that was paused, not to the one on
        // screen now. The new session's own pause arrives with its transcript.
        self.close_approval();
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

/// Every call a transcript is waiting on, in row order.
///
/// The same predicate the loop uses to decide a turn is paused: pending approval
/// and not yet started. A call that ran without asking is `pending` with
/// `succeeded` execution, which is a result row and not a prompt.
fn pending_calls(messages: &[Message]) -> Vec<ToolCallId> {
    messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .filter(|call| {
            call.approval_status == robi_core::message::ApprovalStatus::Pending
                && call.execution_status == robi_core::message::ExecutionStatus::NotStarted
        })
        .map(|call| call.id)
        .collect()
}

/// The `tool_call_id` an `awaiting_approval` frame names.
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

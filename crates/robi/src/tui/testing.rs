//! The `TestBackend` harness, and the view snapshots.
//!
//! A snapshot here asserts on rendered strings, not on a golden file: a token
//! change that does not move the layout must not fail a test.

use chrono::Utc;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

use robi_core::error::TurnOutcome;
use robi_core::event::Event;
use robi_core::ids::{MessageId, SessionId, ToolCallId, WorkspaceId};
use robi_core::message::{ApprovalStatus, ExecutionStatus, Message, ToolCall};

use crate::domain::chat_message::runtime::SubmitOutcome;
use crate::domain::chat_session::model::{
    AgentMode, ChatSession, ModelConfig, PathRules, TurnDisplay,
};
use crate::domain::events::{
    EventEnvelope, MESSAGE_ADDED, MESSAGE_DELTA, SOURCE, TRANSCRIPT_COMPACTED, TURN_FINISHED,
};
use crate::domain::workspace::model::Workspace;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::app::{App, Msg};
use crate::tui::persist::ViewState;
use crate::tui::state::approval::Decision;
use crate::tui::state::{Focus, Mode, Phase};

/// Render one frame and return each row as a trimmed string.
pub fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal.draw(|frame| app.view(frame)).expect("a frame");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The whole frame as one string, for a substring assertion.
pub fn screen(app: &App, width: u16, height: u16) -> String {
    rows(app, width, height).join("\n")
}

/// The background colour of one cell, so a test can check a fill and not just a
/// glyph.
pub fn background(app: &App, width: u16, height: u16, x: u16, y: u16) -> ratatui::style::Color {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal.draw(|frame| app.view(frame)).expect("a frame");
    terminal.backend().buffer()[(x, y)].bg
}

/// The foreground colour of one cell, so a test can check a style and not just
/// a glyph.
pub fn foreground(app: &App, width: u16, height: u16, x: u16, y: u16) -> ratatui::style::Color {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("a test terminal");
    terminal.draw(|frame| app.view(frame)).expect("a frame");
    terminal.backend().buffer()[(x, y)].fg
}

/// The one workspace fixture. The `lifecycle` tests reuse it.
pub fn workspace(name: &str) -> Workspace {
    Workspace {
        id: WorkspaceId::new(),
        name: name.to_owned(),
        root: format!("/workspace/{name}"),
        mcp_project_sha256: None,
        created_at: Utc::now(),
    }
}

/// The one session fixture. The `lifecycle` tests reuse it, so a change to
/// `ChatSession` is one edit for the whole module.
pub fn session(workspace: WorkspaceId, title: Option<&str>) -> ChatSession {
    ChatSession {
        id: SessionId::new(),
        workspace_id: workspace,
        title: title.map(str::to_owned),
        path_rules: PathRules::default(),
        allow_hosts: Vec::new(),
        mcp_allows: Vec::new(),
        mode: AgentMode::Agent,
        model_config: ModelConfig::default(),
        turn_display: TurnDisplay::Idle,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_used_at: Utc::now(),
    }
}

/// Feed the transcript in as the effect would, at the generation the app
/// issued.
fn land_transcript(app: &mut App, session: SessionId, messages: Vec<Message>) {
    // The newest reload, not the oldest: a pause issues one before the test
    // lands its transcript, and the older generation would be dropped.
    let generation = app
        .take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Reload { generation, .. } => Some(generation),
            _ => None,
        })
        .next_back()
        .expect("the app asked for a reload");
    app.update(Msg::Done(ActionOutcome::Transcript {
        session,
        generation,
        messages,
    }));
}

/// An app with one workspace, one session, and no messages yet.
pub fn booted(width: u16, height: u16) -> (App, WorkspaceId, SessionId) {
    let mut app = App::new(ViewState::default(), "sqlite://test.db");
    app.resize(width, height);
    let workspace = workspace("robi");
    let workspace_id = workspace.id;
    let session = session(workspace_id, Some("a session"));
    let session_id = session.id;

    app.update(Msg::Done(ActionOutcome::Workspaces(vec![workspace])));
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace: workspace_id,
        sessions: vec![session],
    }));
    // The actions are left pending: the first one is the reload whose
    // generation the transcript lands at.
    (app, workspace_id, session_id)
}

/// The same, with a transcript that has landed.
pub fn with_messages(width: u16, height: u16, messages: Vec<Message>) -> App {
    let (mut app, _, session_id) = booted(width, height);
    land_transcript(&mut app, session_id, messages);
    app
}

/// Land a transcript at the generation the app just asked for.
pub fn land(app: &mut App, session: SessionId, messages: Vec<Message>) {
    land_transcript(app, session, messages);
}

/// A call that is waiting on a decision, which is how a pause arrives.
pub fn pending_call(name: &str, args: serde_json::Value) -> ToolCall {
    let mut call = ToolCall::new(name, args);
    call.approval_status = ApprovalStatus::Pending;
    call.execution_status = ExecutionStatus::NotStarted;
    call
}

/// The frame the loop emits when a turn pauses on one call.
///
/// Built from the real core event, so a payload key that moves fails here rather
/// than silently leaving the modal unopened.
pub fn approval_frame(session: SessionId, call: ToolCallId) -> EventEnvelope {
    EventEnvelope::from_core_event(Event::AwaitingApproval { session, call })
}

/// The frame the loop emits after a pause, when the actor exits.
///
/// A turn that pauses really does emit this, with outcome `paused`; it is not the
/// end of the turn.
pub fn paused_frame(session: SessionId) -> EventEnvelope {
    EventEnvelope::from_core_event(Event::TurnFinished {
        session,
        outcome: TurnOutcome::Paused,
    })
}

/// One bus frame for a session, with no payload beyond the id.
fn envelope(session: SessionId, event_type: &str) -> EventEnvelope {
    EventEnvelope::from_payload(
        SOURCE,
        event_type,
        session.to_string(),
        serde_json::json!({ "session_id": session.to_string() }),
    )
    .expect("an envelope")
}

/// The frame a stored row raises: it names the row, and the view refetches it.
fn stored_row(session: SessionId, message: MessageId) -> EventEnvelope {
    EventEnvelope::from_payload(
        SOURCE,
        MESSAGE_ADDED,
        session.to_string(),
        serde_json::json!({
            "session_id": session.to_string(),
            "message_id": message.to_string(),
        }),
    )
    .expect("an envelope")
}

/// A frame that rewrites the transcript, so the view refetches all of it.
fn compacted(session: SessionId) -> EventEnvelope {
    envelope(session, TRANSCRIPT_COMPACTED)
}

/// A `message_delta` frame of one kind.
fn delta(session: SessionId, kind: &str) -> EventEnvelope {
    EventEnvelope::from_payload(
        SOURCE,
        MESSAGE_DELTA,
        session.to_string(),
        serde_json::json!({
            "session_id": session.to_string(),
            "delta": { "kind": kind, "text": "ignored, never rendered" },
        }),
    )
    .expect("an envelope")
}

/// A `turn_finished` frame.
fn finished(session: SessionId) -> EventEnvelope {
    EventEnvelope::from_payload(
        SOURCE,
        TURN_FINISHED,
        session.to_string(),
        serde_json::json!({
            "session_id": session.to_string(),
            "outcome": { "kind": "complete" },
        }),
    )
    .expect("an envelope")
}

fn two_message_transcript() -> Vec<Message> {
    vec![
        Message::user("read the config"),
        Message::assistant("It sets the port to 1431."),
    ]
}

#[test]
fn the_empty_state_names_the_missing_workspace() {
    let mut app = App::new(ViewState::default(), "sqlite://test.db");
    app.resize(80, 24);
    let frame = screen(&app, 80, 24);
    assert!(
        frame.contains("No workspaces in this database"),
        "an empty workspace list says why: {frame}"
    );
}

#[test]
fn a_terminal_below_the_minimum_draws_one_line() {
    let app = with_messages(30, 8, two_message_transcript());
    let frame = screen(&app, 30, 8);
    assert!(frame.contains("terminal too small"), "{frame}");
    assert!(!frame.contains("read the config"), "{frame}");
}

fn generations(app: &mut App) -> Vec<u64> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Reload { generation, .. } => Some(generation),
            _ => None,
        })
        .collect()
}

fn loaded_rows(app: &mut App) -> Vec<(MessageId, u64)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::LoadMessage {
                message,
                generation,
                ..
            } => Some((message, generation)),
            _ => None,
        })
        .collect()
}

/// The one load-bearing invariant of the phase: a delta moves the phase and
/// writes no text, and a stored row refetches exactly that row.
#[test]
fn a_delta_moves_the_phase_and_a_stored_row_refetches_only_itself() {
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();

    app.update(Msg::Agent(delta(session_id, "text")));
    assert!(
        generations(&mut app).is_empty(),
        "the bus drops a queued delta under backpressure, so a transcript built \
         from deltas would lose text"
    );
    assert_eq!(app.transcript().phase(), Phase::Responding);

    let row = MessageId::new();
    app.update(Msg::Agent(stored_row(session_id, row)));
    let loads = loaded_rows(&mut app);
    assert_eq!(loads.len(), 1, "exactly one row fetch");
    assert_eq!(loads[0].0, row, "for the row the frame named");
    assert!(
        generations(&mut app).is_empty(),
        "a stored row is not a reason to reread the whole transcript"
    );
}

#[test]
fn a_turn_finished_frame_reloads_and_goes_idle() {
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();
    app.update(Msg::Agent(delta(session_id, "text")));
    assert!(app.is_busy(), "a text delta starts the turn");

    app.update(Msg::Agent(finished(session_id)));
    assert_eq!(generations(&mut app).len(), 1, "the transcript is reread");
    assert!(!app.is_busy(), "the turn is over");
    assert_eq!(app.transcript().phase(), Phase::Idle);
}

#[test]
fn a_stale_transcript_outcome_is_dropped() {
    let (mut app, _, session_id) = booted(80, 24);
    let stale = generations(&mut app);
    assert_eq!(stale.len(), 1, "the first load issued one reload");

    // A second full read, issued by a frame that rewrote the transcript.
    app.update(Msg::Agent(compacted(session_id)));
    let fresh = generations(&mut app);
    assert_eq!(fresh.len(), 1);
    assert!(fresh[0] > stale[0]);

    // The newer one lands first, then the older one: the older must not win.
    app.update(Msg::Done(ActionOutcome::Transcript {
        session: session_id,
        generation: fresh[0],
        messages: vec![Message::assistant("newer")],
    }));
    app.update(Msg::Done(ActionOutcome::Transcript {
        session: session_id,
        generation: stale[0],
        messages: vec![Message::assistant("older")],
    }));

    let frame = screen(&app, 80, 24);
    assert!(frame.contains("newer"), "{frame}");
    assert!(!frame.contains("older"), "the stale outcome lost: {frame}");
}

#[test]
fn a_tool_call_renders_as_a_card_in_a_frame() {
    use robi_core::message::{ApprovalStatus, ExecutionStatus, ToolCall};
    let mut migrate = ToolCall::new(
        "shell",
        serde_json::json!({
            "command": "cd /Users/davidhill/Documents/Local/pocket && php artisan migrate --no-interaction",
            "description": "Run database migrations"
        }),
    );
    migrate.approval_status = ApprovalStatus::Approved;
    migrate.execution_status = ExecutionStatus::Succeeded;
    migrate.result = Some(serde_json::json!({
        "stdout": "INFO  Running migrations.\n\n  2025_12_11_141134_add_date_of_birth_to_users_table ......... 3.41ms DONE\n"
    }));

    let mut test = ToolCall::new(
        "shell",
        serde_json::json!({
            "command": "cd /Users/davidhill/Documents/Local/pocket && php artisan test tests/Feature/Auth/RegistrationTest.php",
            "description": "Run registration tests"
        }),
    );
    test.approval_status = ApprovalStatus::Approved;
    test.execution_status = ExecutionStatus::Succeeded;
    test.result = Some(serde_json::json!({
        "stdout": "PASS  Tests\\Feature\\Auth\\RegistrationTest\n  \u{2713} registration screen can be rendered      0.12s\n  \u{2713} new users can register                     0.03s\n\n  Tests:    4 passed (10 assertions)\n"
    }));

    let app = with_messages(
        92,
        44,
        vec![
            Message::user("migrate and test"),
            Message::assistant_with_tool_calls("", vec![migrate, test]),
        ],
    );
    let frame = screen(&app, 92, 44);

    assert!(frame.contains("Run database migrations"), "{frame}");
    assert!(frame.contains("Run registration tests"), "{frame}");
    assert!(frame.contains("$ cd /Users/davidhill"), "{frame}");
    assert!(frame.contains("Running migrations."), "{frame}");
    assert!(
        !frame.contains('\u{250c}') && !frame.contains('\u{2510}') && !frame.contains('\u{2514}'),
        "a card is a band, not a drawn border: {frame}"
    );
}

#[test]
fn a_user_message_and_a_tool_call_have_rails_of_different_colours() {
    use robi_core::message::{ApprovalStatus, ExecutionStatus, ToolCall};
    let mut call = ToolCall::new(
        "shell",
        serde_json::json!({ "command": "cat test.txt", "description": "Shell" }),
    );
    call.approval_status = ApprovalStatus::Approved;
    call.execution_status = ExecutionStatus::Succeeded;
    call.result = Some(serde_json::json!({ "stdout": "hello" }));

    let app = with_messages(
        60,
        20,
        vec![
            Message::user("add a new field for signups"),
            Message::assistant_with_tool_calls("", vec![call]),
        ],
    );
    let frame = rows(&app, 60, 20);

    // Find each band by its text, then read the colour of its rail cell.
    let row_of = |needle: &str| {
        frame
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no row holds {needle:?}: {frame:#?}")) as u16
    };
    let user_row = row_of("add a new field");
    let card_row = row_of("cat test.txt");
    // The rail sits one gutter in from the frame's edge.
    let rail = crate::tui::view::GUTTER;
    for (row, what) in [(user_row, "user band"), (card_row, "card")] {
        assert_eq!(
            frame[usize::from(row)].chars().nth(usize::from(rail)),
            Some('\u{2502}'),
            "the {what} has a rail"
        );
    }
    assert_eq!(
        foreground(&app, 60, 20, rail, user_row),
        crate::tui::theme::ACCENT,
        "the user rail is blue"
    );
    assert_eq!(
        foreground(&app, 60, 20, rail, card_row),
        crate::tui::theme::INK_FAINT,
        "the tool card rail is gray"
    );
    // Everything is inset: nothing is drawn in the first or last column.
    for (index, row) in frame.iter().enumerate() {
        assert!(
            !row.starts_with('\u{2502}'),
            "row {index} is not inset by the gutter: {row:?}"
        );
    }

    // Both bands are raised surfaces: the cell just inside the rail carries
    // the band's background, not the canvas's.
    for (y, band) in [
        (user_row, crate::tui::theme::BG_USER),
        (card_row, crate::tui::theme::CODE_BG),
    ] {
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).expect("a test terminal");
        terminal.draw(|frame| app.view(frame)).expect("a frame");
        assert_eq!(
            terminal.backend().buffer()[(rail + 1, y)].bg,
            band,
            "row {y} is not on the band colour"
        );
    }
    assert!(frame.iter().any(|row| row.contains("add a new field")));
}

#[test]
fn everything_on_screen_is_inset_by_the_gutter() {
    let gutter = usize::from(crate::tui::view::GUTTER);
    assert!(gutter > 0, "a zero gutter is not a gutter");

    let app = with_messages(60, 20, two_message_transcript());
    let frame = rows(&app, 60, 20);

    // The header, the transcript, and the footer share the same edges.
    let drawn: Vec<&String> = frame.iter().filter(|row| !row.trim().is_empty()).collect();
    assert!(
        drawn.len() >= 4,
        "a header, two rows, and a footer: {frame:#?}"
    );
    for row in &drawn {
        let indent = row.len() - row.trim_start().len();
        // At least the gutter, never less. A region may pad inside its own
        // band — the composer's field pads one cell, like a card's body sits
        // one cell in from its rail — but nothing may touch the frame's edge.
        assert!(
            indent >= gutter,
            "{row:?} starts at column {indent}, inside the {gutter}-column gutter"
        );
    }
    // Nothing is painted in the frame's last column. Cells, not bytes: `·`
    // and `…` are multi-byte, and a byte count would be a false positive.
    let widest = drawn
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0);
    assert!(
        widest <= 60,
        "widest row is {widest} cells in a 60-column frame: {drawn:#?}"
    );

    // At a normal width the whole footer hint fits inside the gutter.
    let wide = rows(&with_messages(100, 20, two_message_transcript()), 100, 20);
    let footer = wide.last().expect("a footer").clone();
    assert!(
        footer.contains("select"),
        "the hint is not clipped at 100 columns: {footer:?}"
    );
    assert!(
        footer.chars().count() + gutter <= 100,
        "the footer ends inside the gutter: {footer:?}"
    );
}

/// Type `text` into the composer, one character at a time, as the pump would.
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.update(Msg::Key(KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::NONE,
        )));
    }
}

fn submits(app: &mut App) -> Vec<(SessionId, String)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Submit { session, text, .. } => Some((session, text)),
            _ => None,
        })
        .collect()
}

fn stops(app: &mut App) -> Vec<SessionId> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Stop { session } => Some(session),
            _ => None,
        })
        .collect()
}

/// Sending, and the three refusals, in one test: the refusals exist so a
/// keystroke that does nothing is distinguishable from one that was lost.
#[test]
fn a_send_records_one_submit_and_every_refusal_says_why() {
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();

    // 1. Nothing typed.
    app.update(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(submits(&mut app).is_empty(), "a blank send is a no-op");
    assert!(app.notice().is_some(), "and it says so");

    // 2. A real send.
    type_text(&mut app, "hello");
    assert_eq!(app.composer().text(), "hello");
    app.update(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert_eq!(
        submits(&mut app),
        vec![(session_id, "hello".to_owned())],
        "exactly one submit, with what was typed"
    );
    assert!(app.is_busy(), "the field locks while the send is in flight");
    assert_eq!(
        app.composer().text(),
        "hello",
        "the buffer is kept until the send succeeds"
    );

    // 3. The field is locked while the send is in flight, so nothing typed
    // reaches it. That is the composer's own lock, not a refusal: the honest
    // "a turn is running" case is tested below, once the send has landed.
    assert!(app.composer().sending());

    // The actor took it: now the buffer clears.
    app.update(Msg::Done(ActionOutcome::Submitted {
        session: session_id,
        outcome: SubmitOutcome::Accepted,
    }));
    assert!(app.composer().text().is_empty(), "a sent draft is gone");

    // 4. A send while the turn now runs is taken, not refused.
    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::TurnStarted {
            session: session_id,
        },
    )));
    app.take_actions();
    type_text(&mut app, "and again");
    app.update(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert_eq!(
        submits(&mut app),
        vec![(session_id, "and again".to_owned())],
        "the desktop shell interrupts and queues; so does the terminal"
    );
    app.update(Msg::Done(ActionOutcome::Submitted {
        session: session_id,
        outcome: SubmitOutcome::Accepted,
    }));

    // 5. A failed send keeps the text, so it can be retried.
    app.update(Msg::Done(ActionOutcome::Stopped {
        session: session_id,
    }));
    app.take_actions();
    type_text(&mut app, "again");
    app.update(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    app.take_actions();
    app.update(Msg::Done(ActionOutcome::SubmitFailed {
        session: session_id,
        message: "the provider refused".to_owned(),
    }));
    assert_eq!(
        app.composer().text(),
        "again",
        "a failed send restores the buffer"
    );
    assert!(app.notice().is_some_and(|notice| notice.is_error()));
    assert!(!app.is_busy(), "and it unlocks the field");
}

#[test]
fn ctrl_c_stops_a_running_turn_and_quits_when_idle() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    // Idle: Ctrl-C quits.
    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.should_quit(), "an idle Ctrl-C leaves");

    // Busy: Ctrl-C stops the turn and does not leave.
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();
    app.update(Msg::Agent(delta(session_id, "text")));
    assert!(app.is_busy());

    // `q` with the transcript focused refuses rather than dropping the turn.
    app.update(Msg::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    assert!(!app.focus().is_composer(), "Tab reached the transcript");
    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
    )));
    assert!(!app.should_quit(), "a running turn is not dropped by q");
    assert_eq!(app.composer().text(), "", "and nothing was typed");

    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(
        stops(&mut app),
        vec![session_id],
        "one stop, for this session"
    );
    assert!(!app.should_quit(), "the process stays up");

    // The stop lands: idle, and the transcript is reread so a cancelled turn
    // is still resolvable.
    app.update(Msg::Done(ActionOutcome::Stopped {
        session: session_id,
    }));
    assert!(!app.is_busy());
    assert_eq!(generations(&mut app).len(), 1);
}

/// The pump, over a real bus: a turn is published as the four frames the actor
/// emits, and the view ends idle with one row refetch and one full reread.
#[tokio::test]
async fn the_pump_follows_a_turn_from_the_bus() {
    use crate::domain::events::EventBus;

    let bus = EventBus::new();
    let mut subscription = bus.subscribe();
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();

    let row = MessageId::new();
    let frames = [
        EventEnvelope::from_payload(
            SOURCE,
            crate::domain::events::TURN_STARTED,
            session.to_string(),
            serde_json::json!({ "session_id": session.to_string() }),
        )
        .expect("a frame"),
        delta(session, "text"),
        stored_row(session, row),
        finished(session),
    ];
    for frame in frames {
        bus.publish(frame);
    }

    // Drain the subscription into the app, exactly as `tui::pump` does. The
    // four frames are already queued, so each `recv` returns at once; the
    // timeout only stops the test hanging if a frame never arrives.
    for _ in 0..4 {
        let frame = tokio::time::timeout(std::time::Duration::from_millis(50), subscription.recv())
            .await
            .expect("a queued frame")
            .expect("the bus is still open");
        app.update(Msg::Agent(frame));
    }

    assert!(!app.is_busy(), "the turn finished");
    // One drain of the actions, because both facts are in the same queue: two
    // `take_actions` calls would have the first one eat the second's outcome.
    let actions = app.take_actions();
    let loads: Vec<MessageId> = actions
        .iter()
        .filter_map(|action| match action {
            Action::LoadMessage { message, .. } => Some(*message),
            _ => None,
        })
        .collect();
    let reloads = actions
        .iter()
        .filter(|action| matches!(action, Action::Reload { .. }))
        .count();
    assert_eq!(loads, vec![row], "one refetch, for the one stored row");
    assert_eq!(reloads, 1, "and one full reread when the turn ended");
}

/// Snapshot one: the composer band, empty, with its placeholder and its rail.
#[test]
fn the_empty_composer_shows_its_placeholder_behind_an_accent_rail() {
    let (app, _, _) = booted(70, 20);
    let first = rows(&app, 70, 20);
    let frame = first.join("\n");
    assert!(
        frame.contains(crate::tui::view::composer::placeholder()),
        "the empty field says what it is for: {frame}"
    );

    // The composer's rail is the transcript's: the accent column, one gutter in
    // from the frame's edge.
    let gutter = usize::from(crate::tui::view::GUTTER);
    let rail_row = first
        .iter()
        .position(|row| row.contains(crate::tui::view::composer::placeholder()))
        .expect("the composer's text") as u16;
    assert_eq!(
        foreground(&app, 70, 20, gutter as u16, rail_row),
        crate::tui::theme::ACCENT,
        "the composer's rail is blue, like a user message's"
    );

    // The field's text is inset from the rail: rail, then two cells of padding.
    // Measured on typed text, in cells not bytes: the placeholder path adds the
    // widget's own one-cell cursor span, so the empty state sits one cell over.
    let mut app = app;
    type_text(&mut app, "x");
    let typed = rows(&app, 70, 20);
    let line = typed
        .iter()
        .find(|row| row.contains('x'))
        .expect("the typed buffer");
    let byte = line.find('x').expect("the character's column");
    let column = line[..byte].chars().count();
    assert_eq!(
        column,
        gutter + 3,
        "the text starts two cells right of the rail, not against it: {line:?}"
    );

    // And one blank row separates the band from the transcript above it. The
    // band's first row is the rail on its top padding row.
    let band_start = typed
        .iter()
        .position(|row| row.chars().nth(gutter) == Some('\u{2502}'))
        .expect("the band's first row");
    assert!(band_start > 0, "the band is not the first region");
    assert!(
        typed[band_start - 1].trim().is_empty(),
        "one blank row separates the transcript from the band: {:?}",
        typed[band_start - 1]
    );
}

/// Snapshot two: a streaming turn, with the activity line and the caret.
#[test]
fn a_streaming_turn_shows_the_activity_line_and_a_caret() {
    use crate::tui::state::activity::PROGRESS_CELLS;
    use crate::tui::view::notice::PROGRESS_CELL;

    let (mut app, _, session_id) = booted(70, 20);
    app.take_actions();
    assert_eq!(
        screen(&app, 70, 20).matches(PROGRESS_CELL).count(),
        0,
        "an idle frame has no progress widget"
    );

    app.update(Msg::Done(ActionOutcome::Transcript {
        session: session_id,
        generation: app.transcript().generation(),
        messages: vec![
            Message::user("read the config"),
            Message::assistant("It sets the port to 1431."),
        ],
    }));
    app.update(Msg::Agent(delta(session_id, "text")));

    let frame = screen(&app, 70, 20);
    assert!(frame.contains("Responding"), "{frame}");
    assert!(
        frame.contains('\u{258d}'),
        "the caret marks the end of the streaming text: {frame}"
    );
    assert_eq!(
        frame.matches(PROGRESS_CELL).count(),
        PROGRESS_CELLS,
        "the footer carries the running turn's progress widget: {frame}"
    );
    // It sits at the right edge, under the composer.
    let footer = frame.lines().last().expect("a footer");
    assert!(
        footer
            .trim_end()
            .ends_with(&PROGRESS_CELL.to_string().repeat(PROGRESS_CELLS)),
        "the widget is the last thing on the footer row: {footer:?}"
    );
}

/// Assistant markdown renders as a document, not as its source characters.
///
/// The reported bug: a heading and a table were painted as the literal `##` and
/// `|` characters, so a structured answer read as a wall of punctuation. This
/// asserts the frame, because that is where it was wrong.
#[test]
fn an_assistant_heading_and_table_render_as_a_document() {
    let app = with_messages(
        60,
        24,
        vec![
            Message::user("what changed"),
            Message::assistant(
                "## Migrations\n\n| File | Change |\n|:-----|-------:|\n| a.rs | added  |\n",
            ),
        ],
    );
    let rows = rows(&app, 60, 24);
    let frame = rows.join("\n");

    assert!(frame.contains("Migrations"), "{frame}");
    assert!(
        !frame.contains("## Migrations"),
        "the heading marker is not printed: {frame}"
    );
    assert!(
        frame.contains('\u{250c}') && frame.contains('\u{2502}'),
        "the table is drawn as a box: {frame}"
    );
    assert!(
        !frame.contains("|:-----"),
        "the delimiter row is markup, not content: {frame}"
    );

    // And the heading is painted differently from the prose beneath it, so the
    // structure survives even where the marker does not.
    let heading_row = rows
        .iter()
        .position(|row| row.contains("Migrations"))
        .expect("the heading's row") as u16;
    let heading_tone = foreground(&app, 60, 24, 2, heading_row);
    assert_eq!(
        heading_tone,
        crate::tui::theme::INK_STRONG,
        "a heading is titled, not body text"
    );
}

#[test]
fn the_arrows_select_from_an_empty_composer_and_the_caret_keeps_them_otherwise() {
    let messages: Vec<Message> = (0..40)
        .map(|n| Message::assistant(format!("line {n}")))
        .collect();
    let mut app = with_messages(60, 14, messages);
    assert_eq!(app.focus(), Focus::Composer, "the field starts with it");

    // An empty field has no caret to move, so `Up` reaches the transcript, takes
    // the keyboard with it, and selects the last block.
    app.update(Msg::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(app.focus(), Focus::Transcript, "the keyboard changed hands");
    assert!(
        app.transcript().selection().is_some(),
        "the last block is selected"
    );
    assert!(app.composer().text().is_empty(), "and it typed nothing");

    // The wheel still scrolls, through the same path as before.
    let offset = app.transcript().offset();
    app.update(Msg::Scroll(-3));
    assert!(app.transcript().offset() < offset, "a wheel notch moved it");

    // Two blocks up from the last is an earlier block, and the selection follows.
    let first = app.transcript().selection();
    app.update(Msg::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_ne!(
        app.transcript().selection(),
        first,
        "the arrow moved the selection rather than scrolling"
    );

    // With text in the field the arrows belong to the caret; nothing is selected.
    press(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    type_text(&mut app, "one");
    app.update(Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
    type_text(&mut app, "two");
    assert_eq!(app.composer().text(), "one\ntwo");
    let selected = app.transcript().selection();
    app.update(Msg::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(
        app.transcript().selection(),
        selected,
        "the field kept the arrow: a multi-line draft needs its caret"
    );
    assert_eq!(app.composer().text(), "one\ntwo", "and nothing was sent");
}

// -- T2: the pause -------------------------------------------------------

/// A paused turn: one waiting call, the card selected, and the transcript landed.
fn paused(width: u16, height: u16) -> (App, SessionId, ToolCallId) {
    let (mut app, _, session) = booted(width, height);
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    let id = call.id;
    app.update(Msg::Agent(approval_frame(session, id)));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );
    app.take_actions();
    (app, session, id)
}

fn decisions(app: &mut App) -> Vec<(ToolCallId, Decision)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Decide { call, decision, .. } => Some((call, decision)),
            _ => None,
        })
        .collect()
}

fn press(app: &mut App, key: KeyEvent) {
    app.update(Msg::Key(key));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn a_pause_selects_the_waiting_card_and_leaves_the_keyboard_alone() {
    let (app, _, call) = paused(80, 24);
    // The card is selected, so the keys are on the thing that needs an answer.
    assert_eq!(app.transcript().selected_call(), Some(call));
    assert_eq!(
        app.approval().map(|approval| approval.pending().to_vec()),
        Some(vec![call])
    );
    // No mode was entered: a pause is state, not a surface.
    assert_eq!(app.mode(), Mode::Chat);
    assert!(app.transcript().selection().is_some());
}

#[test]
fn the_approval_keys_act_on_the_selected_card_only() {
    let (mut app, _, call) = paused(80, 24);
    // A pause hands the keyboard to the card, which is the whole point of it.
    assert_eq!(app.focus(), Focus::Transcript);

    // Back in the field, `a` is a character.
    press(&mut app, key(KeyCode::Tab));
    assert_eq!(app.focus(), Focus::Composer);
    press(&mut app, key(KeyCode::Char('a')));
    assert!(decisions(&mut app).is_empty(), "`a` in the field is text");

    // On the card it approves the selected call.
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Char('a')));
    assert_eq!(decisions(&mut app), vec![(call, Decision::Approve)]);
}

#[test]
fn an_approval_key_off_target_says_so_instead_of_deciding() {
    let (mut app, _, session) = booted(80, 24);
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    land(
        &mut app,
        session,
        vec![
            Message::user("take a look"),
            Message::assistant_with_tool_calls("", vec![call]),
        ],
    );
    // One block up from the card is the message's prose, which is not a call.
    press(&mut app, key(KeyCode::Up));
    assert_eq!(
        app.transcript().selection().and_then(|block| block.call),
        None
    );
    press(&mut app, key(KeyCode::Char('a')));
    assert!(
        decisions(&mut app).is_empty(),
        "an approval key on a paragraph decides nothing"
    );
    assert!(
        app.notice().is_some(),
        "a keystroke that does nothing is indistinguishable from one that was lost"
    );
}

#[test]
fn enter_opens_a_card_in_the_detail_view_and_esc_returns() {
    let (mut app, _, call) = paused(80, 24);
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::Detail);
    let detail = app.detail().expect("the detail view opened");
    assert_eq!(detail.call, call);
    assert!(detail.len() > 1, "the heading and the body: {detail:?}");

    let screen = screen(&app, 80, 24);
    assert!(screen.contains("# shell"), "{screen}");
    assert!(screen.contains("back"), "{screen}");

    press(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Chat);
    assert!(app.detail().is_none());
    assert_eq!(
        app.transcript().selected_call(),
        Some(call),
        "the selection is where the reader left it"
    );
}

#[test]
fn enter_on_a_paragraph_does_nothing() {
    let mut app = with_messages(80, 24, vec![Message::user("just prose")]);
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    assert!(
        app.transcript().selection().is_some(),
        "a block is selected"
    );
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::Chat, "a message has no second view");
    assert!(app.detail().is_none());
}

#[test]
fn the_detail_view_scrolls_a_body_longer_than_the_band() {
    let (mut app, _, session) = booted(80, 24);
    let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut call = pending_call("shell", serde_json::json!({ "command": "seq 200" }));
    call.execution_status = ExecutionStatus::Succeeded;
    call.approval_status = ApprovalStatus::Approved;
    call.result = Some(serde_json::json!({ "stdout": body }));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );

    // Nothing is pending here, so no card is selected on arrival: take the
    // keyboard and pick it.
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    // Two hundred lines and the heading: far past the card's forty-line cap.
    assert!(app.detail().expect("open").len() > 200);

    press(&mut app, key(KeyCode::PageDown));
    assert!(
        app.detail().expect("still open").offset > 0,
        "a page moved the view"
    );
    press(&mut app, key(KeyCode::End));
    let bottom = app.detail().expect("still open").offset;
    press(&mut app, key(KeyCode::PageDown));
    assert_eq!(
        app.detail().expect("still open").offset,
        bottom,
        "the bottom is the bottom"
    );
    press(&mut app, key(KeyCode::Home));
    assert_eq!(app.detail().expect("still open").offset, 0);
}

#[test]
fn esc_unwinds_the_detail_view_then_the_selection_then_the_turn() {
    let (mut app, _, _) = paused(80, 24);
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::Detail);

    press(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Chat);
    assert!(app.transcript().selection().is_some());

    press(&mut app, key(KeyCode::Esc));
    assert!(
        app.transcript().selection().is_none(),
        "the selection is cleared"
    );
    assert!(!app.should_quit());
}

#[test]
fn a_send_while_paused_rejects_the_waiting_calls_by_clearing_the_pause() {
    let (mut app, _, _) = paused(80, 24);
    assert!(app.approval().is_some());

    // The pause took the keyboard for the card; the composer is one `Tab` back.
    press(&mut app, key(KeyCode::Tab));
    assert_eq!(app.focus(), Focus::Composer);
    type_text(&mut app, "never mind");
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(
        submits(&mut app).len(),
        1,
        "the send was taken, not refused"
    );
    assert!(
        app.approval().is_none(),
        "the loop rejects the waiting calls, so the pause is over"
    );
    assert_eq!(app.mode(), Mode::Chat);
}

#[test]
fn a_send_while_a_turn_runs_is_taken_rather_than_refused() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();
    // A turn is already running: no send is in flight, so the field is free.
    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::TurnStarted { session },
    )));
    assert!(app.is_busy());

    type_text(&mut app, "never mind");
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(
        submits(&mut app),
        vec![(session, "never mind".to_owned())],
        "the send is handed to the runtime, which interrupts and queues"
    );
}

#[test]
fn r_opens_the_reason_field_labeled_with_the_card() {
    let (mut app, _, _) = paused(80, 24);
    press(&mut app, key(KeyCode::Char('r')));
    assert_eq!(app.mode(), Mode::Prompt);
    let label = app
        .prompt()
        .map(|prompt| prompt.label.clone())
        .unwrap_or_default();
    assert!(label.contains("Reject"), "{label}");

    // `Esc` closes it and keeps what was typed; `r` reopens it with the text.
    type_text(&mut app, "too risky");
    press(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Chat);
    press(&mut app, key(KeyCode::Char('r')));
    assert_eq!(
        app.prompt().map(|prompt| prompt.text()),
        Some("too risky".to_owned())
    );
}

#[test]
fn enter_in_the_reason_field_issues_one_reject_with_the_typed_reason() {
    let (mut app, _, call) = paused(80, 24);
    press(&mut app, key(KeyCode::Char('r')));
    type_text(&mut app, "it deletes the tree");
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(
        decisions(&mut app),
        vec![(
            call,
            Decision::Reject {
                reason: Some("it deletes the tree".to_owned())
            }
        )]
    );
    assert_eq!(app.mode(), Mode::Chat, "the field closes on a decision");
}

#[test]
fn one_decision_is_in_flight_at_a_time() {
    let (mut app, session, call) = paused(80, 24);
    press(&mut app, key(KeyCode::Char('a')));
    assert_eq!(decisions(&mut app).len(), 1);

    press(&mut app, key(KeyCode::Char('a')));
    assert!(
        decisions(&mut app).is_empty(),
        "a second decision would be refused by the runtime"
    );

    app.update(Msg::Done(ActionOutcome::Decided { session, call }));
    assert!(app.approval().is_none(), "the last call ends the pause");
    assert!(app.is_busy(), "the turn resumed");
}

#[test]
fn approve_all_queues_the_rest_and_the_next_pause_releases_them() {
    let (mut app, session, first) = paused(80, 24);
    let second = pending_call("shell", serde_json::json!({ "command": "ls" }));
    let second_id = second.id;

    app.update(Msg::Agent(approval_frame(session, second_id)));
    app.update(Msg::Agent(approval_frame(session, first)));
    assert_eq!(app.approval().map(|a| a.len()), Some(2));

    // Walk the selection onto the first card, then approve everything.
    press(&mut app, key(KeyCode::Up));
    press(&mut app, key(KeyCode::Up));
    assert_eq!(app.transcript().selected_call(), Some(first));
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT),
    );
    let issued = decisions(&mut app);
    assert_eq!(issued.len(), 1, "one decision in flight");

    app.update(Msg::Done(ActionOutcome::Decided {
        session,
        call: issued[0].0,
    }));
    assert!(decisions(&mut app).is_empty(), "the actor is running");

    let other = if issued[0].0 == first {
        second_id
    } else {
        first
    };
    app.update(Msg::Agent(approval_frame(session, other)));
    assert_eq!(decisions(&mut app), vec![(other, Decision::Approve)]);
}

#[test]
fn ctrl_c_while_paused_refuses_the_batch_instead_of_stopping() {
    let (mut app, _, _) = paused(80, 24);
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert_eq!(decisions(&mut app).len(), 1);
    assert_eq!(decisions(&mut app), vec![], "only one decision was issued");
    assert!(
        stops(&mut app).is_empty(),
        "a paused turn has no actor to stop"
    );
    assert!(!app.should_quit());
}

#[test]
fn the_paused_turn_finish_keeps_the_pause() {
    // The regression: a pause emits `awaiting_approval` and then
    // `turn_finished { kind: "paused" }`, because the actor exits.
    let (mut app, session, call) = paused(80, 24);
    app.update(Msg::Agent(paused_frame(session)));
    assert_eq!(
        app.approval().map(|approval| approval.pending().to_vec()),
        Some(vec![call]),
        "the pause survives the actor exiting"
    );
    assert_eq!(app.title(), "robi \u{2014} approval needed");
}

#[test]
fn a_live_pause_selects_the_card_and_a_decision_ends_it() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    let call_id = call.id;
    let message = Message::assistant_with_tool_calls("", vec![call]);
    let message_id = message.id;

    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::TurnStarted { session },
    )));
    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::MessageAdded {
            session,
            message: message_id,
        },
    )));
    app.update(Msg::Agent(approval_frame(session, call_id)));
    land(&mut app, session, vec![message]);
    app.update(Msg::Agent(paused_frame(session)));

    assert_eq!(app.transcript().selected_call(), Some(call_id));
    let screen = screen(&app, 80, 24);
    assert!(
        screen.contains("approval"),
        "the footer offers the actions: {screen}"
    );

    press(&mut app, key(KeyCode::Char('a')));
    assert_eq!(decisions(&mut app), vec![(call_id, Decision::Approve)]);
    app.update(Msg::Done(ActionOutcome::Decided {
        session,
        call: call_id,
    }));
    assert!(app.approval().is_none());
}

#[test]
fn turn_finished_closes_the_pause() {
    let (mut app, session, _) = paused(80, 24);
    assert!(app.approval().is_some());
    app.update(Msg::Agent(finished(session)));
    assert!(app.approval().is_none());
    assert_eq!(app.title(), "robi");
}

#[test]
fn a_pause_reports_itself_in_the_title_and_rings_only_when_unfocused() {
    let (mut app, _, _) = paused(80, 24);
    assert_eq!(app.title(), "robi \u{2014} approval needed");
    assert!(!app.take_bell(), "the window had focus");

    let (mut app, _, _) = booted(80, 24);
    app.update(Msg::Focus(false));
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    app.update(Msg::Agent(approval_frame(
        app.open_session_id().expect("a session"),
        call.id,
    )));
    assert!(app.take_bell(), "a pause nobody is watching rings");
}

#[test]
fn esc_stops_a_running_turn_and_never_quits_when_idle() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();
    press(&mut app, key(KeyCode::Esc));
    assert!(!app.should_quit(), "an idle Esc must not leave");
    assert!(stops(&mut app).is_empty());

    type_text(&mut app, "hello");
    press(&mut app, key(KeyCode::Enter));
    app.take_actions(); // the submit
    press(&mut app, key(KeyCode::Esc));
    assert_eq!(stops(&mut app).len(), 1, "Esc stops the turn");
    assert!(!app.should_quit());

    // `Ctrl-C` keeps its T1 meaning.
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(app.should_quit(), "an idle Ctrl-C still leaves");
}

#[test]
fn reopening_a_session_paused_by_an_earlier_run_selects_the_card() {
    let (mut app, _, session) = booted(80, 24);
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    let call_id = call.id;
    // No `awaiting_approval` frame: this process never saw the pause, which is
    // what a restart or a session switch looks like.
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );

    assert_eq!(
        app.approval().map(|approval| approval.pending().to_vec()),
        Some(vec![call_id])
    );
    assert_eq!(app.transcript().selected_call(), Some(call_id));

    press(&mut app, key(KeyCode::Char('a')));
    assert_eq!(decisions(&mut app), vec![(call_id, Decision::Approve)]);
}

#[test]
fn a_transcript_with_no_pending_call_opens_no_pause() {
    let (mut app, _, session) = booted(80, 24);
    // A call that ran without asking is `pending` approval with `succeeded`
    // execution: a result row, not a prompt.
    let mut ran = pending_call("shell", serde_json::json!({ "command": "ls" }));
    ran.execution_status = ExecutionStatus::Succeeded;
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![ran])],
    );
    assert!(app.approval().is_none());
    assert_eq!(app.mode(), Mode::Chat);
}

#[test]
fn switching_session_drops_the_previous_sessions_pause() {
    let (mut app, first, _) = paused(80, 24);
    assert!(app.approval().is_some());

    let workspace = app.active_workspace_id().expect("a workspace");
    let second = session(workspace, Some("b"));
    let second_id = second.id;
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        sessions: vec![second],
    }));

    assert_eq!(app.open_session_id(), Some(second_id));
    assert_ne!(app.open_session_id(), Some(first));
    assert!(app.approval().is_none(), "the pause belongs to its session");
    assert_eq!(app.mode(), Mode::Chat);
}

#[test]
fn the_selected_block_is_lighter_than_everything_else() {
    // A marker on a heading was too quiet, and prose had no marker at all: the
    // whole block now carries a lighter background.
    let messages = vec![
        Message::user("read the config"),
        Message::assistant("It sets the port to 1431."),
    ];
    let mut app = with_messages(60, 18, messages);
    // Nothing selected: the user band and the prose are on their own tones.
    let unselected_user = background(&app, 60, 18, 5, 2);
    let unselected_prose = background(&app, 60, 18, 5, 5);
    assert_ne!(unselected_user, crate::tui::theme::BG_SELECTED);

    // Select the user message: every row of it steps up.
    press(&mut app, key(KeyCode::Up));
    press(&mut app, key(KeyCode::Up));
    assert_eq!(
        app.transcript().selection().and_then(|block| block.call),
        None,
        "a prose block is selected"
    );
    let selected_user = background(&app, 60, 18, 5, 2);
    assert_eq!(
        selected_user,
        crate::tui::theme::BG_SELECTED,
        "the selected band is the selection tone"
    );
    assert_ne!(selected_user, unselected_user, "and it is lighter");

    // The prose underneath did not move: only the selected block is lifted.
    assert_eq!(
        background(&app, 60, 18, 5, 5),
        unselected_prose,
        "the unselected block keeps its own tone"
    );

    // The last content column, inside the gutter: a band already fills its row,
    // so the whole line highlights, not just the words on it.
    assert_eq!(
        background(&app, 60, 18, 57, 2),
        crate::tui::theme::BG_SELECTED,
        "the fill reaches the end of the row"
    );
}

#[test]
fn selecting_a_paragraph_lifts_it_off_the_canvas() {
    // Prose had no marker at all, so a selected paragraph was invisible.
    let mut app = with_messages(60, 18, vec![Message::assistant("plain prose")]);
    assert_eq!(
        background(&app, 60, 18, 5, 1),
        crate::tui::theme::BG_CANVAS,
        "prose sits on the canvas"
    );

    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    assert_eq!(
        background(&app, 60, 18, 5, 1),
        crate::tui::theme::BG_SELECTED,
        "a selected paragraph has a background of its own"
    );
    // Prose has no band to fill, so the view pads it: without that the highlight
    // would stop at the last glyph and read as an underline.
    assert_eq!(
        background(&app, 60, 18, 50, 1),
        crate::tui::theme::BG_SELECTED,
        "the paragraph is filled to the row's width"
    );
}

#[test]
fn a_selected_card_lifts_its_whole_band_and_keeps_its_ink() {
    let (mut app, _, _) = paused(80, 24);
    // The selected card's heading, subject, and padding rows all step up...
    for y in 1..8 {
        assert_eq!(
            background(&app, 80, 24, 4, y),
            crate::tui::theme::BG_SELECTED,
            "row {y} of the selected card"
        );
    }
    // ... while the foreground stays the row's own, so a heading keeps its muted
    // tone and a diff keeps its colours. Only the background moved.
    assert_eq!(
        foreground(&app, 80, 24, 6, 2),
        crate::tui::theme::INK_MUTED,
        "the heading is still muted"
    );

    // Deselect: the band falls back to its own tone.
    press(&mut app, key(KeyCode::Esc));
    assert_eq!(
        background(&app, 80, 24, 4, 1),
        crate::tui::theme::CODE_BG,
        "an unselected card is the card band again"
    );
}

#[test]
fn a_pause_takes_the_keyboard_off_the_composer() {
    // The question is the app's, not the reader's: the first `a` has to reach the
    // card, not land in a message nobody meant to write.
    let (mut app, _, call) = paused(80, 24);
    assert_eq!(app.focus(), Focus::Transcript);
    assert_eq!(app.transcript().selected_call(), Some(call));

    // And it really is answerable without a Tab.
    press(&mut app, key(KeyCode::Char('a')));
    assert_eq!(decisions(&mut app), vec![(call, Decision::Approve)]);
    assert!(app.composer().text().is_empty(), "nothing was typed");
}

#[test]
fn a_pause_leaves_a_draft_alone_and_only_takes_an_empty_field() {
    // A draft is the reader's work in progress: the pause selects the card but
    // leaves the caret where it was.
    let (mut app, _, session) = booted(80, 24);
    type_text(&mut app, "half a thought");
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    let call_id = call.id;
    app.update(Msg::Agent(approval_frame(session, call_id)));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );

    assert_eq!(
        app.focus(),
        Focus::Composer,
        "an arriving pause does not take the caret out of a draft"
    );
    assert_eq!(app.composer().text(), "half a thought");
    assert_eq!(
        app.transcript().selected_call(),
        Some(call_id),
        "the card is still selected, and its rail still says it needs an answer"
    );

    // An empty field has nothing to protect, so the pause takes the keyboard —
    // once there is a card to put it on, which is when the transcript lands.
    let (mut app, _, session) = booted(80, 24);
    let call = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    app.update(Msg::Agent(approval_frame(session, call.id)));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );
    assert_eq!(app.focus(), Focus::Transcript);
    assert_eq!(app.mode(), Mode::Chat);
}

#[test]
fn a_pause_does_not_interrupt_an_open_detail_view() {
    let (mut app, _, session) = booted(80, 24);
    let first = pending_call("shell", serde_json::json!({ "command": "one" }));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![first])],
    );
    // The pause selected the card and took the keyboard, so Enter opens it.
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), Mode::Detail);

    let another = pending_call("shell", serde_json::json!({ "command": "two" }));
    app.update(Msg::Agent(approval_frame(session, another.id)));
    assert_eq!(
        app.mode(),
        Mode::Detail,
        "a view the reader opened on purpose stays open"
    );
}

#[test]
fn a_waiting_card_has_an_attention_rail() {
    /// The left edge of a single-card transcript's heading row.
    fn rail(app: &App) -> ratatui::style::Color {
        foreground(app, 80, 24, 2, 2)
    }

    fn with_call(call: ToolCall) -> App {
        let (mut app, _, session) = booted(80, 24);
        land(
            &mut app,
            session,
            vec![Message::assistant_with_tool_calls("", vec![call])],
        );
        app
    }

    let waiting = with_call(pending_call(
        "shell",
        serde_json::json!({ "command": "rm -rf x" }),
    ));
    assert_eq!(
        rail(&waiting),
        crate::tui::theme::ATTENTION,
        "a pending call's rail is the attention tone"
    );

    // The same call, settled: the rail goes quiet again.
    let mut ran = pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
    ran.approval_status = ApprovalStatus::Approved;
    let done = with_call(ran);
    assert_eq!(
        rail(&done),
        crate::tui::theme::INK_FAINT,
        "a settled card is back to its quiet rail"
    );
}

#[test]
fn clearing_a_selection_hands_the_keyboard_back_to_the_composer() {
    // The keys that acted on the block mean nothing once it is gone, and typing is
    // what a chat is for: `Esc` puts the reader back in the field.
    let mut app = with_messages(80, 24, vec![Message::user("just prose")]);
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    assert!(app.transcript().selection().is_some());
    assert_eq!(app.focus(), Focus::Transcript);

    press(&mut app, key(KeyCode::Esc));
    assert!(app.transcript().selection().is_none());
    assert_eq!(app.focus(), Focus::Composer);

    // And it really is typing again, rather than just claiming the focus.
    type_text(&mut app, "hi");
    assert_eq!(app.composer().text(), "hi");
}

#[test]
fn esc_with_nothing_selected_leaves_the_focus_alone() {
    // The rule is about clearing a selection, not about `Esc` in general.
    let mut app = with_messages(80, 24, vec![Message::user("just prose")]);
    press(&mut app, key(KeyCode::Tab));
    assert_eq!(app.focus(), Focus::Transcript);

    press(&mut app, key(KeyCode::Esc));
    assert_eq!(
        app.focus(),
        Focus::Transcript,
        "with nothing to clear, `Esc` is not a focus command"
    );
}

#[test]
fn esc_clearing_a_selection_mid_turn_still_returns_to_the_composer() {
    let mut app = with_messages(80, 24, vec![Message::user("just prose")]);
    let session = app.open_session_id().expect("a session");
    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::TurnStarted { session },
    )));
    assert!(app.is_busy());
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Up));
    assert!(app.transcript().selection().is_some());

    press(&mut app, key(KeyCode::Esc));
    assert!(app.transcript().selection().is_none());
    assert_eq!(app.focus(), Focus::Composer);
    assert!(
        stops(&mut app).is_empty(),
        "the first Esc cleared the selection; it did not stop the turn"
    );
}

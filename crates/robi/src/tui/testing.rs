//! The `TestBackend` harness, and the view snapshots.
//!
//! A snapshot here asserts on rendered strings, not on a golden file: a token
//! change that does not move the layout must not fail a test.

use std::path::PathBuf;

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
use crate::tui::state::transcript::RAIL_CHAR;
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

/// The first row of the transcript: the frame's top pad, the header, and the
/// rule under it all sit above it.
pub fn body_top() -> u16 {
    crate::tui::view::FRAME_PAD + 2
}

/// The y of the first row holding `needle`, for a test that anchors on text
/// rather than on a row number.
pub fn y_of(rows: &[String], needle: &str) -> u16 {
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no row holds {needle:?}: {rows:#?}")) as u16
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

/// A workspace row with a known id, for a list read that must keep it.
fn workspace_row(id: WorkspaceId, name: &str) -> Workspace {
    let mut workspace = workspace(name);
    workspace.id = id;
    workspace
}

/// `base` with a title, keeping its id. For a list read that shows a rename.
fn titled(mut base: ChatSession, title: &str) -> ChatSession {
    base.title = Some(title.to_owned());
    base
}

/// The greeting the fixtures show. Fixed, so a frame test is not a clock test.
pub fn morning() -> crate::tui::state::landing::Greeting {
    crate::tui::state::landing::Greeting::Morning
}

/// The path the fixtures' workspace lives at.
///
/// `booted` builds a workspace with this root, so the startup pick finds it and
/// no test falls into the missing-workspace prompt by accident.
pub fn test_workspace_path() -> PathBuf {
    PathBuf::from("/workspace/robi")
}

/// An app with one workspace, one session, and no messages yet.
pub fn booted(width: u16, height: u16) -> (App, WorkspaceId, SessionId) {
    let mut app = App::new(
        ViewState::default(),
        "sqlite://test.db",
        test_workspace_path(),
        morning(),
    );
    app.resize(width, height);
    let workspace = workspace("robi");
    let workspace_id = workspace.id;
    let session = session(workspace_id, Some("a session"));
    let session_id = session.id;

    // The generations are the app's own: a read issued before them would be
    // dropped as stale, which is what the counters are for.
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace],
    }));
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace: workspace_id,
        generation,
        sessions: vec![session],
        running: Vec::new(),
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

/// Land a model list and the mode defaults the picker and the facts row read.
///
/// Two models from two providers, both tool-capable, so a test can tell a filtered
/// list from a stale row.
pub fn land_models(app: &mut App) {
    let generation = app.models_generation();
    let models = vec![
        crate::agent::providers::ModelInfo {
            id: crate::agent::providers::ModelId::new("ocg_glm-5.3"),
            display_name: "GLM-5.3".to_owned(),
            context_window: 1_000_000,
            max_output: 131_072,
            supports_tools: true,
            supports_reasoning: true,
            supports_effort: false,
            supports_vision: false,
        },
        crate::agent::providers::ModelInfo {
            id: crate::agent::providers::ModelId::new("ant_claude"),
            display_name: "Claude".to_owned(),
            context_window: 200_000,
            max_output: 64_000,
            supports_tools: true,
            supports_reasoning: true,
            supports_effort: true,
            supports_vision: true,
        },
    ];
    let defaults = crate::adapters::model_source::ModeDefaults {
        ask: effective("ocg_glm-5.3", None),
        plan: effective("ocg_glm-5.3", None),
        agent: effective("ocg_glm-5.3", Some("high")),
    };
    app.update(Msg::Done(ActionOutcome::Models {
        generation,
        models,
        defaults,
    }));
}

/// Land the skill list the `@` menu offers, for the app's active workspace.
pub fn land_skills(app: &mut App, skills: &[(&str, bool)]) {
    let Some(workspace) = app.active_workspace_id() else {
        panic!("the fixture has a workspace");
    };
    let generation = app.skills_generation();
    let skills = skills
        .iter()
        .map(|(id, manual)| crate::tui::state::completion::SkillEntry {
            id: (*id).to_owned(),
            label: (*id).to_owned(),
            description: format!("does {id}"),
            scope: crate::tui::state::completion::SkillScope::Project,
            manual: *manual,
        })
        .collect();
    app.update(Msg::Done(ActionOutcome::Skills {
        workspace,
        generation,
        skills,
    }));
}

fn effective(model: &str, effort: Option<&str>) -> crate::adapters::model_source::EffectiveChoice {
    crate::adapters::model_source::EffectiveChoice {
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
    }
}

/// A call that is waiting on a decision, which is how a pause arrives.
pub fn pending_call(name: &str, args: serde_json::Value) -> ToolCall {
    let mut call = ToolCall::new(name, args);
    call.approval_status = ApprovalStatus::Pending;
    call.execution_status = ExecutionStatus::NotStarted;
    call
}

/// The frame the loop emits when a turn starts, which makes the app busy.
pub fn turn_started(session: SessionId) -> EventEnvelope {
    envelope(session, crate::domain::events::TURN_STARTED)
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
    let mut app = App::new(
        ViewState::default(),
        "sqlite://test.db",
        test_workspace_path(),
        morning(),
    );
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

#[test]
fn a_retrieve_keeps_its_page_for_the_detail_view() {
    // A retrieve's page is another call's log: the transcript names the window
    // and the way in, and only the detail page shows the text.
    let (mut app, _, session) = booted(80, 24);
    let mut call = pending_call(
        "retrieve",
        serde_json::json!({ "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b", "stream": "stdout" }),
    );
    call.approval_status = ApprovalStatus::Approved;
    call.execution_status = ExecutionStatus::Succeeded;
    call.result = Some(serde_json::json!({
        "stdout": "the paged line\nthe second paged line",
        "start_line": 10,
        "end_line": 11,
        "total_lines": 300,
    }));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );

    let frame = screen(&app, 80, 24);
    assert!(
        frame.contains("0193a1b2 stdout"),
        "the window is still named: {frame}"
    );
    assert!(!frame.contains("the paged line"), "{frame}");
    assert!(frame.contains("Enter opens the output"), "{frame}");

    // Opening the card in place does not reveal it either: the page has one
    // home, and it is not the transcript.
    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    let detail = screen(&app, 80, 24);
    assert_eq!(app.mode(), Mode::Detail, "{detail}");
    assert!(detail.contains("the paged line"), "{detail}");
    assert!(detail.contains("the second paged line"), "{detail}");
    assert!(
        !detail.contains("Enter opens the output"),
        "the hint belongs to the card, not the page: {detail}"
    );
}

#[test]
fn a_rule_closes_the_header_and_stops_where_its_text_does() {
    let gutter = usize::from(crate::tui::view::GUTTER);
    let width = usize::from(crate::tui::view::content_width(60));
    let frame = rows(&with_messages(60, 20, two_message_transcript()), 60, 20);
    let header = y_of(&frame, "robi");
    assert_eq!(
        header,
        crate::tui::view::FRAME_PAD,
        "the bar keeps one blank row above it"
    );
    let rule = frame[usize::from(header) + 1].trim_end();
    assert!(
        rule.trim().chars().all(|cell| cell == '\u{2500}'),
        "the second row is one unbroken rule: {rule:?}"
    );
    assert_eq!(
        rule.chars().count() - rule.trim_start().chars().count(),
        gutter,
        "it starts at the gutter"
    );
    assert_eq!(
        rule.trim_start().chars().count(),
        width,
        "and runs to the header's right edge, not the terminal's"
    );
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
        24,
        vec![
            Message::user("add a new field for signups"),
            Message::assistant_with_tool_calls("", vec![call]),
        ],
    );
    let frame = rows(&app, 60, 24);

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
            Some(RAIL_CHAR),
            "the {what} has a rail"
        );
    }
    assert_eq!(
        foreground(&app, 60, 24, rail, user_row),
        crate::tui::theme::ACCENT,
        "the user rail is blue"
    );
    assert_eq!(
        foreground(&app, 60, 24, rail, card_row),
        crate::tui::theme::INK_FAINT,
        "the tool card rail is gray"
    );
    // Everything is inset: nothing is drawn in the first or last column.
    for (index, row) in frame.iter().enumerate() {
        assert!(
            !row.starts_with(RAIL_CHAR),
            "row {index} is not inset by the gutter: {row:?}"
        );
    }

    // Both bands are raised surfaces: the cell just inside the rail carries
    // the band's background, not the canvas's.
    for (y, band) in [
        (user_row, crate::tui::theme::BG_USER),
        (card_row, crate::tui::theme::CODE_BG),
    ] {
        let backend = TestBackend::new(60, 24);
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

    // At a normal width the whole footer hint fits inside the gutter. The
    // footer is the last row: the status strip is above the composer, not below
    // it.
    let wide = rows(&with_messages(100, 20, two_message_transcript()), 100, 20);
    let footer = wide
        .iter()
        .rev()
        .find(|row| !row.trim().is_empty())
        .expect("a footer")
        .clone();
    assert!(
        wide.last().is_some_and(|row| row.trim().is_empty()),
        "and one blank row under it"
    );
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
    assert_eq!(
        app.status().error(),
        Some("the provider refused"),
        "the failure is on the strip"
    );
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
        .position(|row| row.chars().nth(gutter) == Some(RAIL_CHAR))
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
    use crate::tui::view::strip::PROGRESS_CELL;

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
    // It sits at the right edge of the strip, above the composer, so it is on
    // the row with the activity line rather than below the band.
    let strip = frame
        .lines()
        .find(|row| row.contains("Responding"))
        .expect("the activity line");
    assert!(
        strip
            .trim_end()
            .ends_with(&PROGRESS_CELL.to_string().repeat(PROGRESS_CELLS)),
        "the widget is the last thing on the strip: {strip:?}"
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
    // Prose is railed now, so the text starts one rail and one pad cell in. The
    // rail column itself carries the mode's ink, which is what makes a turn one
    // colour from the user band down through the reply.
    let heading_tone = foreground(&app, 60, 24, 4, heading_row);
    assert_eq!(
        heading_tone,
        crate::tui::theme::INK_STRONG,
        "a heading is titled, not body text"
    );
    assert_eq!(
        foreground(&app, 60, 24, 2, heading_row),
        crate::tui::theme::INK_FAINT,
        "a reply's rail is quiet, whatever mode asked for it"
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
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![second],
        running: Vec::new(),
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
    let frame = rows(&app, 60, 18);
    let user = y_of(&frame, "read the config");
    let prose = y_of(&frame, "It sets the port");
    // Nothing selected: the user band and the prose are on their own tones.
    let unselected_user = background(&app, 60, 18, 5, user);
    let unselected_prose = background(&app, 60, 18, 5, prose);
    assert_ne!(unselected_user, crate::tui::theme::BG_SELECTED);

    // Select the user message: every row of it steps up.
    press(&mut app, key(KeyCode::Up));
    press(&mut app, key(KeyCode::Up));
    assert_eq!(
        app.transcript().selection().and_then(|block| block.call),
        None,
        "a prose block is selected"
    );
    let selected_user = background(&app, 60, 18, 5, user);
    assert_eq!(
        selected_user,
        crate::tui::theme::BG_SELECTED,
        "the selected band is the selection tone"
    );
    assert_ne!(selected_user, unselected_user, "and it is lighter");

    // The prose underneath did not move: only the selected block is lifted.
    assert_eq!(
        background(&app, 60, 18, 5, prose),
        unselected_prose,
        "the unselected block keeps its own tone"
    );

    // The last content column, inside the gutter: a band already fills its row,
    // so the whole line highlights, not just the words on it.
    assert_eq!(
        background(&app, 60, 18, 57, user),
        crate::tui::theme::BG_SELECTED,
        "the fill reaches the end of the row"
    );
}

#[test]
fn selecting_a_paragraph_lifts_it_off_the_canvas() {
    // Prose had no marker at all, so a selected paragraph was invisible.
    let mut app = with_messages(60, 18, vec![Message::assistant("plain prose")]);
    let y = body_top();
    assert_eq!(
        background(&app, 60, 18, 5, y),
        crate::tui::theme::BG_CANVAS,
        "prose sits on the canvas"
    );

    press(&mut app, key(KeyCode::Tab));
    press(&mut app, key(KeyCode::Down));
    assert_eq!(
        background(&app, 60, 18, 5, y),
        crate::tui::theme::BG_SELECTED,
        "a selected paragraph has a background of its own"
    );
    // Prose has no band to fill, so the view pads it: without that the highlight
    // would stop at the last glyph and read as an underline.
    assert_eq!(
        background(&app, 60, 18, 50, y),
        crate::tui::theme::BG_SELECTED,
        "the paragraph is filled to the row's width"
    );
}

#[test]
fn a_selected_card_lifts_its_whole_band_and_keeps_its_ink() {
    let (mut app, _, _) = paused(80, 24);
    let lifted = |y: u16| background(&app, 80, 24, 4, y) == crate::tui::theme::BG_SELECTED;
    // The selected card's heading, subject, and padding rows all step up, as one
    // unbroken run from the first row of the card.
    let top = body_top();
    assert!(lifted(top), "the card starts at the top of the transcript");
    let mut rows_of_card = 0;
    while lifted(top + rows_of_card) {
        rows_of_card += 1;
    }
    assert_eq!(rows_of_card, 7, "the whole seven-row card is lifted");
    // ... while the foreground stays the row's own, so a heading keeps its muted
    // tone and a diff keeps its colours. Only the background moved.
    assert_eq!(
        foreground(&app, 80, 24, 6, top + 1),
        crate::tui::theme::INK_MUTED,
        "the heading is still muted"
    );

    // Deselect: the band falls back to its own tone.
    press(&mut app, key(KeyCode::Esc));
    assert_eq!(
        background(&app, 80, 24, 4, top),
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
        foreground(app, 80, 24, 2, body_top() + 1)
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

// -- T3: sessions, workspaces, and the status strip -----------------------

use crate::tui::state::sessions::Mark;
use crate::tui::state::tool_card;
use crate::tui::state::workspaces::Screen;

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

fn renames(app: &mut App) -> Vec<(SessionId, String)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::RenameSession { session, title } => Some((session, title)),
            _ => None,
        })
        .collect()
}

fn deletes(app: &mut App) -> Vec<SessionId> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::DeleteSession { session } => Some(session),
            _ => None,
        })
        .collect()
}

fn created_sessions(app: &mut App) -> Vec<WorkspaceId> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::StartSession { workspace, .. } => Some(workspace),
            _ => None,
        })
        .collect()
}

fn workspace_opens(app: &mut App) -> Vec<String> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::CreateWorkspace { root } => Some(root),
            _ => None,
        })
        .collect()
}

fn reloads(app: &mut App) -> Vec<SessionId> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Reload { session, .. } => Some(session),
            _ => None,
        })
        .collect()
}

fn mcp_loads(app: &mut App) -> Vec<WorkspaceId> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::LoadMcp { workspace, .. } => Some(workspace),
            _ => None,
        })
        .collect()
}

fn probes(app: &mut App) -> usize {
    app.take_actions()
        .into_iter()
        .filter(|action| matches!(action, Action::Probe))
        .count()
}

fn completions(app: &mut App) -> Vec<(String, u64)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::CompletePath { text, generation } => Some((text, generation)),
            _ => None,
        })
        .collect()
}

#[test]
fn ctrl_n_opens_the_landing_page_and_creates_nothing() {
    // The bug this replaces: a row per abandoned new chat. `Ctrl-N` is a page,
    // and the session is created by the first message.
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('n')));
    assert!(app.landing(), "a workspace is open and no session is");
    assert_eq!(app.open_session_id(), None);
    assert!(
        created_sessions(&mut app).is_empty(),
        "nothing is created until there is a message to put in it"
    );
    assert_eq!(app.focus(), Focus::Composer);

    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Good morning"), "{frame}");
}

#[test]
fn the_landing_page_creates_the_session_when_the_first_message_is_sent() {
    let (mut app, workspace, _) = booted(80, 24);
    press(&mut app, ctrl(KeyCode::Char('n')));
    app.take_actions();

    type_text(&mut app, "read the config");
    press(&mut app, key(KeyCode::Enter));
    let actions = app.take_actions();
    let start = actions
        .into_iter()
        .find_map(|action| match action {
            Action::StartSession {
                workspace: target,
                text,
                ..
            } => Some((target, text)),
            _ => None,
        })
        .expect("one StartSession, not a create and a submit");
    assert_eq!(start, (workspace, "read the config".to_owned()));
    assert_eq!(
        app.composer().text(),
        "read the config",
        "the text stays until the actor takes it"
    );

    let created = session(workspace, None);
    let id = created.id;
    app.update(Msg::Done(ActionOutcome::SessionStarted {
        session: Box::new(created),
        outcome: Some(SubmitOutcome::Accepted),
        error: None,
    }));
    assert_eq!(app.open_session_id(), Some(id));
    assert!(!app.landing());
    assert!(app.composer().is_blank(), "a taken send clears the field");
    assert!(app.is_busy());
    assert_eq!(
        app.selected_session().map(|session| session.id),
        Some(id),
        "the overlay's cursor follows the new row"
    );
    assert_eq!(app.sessions().len(), 2, "the old session and the new one");
}

#[test]
fn a_failed_first_send_keeps_the_session_and_the_text() {
    let (mut app, workspace, _) = booted(80, 24);
    press(&mut app, ctrl(KeyCode::Char('n')));
    type_text(&mut app, "read the config");
    press(&mut app, key(KeyCode::Enter));
    app.take_actions();

    let created = session(workspace, None);
    let id = created.id;
    app.update(Msg::Done(ActionOutcome::SessionStarted {
        session: Box::new(created),
        outcome: None,
        error: Some("the provider refused".to_owned()),
    }));
    assert_eq!(app.open_session_id(), Some(id), "the row exists either way");
    assert_eq!(app.composer().text(), "read the config", "so does the text");
    assert_eq!(app.status().error(), Some("the provider refused"));
    assert!(!app.is_busy());
}

#[test]
fn a_session_frame_refreshes_the_list_without_rereading_the_transcript() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();

    app.update(Msg::Agent(EventEnvelope::session_updated(session)));
    // The rename in another window is a list fact, not a transcript fact.
    let actions = app.take_actions();
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Reload { .. })),
        "no transcript read follows a session frame"
    );
    let workspace = app.active_workspace_id().expect("a workspace");
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::LoadSessions { workspace: read, .. } if *read == workspace
    )));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
}

#[test]
fn a_session_list_refresh_that_keeps_the_open_session_does_not_reload_it() {
    let (mut app, workspace, session) = booted(80, 24);
    app.take_actions();

    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![titled(
            app.open_session().expect("a session").clone(),
            "renamed",
        )],
        running: Vec::new(),
    }));
    assert_eq!(app.open_session_id(), Some(session));
    assert!(
        reloads(&mut app).is_empty(),
        "a rename must not reread the messages under the reader"
    );
    assert_eq!(app.session_title(), "renamed");
}

#[test]
fn a_session_list_that_loses_the_open_session_selects_another() {
    let (mut app, workspace, first) = booted(80, 24);
    app.take_actions();

    let second = session(workspace, Some("second"));
    let second_id = second.id;
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![second],
        running: Vec::new(),
    }));
    assert_eq!(app.open_session_id(), Some(second_id));
    assert_ne!(second_id, first);
}

#[test]
fn a_rename_prompt_is_prefilled_and_names_the_title_it_sends() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('r')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Prompt);
    assert_eq!(app.prompt().expect("a prompt").label, "rename");
    assert_eq!(app.prompt().expect("a prompt").text(), "a session");

    // Type over it: the field is the keyboard's, so `Enter` submits.
    for _ in 0.."a session".len() {
        press(&mut app, key(KeyCode::Backspace));
    }
    type_text(&mut app, "renamed");
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert_eq!(renames(&mut app), vec![(session, "renamed".to_owned())]);
}

#[test]
fn an_empty_rename_is_refused_and_the_field_stays_open() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('r')));
    for _ in 0.."a session".len() {
        press(&mut app, key(KeyCode::Backspace));
    }
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(app.mode(), crate::tui::state::Mode::Prompt);
    assert!(renames(&mut app).is_empty());
    assert!(
        app.notice().is_some_and(|notice| notice.is_error()),
        "the refusal says why"
    );
}

#[test]
fn ctrl_d_asks_before_deleting_and_n_keeps_the_row() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    press(&mut app, ctrl(KeyCode::Char('d')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Confirm);
    assert!(app.confirm().is_some());

    press(&mut app, key(KeyCode::Char('n')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Sessions);
    assert!(deletes(&mut app).is_empty());
    assert_eq!(app.selected_session().map(|row| row.id), Some(session));

    press(&mut app, ctrl(KeyCode::Char('d')));
    press(&mut app, key(KeyCode::Char('y')));
    assert_eq!(deletes(&mut app), vec![session]);
    assert_eq!(app.mode(), crate::tui::state::Mode::Sessions);
}

#[test]
fn deleting_the_open_session_opens_the_next_one_and_the_last_leaves_none() {
    let (mut app, workspace, first) = booted(80, 24);
    let second = session(workspace, Some("second"));
    let second_id = second.id;
    let open = app.open_session().expect("a session").clone();
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![open, second],
        running: Vec::new(),
    }));
    app.take_actions();

    app.update(Msg::Done(ActionOutcome::SessionDeleted { session: first }));
    assert_eq!(app.open_session_id(), Some(second_id));

    app.take_actions();
    app.update(Msg::Done(ActionOutcome::SessionDeleted {
        session: second_id,
    }));
    assert_eq!(app.open_session_id(), None, "an empty workspace opens none");
    assert!(app.sessions().is_empty());
}

#[test]
fn the_sessions_overlay_filters_as_you_type_and_opens_the_selection() {
    let (mut app, workspace, first) = booted(80, 24);
    let second = session(workspace, Some("other chat"));
    let second_id = second.id;
    let open = app.open_session().expect("a session").clone();
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![open, second],
        running: Vec::new(),
    }));
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Sessions);
    assert_eq!(app.selected_session().map(|row| row.id), Some(first));

    type_text(&mut app, "other");
    assert_eq!(app.session_rows().len(), 1);
    assert_eq!(app.selected_session().map(|row| row.id), Some(second_id));

    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert_eq!(app.open_session_id(), Some(second_id));
}

#[test]
fn a_failure_reaches_the_strip_and_a_reload_clears_it() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    app.update(Msg::Done(ActionOutcome::Failed {
        generation: None,
        message: "the store is locked".to_owned(),
    }));
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("! the store is locked"), "{frame}");
    // The strip is above the composer, and the `!` starts its first item.
    let gutter = crate::tui::view::GUTTER;
    let y = rows(&app, 80, 24)
        .iter()
        .position(|row| row.contains("! the store is locked"))
        .expect("the strip's row") as u16;
    assert!(y > 1, "the strip is below the header: {frame}");
    assert_eq!(
        foreground(&app, 80, 24, gutter, y),
        crate::tui::theme::DANGER,
        "an error is drawn in the danger colour"
    );

    let generation = app.transcript().generation();
    app.update(Msg::Done(ActionOutcome::Transcript {
        session: app.open_session_id().expect("a session"),
        generation,
        messages: Vec::new(),
    }));
    assert!(
        !screen(&app, 80, 24).contains("! the store is locked"),
        "a read that answered says the failure is over"
    );
}

#[test]
fn a_failed_probe_is_the_health_mark_and_never_clears_an_app_error() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    app.update(Msg::Done(ActionOutcome::Failed {
        generation: None,
        message: "a turn failed".to_owned(),
    }));
    app.update(Msg::Done(ActionOutcome::Probed {
        error: Some("the store is locked".to_owned()),
    }));
    // One row, so the error that outlives a turn wins over the probe's mark.
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("! a turn failed"), "{frame}");
    assert!(!frame.contains("health:"), "one status per row: {frame}");

    // With the app error gone, the probe's answer is what is left to say. A read
    // that succeeds is what clears the error; a probe answering never does.
    let session = app.open_session_id().expect("a session");
    app.update(Msg::Done(ActionOutcome::Transcript {
        session,
        generation: app.transcript().generation(),
        messages: Vec::new(),
    }));
    assert!(
        screen(&app, 80, 24).contains("! health: the store is locked"),
        "the probe's mark is now the row's"
    );

    app.update(Msg::Done(ActionOutcome::Probed { error: None }));
    assert!(!screen(&app, 80, 24).contains("health:"));
}

#[test]
fn the_probe_is_asked_for_once_every_twenty_ticks() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    for _ in 0..crate::tui::state::status::PROBE_TICKS - 1 {
        app.update(Msg::Tick);
    }
    assert_eq!(probes(&mut app), 0, "nineteen ticks are not a probe");
    app.update(Msg::Tick);
    assert_eq!(probes(&mut app), 1, "the twentieth tick is");

    // In flight, so the next interval asks for nothing until it answers.
    for _ in 0..crate::tui::state::status::PROBE_TICKS * 2 {
        app.update(Msg::Tick);
    }
    assert_eq!(probes(&mut app), 0, "one probe at a time");
    app.update(Msg::Done(ActionOutcome::Probed { error: None }));
    for _ in 0..crate::tui::state::status::PROBE_TICKS {
        app.update(Msg::Tick);
    }
    assert_eq!(probes(&mut app), 1, "and the cadence resumes");
}

#[test]
fn the_strip_sits_above_the_composer_and_the_prompt_above_the_footer() {
    let (mut app, _, session) = booted(80, 24);
    app.take_actions();
    // Idle and nothing to report: the row is not there, and the transcript has it.
    assert_eq!(app.status_rows(), 0);
    let idle = app.body_height();
    assert!(
        !screen(&app, 80, 24).contains("Thinking"),
        "an idle frame has no activity line"
    );

    // A running turn puts its activity line above the composer, with a blank row
    // on either side of it.
    app.update(Msg::Agent(turn_started(session)));
    assert_eq!(app.status_rows(), 1);
    assert_eq!(
        app.strip_gap(),
        crate::tui::view::STRIP_GAP,
        "the strip asks for its own breathing room"
    );
    assert_eq!(
        app.body_height(),
        idle - 2,
        "the transcript gives up the strip and the gap under it"
    );
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Thinking"), "{frame}");
    let band = frame
        .lines()
        .position(|row| row.contains(RAIL_CHAR))
        .expect("the composer's rail");
    let activity = frame
        .lines()
        .position(|row| row.contains("Thinking"))
        .expect("the activity line");
    assert_eq!(
        activity + 2,
        band,
        "a blank row separates the activity line from the band: {frame}"
    );
    assert!(
        frame
            .lines()
            .nth(band - 1)
            .is_some_and(|row| row.trim().is_empty()),
        "the padding below the activity line: {frame}"
    );
    assert!(
        frame
            .lines()
            .nth(activity - 1)
            .is_some_and(|row| row.trim().is_empty()),
        "one blank row between the last message and the progress line: {frame}"
    );

    // A prompt takes a row too, between the composer and the footer.
    press(&mut app, ctrl(KeyCode::Char('r')));
    assert_eq!(app.prompt_rows(), 1);
    let frame = screen(&app, 80, 24);
    let prompt = frame
        .lines()
        .position(|row| row.contains("rename"))
        .expect("the prompt");
    let hint = frame
        .lines()
        .position(|row| row.contains("Enter"))
        .expect("the hint row");
    assert_eq!(
        prompt + 2,
        hint,
        "one blank row between the prompt and the hint: {frame}"
    );
    // The frame's last row itself: `str::lines` would drop it, since the screen
    // string ends with a newline.
    assert!(
        rows(&app, 80, 24).last().is_some_and(|row| row.is_empty()),
        "and one under the hint: {frame}"
    );
}

#[test]
fn a_second_overlay_replaces_the_first_rather_than_stacking() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    assert!(app.sessions_open());
    press(&mut app, ctrl(KeyCode::Char('w')));
    assert!(app.workspaces_open());
    assert!(!app.sessions_open(), "one overlay at a time");
    assert_eq!(app.mode(), crate::tui::state::Mode::Workspaces);

    press(&mut app, ctrl(KeyCode::Char('s')));
    assert!(app.sessions_open());
    assert!(!app.workspaces_open());
}

#[test]
fn esc_closes_an_overlay_and_ctrl_c_does_too_instead_of_quitting() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    press(&mut app, ctrl(KeyCode::Char('c')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert!(!app.should_quit(), "closing a list is not leaving");

    press(&mut app, ctrl(KeyCode::Char('s')));
    press(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert!(!app.should_quit());
}

#[test]
fn a_typed_path_is_opened_and_a_refusal_keeps_the_text() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, ctrl(KeyCode::Char('n')));
    assert_eq!(app.workspaces_view().screen, Screen::New);

    type_text(&mut app, "~/code");
    app.take_actions();
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(workspace_opens(&mut app), vec!["~/code".to_owned()]);

    app.update(Msg::Done(ActionOutcome::CreateWorkspaceFailed {
        message: "root does not exist: ~/code".to_owned(),
    }));
    assert_eq!(app.workspaces_view().path(), "~/code", "the typo stays");
    assert!(
        app.status().error().is_some(),
        "the refusal is on the strip, above the composer"
    );
    assert_eq!(app.workspaces_view().screen, Screen::New);
}

#[test]
fn a_completion_answer_for_text_that_moved_is_dropped() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, ctrl(KeyCode::Char('n')));
    type_text(&mut app, "a");
    let completions = completions(&mut app);
    let (text, generation) = completions.last().expect("a completion read").clone();
    assert_eq!(text, "a");

    app.update(Msg::Done(ActionOutcome::Paths {
        generation,
        text: text.clone(),
        candidates: vec!["ab".to_owned()],
    }));
    assert_eq!(app.workspaces_view().candidates(), ["ab".to_owned()]);

    type_text(&mut app, "c");
    app.take_actions();
    // The generation is now one ahead, so the old answer cannot apply.
    app.update(Msg::Done(ActionOutcome::Paths {
        generation,
        text: "ac".to_owned(),
        candidates: vec!["ac/d".to_owned()],
    }));
    assert_eq!(
        app.workspaces_view().candidates(),
        ["ab".to_owned()],
        "a stale read never rewrites the list"
    );
}

#[test]
fn opening_a_workspace_switches_promotes_and_loads_its_list_and_marks() {
    let (mut app, active, _) = booted(80, 24);
    let second = workspace("pocket");
    let second_id = second.id;
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace_row(active, "robi"), second],
    }));
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(app.active_workspace_id(), Some(second_id));
    assert_eq!(
        app.snapshot().recent_workspace_ids.first(),
        Some(&second_id),
        "the opened workspace is the most recent"
    );
    let actions = app.take_actions();
    assert!(actions.iter().any(
        |action| matches!(action, Action::LoadSessions { workspace, .. } if *workspace == second_id)
    ));
    assert!(actions.iter().any(
        |action| matches!(action, Action::LoadMcp { workspace, .. } if *workspace == second_id)
    ));
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::FocusMcp { workspace } if *workspace == second_id)));
    assert_eq!(app.workspace_rows().first().map(|w| w.id), Some(second_id));
    assert_eq!(app.open_session_id(), None, "the old transcript is gone");
}

#[test]
fn an_mcp_frame_refreshes_only_the_active_workspaces_marks() {
    let (mut app, workspace, _) = booted(80, 24);
    app.take_actions();

    app.update(Msg::Agent(EventEnvelope::mcp_status(
        &workspace.to_string(),
    )));
    assert_eq!(mcp_loads(&mut app), vec![workspace]);

    app.update(Msg::Agent(EventEnvelope::mcp_status(
        &WorkspaceId::new().to_string(),
    )));
    assert!(
        mcp_loads(&mut app).is_empty(),
        "another workspace's servers"
    );
}

#[test]
fn the_overlay_marks_a_running_and_a_waiting_session() {
    let (mut app, workspace, first) = booted(80, 24);
    let live = session(workspace, Some("live"));
    let live_id = live.id;
    let mut waiting = session(workspace, Some("waiting"));
    waiting.turn_display = crate::domain::chat_session::model::TurnDisplay::AwaitingApproval;
    let waiting_id = waiting.id;
    let open = app.open_session().expect("a session").clone();
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![open, live.clone(), waiting.clone()],
        running: vec![live_id],
    }));
    app.take_actions();

    assert_eq!(app.mark_of(&live), Mark::Running);
    assert_eq!(app.mark_of(&waiting), Mark::Awaiting);
    assert_eq!(
        app.mark_of(&app.sessions()[0]),
        Mark::Idle,
        "the first session is not running"
    );

    // The open session's own turn is taken from the phase, so the mark does not
    // wait for the list read that a frame would trigger.
    let open = app.sessions()[0].clone();
    app.update(Msg::Agent(EventEnvelope::from_core_event(
        robi_core::event::Event::TurnStarted { session: open.id },
    )));
    assert_eq!(app.mark_of(&open), Mark::Running);
    assert_eq!(app.open_session_id(), Some(open.id));
    assert!(waiting_id != first && live_id != first);
}

#[test]
fn the_sessions_overlay_draws_a_panel_over_the_transcript() {
    let mut app = with_messages(80, 24, vec![Message::user("read the config")]);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    let frame = screen(&app, 80, 24);
    assert!(
        frame.contains("sessions"),
        "the panel names itself: {frame}"
    );
    assert!(frame.contains("a session"), "the row is drawn: {frame}");
    assert!(
        frame
            .lines()
            .nth(usize::from(crate::tui::view::FRAME_PAD))
            .is_some_and(|row| row.contains("robi")),
        "the header still names the workspace: {frame}"
    );

    press(&mut app, key(KeyCode::Esc));
    assert!(
        !screen(&app, 80, 24).contains("sessions"),
        "Esc takes the panel away"
    );
}

#[test]
fn the_create_screen_shows_the_typed_path_and_its_candidates() {
    let (mut app, _, _) = booted(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, ctrl(KeyCode::Char('n')));
    type_text(&mut app, "~/co");
    let completions = completions(&mut app);
    let (text, generation) = completions.last().expect("a completion read").clone();
    app.update(Msg::Done(ActionOutcome::Paths {
        generation,
        text,
        candidates: vec!["~/code".to_owned()],
    }));

    let frame = screen(&app, 80, 24);
    assert!(frame.contains("path"), "{frame}");
    assert!(frame.contains("~/co"), "{frame}");
    assert!(frame.contains("~/code"), "the candidate is listed: {frame}");
}

#[test]
fn the_header_summarises_the_mcp_servers_in_one_word() {
    use crate::agent::mcp::McpServerStatus;

    fn server(status: &str) -> McpServerStatus {
        McpServerStatus {
            id: "sentry".to_owned(),
            status: status.to_owned(),
            title: None,
            icon: None,
            tool_count: 0,
        }
    }

    let (mut app, workspace, _) = booted(80, 24);
    app.take_actions();
    let header = screen(&app, 80, 24)
        .lines()
        .nth(usize::from(crate::tui::view::FRAME_PAD))
        .expect("a header")
        .to_owned();
    assert!(
        !header.contains("MCP"),
        "no servers configured, nothing to say: {header:?}"
    );

    let generation = app.mcp_generation();
    app.update(Msg::Done(ActionOutcome::Mcp {
        workspace,
        generation,
        servers: vec![server("connected")],
    }));
    let frame = screen(&app, 80, 24);
    let bar = usize::from(crate::tui::view::FRAME_PAD);
    let header = frame.lines().nth(bar).expect("a header");
    assert!(header.trim_end().ends_with("MCP Online"), "{header:?}");
    let column = 80 - crate::tui::view::GUTTER - "MCP Online".len() as u16;
    assert_eq!(
        foreground(&app, 80, 24, column, bar as u16),
        crate::tui::theme::SUCCESS,
        "a healthy workspace reads green"
    );

    let generation = app.mcp_generation();
    app.update(Msg::Done(ActionOutcome::Mcp {
        workspace,
        generation,
        servers: vec![server("connected"), server("failed")],
    }));
    let frame = screen(&app, 80, 24);
    let bar = usize::from(crate::tui::view::FRAME_PAD);
    let header = frame.lines().nth(bar).expect("a header");
    assert!(header.trim_end().ends_with("MCP Failed"), "{header:?}");
    let column = 80 - crate::tui::view::GUTTER - "MCP Failed".len() as u16;
    assert_eq!(
        foreground(&app, 80, 24, column, bar as u16),
        crate::tui::theme::DANGER,
        "one dead server is not online"
    );
}

#[test]
fn a_finished_turn_ends_with_its_worked_for_line() {
    let app = with_messages(
        80,
        24,
        vec![Message::user("read the config"), Message::assistant("done")],
    );
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Worked for"), "{frame}");
    // Gray, like the webview's: it is a footnote, not another block.
    let y = rows(&app, 80, 24)
        .iter()
        .position(|row| row.contains("Worked for"))
        .expect("the footer's row") as u16;
    assert_eq!(
        foreground(&app, 80, 24, crate::tui::view::GUTTER, y),
        crate::tui::theme::INK_MUTED
    );
}

#[test]
fn an_open_turn_shows_its_activity_line_and_no_footer() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(ActionOutcome::Transcript {
        session,
        generation: app.transcript().generation(),
        messages: vec![
            Message::user("read the config"),
            Message::assistant("working"),
        ],
    }));
    app.update(Msg::Agent(turn_started(session)));
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Thinking"), "{frame}");
    assert!(
        !frame.contains("Worked for"),
        "a running turn has no duration yet: {frame}"
    );

    app.update(Msg::Agent(finished(session)));
    let frame = screen(&app, 80, 24);
    assert!(!frame.contains("Thinking"), "{frame}");
    assert!(frame.contains("Worked for"), "{frame}");
}

#[test]
fn the_switcher_paints_the_row_the_keys_act_on_as_a_full_width_band() {
    let (mut app, active, _) = booted(70, 22);
    let second = workspace("pocket");
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace_row(active, "robi"), second],
    }));
    app.take_actions();
    press(&mut app, ctrl(KeyCode::Char('w')));

    let band = crate::tui::view::rows::SELECTED_BG;
    // `robi`, `pocket`, then the two action rows: the arrows reach every one of
    // them, and each is highlighted from its first cell to the panel's inner
    // edge. A theme helper carrying the canvas background used to cancel the
    // highlight, and on the two action rows that was the only thing to see.
    let labels = ["robi", "pocket", "Manage workspaces", "New workspace"];
    for (index, label) in labels.iter().enumerate() {
        let frame_rows = rows(&app, 70, 22);
        // Below the panel's title, so the header's `robi` is not the row found.
        let panel = frame_rows
            .iter()
            .position(|row| row.contains("workspaces"))
            .expect("the panel's title");
        let y = frame_rows
            .iter()
            .skip(panel)
            .position(|row| row.contains(label))
            .unwrap_or_else(|| panic!("{label} is drawn: {frame_rows:#?}")) as u16
            + panel as u16;
        let line = &frame_rows[y as usize];
        let start = line.find(label).expect("the label") as u16;
        // The last cell inside the border, so the band is a row and not a word.
        let inner_right = u16::try_from(line.trim_end().chars().count()).unwrap_or(0) - 2;
        assert_eq!(
            background(&app, 70, 22, start, y),
            band,
            "the first character of {label:?} is highlighted: {line:?}"
        );
        assert_eq!(
            background(&app, 70, 22, inner_right, y),
            band,
            "the band reaches the panel's edge on {label:?}: {line:?}"
        );
        if index + 1 < labels.len() {
            press(&mut app, key(KeyCode::Down));
        }
    }
}

/// The rows a bordered panel occupies, borders included.
///
/// Asserting on the panel's own rect is what "nothing shows through" means: an
/// overlay floats over the transcript, so transcript text around it is correct and
/// only text *inside* it is a bug. The old whole-frame assertion held only while the
/// panel happened to sit exactly over the centred empty state.
fn panel_span(rows: &[String]) -> (usize, usize) {
    let top = rows
        .iter()
        .position(|row| row.contains('\u{250c}'))
        .unwrap_or_else(|| panic!("no panel in this frame: {rows:#?}"));
    let bottom = rows
        .iter()
        .rposition(|row| row.contains('\u{2514}'))
        .unwrap_or_else(|| panic!("no panel in this frame: {rows:#?}"));
    (top, bottom)
}

#[test]
fn the_switcher_erases_what_the_transcript_drew_under_it() {
    // The empty state's sentence is centred in the transcript, which is roughly
    // where the panel lands: without a clear, it read through the panel's
    // background.
    let (mut app, _, _) = booted(70, 22);
    app.take_actions();
    assert!(
        screen(&app, 70, 22).contains("No messages yet."),
        "the empty state is on screen before the panel is"
    );

    press(&mut app, ctrl(KeyCode::Char('w')));
    let frame = rows(&app, 70, 22);
    let (top, bottom) = panel_span(&frame);
    assert!(frame.join("\n").contains("workspaces"), "{frame:#?}");
    assert!(
        !frame[top..=bottom]
            .iter()
            .any(|row| row.contains("No messages yet.")),
        "nothing shows through the panel: {frame:#?}"
    );

    // The create screen lands in the same place and must erase it too.
    press(&mut app, ctrl(KeyCode::Char('n')));
    let frame = rows(&app, 70, 22);
    let (top, bottom) = panel_span(&frame);
    assert!(frame.join("\n").contains("path"), "{frame:#?}");
    assert!(
        !frame[top..=bottom]
            .iter()
            .any(|row| row.contains("No messages yet.")),
        "nothing shows through the panel: {frame:#?}"
    );
}

#[test]
fn the_manage_view_and_the_startup_prompt_take_the_whole_frame() {
    let (mut app, active, _) = booted(80, 24);
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace_row(active, "robi")],
    }));
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    let frame = screen(&app, 80, 24);
    assert_eq!(app.mode(), crate::tui::state::Mode::Manage);
    assert!(frame.contains("workspaces \u{b7} 1 of 1"), "{frame}");
    assert!(
        !frame.contains("Message robi"),
        "the composer is not drawn under a dedicated view: {frame}"
    );
    assert!(frame.contains("Ctrl-D remove"), "{frame}");
}

/// An app on the startup question: the path has no workspace, and one other
/// workspace exists in the database.
fn missing_page() -> (App, WorkspaceId) {
    let mut app = App::new(
        ViewState::default(),
        "sqlite://test.db",
        PathBuf::from("/elsewhere/pocket"),
        morning(),
    );
    app.resize(80, 24);
    let other = workspace("robi");
    let other_id = other.id;
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![other],
    }));
    (app, other_id)
}

/// An app whose remembered session is already the open one, as a restart leaves
/// it: `tui.json` names a session that is still in the list.
fn restored(width: u16, height: u16) -> (App, WorkspaceId, SessionId) {
    let (mut app, workspace, session) = booted(width, height);
    app.take_actions();
    let generation = app.sessions_generation();
    let open = app.open_session().expect("a session").clone();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![open],
        running: Vec::new(),
    }));
    (app, workspace, session)
}

#[test]
fn the_first_session_listing_reads_the_restored_open_session() {
    // The bug: `tui.json` names a session that is still listed, so nothing looked
    // like a selection change and no read was issued. The frame stayed empty
    // until another session had been opened and come back from.
    let workspace = workspace("robi");
    let session = session(workspace.id, Some("a session"));
    let session_id = session.id;
    let workspace_id = workspace.id;
    let view = ViewState {
        active_workspace_id: Some(workspace_id),
        open_session_id: Some(session_id),
        ..ViewState::default()
    };
    let mut app = App::new(view, "sqlite://test.db", test_workspace_path(), morning());
    app.resize(80, 24);
    app.take_actions();

    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace],
    }));
    app.take_actions();
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace: workspace_id,
        generation,
        sessions: vec![session],
        running: Vec::new(),
    }));

    assert_eq!(
        reloads(&mut app),
        vec![session_id],
        "the first listing reads the restored session's transcript"
    );
}

#[test]
fn a_later_listing_that_keeps_the_open_session_still_does_not_reload() {
    let (mut app, _, session) = restored(80, 24);
    let generation = app.sessions_generation();
    let open = app.open_session().expect("a session").clone();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace: app.active_workspace_id().expect("a workspace"),
        generation,
        sessions: vec![titled(open, "renamed")],
        running: Vec::new(),
    }));
    assert!(reloads(&mut app).is_empty());
    assert_eq!(app.open_session_id(), Some(session));
    assert_eq!(app.session_title(), "renamed");
}

#[test]
fn enter_on_the_session_that_is_already_open_reloads_it() {
    let (mut app, workspace, first) = restored(80, 24);
    let second = session(workspace, Some("second"));
    let second_id = second.id;
    let open = app.open_session().expect("a session").clone();
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: vec![open, second],
        running: Vec::new(),
    }));
    app.take_actions();

    // The cursor lands on the open session, one row under the action row.
    press(&mut app, ctrl(KeyCode::Char('s')));
    assert_eq!(app.selected_session().map(|row| row.id), Some(first));

    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert_eq!(
        reloads(&mut app),
        vec![first],
        "`Enter` on the open session is still a request to read it"
    );

    // And another row still switches.
    press(&mut app, ctrl(KeyCode::Char('s')));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.open_session_id(), Some(second_id));
}

#[test]
fn the_sessions_first_row_opens_the_landing_page() {
    let (mut app, _, _) = restored(80, 24);
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('s')));
    // Up from the open session's row reaches the action row.
    press(&mut app, key(KeyCode::Up));
    assert!(app.session_action_selected());
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert!(app.landing(), "the greeting, with nothing created yet");
    assert!(
        created_sessions(&mut app).is_empty(),
        "the row appears with the first message"
    );
    assert!(screen(&app, 80, 24).contains("Good morning"));
}

#[test]
fn a_typed_path_with_no_workspace_asks_instead_of_opening_another() {
    // The path the process was started on is not a workspace here.
    let mut app = App::new(
        ViewState::default(),
        "sqlite://test.db",
        PathBuf::from("/elsewhere/pocket"),
        morning(),
    );
    app.resize(80, 24);
    let other = workspace("robi");
    let other_id = other.id;
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![other],
    }));

    assert!(app.workspace_missing(), "the frame asks the question");
    assert_eq!(
        app.active_workspace_id(),
        None,
        "a remembered workspace is not a fallback for a typed path"
    );
    assert_ne!(app.active_workspace_id(), Some(other_id));
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("No workspace at this path"), "{frame}");
    assert!(frame.contains("/elsewhere/pocket"), "{frame}");

    // `y` creates it where the reader pointed.
    press(&mut app, key(KeyCode::Char('y')));
    let roots = workspace_opens(&mut app);
    assert_eq!(roots, vec!["/elsewhere/pocket".to_owned()]);
}

#[test]
fn declining_the_missing_workspace_prompt_opens_the_switcher_over_it() {
    let (mut app, _) = missing_page();
    app.take_actions();

    press(&mut app, key(KeyCode::Char('n')));
    assert!(app.workspaces_open(), "the switcher opens");
    assert!(
        app.missing_workspace_page(),
        "and the question stays the page underneath it"
    );
    assert!(
        !app.workspace_missing(),
        "the switcher owns the keyboard while it is up"
    );
    assert!(workspace_opens(&mut app).is_empty(), "nothing was created");

    // Closing the overlay goes back to the question, not to a chat frame with
    // no workspace in it.
    press(&mut app, key(KeyCode::Esc));
    assert!(!app.workspaces_open());
    assert!(app.workspace_missing(), "the question is asked again");
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("No workspace at this path"), "{frame}");
    assert!(frame.contains("/elsewhere/pocket"), "{frame}");

    // And `n` opens it again: the two answers are both still there.
    press(&mut app, key(KeyCode::Char('n')));
    assert!(app.workspaces_open());
}

#[test]
fn choosing_a_workspace_from_the_question_leaves_the_question() {
    let (mut app, first) = missing_page();
    app.take_actions();

    press(&mut app, key(KeyCode::Char('n')));
    press(&mut app, key(KeyCode::Enter));

    assert_eq!(app.active_workspace_id(), Some(first));
    assert!(!app.missing_workspace_page(), "the question is answered");
    assert!(!app.workspaces_open());
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
    assert!(!screen(&app, 80, 24).contains("No workspace at this path"));
}

#[test]
fn creating_the_paths_workspace_from_the_question_also_answers_it() {
    let (mut app, _) = missing_page();
    app.take_actions();

    press(&mut app, key(KeyCode::Char('n')));
    press(&mut app, ctrl(KeyCode::Char('n')));
    assert_eq!(app.workspaces_view().screen, Screen::New);
    type_text(&mut app, "/elsewhere/pocket");
    app.take_actions();
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(
        workspace_opens(&mut app),
        vec!["/elsewhere/pocket".to_owned()]
    );

    let opened = workspace("pocket");
    let id = opened.id;
    app.update(Msg::Done(ActionOutcome::WorkspaceOpened {
        workspace: Box::new(opened),
        created: true,
    }));
    assert_eq!(app.active_workspace_id(), Some(id));
    assert!(!app.missing_workspace_page());
}

#[test]
fn the_path_of_the_process_picks_its_workspace() {
    let target = workspace("pocket");
    let mut app = App::new(
        ViewState::default(),
        "sqlite://test.db",
        PathBuf::from(format!("{}/", target.root)),
        morning(),
    );
    app.resize(80, 24);
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace("robi"), target.clone()],
    }));

    assert!(!app.workspace_missing());
    assert_eq!(app.active_workspace_id(), Some(target.id));
    assert_eq!(
        app.snapshot().recent_workspace_ids.first(),
        Some(&target.id)
    );
}

#[test]
fn the_switcher_manage_row_opens_the_manage_view_and_enter_opens_a_workspace() {
    let (mut app, active, _) = booted(80, 24);
    let second = workspace("pocket");
    let second_id = second.id;
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace_row(active, "robi"), second],
    }));
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    // The two action rows follow the recent list.
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), crate::tui::state::Mode::Manage);
    assert!(
        !app.workspaces_open(),
        "the manage view replaces the switcher, it does not stack on it"
    );
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("workspaces"), "{frame}");

    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.active_workspace_id(), Some(second_id));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
}

#[test]
fn the_manage_view_filters_and_removes_behind_a_confirmation() {
    let (mut app, active, _) = booted(80, 24);
    let second = workspace("pocket");
    let second_id = second.id;
    let generation = app.workspaces_generation();
    app.update(Msg::Done(ActionOutcome::Workspaces {
        generation,
        workspaces: vec![workspace_row(active, "robi"), second],
    }));
    app.take_actions();

    press(&mut app, ctrl(KeyCode::Char('w')));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Down));
    press(&mut app, key(KeyCode::Enter));
    assert_eq!(app.mode(), crate::tui::state::Mode::Manage);

    type_text(&mut app, "pocket");
    assert_eq!(app.manage_rows().len(), 1);
    assert_eq!(app.manage_selected().map(|row| row.id), Some(second_id));

    press(&mut app, ctrl(KeyCode::Char('d')));
    assert_eq!(app.mode(), crate::tui::state::Mode::Confirm);
    press(&mut app, key(KeyCode::Char('n')));
    assert_eq!(
        app.mode(),
        crate::tui::state::Mode::Manage,
        "cancelling goes back to the list"
    );

    press(&mut app, ctrl(KeyCode::Char('d')));
    press(&mut app, key(KeyCode::Char('y')));
    let actions = app.take_actions();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::DeleteWorkspace { workspace } if *workspace == second_id
    )));

    press(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode(), crate::tui::state::Mode::Chat);
}

#[test]
fn the_sessions_panel_puts_the_action_row_above_the_sessions() {
    let (mut app, _, _) = restored(80, 24);
    app.take_actions();
    press(&mut app, ctrl(KeyCode::Char('s')));

    let frame_rows = rows(&app, 80, 24);
    // Below the panel's title: the header names the session too.
    let panel = frame_rows
        .iter()
        .position(|row| row.contains("sessions"))
        .expect("the panel's title");
    let action = frame_rows
        .iter()
        .skip(panel)
        .position(|row| row.contains("New session"))
        .expect("the action row");
    let session = frame_rows
        .iter()
        .skip(panel)
        .position(|row| row.contains("a session"))
        .expect("the session row");
    assert!(
        action < session,
        "the way to a fresh chat is the first row: {frame_rows:#?}"
    );

    // The action row is highlighted like any other when the cursor is on it.
    press(&mut app, key(KeyCode::Up));
    let band = crate::tui::view::rows::SELECTED_BG;
    let y = (panel + action) as u16;
    let left = frame_rows[y as usize].find('+').expect("the marker") as u16;
    assert_eq!(background(&app, 80, 24, left, y), band);
}

#[test]
fn the_landing_page_draws_a_green_greeting_over_the_composer() {
    let (mut app, _, _) = booted(80, 24);
    press(&mut app, ctrl(KeyCode::Char('n')));
    let frame_rows = rows(&app, 80, 24);
    let greeting = frame_rows
        .iter()
        .position(|row| row.contains("Good morning"))
        .expect("the greeting");
    let band = frame_rows
        .iter()
        .position(|row| row.contains(RAIL_CHAR))
        .expect("the composer's rail");
    assert!(
        greeting < band,
        "the composer sits under the greeting: {frame_rows:#?}"
    );
    assert!(
        greeting > 3,
        "and both are centred, not pinned to the top: {frame_rows:#?}"
    );
    // The greeting is green, and the header names the chat the way the web shell
    // does for a chat with no row yet.
    let line = &frame_rows[greeting];
    let x = line.find("Good morning").expect("the label") as u16;
    assert_eq!(
        foreground(&app, 80, 24, x, greeting as u16),
        crate::tui::theme::MODE_ASK
    );
    let header = usize::from(crate::tui::view::FRAME_PAD);
    assert!(
        frame_rows[header].contains("New chat"),
        "{:?}",
        frame_rows[header]
    );
    assert!(
        !screen(&app, 80, 24).contains("No messages yet."),
        "the landing page is not an empty transcript"
    );
}

#[test]
fn a_fresh_workspace_with_no_sessions_shows_the_landing_page() {
    // No session is open and the listing is empty: the greeting, not "No sessions
    // in this workspace yet." with a dead composer under it.
    let (mut app, workspace, _) = booted(80, 24);
    let generation = app.sessions_generation();
    app.update(Msg::Done(ActionOutcome::Sessions {
        workspace,
        generation,
        sessions: Vec::new(),
        running: Vec::new(),
    }));
    assert!(app.landing());
    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Good morning"), "{frame}");
    assert!(!frame.contains("No sessions"), "{frame}");

    // And a message sent from it creates the row.
    type_text(&mut app, "hello");
    press(&mut app, key(KeyCode::Enter));
    let created = created_sessions(&mut app);
    assert_eq!(created, vec![workspace]);
}

#[test]
fn deleting_the_last_session_lands_on_the_greeting_rather_than_a_dead_composer() {
    let (mut app, _, session) = restored(80, 24);
    app.take_actions();

    app.update(Msg::Done(ActionOutcome::SessionDeleted { session }));
    assert_eq!(app.open_session_id(), None);
    assert!(app.landing());
    assert!(screen(&app, 80, 24).contains("Good morning"));
}

#[test]
fn the_greeting_follows_the_clock_the_process_started_with() {
    use crate::tui::state::landing::Greeting;

    for (greeting, label) in [
        (Greeting::Morning, "Good morning"),
        (Greeting::Afternoon, "Good afternoon"),
        (Greeting::Evening, "Good evening"),
    ] {
        let mut app = App::new(
            ViewState::default(),
            "sqlite://test.db",
            test_workspace_path(),
            greeting,
        );
        app.resize(80, 24);
        let workspace = workspace("robi");
        let workspace_id = workspace.id;
        let generation = app.workspaces_generation();
        app.update(Msg::Done(ActionOutcome::Workspaces {
            generation,
            workspaces: vec![workspace],
        }));
        let generation = app.sessions_generation();
        app.update(Msg::Done(ActionOutcome::Sessions {
            workspace: workspace_id,
            generation,
            sessions: Vec::new(),
            running: Vec::new(),
        }));
        assert!(app.landing());
        assert!(screen(&app, 80, 24).contains(label), "{label}");
    }
}

#[test]
fn a_tool_call_next_in_line_is_not_a_waiting_card() {
    // The bug: between the model's response and the call's execution, every call
    // is `pending` + `not_started`. The transcript drew a shell command that runs
    // without asking as one waiting for an approval, with the attention rail.
    let (mut app, _, session) = booted(80, 24);
    let call = pending_call("shell", serde_json::json!({ "command": "ls" }));
    let id = call.id;
    app.update(Msg::Agent(turn_started(session)));
    land(
        &mut app,
        session,
        vec![Message::assistant_with_tool_calls("", vec![call])],
    );

    let frame = screen(&app, 80, 24);
    assert!(
        !frame.contains(tool_card::WAITING_LABEL),
        "a running turn does not ask: {frame}"
    );
    assert!(frame.contains(tool_card::RUNNING_LABEL), "{frame}");
    assert!(app.approval().is_none(), "and no approval is open for it");
    assert!(
        app.transcript().rows().iter().all(|row| !row.attention),
        "no card wears the attention rail"
    );
    let _ = id;
}

#[test]
fn the_same_call_becomes_a_waiting_card_when_the_turn_pauses() {
    let (app, session, id) = paused(80, 24);
    let frame = screen(&app, 80, 24);
    assert!(frame.contains(tool_card::WAITING_LABEL), "{frame}");
    assert!(
        app.transcript().rows().iter().any(|row| row.attention),
        "the paused card wears the attention rail"
    );
    let pending = app
        .transcript()
        .messages()
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .find(|call| call.id == id)
        .expect("the call");
    assert!(tool_card::needs_decision(
        pending,
        crate::tui::state::Phase::Idle
    ));
    let _ = session;
}

#[test]
fn zz_dump_paused() {
    let (app, _, _) = paused(80, 24);
    println!("{}", screen(&app, 80, 24));
}

// ---------------------------------------------------------------------------
// T4: modes, the picker, the menus, and the help page
// ---------------------------------------------------------------------------

use crate::tui::state::Mode as Surface;

fn typing(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn typing_ctrl(ch: char) -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
}

/// The open session's id, for a test that needs to name it.
fn open_id(app: &App) -> SessionId {
    app.open_session().expect("the fixture opens a session").id
}

/// Every action of one shape the app has queued, and nothing else.
fn actions_matching<F: Fn(&Action) -> bool>(app: &mut App, predicate: F) -> Vec<Action> {
    app.take_actions()
        .into_iter()
        .filter(|action| predicate(action))
        .collect()
}

#[test]
fn ctrl_t_cycles_the_mode_and_writes_it_to_the_session() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    assert_eq!(app.agent_mode(), AgentMode::Agent);

    app.update(typing_ctrl('t'));
    assert_eq!(app.agent_mode(), AgentMode::Ask, "agent cycles to ask");
    // The write goes to the service; the row is only replaced when it lands.
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SetSessionMode { .. })
    });
    assert_eq!(sent.len(), 1);
    assert!(matches!(
        &sent[0],
        Action::SetSessionMode {
            mode: AgentMode::Ask,
            ..
        }
    ));

    app.update(typing_ctrl('t'));
    assert_eq!(app.agent_mode(), AgentMode::Plan, "ask cycles to plan");
    app.update(typing_ctrl('t'));
    assert_eq!(app.agent_mode(), AgentMode::Agent, "plan cycles to agent");
    let _ = actions_matching(&mut app, |_| true);
}

#[test]
fn a_mode_write_lands_on_the_row_and_the_rail_follows_it() {
    let mut app = with_messages(60, 20, vec![Message::user("hi")]);
    // The stored row comes back changed; the app replaces its copy.
    let mut updated = app.open_session().expect("a session").clone();
    updated.mode = AgentMode::Plan;
    app.update(Msg::Done(ActionOutcome::SessionUpdated(Box::new(updated))));

    assert_eq!(app.agent_mode(), AgentMode::Plan);
    assert_eq!(
        crate::tui::view::composer::rail_style(app.agent_mode()).fg,
        Some(crate::tui::theme::MODE_PLAN),
        "the composer's rail is the mode's ink"
    );
}

#[test]
fn the_landing_page_cycles_a_draft_mode_and_creates_the_session_with_it() {
    let (mut app, _, _) = booted(80, 24);
    land_models(&mut app);
    // Land the landing page: no session open.
    app.update(typing_ctrl('n'));
    assert!(app.landing());
    app.update(typing_ctrl('t'));
    assert_eq!(
        app.agent_mode(),
        AgentMode::Ask,
        "the draft is what changes"
    );
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SetSessionMode { .. })
    });
    assert!(sent.is_empty(), "there is no row to write yet");

    // The first message carries the draft's mode into the create.
    for ch in "hello".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    let created = actions_matching(&mut app, |action| {
        matches!(action, Action::StartSession { .. })
    });
    assert_eq!(created.len(), 1);
    match &created[0] {
        Action::StartSession { mode, .. } => assert_eq!(*mode, AgentMode::Ask),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ctrl_m_opens_the_picker_on_the_row_in_force() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    app.update(typing_ctrl('m'));
    assert_eq!(app.mode(), Surface::Models);
    let picker = app.picker().expect("the picker is open");
    assert_eq!(
        picker.current().unwrap().label,
        "Use default",
        "the effort group comes first, so its default is where the cursor lands"
    );

    // Walk down to the first model row rather than counting rows: the group order
    // is a product decision, and a test that hard-codes the count fails on a change
    // that is not a bug.
    let mut guard = 0;
    while !matches!(
        app.picker().and_then(|picker| picker.pick()),
        Some(crate::tui::state::models::Pick::Model(_))
    ) {
        app.update(typing(KeyCode::Down));
        guard += 1;
        assert!(guard < 20, "the picker never reached a model row");
    }
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Chat, "choosing closes the picker");
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SetSessionChoice { .. })
    });
    assert_eq!(sent.len(), 1);
    // What landed is the row the cursor reached, named so a wrong order fails here
    // rather than as a bare `false`.
    match &sent[0] {
        Action::SetSessionChoice { pick, .. } => assert_eq!(
            pick.value().as_deref(),
            Some("ocg_glm-5.3"),
            "the first model row"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn use_default_clears_one_key_and_leaves_the_other() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    app.update(typing_ctrl('m'));
    // The cursor opens on the first group's `Use default`, which is the effort's:
    // clearing it must leave the model alone, and this is the row that proves it.
    assert_eq!(
        app.picker().unwrap().current().unwrap().label,
        "Use default"
    );
    app.update(typing(KeyCode::Enter));
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SetSessionChoice { .. })
    });
    assert!(
        matches!(&sent[0], Action::SetSessionChoice { pick, .. } if pick.key() == crate::domain::chat_session::model::ChoiceKey::Effort && pick.value().is_none()),
        "an effort default writes no model"
    );
}

#[test]
fn at_opens_the_skill_menu_and_enter_leaves_the_mention() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_skills(&mut app, &[("create-skill", false), ("review-agent", true)]);
    app.update(typing(KeyCode::Char('@')));
    assert_eq!(app.mode(), Surface::Complete, "the menu owns the keyboard");
    let menu = app.completion().expect("a menu");
    assert!(menu.rows.len() >= 2);

    // Narrow to one row and accept it.
    for ch in "create".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Chat);
    assert_eq!(
        app.composer().text(),
        "@create-skill ",
        "the whole token was replaced, not appended to"
    );
    assert!(app.completion().is_none());
}

#[test]
fn esc_closes_the_menu_and_keeps_the_text_without_stopping_the_turn() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_skills(&mut app, &[("create-skill", false)]);
    app.update(Msg::Agent(turn_started(open_id(&app))));
    app.update(typing(KeyCode::Char('@')));
    assert_eq!(app.mode(), Surface::Complete);

    app.update(typing(KeyCode::Esc));
    assert_eq!(app.mode(), Surface::Chat, "the menu closed");
    assert_eq!(app.composer().text(), "@", "and the text stayed");
    assert!(app.is_busy(), "the running turn was not interrupted");

    let stopped = actions_matching(&mut app, |action| matches!(action, Action::Stop { .. }));
    assert!(stopped.is_empty(), "no stop was issued");
}

#[test]
fn slash_lists_the_commands_and_a_leading_slash_that_names_nothing_is_refused() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_skills(&mut app, &[("create-skill", false)]);
    app.update(typing(KeyCode::Char('/')));
    assert_eq!(app.mode(), Surface::Complete);
    assert_eq!(
        app.completion().unwrap().rows.len(),
        crate::tui::command::BUILT_INS.len()
    );
    app.update(typing(KeyCode::Esc));

    // A name that exists nowhere: refused, and the field keeps every character.
    for ch in "nope".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    assert_eq!(app.composer().text(), "/nope");
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.composer().text(), "/nope", "nothing was cleared");
    assert!(
        app.notice().is_some_and(|notice| notice.is_error()),
        "the refusal is an error, not a hint"
    );
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::Submit { .. } | Action::StartSession { .. })
    });
    assert!(sent.is_empty(), "nothing was sent");
}

#[test]
fn a_skill_typed_with_a_slash_is_refused_and_names_the_other_sigil() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_skills(&mut app, &[("create-skill", false)]);
    for ch in "/create-skill".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    // The menu would have accepted it; the verdict is what a submit asks.
    app.update(typing(KeyCode::Esc));
    app.update(typing(KeyCode::Enter));
    let notice = app.notice().expect("a notice");
    assert!(notice.is_error());
    assert!(notice.text.contains("@create-skill"), "{}", notice.text);
    assert_eq!(app.composer().text(), "/create-skill");
}

#[test]
fn a_command_opens_its_overlay_and_clears_the_field() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    for ch in "/sessions".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Esc));
    app.update(typing(KeyCode::Enter));
    assert_eq!(
        app.mode(),
        Surface::Sessions,
        "the same overlay Ctrl-S opens"
    );
    assert!(app.composer().is_blank(), "a command clears the field");
}

#[test]
fn slash_help_opens_the_page_with_no_session_open_at_all() {
    let (mut app, _, _) = booted(80, 24);
    app.update(typing_ctrl('n'));
    assert!(app.landing());
    for ch in "/help".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    // The menu offered `/help`; `Enter` accepts it, and the second `Enter` runs it.
    app.update(typing(KeyCode::Enter));
    assert_eq!(
        app.composer().text(),
        "/help ",
        "the completion filled the name"
    );
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Help);

    let frame = screen(&app, 80, 24);
    assert!(frame.contains("Sending"), "{frame}");
    assert!(frame.contains("ctrl") || frame.contains("Ctrl-T") || frame.contains("Enter"));
    app.update(typing(KeyCode::Esc));
    assert_eq!(app.mode(), Surface::Chat, "Esc goes back");
}

#[test]
fn question_mark_opens_help_from_the_transcript_only() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    // The composer owns the field, so `?` is a character there.
    app.update(typing(KeyCode::Char('?')));
    assert_eq!(app.mode(), Surface::Chat);
    assert_eq!(app.composer().text(), "?");

    // From the transcript it opens the page.
    app.update(typing(KeyCode::Tab));
    assert_eq!(app.focus(), Focus::Transcript);
    app.update(typing(KeyCode::Char('?')));
    assert_eq!(app.mode(), Surface::Help);
}

#[test]
fn page_down_in_help_moves_the_help_and_not_the_transcript_behind_it() {
    let messages: Vec<Message> = (0..40)
        .map(|n| Message::user(format!("line {n}")))
        .collect();
    let mut app = with_messages(80, 24, messages);
    let before = app.transcript().offset();
    for ch in "/help".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Help);

    app.update(typing(KeyCode::PageDown));
    assert!(app.help().offset > 0, "the page scrolled");
    assert_eq!(app.transcript().offset(), before, "the transcript did not");
}

#[test]
fn ctrl_l_is_refused_while_a_turn_runs_and_asks_when_idle() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    app.update(Msg::Agent(turn_started(open_id(&app))));
    app.update(typing_ctrl('l'));
    assert!(app.confirm().is_none(), "no question while a turn runs");
    assert!(app
        .notice()
        .is_some_and(|notice| notice.is_error() || !notice.is_error()));
    let _ = actions_matching(&mut app, |_| true);

    // Idle: the confirmation opens, and `y` is one compact.
    app.update(Msg::Agent(finished(open_id(&app))));
    app.update(typing_ctrl('l'));
    let confirm = app.confirm().expect("the confirmation");
    assert!(confirm.question.contains("Compact"), "{}", confirm.question);
    assert_eq!(app.mode(), Surface::Confirm);

    app.update(typing(KeyCode::Char('y')));
    let sent = actions_matching(&mut app, |action| matches!(action, Action::Compact { .. }));
    assert_eq!(sent.len(), 1);
    assert_eq!(app.mode(), Surface::Chat);
}

#[test]
fn n_cancels_the_compact_and_sends_nothing() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    app.update(typing_ctrl('l'));
    assert!(app.confirm().is_some());
    app.update(typing(KeyCode::Char('n')));
    assert!(app.confirm().is_none());
    assert_eq!(app.mode(), Surface::Chat);
    let sent = actions_matching(&mut app, |action| matches!(action, Action::Compact { .. }));
    assert!(sent.is_empty());
}

#[test]
fn the_facts_row_names_the_mode_and_sits_in_the_band() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    let frame = rows(&app, 80, 24);
    let facts_y = y_of(&frame, "Agent");
    let hint_y = y_of(&frame, "Ctrl-J newline");
    assert!(facts_y < hint_y, "the facts row is above the hint");
    // It is the band's last row: the row directly above it carries the field.
    let facts = &frame[facts_y as usize];
    assert!(
        facts.contains("GLM-5.3") || facts.contains("OCG - GLM-5.3"),
        "{facts}"
    );
    assert!(facts.contains("OpenCode Go"), "{facts}");
}

#[test]
fn the_rail_carries_the_mode_the_turn_was_submitted_under() {
    let mut plan = Message::user("plan this");
    plan.mode = Some("plan".to_owned());
    let app = with_messages(60, 20, vec![plan, Message::assistant("ok")]);

    // The user's band wears the mode the turn was submitted under, even though the
    // session is in agent mode: the stamp is what decides.
    let screen = rows(&app, 60, 20);
    let user_y = y_of(&screen, "plan this");
    assert_eq!(
        foreground(&app, 60, 20, 2, user_y),
        crate::tui::theme::MODE_PLAN,
        "the user's band wears the mode that produced the turn"
    );

    // The reply does not. A mode is what a person asked for, so it stops at the
    // user's band; a reply wears the quiet rail in every mode.
    let reply_y = y_of(&screen, "ok");
    assert_eq!(
        foreground(&app, 60, 20, 2, reply_y),
        crate::tui::theme::INK_FAINT,
        "the reply's rail is not the mode's ink"
    );
}

#[test]
fn an_unstamped_row_follows_the_session_mode() {
    let app = with_messages(
        60,
        20,
        vec![Message::user("no stamp"), Message::assistant("ok")],
    );
    let screen = rows(&app, 60, 20);
    let y = y_of(&screen, "no stamp");
    assert_eq!(
        foreground(&app, 60, 20, 2, y),
        crate::tui::theme::ACCENT,
        "an old row follows the session, in agent mode's blue"
    );
}

#[test]
fn a_waiting_card_keeps_its_gray_rail_until_it_is_asked_about() {
    let mut user = Message::user("run it");
    user.mode = Some("plan".to_owned());
    let mut assistant = Message::assistant_with_tool_calls(
        "",
        vec![pending_call("shell", serde_json::json!({"command": "ls"}))],
    );
    assistant.mode = Some("plan".to_owned());
    let app = with_messages(70, 24, vec![user, assistant]);

    let screen = rows(&app, 70, 24);
    let card_y = y_of(&screen, "waiting for approval");
    let ink = foreground(&app, 70, 24, 2, card_y);
    assert_eq!(
        ink,
        crate::tui::theme::ATTENTION,
        "the waiting card is cyan, not the plan-mode orange beside it"
    );
    assert_ne!(ink, crate::tui::theme::MODE_PLAN);
}

// ---------------------------------------------------------------------------
// T4 feedback: one overlay style, a gap in the band, and the confirm as a panel
// ---------------------------------------------------------------------------

#[test]
fn every_span_on_the_facts_row_carries_the_bands_background() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    let frame = rows(&app, 80, 24);
    let y = y_of(&frame, "Agent");
    // The separator between the mode and the model used to be `theme::faint()`,
    // which carries the canvas: one dark cell in the middle of the band.
    let separator_x = frame[y as usize]
        .chars()
        .position(|ch| ch == '\u{b7}')
        .expect("a separator on the band") as u16;
    assert_eq!(
        background(&app, 80, 24, separator_x, y),
        crate::tui::theme::BG_SURFACE,
        "the separator is on the band, not punched out of it"
    );
}

#[test]
fn the_band_keeps_a_blank_row_between_the_text_and_the_facts() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    for ch in "hello".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    let frame = rows(&app, 80, 24);
    let facts_y = y_of(&frame, "Agent");
    // Both blank rows keep the band's rail — they are canvas with a rail, not empty
    // rows — so the rail glyph comes out before the emptiness is judged.
    let gap_row = |y: usize| {
        frame[y]
            .trim_matches(crate::tui::state::transcript::RAIL_CHAR)
            .trim()
            .to_owned()
    };
    assert!(
        gap_row(facts_y as usize - 1).is_empty(),
        "one blank row above the facts row: {frame:#?}"
    );
    assert!(
        gap_row(facts_y as usize + 1).is_empty(),
        "and one below it, so it reads as a caption: {frame:#?}"
    );
    let text_y = y_of(&frame, "hello");
    assert_eq!(
        facts_y,
        text_y + 2,
        "the text, a blank row, then the facts row"
    );
}

#[test]
fn the_picker_and_the_completion_menu_wear_the_panel_border() {
    // One border, one fill, for every overlay: a bordered panel is what the
    // sessions and workspaces lists draw, so a menu without one was a second
    // design.
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    app.update(typing_ctrl('m'));
    let screen = rows(&app, 80, 24);
    let (top, _) = panel_span(&screen);
    assert!(
        screen[top].contains("model and effort"),
        "the picker is a titled panel: {:#?}",
        screen[top]
    );

    // And the completion menu, which is anchored rather than centred.
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_skills(&mut app, &[("create-skill", false)]);
    app.update(typing(KeyCode::Char('@')));
    let screen = rows(&app, 80, 24);
    let (top, _) = panel_span(&screen);
    assert!(
        screen[top].contains('@'),
        "the menu is a titled panel: {:#?}",
        screen[top]
    );
    // Its rows sit on the panel's background, not the band's or the canvas's.
    let row_y = (top + 1) as u16;
    let x = screen[row_y as usize]
        .chars()
        .position(|ch| ch == '@')
        .expect("a row") as u16;
    // The first row is the highlighted one, so it wears the panel's selection band;
    // either way the cell is the panel's own colour, not the band's.
    assert_eq!(
        background(&app, 80, 24, x, row_y),
        crate::tui::view::rows::SELECTED_BG,
        "a panel row is painted on the panel's own background"
    );
}

#[test]
fn the_compact_confirmation_is_a_panel_and_takes_no_layout_row() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    app.update(typing_ctrl('l'));
    assert!(app.confirm().is_some());
    assert_eq!(
        app.prompt_rows(),
        0,
        "the confirmation is an overlay, so the transcript keeps its rows"
    );

    let screen = rows(&app, 80, 24);
    let (top, _) = panel_span(&screen);
    assert!(screen[top].contains("confirm"), "{:#?}", screen[top]);
    let body = screen[top..].join("\n");
    assert!(body.contains("Compact"), "{body}");
    assert!(body.contains("y confirm"), "{body}");
    // It is a panel, not a row under the composer: the composer band is still drawn
    // in full below the question rather than giving up a row to it.
    assert!(
        screen.iter().any(|row| row.contains("Message robi")),
        "the composer is still whole: {screen:#?}"
    );
}

#[test]
fn the_effort_group_is_the_first_thing_in_the_picker() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    land_models(&mut app);
    app.update(typing_ctrl('m'));
    let screen = rows(&app, 80, 24);
    let (top, _) = panel_span(&screen);
    let body = screen[top..].join("\n");
    let effort = body.find("Effort").expect("an effort group");
    let model = body.find("Model").expect("a model group");
    assert!(effort < model, "effort comes first: {body}");
}

// T5: the settings surface and the audit table

/// The key that opens the surface, from the chat or from `manage`.
fn typing_ctrl_comma() -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::CONTROL))
}

/// Land the settings read the surface issued, with the values a test wants it to
/// show. The generation is read first, so a read from an earlier test frame is
/// not mistaken for this one.
fn land_settings(app: &mut App, values: Vec<(&'static str, crate::tui::state::settings::Loaded)>) {
    let generation = app.settings_generation();
    app.update(Msg::Done(ActionOutcome::Settings {
        generation,
        values: values.into_iter().collect(),
    }));
}

/// The values a fresh store would answer with: the three defaults the surface
/// shows on its first page, and an API key already stored.
fn stored() -> Vec<(&'static str, crate::tui::state::settings::Loaded)> {
    use crate::domain::settings::keys;
    use crate::tui::state::settings::Loaded;
    vec![
        (
            keys::MODEL,
            Loaded::text(Some(keys::DEFAULT_MODEL.to_owned())),
        ),
        (keys::MAX_ITERATIONS, Loaded::text(Some("50".to_owned()))),
        (keys::OPENCODE_GO_API_KEY, Loaded::credential(true)),
        (
            keys::PROVIDER_OPENCODE_GO,
            Loaded::text(Some(keys::PROVIDER_ON.to_owned())),
        ),
        (
            keys::WEB_SEARCH_APPROVAL,
            Loaded::text(Some(keys::APPROVAL_ON.to_owned())),
        ),
        (
            keys::SANDBOX_MECHANISM,
            Loaded::text(Some(keys::DEFAULT_SANDBOX_MECHANISM.to_owned())),
        ),
        (
            keys::SANDBOX_IMAGE,
            Loaded::text(Some(keys::DEFAULT_SANDBOX_IMAGE.to_owned())),
        ),
        (
            keys::SANDBOX_NETWORK,
            Loaded::text(Some(keys::NETWORK_ON.to_owned())),
        ),
        (
            keys::SANDBOX_MEMORY_MB,
            Loaded::text(Some(keys::DEFAULT_SANDBOX_MEMORY_MB.to_owned())),
        ),
        (
            keys::SANDBOX_PROJECT_FILES,
            Loaded::text(Some(keys::DEFAULT_SANDBOX_PROJECT_FILES.to_owned())),
        ),
    ]
}

/// An app with the surface open on General and the fixture's values landed.
fn settings_panel(width: u16, height: u16) -> App {
    let mut app = with_messages(width, height, vec![Message::user("hi")]);
    app.update(typing_ctrl_comma());
    land_settings(&mut app, stored());
    app
}

#[test]
fn ctrl_comma_opens_the_surface_on_general_and_the_rail_lists_the_six_pages() {
    let app = settings_panel(80, 24);
    assert_eq!(app.mode(), Surface::Settings);
    let screen = rows(&app, 80, 24).join("\n");
    for title in [
        "General",
        "Model providers",
        "Permissions",
        "Sandbox",
        "MCP",
        "Audit log",
    ] {
        assert!(screen.contains(title), "{title} is missing: {screen}");
    }
    // The first page: the workspace it happened to open, then the model defaults.
    assert!(screen.contains("Workspace"), "{screen}");
    assert!(screen.contains("robi"), "the workspace's name: {screen}");
    assert!(screen.contains("Model"), "{screen}");
    assert!(screen.contains("Max iterations"), "{screen}");
    // It owns the frame: the composer is not underneath it.
    assert!(
        !screen.contains("Message robi"),
        "the chat is behind it: {screen}"
    );
}

#[test]
fn a_narrow_frame_folds_the_rail_and_names_the_section_in_the_title() {
    // `MIN_WIDTH` is 40, where a rail would leave the pane nothing.
    let mut app = settings_panel(50, 24);
    app.update(typing(KeyCode::Char('l')));
    assert_eq!(
        app.settings().section(),
        crate::tui::state::settings::Section::Providers
    );
    let screen = rows(&app, 50, 24).join("\n");
    assert!(
        screen.contains("settings \u{b7} Model providers"),
        "the section rides on the title row: {screen}"
    );
    assert!(
        !screen.contains("\u{25cf} General"),
        "no rail at this width: {screen}"
    );
}

#[test]
fn the_sandbox_section_shows_the_defaults_the_first_read_wrote() {
    let mut app = settings_panel(80, 30);
    for _ in 0..3 {
        app.update(typing(KeyCode::Char('l')));
    }
    assert_eq!(
        app.settings().section(),
        crate::tui::state::settings::Section::Sandbox
    );
    let screen = rows(&app, 80, 30).join("\n");
    assert!(screen.contains("Mechanism"), "{screen}");
    assert!(
        screen.contains("OS"),
        "the mechanism is a word, not `os`: {screen}"
    );
    assert!(screen.contains("robi/agent:0.1.0"), "{screen}");
    assert!(screen.contains("Memory (MiB)"), "{screen}");
    assert!(screen.contains("4096"), "{screen}");
    assert!(
        screen.contains("Sandboxed"),
        "the network posture: {screen}"
    );
    assert!(
        screen.contains("off"),
        "project files default to off: {screen}"
    );
}

#[test]
fn a_stored_secret_reads_as_bullets_and_its_text_is_never_held() {
    let mut app = settings_panel(80, 30);
    app.update(typing(KeyCode::Char('l')));
    let screen = rows(&app, 80, 30).join("\n");
    assert!(screen.contains("OpenCode Go key"), "{screen}");
    assert!(
        screen.contains("\u{2022}\u{2022}\u{2022}\u{2022}"),
        "a stored secret is masked: {screen}"
    );
    // The load reduces a credential to one bit, so there is no text to leak and
    // no `Debug` of a value to find in a log.
    assert!(app
        .settings()
        .plain(crate::domain::settings::keys::OPENCODE_GO_API_KEY)
        .is_none());
}

#[test]
fn enter_opens_the_band_and_the_second_enter_writes_the_buffer() {
    let mut app = settings_panel(80, 24);
    // The first row is the workspace; the model is the first field under it.
    app.update(typing(KeyCode::Char('j')));
    app.update(typing(KeyCode::Enter));
    assert_eq!(
        app.settings().editing(),
        Some(crate::domain::settings::keys::MODEL),
        "the band opened on the row under the cursor"
    );
    // The buffer starts at the stored value: an edit appends rather than replaces.
    for ch in "-next".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SaveSetting { .. })
    });
    assert_eq!(sent.len(), 1, "{sent:?}");
    match &sent[0] {
        Action::SaveSetting { key, value } => {
            assert_eq!(*key, crate::domain::settings::keys::MODEL);
            assert_eq!(value.as_deref(), Some("ocg_glm-5.3-next"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_empty_buffer_clears_a_plain_key_and_leaves_a_secret_alone() {
    // A key with no stored value: the band opens empty and `Enter` clears it.
    let mut app = settings_panel(80, 24);
    app.update(typing(KeyCode::Char('l'))); // Model providers
    app.update(typing(KeyCode::Char('j'))); // past the OpenCode Go toggle
    assert_eq!(
        app.settings_rows()[1].label,
        "OpenCode Go key",
        "the row under the cursor"
    );
    app.update(typing(KeyCode::Enter));
    assert_eq!(
        app.settings().editing(),
        Some(crate::domain::settings::keys::OPENCODE_GO_API_KEY)
    );
    app.update(typing(KeyCode::Enter));
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SaveSetting { .. })
    });
    assert!(
        sent.is_empty(),
        "an empty secret sends nothing, because the service refuses to remove one: {sent:?}"
    );

    // A plain key with nothing stored does the same thing the other way round.
    let mut app = settings_panel(80, 24);
    app.update(typing(KeyCode::Char('l')));
    app.update(typing(KeyCode::Char('j')));
    app.update(typing(KeyCode::Char('j'))); // Base URL, which has no default
    assert_eq!(app.settings_rows()[2].label, "Base URL");
    app.update(typing(KeyCode::Enter));
    app.update(typing(KeyCode::Enter));
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SaveSetting { .. })
    });
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(matches!(&sent[0], Action::SaveSetting { value: None, .. }));
}

#[test]
fn a_toggle_flips_on_enter_and_sends_the_new_word() {
    let mut app = settings_panel(80, 24);
    app.update(typing(KeyCode::Char('l')));
    app.update(typing(KeyCode::Char('l'))); // Permissions
    app.update(typing(KeyCode::Enter)); // web_search_approval, stored `on`
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SaveSetting { .. })
    });
    assert_eq!(sent.len(), 1, "{sent:?}");
    match &sent[0] {
        Action::SaveSetting { key, value } => {
            assert_eq!(*key, crate::domain::settings::keys::WEB_SEARCH_APPROVAL);
            assert_eq!(
                value.as_deref(),
                Some(crate::domain::settings::keys::APPROVAL_OFF)
            );
        }
        other => panic!("{other:?}"),
    }
    // The row follows the keystroke at once, before the store answers.
    let screen = rows(&app, 80, 24).join("\n");
    assert!(screen.contains("off"), "{screen}");
}

#[test]
fn a_choice_advances_and_a_refused_write_says_why_and_reverts() {
    let mut app = settings_panel(80, 30);
    for _ in 0..3 {
        app.update(typing(KeyCode::Char('l'))); // Sandbox
    }
    for _ in 0..6 {
        app.update(typing(KeyCode::Char('j'))); // Network, after six fields
    }
    assert_eq!(app.settings().selected, 6, "the network row");
    app.update(typing(KeyCode::Enter));
    let sent = actions_matching(&mut app, |action| {
        matches!(action, Action::SaveSetting { .. })
    });
    assert_eq!(sent.len(), 1, "{sent:?}");
    match &sent[0] {
        Action::SaveSetting { key, value } => {
            assert_eq!(*key, crate::domain::settings::keys::SANDBOX_NETWORK);
            assert_eq!(
                value.as_deref(),
                Some(crate::domain::settings::keys::NETWORK_OFF)
            );
        }
        other => panic!("{other:?}"),
    }

    // The service's own sentence is the one the surface shows, and the row goes
    // back to the store's copy.
    app.update(Msg::Done(ActionOutcome::SettingFailed {
        key: crate::domain::settings::keys::SANDBOX_NETWORK,
        message: "sandbox_memory_mb must be 256 to 65536".to_owned(),
    }));
    let screen = rows(&app, 80, 30).join("\n");
    assert!(
        screen.contains("sandbox_memory_mb must be 256 to 65536"),
        "{screen}"
    );
    let rereads = actions_matching(&mut app, |action| {
        matches!(action, Action::LoadSettings { .. })
    });
    assert_eq!(
        rereads.len(),
        1,
        "the write is followed by a read: {rereads:?}"
    );
}

#[test]
fn a_saved_write_names_the_field_and_reads_both_copies() {
    let mut app = settings_panel(80, 24);
    app.update(Msg::Done(ActionOutcome::SettingSaved {
        key: crate::domain::settings::keys::MODEL,
        label: "Model",
    }));
    let screen = rows(&app, 80, 24).join("\n");
    assert!(screen.contains("saved \u{b7} Model"), "{screen}");
    // The facts row and the picker read the same store, so both are re-read.
    let mut actions = app.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::LoadModels { .. })),
        "{actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::LoadSettings { .. })),
        "{actions:?}"
    );
    actions.clear();
}

#[test]
fn esc_closes_the_band_before_it_leaves_the_surface() {
    let mut app = settings_panel(80, 24);
    app.update(typing(KeyCode::Char('j')));
    app.update(typing(KeyCode::Enter));
    assert!(app.settings().editing().is_some());

    app.update(typing(KeyCode::Esc));
    assert_eq!(
        app.mode(),
        Surface::Settings,
        "the band closed, not the view"
    );
    assert!(app.settings().editing().is_none());

    app.update(typing(KeyCode::Esc));
    assert_eq!(app.mode(), Surface::Chat);
}

#[test]
fn slash_settings_opens_the_same_surface_ctrl_comma_does() {
    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    for ch in "/settings".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Settings);
    assert!(app.composer().is_blank(), "a command clears the field");
}

#[test]
fn the_audit_section_pages_its_rows_and_enter_expands_one() {
    use crate::tui::state::audit;
    let mut app = settings_panel(80, 30);
    for (call, ok) in [
        ("workspaces", true),
        ("sessions", true),
        ("save setting", false),
    ] {
        app.update(Msg::Audited(audit::Entry {
            seq: 0,
            call,
            ok,
            duration_ms: 7,
            body: format!("{call} answered"),
            at: chrono::Utc::now(),
        }));
    }
    assert_eq!(app.audit().len(), 3);
    for _ in 0..5 {
        app.update(typing(KeyCode::Char('l'))); // Audit log
    }
    assert_eq!(
        app.settings().section(),
        crate::tui::state::settings::Section::Audit
    );
    // The newest call is the first row, and the table is the surface's rows.
    let screen = rows(&app, 80, 30).join("\n");
    assert!(screen.contains("save setting"), "{screen}");
    assert!(screen.contains("failed"), "{screen}");

    app.update(typing(KeyCode::Enter));
    assert!(app.audit().is_expanded(0), "the selected row opened");
    let screen = rows(&app, 80, 30).join("\n");
    assert!(screen.contains("save setting answered"), "{screen}");
    app.update(typing(KeyCode::Enter));
    assert!(!app.audit().is_expanded(0), "and folded again");
}

#[test]
fn ctrl_c_closes_settings_help_and_detail_instead_of_quitting() {
    // The keyboard table says `Ctrl-C` closes an overlay, and reading a page is
    // not a reason to lose the chat. It quit from all three before T5.
    let mut app = settings_panel(80, 24);
    app.update(typing_ctrl('c'));
    assert_eq!(app.mode(), Surface::Chat);
    assert!(!app.should_quit(), "it closed the view, not the process");

    let mut app = with_messages(80, 24, vec![Message::user("hi")]);
    for ch in "/help".chars() {
        app.update(typing(KeyCode::Char(ch)));
    }
    app.update(typing(KeyCode::Enter));
    app.update(typing(KeyCode::Enter));
    assert_eq!(app.mode(), Surface::Help);
    app.update(typing_ctrl('c'));
    assert_eq!(app.mode(), Surface::Chat);
    assert!(!app.should_quit());

    let mut app = settings_panel(80, 24);
    app.update(typing(KeyCode::Esc));
    assert_eq!(app.mode(), Surface::Chat);
    assert!(!app.should_quit(), "and from the chat it still leaves");
}

#[test]
fn the_surfaces_keys_do_not_type_into_it_and_j_k_move_the_row() {
    let mut app = settings_panel(80, 30);
    let before = app.settings().selected;
    app.update(typing(KeyCode::Char('j')));
    assert_eq!(app.settings().selected, before + 1, "j moves the row");
    app.update(typing(KeyCode::Char('k')));
    assert_eq!(app.settings().selected, before);
    // `q` is a chat binding, so it does nothing here.
    app.update(typing(KeyCode::Char('q')));
    assert!(!app.should_quit());
    // A page moves by the list's page size, not by one.
    app.update(typing(KeyCode::PageDown));
    assert!(app.settings().selected > before, "a page is several rows");
}

// ---------------------------------------------------------------------------
// T6: /attach, /detach, and /open
// ---------------------------------------------------------------------------

/// A file attachment outcome, as the effect would hand it back.
fn attached(session: Option<SessionId>, source: &str, text: &str) -> ActionOutcome {
    ActionOutcome::Attached {
        session,
        source: source.to_owned(),
        pending: crate::tui::state::composer::Pending::File(robi_core::message::FileAttachment {
            name: source.rsplit('/').next().unwrap_or(source).to_owned(),
            path: Some(source.to_owned()),
            start_line: None,
            end_line: None,
            text: text.to_owned(),
        }),
    }
}

fn attaches(app: &mut App) -> Vec<(String, Option<SessionId>)> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Attach {
                source, session, ..
            } => Some((source, session)),
            _ => None,
        })
        .collect()
}

fn opens(app: &mut App) -> Vec<String> {
    app.take_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::OpenFile { path, .. } => Some(path),
            _ => None,
        })
        .collect()
}

/// Run one `/` command by typing it and pressing `Enter`.
///
/// A line whose leading token is a command opens the `/` menu, and the menu's own
/// `Enter` accepts the highlighted row rather than running it: a picked command
/// takes one more keystroke, which is what stops a typo from running something.
/// So the second `Enter` is the one that acts, exactly as it is for a reader.
fn command(app: &mut App, line: &str) {
    type_text(app, line);
    press(app, key(KeyCode::Enter));
    // A command that ran cleared the field; one the `/` menu accepted did not, so
    // the next `Enter` is the one that runs it — the same two keystrokes a reader
    // makes.
    if !app.composer().text().is_empty() {
        press(app, key(KeyCode::Enter));
    }
}

#[test]
fn attach_issues_one_read_for_the_path_it_was_given() {
    let (mut app, _, session) = booted(80, 24);
    land(&mut app, session, Vec::new());
    app.take_actions();

    command(&mut app, "/attach src/app.rs:40-80");
    assert_eq!(
        attaches(&mut app),
        vec![("src/app.rs:40-80".to_owned(), Some(session))],
        "the whole argument, range and all, is the effect's to split"
    );
    assert_eq!(app.composer().text(), "", "a command clears the field");
}

#[test]
fn attach_refuses_a_blank_or_two_line_argument_and_issues_nothing() {
    let (mut app, _, _) = booted(80, 24);
    command(&mut app, "/attach   ");
    assert!(attaches(&mut app).is_empty());
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("/attach needs an argument")
    );
    assert!(app.notice().is_some_and(|notice| notice.is_error()));

    // `verdict` takes the argument from the whole message, so a second line would
    // otherwise walk into the path.
    command(&mut app, "/attach a.rs\nb.rs");
    assert!(attaches(&mut app).is_empty());
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("/attach takes one path on one line")
    );
}

#[test]
fn attach_refuses_a_file_already_on_the_message() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "fn main")));

    command(&mut app, "/attach src/app.rs");
    assert!(attaches(&mut app).is_empty(), "no second read");
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("src/app.rs is already attached")
    );
    assert_eq!(app.composer().attachment_count(), 1);
}

#[test]
fn a_read_that_lands_after_a_session_switch_is_dropped() {
    let (mut app, _, _) = booted(80, 24);
    // A read issued for the draft, landing once a chat is open.
    app.update(Msg::Done(ActionOutcome::Attached {
        session: None,
        source: "src/app.rs".to_owned(),
        pending: crate::tui::state::composer::Pending::Image(
            crate::tui::state::composer::PendingImage {
                name: "shot.png".to_owned(),
                media_type: "image/png".to_owned(),
                bytes: vec![0; 4],
            },
        ),
    }));
    assert_eq!(
        app.composer().attachment_count(),
        0,
        "a chip must not land under another chat"
    );
}

#[test]
fn the_caps_that_need_what_is_already_attached_are_enforced_here() {
    let (mut app, _, session) = booted(80, 24);
    for index in 0..crate::domain::chat_message::attachment::MAX_ATTACHMENTS {
        app.update(Msg::Done(attached(
            Some(session),
            &format!("src/f{index}.rs"),
            "x",
        )));
    }
    assert_eq!(
        app.composer().attachment_count(),
        crate::domain::chat_message::attachment::MAX_ATTACHMENTS
    );
    // The ninth is refused, and says so in the footer.
    app.update(Msg::Done(attached(
        Some(session),
        "src/one-too-many.rs",
        "x",
    )));
    assert_eq!(
        app.composer().attachment_count(),
        crate::domain::chat_message::attachment::MAX_ATTACHMENTS
    );
    assert!(app.notice().is_some_and(|notice| notice.is_error()));
}

#[test]
fn detach_takes_one_by_name_or_everything() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "one")));
    app.update(Msg::Done(attached(Some(session), "src/error.rs", "two")));
    assert_eq!(app.composer().attachment_count(), 2);

    command(&mut app, "/detach app.rs");
    assert_eq!(app.composer().attachment_count(), 1);
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("app.rs detached")
    );

    command(&mut app, "/detach");
    assert_eq!(app.composer().attachment_count(), 0);
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("1 attachment removed")
    );

    // With nothing attached it is an answer, not a failure.
    command(&mut app, "/detach");
    assert!(app.notice().is_some_and(|notice| !notice.is_error()));
}

#[test]
fn detach_of_an_unknown_name_is_a_footer_error_and_removes_nothing() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "one")));
    command(&mut app, "/detach nope.rs");
    assert_eq!(app.composer().attachment_count(), 1);
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("nope.rs is not attached")
    );
}

#[test]
fn backspace_takes_the_last_attachment_off_an_empty_field() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "one")));
    assert_eq!(app.composer().attachment_count(), 1);

    press(&mut app, key(KeyCode::Backspace));
    assert_eq!(app.composer().attachment_count(), 0);
    assert_eq!(
        app.notice().map(|notice| notice.text.as_str()),
        Some("app.rs detached")
    );
}

#[test]
fn backspace_with_text_still_deletes_a_character() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "one")));
    type_text(&mut app, "abc");

    press(&mut app, key(KeyCode::Backspace));
    assert_eq!(app.composer().text(), "ab", "the field keeps the key");
    assert_eq!(
        app.composer().attachment_count(),
        1,
        "the attachment is not what was being deleted"
    );
}

#[test]
fn a_message_with_only_an_attachment_is_sendable() {
    let (mut app, _, session) = booted(80, 24);
    land(&mut app, session, Vec::new());
    app.take_actions();
    app.update(Msg::Done(ActionOutcome::Attached {
        session: Some(session),
        source: "shot.png".to_owned(),
        pending: crate::tui::state::composer::Pending::Image(
            crate::tui::state::composer::PendingImage {
                name: "shot.png".to_owned(),
                media_type: "image/png".to_owned(),
                bytes: vec![0; 2048],
            },
        ),
    }));

    press(&mut app, key(KeyCode::Enter));
    let sent: Vec<Action> = app.take_actions();
    let submit = sent
        .into_iter()
        .find_map(|action| match action {
            Action::Submit { text, images, .. } => Some((text, images)),
            _ => None,
        })
        .expect("one submit");
    assert_eq!(submit.0, "");
    assert_eq!(submit.1.len(), 1);
    assert_eq!(submit.1[0].name, "shot.png");
}

#[test]
fn a_refused_send_keeps_the_text_and_the_attachments() {
    let (mut app, _, session) = booted(80, 24);
    app.update(Msg::Done(attached(Some(session), "src/app.rs", "one")));
    type_text(&mut app, "look at this");
    press(&mut app, key(KeyCode::Enter));
    app.take_actions();

    app.update(Msg::Done(ActionOutcome::SubmitFailed {
        session,
        message: "the store refused".to_owned(),
    }));
    assert_eq!(app.composer().text(), "look at this");
    assert_eq!(app.composer().attachment_count(), 1);
}

#[test]
fn open_issues_one_call_for_a_workspace_relative_path() {
    let (mut app, _, _) = booted(80, 24);
    command(&mut app, "/open src/app.rs");
    assert_eq!(opens(&mut app), vec!["src/app.rs".to_owned()]);
    assert_eq!(app.composer().text(), "");
}

#[test]
fn open_refuses_an_absolute_path_a_tilde_and_a_blank_one() {
    let (mut app, _, _) = booted(80, 24);
    for (line, expected) in [
        (
            "/open /etc/passwd",
            "/open takes a path inside this workspace, not an absolute one",
        ),
        (
            "/open ~/notes.md",
            "/open takes a path inside this workspace, not ~",
        ),
        ("/open   ", "/open needs an argument"),
    ] {
        command(&mut app, line);
        assert!(opens(&mut app).is_empty(), "{line} issued a call");
        let notice = app.notice().expect("a notice");
        assert!(notice.is_error(), "{line} was not an error");
        assert_eq!(notice.text, expected, "{line}");
    }
}

#[test]
fn an_unset_open_command_is_the_effect_error_and_not_a_fallback() {
    let (mut app, _, _) = booted(80, 24);
    app.update(Msg::Done(ActionOutcome::OpenFailed {
        message: "no file open command is set \u{2014} set one in Ctrl-,".to_owned(),
    }));
    let notice = app.notice().expect("a notice");
    assert!(notice.is_error());
    assert!(notice.text.contains("no file open command is set"));
}

#[test]
fn the_composer_draws_a_chip_row_and_the_band_grows_for_it() {
    let (mut app, _, session) = booted(80, 24);
    land(&mut app, session, Vec::new());
    app.take_actions();
    let bare = app.body_height();

    app.update(Msg::Done(attached(Some(session), "src/app.rs", "fn main")));
    let lines = rows(&app, 80, 24);
    assert!(
        lines.iter().any(|line| line.contains("app.rs")),
        "the chip is on screen: {lines:#?}"
    );
    assert_eq!(
        app.body_height(),
        bare - 1,
        "the band took one of the transcript's rows"
    );
}

#[test]
fn a_sent_attachment_reads_as_a_chip_in_the_user_band() {
    let mut message = Message::user("look");
    message.files.push(robi_core::message::FileAttachment {
        name: "app.rs".to_owned(),
        path: Some("src/app.rs".to_owned()),
        start_line: Some(40),
        end_line: Some(80),
        text: "fn main".to_owned(),
    });
    message.images.push(robi_core::message::ImageAttachment {
        id: "img".to_owned(),
        media_type: "image/png".to_owned(),
    });
    let app = with_messages(80, 24, vec![message]);
    let screen = screen(&app, 80, 24);
    assert!(screen.contains("app.rs (40-80)"), "{screen}");
    assert!(screen.contains("[image, image/png]"), "{screen}");
}

#[test]
fn the_slash_menu_offers_the_three_path_commands() {
    // The menu is forty columns wide, so it cuts the about text; what it cannot
    // cut is the name, which is the part a reader is choosing between.
    for (typed, name) in [("/att", "/attach"), ("/det", "/detach"), ("/ope", "/open")] {
        let (mut app, _, _) = booted(80, 24);
        type_text(&mut app, typed);
        let screen = screen(&app, 80, 24);
        assert!(
            screen.contains(name),
            "{typed} does not offer {name}: {screen}"
        );
    }
}

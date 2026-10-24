//! The `TestBackend` harness, and the view snapshots.
//!
//! A snapshot here asserts on rendered strings, not on a golden file: a token
//! change that does not move the layout must not fail a test.

use chrono::Utc;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use robi_core::ids::{SessionId, WorkspaceId};
use robi_core::message::Message;

use crate::domain::chat_session::model::{
    AgentMode, ChatSession, ModelConfig, PathRules, TurnDisplay,
};
use crate::domain::events::{EventEnvelope, MESSAGE_ADDED, MESSAGE_DELTA, SOURCE};
use crate::domain::workspace::model::Workspace;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::app::{App, Msg};
use crate::tui::persist::ViewState;

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
    let generation = app
        .take_actions()
        .into_iter()
        .find_map(|action| match action {
            Action::Reload { generation, .. } => Some(generation),
            _ => None,
        })
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

/// One bus frame for a session.
fn envelope(session: SessionId, event_type: &str) -> EventEnvelope {
    EventEnvelope::from_payload(
        SOURCE,
        event_type,
        session.to_string(),
        serde_json::json!({ "session_id": session.to_string() }),
    )
    .expect("an envelope")
}

/// The frame a stored row raises, which means refetch that transcript.
fn stored_row(session: SessionId) -> EventEnvelope {
    envelope(session, MESSAGE_ADDED)
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

#[test]
fn a_stored_row_frame_issues_exactly_one_reload() {
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();
    app.update(Msg::Agent(stored_row(session_id)));
    assert_eq!(generations(&mut app).len(), 1);
}

#[test]
fn a_message_delta_issues_no_reload() {
    // The bus drops a queued delta under backpressure, so a transcript built
    // from deltas would lose text. A delta never touches the rows.
    let (mut app, _, session_id) = booted(80, 24);
    app.take_actions();
    app.update(Msg::Agent(envelope(session_id, MESSAGE_DELTA)));
    assert!(generations(&mut app).is_empty());
}

#[test]
fn a_stale_transcript_outcome_is_dropped() {
    let (mut app, _, session_id) = booted(80, 24);
    let stale = generations(&mut app);
    assert_eq!(stale.len(), 1, "the first load issued one reload");

    // A stored row arrives, which issues a newer reload of the same session.
    app.update(Msg::Agent(stored_row(session_id)));
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
        24,
        vec![
            Message::user("migrate and test"),
            Message::assistant_with_tool_calls("", vec![migrate, test]),
        ],
    );
    let frame = screen(&app, 92, 24);

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
        assert_eq!(
            indent, gutter,
            "{row:?} starts at column {indent}, not {gutter}"
        );
    }
    // Nothing reaches the last column either, on a full-width band.
    let widest = drawn.iter().map(|row| row.len()).max().unwrap_or(0);
    assert!(
        widest <= 60 - gutter,
        "widest row is {widest} in a 60-column frame"
    );
}

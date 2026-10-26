//! The sessions overlay: the filter, the selection, and a row's activity mark.
//!
//! The terminal's peer of `RecentsMenu.tsx` and `recents.ts`, plus the one row
//! the GUI's top bar carries and a menu cannot: **New session**. It is the first
//! row, so the way to start a fresh chat is the first thing under the cursor when
//! the overlay opens, and `Enter` on it creates one.
//!
//! The list, the filter, and the marks are pure functions of the rows the app was
//! handed, so they are tested without a terminal; the module owns only what the
//! overlay itself remembers between keys. The selection is an **index**, not a
//! session id, because row zero is an action rather than a session.

use std::collections::BTreeSet;

use ratatui::crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui_textarea::TextArea;
use robi_core::ids::SessionId;

use crate::domain::chat_session::model::{ChatSession, TurnDisplay};

/// What a session with no title reads as, matching `sessionDisplayTitle`.
pub const UNTITLED: &str = "Untitled session";

/// The action row above the sessions.
pub const NEW_SESSION: &str = "New session\u{2026}";

/// Rows the overlay has for `sessions`: the action row, then one per session.
pub fn row_count(sessions: usize) -> usize {
    sessions + 1
}

/// The session a row index names, or `None` for the action row.
pub fn session_at(rows: &[SessionId], selected: usize) -> Option<SessionId> {
    selected
        .checked_sub(1)
        .and_then(|index| rows.get(index).copied())
}

/// The spinner frames, one cell each, so a mark never moves the layout.
const SPINNER: [char; 4] = ['\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}'];

/// A row's activity, in the precedence `recents.ts` uses: a waiting approval is
/// the thing that needs the reader, then a running actor, then nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Idle,
    Running,
    Awaiting,
}

/// One spinner step every two ticks, so a pass takes two seconds.
pub fn spinner(ticks: u64) -> char {
    SPINNER[((ticks / 2) % SPINNER.len() as u64) as usize]
}

pub fn display_title(session: &ChatSession) -> &str {
    session
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .unwrap_or(UNTITLED)
}

pub fn mark(session: &ChatSession, running: &BTreeSet<SessionId>) -> Mark {
    if session.turn_display == TurnDisplay::AwaitingApproval {
        return Mark::Awaiting;
    }
    if running.contains(&session.id) || session.turn_display == TurnDisplay::Pending {
        return Mark::Running;
    }
    Mark::Idle
}

/// The sessions a query matches, newest first as the service returns them.
pub fn visible<'a>(sessions: &'a [ChatSession], query: &str) -> Vec<&'a ChatSession> {
    let needle = query.trim().to_lowercase();
    sessions
        .iter()
        .filter(|session| {
            needle.is_empty() || display_title(session).to_lowercase().contains(&needle)
        })
        .collect()
}

/// The overlay's own state: what was typed, and which row the keys act on.
#[derive(Debug)]
pub struct SessionsView {
    filter: TextArea<'static>,
    /// The row the keys act on. Zero is the new-session action.
    selected: usize,
}

impl Default for SessionsView {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionsView {
    pub fn new() -> Self {
        let mut filter = TextArea::default();
        filter.set_cursor_line_style(Style::default());
        filter.set_placeholder_text("filter");
        Self {
            filter,
            selected: 0,
        }
    }

    /// Open with the action row first and the open session selected when it is
    /// listed, and no filter.
    ///
    /// The query clears on open, as the GUI clears it on close: a filter left
    /// over from yesterday would hide the session the reader came back for.
    pub fn open(&mut self) {
        self.filter = TextArea::default();
        self.filter.set_cursor_line_style(Style::default());
        self.filter.set_placeholder_text("filter");
        self.selected = 0;
    }

    pub fn query(&self) -> String {
        self.filter.lines().join(" ")
    }

    /// The filter, for the frame to draw and place its caret.
    pub fn field(&self) -> &TextArea<'static> {
        &self.filter
    }

    pub fn input(&mut self, key: &KeyEvent) {
        self.filter.input(*key);
    }

    /// Insert pasted text. The filter is one line, so a break is a space.
    pub fn insert_text(&mut self, text: &str) {
        self.filter.insert_str(text);
    }

    /// The selected row, where zero is the new-session action.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Select `id`'s row, or the action row when it is not listed.
    pub fn set_selected(&mut self, id: Option<SessionId>, rows: &[SessionId]) {
        self.selected = id
            .and_then(|id| rows.iter().position(|row| *row == id))
            .map(|index| index + 1)
            .unwrap_or(0);
    }

    /// Keep the selection on a row that exists.
    ///
    /// Called after the list and the filter both change, so the keys never act on
    /// a row that has gone: a session that vanished takes its row with it, and
    /// the selection falls back to the one now in that place.
    pub fn reconcile(&mut self, rows: &[SessionId]) {
        let count = row_count(rows.len());
        if self.selected >= count {
            self.selected = count - 1;
        }
    }

    /// Move the selection one row, clamped. Row zero is always there.
    pub fn select(&mut self, delta: isize, rows: &[SessionId]) {
        let count = row_count(rows.len()) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, count - 1) as usize;
    }

    /// Move by `page` rows, clamped the same way.
    pub fn page(&mut self, delta: isize, rows: &[SessionId], page: usize) {
        self.select(delta * page as isize, rows);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use robi_core::ids::WorkspaceId;

    use crate::domain::chat_session::model::{
        AgentMode, ChatSession, ModelConfig, PathRules, TurnDisplay,
    };

    use super::*;

    fn session(workspace: WorkspaceId, title: Option<&str>) -> ChatSession {
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

    #[test]
    fn the_filter_matches_the_title_case_insensitively_and_an_empty_query_matches_all() {
        let workspace = WorkspaceId::new();
        let sessions = vec![
            session(workspace, Some("Fix the parser")),
            session(workspace, Some("Read the roadmap")),
            session(workspace, None),
        ];
        assert_eq!(visible(&sessions, "").len(), 3);
        assert_eq!(visible(&sessions, "  ").len(), 3);
        assert_eq!(visible(&sessions, "PARSER")[0].id, sessions[0].id);
        assert_eq!(visible(&sessions, "untitled")[0].id, sessions[2].id);
        assert!(visible(&sessions, "nothing").is_empty());
    }

    #[test]
    fn a_titleless_session_reads_as_untitled() {
        let session = session(WorkspaceId::new(), None);
        assert_eq!(display_title(&session), UNTITLED);
    }

    #[test]
    fn awaiting_wins_over_running_and_running_over_idle() {
        let workspace = WorkspaceId::new();
        let mut waiting = session(workspace, Some("a"));
        waiting.turn_display = TurnDisplay::AwaitingApproval;
        let mut running = session(workspace, Some("b"));
        running.turn_display = TurnDisplay::Pending;
        let idle = session(workspace, Some("c"));
        let live = session(workspace, Some("d"));

        let mut live_ids = BTreeSet::new();
        live_ids.insert(running.id);
        live_ids.insert(live.id);
        live_ids.insert(waiting.id);

        assert_eq!(mark(&waiting, &live_ids), Mark::Awaiting);
        assert_eq!(mark(&running, &live_ids), Mark::Running);
        assert_eq!(mark(&live, &live_ids), Mark::Running);
        assert_eq!(mark(&idle, &BTreeSet::new()), Mark::Idle);
    }

    #[test]
    fn the_spinner_advances_one_step_every_two_ticks() {
        assert_eq!(spinner(0), SPINNER[0]);
        assert_eq!(spinner(1), SPINNER[0]);
        assert_eq!(spinner(2), SPINNER[1]);
        assert_eq!(spinner(8), SPINNER[0]);
    }

    #[test]
    fn row_zero_is_the_new_session_action_and_the_sessions_follow_it() {
        let workspace = WorkspaceId::new();
        let sessions = [
            session(workspace, Some("alpha")),
            session(workspace, Some("beta")),
            session(workspace, Some("gamma")),
        ];
        let ids: Vec<_> = sessions.iter().map(|session| session.id).collect();
        let mut view = SessionsView::new();
        view.open();
        assert_eq!(view.selected(), 0, "the action row is where the keys start");
        assert_eq!(session_at(&ids, 0), None);
        assert_eq!(session_at(&ids, 1), Some(ids[0]));

        view.set_selected(Some(ids[1]), &ids);
        assert_eq!(view.selected(), 2, "beta is the second session, third row");
        assert_eq!(session_at(&ids, view.selected()), Some(ids[1]));

        view.select(1, &ids);
        assert_eq!(session_at(&ids, view.selected()), Some(ids[2]));
        view.select(1, &ids);
        assert_eq!(view.selected(), 3, "clamped at the last session");
        view.select(-9, &ids);
        assert_eq!(view.selected(), 0, "and at the action row");

        // A row that goes takes the selection with it.
        view.selected = row_count(ids.len()) - 1;
        view.reconcile(&ids[..1]);
        assert_eq!(view.selected(), 1, "the last row that exists");

        // A session that is not listed selects nothing in particular.
        view.set_selected(Some(SessionId::new()), &ids);
        assert_eq!(view.selected(), 0);

        view.page(1, &ids, 2);
        assert_eq!(view.selected(), 2);
    }

    #[test]
    fn the_action_row_survives_an_empty_list() {
        let mut view = SessionsView::new();
        view.open();
        let rows: Vec<SessionId> = Vec::new();
        view.select(1, &rows);
        assert_eq!(view.selected(), 0, "there is nowhere else to go");
        view.reconcile(&rows);
        assert_eq!(view.selected(), 0);
    }
}

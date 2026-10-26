//! Startup: the view state and the first pick.
//!
//! The picks are pure functions of what the services returned, so they are
//! tested without a database. `app::update` calls them when an outcome lands.
//!
//! The terminal lifecycle helpers here are the ones `ratatui::init` does not
//! cover: mouse reporting, which has to be turned on next to raw mode and off
//! again before the alternate screen goes.

use std::io;
use std::path::Path;

use robi_core::ids::{SessionId, WorkspaceId};

use crate::domain::chat_session::model::ChatSession;
use crate::domain::workspace::model::Workspace;
use crate::tui::persist::{self, ViewState};

/// The view state from the last run.
pub fn restore(state_dir: &Path) -> ViewState {
    persist::load(state_dir)
}

/// Turn on mouse reporting, so the wheel scrolls the transcript.
///
/// `ratatui::init` takes raw mode and the alternate screen but leaves mouse
/// reporting off, and that default is right for a terminal: a program that
/// always captures the pointer takes text selection away from the user. The TUI
/// turns it on because the wheel is the scroll gesture most people try first,
/// and it reads only wheel events — a click, a drag, and a move are ignored.
pub fn enable_mouse() -> io::Result<()> {
    ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::event::EnableMouseCapture)
}

/// Ask the terminal to mark a paste, so it arrives as one event.
///
/// DECSET 2004. Without it a terminal *types* the pasted text at the field,
/// newline and all, and a newline is `Enter`: pasting a block of text sent its
/// first line as a message. With it the block arrives as one [`Msg::Paste`] and
/// the field takes it as text. A terminal that does not implement it degrades
/// to the old behaviour, which is why the key path is still the one that
/// submits.
///
/// [`Msg::Paste`]: crate::tui::app::Msg::Paste
pub fn enable_bracketed_paste() -> io::Result<()> {
    ratatui::crossterm::execute!(
        io::stdout(),
        ratatui::crossterm::event::EnableBracketedPaste
    )
}

/// Stop paste marking, before the alternate screen goes.
pub fn disable_bracketed_paste() {
    if let Err(error) = ratatui::crossterm::execute!(
        io::stdout(),
        ratatui::crossterm::event::DisableBracketedPaste
    ) {
        tracing::warn!(%error, "could not turn bracketed paste off");
    }
}

/// Ask the terminal to report whether its window is in front.
///
/// DECSET 1004. Optional by nature: a terminal that does not implement it simply
/// never sends the events, and the app stays "focused", which means a pause rings
/// no bell. That is the honest degradation — a bell is an alert, not a fact.
pub fn enable_focus_change() -> io::Result<()> {
    ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::event::EnableFocusChange)
}

/// Stop focus reporting, before the alternate screen goes.
pub fn disable_focus_change() {
    if let Err(error) =
        ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::event::DisableFocusChange)
    {
        tracing::warn!(%error, "could not turn focus reporting off");
    }
}

/// Write the window title.
///
/// Best effort on purpose: `SIGTERM` and a restored terminal are more important
/// than a title, and there is no one left to tell on the way out.
pub fn set_title(title: &str) {
    if let Err(error) =
        ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::terminal::SetTitle(title))
    {
        tracing::warn!(%error, "could not set the window title");
    }
}

/// Turn mouse reporting off again, before the alternate screen goes.
///
/// Restoring is best effort and logged rather than returned: it runs on the way
/// out, after the last frame, and there is no one left to tell.
pub fn disable_mouse() {
    if let Err(error) =
        ratatui::crossterm::execute!(io::stdout(), ratatui::crossterm::event::DisableMouseCapture)
    {
        tracing::warn!(%error, "could not turn mouse reporting off");
    }
}

/// The workspace to show.
///
/// The remembered one when it still exists, otherwise the first the service
/// returned, which `list_workspaces` orders newest first.
pub fn pick_workspace(
    workspaces: &[Workspace],
    preferred: Option<WorkspaceId>,
) -> Option<WorkspaceId> {
    if let Some(preferred) = preferred {
        if workspaces.iter().any(|workspace| workspace.id == preferred) {
            return Some(preferred);
        }
    }
    workspaces.first().map(|workspace| workspace.id)
}

/// The session to show.
///
/// The remembered one when it is still in this workspace, otherwise the first,
/// which `list_chat_sessions` orders by `last_used_at` — the most recent
/// session.
pub fn pick_session(
    sessions: &[ChatSession],
    workspace: WorkspaceId,
    preferred: Option<SessionId>,
) -> Option<SessionId> {
    if let Some(preferred) = preferred {
        if sessions
            .iter()
            .any(|session| session.id == preferred && session.workspace_id == workspace)
        {
            return Some(preferred);
        }
    }
    sessions.first().map(|session| session.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    // The row fixtures live in one place, so a field added to or removed from
    // `ChatSession` is one edit for the whole module.
    use crate::tui::testing::{session, workspace};

    #[test]
    fn the_remembered_workspace_wins_when_it_still_exists() {
        let list = vec![workspace("a"), workspace("b")];
        assert_eq!(pick_workspace(&list, Some(list[1].id)), Some(list[1].id));
    }

    #[test]
    fn a_vanished_workspace_falls_back_to_the_newest() {
        let list = vec![workspace("a"), workspace("b")];
        assert_eq!(
            pick_workspace(&list, Some(WorkspaceId::new())),
            Some(list[0].id)
        );
        assert_eq!(pick_workspace(&[], Some(WorkspaceId::new())), None);
    }

    #[test]
    fn the_remembered_session_wins_only_inside_its_workspace() {
        let workspace_id = WorkspaceId::new();
        let list = vec![
            session(workspace_id, Some("one")),
            session(workspace_id, Some("two")),
        ];
        assert_eq!(
            pick_session(&list, workspace_id, Some(list[1].id)),
            Some(list[1].id)
        );
        // A session from another workspace, or one that is gone, falls back.
        let other = session(WorkspaceId::new(), Some("elsewhere"));
        assert_eq!(
            pick_session(&list, workspace_id, Some(other.id)),
            Some(list[0].id)
        );
        assert_eq!(pick_session(&[], workspace_id, None), None);
    }
}

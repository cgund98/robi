//! Startup: the view state, the first pick, and the stopgap process lock.
//!
//! The picks are pure functions of what the services returned, so they are
//! tested without a database. `app::update` calls them when an outcome lands.
//!
//! The terminal lifecycle helpers here are the ones `ratatui::init` does not
//! cover: mouse reporting, which has to be turned on next to raw mode and off
//! again before the alternate screen goes.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use robi_core::ids::{SessionId, WorkspaceId};

use crate::domain::chat_session::model::ChatSession;
use crate::domain::workspace::model::Workspace;
use crate::tui::persist::{self, ViewState};

/// The stopgap lock file, under `~/.robi`.
///
/// Deleted, not extended, when the per-session lease lands: the lease in
/// `docs/src/design/shell/parallel-front-ends.md` replaces it.
pub const LOCK_FILE: &str = "tui.lock";

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

/// The advisory lock one process holds for its lifetime.
///
/// Two front ends on one database can drive two turns on one session, and this
/// detects terminal-against-terminal only. It cannot see `robi-api` or the
/// desktop app, which is why the design doc calls it a stopgap.
#[derive(Debug)]
pub struct Lock {
    /// Held for its descriptor: the OS releases the lock when it closes,
    /// including when the process is killed.
    _file: File,
}

impl Lock {
    /// Take the lock, or fail with a message naming the holder.
    pub fn acquire(state_dir: &Path, database_url: &str) -> io::Result<Lock> {
        std::fs::create_dir_all(state_dir)?;
        let path = state_dir.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let holder = read_holder(&mut file);
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    holder_message(holder.as_deref()),
                ));
            }
            Err(std::fs::TryLockError::Error(err)) => return Err(err),
        }
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        writeln!(file, "pid {} database {database_url}", std::process::id())?;
        // The holder line is read by the next process, so it has to be on disk
        // before that process can fail the lock.
        file.sync_all()?;
        Ok(Lock { _file: file })
    }
}

fn read_holder(file: &mut File) -> Option<String> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    let line = text.lines().next().unwrap_or("").trim().to_owned();
    (!line.is_empty()).then_some(line)
}

fn holder_message(holder: Option<&str>) -> String {
    match holder {
        Some(holder) => format!("robi-tui is already running ({holder})"),
        None => "robi-tui is already running in another process".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    // The row fixtures live in one place, so a field added to or removed from
    // `ChatSession` is one edit for the whole module.
    use crate::tui::testing::{session, workspace};

    fn state_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("robi-tui-lock-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

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

    #[test]
    fn the_lock_is_exclusive_and_reacquirable() {
        let dir = state_dir();
        let first = Lock::acquire(&dir, "sqlite://a.db").expect("the first lock");

        let error = Lock::acquire(&dir, "sqlite://a.db").expect_err("the second must fail");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(error.to_string().contains("already running"), "{error}");
        assert!(
            error.to_string().contains(&std::process::id().to_string()),
            "the message names the holder: {error}"
        );

        drop(first);
        let again = Lock::acquire(&dir, "sqlite://a.db");
        assert!(again.is_ok(), "dropping the lock frees it");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

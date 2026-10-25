//! The one-line confirmation behind a destructive key.
//!
//! Its own mode rather than a flag on the overlay: the overlay's field owns
//! printable keys, so `y` would be a character in a filter and a confirmation at
//! once. A mode above the overlay keeps `(Mode, Focus, Key) -> Command` a table.

use robi_core::ids::{SessionId, WorkspaceId};

/// What the confirmation will do if it is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    DeleteSession(SessionId),
    DeleteWorkspace(WorkspaceId),
}

/// The question, and the thing it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub target: Target,
    /// The question, naming what is about to go.
    pub question: String,
    /// A second sentence: what else the action takes with it.
    pub detail: Option<String>,
}

impl Confirm {
    /// `Delete “a session”?` and, while a turn runs, what that costs.
    pub fn delete_session(id: SessionId, title: &str, running: bool) -> Self {
        Self {
            target: Target::DeleteSession(id),
            question: format!("Delete \u{201c}{title}\u{201d}?"),
            detail: running.then(|| "a running turn will be stopped".to_owned()),
        }
    }

    /// `Remove “robi”?` and the sessions that go with it.
    pub fn delete_workspace(id: WorkspaceId, name: &str) -> Self {
        Self {
            target: Target::DeleteWorkspace(id),
            question: format!("Remove \u{201c}{name}\u{201d}?"),
            detail: Some("its sessions will be deleted".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_session_says_its_turn_will_be_stopped() {
        let confirm = Confirm::delete_session(SessionId::new(), "a chat", true);
        assert_eq!(confirm.question, "Delete \u{201c}a chat\u{201d}?");
        assert_eq!(
            confirm.detail.as_deref(),
            Some("a running turn will be stopped")
        );
        assert!(matches!(confirm.target, Target::DeleteSession(_)));

        let idle = Confirm::delete_session(SessionId::new(), "a chat", false);
        assert_eq!(idle.detail, None);
    }

    #[test]
    fn removing_a_workspace_names_the_sessions_it_takes() {
        let id = WorkspaceId::new();
        let confirm = Confirm::delete_workspace(id, "robi");
        assert_eq!(confirm.target, Target::DeleteWorkspace(id));
        assert_eq!(confirm.question, "Remove \u{201c}robi\u{201d}?");
        assert_eq!(
            confirm.detail.as_deref(),
            Some("its sessions will be deleted")
        );
    }
}

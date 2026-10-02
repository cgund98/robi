use chrono::{DateTime, Utc};
use robi_core::ids::WorkspaceId;

/// A directory the agent operates in, and the parent of its chat sessions.
///
/// `name` is the final path component of `root`, for the shell menu.
/// `root` is the canonical absolute directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub root: String,
    pub created_at: DateTime<Utc>,
}

/// Result of opening a root. `created` is false when that directory was
/// already a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedWorkspace {
    pub workspace: Workspace,
    pub created: bool,
}

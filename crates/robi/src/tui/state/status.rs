//! What the frame says about the process rather than the transcript: the
//! app-level error, the service probe, and the MCP servers.
//!
//! Three facts that are about the process rather than the transcript. They live
//! together because one row says them together, and because none of them belongs
//! in a notice: a notice is an answer to something the user just did, and these
//! are true whether or not anything was pressed. Only the error and the health
//! mark are that row's; the server marks are summarised in the header, because
//! they are about the workspace rather than the process.
//!
//! The health probe is a direct service call, not `GET /api/v1/health`. The
//! terminal does not speak HTTP, and the route only proves the router is up:
//! `list_workspaces` proves the store answers, which is the fact the GUI's mark
//! is standing in for.

use ratatui::style::Color;
use robi_core::ids::WorkspaceId;

use crate::agent::mcp::McpServerStatus;
use crate::tui::theme;

/// Ticks between health probes: 20 x 250 ms = 5 s, the GUI's `POLL_MS`.
pub const PROBE_TICKS: u64 = 20;

/// How a server's connection reads in one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Connected,
    Starting,
    Failed,
    Disconnected,
}

impl MarkKind {
    pub fn from_status(status: &str) -> Self {
        match status {
            "connected" => Self::Connected,
            "starting" => Self::Starting,
            "failed" => Self::Failed,
            _ => Self::Disconnected,
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Connected => theme::SUCCESS,
            Self::Starting => theme::WARN,
            Self::Failed => theme::DANGER,
            Self::Disconnected => theme::INK_FAINT,
        }
    }
}

/// What the frame shows about the process, and about the workspace's servers.
///
/// The workspace name comes from `App::active_workspace`, because it is already
/// in the header and there is no second source of truth for it.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// `robi.app.v1.error` and a failed list read. Cleared by a successful one.
    error: Option<String>,
    /// Why the last probe failed, or `None` while it answered.
    health: Option<String>,
    /// The active workspace's servers, and which workspace they belong to.
    servers: Vec<McpServerStatus>,
    servers_workspace: Option<WorkspaceId>,
    /// Ticks since the last probe was issued.
    since_probe: u64,
    /// A probe is in flight; another would only queue behind it.
    probing: bool,
}

impl Status {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// A read the app asked for succeeded, so whatever failed has recovered.
    ///
    /// Only outcomes that read the store call this. A successful probe does
    /// not: otherwise an app error would disappear five seconds after it landed.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn health(&self) -> Option<&str> {
        self.health.as_deref()
    }

    /// A probe finished. `reason` is `None` when the store answered.
    pub fn probed(&mut self, reason: Option<String>) {
        self.probing = false;
        self.health = reason;
    }

    /// Whether a probe is due now, marking one in flight when it is.
    ///
    /// Called once per tick, so the cadence is counted in ticks and `update`
    /// stays a pure function of the messages it has seen.
    pub fn due(&mut self) -> bool {
        if self.probing {
            return false;
        }
        self.since_probe += 1;
        if self.since_probe < PROBE_TICKS {
            return false;
        }
        self.since_probe = 0;
        self.probing = true;
        true
    }

    pub fn is_probing(&self) -> bool {
        self.probing
    }

    pub fn servers(&self) -> &[McpServerStatus] {
        &self.servers
    }

    /// Replace the marks. A late answer for a workspace that is no longer active
    /// is dropped rather than shown under the wrong name.
    pub fn set_servers(&mut self, workspace: WorkspaceId, servers: Vec<McpServerStatus>) {
        self.servers = servers;
        self.servers_workspace = Some(workspace);
    }

    pub fn clear_servers(&mut self) {
        self.servers.clear();
        self.servers_workspace = None;
    }

    /// One word about the workspace's servers, for the header's right edge.
    ///
    /// Failed beats starting beats online: a workspace with one dead server is
    /// not `Online`, and saying so is the whole point of putting it on the one
    /// row that is always visible. `None` when no servers are configured, so an
    /// unused feature takes no space.
    pub fn summary(&self) -> Option<(String, MarkKind)> {
        if self.servers.is_empty() {
            return None;
        }
        let kind = if self.servers.iter().any(|s| s.status == "failed") {
            MarkKind::Failed
        } else if self.servers.iter().any(|s| s.status == "starting") {
            MarkKind::Starting
        } else if self.servers.iter().any(|s| s.status == "connected") {
            MarkKind::Connected
        } else {
            MarkKind::Disconnected
        };
        let label = match kind {
            MarkKind::Connected => "MCP Online",
            MarkKind::Failed => "MCP Failed",
            MarkKind::Starting => "MCP Starting",
            MarkKind::Disconnected => "MCP Offline",
        };
        Some((label.to_owned(), kind))
    }

    /// One `name:status` pair per configured server.
    ///
    /// The name rather than the GUI's icon: a terminal has no hover, so the
    /// status word is the only place the state can be said. Not drawn today —
    /// the header has the one-word summary — but it is the detail a status
    /// surface will read, and it is what the tests assert on.
    pub fn marks(&self) -> Vec<(String, MarkKind)> {
        self.servers
            .iter()
            .map(|server| {
                let name = server.title.as_deref().unwrap_or(&server.id);
                (
                    format!("{name}:{}", server.status),
                    MarkKind::from_status(&server.status),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: &str, status: &str) -> McpServerStatus {
        McpServerStatus {
            id: id.to_owned(),
            status: status.to_owned(),
            title: None,
            icon: None,
            tool_count: 0,
        }
    }

    #[test]
    fn a_probe_is_due_once_every_five_seconds_and_never_two_at_a_time() {
        let mut status = Status::new();
        for _ in 0..PROBE_TICKS - 1 {
            assert!(!status.due());
        }
        assert!(status.due(), "the twentieth tick asks for a probe");
        // In flight: the next ticks are quiet even past the interval.
        assert!(status.is_probing());
        for _ in 0..PROBE_TICKS * 2 {
            assert!(!status.due());
        }
        status.probed(None);
        assert!(!status.is_probing());
    }

    #[test]
    fn a_failed_probe_sets_the_mark_and_a_good_one_clears_it() {
        let mut status = Status::new();
        assert_eq!(status.health(), None);
        status.probed(Some("store is locked".to_owned()));
        assert_eq!(status.health(), Some("store is locked"));
        status.probed(None);
        assert_eq!(status.health(), None);
    }

    #[test]
    fn a_good_probe_does_not_clear_an_app_error() {
        let mut status = Status::new();
        status.set_error("a turn failed");
        status.probed(None);
        assert_eq!(status.error(), Some("a turn failed"));
        status.clear_error();
        assert_eq!(status.error(), None);
    }

    #[test]
    fn the_summary_is_one_word_and_failure_beats_the_rest() {
        let mut status = Status::new();
        assert_eq!(status.summary(), None, "no servers, no mark");
        let workspace = WorkspaceId::new();

        status.set_servers(workspace, vec![server("a", "connected")]);
        assert_eq!(
            status.summary(),
            Some(("MCP Online".to_owned(), MarkKind::Connected))
        );

        status.set_servers(
            workspace,
            vec![server("a", "connected"), server("b", "starting")],
        );
        assert_eq!(
            status.summary(),
            Some(("MCP Starting".to_owned(), MarkKind::Starting))
        );

        status.set_servers(
            workspace,
            vec![
                server("a", "connected"),
                server("b", "starting"),
                server("c", "failed"),
            ],
        );
        assert_eq!(
            status.summary(),
            Some(("MCP Failed".to_owned(), MarkKind::Failed))
        );

        status.set_servers(workspace, vec![server("a", "disconnected")]);
        assert_eq!(
            status.summary(),
            Some(("MCP Offline".to_owned(), MarkKind::Disconnected))
        );
    }

    #[test]
    fn marks_name_the_server_and_colour_the_status() {
        let mut status = Status::new();
        let workspace = WorkspaceId::new();
        status.set_servers(
            workspace,
            vec![server("sentry", "connected"), server("docs", "failed")],
        );
        assert_eq!(
            status.marks(),
            vec![
                ("sentry:connected".to_owned(), MarkKind::Connected),
                ("docs:failed".to_owned(), MarkKind::Failed),
            ]
        );
        assert_eq!(MarkKind::Connected.color(), theme::SUCCESS);
        assert_eq!(MarkKind::Failed.color(), theme::DANGER);
        assert_eq!(MarkKind::Starting.color(), theme::WARN);
        assert_eq!(MarkKind::Disconnected.color(), theme::INK_FAINT);

        status.clear_servers();
        assert!(status.marks().is_empty());
    }
}

use std::sync::Arc;

use crate::agent::index::IndexHub;
use crate::domain::{
    chat_message::service::ChatMessageService, chat_session::service::ChatSessionService,
    events::EventBus, file_change::repo::FileChangeRepository, settings::SettingsService,
    workspace::service::WorkspaceService,
};

/// Services the handlers call. The pool and the agent factory stay in the
/// composition root. A session actor builds its own agent from that factory.
#[derive(Clone)]
pub struct AppState {
    pub workspace_service: Arc<WorkspaceService>,
    pub chat_session_service: Arc<ChatSessionService>,
    pub chat_message_service: Arc<ChatMessageService>,
    pub settings_service: Arc<SettingsService>,
    pub event_bus: Arc<EventBus>,
    pub file_changes: Arc<dyn FileChangeRepository>,
    pub index: Arc<IndexHub>,
    /// MCP client supervisor. Absent in tests that do not list servers.
    pub mcp: Option<Arc<crate::agent::mcp::McpHub>>,
    /// Capped streams for the shell card.
    pub originals: Arc<dyn crate::agent::compress::OriginalStore>,
}

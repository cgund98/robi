use std::sync::Arc;

use crate::domain::{
    chat_message::service::ChatMessageService, chat_session::service::ChatSessionService,
    events::EventFanOut, file_change::repo::FileChangeRepository, settings::SettingsService,
    workspace::service::WorkspaceService,
};
use crate::index::IndexHub;

/// Services the handlers call. The pool and the agent factory stay in the
/// composition root. A session actor builds its own agent from that factory.
#[derive(Clone)]
pub struct AppState {
    pub workspace_service: Arc<WorkspaceService>,
    pub chat_session_service: Arc<ChatSessionService>,
    pub chat_message_service: Arc<ChatMessageService>,
    pub settings_service: Arc<SettingsService>,
    pub event_fanout: Arc<EventFanOut>,
    pub file_changes: Arc<dyn FileChangeRepository>,
    pub index: Arc<IndexHub>,
}

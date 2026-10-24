//! Session data on disk, removed when the chat session row goes away.
//!
//! Each session keeps its blobs under `sessions/<id>/` in the app home. Deleting
//! the row removes that directory. See
//! [`SessionDataCleaner`](crate::domain::chat_session::data::SessionDataCleaner).
//!
//! Plan directories are deliberately not here. Since S9 a plan is a markdown
//! directory in the **workspace** (`.robi/plans/<name>/`), so it is the user's
//! file and outlives the chat session that wrote it.

use async_trait::async_trait;
use robi_core::ids::SessionId;

use crate::{
    adapters::session_blobs::SessionBlobs, domain::chat_session::data::SessionDataCleaner,
};

/// Deletes `sessions/<id>/` when the chat session row goes away.
#[derive(Clone)]
pub struct FilesystemSessionData {
    blobs: SessionBlobs,
}

impl FilesystemSessionData {
    pub fn new(blobs: SessionBlobs) -> Self {
        Self { blobs }
    }
}

#[async_trait]
impl SessionDataCleaner for FilesystemSessionData {
    async fn remove_session_data(&self, session: SessionId) -> Result<(), String> {
        self.blobs.remove_session(session)
    }
}

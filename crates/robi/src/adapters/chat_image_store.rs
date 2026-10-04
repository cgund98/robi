//! `ImageSource` on the per-session blob file.
//!
//! The provider adapter resolves an attachment id back to bytes when it builds a
//! request. The transcript carries only the id and media type, which keeps the
//! bytes out of `chat_messages.body`, the event stream, and every DTO.

use async_trait::async_trait;
use robi_core::ids::SessionId;
use uuid::Uuid;

use crate::agent::providers::{ImageSource, ImageStore, ProviderError};

use super::session_blobs::SessionBlobs;

/// Reads and writes image bytes in the session blob file.
pub struct BlobImageStore {
    blobs: SessionBlobs,
}

impl BlobImageStore {
    pub fn new(blobs: SessionBlobs) -> Self {
        Self { blobs }
    }
}

#[async_trait]
impl ImageStore for BlobImageStore {
    /// Store one attachment under `id` for a session.
    async fn store(
        &self,
        chat_session_id: &str,
        id: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ProviderError> {
        let uuid = Uuid::parse_str(chat_session_id).map_err(|_| {
            ProviderError::Malformed(format!("chat session id is not a uuid: {chat_session_id}"))
        })?;
        let session = SessionId::from_uuid(uuid);
        let blobs = self.blobs.clone();
        let id = id.to_owned();
        let media_type = media_type.to_owned();
        tokio::task::spawn_blocking(move || blobs.put_image(session, &id, &media_type, &bytes))
            .await
            .map_err(|err| ProviderError::Malformed(err.to_string()))?
            .map_err(|err| ProviderError::Malformed(format!("writing the image failed: {err}")))?;
        Ok(())
    }
}

#[async_trait]
impl ImageSource for BlobImageStore {
    async fn image(&self, id: &str) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
        let blobs = self.blobs.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || blobs.get_image(&id))
            .await
            .map_err(|err| ProviderError::Malformed(err.to_string()))?
            .map_err(|err| ProviderError::Malformed(format!("reading the image failed: {err}")))
    }
}

/// An in-memory [`ImageStore`] for tests that do not want a SQLite pool.
#[derive(Default)]
pub struct MemoryImageStore {
    rows: std::sync::Mutex<std::collections::HashMap<String, (String, Vec<u8>)>>,
}

impl MemoryImageStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ImageSource for MemoryImageStore {
    async fn image(&self, id: &str) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
        Ok(self
            .rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned())
    }
}

#[async_trait]
impl ImageStore for MemoryImageStore {
    async fn store(
        &self,
        _chat_session_id: &str,
        id: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ProviderError> {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned(), (media_type.to_owned(), bytes));
        Ok(())
    }
}

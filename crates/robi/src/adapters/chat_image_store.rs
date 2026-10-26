//! `ImageSource` on the per-session blob file.
//!
//! The provider adapter resolves an attachment id back to bytes when it builds a
//! request. The transcript carries only the id and media type, which keeps the
//! bytes out of `chat_messages.body`, the event stream, and every DTO.
//!
//! The read takes the session id because the session's file is the index. There
//! is no shared image index any more: it was the only `redb` file no session
//! owned, so no session lock covered it and two processes writing images could
//! collide on it.

use async_trait::async_trait;
use robi_core::ids::SessionId;

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
        session: SessionId,
        id: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ProviderError> {
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
    /// The bytes are in the session's own file, which is also the index.
    ///
    /// An id from another session misses rather than resolving, so a wrong or
    /// stale session id reads as `None` and the caller reports a missing image.
    async fn image(
        &self,
        session: SessionId,
        id: &str,
    ) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
        let blobs = self.blobs.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || blobs.get_image(session, &id))
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
    async fn image(
        &self,
        _session: SessionId,
        id: &str,
    ) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
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
        _session: SessionId,
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

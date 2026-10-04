//! The port that provides the bytes behind an [`ImageAttachment`].
//!
//! The transcript holds only an id and a media type (D10). `Model::generate`
//! receives the transcript and the adapter is stateless across restarts by
//! design, so `providers` declares its own port here and the adapters implement
//! it over the per-session blob file. `factory::build_model` wires it in, the same
//! way it already wires in the `ToolRegistry`: `generate` is not given these
//! things.
//!
//! The trait lives in `providers` (not the core) because it is an I/O concern:
//! only the adapter's request builder reaches it.

use async_trait::async_trait;

use super::error::ProviderError;

/// The stored bytes and media type behind one attachment id.
///
/// Read-only: the model's request builder uses this to resolve ids to bytes.
#[async_trait]
pub trait ImageSource: Send + Sync {
    /// The stored bytes for one attachment id, with its media type.
    ///
    /// `Ok(None)` means the id is unknown (a deleted row or a corrupt
    /// database). The caller must fail the turn rather than drop the part: the
    /// user attached it, and quiet loss is the one failure neither the model
    /// nor the user can detect.
    async fn image(&self, id: &str) -> Result<Option<(String, Vec<u8>)>, ProviderError>;
}

/// An [`ImageSource`] that also accepts new images.
///
/// The ingestion handler writes through this; the model only reads through
/// [`ImageSource`]. Splitting the two keeps the write surface out of the
/// read-only port the provider sees.
#[async_trait]
pub trait ImageStore: ImageSource {
    /// Store one attachment under `id`. The caller persists the reference in the
    /// transcript's `ImageAttachment`.
    async fn store(
        &self,
        chat_session_id: &str,
        id: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ProviderError>;
}

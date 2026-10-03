//! Tree-sitter chunks, on-device embeddings, and a per-workspace sqlite-vec file.
//!
//! `robi-core` does not depend on this crate. The loop only sees a tool result.

mod chunk;
mod embed;
mod error;
mod fuse;
mod index;
mod store;

pub use chunk::{chunk_source, language_for_path, Chunk, ChunkKind, Language};
pub use embed::{Embedder, FakeEmbedder, MODEL_ID};
pub use error::IndexError;
pub use index::{Index, IndexState, IndexStatus};
pub use store::ChunkHit;

#[cfg(feature = "local-embed")]
pub use embed::LocalEmbedder;

pub fn index_db_path(home: &std::path::Path, workspace_id: &str) -> std::path::PathBuf {
    home.join("index").join(workspace_id).join("index.sqlite")
}

pub fn model_cache_dir(home: &std::path::Path) -> std::path::PathBuf {
    home.join("models")
}

/// Remember pause when no index task is running. Creates the file when pausing.
pub fn set_stored_pause(path: &std::path::Path, paused: bool) -> Result<(), IndexError> {
    if !path.exists() && !paused {
        return Ok(());
    }
    let connection = store::open_connection(path)?;
    store::ensure_schema(&connection, embed::DIMENSIONS)?;
    store::set_paused(&connection, paused)
}

pub fn stored_pause(path: &std::path::Path) -> bool {
    if !path.exists() {
        return false;
    }
    store::open_connection(path)
        .and_then(|connection| store::is_paused(&connection))
        .unwrap_or(false)
}

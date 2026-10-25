//! Session data outside the row, removed when the chat session row goes away.
//!
//! Two keepers, removed in order:
//!
//! - the session's blobs under `sessions/<id>/` in the app home;
//! - the session's sandbox container, if the container mechanism made one.
//!
//! See [`SessionDataCleaner`](crate::domain::chat_session::data::SessionDataCleaner).
//!
//! Plan directories are deliberately not here. Since S9 a plan is a markdown
//! directory in the **workspace** (`.robi/plans/<name>/`), so it is the user's
//! file and outlives the chat session that wrote it.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use robi_core::ids::SessionId;

use crate::{
    adapters::session_blobs::SessionBlobs,
    agent::sandbox::container,
    domain::chat_session::data::SessionDataCleaner,
};

/// How long the container remove may take before the delete carries on without it.
pub const RELEASE_TIMEOUT: Duration = Duration::from_secs(30);

/// Deletes `sessions/<id>/` and the session's sandbox container.
#[derive(Clone)]
pub struct SessionCleanup {
    data: Arc<dyn SessionDataCleaner>,
}

impl SessionCleanup {
    /// The shipped composition: filesystem blobs plus the process-wide sandbox
    /// registry.
    pub fn new(blobs: SessionBlobs) -> Self {
        Self {
            data: Arc::new(FilesystemSessionData::new(blobs)),
        }
    }

    /// Composes any inner cleaner, so a test can drive the order without a disk
    /// or a daemon.
    pub fn with_data(data: Arc<dyn SessionDataCleaner>) -> Self {
        Self { data }
    }
}

#[async_trait]
impl SessionDataCleaner for SessionCleanup {
    async fn remove_session_data(&self, session: SessionId) -> Result<(), String> {
        // First, and independently of the blobs: the container is the larger
        // leak, and a blob failure must not strand it. A session with no
        // container is a no-op, which is every session on the `os` mechanism.
        let released =
            tokio::time::timeout(RELEASE_TIMEOUT, container::Registry::global().release(session))
                .await;
        if released.is_err() {
            tracing::warn!(
                %session,
                "timed out removing the sandbox container; it stays until the daemon restarts"
            );
        }
        self.data.remove_session_data(session).await
    }
}

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Recorder {
        seen: Mutex<Vec<SessionId>>,
        fail: bool,
    }

    impl Recorder {
        fn new(fail: bool) -> Arc<Self> {
            Arc::new(Self {
                seen: Mutex::new(Vec::new()),
                fail,
            })
        }

        fn seen(&self) -> Vec<SessionId> {
            self.seen.lock().expect("recorder lock").clone()
        }
    }

    #[async_trait]
    impl SessionDataCleaner for Recorder {
        async fn remove_session_data(&self, session: SessionId) -> Result<(), String> {
            self.seen.lock().expect("recorder lock").push(session);
            if self.fail {
                return Err("blobs failed".into());
            }
            Ok(())
        }
    }

    // A session with no container is the common case, so it is the one that must
    // be a no-op rather than an error: the release tries the empty process-wide
    // registry and moves on.
    #[tokio::test]
    async fn a_session_without_a_container_still_removes_its_data() {
        let inner = Recorder::new(false);
        let cleanup = SessionCleanup::with_data(inner.clone());
        let session = SessionId::new();

        assert!(cleanup.remove_session_data(session).await.is_ok());
        assert_eq!(inner.seen(), vec![session]);
    }

    #[tokio::test]
    async fn an_inner_failure_is_reported_and_not_skipped() {
        let inner = Recorder::new(true);
        let cleanup = SessionCleanup::with_data(inner.clone());
        let session = SessionId::new();

        let error = cleanup.remove_session_data(session).await.unwrap_err();
        assert_eq!(error, "blobs failed");
        assert_eq!(inner.seen(), vec![session]);
    }
}

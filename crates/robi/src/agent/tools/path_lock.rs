//! One lock per absolute path, shared by every read-modify-write.
//!
//! The edit tools, the plan and todo tools, and the docs editor all take this
//! guard before they read a file they are about to write, so two callers cannot
//! interleave a read with someone else's write.
//!
//! Two locks, in this order:
//!
//! 1. An in-process mutex, so two tool calls in one turn serialize. Cheap and
//!    always present.
//! 2. A cross-process file lock under `~/.robi/locks/paths/`, taken only when the
//!    composition root has wired the lock adapter in with [`init`]. It waits
//!    `LOCK_TIMEOUT` and then refuses with [`ToolError::Locked`], which is what
//!    stops two terminals overwriting each other's read-modify-write and losing a
//!    write silently.
//!
//! The file lock is skipped when `init` was never called, which is the case in a
//! unit test of one tool. That is not a silent downgrade in a running process:
//! `bootstrap::build_app_state` is the only composition root, and it always calls
//! `init`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock};

use robi_core::error::ToolError;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::domain::chat_session::lease::{
    acquire_waiting, LeaseGuard, Locks, Resource, Scope, Wait,
};

static LOCKS: LazyLock<StdMutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// The cross-process locks, once the composition root has wired them in.
static CROSS_PROCESS: OnceLock<Arc<dyn Locks>> = OnceLock::new();

/// Give this module the lock adapter.
///
/// Called once by `bootstrap::build_app_state`. Later calls are ignored, so a
/// second app state in one process (a test) keeps the first set.
pub fn init(locks: Arc<dyn Locks>) {
    let _ = CROSS_PROCESS.set(locks);
}

/// Hold this across a read-modify-write of `path`.
#[derive(Debug)]
pub struct PathGuard {
    /// Released last: this process holds the file first, then lets its own
    /// callers in.
    _lease: Option<LeaseGuard>,
    _local: OwnedMutexGuard<()>,
}

/// Take both locks for `path`.
///
/// `Err(Locked)` means another process is editing the same file and the wait ran
/// out. The caller turns that into the tool's failure: the model reads a `locked`
/// payload and can wait, or pick another file.
pub async fn lock_path(path: &Path) -> Result<PathGuard, ToolError> {
    let lease = match CROSS_PROCESS.get() {
        Some(locks) => Some(
            acquire_waiting(
                locks.as_ref(),
                &Resource::path(path),
                Scope::Turn,
                Wait::edit(),
            )
            .await
            .map_err(|error| ToolError::Locked(error.message()))?,
        ),
        None => None,
    };
    let mutex = {
        let mut locks = LOCKS.lock().unwrap_or_else(|err| err.into_inner());
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let local = mutex.lock_owned().await;
    Ok(PathGuard {
        _lease: lease,
        _local: local,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::locks::FileLocks;
    use crate::domain::chat_session::lease::LOCK_TIMEOUT;
    use std::time::Duration;

    #[test]
    fn the_edit_wait_is_the_lease_timeout() {
        assert_eq!(Wait::edit(), Wait::UpTo(LOCK_TIMEOUT));
        assert_eq!(LOCK_TIMEOUT, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn the_in_process_lock_serializes_two_callers() {
        let path = Path::new("/workspace/serialized.txt");
        let first = lock_path(path).await.expect("the first lock");
        let waiting = tokio::spawn(async move {
            let _second = lock_path(Path::new("/workspace/serialized.txt")).await;
        });
        // The second caller cannot finish while the first holds it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        drop(first);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("the second lock lands")
            .expect("no panic");
    }

    #[tokio::test]
    async fn a_file_lock_refuses_when_another_process_holds_the_path() {
        // Set the cross-process adapter directly, because `OnceLock` cannot be
        // reset: a test that runs after this keeps the same home.
        let home =
            std::env::temp_dir().join(format!("robi-path-lock-{}", uuid::Uuid::now_v7().simple()));
        let path = Path::new("/workspace/contended.txt");
        let blocker = FileLocks::new(&home, "sqlite://path-lock.db");
        let _held = blocker
            .try_acquire(&Resource::path(path), Scope::Turn)
            .expect("the blocker takes it");
        init(Arc::new(FileLocks::new(&home, "sqlite://path-lock.db")));

        let error = lock_path(path).await.expect_err("held elsewhere");
        assert!(
            error.to_string().contains("another process"),
            "the refusal is a lock, not a generic failure: {error}"
        );
        assert_eq!(error.kind(), "locked");
        let _ = std::fs::remove_dir_all(&home);
    }
}

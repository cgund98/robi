//! One lock per absolute path, shared by the edit tools and review decisions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};

use tokio::sync::{Mutex, OwnedMutexGuard};

static LOCKS: LazyLock<StdMutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Hold this guard across a read-modify-write of `path`.
pub async fn lock_path(path: &Path) -> OwnedMutexGuard<()> {
    let mutex = {
        let mut locks = LOCKS.lock().unwrap_or_else(|err| err.into_inner());
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    mutex.lock_owned().await
}

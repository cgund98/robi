//! Filesystem reads that must not occupy an async worker.

/// Run `work` on Tokio's blocking pool.
///
/// A panic in `work` comes back as `Err`. The async worker stays free while
/// the read is in progress.
pub async fn call<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| format!("filesystem read failed: {err}"))
}

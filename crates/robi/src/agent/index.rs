//! One index task per workspace with an open surface.
//!
//! A surface is a chat session's event stream or a docs search. The task
//! starts on the first [`IndexHub::acquire`] and stops once every surface has
//! been closed for [`LINGER`], so a search that acquires and releases on each
//! request keeps a warm index instead of restarting the scan between queries.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use robi_core::ids::WorkspaceId;
use robi_index::{
    index_db_path, set_stored_pause, stored_pause, Embedder, Index, IndexState, IndexStatus,
};

use crate::domain::{
    events::{EventBus, EventEnvelope},
    workspace::assets::WorkspaceAssetCleaner,
};

/// How long an index task outlives its last surface. A docs search holds a
/// lease only for one request, so stopping at zero would tear the scan down
/// between queries.
const LINGER: Duration = Duration::from_secs(60);

struct Slot {
    index: Arc<Index>,
    users: usize,
    /// The stop check this slot is waiting for, if any. The counter is global,
    /// so a check left over from an earlier task can never match its
    /// successor: `None` matches nothing either.
    stop_token: Option<u64>,
}

pub struct IndexHub {
    home: PathBuf,
    embedder: Arc<dyn Embedder>,
    bus: Arc<EventBus>,
    slots: Mutex<HashMap<WorkspaceId, Slot>>,
    /// Delay before an idle task stops. [`LINGER`] in production, shorter in
    /// tests that watch the window pass.
    linger: Duration,
    next_stop_token: AtomicU64,
}

impl IndexHub {
    pub fn new(home: PathBuf, bus: Arc<EventBus>, embedder: Arc<dyn Embedder>) -> Self {
        Self::with_linger(home, bus, embedder, LINGER)
    }

    /// A hub with a custom idle delay.
    pub fn with_linger(
        home: PathBuf,
        bus: Arc<EventBus>,
        embedder: Arc<dyn Embedder>,
        linger: Duration,
    ) -> Self {
        Self {
            home,
            embedder,
            bus,
            slots: Mutex::new(HashMap::new()),
            linger,
            next_stop_token: AtomicU64::new(1),
        }
    }

    pub fn acquire(self: &Arc<Self>, id: WorkspaceId, root: PathBuf) -> IndexLease {
        let mut slots = self.slots.lock().expect("index slots");
        if let Some(slot) = slots.get_mut(&id) {
            slot.users += 1;
        } else {
            let bus = Arc::clone(&self.bus);
            let workspace = id.to_string();
            let reported_ready = Arc::new(AtomicU8::new(0));
            let index = Index::start(
                id,
                root.clone(),
                index_db_path(&self.home, &workspace),
                Arc::clone(&self.embedder),
                Arc::new(move |status| {
                    let ready = u8::from(status.state == IndexState::Ready);
                    if reported_ready.swap(ready, Ordering::Relaxed) != ready && ready == 1 {
                        tracing::info!(
                            workspace,
                            files = status.files_done,
                            "workspace index ready"
                        );
                    }
                    let data =
                        serde_json::to_value(&status).unwrap_or_else(|_| serde_json::json!({}));
                    bus.publish(EventEnvelope::index_progress(&workspace, data));
                }),
            );
            tracing::info!(%id, root = %root.display(), "workspace index started");
            slots.insert(
                id,
                Slot {
                    index,
                    users: 1,
                    stop_token: None,
                },
            );
        }
        IndexLease {
            hub: Arc::clone(self),
            id,
        }
    }

    /// Drops one surface. The task stops after [`Self::linger`] once the
    /// count reaches zero, so a lease taken per request does not restart the
    /// scan on the next one.
    pub fn release(self: &Arc<Self>, id: WorkspaceId) {
        let token = self.next_stop_token.fetch_add(1, Ordering::Relaxed);
        {
            let mut slots = self.slots.lock().expect("index slots");
            let Some(slot) = slots.get_mut(&id) else {
                return;
            };
            slot.users = slot.users.saturating_sub(1);
            if slot.users > 0 {
                return;
            }
            // Supersedes any check scheduled by an earlier release: only the
            // newest token can stop this slot.
            slot.stop_token = Some(token);
        }
        let linger = self.linger;
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // Nowhere to sleep: fall back to stopping at once.
            self.stop_if_idle(id, token);
            return;
        };
        let hub = Arc::clone(self);
        // An absolute deadline, taken now: a task that starts late must not
        // stretch the window by the time it was first polled.
        let deadline = tokio::time::Instant::now() + linger;
        handle.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            hub.stop_if_idle(id, token);
        });
    }

    /// Stops a task that is still idle: no surface, and still waiting on the
    /// check `token` it was scheduled with.
    fn stop_if_idle(&self, id: WorkspaceId, token: u64) {
        let mut slots = self.slots.lock().expect("index slots");
        let idle = slots
            .get(&id)
            .is_some_and(|slot| slot.users == 0 && slot.stop_token == Some(token));
        if !idle {
            return;
        }
        if let Some(slot) = slots.remove(&id) {
            slot.index.stop();
        }
    }

    /// Publish the current status so a subscriber that just connected sees it.
    pub fn publish_status(&self, id: WorkspaceId) {
        if let Some(index) = self.index(id) {
            index.report();
            return;
        }
        let status = self.status(id);
        let data = serde_json::to_value(&status).unwrap_or_else(|_| serde_json::json!({}));
        self.bus
            .publish(EventEnvelope::index_progress(&id.to_string(), data));
    }

    pub fn status(&self, id: WorkspaceId) -> IndexStatus {
        if let Some(index) = self.index(id) {
            return index.status();
        }
        if stored_pause(&self.db_path(id)) {
            return IndexStatus {
                state: IndexState::Paused,
                files_done: 0,
                files_total: 0,
                error: None,
            };
        }
        IndexStatus {
            state: IndexState::Ready,
            files_done: 0,
            files_total: 0,
            error: None,
        }
    }

    pub fn set_paused(&self, id: WorkspaceId, paused: bool) -> Result<(), robi_index::IndexError> {
        if let Some(index) = self.index(id) {
            if paused {
                index.pause();
            } else {
                index.resume();
            }
            return Ok(());
        }
        set_stored_pause(&self.db_path(id), paused)
    }

    pub fn index(&self, id: WorkspaceId) -> Option<Arc<Index>> {
        self.slots
            .lock()
            .expect("index slots")
            .get(&id)
            .map(|slot| Arc::clone(&slot.index))
    }

    pub fn remove_files(&self, id: WorkspaceId) {
        if let Some(slot) = self.slots.lock().expect("index slots").remove(&id) {
            slot.index.stop();
        }
        let path = self.db_path(id);
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn db_path(&self, id: WorkspaceId) -> PathBuf {
        index_db_path(&self.home, &id.to_string())
    }
}

#[async_trait::async_trait]
impl WorkspaceAssetCleaner for IndexHub {
    async fn remove_workspace_assets(&self, id: WorkspaceId) -> Result<(), String> {
        self.remove_files(id);
        Ok(())
    }
}

/// Drops one surface when it closes: a session stream that ends, or a docs
/// search request that finishes. The task stops once the last one has been
/// gone for [`LINGER`].
pub struct IndexLease {
    hub: Arc<IndexHub>,
    id: WorkspaceId,
}

impl Drop for IndexLease {
    fn drop(&mut self) {
        self.hub.release(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with(linger: Duration) -> (Arc<IndexHub>, PathBuf) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "robi-index-hub-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let hub = Arc::new(IndexHub::with_linger(
            root.clone(),
            Arc::new(EventBus::new()),
            Arc::new(robi_index::FakeEmbedder::new(4)),
            linger,
        ));
        (hub, root)
    }

    #[tokio::test]
    async fn a_released_task_stops_after_the_linger() {
        let (hub, root) = hub_with(Duration::from_millis(300));
        let id = WorkspaceId::new();
        let lease = hub.acquire(id, root.clone());
        assert!(hub.index(id).is_some());
        drop(lease);
        assert!(
            hub.index(id).is_some(),
            "the task outlives its last surface for the linger"
        );
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert!(hub.index(id).is_none(), "stopped once the window passed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_reacquire_within_the_window_keeps_the_task() {
        let (hub, root) = hub_with(Duration::from_millis(500));
        let id = WorkspaceId::new();
        let lease = hub.acquire(id, root.clone());
        drop(lease);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let again = hub.acquire(id, root.clone());
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert!(
            hub.index(id).is_some(),
            "a surface inside the window cancels the stop"
        );
        drop(again);
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert!(hub.index(id).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_stale_check_does_not_stop_a_newer_task() {
        let (hub, root) = hub_with(Duration::from_millis(500));
        let id = WorkspaceId::new();
        let first = hub.acquire(id, root.clone());
        drop(first); // stop check A lands at +500ms
        tokio::time::sleep(Duration::from_millis(200)).await;
        let second = hub.acquire(id, root.clone());
        drop(second); // check B supersedes A at +700ms
        tokio::time::sleep(Duration::from_millis(400)).await; // t=600: A has fired
        assert!(
            hub.index(id).is_some(),
            "A fired past its deadline and must not stop the task B is waiting on"
        );
        tokio::time::sleep(Duration::from_millis(400)).await; // t=1000 > B
        assert!(hub.index(id).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_files_deletes_the_index_directory() {
        let (hub, root) = hub_with(Duration::from_secs(60));
        let id = WorkspaceId::new();
        let dir = root.join("index").join(id.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.sqlite"), b"db").unwrap();
        hub.remove_files(id);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

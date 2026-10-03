//! One index task per workspace that has an open chat session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use robi_core::ids::WorkspaceId;
use robi_index::{
    index_db_path, set_stored_pause, stored_pause, Embedder, Index, IndexState, IndexStatus,
};

use crate::domain::events::{EventEnvelope, EventFanOut};

struct Slot {
    index: Arc<Index>,
    users: usize,
}

pub struct IndexHub {
    home: PathBuf,
    embedder: Arc<dyn Embedder>,
    fanout: Arc<EventFanOut>,
    slots: Mutex<HashMap<WorkspaceId, Slot>>,
}

impl IndexHub {
    pub fn new(home: PathBuf, fanout: Arc<EventFanOut>, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            home,
            embedder,
            fanout,
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub fn acquire(self: &Arc<Self>, id: WorkspaceId, root: PathBuf) -> IndexLease {
        let mut slots = self.slots.lock().expect("index slots");
        if let Some(slot) = slots.get_mut(&id) {
            slot.users += 1;
        } else {
            let fanout = Arc::clone(&self.fanout);
            let workspace = id.to_string();
            let index = Index::start(
                id,
                root,
                index_db_path(&self.home, &workspace),
                Arc::clone(&self.embedder),
                Arc::new(move |status| {
                    let data =
                        serde_json::to_value(&status).unwrap_or_else(|_| serde_json::json!({}));
                    fanout.publish(EventEnvelope::index_progress(&workspace, data));
                }),
            );
            slots.insert(id, Slot { index, users: 1 });
        }
        IndexLease {
            hub: Arc::clone(self),
            id,
        }
    }

    pub fn release(&self, id: WorkspaceId) {
        let mut slots = self.slots.lock().expect("index slots");
        let Some(slot) = slots.get_mut(&id) else {
            return;
        };
        slot.users = slot.users.saturating_sub(1);
        if slot.users == 0 {
            if let Some(slot) = slots.remove(&id) {
                slot.index.stop();
            }
        }
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

/// Drops the workspace's index task when the last session stream closes.
pub struct IndexLease {
    hub: Arc<IndexHub>,
    id: WorkspaceId,
}

impl Drop for IndexLease {
    fn drop(&mut self) {
        self.hub.release(self.id);
    }
}

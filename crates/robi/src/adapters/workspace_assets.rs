//! The workspace asset cleaner for the semantic index, without an index.
//!
//! Deleting a workspace must still drop `~/.robi/index/<workspace_id>/`, and the
//! default build has no `IndexHub` and no `robi-index` crate to ask. The path is
//! therefore built here, from the two facts that make it: the settings directory
//! and the workspace id.

use std::path::PathBuf;

use async_trait::async_trait;
use robi_core::ids::WorkspaceId;

use crate::domain::workspace::assets::WorkspaceAssetCleaner;

/// Removes one workspace's index directory.
pub struct WorkspaceIndexAssets {
    /// `~/.robi`. The index lives at `<home>/index/<workspace_id>/index.sqlite`,
    /// matching `robi_index::index_db_path`.
    home: PathBuf,
}

impl WorkspaceIndexAssets {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }
}

#[async_trait]
impl WorkspaceAssetCleaner for WorkspaceIndexAssets {
    async fn remove_workspace_assets(&self, id: WorkspaceId) -> Result<(), String> {
        let dir = self.home.join("index").join(id.to_string());
        if !dir.exists() {
            return Ok(());
        }
        let display = dir.display().to_string();
        crate::agent::blocking::call(move || {
            std::fs::remove_dir_all(&dir).map_err(|err| err.to_string())
        })
        .await
        .map_err(|_| format!("failed to remove {display}"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deleting_a_workspace_removes_its_index_directory() {
        let home = std::env::temp_dir().join(format!("robi-assets-{}", WorkspaceId::new()));
        let id = WorkspaceId::new();
        let dir = home.join("index").join(id.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.sqlite"), b"db").unwrap();
        // A sibling workspace's directory must survive.
        let other = WorkspaceId::new();
        let other_dir = home.join("index").join(other.to_string());
        std::fs::create_dir_all(&other_dir).unwrap();

        WorkspaceIndexAssets::new(home.clone())
            .remove_workspace_assets(id)
            .await
            .unwrap();

        assert!(!dir.exists());
        assert!(other_dir.exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}

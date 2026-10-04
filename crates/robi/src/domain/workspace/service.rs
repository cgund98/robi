use std::sync::Arc;

use robi_core::ids::WorkspaceId;

use crate::domain::{
    error::ServiceError,
    workspace::{
        assets::WorkspaceAssetCleaner,
        model::{OpenedWorkspace, Workspace},
        repo::WorkspaceRepository,
    },
};

pub struct WorkspaceService {
    pub repository: Arc<dyn WorkspaceRepository>,
    /// Removes derived files such as the semantic index. Absent in tests.
    /// Best effort: a failure is logged and the delete still succeeds, because
    /// the row is already gone.
    pub asset_cleaner: Option<Arc<dyn WorkspaceAssetCleaner>>,
}

impl WorkspaceService {
    /// Open `root`. The same canonical directory returns the existing row.
    pub async fn open_workspace(&self, root: &str) -> Result<OpenedWorkspace, ServiceError> {
        let root = root.trim();
        if root.is_empty() {
            return Err(ServiceError::BadRequest("root is required".to_string()));
        }

        let canonical = self.repository.canonicalize_root(root).await?;
        if let Some(workspace) = self.repository.get_workspace_by_root(&canonical).await? {
            return Ok(OpenedWorkspace {
                workspace,
                created: false,
            });
        }

        let name = workspace_name(&canonical);
        match self.repository.insert_workspace(&canonical, &name).await {
            Ok(workspace) => Ok(OpenedWorkspace {
                workspace,
                created: true,
            }),
            Err(ServiceError::Conflict(_)) => {
                let workspace = self
                    .repository
                    .get_workspace_by_root(&canonical)
                    .await?
                    .ok_or(ServiceError::Unknown)?;
                Ok(OpenedWorkspace {
                    workspace,
                    created: false,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub async fn get_workspace(&self, id: WorkspaceId) -> Result<Workspace, ServiceError> {
        self.repository
            .get_workspace(id)
            .await?
            .ok_or_else(|| ServiceError::NotFound(id.to_string()))
    }

    pub async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
        self.repository.list_workspaces().await
    }

    pub async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
        self.repository.delete_workspace(id).await?;
        if let Some(cleaner) = &self.asset_cleaner {
            if let Err(err) = cleaner.remove_workspace_assets(id).await {
                tracing::warn!(workspace = %id, %err, "failed to remove workspace assets");
            }
        }
        Ok(())
    }
}

/// The menu label is the last path component. A root with none keeps the path.
fn workspace_name(root: &str) -> String {
    std::path::Path::new(root)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(root)
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::{Duration, Utc};

    use super::*;
    use crate::domain::workspace::assets::WorkspaceAssetCleaner;

    struct FakeRepo {
        canonical: Mutex<HashMap<String, Result<String, ServiceError>>>,
        by_root: Mutex<HashMap<String, Workspace>>,
        inserts: Mutex<u32>,
        canonicalize_calls: Mutex<u32>,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self {
                canonical: Mutex::new(HashMap::new()),
                by_root: Mutex::new(HashMap::new()),
                inserts: Mutex::new(0),
                canonicalize_calls: Mutex::new(0),
            }
        }

        fn resolve(&self, input: &str, canonical: Result<&str, ServiceError>) {
            self.canonical
                .lock()
                .expect("canonical")
                .insert(input.to_string(), canonical.map(str::to_string));
        }
    }

    #[async_trait]
    impl WorkspaceRepository for FakeRepo {
        async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
            *self.canonicalize_calls.lock().expect("calls") += 1;
            self.canonical
                .lock()
                .expect("canonical")
                .get(root)
                .cloned()
                .unwrap_or_else(|| Ok(root.to_string()))
        }

        async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, ServiceError> {
            Ok(self
                .by_root
                .lock()
                .expect("by_root")
                .values()
                .find(|workspace| workspace.id == id)
                .cloned())
        }

        async fn get_workspace_by_root(
            &self,
            root: &str,
        ) -> Result<Option<Workspace>, ServiceError> {
            Ok(self.by_root.lock().expect("by_root").get(root).cloned())
        }

        async fn insert_workspace(
            &self,
            root: &str,
            name: &str,
        ) -> Result<Workspace, ServiceError> {
            *self.inserts.lock().expect("inserts") += 1;
            if self.by_root.lock().expect("by_root").contains_key(root) {
                return Err(ServiceError::Conflict(root.to_string()));
            }
            let workspace = Workspace {
                id: WorkspaceId::new(),
                name: name.to_string(),
                root: root.to_string(),
                mcp_project_sha256: None,
                created_at: Utc::now(),
            };
            self.by_root
                .lock()
                .expect("by_root")
                .insert(root.to_string(), workspace.clone());
            Ok(workspace)
        }

        async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
            let mut workspaces: Vec<_> = self
                .by_root
                .lock()
                .expect("by_root")
                .values()
                .cloned()
                .collect();
            workspaces.sort_by(|left, right| {
                right
                    .created_at
                    .cmp(&left.created_at)
                    .then(right.id.cmp(&left.id))
            });
            Ok(workspaces)
        }

        async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
            let mut rows = self.by_root.lock().expect("by_root");
            let Some(root) = rows
                .iter()
                .find(|(_, workspace)| workspace.id == id)
                .map(|(root, _)| root.clone())
            else {
                return Err(ServiceError::NotFound(id.to_string()));
            };
            rows.remove(&root);
            Ok(())
        }

        async fn set_mcp_project_sha256(
            &self,
            _id: WorkspaceId,
            _hash: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    fn service(repo: Arc<FakeRepo>) -> WorkspaceService {
        WorkspaceService {
            repository: repo,
            asset_cleaner: None,
        }
    }

    #[tokio::test]
    async fn open_workspace_stores_the_directory_name() {
        let repo = Arc::new(FakeRepo::new());
        repo.resolve("/work/robi", Ok("/work/robi"));
        let opened = service(Arc::clone(&repo))
            .open_workspace("/work/robi")
            .await
            .unwrap();

        assert!(opened.created);
        assert_eq!(opened.workspace.name, "robi");
        assert_eq!(opened.workspace.root, "/work/robi");
        assert_eq!(*repo.inserts.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn open_workspace_is_idempotent_for_the_same_root() {
        let repo = Arc::new(FakeRepo::new());
        repo.resolve("/work/robi", Ok("/canonical/robi"));
        repo.resolve("/canonical/robi", Ok("/canonical/robi"));
        let service = service(Arc::clone(&repo));
        let first = service.open_workspace("/work/robi").await.unwrap();
        let second = service.open_workspace("/canonical/robi").await.unwrap();

        assert!(!second.created);
        assert_eq!(first.workspace.id, second.workspace.id);
        assert_eq!(*repo.inserts.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn open_workspace_rejects_a_missing_path_before_insert() {
        let repo = Arc::new(FakeRepo::new());
        repo.resolve(
            "/missing",
            Err(ServiceError::BadRequest(
                "root does not exist: /missing".into(),
            )),
        );
        let error = service(Arc::clone(&repo))
            .open_workspace("/missing")
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ServiceError::BadRequest("root does not exist: /missing".into())
        );
        assert_eq!(*repo.inserts.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn open_workspace_rejects_an_empty_root_before_canonicalize() {
        let repo = Arc::new(FakeRepo::new());
        let error = service(Arc::clone(&repo))
            .open_workspace("   ")
            .await
            .unwrap_err();

        assert_eq!(error, ServiceError::BadRequest("root is required".into()));
        assert_eq!(*repo.canonicalize_calls.lock().unwrap(), 0);
        assert_eq!(*repo.inserts.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn get_workspace_is_not_found_when_missing() {
        let service = service(Arc::new(FakeRepo::new()));
        let id = WorkspaceId::new();
        assert_eq!(
            service.get_workspace(id).await.unwrap_err(),
            ServiceError::NotFound(id.to_string())
        );
    }

    #[tokio::test]
    async fn list_workspaces_orders_by_created_at_then_id() {
        let repo = Arc::new(FakeRepo::new());
        let service = service(Arc::clone(&repo));
        let older = service.open_workspace("/work/older").await.unwrap();
        let newer = service.open_workspace("/work/newer").await.unwrap();
        {
            let mut rows = repo.by_root.lock().unwrap();
            rows.get_mut("/work/older").unwrap().created_at = Utc::now() - Duration::seconds(10);
            rows.get_mut("/work/newer").unwrap().created_at = Utc::now();
            let _ = (older, newer);
        }

        let listed = service.list_workspaces().await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|workspace| workspace.name.as_str())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
    }

    #[tokio::test]
    async fn delete_workspace_is_not_found_when_missing() {
        let service = service(Arc::new(FakeRepo::new()));
        let id = WorkspaceId::new();
        assert_eq!(
            service.delete_workspace(id).await.unwrap_err(),
            ServiceError::NotFound(id.to_string())
        );
    }

    struct FakeCleaner {
        removed: Mutex<Vec<WorkspaceId>>,
        fail: bool,
    }

    #[async_trait]
    impl WorkspaceAssetCleaner for FakeCleaner {
        async fn remove_workspace_assets(&self, id: WorkspaceId) -> Result<(), String> {
            self.removed.lock().expect("cleaner").push(id);
            if self.fail {
                return Err("cleaner failed".into());
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn delete_workspace_removes_derived_assets() {
        let repo = Arc::new(FakeRepo::new());
        let opened = service(Arc::clone(&repo))
            .open_workspace("/work/robi")
            .await
            .unwrap();
        let cleaner = Arc::new(FakeCleaner {
            removed: Mutex::new(Vec::new()),
            fail: false,
        });
        let service = WorkspaceService {
            repository: repo,
            asset_cleaner: Some(cleaner.clone()),
        };
        service.delete_workspace(opened.workspace.id).await.unwrap();
        assert_eq!(
            cleaner.removed.lock().unwrap().as_slice(),
            &[opened.workspace.id]
        );
    }

    #[tokio::test]
    async fn delete_workspace_succeeds_when_asset_cleanup_fails() {
        let repo = Arc::new(FakeRepo::new());
        let opened = service(Arc::clone(&repo))
            .open_workspace("/work/robi")
            .await
            .unwrap();
        let service = WorkspaceService {
            repository: repo,
            asset_cleaner: Some(Arc::new(FakeCleaner {
                removed: Mutex::new(Vec::new()),
                fail: true,
            })),
        };
        service.delete_workspace(opened.workspace.id).await.unwrap();
        assert!(service.get_workspace(opened.workspace.id).await.is_err());
    }

    #[tokio::test]
    async fn delete_workspace_skips_assets_when_the_row_is_missing() {
        let cleaner = Arc::new(FakeCleaner {
            removed: Mutex::new(Vec::new()),
            fail: false,
        });
        let service = WorkspaceService {
            repository: Arc::new(FakeRepo::new()),
            asset_cleaner: Some(cleaner.clone()),
        };
        let id = WorkspaceId::new();
        assert!(service.delete_workspace(id).await.is_err());
        assert!(cleaner.removed.lock().unwrap().is_empty());
    }

    /// The pre-check misses a row that insert then rejects. The follow-up read
    /// returns that row, so two openers still share one workspace.
    struct RaceRepo {
        workspace: Workspace,
        gets: Mutex<u32>,
    }

    #[async_trait]
    impl WorkspaceRepository for RaceRepo {
        async fn canonicalize_root(&self, root: &str) -> Result<String, ServiceError> {
            Ok(root.to_string())
        }

        async fn get_workspace(&self, _id: WorkspaceId) -> Result<Option<Workspace>, ServiceError> {
            Ok(None)
        }

        async fn get_workspace_by_root(
            &self,
            root: &str,
        ) -> Result<Option<Workspace>, ServiceError> {
            let mut gets = self.gets.lock().expect("gets");
            *gets += 1;
            if *gets == 1 {
                return Ok(None);
            }
            assert_eq!(root, self.workspace.root);
            Ok(Some(self.workspace.clone()))
        }

        async fn insert_workspace(
            &self,
            root: &str,
            _name: &str,
        ) -> Result<Workspace, ServiceError> {
            Err(ServiceError::Conflict(root.to_string()))
        }

        async fn list_workspaces(&self) -> Result<Vec<Workspace>, ServiceError> {
            Ok(Vec::new())
        }

        async fn delete_workspace(&self, id: WorkspaceId) -> Result<(), ServiceError> {
            Err(ServiceError::NotFound(id.to_string()))
        }

        async fn set_mcp_project_sha256(
            &self,
            _id: WorkspaceId,
            _hash: Option<String>,
        ) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_duplicate_insert_returns_the_existing_row() {
        let workspace = Workspace {
            id: WorkspaceId::new(),
            name: "robi".into(),
            root: "/work/robi".into(),
            mcp_project_sha256: None,
            created_at: Utc::now(),
        };
        let service = WorkspaceService {
            repository: Arc::new(RaceRepo {
                workspace: workspace.clone(),
                gets: Mutex::new(0),
            }),
            asset_cleaner: None,
        };
        let opened = service.open_workspace("/work/robi").await.unwrap();
        assert!(!opened.created);
        assert_eq!(opened.workspace, workspace);
    }
}

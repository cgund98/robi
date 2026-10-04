//! Plan files on disk for a chat session.
//!
//! `write_plan` and `todos` create markdown under
//! `~/.robi/plans/<session_id>`. The `chat_sessions.plan_path` column points at
//! one of them, and it cascades away with the row; these files do not, so
//! deleting a session removes this directory too. See
//! [`SessionPlanCleaner`](crate::domain::chat_session::plans::SessionPlanCleaner).

use std::path::PathBuf;

use async_trait::async_trait;
use robi_core::ids::SessionId;

use crate::{
    adapters::session_blobs::SessionBlobs,
    agent::{tools::plan_file::session_plan_directory, workspace::user_home},
    domain::chat_session::plans::SessionPlanCleaner,
};

/// Deletes `~/.robi/plans/<session_id>` and, when set, the session blob directory.
///
/// `home` is resolved once from `$HOME`. A test can pin it with
/// [`FilesystemSessionPlans::with_home`] so it never touches the real home.
pub struct FilesystemSessionPlans {
    home: PathBuf,
    blobs: Option<SessionBlobs>,
}

impl FilesystemSessionPlans {
    /// Clean plans under `home` instead of the environment's home directory.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            blobs: None,
        }
    }

    /// Also delete `sessions/<id>/` when the chat session row goes away.
    pub fn with_blobs(mut self, blobs: SessionBlobs) -> Self {
        self.blobs = Some(blobs);
        self
    }
}

impl Default for FilesystemSessionPlans {
    fn default() -> Self {
        Self {
            home: user_home().unwrap_or_default(),
            blobs: None,
        }
    }
}

#[async_trait]
impl SessionPlanCleaner for FilesystemSessionPlans {
    async fn remove_session_plans(&self, session: SessionId) -> Result<(), String> {
        if self.home.as_os_str().is_empty() {
            // No home to look under. Nothing was written this run, so nothing
            // to remove.
            return Ok(());
        }
        let directory = session_plan_directory(&self.home, session);
        let plans = match std::fs::remove_dir_all(&directory) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("remove {}: {err}", directory.display())),
        };
        if let Some(blobs) = &self.blobs {
            blobs.remove_session(session)?;
        }
        plans
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    /// A session directory under a throwaway home, removed on drop.
    struct TempHome(PathBuf);

    impl TempHome {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("robi-plans-{}", Uuid::now_v7().simple())))
        }

        fn write_plan(&self, session: SessionId) -> PathBuf {
            let directory = session_plan_directory(&self.0, session);
            std::fs::create_dir_all(&directory).unwrap();
            let file = directory.join("ship.md");
            std::fs::write(&file, "# Ship\n").unwrap();
            file
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn removes_this_sessions_directory_and_leaves_a_neighbour() {
        let home = TempHome::new();
        let mine = SessionId::new();
        let other = SessionId::new();
        let file = home.write_plan(mine);
        home.write_plan(other);

        let cleaner = FilesystemSessionPlans::with_home(home.0.clone());
        cleaner.remove_session_plans(mine).await.unwrap();

        assert!(!file.exists());
        assert!(!session_plan_directory(&home.0, mine).exists());
        assert!(session_plan_directory(&home.0, other).exists());
    }

    #[tokio::test]
    async fn a_missing_directory_is_ok() {
        let home = TempHome::new();
        let cleaner = FilesystemSessionPlans::with_home(home.0.clone());
        cleaner
            .remove_session_plans(SessionId::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_empty_home_is_ok() {
        FilesystemSessionPlans::with_home(PathBuf::new())
            .remove_session_plans(SessionId::new())
            .await
            .unwrap();
    }
}

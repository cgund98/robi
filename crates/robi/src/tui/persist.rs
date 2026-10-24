//! `~/.robi/tui.json`: the view state that survives a restart.
//!
//! Not the settings store. Settings are product configuration; this is which
//! workspace and session the terminal had open. Mixing them would put
//! `open_session_id` in the settings UI.
//!
//! Nothing here is required to start. A missing or malformed file starts at
//! the defaults, because a bad scroll offset must not stop the app opening.

use std::io;
use std::path::{Path, PathBuf};

use robi_core::ids::{SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// The file name under `~/.robi`.
pub const FILE: &str = "tui.json";

/// The schema version this build writes.
pub const VERSION: u32 = 1;

/// What the terminal remembers between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewState {
    /// Written for a later migration. A higher version is still read: the two
    /// ids are the whole payload, and dropping them would lose the session.
    #[serde(default = "version")]
    pub version: u32,
    /// The workspace on screen.
    #[serde(default)]
    pub active_workspace_id: Option<WorkspaceId>,
    /// The session on screen.
    #[serde(default)]
    pub open_session_id: Option<SessionId>,
}

fn version() -> u32 {
    VERSION
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            version: VERSION,
            active_workspace_id: None,
            open_session_id: None,
        }
    }
}

/// `~/.robi/tui.json`.
pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join(FILE)
}

/// Read the view state. A missing, unreadable, or malformed file is the
/// defaults.
pub fn load(state_dir: &Path) -> ViewState {
    let path = path(state_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ViewState::default();
    };
    match serde_json::from_str(&text) {
        Ok(view) => view,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "could not read the view state");
            ViewState::default()
        }
    }
}

/// Write the view state through a temporary file and a rename.
///
/// The rename is the commit: a reader either sees the previous file or the
/// next one, never a half-written line.
pub fn save(state_dir: &Path, view: &ViewState) -> io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let target = path(state_dir);
    let temporary = target.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(view).map_err(io::Error::other)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &target)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn state_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("robi-tui-persist-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    #[test]
    fn both_ids_round_trip() {
        let dir = state_dir();
        let view = ViewState {
            version: VERSION,
            active_workspace_id: Some(WorkspaceId::new()),
            open_session_id: Some(SessionId::new()),
        };
        save(&dir, &view).expect("a write");
        assert_eq!(load(&dir), view);
        assert!(
            !path(&dir).with_extension("json.tmp").exists(),
            "the temporary file is renamed, not left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_starts_at_the_defaults() {
        let dir = state_dir();
        assert_eq!(load(&dir), ViewState::default());
        assert_eq!(ViewState::default(), ViewState::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_file_starts_at_the_defaults() {
        let dir = state_dir();
        std::fs::write(path(&dir), b"{ this is not json").expect("a write");
        assert_eq!(load(&dir), ViewState::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_key_and_a_missing_version_still_load() {
        let dir = state_dir();
        let session = SessionId::new();
        std::fs::write(
            path(&dir),
            format!("{{ \"open_session_id\": \"{session}\", \"future\": 1 }}"),
        )
        .expect("a write");
        let view = load(&dir);
        assert_eq!(view.open_session_id, Some(session));
        assert_eq!(view.active_workspace_id, None);
        assert_eq!(view.version, VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

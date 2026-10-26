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

use std::collections::BTreeSet;

use robi_core::ids::{SessionId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// The file name under `~/.robi`.
pub const FILE: &str = "tui.json";

/// The schema version this build writes.
///
/// 2 added `expanded`; 3 added `recent_workspace_ids`. An older file still
/// loads: every field is `serde(default)`, so a missing set or list starts
/// empty.
pub const VERSION: u32 = 3;

/// What the terminal remembers between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewState {
    /// Written for a later migration. A higher version is still read: the ids
    /// and the expansion set are the whole payload, and dropping them would
    /// lose the session or the reader's place in it.
    #[serde(default = "version")]
    pub version: u32,
    /// The workspace on screen.
    #[serde(default)]
    pub active_workspace_id: Option<WorkspaceId>,
    /// The session on screen.
    #[serde(default)]
    pub open_session_id: Option<SessionId>,
    /// The tool cards the reader opened, by call id.
    ///
    /// One key per call — the terminal card is one band, so the GUI's separate
    /// `:diff` and `:answer` keys have nothing to address. A call id that is no
    /// longer in a transcript is kept: reopening the session restores it.
    #[serde(default)]
    pub expanded: BTreeSet<String>,
    /// Workspaces, most recently selected first.
    ///
    /// The GUI keeps `robi.recentWorkspaceIds` in local storage for the same
    /// reason: the switcher lists the eight most recent, and the service only
    /// knows creation order. Unbounded, like the GUI's list; a stale id is
    /// ignored by the ordering and dropped the next time the file is written.
    #[serde(default)]
    pub recent_workspace_ids: Vec<WorkspaceId>,
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
            expanded: BTreeSet::new(),
            recent_workspace_ids: Vec::new(),
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
    // One temp name per process. The name was constant, which was safe only while
    // one terminal ran at a time: two processes racing a rename onto the same
    // temp file can leave a half-written `tui.json` or lose a save. The rename is
    // still what commits, so a reader sees the previous file or the next one.
    let temporary = target.with_extension(format!("{}.json.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(view).map_err(io::Error::other)?;
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &target)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn state_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("robi-persist-{}", Uuid::now_v7().simple()));
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
            expanded: ["one call".to_owned(), "another call".to_owned()]
                .into_iter()
                .collect(),
            recent_workspace_ids: vec![WorkspaceId::new(), WorkspaceId::new()],
        };
        save(&dir, &view).expect("a write");
        assert_eq!(load(&dir), view);
        assert!(
            !dir.join(format!("tui.{}.tmp", std::process::id())).exists(),
            "the temporary file is renamed, not left behind"
        );
        let leftovers = std::fs::read_dir(&dir)
            .expect("the directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "and two processes cannot share one temp name");
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
    fn a_version_one_file_loads_with_no_card_open() {
        // What T1 wrote: the two ids, no expansion set.
        let dir = state_dir();
        let session = SessionId::new();
        std::fs::write(
            path(&dir),
            format!("{{ \"version\": 1, \"open_session_id\": \"{session}\" }}"),
        )
        .expect("a write");
        let view = load(&dir);
        assert_eq!(view.open_session_id, Some(session));
        assert!(view.expanded.is_empty(), "T1 opened no cards");
        assert!(view.recent_workspace_ids.is_empty(), "T1 had no recents");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_two_file_loads_with_no_recents() {
        // What T2 wrote: the expansion set, no recents.
        let dir = state_dir();
        let session = SessionId::new();
        std::fs::write(
            path(&dir),
            format!(
                "{{ \"version\": 2, \"open_session_id\": \"{session}\", \"expanded\": [\"a\"] }}"
            ),
        )
        .expect("a write");
        let view = load(&dir);
        assert_eq!(view.open_session_id, Some(session));
        assert_eq!(view.expanded.len(), 1);
        assert!(view.recent_workspace_ids.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_recent_workspace_is_kept_and_simply_ignored() {
        let dir = state_dir();
        let gone = WorkspaceId::new();
        let view = ViewState {
            recent_workspace_ids: vec![gone],
            ..ViewState::default()
        };
        save(&dir, &view).expect("a write");
        assert_eq!(load(&dir).recent_workspace_ids, vec![gone]);
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

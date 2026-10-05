//! One language server per workspace root. Sessions on that root share it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ignore::WalkBuilder;
use tokio::sync::Mutex;

use crate::agent::workspace::PathFilter;

use super::catalog::{self, ServerSpec};
use super::client::{self, LspClient};
use super::Timing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    NoServer,
    /// Startup failed. The string is the server's stderr, when it wrote any.
    ServerFailed(String),
    Timeout,
}

struct Slot {
    crashes: u8,
    failed: bool,
    /// A missing binary is logged once, until this process restarts.
    missing_logged: bool,
    /// Why the last start failed. Kept after the server is marked failed.
    detail: String,
    client: Option<Arc<LspClient>>,
}

type SlotGate = Arc<Mutex<Slot>>;
type Slots = HashMap<(PathBuf, &'static str), SlotGate>;
type Resolve = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

pub struct LspHub {
    slots: Mutex<Slots>,
    resolve: Resolve,
    pub(crate) timing: Timing,
}

impl LspHub {
    pub fn new() -> Arc<Self> {
        Self::build(Timing::default(), Arc::new(catalog::find_on_path))
    }

    pub fn build(timing: Timing, resolve: Resolve) -> Arc<Self> {
        let hub = Arc::new(Self {
            slots: Mutex::new(HashMap::new()),
            resolve,
            timing,
        });
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let weak = Arc::downgrade(&hub);
            runtime.spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let Some(hub) = weak.upgrade() else { break };
                    hub.sweep().await;
                }
            });
        }
        hub
    }

    /// A running server, starting one when the binary is on `PATH`.
    pub async fn client(
        &self,
        root: &Path,
        spec: &'static ServerSpec,
    ) -> Result<Arc<LspClient>, Unavailable> {
        let root = canonical(root);
        let gate = self.gate(&root, spec.id).await;
        let mut slot = gate.lock().await;
        if slot.failed {
            return Err(Unavailable::ServerFailed(slot.detail.clone()));
        }
        if let Some(client) = &slot.client {
            if client.is_running() {
                return Ok(Arc::clone(client));
            }
            let idle = client.idle_stop();
            slot.client = None;
            if idle {
                tracing::info!(server = spec.id, "language server stopped after idle");
            } else {
                slot.crashes += 1;
                tracing::warn!(
                    server = spec.id,
                    crashes = slot.crashes,
                    "language server exited"
                );
                if slot.crashes >= 2 {
                    slot.failed = true;
                    tracing::error!(
                        server = spec.id,
                        root = %root.display(),
                        "language server marked failed"
                    );
                    return Err(Unavailable::ServerFailed(slot.detail.clone()));
                }
            }
        }
        loop {
            let Some(binary) = (self.resolve)(spec.argv[0]) else {
                if !slot.missing_logged {
                    slot.missing_logged = true;
                    tracing::warn!(
                        server = spec.id,
                        binary = spec.argv[0],
                        root = %root.display(),
                        "language server is not on PATH"
                    );
                }
                return Err(Unavailable::NoServer);
            };
            match client::spawn(&root, spec, &binary, self.timing).await {
                Ok(client) => {
                    slot.crashes = 0;
                    slot.client = Some(Arc::clone(&client));
                    tracing::info!(
                        server = spec.id,
                        root = %root.display(),
                        "language server started"
                    );
                    return Ok(client);
                }
                Err(err) => {
                    slot.detail = err.to_string();
                    tracing::error!(
                        server = spec.id,
                        root = %root.display(),
                        %err,
                        "language server failed to start"
                    );
                    slot.crashes += 1;
                    if slot.crashes >= 2 {
                        slot.failed = true;
                        tracing::error!(
                            server = spec.id,
                            root = %root.display(),
                            "language server marked failed"
                        );
                        return Err(Unavailable::ServerFailed(slot.detail.clone()));
                    }
                }
            }
        }
    }

    pub async fn note_disk(&self, root: &Path, absolute: &Path, deleted: bool) {
        let root = canonical(root);
        for (spec, language) in catalog::matching(absolute) {
            let Some(client) = self.running(&root, spec.id).await else {
                continue;
            };
            if deleted || !client.is_open(absolute) {
                if deleted {
                    client.close_if_open(absolute).await;
                }
                continue;
            }
            let path = absolute.to_path_buf();
            let read = crate::agent::blocking::call(move || {
                let meta = std::fs::metadata(&path).ok()?;
                let bytes = std::fs::read(&path).ok()?;
                Some((meta, bytes))
            })
            .await
            .ok()
            .flatten();
            let Some((meta, bytes)) = read else {
                tracing::warn!(
                    path = %absolute.display(),
                    "failed to read a file for the language server"
                );
                client.close_if_open(absolute).await;
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                tracing::warn!(
                    path = %absolute.display(),
                    "skipped a non-utf-8 file for the language server"
                );
                client.close_if_open(absolute).await;
                continue;
            };
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if let Err(err) = client
                .sync(absolute, language, &text, modified, meta.len())
                .await
            {
                tracing::warn!(
                    server = spec.id,
                    path = %absolute.display(),
                    %err,
                    "failed to sync a document"
                );
            }
        }
    }

    /// The server for this file. Preference order is the catalog order.
    pub fn server_for(&self, path: &Path) -> Option<catalog::Choice> {
        let resolve = &self.resolve;
        catalog::select(path, |bin| resolve(bin).is_some())
    }

    /// Server ids that have at least one matching file, and whether the binary exists.
    pub fn present(&self, root: &Path, filter: &PathFilter) -> Vec<(&'static ServerSpec, bool)> {
        let mut found = Vec::new();
        let mut seen = Vec::new();
        let root_buf = root.to_path_buf();
        let filter = filter.clone();
        let walker = WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .parents(false)
            .follow_links(false)
            .filter_entry(move |entry| {
                if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                    return true;
                }
                let relative = crate::agent::workspace::workspace_relative(&root_buf, entry.path());
                !filter.skip_dir(&relative)
            })
            .build();
        for entry in walker.flatten() {
            if found.len() == catalog::servers().len() {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let Some(choice) = self.server_for(path) else {
                continue;
            };
            if seen.contains(&choice.spec.id) {
                continue;
            }
            seen.push(choice.spec.id);
            found.push((choice.spec, choice.installed));
        }
        found
    }

    pub async fn install_for_test(
        &self,
        root: &Path,
        server_id: &'static str,
        client: Arc<LspClient>,
    ) {
        let gate = self.gate(&canonical(root), server_id).await;
        let mut slot = gate.lock().await;
        slot.client = Some(client);
        slot.failed = false;
        slot.crashes = 0;
    }

    pub async fn sweep(&self) {
        let idle = {
            let slots = self.slots.lock().await;
            let mut idle = Vec::new();
            for gate in slots.values() {
                let slot = gate.lock().await;
                if let Some(client) = &slot.client {
                    if client.is_running() && client.is_idle(self.timing.idle) {
                        idle.push(Arc::clone(client));
                    }
                }
            }
            idle
        };
        for client in idle {
            client.shutdown_idle().await;
        }
        let slots = self.slots.lock().await;
        for gate in slots.values() {
            let mut slot = gate.lock().await;
            if slot
                .client
                .as_ref()
                .is_some_and(|client| !client.is_running())
            {
                slot.client = None;
            }
        }
    }

    async fn running(&self, root: &Path, server_id: &str) -> Option<Arc<LspClient>> {
        let slots = self.slots.lock().await;
        let gate = slots.get(&(root.to_path_buf(), server_id_key(server_id)?))?;
        let slot = gate.lock().await;
        slot.client
            .as_ref()
            .filter(|client| client.is_running())
            .cloned()
    }

    async fn gate(&self, root: &Path, server_id: &'static str) -> Arc<Mutex<Slot>> {
        let mut slots = self.slots.lock().await;
        let key = (root.to_path_buf(), server_id);
        slots
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Mutex::new(Slot {
                    crashes: 0,
                    failed: false,
                    missing_logged: false,
                    detail: String::new(),
                    client: None,
                }))
            })
            .clone()
    }
}

fn server_id_key(id: &str) -> Option<&'static str> {
    catalog::spec(id).map(|spec| spec.id)
}

fn canonical(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn a_write_does_not_start_a_server() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let hub = LspHub::build(
            Timing::fast(),
            Arc::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                None
            }),
        );
        let root = std::env::temp_dir();
        hub.note_disk(&root, &root.join("lib.rs"), false).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_server_that_exits_twice_stays_failed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let hub = LspHub::build(
            Timing {
                initialize: std::time::Duration::from_millis(400),
                ..Timing::fast()
            },
            Arc::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                Some(PathBuf::from("/usr/bin/false"))
            }),
        );
        let spec = catalog::spec("rust-analyzer").unwrap();
        let root = std::env::temp_dir();
        match hub.client(&root, spec).await {
            Err(Unavailable::ServerFailed(detail)) => {
                assert!(
                    detail.contains("exited before initialize"),
                    "expected the exit reason, got {detail}"
                );
            }
            Err(reason) => panic!("expected a failed server, got {reason:?}"),
            Ok(_) => panic!("expected a failed server"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        match hub.client(&root, spec).await {
            Err(Unavailable::ServerFailed(detail)) => {
                assert!(detail.contains("exited before initialize"), "{detail}");
            }
            Err(reason) => panic!("expected the failure to stick, got {reason:?}"),
            Ok(_) => panic!("expected the failure to stick"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn startup_stderr_is_the_failure_detail() {
        let dir = std::env::temp_dir().join(format!("robi-lsp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("fail-server");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho 'Unknown binary rust-analyzer' >&2\nexit 1\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let hub = LspHub::build(
            Timing {
                initialize: std::time::Duration::from_millis(400),
                ..Timing::fast()
            },
            Arc::new(move |_| Some(bin.clone())),
        );
        let spec = catalog::spec("rust-analyzer").unwrap();
        match hub.client(&dir, spec).await {
            Err(Unavailable::ServerFailed(detail)) => {
                assert!(detail.contains("Unknown binary rust-analyzer"), "{detail}");
            }
            Err(reason) => panic!("expected stderr in the failure, got {reason:?}"),
            Ok(_) => panic!("expected the stand-in server to fail"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn rust_analyzer_initializes_when_installed() {
        let Some(binary) = catalog::find_on_path("rust-analyzer") else {
            return;
        };
        let version = std::process::Command::new(&binary)
            .arg("--version")
            .output()
            .expect("rust-analyzer --version");
        if !version.status.success() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("robi-ra-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        let spec = catalog::spec("rust-analyzer").unwrap();
        let client = client::spawn(&dir, spec, &binary, Timing::default())
            .await
            .expect("rust-analyzer initializes");
        assert!(client.is_running());
        drop(client);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

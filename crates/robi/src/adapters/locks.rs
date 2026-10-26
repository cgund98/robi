//! The file locks behind [`Locks`].
//!
//! One file per resource under `~/.robi/locks/`, held through an OS advisory
//! lock on an open descriptor. The lock is released when the descriptor closes,
//! including when the process is killed, so there is no stale-lock sweep and no
//! heartbeat.
//!
//! Two things this does that a bare `try_lock` does not:
//!
//! - **Refcounts within the process.** Two scopes on one session are one lock, so
//!   a background turn in a window showing another session never conflicts with
//!   itself. A second `flock` on a second descriptor would, which is why the
//!   count lives here rather than in the kernel.
//! - **Writes a holder line.** A refused process reads the pid out of the file it
//!   failed to lock, so the refusal can say who has it.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use crate::domain::chat_session::lease::{
    lock_path, LeaseError, LeaseGuard, LeaseHolder, LeaseRelease, Locks, Resource, Scope,
};

/// Path lock files older than this are removed at startup.
///
/// They are never unlinked on release, because unlinking a held inode lets a
/// second process lock a new file at the same path. A directory of them
/// accumulates one file per workspace file ever edited, so the sweep is by age.
pub const PATH_LOCK_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What runs when the last reference to a resource goes.
///
/// Session locks use it to close the cached `blobs.redb` handle: the invariant is
/// that a session's file is open only while this process holds that session's
/// lock, so the handle has to close before the lock releases.
pub type ReleaseHook = Box<dyn Fn(&Resource) + Send + Sync>;

/// The file-backed [`Locks`].
pub struct FileLocks {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    database_url: String,
    entries: Mutex<HashMap<Resource, Entry>>,
    on_release: Option<ReleaseHook>,
}

struct Entry {
    /// Held for its descriptor: drop it and the OS releases the lock.
    _file: Arc<File>,
    /// How many guards in this process hold it.
    count: usize,
}

impl std::fmt::Debug for FileLocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileLocks")
            .field("root", &self.inner.root)
            .finish()
    }
}

impl FileLocks {
    /// Locks under `settings_dir`, which is `~/.robi`.
    pub fn new(settings_dir: impl Into<PathBuf>, database_url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: settings_dir.into(),
                database_url: database_url.into(),
                entries: Mutex::new(HashMap::new()),
                on_release: None,
            }),
        }
    }

    /// Run `hook` when the last reference to a resource is released.
    pub fn with_release(mut self, hook: ReleaseHook) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.on_release = Some(hook);
        }
        self
    }

    /// The directory the lock files live in.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// How many references this process holds to `resource`.
    pub fn count(&self, resource: &Resource) -> usize {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(resource)
            .map(|entry| entry.count)
            .unwrap_or(0)
    }

    /// Remove path lock files nothing holds and nothing has touched recently.
    ///
    /// Returns how many were removed. Idempotent and safe to call on every start.
    pub fn prune_path_locks(&self, max_age: Duration) -> usize {
        let directory = self.inner.root.join("paths");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return 0;
        };
        let now = SystemTime::now();
        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
                continue;
            }
            let aged = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > max_age);
            if !aged {
                continue;
            }
            // Only a file no one holds can go: locking it first is what makes
            // the unlink safe, because a holder would have it locked already.
            let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Check that the filesystem actually implements locking.
    ///
    /// A network home or some FUSE mounts make `flock` a local no-op, which is
    /// the one way this design can corrupt data. Called once at startup.
    pub fn probe(&self) -> Result<(), String> {
        let directory = self.inner.root.join("probe");
        std::fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
        let path = directory.join("probe.lock");
        let cleanup = || {
            let _ = std::fs::remove_file(&path);
        };
        let first = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| err.to_string())?;
        first.try_lock().map_err(|err| err.to_string())?;
        let second = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| err.to_string())?;
        let conflicts = second.try_lock().is_err();
        let _ = second.unlock();
        let _ = first.unlock();
        cleanup();
        if conflicts {
            Ok(())
        } else {
            Err(format!(
                "{} does not enforce file locks, so two robi processes could write the same session",
                self.inner.root.display()
            ))
        }
    }
}

impl Locks for FileLocks {
    fn try_acquire(&self, resource: &Resource, scope: Scope) -> Result<LeaseGuard, LeaseError> {
        let mut entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = entries.get_mut(resource) {
            entry.count += 1;
            return Ok(LeaseGuard::new(Box::new(Release {
                inner: Arc::clone(&self.inner),
                resource: resource.clone(),
            })));
        }

        let path = lock_path(&self.inner.root, resource);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| LeaseError::Io(format!("{}: {err}", parent.display())))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| LeaseError::Io(format!("{}: {err}", path.display())))?;

        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let holder = read_holder(&mut file);
                return Err(match holder {
                    Some(holder) => LeaseError::held(resource, holder),
                    // The holder wrote the line before it could fail, so an
                    // empty file means a process between the two steps. Name
                    // the resource without a pid rather than inventing one.
                    None => LeaseError::Io(format!(
                        "{} is held by another process",
                        resource.describe()
                    )),
                });
            }
            Err(std::fs::TryLockError::Error(err)) => {
                return Err(LeaseError::Io(format!("{}: {err}", path.display())));
            }
        }

        // The holder line is read by a process that just failed to lock, so it
        // has to be on disk before anything else can see the file as held.
        write_holder(&mut file, scope, &self.inner.database_url)
            .map_err(|err| LeaseError::Io(format!("{}: {err}", path.display())))?;

        entries.insert(
            resource.clone(),
            Entry {
                _file: Arc::new(file),
                count: 1,
            },
        );
        Ok(LeaseGuard::new(Box::new(Release {
            inner: Arc::clone(&self.inner),
            resource: resource.clone(),
        })))
    }

    fn holder(&self, resource: &Resource) -> Option<LeaseHolder> {
        // This process holding it is not a refusal, so it reads as no holder.
        if self.count(resource) > 0 {
            return None;
        }
        let path = lock_path(&self.inner.root, resource);
        let mut file = OpenOptions::new().read(true).open(path).ok()?;
        read_holder(&mut file)
    }
}

/// One guard's share of a resource.
struct Release {
    inner: Arc<Inner>,
    resource: Resource,
}

impl LeaseRelease for Release {
    fn release(self: Box<Self>) {
        let mut entries = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = entries.get_mut(&self.resource) else {
            return;
        };
        entry.count -= 1;
        if entry.count > 0 {
            return;
        }
        entries.remove(&self.resource);
        // Still holding the entries lock: the release hook closes the session's
        // blob handle, and a concurrent acquire of the same session must not slip
        // in between the close and the unlock with a stale handle.
        if let Some(hook) = &self.inner.on_release {
            hook(&self.resource);
        }
    }
}

/// `pid <n> scope <scope> database <url> at <time>`.
fn write_holder(file: &mut File, scope: Scope, database_url: &str) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    writeln!(
        file,
        "pid {} scope {} database {} at {now}",
        std::process::id(),
        scope.as_str(),
        database_url
    )?;
    file.sync_all()
}

/// Read the holder line back. A truncated or absent line reads as `None`.
fn read_holder(file: &mut File) -> Option<LeaseHolder> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    let line = text.lines().next().unwrap_or("").trim();
    let mut words = line.split_whitespace();
    let mut pid = None;
    let mut scope = Scope::Open;
    while let Some(word) = words.next() {
        match word {
            "pid" => pid = words.next().and_then(|value| value.parse::<u32>().ok()),
            "scope" => {
                if let Some(value) = words.next() {
                    scope = Scope::from_wire(value);
                }
            }
            _ => {}
        }
    }
    pid.map(|pid| LeaseHolder::new(pid, scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use robi_core::ids::SessionId;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn home() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("robi-locks-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).expect("a temp home");
        dir
    }

    fn locks(home: &Path) -> FileLocks {
        FileLocks::new(home, "sqlite://robi.db")
    }

    #[test]
    fn a_second_instance_is_refused_with_the_first_pid() {
        let home = home();
        let first = locks(&home);
        let second = locks(&home);
        let resource = Resource::Session(SessionId::new());

        let guard = first
            .try_acquire(&resource, Scope::Turn)
            .expect("the first lock");

        let error = second
            .try_acquire(&resource, Scope::Open)
            .expect_err("the second must fail");
        let holder = error.holder().expect("the holder is named");
        assert_eq!(holder.pid, std::process::id());
        assert_eq!(
            holder.scope,
            Scope::Turn,
            "the holder line records the scope"
        );
        assert!(error.message().contains(&std::process::id().to_string()));

        drop(guard);
        assert!(
            second.try_acquire(&resource, Scope::Open).is_ok(),
            "dropping the last guard frees it"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn two_scopes_in_one_process_are_one_lock() {
        let home = home();
        let locks = locks(&home);
        let resource = Resource::Session(SessionId::new());

        let open = locks.try_acquire(&resource, Scope::Open).expect("open");
        let turn = locks.try_acquire(&resource, Scope::Turn).expect("turn");
        assert_eq!(locks.count(&resource), 2);
        assert_eq!(
            locks.holder(&resource),
            None,
            "our own lock is not a holder"
        );

        drop(open);
        assert_eq!(locks.count(&resource), 1);
        drop(turn);
        assert_eq!(locks.count(&resource), 0);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_release_hook_runs_once_on_the_last_drop() {
        let home = home();
        let released = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&released);
        let locks = FileLocks::new(&home, "sqlite://robi.db").with_release(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let resource = Resource::Session(SessionId::new());

        let first = locks.try_acquire(&resource, Scope::Open).expect("first");
        let second = locks.try_acquire(&resource, Scope::Turn).expect("second");
        drop(first);
        assert_eq!(released.load(Ordering::SeqCst), 0, "not while still held");
        drop(second);
        assert_eq!(released.load(Ordering::SeqCst), 1, "once, at the end");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn each_resource_is_independent() {
        let home = home();
        let first = locks(&home);
        let second = locks(&home);
        let held = Resource::Session(SessionId::new());
        let other = Resource::Session(SessionId::new());

        let _guard = first.try_acquire(&held, Scope::Turn).expect("held");
        assert!(second.try_acquire(&held, Scope::Open).is_err());
        assert!(
            second.try_acquire(&other, Scope::Open).is_ok(),
            "another session is another lock"
        );
        assert!(
            second.try_acquire(&Resource::Startup, Scope::Turn).is_ok(),
            "and so is a different kind of resource"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_probe_passes_on_a_local_filesystem() {
        let home = home();
        let locks = locks(&home);
        locks.probe().expect("a temp directory locks");
        assert!(
            !home.join("probe").join("probe.lock").exists(),
            "the probe cleans up after itself"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_prune_spares_a_held_file_and_a_recent_one() {
        let home = home();
        let locks = locks(&home);
        let resource = Resource::path("/workspace/src/lib.rs");
        let guard = locks.try_acquire(&resource, Scope::Turn).expect("held");
        let held = crate::domain::chat_session::lease::lock_path(&home, &resource);

        let recent = home.join("paths").join("recent.lock");
        std::fs::create_dir_all(home.join("paths")).expect("the paths directory");
        std::fs::write(&recent, b"pid 1 scope turn").expect("a recent file");

        let removed = locks.prune_path_locks(Duration::from_secs(3600));
        assert_eq!(removed, 0, "nothing is old enough");
        assert!(held.exists());
        assert!(recent.exists());

        // Backdate both, then prune: the held one survives because it is locked.
        let old = SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
        for path in [&held, &recent] {
            let file = OpenOptions::new().write(true).open(path).expect("open");
            file.set_modified(old).expect("backdate");
        }
        let removed = locks.prune_path_locks(PATH_LOCK_MAX_AGE);
        assert_eq!(removed, 1, "only the unlocked, aged file goes");
        assert!(held.exists(), "a held lock file is never unlinked");
        assert!(!recent.exists());
        drop(guard);
        let _ = std::fs::remove_dir_all(&home);
    }
}

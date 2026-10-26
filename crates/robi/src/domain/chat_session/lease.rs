//! Cross-process ownership of the things two processes can both want.
//!
//! One process per home directory used to be the rule, enforced by an advisory
//! lock the terminal took for its whole life. That refused too much: two
//! terminals in one workspace could not coexist. This module is the replacement.
//! A lock covers one value — a chat session, a workspace file, the startup work,
//! the settings files — and it is taken when that value is touched rather than
//! when the process starts.
//!
//! # Two scopes, one lock
//!
//! A session lock has two scopes. `Open` means this process has the session's
//! `blobs.redb` open. `Turn` means it is driving a turn on it. Both refcount onto
//! the same lock, so a background turn in a process whose window shows another
//! session never conflicts with itself.
//!
//! `Turn` is exclusion and `Open` is ownership. A second `Turn` is refused, so
//! two processes cannot interleave one transcript. A second `Open` only decides
//! whether the blob file may be opened, and the caller falls back to a degraded
//! transcript rather than failing.
//!
//! # What waits, and what does not
//!
//! Ownership refuses at once ([`Wait::None`]): the holder is usually mid-turn, so
//! waiting only delays a refusal that is already decided. Edits wait
//! ([`Wait::UpTo`]) up to [`LOCK_TIMEOUT`], because contention there is short and
//! losing a race to another process should not fail a turn.
//!
//! No lock expires on a timer. The operating system drops a file lock when the
//! owning descriptor closes, including on `kill -9`, so there is no heartbeat, no
//! TTL, and no stale-lease sweep.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use robi_core::ids::SessionId;

/// How long an edit waits for a lock another process holds.
///
/// Ten seconds, the same order as the connection pool's `busy_timeout`. A tool
/// that loses a race to another process waits this out instead of failing a turn
/// over a write that was going to take milliseconds.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a bounded wait retries while the lock is held.
pub const RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Something one process can hold and another can want.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resource {
    /// One chat session, covering its row, its transcript, and its `blobs.redb`.
    Session(SessionId),
    /// One workspace file, for the length of a read-modify-write.
    Path(PathBuf),
    /// The work `build_app_state` does before a front end exists: migrations,
    /// blob evacuation, the session prune, and the turn-display reconcile.
    Startup,
    /// A write to `config.toml` or `secrets.toml`.
    Settings,
}

impl Resource {
    /// The resource for a workspace file.
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    /// How the resource reads in a refusal, after "held by another process".
    ///
    /// A session is named by the caller, which has the title. This is the
    /// fallback: a path, or a word for the two singletons.
    pub fn describe(&self) -> String {
        match self {
            Self::Session(id) => format!("chat {id}"),
            Self::Path(path) => path.display().to_string(),
            Self::Startup => "startup".to_owned(),
            Self::Settings => "the settings files".to_owned(),
        }
    }
}

/// Whether a session lock covers reading and only, or driving a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// This process has the session's `blobs.redb` open.
    Open,
    /// This process is driving a turn on the session. Implies `Open`.
    Turn,
}

impl Scope {
    /// The word written into the lock file's holder line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Turn => "turn",
        }
    }

    /// Parse the holder line's word. Unknown text reads as `Open`, which is the
    /// weaker claim: a refusal still names the pid.
    pub fn from_wire(word: &str) -> Self {
        if word == "turn" {
            Self::Turn
        } else {
            Self::Open
        }
    }
}

/// Whether to wait for a held lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// Refuse at once. For ownership, where the holder is in for the long haul.
    None,
    /// Retry until the duration elapses, then refuse. For edits.
    UpTo(Duration),
}

impl Wait {
    /// The bounded wait an edit uses.
    pub fn edit() -> Self {
        Self::UpTo(LOCK_TIMEOUT)
    }
}

/// Who holds a lock, read from the holder line without taking anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseHolder {
    /// The holding process.
    pub pid: u32,
    /// What it claims to be doing.
    pub scope: Scope,
}

impl LeaseHolder {
    pub fn new(pid: u32, scope: Scope) -> Self {
        Self { pid, scope }
    }
}

/// Why a lock was not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LeaseError {
    /// Another process holds it. `resource` renders in the message.
    #[error(
        "{resource} is held by another process ({} {})",
        .holder.scope.as_str(),
        .holder.pid
    )]
    Held {
        resource: String,
        holder: LeaseHolder,
    },
    /// The lock file could not be opened, read, or written.
    #[error("{0}")]
    Io(String),
}

impl LeaseError {
    /// A refusal naming the resource and the holder.
    pub fn held(resource: &Resource, holder: LeaseHolder) -> Self {
        Self::Held {
            resource: resource.describe(),
            holder,
        }
    }

    /// The holding process, when another process is the reason.
    pub fn holder(&self) -> Option<&LeaseHolder> {
        match self {
            Self::Held { holder, .. } => Some(holder),
            Self::Io(_) => None,
        }
    }

    /// A sentence for the user, without a type prefix.
    ///
    /// The TUI shows this in the footer, so it has to read as prose rather than
    /// as `LeaseError::Held { .. }`.
    pub fn message(&self) -> String {
        self.to_string()
    }

    /// The service refusal a held lock becomes.
    ///
    /// `Conflict` is what `web_api` already maps to 409, so the desktop app and
    /// the terminal agree on the meaning without either knowing about locks.
    pub fn conflict(&self) -> crate::domain::error::ServiceError {
        match self {
            Self::Held { resource, holder } => crate::domain::error::ServiceError::Conflict(
                format!("{resource} is open in another process (pid {})", holder.pid),
            ),
            Self::Io(message) => {
                tracing::error!(%message, "could not read or write a lock file");
                crate::domain::error::ServiceError::Unknown
            }
        }
    }
}

/// Released when dropped. The last drop unlocks the file and, for a session,
/// closes the cached `blobs.redb` handle.
pub struct LeaseGuard {
    release: Option<Box<dyn LeaseRelease>>,
}

impl LeaseGuard {
    /// Wrap a release action.
    pub fn new(release: Box<dyn LeaseRelease>) -> Self {
        Self {
            release: Some(release),
        }
    }

    /// A guard that holds nothing, for the in-memory implementation and tests.
    pub fn noop() -> Self {
        Self { release: None }
    }
}

impl std::fmt::Debug for LeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaseGuard")
            .field("held", &self.release.is_some())
            .finish()
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release.release();
        }
    }
}

/// What a guard does when it drops.
///
/// Boxed rather than a closure so the adapter can name the session and the
/// refcount it is giving back.
pub trait LeaseRelease: Send + Sync {
    fn release(self: Box<Self>);
}

/// Takes and inspects locks.
///
/// Sync rather than async because the blob store is sync and calls it from
/// `spawn_blocking`. A caller that wants to wait uses [`acquire_waiting`], which
/// polls this in an async loop.
pub trait Locks: Send + Sync {
    /// Take the lock, or report the holder. Refcounts within one process.
    fn try_acquire(&self, resource: &Resource, scope: Scope) -> Result<LeaseGuard, LeaseError>;

    /// The current holder, without taking anything. For a notice.
    fn holder(&self, resource: &Resource) -> Option<LeaseHolder>;

    /// Take the lock, waiting out another process for at most `wait`.
    ///
    /// The default polls [`Self::try_acquire`]; an implementation with a better
    /// primitive can override it.
    fn acquire(
        &self,
        resource: &Resource,
        scope: Scope,
        wait: Wait,
    ) -> Result<LeaseGuard, LeaseError> {
        let deadline = match wait {
            Wait::None => return self.try_acquire(resource, scope),
            Wait::UpTo(limit) => Instant::now() + limit,
        };
        loop {
            match self.try_acquire(resource, scope) {
                Ok(guard) => return Ok(guard),
                Err(error @ LeaseError::Held { .. }) => {
                    if Instant::now() >= deadline {
                        return Err(error);
                    }
                    std::thread::sleep(RETRY_INTERVAL);
                }
                Err(other) => return Err(other),
            }
        }
    }
}

/// Grants everything. The default in tests and in any build with one process.
#[derive(Debug, Default)]
pub struct MemoryLocks;

impl MemoryLocks {
    pub fn new() -> Self {
        Self
    }
}

impl Locks for MemoryLocks {
    fn try_acquire(&self, _resource: &Resource, _scope: Scope) -> Result<LeaseGuard, LeaseError> {
        Ok(LeaseGuard::noop())
    }

    fn holder(&self, _resource: &Resource) -> Option<LeaseHolder> {
        None
    }
}

/// Take the lock, waiting out another process for at most `wait`.
///
/// The async form of [`Locks::acquire`]: it sleeps on the runtime instead of
/// blocking the thread, so a tool awaiting a file lock does not stall the worker.
pub async fn acquire_waiting(
    locks: &dyn Locks,
    resource: &Resource,
    scope: Scope,
    wait: Wait,
) -> Result<LeaseGuard, LeaseError> {
    let deadline = match wait {
        Wait::None => return locks.try_acquire(resource, scope),
        Wait::UpTo(limit) => Instant::now() + limit,
    };
    loop {
        match locks.try_acquire(resource, scope) {
            Ok(guard) => return Ok(guard),
            Err(error @ LeaseError::Held { .. }) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
            Err(other) => return Err(other),
        }
    }
}

/// `~/.robi/locks/<relative>`, the file backing one resource.
///
/// Here rather than in the adapter because the layout is part of the answer: a
/// reader looking for `~/.robi/locks/sessions/<id>.lock` should find the rule in
/// one place.
pub fn lock_path(settings_dir: &Path, resource: &Resource) -> PathBuf {
    let root = settings_dir.join("locks");
    match resource {
        Resource::Session(id) => root.join("sessions").join(format!("{id}.lock")),
        Resource::Path(path) => root.join("paths").join(format!("{}.lock", path_key(path))),
        Resource::Startup => root.join("startup.lock"),
        Resource::Settings => root.join("settings.lock"),
    }
}

/// A stable file name for an absolute path.
///
/// Hashed rather than escaped: a path is bounded by nothing the filesystem
/// accepts, and a hash of the absolute path is unique enough that two different
/// paths cannot collide in practice.
pub fn path_key(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grants everything except the resources it is told to refuse.
    struct SelectiveLocks {
        held: Vec<Resource>,
    }

    impl Locks for SelectiveLocks {
        fn try_acquire(&self, resource: &Resource, scope: Scope) -> Result<LeaseGuard, LeaseError> {
            if self.held.contains(resource) {
                return Err(LeaseError::held(resource, LeaseHolder::new(4812, scope)));
            }
            Ok(LeaseGuard::noop())
        }

        fn holder(&self, resource: &Resource) -> Option<LeaseHolder> {
            self.held
                .contains(resource)
                .then(|| LeaseHolder::new(4812, Scope::Turn))
        }
    }

    fn session() -> Resource {
        Resource::Session(SessionId::new())
    }

    #[tokio::test]
    async fn a_free_resource_is_taken_at_once() {
        let locks = SelectiveLocks { held: Vec::new() };
        let guard = acquire_waiting(&locks, &session(), Scope::Turn, Wait::None).await;
        assert!(guard.is_ok());
    }

    #[tokio::test]
    async fn ownership_refuses_without_waiting() {
        let resource = session();
        let locks = SelectiveLocks {
            held: vec![resource.clone()],
        };
        let started = Instant::now();
        let error = acquire_waiting(&locks, &resource, Scope::Turn, Wait::None)
            .await
            .expect_err("held");
        assert!(
            started.elapsed() < RETRY_INTERVAL,
            "Wait::None must not sleep: {:?}",
            started.elapsed()
        );
        let holder = error.holder().expect("the holder is named");
        assert_eq!(holder.pid, 4812);
        assert!(error.message().contains("4812"), "{error}");
    }

    #[tokio::test]
    async fn an_edit_waits_out_the_window_then_refuses() {
        let resource = Resource::path("/workspace/src/lib.rs");
        let locks = SelectiveLocks {
            held: vec![resource.clone()],
        };
        let wait = Duration::from_millis(120);
        let started = Instant::now();
        let error = acquire_waiting(&locks, &resource, Scope::Turn, Wait::UpTo(wait))
            .await
            .expect_err("held");
        let elapsed = started.elapsed();
        assert!(elapsed >= wait, "it waited the whole window: {elapsed:?}");
        assert!(
            elapsed < wait + RETRY_INTERVAL * 4,
            "and no longer: {elapsed:?}"
        );
        assert!(error.message().contains("src/lib.rs"), "{error}");
    }

    #[tokio::test]
    async fn the_default_edit_wait_is_ten_seconds() {
        assert_eq!(Wait::edit(), Wait::UpTo(Duration::from_secs(10)));
        assert_eq!(LOCK_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn a_scope_round_trips_through_its_word() {
        assert_eq!(Scope::from_wire(Scope::Turn.as_str()), Scope::Turn);
        assert_eq!(Scope::from_wire(Scope::Open.as_str()), Scope::Open);
        assert_eq!(Scope::from_wire("nonsense"), Scope::Open);
    }

    #[test]
    fn each_resource_gets_its_own_file() {
        let home = Path::new("/home/u/.robi");
        let id = SessionId::new();
        let session = lock_path(home, &Resource::Session(id));
        assert_eq!(
            session,
            home.join("locks")
                .join("sessions")
                .join(format!("{id}.lock"))
        );
        assert_eq!(
            lock_path(home, &Resource::Startup),
            home.join("locks").join("startup.lock")
        );
        assert_eq!(
            lock_path(home, &Resource::Settings),
            home.join("locks").join("settings.lock")
        );
    }

    #[test]
    fn a_path_key_is_stable_and_distinct() {
        let a = path_key(Path::new("/workspace/src/lib.rs"));
        assert_eq!(a, path_key(Path::new("/workspace/src/lib.rs")));
        assert_ne!(a, path_key(Path::new("/workspace/src/main.rs")));
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn a_refusal_names_the_resource_and_the_pid() {
        let resource = Resource::path("/workspace/README.md");
        let error = LeaseError::held(&resource, LeaseHolder::new(991, Scope::Turn));
        assert_eq!(
            error.message(),
            "/workspace/README.md is held by another process (turn 991)"
        );
    }
}

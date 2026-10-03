//! Walk a workspace, embed chunks, and keep the file current.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use robi_core::ids::WorkspaceId;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::chunk::{chunk_source, language_for_path, Chunk};
use crate::embed::Embedder;
use crate::error::IndexError;
use crate::store::{self, ChunkHit};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
const EMBED_BATCH: usize = 16;
const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Downloading,
    Indexing,
    Ready,
    Paused,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatus {
    pub state: IndexState,
    pub files_done: u64,
    pub files_total: u64,
    pub error: Option<String>,
}

impl Default for IndexStatus {
    fn default() -> Self {
        Self {
            state: IndexState::Indexing,
            files_done: 0,
            files_total: 0,
            error: None,
        }
    }
}

struct Shared {
    workspace_id: String,
    root: PathBuf,
    db_path: PathBuf,
    status: RwLock<IndexStatus>,
    pause: AtomicBool,
    resume: Notify,
    stop: CancellationToken,
    embedder: Arc<dyn Embedder>,
    on_status: Arc<dyn Fn(IndexStatus) + Send + Sync>,
}

pub struct Index {
    shared: Arc<Shared>,
}

impl Index {
    pub fn start(
        workspace_id: WorkspaceId,
        root: PathBuf,
        db_path: PathBuf,
        embedder: Arc<dyn Embedder>,
        on_status: Arc<dyn Fn(IndexStatus) + Send + Sync>,
    ) -> Arc<Self> {
        let root = root.canonicalize().unwrap_or(root);
        let shared = Arc::new(Shared {
            workspace_id: workspace_id.to_string(),
            root,
            db_path,
            status: RwLock::new(IndexStatus::default()),
            pause: AtomicBool::new(false),
            resume: Notify::new(),
            stop: CancellationToken::new(),
            embedder,
            on_status,
        });
        let task = Arc::clone(&shared);
        tokio::spawn(async move {
            if let Err(err) = run(task.clone()).await {
                set_status(&task, |status| {
                    status.state = IndexState::Failed;
                    status.error = Some(err.to_string());
                });
            }
        });
        Arc::new(Self { shared })
    }

    pub fn status(&self) -> IndexStatus {
        self.shared.status.read().expect("index status").clone()
    }

    /// Publish the current status. The read lock is held across the callback so
    /// a newer `set_status` cannot publish ahead of this snapshot.
    pub fn report(&self) {
        let status = self.shared.status.read().expect("index status");
        (self.shared.on_status)(status.clone());
    }

    pub fn pause(&self) {
        self.shared.pause.store(true, Ordering::SeqCst);
        if let Ok(connection) = store::open_connection(&self.shared.db_path) {
            let _ = store::set_paused(&connection, true);
        }
    }

    pub fn resume(&self) {
        self.shared.pause.store(false, Ordering::SeqCst);
        if let Ok(connection) = store::open_connection(&self.shared.db_path) {
            let _ = store::set_paused(&connection, false);
        }
        self.shared.resume.notify_waiters();
    }

    pub fn stop(&self) {
        self.shared.stop.cancel();
        self.shared.resume.notify_waiters();
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, IndexError> {
        self.shared.embedder.embed_query(text).await
    }

    pub fn search(
        &self,
        query_vec: &[f32],
        fts: &str,
        limit: usize,
    ) -> Result<Vec<ChunkHit>, IndexError> {
        if !self.shared.db_path.exists() {
            return Ok(Vec::new());
        }
        let connection = store::open_connection(&self.shared.db_path)?;
        store::search(&connection, query_vec, fts, limit)
    }
}

async fn run(shared: Arc<Shared>) -> Result<(), IndexError> {
    prepare_db(&shared)?;
    let (mut watcher, mut rx) = start_watcher(&shared.root)?;
    if store::is_paused(&store::open_connection(&shared.db_path)?)? {
        shared.pause.store(true, Ordering::SeqCst);
        set_status(&shared, |status| {
            status.state = IndexState::Paused;
            status.error = None;
        });
        wait_until_running(&shared).await;
        if shared.stop.is_cancelled() {
            return Ok(());
        }
    }
    set_status(&shared, |status| {
        status.state = IndexState::Downloading;
        status.error = None;
    });
    if let Err(err) = shared.embedder.warmup().await {
        set_status(&shared, |status| {
            status.state = IndexState::Failed;
            status.error = Some(err.to_string());
        });
        return Ok(());
    }
    scan(&shared).await?;
    watch_loop(&shared, &mut watcher, &mut rx).await
}

fn start_watcher(
    root: &Path,
) -> Result<
    (
        RecommendedWatcher,
        tokio::sync::mpsc::UnboundedReceiver<Event>,
    ),
    IndexError,
> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = RecommendedWatcher::new(
        move |result: Result<Event, notify::Error>| {
            if let Ok(event) = result {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    )
    .map_err(|err| IndexError::Message(format!("watch workspace: {err}")))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|err| IndexError::Message(format!("watch workspace: {err}")))?;
    Ok((watcher, rx))
}

fn prepare_db(shared: &Shared) -> Result<(), IndexError> {
    if shared.db_path.exists() {
        let connection = store::open_connection(&shared.db_path)?;
        let matches = store::meta_matches(
            &connection,
            shared.embedder.model_id(),
            shared.embedder.dimensions(),
            &shared.workspace_id,
        )?;
        if !matches && store::meta_get(&connection, "model_id")?.is_some() {
            drop(connection);
            store::remove_db_files(&shared.db_path);
        }
    }
    let connection = store::open_connection(&shared.db_path)?;
    store::ensure_schema(&connection, shared.embedder.dimensions())?;
    if store::meta_get(&connection, "model_id")?.is_none() {
        store::write_meta(
            &connection,
            shared.embedder.model_id(),
            shared.embedder.dimensions(),
            &shared.workspace_id,
        )?;
    }
    Ok(())
}

async fn scan(shared: &Shared) -> Result<(), IndexError> {
    let files = list_files(&shared.root);
    set_status(shared, |status| {
        status.state = IndexState::Indexing;
        status.files_done = 0;
        status.files_total = files.len() as u64;
        status.error = None;
    });
    let mut pending = Vec::new();
    let mut done = 0u64;
    let mut failures = 0u64;
    for path in &files {
        if !wait_if_paused(shared).await {
            return Ok(());
        }
        match classify_path(shared, path) {
            Ok(Classified::Finished) => done += 1,
            Ok(Classified::Pending(job)) => pending.push(job),
            Err(err) => {
                failures += 1;
                done += 1;
                set_status(shared, |status| {
                    status.error = Some(format!("{}: {err}", display_rel(&shared.root, path)));
                });
            }
        }
        set_status(shared, |status| status.files_done = done);
    }
    let mut embedded = 0u64;
    for job in pending {
        if !wait_if_paused(shared).await {
            return Ok(());
        }
        match embed_pending(shared, &job).await {
            Ok(()) => embedded += 1,
            Err(err) => {
                failures += 1;
                set_status(shared, |status| {
                    status.error = Some(format!("{}: {err}", job.relative));
                });
            }
        }
        done += 1;
        set_status(shared, |status| status.files_done = done);
    }
    if failures > 0 && embedded == 0 && !files.is_empty() {
        set_status(shared, |status| status.state = IndexState::Failed);
        return Ok(());
    }
    set_status(shared, |status| {
        status.state = IndexState::Ready;
        if failures == 0 {
            status.error = None;
        }
    });
    Ok(())
}

async fn wait_if_paused(shared: &Shared) -> bool {
    if shared.stop.is_cancelled() {
        return false;
    }
    if shared.pause.load(Ordering::SeqCst) {
        set_status(shared, |status| status.state = IndexState::Paused);
        wait_until_running(shared).await;
        if shared.stop.is_cancelled() {
            return false;
        }
        set_status(shared, |status| status.state = IndexState::Indexing);
    }
    true
}

struct PendingEmbed {
    relative: String,
    hash: String,
    language: String,
    chunks: Vec<Chunk>,
}

enum Classified {
    /// Skipped, unchanged, or stored with no chunks. Already finished.
    Finished,
    Pending(PendingEmbed),
}

fn classify_path(shared: &Shared, path: &Path) -> Result<Classified, IndexError> {
    let relative = display_rel(&shared.root, path);
    let Some(language) = language_for_path(&relative) else {
        return Ok(Classified::Finished);
    };
    if is_secret(&relative) {
        return Ok(Classified::Finished);
    }
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Ok(Classified::Finished);
    }
    let bytes = std::fs::read(path)?;
    if bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0) {
        return Ok(Classified::Finished);
    }
    let hash = store::content_hash(&bytes);
    let connection = store::open_connection(&shared.db_path)?;
    if store::stored_hash(&connection, &relative)?.as_deref() == Some(hash.as_str()) {
        return Ok(Classified::Finished);
    }
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let chunks = chunk_source(language, &relative, &source)?;
    if chunks.is_empty() {
        store::replace_file(&connection, &relative, &hash, language.as_str(), &[], &[])?;
        return Ok(Classified::Finished);
    }
    Ok(Classified::Pending(PendingEmbed {
        relative,
        hash,
        language: language.as_str().to_string(),
        chunks,
    }))
}

async fn embed_pending(shared: &Shared, job: &PendingEmbed) -> Result<(), IndexError> {
    let vectors = embed_chunks(&shared.embedder, &job.chunks).await?;
    let connection = store::open_connection(&shared.db_path)?;
    store::replace_file(
        &connection,
        &job.relative,
        &job.hash,
        &job.language,
        &job.chunks,
        &vectors,
    )?;
    Ok(())
}

async fn index_path(shared: &Shared, path: &Path) -> Result<(), IndexError> {
    if let Classified::Pending(job) = classify_path(shared, path)? {
        embed_pending(shared, &job).await?;
    }
    Ok(())
}

async fn embed_chunks(
    embedder: &Arc<dyn Embedder>,
    chunks: &[Chunk],
) -> Result<Vec<Vec<f32>>, IndexError> {
    let mut vectors = Vec::with_capacity(chunks.len());
    for batch in chunks.chunks(EMBED_BATCH) {
        let texts: Vec<String> = batch.iter().map(|chunk| chunk.embed_text.clone()).collect();
        vectors.extend(embedder.embed_documents(&texts).await?);
    }
    Ok(vectors)
}

async fn watch_loop(
    shared: &Shared,
    watcher: &mut RecommendedWatcher,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>,
) -> Result<(), IndexError> {
    let _watcher = watcher;
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
    loop {
        if shared.stop.is_cancelled() {
            return Ok(());
        }
        let wait = pending
            .values()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .min();
        tokio::select! {
            _ = shared.stop.cancelled() => return Ok(()),
            _ = shared.resume.notified(), if shared.pause.load(Ordering::SeqCst) => {
                set_status(shared, |status| status.state = IndexState::Indexing);
            }
            event = rx.recv() => {
                let Some(event) = event else { return Ok(()) };
                let deadline = Instant::now() + DEBOUNCE;
                for path in event.paths {
                    pending.insert(path, deadline);
                }
            }
            _ = sleep_until(wait), if wait.is_some() => {
                let due: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, deadline)| **deadline <= Instant::now())
                    .map(|(path, _)| path.clone())
                    .collect();
                for path in due {
                    pending.remove(&path);
                    if shared.pause.load(Ordering::SeqCst) {
                        set_status(shared, |status| status.state = IndexState::Paused);
                        wait_until_running(shared).await;
                    }
                    apply_change(shared, &path).await;
                }
                if pending.is_empty() && !shared.pause.load(Ordering::SeqCst) {
                    set_status(shared, |status| status.state = IndexState::Ready);
                }
            }
        }
    }
}

async fn sleep_until(wait: Option<Duration>) {
    tokio::time::sleep(wait.unwrap_or(Duration::from_secs(3600))).await;
}

async fn apply_change(shared: &Shared, path: &Path) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path.starts_with(&shared.root) {
        return;
    }
    let connection = match store::open_connection(&shared.db_path) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    if !path.exists() {
        let relative = display_rel(&shared.root, &path);
        let _ = store::delete_path(&connection, &relative);
        return;
    }
    set_status(shared, |status| status.state = IndexState::Indexing);
    if path.is_dir() {
        for file in list_files(&path) {
            let _ = index_path(shared, &file).await;
        }
        return;
    }
    let _ = index_path(shared, &path).await;
}

async fn wait_until_running(shared: &Shared) {
    while shared.pause.load(Ordering::SeqCst) && !shared.stop.is_cancelled() {
        shared.resume.notified().await;
    }
}

fn set_status(shared: &Shared, update: impl FnOnce(&mut IndexStatus)) {
    let status = {
        let mut status = shared.status.write().expect("index status");
        update(&mut status);
        status.clone()
    };
    (shared.on_status)(status);
}

fn list_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .build()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.into_path())
        .filter(|path| path.is_file())
        .collect()
}

fn display_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_secret(relative: &str) -> bool {
    static PATTERNS: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"(^|/)\.git(/|$)",
            r"(^|/)\.env$",
            r"(^|/)\.env\.[^/]+$",
            r"(^|/)[^/]+\.(pem|key)$",
            r"(^|/)id_rsa$",
            r"(^|/)id_ed25519$",
            r"(^|/)credentials\.json$",
            r"(^|/)secrets\.json$",
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("secret pattern"))
        .collect()
    });
    patterns.iter().any(|pattern| pattern.is_match(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{Embedder, FakeEmbedder};
    use std::fs;

    fn temp_workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!("robi-index-{}", uuid_suffix()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn uuid_suffix() -> String {
        format!("{:?}", Instant::now())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect()
    }

    async fn wait_ready(index: &Index) {
        for _ in 0..200 {
            let status = index.status();
            if status.state == IndexState::Ready || status.state == IndexState::Failed {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("index did not finish: {:?}", index.status());
    }

    #[tokio::test]
    async fn hash_skip_and_secret_and_gitignore() {
        let root = temp_workspace();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(root.join("ignored.rs"), "fn ignored() {}\n").unwrap();
        fs::write(root.join(".env"), "SECRET=1\n").unwrap();
        fs::write(root.join("credentials.json"), "{}\n").unwrap();
        fs::write(root.join("kept.rs"), "fn append() {}\n").unwrap();
        let fake = Arc::new(FakeEmbedder::new(4));
        let db = root.join("index.sqlite");
        let workspace_id = WorkspaceId::new();
        let index = Index::start(
            workspace_id,
            root.clone(),
            db.clone(),
            Arc::clone(&fake) as Arc<dyn Embedder>,
            Arc::new(|_| {}),
        );
        wait_ready(&index).await;
        let first = fake.document_calls();
        assert!(first >= 1, "expected an embed, got {first}");
        let connection = store::open_connection(&db).unwrap();
        assert!(store::stored_hash(&connection, "kept.rs")
            .unwrap()
            .is_some());
        assert!(store::stored_hash(&connection, "ignored.rs")
            .unwrap()
            .is_none());
        assert!(store::stored_hash(&connection, ".env").unwrap().is_none());
        assert!(store::stored_hash(&connection, "credentials.json")
            .unwrap()
            .is_none());
        drop(connection);
        index.stop();
        drop(index);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let second = Index::start(
            workspace_id,
            root.clone(),
            db,
            Arc::clone(&fake) as Arc<dyn Embedder>,
            Arc::new(|_| {}),
        );
        wait_ready(&second).await;
        assert_eq!(fake.document_calls(), first);
        fs::write(root.join("kept.rs"), "fn append() { let _changed = 1; }\n").unwrap();
        let mut saw_edit = false;
        for _ in 0..40 {
            if fake.document_calls() > first {
                saw_edit = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_edit, "watcher did not re-embed the changed file");
        second.stop();
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_syntax_error_does_not_stop_the_next_file() {
        let root = temp_workspace();
        fs::write(root.join("broken.rs"), "fn append( {\n").unwrap();
        fs::write(root.join("ok.rs"), "fn append() {}\n").unwrap();
        let fake = Arc::new(FakeEmbedder::new(4));
        let db = root.join("index.sqlite");
        let index = Index::start(
            WorkspaceId::new(),
            root.clone(),
            db.clone(),
            fake,
            Arc::new(|_| {}),
        );
        wait_ready(&index).await;
        let connection = store::open_connection(&db).unwrap();
        assert!(store::stored_hash(&connection, "broken.rs")
            .unwrap()
            .is_some());
        assert!(store::stored_hash(&connection, "ok.rs").unwrap().is_some());
        index.stop();
        let _ = fs::remove_dir_all(&root);
    }
}

//! One language-server process and the documents it has open.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::lsp_types::notification::{
    Cancel, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Exit, Initialized, LogMessage, Progress, PublishDiagnostics, ShowMessage,
};
use async_lsp::lsp_types::request::{
    ApplyWorkspaceEdit, GotoDefinition, HoverRequest, Initialize, References, RegisterCapability,
    Shutdown, WorkDoneProgressCreate, WorkspaceConfiguration, WorkspaceFoldersRequest,
    WorkspaceSymbolRequest,
};
use async_lsp::lsp_types::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResponse, CancelParams, ClientCapabilities,
    ClientInfo, ConfigurationItem, ConfigurationParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    GotoCapability, GotoDefinitionParams, GotoDefinitionResponse, HoverClientCapabilities,
    HoverContents, HoverParams, InitializeParams, InitializedParams, LogMessageParams,
    MarkupContent, MarkupKind, MessageType, NumberOrString, PartialResultParams, Position,
    ProgressParams, ProgressParamsValue, PublishDiagnosticsClientCapabilities,
    PublishDiagnosticsParams, ReferenceClientCapabilities, ReferenceContext, ReferenceParams,
    RegistrationParams, ShowMessageParams, TextDocumentClientCapabilities,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, TextDocumentSyncClientCapabilities, Url,
    VersionedTextDocumentIdentifier, WindowClientCapabilities, WorkDoneProgress,
    WorkDoneProgressCreateParams, WorkDoneProgressParams, WorkspaceClientCapabilities,
    WorkspaceFolder, WorkspaceSymbolClientCapabilities, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::{MainLoop, ServerSocket};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Notify;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;
use tower::ServiceBuilder;

use super::catalog::ServerSpec;
use super::convert::{points_from_links, points_from_locations, Point};
use super::Timing;

const MAX_OPEN: usize = 32;
pub(crate) const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("cancelled")]
    Cancelled,
    #[error("timeout")]
    Timeout,
    #[error("stopped")]
    Stopped,
    #[error("{0}")]
    Protocol(String),
}

struct Published {
    version: Option<i32>,
    at: Instant,
    items: Vec<async_lsp::lsp_types::Diagnostic>,
}

struct Inner {
    diagnostics: HashMap<Url, Published>,
    progress: HashSet<String>,
}

struct Shared {
    root_uri: Url,
    folder_name: String,
    settings: Value,
    inner: Mutex<Inner>,
    changed: Notify,
}

impl Shared {
    fn publish(&self, params: PublishDiagnosticsParams) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        inner.diagnostics.insert(
            params.uri,
            Published {
                version: params.version,
                at: Instant::now(),
                items: params.diagnostics,
            },
        );
        drop(inner);
        self.changed.notify_waiters();
    }

    fn progress(&self, params: ProgressParams) {
        let ProgressParamsValue::WorkDone(progress) = params.value;
        let key = token_key(&params.token);
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        match progress {
            WorkDoneProgress::Begin(_) => {
                inner.progress.insert(key);
            }
            WorkDoneProgress::End(_) => {
                inner.progress.remove(&key);
            }
            WorkDoneProgress::Report(_) => {}
        }
        drop(inner);
        self.changed.notify_waiters();
    }

    fn begin_progress(&self, params: &WorkDoneProgressCreateParams) {
        let mut inner = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        inner.progress.insert(token_key(&params.token));
        drop(inner);
        self.changed.notify_waiters();
    }

    fn progress_open(&self) -> bool {
        !self
            .inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .progress
            .is_empty()
    }

    fn published(
        &self,
        uri: &Url,
    ) -> Option<(Option<i32>, Instant, Vec<async_lsp::lsp_types::Diagnostic>)> {
        self.inner
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .diagnostics
            .get(uri)
            .map(|published| (published.version, published.at, published.items.clone()))
    }
}

fn token_key(token: &NumberOrString) -> String {
    match token {
        NumberOrString::Number(number) => number.to_string(),
        NumberOrString::String(text) => text.clone(),
    }
}

pub struct SyncedDoc {
    pub uri: Url,
    pub version: i32,
    pub synced_at: Instant,
}

struct OpenDoc {
    version: i32,
    modified: SystemTime,
    len: u64,
    used: Instant,
    synced_at: Instant,
}

struct SendState {
    next_id: i32,
}

pub struct LspClient {
    socket: ServerSocket,
    shared: Arc<Shared>,
    pub server_id: &'static str,
    docs: Mutex<HashMap<Url, OpenDoc>>,
    sync_lock: tokio::sync::Mutex<()>,
    send: Mutex<SendState>,
    running: Arc<AtomicBool>,
    idle_stop: AtomicBool,
    inflight: AtomicUsize,
    last_used: Mutex<Instant>,
    child: Mutex<Option<Child>>,
    timing: Timing,
}

pub struct Flight<'a> {
    client: &'a LspClient,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.client.inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl LspClient {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn idle_stop(&self) -> bool {
        self.idle_stop.load(Ordering::SeqCst)
    }

    pub fn touch(&self) -> Flight<'_> {
        self.inflight.fetch_add(1, Ordering::SeqCst);
        *self.last_used.lock().unwrap_or_else(|err| err.into_inner()) = Instant::now();
        Flight { client: self }
    }

    pub fn is_idle(&self, idle: Duration) -> bool {
        if self.inflight.load(Ordering::SeqCst) > 0 || self.shared.progress_open() {
            return false;
        }
        self.last_used
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .elapsed()
            >= idle
    }

    pub fn is_open(&self, absolute: &Path) -> bool {
        let Ok(uri) = file_url(absolute) else {
            return false;
        };
        self.docs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&uri)
    }

    pub async fn sync(
        &self,
        absolute: &Path,
        language_id: &str,
        text: &str,
        modified: SystemTime,
        len: u64,
    ) -> Result<SyncedDoc, LspError> {
        let _flight = self.touch();
        let _order = self.sync_lock.lock().await;
        let uri = file_url(absolute)?;
        let now = Instant::now();
        let mut docs = self.docs.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(open) = docs.get_mut(&uri) {
            open.used = now;
            if open.modified == modified && open.len == len {
                return Ok(SyncedDoc {
                    uri,
                    version: open.version,
                    synced_at: open.synced_at,
                });
            }
            open.version += 1;
            open.modified = modified;
            open.len = len;
            open.synced_at = now;
            let version = open.version;
            drop(docs);
            self.did_change(&uri, version, text)?;
            self.did_save(&uri, text)?;
            return Ok(SyncedDoc {
                uri,
                version,
                synced_at: now,
            });
        }
        if docs.len() >= MAX_OPEN {
            if let Some(stale) = docs
                .iter()
                .min_by_key(|(_, doc)| doc.used)
                .map(|(uri, _)| uri.clone())
            {
                docs.remove(&stale);
                drop(docs);
                self.did_close(&stale)?;
                docs = self.docs.lock().unwrap_or_else(|err| err.into_inner());
            }
        }
        docs.insert(
            uri.clone(),
            OpenDoc {
                version: 1,
                modified,
                len,
                used: now,
                synced_at: now,
            },
        );
        drop(docs);
        self.did_open(&uri, language_id, text)?;
        self.did_save(&uri, text)?;
        Ok(SyncedDoc {
            uri,
            version: 1,
            synced_at: now,
        })
    }

    pub async fn close_if_open(&self, absolute: &Path) {
        let Ok(uri) = file_url(absolute) else {
            return;
        };
        let _order = self.sync_lock.lock().await;
        let removed = self
            .docs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&uri)
            .is_some();
        if removed {
            let _ = self.did_close(&uri);
        }
    }

    pub async fn definition(
        &self,
        uri: &Url,
        position: Position,
        cancel: &CancellationToken,
    ) -> Result<Vec<Point>, LspError> {
        let _flight = self.touch();
        let response = self
            .request::<GotoDefinition>(
                GotoDefinitionParams {
                    text_document_position_params: at(uri, position),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                },
                cancel,
                self.timing.request,
            )
            .await?;
        Ok(match response {
            Some(GotoDefinitionResponse::Scalar(location)) => points_from_locations(vec![location]),
            Some(GotoDefinitionResponse::Array(locations)) => points_from_locations(locations),
            Some(GotoDefinitionResponse::Link(links)) => points_from_links(links),
            None => Vec::new(),
        })
    }

    pub async fn references(
        &self,
        uri: &Url,
        position: Position,
        cancel: &CancellationToken,
    ) -> Result<Vec<Point>, LspError> {
        let _flight = self.touch();
        let response = self
            .request::<References>(
                ReferenceParams {
                    text_document_position: at(uri, position),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                },
                cancel,
                self.timing.request,
            )
            .await?;
        Ok(points_from_locations(response.unwrap_or_default()))
    }

    pub async fn hover(
        &self,
        uri: &Url,
        position: Position,
        cancel: &CancellationToken,
    ) -> Result<String, LspError> {
        let _flight = self.touch();
        let response = self
            .request::<HoverRequest>(
                HoverParams {
                    text_document_position_params: at(uri, position),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                },
                cancel,
                self.timing.request,
            )
            .await?;
        Ok(response.map(hover_text).unwrap_or_default())
    }

    pub async fn symbols(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<SymbolHit>, LspError> {
        let _flight = self.touch();
        let response = self
            .request::<WorkspaceSymbolRequest>(
                WorkspaceSymbolParams {
                    query: query.to_owned(),
                    partial_result_params: PartialResultParams::default(),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                },
                cancel,
                self.timing.request,
            )
            .await?;
        Ok(symbol_hits(response))
    }

    /// Wait until diagnostics for `version` settle, or the wait expires.
    ///
    /// The bool is `pending`: the cap was hit with a check still running, or
    /// before any publish for this version.
    pub async fn diagnostics(
        &self,
        uri: &Url,
        version: i32,
        synced_at: Instant,
        cancel: &CancellationToken,
    ) -> Result<(Vec<async_lsp::lsp_types::Diagnostic>, bool), LspError> {
        let _flight = self.touch();
        let deadline = Instant::now() + self.timing.diagnostics;
        loop {
            if cancel.is_cancelled() {
                tracing::info!(server = self.server_id, "diagnostics cancelled");
                return Err(LspError::Cancelled);
            }
            if !self.is_running() {
                tracing::error!(
                    server = self.server_id,
                    "language server stopped during diagnostics"
                );
                return Err(LspError::Stopped);
            }
            let published = self.shared.published(uri);
            let fresh = published
                .as_ref()
                .is_some_and(|(published_version, at, _)| {
                    *published_version == Some(version)
                        || (published_version.is_none() && *at >= synced_at)
                });
            let progress = self.shared.progress_open();
            if fresh {
                let at = published
                    .as_ref()
                    .map(|(_, at, _)| *at)
                    .unwrap_or(synced_at);
                if !progress && at.elapsed() >= self.timing.settle {
                    let items = published.map(|(_, _, items)| items).unwrap_or_default();
                    tracing::info!(
                        server = self.server_id,
                        count = items.len(),
                        "diagnostics settled"
                    );
                    return Ok((items, false));
                }
            }
            if Instant::now() >= deadline {
                let items = if fresh {
                    published.map(|(_, _, items)| items).unwrap_or_default()
                } else {
                    Vec::new()
                };
                tracing::info!(
                    server = self.server_id,
                    count = items.len(),
                    "diagnostics still pending"
                );
                return Ok((items, true));
            }
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(self.timing.settle);
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    tracing::info!(server = self.server_id, "diagnostics cancelled");
                    return Err(LspError::Cancelled);
                }
                () = notified => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    pub async fn shutdown_idle(&self) {
        tracing::info!(server = self.server_id, "stopping an idle language server");
        self.idle_stop.store(true, Ordering::SeqCst);
        match tokio::time::timeout(Duration::from_secs(2), self.socket.request::<Shutdown>(()))
            .await
        {
            Ok(Ok(_)) => {
                tracing::info!(server = self.server_id, "language server shut down");
            }
            Ok(Err(err)) => {
                tracing::warn!(server = self.server_id, %err, "language server shutdown failed");
            }
            Err(_) => {
                tracing::warn!(
                    server = self.server_id,
                    "language server shutdown timed out"
                );
            }
        }
        if self.socket.notify::<Exit>(()).is_err() {
            tracing::warn!(
                server = self.server_id,
                "language server exit notification failed"
            );
        }
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take()
        {
            let _ = child.start_kill();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    fn did_open(&self, uri: &Url, language_id: &str, text: &str) -> Result<(), LspError> {
        self.socket
            .notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id.to_owned(),
                    version: 1,
                    text: text.to_owned(),
                },
            })
            .map_err(|_| {
                tracing::error!(server = self.server_id, %uri, "failed to open a document");
                LspError::Stopped
            })?;
        tracing::info!(server = self.server_id, %uri, "opened a document");
        Ok(())
    }

    fn did_change(&self, uri: &Url, version: i32, text: &str) -> Result<(), LspError> {
        self.socket
            .notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_owned(),
                }],
            })
            .map_err(|_| {
                tracing::error!(server = self.server_id, %uri, "failed to update a document");
                LspError::Stopped
            })?;
        tracing::info!(server = self.server_id, %uri, version, "updated a document");
        Ok(())
    }

    fn did_save(&self, uri: &Url, text: &str) -> Result<(), LspError> {
        self.socket
            .notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                text: Some(text.to_owned()),
            })
            .map_err(|_| {
                tracing::error!(server = self.server_id, %uri, "failed to save a document");
                LspError::Stopped
            })
    }

    fn did_close(&self, uri: &Url) -> Result<(), LspError> {
        self.socket
            .notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .map_err(|_| {
                tracing::error!(server = self.server_id, %uri, "failed to close a document");
                LspError::Stopped
            })?;
        tracing::info!(server = self.server_id, %uri, "closed a document");
        Ok(())
    }

    async fn request<R: async_lsp::lsp_types::request::Request>(
        &self,
        params: R::Params,
        cancel: &CancellationToken,
        limit: Duration,
    ) -> Result<R::Result, LspError> {
        if cancel.is_cancelled() {
            return Err(LspError::Cancelled);
        }
        let (id, fut) = {
            let mut send = self.send.lock().unwrap_or_else(|err| err.into_inner());
            let id = send.next_id;
            send.next_id += 1;
            let fut = self.socket.request::<R>(params);
            (id, fut)
        };
        let cancel_id = NumberOrString::Number(id);
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!(
                    server = self.server_id,
                    method = R::METHOD,
                    "language server request cancelled"
                );
                let _ = self.socket.notify::<Cancel>(CancelParams { id: cancel_id });
                Err(LspError::Cancelled)
            }
            result = tokio::time::timeout(limit, fut) => match result {
                Ok(Ok(value)) => {
                    tracing::info!(
                        server = self.server_id,
                        method = R::METHOD,
                        "language server request succeeded"
                    );
                    Ok(value)
                }
                Ok(Err(async_lsp::Error::ServiceStopped)) => {
                    self.running.store(false, Ordering::SeqCst);
                    tracing::error!(
                        server = self.server_id,
                        method = R::METHOD,
                        "language server stopped during a request"
                    );
                    Err(LspError::Stopped)
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        server = self.server_id,
                        method = R::METHOD,
                        %err,
                        "language server request failed"
                    );
                    Err(LspError::Protocol(err.to_string()))
                }
                Err(_) => {
                    tracing::warn!(
                        server = self.server_id,
                        method = R::METHOD,
                        "language server request timed out"
                    );
                    let _ = self.socket.notify::<Cancel>(CancelParams { id: cancel_id });
                    Err(LspError::Timeout)
                }
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut child) = self
            .child
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take()
        {
            let _ = child.start_kill();
        }
    }
}

pub struct SymbolHit {
    pub name: String,
    pub kind: async_lsp::lsp_types::SymbolKind,
    pub absolute: PathBuf,
    pub line: u32,
    pub character: u32,
}

fn symbol_hits(response: Option<WorkspaceSymbolResponse>) -> Vec<SymbolHit> {
    let Some(response) = response else {
        return Vec::new();
    };
    match response {
        WorkspaceSymbolResponse::Flat(symbols) => symbols
            .into_iter()
            .filter_map(|symbol| {
                let absolute = symbol.location.uri.to_file_path().ok()?;
                Some(SymbolHit {
                    name: symbol.name,
                    kind: symbol.kind,
                    absolute,
                    line: symbol.location.range.start.line,
                    character: symbol.location.range.start.character,
                })
            })
            .collect(),
        WorkspaceSymbolResponse::Nested(symbols) => symbols
            .into_iter()
            .filter_map(|symbol| {
                let (uri, range) = match symbol.location {
                    async_lsp::lsp_types::OneOf::Left(location) => {
                        (location.uri, Some(location.range.start))
                    }
                    async_lsp::lsp_types::OneOf::Right(location) => (location.uri, None),
                };
                let absolute = uri.to_file_path().ok()?;
                let start = range.unwrap_or(Position::new(0, 0));
                Some(SymbolHit {
                    name: symbol.name,
                    kind: symbol.kind,
                    absolute,
                    line: start.line,
                    character: start.character,
                })
            })
            .collect(),
    }
}

fn at(uri: &Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

fn hover_text(hover: async_lsp::lsp_types::Hover) -> String {
    let text = match hover.contents {
        HoverContents::Scalar(marked) => marked_text(marked),
        HoverContents::Array(items) => items
            .into_iter()
            .map(marked_text)
            .collect::<Vec<_>>()
            .join("\n"),
        HoverContents::Markup(MarkupContent { value, .. }) => value,
    };
    text.chars().take(8 * 1024).collect()
}

fn marked_text(marked: async_lsp::lsp_types::MarkedString) -> String {
    match marked {
        async_lsp::lsp_types::MarkedString::String(text) => text,
        async_lsp::lsp_types::MarkedString::LanguageString(text) => text.value,
    }
}

fn file_url(path: &Path) -> Result<Url, LspError> {
    Url::from_file_path(path)
        .map_err(|()| LspError::Protocol(format!("not a file uri: {}", path.display())))
}

pub async fn spawn(
    root: &Path,
    spec: &ServerSpec,
    binary: &Path,
    timing: Timing,
) -> Result<Arc<LspClient>, LspError> {
    let mut command = Command::new(binary);
    command
        .args(spec.argv.iter().skip(1))
        .current_dir(root)
        .env_clear()
        .envs(scrubbed_env())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|err| LspError::Protocol(err.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LspError::Protocol("language server has no stdout".into()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| LspError::Protocol("language server has no stdin".into()))?;
    let stderr_text = Arc::new(Mutex::new(String::new()));
    let stderr_task = child.stderr.take().map(|stderr| {
        let server = spec.id;
        let stderr_text = Arc::clone(&stderr_text);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(server, "{line}");
                push_stderr(&stderr_text, &line);
            }
        })
    });
    let started = connect(root, spec, stdout.compat(), stdin.compat_write(), timing).await;
    let client = match started {
        Ok(client) => client,
        Err(err) => {
            drop(child);
            if let Some(task) = stderr_task {
                let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
            }
            return Err(with_stderr(err, &stderr_text));
        }
    };
    *client.child.lock().unwrap_or_else(|err| err.into_inner()) = Some(child);
    Ok(client)
}

const STDERR_CAP: usize = 300;

fn push_stderr(buf: &Mutex<String>, line: &str) {
    let mut buf = buf.lock().unwrap_or_else(|err| err.into_inner());
    if buf.len() >= STDERR_CAP {
        return;
    }
    if !buf.is_empty() {
        buf.push(' ');
    }
    let room = STDERR_CAP - buf.len();
    buf.extend(line.chars().take(room));
}

fn with_stderr(err: LspError, buf: &Mutex<String>) -> LspError {
    let text = buf.lock().unwrap_or_else(|err| err.into_inner()).clone();
    let text = text.trim();
    if text.is_empty() {
        return match err {
            LspError::Stopped => {
                LspError::Protocol("language server exited before initialize finished".into())
            }
            LspError::Timeout => LspError::Protocol("initialize timed out".into()),
            other => other,
        };
    }
    match err {
        LspError::Timeout => LspError::Protocol(format!("initialize timed out: {text}")),
        LspError::Stopped | LspError::Protocol(_) => LspError::Protocol(text.to_owned()),
        other => other,
    }
}

pub async fn connect<R, W>(
    root: &Path,
    spec: &ServerSpec,
    read: R,
    write: W,
    timing: Timing,
) -> Result<Arc<LspClient>, LspError>
where
    R: futures_io::AsyncRead + Unpin + Send + 'static,
    W: futures_io::AsyncWrite + Unpin + Send + 'static,
{
    let root_uri = file_url(root)?;
    let shared = Arc::new(Shared {
        root_uri: root_uri.clone(),
        folder_name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned(),
        settings: (spec.settings)(),
        inner: Mutex::new(Inner {
            diagnostics: HashMap::new(),
            progress: HashSet::new(),
        }),
        changed: Notify::new(),
    });
    let running = Arc::new(AtomicBool::new(true));
    let (mainloop, socket) = MainLoop::new_client({
        let shared = Arc::clone(&shared);
        move |_server| {
            ServiceBuilder::new()
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router(shared, spec.id))
        }
    });
    let flag = Arc::clone(&running);
    let server = spec.id;
    tokio::spawn(async move {
        if let Err(err) = mainloop.run_buffered(read, write).await {
            tracing::warn!(server, %err, "language server main loop ended");
        } else {
            tracing::info!(server, "language server main loop ended");
        }
        flag.store(false, Ordering::SeqCst);
    });
    let client = Arc::new(LspClient {
        socket,
        shared,
        server_id: spec.id,
        docs: Mutex::new(HashMap::new()),
        sync_lock: tokio::sync::Mutex::new(()),
        send: Mutex::new(SendState { next_id: 0 }),
        running,
        idle_stop: AtomicBool::new(false),
        inflight: AtomicUsize::new(0),
        last_used: Mutex::new(Instant::now()),
        child: Mutex::new(None),
        timing,
    });
    client.initialize().await?;
    Ok(client)
}

impl LspClient {
    async fn initialize(&self) -> Result<(), LspError> {
        let folder = WorkspaceFolder {
            uri: self.shared.root_uri.clone(),
            name: self.shared.folder_name.clone(),
        };
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: Some(self.shared.root_uri.clone()),
            initialization_options: None,
            capabilities: capabilities(),
            trace: None,
            workspace_folders: Some(vec![folder]),
            client_info: Some(ClientInfo {
                name: "robi".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            locale: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let _ = self
            .request::<Initialize>(params, &CancellationToken::new(), self.timing.initialize)
            .await?;
        self.socket
            .notify::<Initialized>(InitializedParams {})
            .map_err(|_| LspError::Stopped)?;
        Ok(())
    }
}

fn log_server_note(server: &'static str, typ: MessageType, message: &str) {
    match typ {
        MessageType::ERROR => {
            tracing::error!(server, message, "language server error");
        }
        MessageType::WARNING => {
            tracing::warn!(server, message, "language server warning");
        }
        MessageType::INFO => {
            tracing::info!(server, message, "language server message");
        }
        _ => {
            tracing::debug!(server, message, "language server log");
        }
    }
}

fn router(shared: Arc<Shared>, server: &'static str) -> Router<()> {
    let mut router = Router::new(());
    let folders = Arc::clone(&shared);
    router.request::<WorkspaceFoldersRequest, _>(move |_, ()| {
        let folders = Arc::clone(&folders);
        async move {
            Ok(Some(vec![WorkspaceFolder {
                uri: folders.root_uri.clone(),
                name: folders.folder_name.clone(),
            }]))
        }
    });
    let settings = Arc::clone(&shared);
    router.request::<WorkspaceConfiguration, _>(move |_, params: ConfigurationParams| {
        let settings = Arc::clone(&settings);
        async move {
            let values = params
                .items
                .into_iter()
                .map(|item: ConfigurationItem| {
                    super::catalog::configuration_value(&settings.settings, item.section.as_deref())
                })
                .collect();
            Ok(values)
        }
    });
    let progress = Arc::clone(&shared);
    router.request::<WorkDoneProgressCreate, _>(move |_, params| {
        let progress = Arc::clone(&progress);
        async move {
            progress.begin_progress(&params);
            Ok(())
        }
    });
    router.request::<RegisterCapability, _>(|_, _params: RegistrationParams| async move { Ok(()) });
    router.request::<ApplyWorkspaceEdit, _>(|_, _params: ApplyWorkspaceEditParams| async move {
        Ok(ApplyWorkspaceEditResponse {
            applied: false,
            failure_reason: Some("robi applies edits itself".into()),
            failed_change: None,
        })
    });
    let published = Arc::clone(&shared);
    router.notification::<PublishDiagnostics>(move |_, params| {
        published.publish(params);
        std::ops::ControlFlow::Continue(())
    });
    let progressing = Arc::clone(&shared);
    router.notification::<Progress>(move |_, params| {
        progressing.progress(params);
        std::ops::ControlFlow::Continue(())
    });
    router.notification::<ShowMessage>(move |_, params: ShowMessageParams| {
        log_server_note(server, params.typ, &params.message);
        std::ops::ControlFlow::Continue(())
    });
    router.notification::<LogMessage>(move |_, params: LogMessageParams| {
        log_server_note(server, params.typ, &params.message);
        std::ops::ControlFlow::Continue(())
    });
    router.unhandled_notification(|_, _| std::ops::ControlFlow::Continue(()));
    router
}

fn capabilities() -> ClientCapabilities {
    ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            apply_edit: Some(false),
            symbol: Some(WorkspaceSymbolClientCapabilities::default()),
            workspace_folders: Some(true),
            configuration: Some(true),
            ..WorkspaceClientCapabilities::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: Some(false),
                will_save: Some(false),
                will_save_wait_until: Some(false),
                did_save: Some(true),
            }),
            hover: Some(HoverClientCapabilities {
                dynamic_registration: Some(false),
                content_format: Some(vec![MarkupKind::PlainText]),
            }),
            references: Some(ReferenceClientCapabilities {
                dynamic_registration: Some(false),
            }),
            definition: Some(GotoCapability {
                dynamic_registration: Some(false),
                link_support: Some(false),
            }),
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                version_support: Some(true),
                ..PublishDiagnosticsClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..WindowClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}

fn scrubbed_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(key, _)| !crate::sandbox::secret_name(key))
        .collect()
}

#[cfg(test)]
pub(crate) async fn connect_fake(root: &Path, timing: Timing) -> Arc<LspClient> {
    let (client_read, server_write) = tokio::io::duplex(64 * 1024);
    let (server_read, client_write) = tokio::io::duplex(64 * 1024);
    let symbol_at = root.to_path_buf();
    tokio::spawn(async move {
        let (mainloop, _socket) =
            MainLoop::new_server(move |socket| fake_router(socket, symbol_at.clone()));
        let _ = mainloop
            .run_buffered(server_read.compat(), server_write.compat_write())
            .await;
    });
    let spec = super::catalog::spec("rust-analyzer").expect("rust-analyzer row");
    connect(
        root,
        spec,
        client_read.compat(),
        client_write.compat_write(),
        timing,
    )
    .await
    .expect("fake language server initializes")
}

#[cfg(test)]
struct FakeState {
    socket: async_lsp::ClientSocket,
    root: PathBuf,
}

#[cfg(test)]
#[allow(deprecated)]
fn fake_router(socket: async_lsp::ClientSocket, root: PathBuf) -> Router<FakeState> {
    use async_lsp::lsp_types::notification::DidSaveTextDocument;
    use async_lsp::lsp_types::request::{GotoDefinition, HoverRequest, Initialize, References};
    use async_lsp::lsp_types::{
        ConfigurationItem, ConfigurationParams, Diagnostic, DiagnosticSeverity, Hover,
        InitializeResult, Location, OneOf, Position, PublishDiagnosticsParams, Range,
        ServerCapabilities, SymbolInformation, SymbolKind, TextDocumentSyncCapability,
        TextDocumentSyncKind, WorkspaceSymbolResponse,
    };
    use std::ops::ControlFlow;

    let mut router = Router::new(FakeState { socket, root });
    router.request::<Initialize, _>(|state, _| {
        let socket = state.socket.clone();
        async move {
            socket
                .request::<WorkspaceFoldersRequest>(())
                .await
                .expect("workspace folders");
            let config = socket
                .request::<WorkspaceConfiguration>(ConfigurationParams {
                    items: vec![ConfigurationItem {
                        scope_uri: None,
                        section: Some("rust-analyzer".into()),
                    }],
                })
                .await
                .expect("configuration");
            let section = config.first().expect("rust-analyzer section");
            assert_eq!(section.get("checkOnSave"), Some(&serde_json::json!(true)));
            assert_eq!(
                section.pointer("/cargo/targetDir"),
                Some(&serde_json::json!(true))
            );
            assert_eq!(
                section.pointer("/cargo/extraArgs"),
                Some(&serde_json::json!(["--locked"]))
            );
            Ok(InitializeResult {
                capabilities: ServerCapabilities {
                    text_document_sync: Some(TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
                    )),
                    definition_provider: Some(OneOf::Left(true)),
                    references_provider: Some(OneOf::Left(true)),
                    hover_provider: Some(async_lsp::lsp_types::HoverProviderCapability::Simple(
                        true,
                    )),
                    workspace_symbol_provider: Some(OneOf::Left(true)),
                    ..ServerCapabilities::default()
                },
                server_info: None,
            })
        }
    });
    router.notification::<DidSaveTextDocument>(|state, params| {
        let _ = state
            .socket
            .notify::<PublishDiagnostics>(PublishDiagnosticsParams {
                uri: params.text_document.uri,
                diagnostics: vec![Diagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("E0425".into())),
                    code_description: None,
                    source: Some("fake".into()),
                    message: "cannot find value `missing` in this scope".into(),
                    related_information: None,
                    tags: None,
                    data: None,
                }],
                version: Some(1),
            });
        ControlFlow::Continue(())
    });
    router.request::<GotoDefinition, _>(|_, params| async move {
        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
            range: Range::new(Position::new(0, 3), Position::new(0, 7)),
        })))
    });
    router.request::<References, _>(|_, params| async move {
        let uri = params.text_document_position.text_document.uri.clone();
        Ok(Some(vec![Location {
            uri,
            range: Range::new(Position::new(0, 3), Position::new(0, 7)),
        }]))
    });
    router.request::<HoverRequest, _>(|_, _| async move {
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::PlainText,
                value: "fn demo".into(),
            }),
            range: None,
        }))
    });
    router.request::<WorkspaceSymbolRequest, _>(|state, _| {
        let uri = Url::from_file_path(state.root.join("lib.rs")).expect("symbol uri");
        async move {
            Ok(Some(WorkspaceSymbolResponse::Flat(vec![
                SymbolInformation {
                    name: "demo".into(),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri,
                        range: Range::new(Position::new(0, 3), Position::new(0, 7)),
                    },
                    container_name: None,
                },
            ])))
        }
    });
    router.unhandled_notification(|_, _| ControlFlow::Continue(()));
    router
}

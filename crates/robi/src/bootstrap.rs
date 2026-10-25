//! Shared composition root for `robi-api` and the Tauri process.
//!
//! Builds [`AppState`](crate::web_api::state::AppState), binds a loopback
//! listener, and returns the router. Callers run the server on their own runtime.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use http::{header::CONTENT_TYPE, HeaderValue, Method};
use robi_core::config::LoopConfig;
use robi_core::tool::ToolRegistry;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    adapters::{
        chat_image_store::BlobImageStore,
        chat_message::SqliteMessageStore,
        chat_runtime::{AgentFactory, SerializedChatRuntime},
        chat_session::repo::SqliteChatSessionRepository,
        file_change::repo::SqliteFileChangeRepository,
        model_source::SettingsModelSource,
        originals::BlobOriginals,
        session_blobs::{evacuate_sqlite, SessionBlobs},
        session_data::SessionCleanup,
        settings::TomlSettingsStore,
        sqlite,
        workspace::repo::SqliteWorkspaceRepository,
        workspace_assets::WorkspaceIndexAssets,
    },
    agent::providers::ImageSource,
    agent::{compress::OriginalStore, mcp::McpHub, web::BraveSearch},
    domain::{
        chat_message::service::ChatMessageService,
        chat_session::service::ChatSessionService,
        events::{BusEventSink, EventBus},
        file_change::repo::FileChangeRepository,
        settings::{store::SettingsStore, SettingsService},
        workspace::{assets::WorkspaceAssetCleaner, service::WorkspaceService},
    },
    web_api::{self, state::AppState},
};

/// Default listen port. Vite owns `1430`.
pub const DEFAULT_PORT: u16 = 1431;

/// The Tauri identifier from `src-tauri/tauri.conf.json`.
///
/// Tauri derives its app data directory from this, and the terminal front end
/// has to open the same database, so the two agree on it here.
pub const DESKTOP_IDENTIFIER: &str = "com.github.cgund98.robi";

/// The database URL `robi-tui` uses when `ROBI_DATABASE_URL` is unset.
///
/// This mirrors `src-tauri/src/lib.rs::database_url`, which is the
/// authoritative version: it asks Tauri for `app_data_dir()` and appends `dev`
/// when `is_dev()`. The terminal cannot call that (nothing depends on
/// `src-tauri`), so it repeats the rule. A terminal started from a source
/// checkout is a development build, so it opens the **dev** database, the same
/// one `pnpm tauri dev` uses.
///
/// `None` when the platform data directory cannot be resolved, and the caller
/// falls back to a path relative to the working directory.
pub fn desktop_dev_database_url() -> Option<String> {
    Some(desktop_dev_database_url_in(&data_dir()?))
}

/// The URL for one platform data directory. Separated from [`data_dir`] so the
/// rule is a unit test rather than an environment variable.
fn desktop_dev_database_url_in(data_dir: &std::path::Path) -> String {
    let dir = data_dir.join(DESKTOP_IDENTIFIER).join("dev");
    sqlite_url(&dir.join("robi.db"))
}

/// `sqlite://{path}?mode=rwc`, the shape `src-tauri` writes.
fn sqlite_url(path: &std::path::Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

/// The platform's per-user data directory, as Tauri's `app_data_dir()` sees it
/// before the identifier is appended.
fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(|home| {
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
            });
    }
    // Everything else follows the XDG base directory specification.
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
}

/// How many ports to try, starting at [`DEFAULT_PORT`], when no address is fixed.
pub const PORT_ATTEMPTS: u16 = 20;

/// What the composition root needs that is not a default.
pub struct AppConfig {
    pub database_url: String,
    pub settings_dir: PathBuf,
}

/// Where to listen.
pub enum Listen {
    /// Bind this address. Fail when it is taken.
    Exact(SocketAddr),
    /// Try `127.0.0.1:port`, then the following ports, up to `attempts`.
    Fallback { port: u16, attempts: u16 },
}

/// Why startup or bind failed.
#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("failed to open database: {0}")]
    Database(#[from] sqlite::InitError),

    #[error("{0}")]
    Settings(#[from] crate::adapters::settings::SettingsLoadError),

    #[error("{0}")]
    Bind(String),

    #[error("failed to prune chat sessions: {0}")]
    Prune(String),

    #[error("failed to move session blobs out of sqlite: {0}")]
    Blobs(String),
}

/// Parse `ROBI_BIND`. The address must be loopback.
pub fn parse_bind(raw: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = raw
        .parse()
        .map_err(|_| format!("ROBI_BIND must be an address and port, got {raw}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!("ROBI_BIND must be a loopback address, got {raw}"));
    }
    Ok(addr)
}

/// Wire the pool, settings, tools, and chat runtime.
pub async fn build_app_state(config: AppConfig) -> Result<AppState, BootstrapError> {
    let pool = Arc::new(sqlite::init_pool(&config.database_url).await?);
    tracing::info!("opened the database");
    let settings_store = Arc::new(TomlSettingsStore::load(&config.settings_dir)?);
    tracing::info!(dir = %config.settings_dir.display(), "loaded settings");
    let settings: Arc<dyn SettingsStore> = settings_store;
    let settings_service = Arc::new(SettingsService {
        store: Arc::clone(&settings),
    });
    let search = Arc::new(BraveSearch::new(
        Arc::clone(&settings_service),
        reqwest::Client::new(),
    ));
    let tools = Arc::new(ToolRegistry::new());
    let event_bus = Arc::new(EventBus::new());
    let index_assets = Arc::new(WorkspaceIndexAssets::new(config.settings_dir.clone()));
    let blobs = SessionBlobs::new(SessionBlobs::directory(&config.settings_dir));
    evacuate_sqlite(&pool, &blobs)
        .await
        .map_err(BootstrapError::Blobs)?;
    let store: Arc<dyn robi_core::store::MessageStore> =
        Arc::new(SqliteMessageStore::new(Arc::clone(&pool), blobs.clone()));
    let image_source: Arc<BlobImageStore> = Arc::new(BlobImageStore::new(blobs.clone()));
    let originals: Arc<dyn OriginalStore> = Arc::new(BlobOriginals::new(blobs.clone()));
    let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
    let chat_session_service = Arc::new(ChatSessionService {
        repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
        workspaces: workspaces.clone(),
        events: Some(Arc::clone(&event_bus)),
        session_cleaner: Some(Arc::new(SessionCleanup::new(blobs))),
    });
    let file_changes: Arc<dyn FileChangeRepository> =
        Arc::new(SqliteFileChangeRepository::new(Arc::clone(&pool)));
    let mcp = Arc::new(McpHub::new(
        Arc::clone(&settings),
        config.settings_dir,
        Arc::clone(&event_bus),
    ));
    let runtime = Arc::new(SerializedChatRuntime::new(AgentFactory {
        store: Arc::clone(&store),
        events: Arc::new(BusEventSink::new(Arc::clone(&event_bus))),
        models: Arc::new(SettingsModelSource::new(
            Arc::clone(&settings),
            Arc::clone(&image_source) as Arc<dyn ImageSource>,
        )),
        tools,
        config: LoopConfig::default(),
        sessions: Some(Arc::clone(&chat_session_service)),
        file_changes: Some(Arc::clone(&file_changes)),
        bus: Some(Arc::clone(&event_bus)),
        search,
        settings: Some(Arc::clone(&settings)),
        mcp: Some(Arc::clone(&mcp)),
        originals: Some(Arc::clone(&originals)),
    }));
    let removed = chat_session_service
        .prune_excess_sessions()
        .await
        .map_err(|err| BootstrapError::Prune(err.to_string()))?;
    if removed > 0 {
        tracing::info!(removed, "pruned chat sessions");
    }
    chat_session_service
        .reconcile_turn_displays(store.as_ref())
        .await
        .map_err(|err| BootstrapError::Prune(err.to_string()))?;
    // Removes a sandbox container that nothing has run in for a while. On the
    // runtime that serves the API, so it ends with the process.
    crate::agent::sandbox::container::reaper::spawn();
    Ok(AppState {
        workspace_service: Arc::new(WorkspaceService {
            repository: workspaces,
            asset_cleaner: Some(index_assets as Arc<dyn WorkspaceAssetCleaner>),
        }),
        chat_session_service: Arc::clone(&chat_session_service),
        chat_message_service: Arc::new(ChatMessageService {
            sessions: chat_session_service,
            runtime,
            store,
        }),
        settings_service,
        event_bus,
        file_changes,
        mcp: Some(mcp),
        originals,
        image_source,
        docs_edits: Arc::new(crate::agent::docs::DocsEditCache::default()),
    })
}

/// The HTTP app: API routes, Swagger UI, and the CORS allow-list.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(web_api::router(state))
        .merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json", web_api::openapi()))
        .layer(cors_layer())
}

/// A running API server: where it listens, and how to stop it.
///
/// The desktop app holds one so it can shut the server down, and with it the
/// sandbox containers, before the process exits. `robi-api` and `robi-tui` own
/// the runtime themselves and do not need it.
pub struct ApiServer {
    addr: SocketAddr,
    shutdown: CancellationToken,
    done: std::sync::mpsc::Receiver<()>,
}

impl ApiServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops the server, waits for the sandbox cleanup, and gives up after
    /// `timeout`.
    ///
    /// Returns whether the cleanup finished. Called on the way out, so a slow or
    /// wedged daemon delays the quit by at most `timeout` and never hangs it.
    pub fn shutdown(self, timeout: Duration) -> bool {
        self.shutdown.cancel();
        match self.done.recv_timeout(timeout) {
            Ok(()) => true,
            Err(_) => {
                tracing::warn!("the API server did not shut down in time");
                false
            }
        }
    }
}

/// Removes every sandbox container this process owns.
///
/// The containers are labels in a daemon, not files a process holds, so they
/// outlive an exit unless something removes them. Every surface calls this once,
/// late, on its own runtime: the Docker client is only used from the runtime
/// that built it.
pub async fn shutdown_sandboxes() {
    let removed = crate::agent::sandbox::container::Registry::global()
        .shutdown_all()
        .await;
    if removed > 0 {
        tracing::info!(removed, "removed sandbox containers on exit");
    }
}

/// Resolves on `Ctrl-C` or, on Unix, `SIGTERM`.
///
/// `SIGTERM` is what a supervisor sends and what a terminal close turns into, so
/// handling only `Ctrl-C` would leave the common case unhandled. A `SIGKILL` and
/// a crash cannot be caught, and the sandbox containers from those are the gap
/// the container page documents.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!(%err, "could not listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                tracing::warn!(%err, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutting down");
}

/// Start the server on a background thread. Returns after the listener is bound.
pub fn spawn(config: AppConfig, listen: Listen) -> Result<ApiServer, BootstrapError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let shutdown = CancellationToken::new();
    let serve_token = shutdown.clone();
    std::thread::Builder::new()
        .name("robi-api".into())
        .spawn(move || {
            let report = tx.clone();
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = tx.send(Err(BootstrapError::Bind(format!(
                            "failed to start the server runtime: {err}"
                        ))));
                        return;
                    }
                };
                let started = rt.block_on(async move {
                    let state = build_app_state(config).await?;
                    let listener = bind(listen).await?;
                    let addr = listener.local_addr().map_err(|err| {
                        BootstrapError::Bind(format!("failed to read the bound address: {err}"))
                    })?;
                    let app = router(state);
                    Ok::<_, BootstrapError>((listener, app, addr))
                });
                match started {
                    Ok((listener, app, addr)) => {
                        let _ = tx.send(Ok(addr));
                        let served = rt.block_on(async move {
                            let stop = async move { serve_token.cancelled().await };
                            axum::serve(listener, app)
                                .with_graceful_shutdown(stop)
                                .await
                        });
                        if let Err(err) = served {
                            tracing::error!(%err, "server stopped");
                        }
                        // On this runtime, where the Docker client was built.
                        rt.block_on(shutdown_sandboxes());
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err));
                    }
                }
                let _ = done_tx.send(());
            }));
            if let Err(payload) = panicked {
                let message = panic_payload(payload.as_ref());
                tracing::error!(%message, "server thread panicked");
                crate::logs::record_error(&format!("server thread panicked: {message}"));
                let _ = report.send(Err(BootstrapError::Bind(format!(
                    "the server thread panicked: {message}"
                ))));
            }
        })
        .map_err(|err| BootstrapError::Bind(format!("failed to spawn the server thread: {err}")))?;
    let addr = rx.recv().map_err(|_| {
        BootstrapError::Bind("the server thread stopped before it bound a port".into())
    })??;
    Ok(ApiServer {
        addr,
        shutdown,
        done: done_rx,
    })
}

/// Bind `listen` and return the listener. The bound port is `local_addr`.
pub async fn bind(listen: Listen) -> Result<TcpListener, BootstrapError> {
    match listen {
        Listen::Exact(addr) => TcpListener::bind(addr)
            .await
            .map_err(|err| BootstrapError::Bind(format!("failed to bind {addr}: {err}"))),
        Listen::Fallback { port, attempts } => {
            if attempts == 0 {
                return Err(BootstrapError::Bind(
                    "port fallback needs at least one attempt".into(),
                ));
            }
            let mut last = String::new();
            for offset in 0..attempts {
                let candidate =
                    SocketAddr::from((Ipv4Addr::LOCALHOST, port.saturating_add(offset)));
                match TcpListener::bind(candidate).await {
                    Ok(listener) => return Ok(listener),
                    Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                        last = format!("{candidate} is in use");
                    }
                    Err(err) => {
                        return Err(BootstrapError::Bind(format!(
                            "failed to bind {candidate}: {err}"
                        )));
                    }
                }
            }
            Err(BootstrapError::Bind(format!(
                "no open port from {port} through {} ({last})",
                port.saturating_add(attempts - 1)
            )))
        }
    }
}

fn panic_payload(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Box<dyn Any>")
        .to_string()
}

fn cors_layer() -> CorsLayer {
    let origins = [
        "http://localhost:1430",
        "http://127.0.0.1:1430",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "tauri://localhost",
    ]
    .into_iter()
    .map(|origin| {
        origin
            .parse::<HeaderValue>()
            .expect("static CORS origin is valid")
    });
    CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([CONTENT_TYPE])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_accepts_ipv4_and_ipv6_loopback() {
        assert_eq!(
            parse_bind("127.0.0.1:1431").unwrap(),
            "127.0.0.1:1431".parse().unwrap()
        );
        assert_eq!(
            parse_bind("[::1]:1431").unwrap(),
            "[::1]:1431".parse().unwrap()
        );
    }

    #[test]
    fn bind_rejects_a_non_loopback_address() {
        let error = parse_bind("0.0.0.0:1431").unwrap_err();
        assert!(error.contains("loopback"));
    }

    #[tokio::test]
    async fn fallback_skips_a_taken_port() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = taken.local_addr().unwrap().port();
        let listener = bind(Listen::Fallback { port, attempts: 8 }).await.unwrap();
        assert_ne!(listener.local_addr().unwrap().port(), port);
    }

    #[tokio::test]
    async fn exact_bind_fails_when_the_port_is_taken() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = taken.local_addr().unwrap();
        let error = bind(Listen::Exact(addr)).await.unwrap_err();
        assert!(error.to_string().contains("failed to bind"));
    }

    #[test]
    fn the_terminal_opens_the_database_tauri_dev_opens() {
        // `pnpm tauri dev` resolves this itself through Tauri's `app_data_dir`.
        // If that path moves, this test and the terminal are both wrong.
        let url = desktop_dev_database_url_in(std::path::Path::new("/data"));
        assert_eq!(
            url, "sqlite:///data/com.github.cgund98.robi/dev/robi.db?mode=rwc",
            "the identifier and the `dev` subdirectory are the whole rule"
        );
    }

    #[test]
    fn a_path_with_a_space_needs_no_encoding() {
        // The macOS app data directory has a space in it, and sqlx accepts the
        // URL as written.
        let url = desktop_dev_database_url_in(std::path::Path::new(
            "/Home/me/Library/Application Support",
        ));
        assert_eq!(
            url,
            "sqlite:///Home/me/Library/Application Support/com.github.cgund98.robi/dev/robi.db?mode=rwc"
        );
    }

    #[test]
    fn this_platform_resolves_a_data_directory() {
        // The fallback to a working-directory path is for a broken environment,
        // not the normal case.
        assert!(desktop_dev_database_url().is_some());
    }
}

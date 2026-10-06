//! Shared composition root for `robi-api` and the Tauri process.
//!
//! Builds [`AppState`](crate::web_api::state::AppState), binds a loopback
//! listener, and returns the router. Callers run the server on their own runtime.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use http::{header::CONTENT_TYPE, HeaderValue, Method};
use robi_core::config::LoopConfig;
use robi_core::tool::ToolRegistry;
use thiserror::Error;
use tokio::net::TcpListener;
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
        session_plans::FilesystemSessionPlans,
        settings::TomlSettingsStore,
        sqlite,
        workspace::repo::SqliteWorkspaceRepository,
    },
    agent::providers::ImageSource,
    agent::{compress::OriginalStore, index::IndexHub, lsp::LspHub, mcp::McpHub, web::BraveSearch},
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
    let index = Arc::new(IndexHub::new(
        config.settings_dir.clone(),
        Arc::clone(&event_bus),
        Arc::new(robi_index::LocalEmbedder::new(robi_index::model_cache_dir(
            &config.settings_dir,
        ))),
    ));
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
        plan_cleaner: Some(Arc::new(
            FilesystemSessionPlans::default().with_blobs(blobs),
        )),
    });
    let file_changes: Arc<dyn FileChangeRepository> =
        Arc::new(SqliteFileChangeRepository::new(Arc::clone(&pool)));
    let mcp = Arc::new(McpHub::new(
        Arc::clone(&settings),
        config.settings_dir,
        Arc::clone(&event_bus),
    ));
    let lsp_path_entries = settings
        .get(crate::domain::settings::keys::PATH_ENTRIES)
        .await
        .ok()
        .flatten()
        .map(|setting| setting.value)
        .unwrap_or_default();
    let lsp_path =
        crate::agent::blocking::call(move || crate::agent::mcp::resolve_path(&lsp_path_entries))
            .await
            .unwrap_or_else(|_| std::env::var("PATH").unwrap_or_default());
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
        index: Some(Arc::clone(&index)),
        lsp: Some(LspHub::with_search_path(lsp_path)),
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
    Ok(AppState {
        workspace_service: Arc::new(WorkspaceService {
            repository: workspaces,
            asset_cleaner: Some(index.clone() as Arc<dyn WorkspaceAssetCleaner>),
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
        index,
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

/// Start the server on a background thread. Returns after the listener is bound.
pub fn spawn(config: AppConfig, listen: Listen) -> Result<SocketAddr, BootstrapError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
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
                        if let Err(err) =
                            rt.block_on(async move { axum::serve(listener, app).await })
                        {
                            tracing::error!(%err, "server stopped");
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err));
                    }
                }
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
    rx.recv().map_err(|_| {
        BootstrapError::Bind("the server thread stopped before it bound a port".into())
    })?
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
}

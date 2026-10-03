use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use robi_core::config::LoopConfig;
use robi_core::tool::ToolRegistry;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa_swagger_ui::SwaggerUi;

use robi::{
    adapters::{
        chat_message::SqliteMessageStore,
        chat_runtime::{AgentFactory, SerializedChatRuntime},
        chat_session::repo::SqliteChatSessionRepository,
        file_change::repo::SqliteFileChangeRepository,
        model_source::SettingsModelSource,
        settings::{home_dir, TomlSettingsStore},
        sqlite,
        workspace::repo::SqliteWorkspaceRepository,
    },
    domain::{
        chat_message::service::ChatMessageService,
        chat_session::service::ChatSessionService,
        events::{EventFanOut, FanOutEventSink},
        file_change::repo::FileChangeRepository,
        settings::{store::SettingsStore, SettingsService},
        workspace::service::WorkspaceService,
    },
    web_api::{self, state::AppState},
};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "robi=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = std::env::var("ROBI_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://robi.db?mode=rwc".to_string());
    let bind = std::env::var("ROBI_BIND").unwrap_or_else(|_| "127.0.0.1:1431".to_string());
    let addr = match bind_addr_from(&bind) {
        Ok(addr) => addr,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let pool = match sqlite::init_pool(&database_url).await {
        Ok(pool) => Arc::new(pool),
        Err(err) => {
            eprintln!("failed to open database: {err}");
            std::process::exit(1);
        }
    };
    let settings_dir = match home_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let settings_store = match TomlSettingsStore::load(&settings_dir) {
        Ok(store) => Arc::new(store),
        Err(err) => {
            eprintln!("failed to load settings: {err}");
            std::process::exit(1);
        }
    };
    let settings: Arc<dyn SettingsStore> = settings_store;
    let tools = Arc::new(ToolRegistry::new());
    let event_fanout = Arc::new(EventFanOut::new());
    let store: Arc<dyn robi_core::store::MessageStore> =
        Arc::new(SqliteMessageStore::new(Arc::clone(&pool)));
    let workspaces = Arc::new(SqliteWorkspaceRepository::new(Arc::clone(&pool)));
    let chat_session_service = Arc::new(ChatSessionService {
        repository: Arc::new(SqliteChatSessionRepository::new(Arc::clone(&pool))),
        workspaces: workspaces.clone(),
    });
    let file_changes: Arc<dyn FileChangeRepository> =
        Arc::new(SqliteFileChangeRepository::new(Arc::clone(&pool)));
    let runtime = Arc::new(SerializedChatRuntime::new(AgentFactory {
        store: Arc::clone(&store),
        events: Arc::new(FanOutEventSink::new(Arc::clone(&event_fanout))),
        models: Arc::new(SettingsModelSource::new(Arc::clone(&settings))),
        tools,
        config: LoopConfig::default(),
        sessions: Some(Arc::clone(&chat_session_service)),
        file_changes: Some(Arc::clone(&file_changes)),
        fanout: Some(Arc::clone(&event_fanout)),
    }));
    let state = AppState {
        workspace_service: Arc::new(WorkspaceService {
            repository: workspaces,
        }),
        chat_session_service: Arc::clone(&chat_session_service),
        chat_message_service: Arc::new(ChatMessageService {
            sessions: chat_session_service,
            runtime,
            store,
        }),
        settings_service: Arc::new(SettingsService { store: settings }),
        event_fanout,
        file_changes,
    };

    let app = Router::new()
        .merge(web_api::router(state))
        .merge(SwaggerUi::new("/docs").url("/api-doc/openapi.json", web_api::openapi()));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("failed to bind {addr}: {err}");
            std::process::exit(1);
        }
    };
    tracing::info!(%addr, "robi-api listening");

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("server stopped: {err}");
        std::process::exit(1);
    }
}

fn bind_addr_from(raw: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = raw
        .parse()
        .map_err(|_| format!("ROBI_BIND must be an address and port, got {raw}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!("ROBI_BIND must be a loopback address, got {raw}"));
    }
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_accepts_ipv4_and_ipv6_loopback() {
        assert_eq!(
            bind_addr_from("127.0.0.1:1431").unwrap(),
            "127.0.0.1:1431".parse().unwrap()
        );
        assert_eq!(
            bind_addr_from("[::1]:1431").unwrap(),
            "[::1]:1431".parse().unwrap()
        );
    }

    #[test]
    fn bind_rejects_a_non_loopback_address() {
        let error = bind_addr_from("0.0.0.0:1431").unwrap_err();
        assert!(error.contains("loopback"));
    }
}

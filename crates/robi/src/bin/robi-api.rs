use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use robi_core::config::LoopConfig;
use robi_core::event::NopSink;
use robi_core::tool::ToolRegistry;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa_swagger_ui::SwaggerUi;

use robi::{
    adapters::{
        chat_message::SqliteMessageStore,
        chat_runtime::{AgentFactory, SerializedChatRuntime},
        chat_session::repo::SqliteChatSessionRepository,
        sqlite,
    },
    domain::{
        chat_message::service::ChatMessageService, chat_session::service::ChatSessionService,
    },
    providers::{build_model, ApiKey, ModelId, ProviderSettings, ReasoningEffort},
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

    let settings = match settings_from_env() {
        Ok(settings) => settings,
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
    let tools = Arc::new(ToolRegistry::new());
    let model = match build_model(settings, Arc::clone(&tools)) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("failed to build model: {err}");
            std::process::exit(1);
        }
    };
    let store: Arc<dyn robi_core::store::MessageStore> =
        Arc::new(SqliteMessageStore::new(Arc::clone(&pool)));
    let chat_session_service = Arc::new(ChatSessionService {
        repository: Arc::new(SqliteChatSessionRepository::new(pool)),
    });
    let runtime = Arc::new(SerializedChatRuntime::new(AgentFactory {
        store: Arc::clone(&store),
        events: Arc::new(NopSink),
        model,
        tools,
        config: LoopConfig::default(),
    }));
    let state = AppState {
        chat_session_service: Arc::clone(&chat_session_service),
        chat_message_service: Arc::new(ChatMessageService {
            sessions: chat_session_service,
            runtime,
            store,
        }),
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

fn settings_from_env() -> Result<ProviderSettings, String> {
    let key = std::env::var("OPENCODE_GO_API_KEY").map_err(|_| {
        "set OPENCODE_GO_API_KEY to an OpenCode Go key (and optionally ROBI_MODEL, \
         ROBI_BASE_URL, ROBI_EFFORT)"
            .to_owned()
    })?;
    if key.trim().is_empty() {
        return Err("OPENCODE_GO_API_KEY is empty".into());
    }

    let model = std::env::var("ROBI_MODEL").unwrap_or_else(|_| "glm-5.3".to_owned());
    let mut settings =
        ProviderSettings::opencode_go(ApiKey::new(key), ModelId::new(model.as_str()));
    if let Ok(base_url) = std::env::var("ROBI_BASE_URL") {
        settings.base_url = base_url;
    }
    if let Ok(effort) = std::env::var("ROBI_EFFORT") {
        settings.reasoning_effort = Some(match effort.to_ascii_lowercase().as_str() {
            "low" => ReasoningEffort::Low,
            "medium" => ReasoningEffort::Medium,
            "high" => ReasoningEffort::High,
            other => return Err(format!("ROBI_EFFORT must be low, medium, or high: {other}")),
        });
    }
    Ok(settings)
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

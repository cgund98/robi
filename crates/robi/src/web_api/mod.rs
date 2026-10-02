//! Axum routes for the local API.
//!
//! Depends on `domain`. Does not depend on `adapters`.

use axum::{routing::get, Router};
use utoipa::OpenApi;

use crate::web_api::state::AppState;

pub mod chat_message;
pub mod chat_session;
pub mod error;
pub mod events;
pub mod settings;
pub mod state;
pub mod workspace;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_check))
        .with_state(state.clone())
        .merge(workspace::router(state.clone()))
        .merge(chat_session::router(state.clone()))
        .merge(chat_message::router(state.clone()))
        .merge(settings::router(state.clone()))
        .merge(events::router(state))
}

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    responses(
        (status = 200, description = "Health check", body = String)
    )
)]
async fn health_check() -> &'static str {
    "Ok"
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health_check,
        workspace::create_workspace,
        workspace::list_workspaces,
        workspace::get_workspace,
        workspace::delete_workspace,
        chat_session::create_chat_session,
        chat_session::list_chat_sessions,
        chat_session::get_chat_session,
        chat_session::update_chat_session,
        chat_session::delete_chat_session,
        chat_message::submit_instruction,
        chat_message::list_chat_messages,
        chat_message::get_chat_message,
        settings::get_setting,
        settings::set_setting,
        events::stream_events
    ),
    components(schemas(
        workspace::CreateWorkspace,
        workspace::Workspace,
        chat_session::CreateChatSession,
        chat_session::UpdateChatSession,
        chat_session::ChatSession,
        chat_message::SubmitInstruction,
        chat_message::AcceptedInstruction,
        chat_message::ChatMessage,
        chat_message::ChatToolCall,
        settings::SetSetting,
        settings::SettingResponse
    )),
    tags(
        (name = "robi", description = "Robi API")
    )
)]
struct ApiDoc;

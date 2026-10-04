//! Axum routes for the local API.
//!
//! Depends on `domain`. Does not depend on `adapters`.

use axum::{routing::get, Router};
use utoipa::OpenApi;

use crate::web_api::state::AppState;

pub mod chat_message;
pub mod chat_session;
pub mod code_index;
pub mod docs;
pub mod error;
pub mod events;
pub mod models;
pub mod review;
pub mod settings;
pub mod state;
pub mod workspace;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_check))
        .with_state(state.clone())
        .merge(workspace::router(state.clone()))
        .merge(code_index::router(state.clone()))
        .merge(docs::router(state.clone()))
        .merge(chat_session::router(state.clone()))
        .merge(review::router(state.clone()))
        .merge(chat_message::router(state.clone()))
        .merge(settings::router(state.clone()))
        .merge(models::router(state.clone()))
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
        workspace::list_skills,
        workspace::list_mcp_servers,
        workspace::focus_mcp,
        workspace::get_mcp_config,
        code_index::get_index,
        code_index::put_index,
        docs::list_docs,
        docs::get_doc,
        docs::search_docs,
        chat_session::create_chat_session,
        chat_session::list_chat_sessions,
        chat_session::get_chat_session,
        chat_session::update_chat_session,
        chat_session::delete_chat_session,
        review::get_session_review,
        review::decide_session_review,
        chat_message::submit_instruction,
        chat_message::list_chat_messages,
        chat_message::get_chat_message,
        chat_message::decide_tool_call,
        chat_message::get_tool_original,
        chat_message::stop_agent,
        settings::get_setting,
        settings::set_setting,
        settings::delete_setting,
        models::list_models,
        events::stream_events
    ),
    components(schemas(
        workspace::CreateWorkspace,
        workspace::Workspace,
        workspace::SkillEntry,
        workspace::McpServer,
        workspace::McpConfig,
        code_index::IndexStatusBody,
        code_index::IndexCommand,
        docs::DocsListing,
        docs::DocEntry,
        docs::DocContent,
        docs::DocSearchResult,
        docs::DocSearchHit,
        chat_session::CreateChatSession,
        chat_session::UpdateChatSession,
        chat_session::ChatSession,
        chat_session::ModelConfigBody,
        chat_session::ModeOverrideBody,
        chat_session::ModelConfigPatch,
        chat_session::ModeOverridePatch,
        chat_message::SubmitInstruction,
        chat_message::DecideToolCall,
        chat_message::AcceptedInstruction,
        chat_message::StoppedAgent,
        chat_message::ChatMessage,
        chat_message::ChatSkill,
        chat_message::ChatToolCall,
        chat_message::ToolOriginal,
        chat_message::ChatUsage,
        review::SessionReview,
        review::ReviewFileBody,
        review::ReviewStatus,
        review::ReviewLineBody,
        review::ReviewLineStatus,
        review::ReviewHunkBody,
        review::DecideReview,
        settings::SetSetting,
        settings::SettingResponse,
        models::CatalogModel
    )),
    tags(
        (name = "robi", description = "Robi API")
    )
)]
struct ApiDoc;

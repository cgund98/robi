//! Axum routes for the local API.
//!
//! Depends on `domain`. Does not depend on `adapters`.
//!
//! A route from a gated workstream is registered here **and** in the OpenAPI
//! document, both behind the same feature. See
//! `docs/src/design/core/feature-switches.md`.

use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    middleware::{from_fn, Next},
    response::Response,
    routing::get,
    Router,
};
use utoipa::OpenApi;

use crate::web_api::state::AppState;

pub mod chat_message;
pub mod chat_session;
#[cfg(feature = "semantic-index")]
pub mod code_index;
#[cfg(feature = "docs-viewer")]
pub mod docs;
pub mod error;
pub mod events;
pub mod models;
#[cfg(feature = "review")]
pub mod review;
pub mod settings;
pub mod state;
pub mod workspace;

pub fn router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/v1/health", get(health_check))
        .with_state(state.clone())
        .merge(workspace::router(state.clone()))
        .merge(chat_session::router(state.clone()))
        .merge(chat_message::router(state.clone()))
        .merge(settings::router(state.clone()))
        .merge(models::router(state.clone()))
        .merge(events::router(state.clone()));
    #[cfg(feature = "semantic-index")]
    let router = router.merge(code_index::router(state.clone()));
    #[cfg(feature = "docs-viewer")]
    let router = router.merge(docs::router(state.clone()));
    #[cfg(feature = "review")]
    let router = router.merge(review::router(state.clone()));
    router.layer(from_fn(log_requests))
}

const SLOW_REQUEST: Duration = Duration::from_secs(2);

/// A warning when the response is outside 2xx, and another when the handler
/// takes 2 seconds or longer. Timing ends when the handler returns the
/// response, including a stream that later fails in the body.
async fn log_requests(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    let elapsed = started.elapsed();
    if !status.is_success() {
        tracing::warn!(%method, %path, %status, "request failed");
    }
    if elapsed >= SLOW_REQUEST {
        tracing::warn!(
            %method,
            %path,
            %status,
            elapsed_ms = elapsed.as_millis(),
            "slow request"
        );
    }
    response
}

/// The whole document: the unconditional routes, plus each gated workstream's
/// own fragment. `OpenApi::merge` combines the paths and the components.
pub fn openapi() -> utoipa::openapi::OpenApi {
    #[allow(unused_mut)]
    let mut document = ApiDoc::openapi();
    #[cfg(feature = "semantic-index")]
    document.merge(IndexDoc::openapi());
    #[cfg(feature = "docs-viewer")]
    document.merge(DocsDoc::openapi());
    #[cfg(feature = "review")]
    document.merge(ReviewDoc::openapi());
    document
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
        workspace::resolve_workspace_path,
        workspace::open_workspace_file,
        workspace::read_workspace_file,
        chat_session::create_chat_session,
        chat_session::list_chat_sessions,
        chat_session::get_chat_session,
        chat_session::update_chat_session,
        chat_session::delete_chat_session,
        chat_message::submit_instruction,
        chat_message::list_chat_messages,
        chat_message::get_chat_message,
        chat_message::decide_tool_call,
        chat_message::get_tool_original,
        chat_message::stop_agent,
        chat_message::compact_session,
        settings::list_settings,
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
        workspace::AbsolutePath,
        workspace::OpenWorkspaceFile,
        workspace::WorkspaceFile,
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
        chat_message::CompactingAgent,
        chat_message::ChatMessage,
        chat_message::ChatSkill,
        chat_message::ChatToolCall,
        chat_message::ToolOriginal,
        chat_message::ChatUsage,
        settings::SetSetting,
        settings::SettingResponse,
        models::CatalogModel
    )),
    tags(
        (name = "robi", description = "Robi API")
    )
)]
struct ApiDoc;

/// S3: the index routes.
#[cfg(feature = "semantic-index")]
#[derive(OpenApi)]
#[openapi(
    paths(code_index::get_index, code_index::put_index),
    components(schemas(code_index::IndexStatusBody, code_index::IndexCommand))
)]
struct IndexDoc;

/// S5: the docs routes.
#[cfg(feature = "docs-viewer")]
#[derive(OpenApi)]
#[openapi(
    paths(docs::list_docs, docs::get_doc, docs::put_doc, docs::search_docs),
    components(schemas(
        docs::DocsListing,
        docs::DocEntry,
        docs::DocKind,
        docs::DocContent,
        docs::DocWriteRequest,
        docs::DocWriteResponse,
        docs::DocWriteConflict,
        docs::DocSearchResult,
        docs::DocSearchHit
    ))
)]
struct DocsDoc;

/// S2: the review routes.
#[cfg(feature = "review")]
#[derive(OpenApi)]
#[openapi(
    paths(
        review::get_session_review,
        review::get_review_file,
        review::decide_session_review
    ),
    components(schemas(
        review::SessionReview,
        review::ReviewFileSummary,
        review::ReviewFileBody,
        review::ReviewStatus,
        review::ReviewLineBody,
        review::ReviewLineStatus,
        review::ReviewHunkBody,
        review::DecideReview
    ))
)]
struct ReviewDoc;

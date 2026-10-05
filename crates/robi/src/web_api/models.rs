use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::agent::providers::{ModelCatalog, ProviderKind};
use crate::domain::settings::keys::{self, provider_enabled};
use crate::web_api::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/models", get(list_models))
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/models",
    responses(
        (status = 200, description = "Tool-capable models the provider can drive", body = Vec<CatalogModel>)
    )
)]
pub async fn list_models(State(state): State<AppState>) -> Json<Vec<CatalogModel>> {
    // A11: one flat list. A provider whose setting is `off` is left out, so the
    // picker only offers models a turn can actually drive.
    let catalog = ModelCatalog::all();
    let opencode = provider_on(&state, keys::PROVIDER_OPENCODE_GO).await;
    let anthropic = provider_on(&state, keys::PROVIDER_ANTHROPIC).await;
    let deepseek = provider_on(&state, keys::PROVIDER_DEEPSEEK).await;
    Json(
        catalog
            .tool_capable()
            .filter(|info| match ProviderKind::of(info.id.as_str()) {
                ProviderKind::OpenCodeGo => opencode,
                ProviderKind::Anthropic => anthropic,
                ProviderKind::DeepSeek => deepseek,
            })
            .map(|info| CatalogModel {
                id: info.id.as_str().to_owned(),
                display_name: info.display_name.clone(),
                context_window: info.context_window,
            })
            .collect(),
    )
}

/// A settings read that fails leaves the provider on, so a store error does not
/// empty the picker.
async fn provider_on(state: &AppState, key: &str) -> bool {
    match state.settings_service.get(key).await {
        Ok(setting) => provider_enabled(setting.value.as_deref()),
        Err(_) => true,
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    /// The model's advertised context window, in tokens.
    pub context_window: u64,
}

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

use crate::adapters::model_source::pickable_models;
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
    // A11: one flat list. The filter lives in `adapters::model_source` because the
    // terminal's picker asks the same question; a copy here would be a second
    // answer to "which models can a turn drive".
    Json(
        pickable_models(&state.settings_service)
            .await
            .into_iter()
            .map(|info| CatalogModel {
                id: info.id.as_str().to_owned(),
                display_name: info.display_name,
                context_window: info.context_window,
            })
            .collect(),
    )
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    /// The model's advertised context window, in tokens.
    pub context_window: u64,
}

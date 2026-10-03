use axum::{routing::get, Json, Router};
use serde::Serialize;

use crate::agent::providers::ModelCatalog;
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
pub async fn list_models() -> Json<Vec<CatalogModel>> {
    let catalog = ModelCatalog::opencode_go();
    Json(
        catalog
            .tool_capable()
            .map(|info| CatalogModel {
                id: info.id.as_str().to_owned(),
                display_name: info.display_name.clone(),
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

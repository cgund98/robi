use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

use crate::domain::{error::ServiceError, settings::service::ReadSetting};
use crate::web_api::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/settings/{key}", get(get_setting).put(set_setting))
        .with_state(state)
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/settings/{key}",
    params(("key" = String, Path, description = "Setting key")),
    responses(
        (status = 200, description = "Setting. An unset key with a default is stored, then returned. An unset key with no default has a null value. Secret values are omitted.", body = SettingResponse)
    )
)]
pub async fn get_setting(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<SettingResponse>, ServiceError> {
    let setting = state.settings_service.get(&key).await?;
    Ok(Json(SettingResponse::from_read(&key, setting)))
}

#[axum::debug_handler]
#[utoipa::path(
    put,
    path = "/api/v1/settings/{key}",
    request_body = SetSetting,
    params(("key" = String, Path, description = "Setting key")),
    responses(
        (status = 204, description = "Setting stored")
    )
)]
pub async fn set_setting(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(payload): Json<SetSetting>,
) -> Result<StatusCode, ServiceError> {
    state
        .settings_service
        .set(&key, payload.value, payload.secret)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetSetting {
    pub value: String,
    /// When true, the value is written to the secrets file and later reads
    /// over HTTP omit it.
    pub secret: bool,
}

#[derive(utoipa::ToSchema)]
pub struct SettingResponse {
    pub key: String,
    pub secret: bool,
    /// `null` when the key is unset and has no default. Omitted when the stored value is a secret.
    pub value: Option<String>,
    /// Stored secrets leave `value` out of the JSON. Unset keys send `null`.
    #[schema(ignore)]
    pub omit_value: bool,
}

impl SettingResponse {
    fn from_read(key: &str, setting: ReadSetting) -> Self {
        let omit_value = setting.secret && setting.value.is_some();
        Self {
            key: key.to_owned(),
            secret: setting.secret,
            value: if omit_value { None } else { setting.value },
            omit_value,
        }
    }
}

impl Serialize for SettingResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state =
            serializer.serialize_struct("SettingResponse", if self.omit_value { 2 } else { 3 })?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("secret", &self.secret)?;
        if !self.omit_value {
            state.serialize_field("value", &self.value)?;
        }
        state.end()
    }
}

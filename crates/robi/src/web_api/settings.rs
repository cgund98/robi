use axum::{
    extract::{Path, RawQuery, State},
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
        .route("/api/v1/settings", get(list_settings))
        .route(
            "/api/v1/settings/{key}",
            get(get_setting).put(set_setting).delete(delete_setting),
        )
        .with_state(state)
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListSettingsQuery {
    /// Repeat to read several keys in one request.
    pub key: Vec<String>,
}

#[axum::debug_handler]
#[utoipa::path(
    get,
    path = "/api/v1/settings",
    params(ListSettingsQuery),
    responses(
        (status = 200, description = "Settings in request order. An unset key with a default is stored, then returned. An unset key with no default has a null value. Secret values are omitted.", body = Vec<SettingResponse>)
    )
)]
pub async fn list_settings(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Json<Vec<SettingResponse>>, ServiceError> {
    // Axum's `Query` extractor rejects a repeated key: it keeps one string and
    // then fails to deserialize `Vec`.
    let keys = setting_keys(raw.as_deref());
    let settings = state.settings_service.get_many(&keys).await?;
    let body = keys
        .iter()
        .zip(settings)
        .map(|(key, setting)| SettingResponse::from_read(key, setting))
        .collect();
    Ok(Json(body))
}

fn setting_keys(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    url::form_urlencoded::parse(raw.as_bytes())
        .filter(|(name, value)| name == "key" && !value.is_empty())
        .map(|(_, value)| value.into_owned())
        .collect()
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

#[axum::debug_handler]
#[utoipa::path(
    delete,
    path = "/api/v1/settings/{key}",
    params(("key" = String, Path, description = "Setting key")),
    responses(
        (status = 204, description = "Setting removed. The next read inherits.")
    )
)]
pub async fn delete_setting(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<StatusCode, ServiceError> {
    state.settings_service.remove(&key).await?;
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

#[cfg(test)]
mod tests {
    use super::setting_keys;

    #[test]
    fn repeated_key_query_keeps_every_value() {
        let keys = setting_keys(Some("key=base_url&key=model&other=1"));
        assert_eq!(keys, ["base_url", "model"]);
    }
}

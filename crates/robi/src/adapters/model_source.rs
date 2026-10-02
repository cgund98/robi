//! Builds the model a session actor will drive.
//!
//! [`SettingsModelSource`] reads the settings store on every call, so a value
//! written while the process is running is what the next actor uses. An actor
//! that has already started keeps the model it was given.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::model::Model;
use robi_core::tool::ToolRegistry;

use crate::domain::{
    chat_session::model::ModelConfig,
    error::ServiceError,
    settings::{
        keys::{self, DEFAULT_MODEL},
        store::SettingsStore,
    },
};
use crate::providers::{build_model, ApiKey, ModelId, ProviderSettings, ReasoningEffort};

/// The model a new session actor should hold.
#[async_trait]
pub trait ModelSource: Send + Sync {
    /// Build the model for one actor.
    ///
    /// `tools` is the registry that actor will execute, so the provider is
    /// offered the same tools.
    async fn model(
        &self,
        tools: Arc<ToolRegistry>,
        workspace: Option<std::path::PathBuf>,
        choice: ModelConfig,
    ) -> Result<Arc<dyn Model>, ServiceError>;
}

/// Returns one model. Runtime tests use this so they do not need settings files.
pub struct FixedModelSource {
    model: Arc<dyn Model>,
}

impl FixedModelSource {
    pub fn new(model: Arc<dyn Model>) -> Self {
        Self { model }
    }
}

#[async_trait]
impl ModelSource for FixedModelSource {
    async fn model(
        &self,
        _tools: Arc<ToolRegistry>,
        _workspace: Option<std::path::PathBuf>,
        _choice: ModelConfig,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        Ok(Arc::clone(&self.model))
    }
}

/// Resolves provider settings from the store, then builds a model.
pub struct SettingsModelSource {
    settings: Arc<dyn SettingsStore>,
}

impl SettingsModelSource {
    pub fn new(settings: Arc<dyn SettingsStore>) -> Self {
        Self { settings }
    }

    async fn provider_settings(
        &self,
        choice: &ModelConfig,
    ) -> Result<ProviderSettings, ServiceError> {
        let api_key = match self.settings.get(keys::OPENCODE_GO_API_KEY).await? {
            Some(setting) if !setting.value.trim().is_empty() => setting.value,
            _ => {
                return Err(ServiceError::BadRequest(
                    "opencode_go_api_key is not set".into(),
                ));
            }
        };
        let model = match choice.model.as_deref().filter(|model| !model.is_empty()) {
            Some(model) => model.to_owned(),
            None => match self.settings.get(keys::MODEL).await? {
                Some(setting) if !setting.value.is_empty() => setting.value,
                _ => DEFAULT_MODEL.to_owned(),
            },
        };
        let mut settings =
            ProviderSettings::opencode_go(ApiKey::new(api_key), ModelId::new(model.as_str()));
        if let Some(base_url) = self.settings.get(keys::BASE_URL).await? {
            if !base_url.value.is_empty() {
                settings.base_url = base_url.value;
            }
        }
        settings.reasoning_effort = match choice
            .reasoning_effort
            .as_deref()
            .filter(|effort| !effort.is_empty())
        {
            Some(effort) => Some(parse_effort(effort)?),
            None => match self.settings.get(keys::REASONING_EFFORT).await? {
                Some(effort) if !effort.value.is_empty() => Some(parse_effort(&effort.value)?),
                _ => None,
            },
        };
        Ok(settings)
    }
}

#[async_trait]
impl ModelSource for SettingsModelSource {
    async fn model(
        &self,
        tools: Arc<ToolRegistry>,
        workspace: Option<std::path::PathBuf>,
        choice: ModelConfig,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        let mut settings = self.provider_settings(&choice).await?;
        let user_prompt = match self.settings.get(keys::SYSTEM_PROMPT).await? {
            Some(setting) if !setting.value.trim().is_empty() => Some(setting.value),
            _ => None,
        };
        settings.system_prompt = crate::prompt::assemble_session(crate::prompt::SessionPrompt {
            tools: &tools,
            user_prompt,
            config_dir: crate::adapters::settings::home_dir().ok(),
            workspace,
            max_bytes: crate::prompt::DEFAULT_MAX_BYTES,
        });
        build_model(settings, tools)
            .map_err(|error| ServiceError::BadRequest(format!("failed to build model: {error}")))
    }
}

fn parse_effort(value: &str) -> Result<ReasoningEffort, ServiceError> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Ok(ReasoningEffort::Low),
        "medium" => Ok(ReasoningEffort::Medium),
        "high" => Ok(ReasoningEffort::High),
        other => Err(ServiceError::BadRequest(format!(
            "reasoning_effort must be low, medium, or high: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::settings::{
        keys::{self, OPENCODE_GO_API_KEY},
        memory::MemorySettingsStore,
    };

    #[tokio::test]
    async fn a_later_build_uses_the_key_written_between_calls() {
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(OPENCODE_GO_API_KEY, "sk-one".into(), true)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT, "low".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store.clone());
        let tools = Arc::new(ToolRegistry::new());

        let first = source
            .model(Arc::clone(&tools), None, ModelConfig::default())
            .await
            .unwrap();
        let first_settings = source
            .provider_settings(&ModelConfig::default())
            .await
            .unwrap();
        assert_eq!(first_settings.api_key.expose(), "sk-one");
        assert_eq!(first_settings.model.as_str(), keys::DEFAULT_MODEL);
        assert_eq!(first_settings.reasoning_effort, Some(ReasoningEffort::Low));

        store
            .set(OPENCODE_GO_API_KEY, "sk-two".into(), true)
            .await
            .unwrap();
        store
            .set(keys::MODEL, "glm-5.2".into(), false)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT, "high".into(), false)
            .await
            .unwrap();

        let second = source
            .model(tools, None, ModelConfig::default())
            .await
            .unwrap();
        let second_settings = source
            .provider_settings(&ModelConfig::default())
            .await
            .unwrap();
        assert_eq!(second_settings.api_key.expose(), "sk-two");
        assert_eq!(second_settings.model.as_str(), "glm-5.2");
        assert_eq!(
            second_settings.reasoning_effort,
            Some(ReasoningEffort::High)
        );
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn a_missing_key_refuses_to_build() {
        let source = SettingsModelSource::new(Arc::new(MemorySettingsStore::new()));
        let error = match source
            .model(Arc::new(ToolRegistry::new()), None, ModelConfig::default())
            .await
        {
            Ok(_) => panic!("a missing key must not build a model"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ServiceError::BadRequest("opencode_go_api_key is not set".into())
        );
    }

    #[tokio::test]
    async fn a_session_override_beats_the_setting_and_a_missing_key_inherits() {
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(OPENCODE_GO_API_KEY, "sk-one".into(), true)
            .await
            .unwrap();
        store
            .set(keys::MODEL, "glm-5.3".into(), false)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT, "low".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store);
        let tools = Arc::new(ToolRegistry::new());

        let effort_only = source
            .provider_settings(&ModelConfig {
                model: None,
                reasoning_effort: Some("high".into()),
            })
            .await
            .unwrap();
        assert_eq!(effort_only.model.as_str(), "glm-5.3");
        assert_eq!(effort_only.reasoning_effort, Some(ReasoningEffort::High));

        let both = source
            .provider_settings(&ModelConfig {
                model: Some("glm-5.2".into()),
                reasoning_effort: Some("medium".into()),
            })
            .await
            .unwrap();
        assert_eq!(both.model.as_str(), "glm-5.2");
        assert_eq!(both.reasoning_effort, Some(ReasoningEffort::Medium));

        source
            .model(
                tools,
                None,
                ModelConfig {
                    model: Some("glm-5.2".into()),
                    reasoning_effort: None,
                },
            )
            .await
            .unwrap();
    }
}

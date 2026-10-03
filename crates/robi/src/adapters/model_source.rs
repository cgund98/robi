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
    chat_session::model::{AgentMode, ModeOverride},
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
        mode: AgentMode,
        choice: ModeOverride,
    ) -> Result<Arc<dyn Model>, ServiceError>;

    /// Build a model that offers `tools` and uses `system_prompt` as written.
    ///
    /// The default ignores the prompt and calls [`ModelSource::model`]. A source
    /// that bakes the session prompt into the model overrides this.
    async fn model_with_prompt(
        &self,
        tools: Arc<ToolRegistry>,
        mode: AgentMode,
        choice: ModeOverride,
        system_prompt: String,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        let _ = system_prompt;
        self.model(tools, None, mode, choice).await
    }
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
        _mode: AgentMode,
        _choice: ModeOverride,
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
        mode: AgentMode,
        choice: &ModeOverride,
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
            None => match self.stored(keys::model_key(mode)).await? {
                Some(model) => model,
                None => match self.stored(keys::MODEL).await? {
                    Some(model) => model,
                    None => DEFAULT_MODEL.to_owned(),
                },
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
            None => match self.stored(keys::effort_key(mode)).await? {
                Some(effort) => Some(parse_effort(&effort)?),
                None => match self.stored(keys::REASONING_EFFORT).await? {
                    Some(effort) => Some(parse_effort(&effort)?),
                    None => None,
                },
            },
        };
        Ok(settings)
    }

    async fn stored(&self, key: &str) -> Result<Option<String>, ServiceError> {
        match self.settings.get(key).await? {
            Some(setting) if !setting.value.is_empty() => Ok(Some(setting.value)),
            _ => Ok(None),
        }
    }
}

#[async_trait]
impl ModelSource for SettingsModelSource {
    async fn model(
        &self,
        tools: Arc<ToolRegistry>,
        workspace: Option<std::path::PathBuf>,
        mode: AgentMode,
        choice: ModeOverride,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        let mut settings = self.provider_settings(mode, &choice).await?;
        let user_prompt = match self.settings.get(keys::SYSTEM_PROMPT).await? {
            Some(setting) if !setting.value.trim().is_empty() => Some(setting.value),
            _ => None,
        };
        settings.system_prompt = crate::prompt::assemble_session(crate::prompt::SessionPrompt {
            tools: &tools,
            user_prompt,
            config_dir: crate::adapters::settings::home_dir().ok(),
            workspace,
            mode,
            max_bytes: crate::prompt::DEFAULT_MAX_BYTES,
        });
        build_model(settings, tools)
            .map_err(|error| ServiceError::BadRequest(format!("failed to build model: {error}")))
    }

    async fn model_with_prompt(
        &self,
        tools: Arc<ToolRegistry>,
        mode: AgentMode,
        choice: ModeOverride,
        system_prompt: String,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        let mut settings = self.provider_settings(mode, &choice).await?;
        settings.system_prompt = system_prompt;
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
            .model(
                Arc::clone(&tools),
                None,
                AgentMode::Agent,
                ModeOverride::default(),
            )
            .await
            .unwrap();
        let first_settings = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
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
            .model(tools, None, AgentMode::Agent, ModeOverride::default())
            .await
            .unwrap();
        let second_settings = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
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
            .model(
                Arc::new(ToolRegistry::new()),
                None,
                AgentMode::Agent,
                ModeOverride::default(),
            )
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
            .provider_settings(
                AgentMode::Ask,
                &ModeOverride {
                    model: None,
                    reasoning_effort: Some("high".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(effort_only.model.as_str(), "glm-5.3");
        assert_eq!(effort_only.reasoning_effort, Some(ReasoningEffort::High));

        let both = source
            .provider_settings(
                AgentMode::Agent,
                &ModeOverride {
                    model: Some("glm-5.2".into()),
                    reasoning_effort: Some("medium".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(both.model.as_str(), "glm-5.2");
        assert_eq!(both.reasoning_effort, Some(ReasoningEffort::Medium));

        source
            .model(
                tools,
                None,
                AgentMode::Agent,
                ModeOverride {
                    model: Some("glm-5.2".into()),
                    reasoning_effort: None,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_mode_setting_beats_the_fallback_and_a_session_override_beats_both() {
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
        store
            .set(keys::MODEL_PLAN, "glm-5.2".into(), false)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT_PLAN, "medium".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store);

        let from_mode = source
            .provider_settings(AgentMode::Plan, &ModeOverride::default())
            .await
            .unwrap();
        assert_eq!(from_mode.model.as_str(), "glm-5.2");
        assert_eq!(from_mode.reasoning_effort, Some(ReasoningEffort::Medium));

        let from_ask = source
            .provider_settings(AgentMode::Ask, &ModeOverride::default())
            .await
            .unwrap();
        assert_eq!(from_ask.model.as_str(), "glm-5.3");
        assert_eq!(from_ask.reasoning_effort, Some(ReasoningEffort::Low));

        let overridden = source
            .provider_settings(
                AgentMode::Plan,
                &ModeOverride {
                    model: Some("glm-5.1".into()),
                    reasoning_effort: Some("high".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(overridden.model.as_str(), "glm-5.1");
        assert_eq!(overridden.reasoning_effort, Some(ReasoningEffort::High));
    }
}

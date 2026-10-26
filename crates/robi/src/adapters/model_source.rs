//! Builds the model a session actor will drive.
//!
//! [`SettingsModelSource`] reads the settings store on every call, so a value
//! written while the process is running is what the next actor uses. An actor
//! that has already started keeps the model it was given.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::model::Model;
use robi_core::tool::ToolRegistry;

use crate::agent::providers::{
    build_model, ApiKey, ImageSource, ModelCatalog, ModelId, ModelInfo, ProviderKind,
    ProviderSettings, ReasoningEffort,
};
use crate::domain::{
    chat_session::model::{AgentMode, ModeOverride},
    error::ServiceError,
    settings::{
        keys::{self, provider_enabled, DEFAULT_MODEL},
        service::{ReadSetting, SettingsService},
        store::SettingsStore,
    },
};

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
    /// Resolves a user message's attachment ids to bytes for the model.
    images: Arc<dyn ImageSource>,
}

impl SettingsModelSource {
    pub fn new(settings: Arc<dyn SettingsStore>, images: Arc<dyn ImageSource>) -> Self {
        Self { settings, images }
    }

    fn images(&self) -> Arc<dyn ImageSource> {
        Arc::clone(&self.images)
    }

    async fn provider_settings(
        &self,
        mode: AgentMode,
        choice: &ModeOverride,
    ) -> Result<ProviderSettings, ServiceError> {
        // Every layer of the model and effort resolution is one function now, so
        // the terminal's picker and this builder cannot disagree about what a
        // mode will actually use.
        let settings_service = SettingsService {
            store: Arc::clone(&self.settings),
        };
        let effective = effective_choice(&settings_service, mode, choice).await?;
        let model = effective.model;

        // A11: the prefix names the provider. An id with neither known prefix is a
        // legacy OpenCode Go id, read-compatible forever; new writes are prefixed.
        let kind = ProviderKind::of(&model);
        let provider_key = match kind {
            ProviderKind::OpenCodeGo => keys::PROVIDER_OPENCODE_GO,
            ProviderKind::Anthropic => keys::PROVIDER_ANTHROPIC,
            ProviderKind::DeepSeek => keys::PROVIDER_DEEPSEEK,
        };
        let provider_value = self.settings.get(provider_key).await?;
        if !keys::provider_enabled(
            provider_value
                .as_ref()
                .map(|setting| setting.value.as_str()),
        ) {
            return Err(ServiceError::BadRequest(format!("{provider_key} is off")));
        }
        let (key_name, key_value) = match kind {
            ProviderKind::OpenCodeGo => (keys::OPENCODE_GO_API_KEY, keys::OPENCODE_GO_API_KEY),
            ProviderKind::Anthropic => (keys::ANTHROPIC_API_KEY, keys::ANTHROPIC_API_KEY),
            ProviderKind::DeepSeek => (keys::DEEPSEEK_API_KEY, keys::DEEPSEEK_API_KEY),
        };
        let api_key = match self.settings.get(key_value).await? {
            Some(setting) if !setting.value.trim().is_empty() => setting.value,
            _ => {
                return Err(ServiceError::BadRequest(format!("{key_name} is not set")));
            }
        };

        let mut settings = match kind {
            ProviderKind::OpenCodeGo => {
                ProviderSettings::opencode_go(ApiKey::new(api_key), ModelId::new(model.as_str()))
            }
            ProviderKind::Anthropic => {
                let mut settings =
                    ProviderSettings::anthropic(ApiKey::new(api_key), ModelId::new(model.as_str()));
                // A5: the Messages API requires max_tokens. Take it from the catalog.
                if let Some(info) = ModelCatalog::anthropic().get(&settings.model) {
                    settings.max_tokens = Some(info.max_output);
                }
                settings
            }
            ProviderKind::DeepSeek => {
                ProviderSettings::deepseek(ApiKey::new(api_key), ModelId::new(model.as_str()))
            }
        };
        let base_key = match kind {
            ProviderKind::DeepSeek => keys::DEEPSEEK_BASE_URL,
            ProviderKind::OpenCodeGo | ProviderKind::Anthropic => keys::BASE_URL,
        };
        if let Some(base_url) = self.settings.get(base_key).await? {
            if !base_url.value.is_empty() {
                settings.base_url = base_url.value;
            }
        }
        settings.reasoning_effort = match effective.effort.as_deref() {
            Some(effort) => Some(parse_effort(effort)?),
            None => None,
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
        mode: AgentMode,
        choice: ModeOverride,
    ) -> Result<Arc<dyn Model>, ServiceError> {
        let mut settings = self.provider_settings(mode, &choice).await?;
        let user_prompt = match self.settings.get(keys::SYSTEM_PROMPT).await? {
            Some(setting) if !setting.value.trim().is_empty() => Some(setting.value),
            _ => None,
        };
        // Resolved before the blocking call: the prompt needs to know whether the
        // workspace will be mounted at `/workspace` or stay at its host path.
        let container_sandbox =
            crate::agent::sandbox::container::config::mechanism(Some(self.settings.as_ref())).await
                == crate::agent::sandbox::Mechanism::Container;
        let tools_for_prompt = Arc::clone(&tools);
        settings.system_prompt = crate::agent::blocking::call(move || {
            crate::agent::prompt::assemble_session(crate::agent::prompt::SessionPrompt {
                tools: &tools_for_prompt,
                user_prompt,
                config_dir: crate::adapters::settings::home_dir().ok(),
                workspace,
                mode,
                max_bytes: crate::agent::prompt::DEFAULT_MAX_BYTES,
                container_sandbox,
            })
        })
        .await
        .unwrap_or_default();
        build_model(settings, tools, self.images())
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
        build_model(settings, tools, self.images())
            .map_err(|error| ServiceError::BadRequest(format!("failed to build model: {error}")))
    }
}

/// Models a turn can actually drive: tool-capable, and offered by a provider
/// whose setting is `on`.
///
/// The one implementation behind `/api/v1/models` and the terminal's picker.
/// A provider whose setting is `off` is left out, so neither list offers a model
/// a turn would fail to build (A11: one flat list, disjoint prefixed ids).
pub async fn pickable_models(settings: &SettingsService) -> Vec<ModelInfo> {
    let opencode = provider_on(settings, keys::PROVIDER_OPENCODE_GO).await;
    let anthropic = provider_on(settings, keys::PROVIDER_ANTHROPIC).await;
    let deepseek = provider_on(settings, keys::PROVIDER_DEEPSEEK).await;
    ModelCatalog::all()
        .tool_capable()
        .filter(|info| match ProviderKind::of(info.id.as_str()) {
            ProviderKind::OpenCodeGo => opencode,
            ProviderKind::Anthropic => anthropic,
            ProviderKind::DeepSeek => deepseek,
        })
        .cloned()
        .collect()
}

/// A settings read that fails leaves the provider on, so a store error does not
/// empty the picker.
async fn provider_on(settings: &SettingsService, key: &str) -> bool {
    match settings.get(key).await {
        Ok(setting) => provider_enabled(setting.value.as_deref()),
        Err(_) => true,
    }
}

/// The model and effort one mode will use, after every layer has had its say.
///
/// `model` is always present: the last fallback is [`DEFAULT_MODEL`]. `effort` is
/// absent when nothing anywhere names one, which is what leaves the provider's own
/// default alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveChoice {
    pub model: String,
    pub effort: Option<String>,
}

impl EffectiveChoice {
    /// The provider the model id names. A bare legacy id reads as OpenCode Go.
    pub fn provider(&self) -> ProviderKind {
        ProviderKind::of(&self.model)
    }
}

/// Resolve one mode's model and effort: the session's override, then that mode's
/// setting, then the fallback setting, then the default model.
///
/// An empty string counts as absent at every layer, because that is how the
/// settings screen clears a mode's override.
pub async fn effective_choice(
    settings: &SettingsService,
    mode: AgentMode,
    choice: &ModeOverride,
) -> Result<EffectiveChoice, ServiceError> {
    let model = match non_empty(choice.model.as_deref()) {
        Some(model) => model.to_owned(),
        None => match stored(settings, keys::model_key(mode)).await? {
            Some(model) => model,
            None => match stored(settings, keys::MODEL).await? {
                Some(model) => model,
                None => DEFAULT_MODEL.to_owned(),
            },
        },
    };
    let effort = match non_empty(choice.reasoning_effort.as_deref()) {
        Some(effort) => Some(effort.to_owned()),
        None => match stored(settings, keys::effort_key(mode)).await? {
            Some(effort) => Some(effort),
            None => stored(settings, keys::REASONING_EFFORT).await?,
        },
    };
    Ok(EffectiveChoice { model, effort })
}

/// Each mode's inheritable choice: what a session with no override of its own
/// would use.
///
/// The picker needs all three at once, so a `Use default` row can name the value
/// it is falling back to, and so switching mode does not require a second read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeDefaults {
    pub ask: EffectiveChoice,
    pub plan: EffectiveChoice,
    pub agent: EffectiveChoice,
}

impl ModeDefaults {
    pub async fn load(settings: &SettingsService) -> Result<Self, ServiceError> {
        let inherited = ModeOverride::default();
        Ok(Self {
            ask: effective_choice(settings, AgentMode::Ask, &inherited).await?,
            plan: effective_choice(settings, AgentMode::Plan, &inherited).await?,
            agent: effective_choice(settings, AgentMode::Agent, &inherited).await?,
        })
    }

    pub fn for_mode(&self, mode: AgentMode) -> &EffectiveChoice {
        match mode {
            AgentMode::Ask => &self.ask,
            AgentMode::Plan => &self.plan,
            AgentMode::Agent => &self.agent,
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

async fn stored(settings: &SettingsService, key: &str) -> Result<Option<String>, ServiceError> {
    let ReadSetting { value, .. } = settings.get(key).await?;
    Ok(non_empty(value.as_deref()).map(str::to_owned))
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
    use crate::agent::providers::ProviderError;
    use crate::domain::settings::{
        keys::{self, OPENCODE_GO_API_KEY},
        memory::MemorySettingsStore,
    };
    use robi_core::ids::SessionId;

    /// No attachment is ever resolved in these tests; build_model only touches it
    /// when a transcript carries images, which none of these do.
    struct NoImages;

    #[async_trait]
    impl ImageSource for NoImages {
        async fn image(
            &self,
            _session: SessionId,
            _id: &str,
        ) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
            Ok(None)
        }
    }

    fn no_images() -> Arc<dyn ImageSource> {
        Arc::new(NoImages)
    }

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
        let source = SettingsModelSource::new(store.clone(), no_images());
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
        let source = SettingsModelSource::new(Arc::new(MemorySettingsStore::new()), no_images());
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
        let source = SettingsModelSource::new(store, no_images());
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
        let source = SettingsModelSource::new(store, no_images());

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

    #[tokio::test]
    async fn an_ant_prefix_resolves_anthropic_and_requires_the_anthropic_key() {
        // A11: the prefix names the provider and the key follows it.
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(keys::MODEL, "ant_claude-sonnet-5-5".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store.clone(), no_images());

        // No Anthropic key yet: refuse, naming the Anthropic key.
        let error = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("anthropic_api_key is not set".into())
        );

        store
            .set(keys::ANTHROPIC_API_KEY, "sk-ant".into(), true)
            .await
            .unwrap();
        let settings = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap();
        assert_eq!(settings.id.as_str(), "anthropic");
        assert_eq!(settings.api_key.expose(), "sk-ant");
        // A5: max_tokens comes from the catalog for the Anthropic provider.
        assert_eq!(settings.max_tokens, Some(128_000));
    }

    #[tokio::test]
    async fn a_dsk_prefix_resolves_deepseek_and_uses_its_own_base_url() {
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(keys::MODEL, "dsk_deepseek-flash".into(), false)
            .await
            .unwrap();
        store
            .set(keys::BASE_URL, "https://opencode.example/v1".into(), false)
            .await
            .unwrap();
        store
            .set(
                keys::DEEPSEEK_BASE_URL,
                "https://deepseek.example".into(),
                false,
            )
            .await
            .unwrap();
        let source = SettingsModelSource::new(store.clone(), no_images());

        let error = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("deepseek_api_key is not set".into())
        );

        store
            .set(keys::DEEPSEEK_API_KEY, "sk-dsk".into(), true)
            .await
            .unwrap();
        let settings = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap();
        assert_eq!(settings.id.as_str(), "deepseek");
        assert_eq!(settings.api_key.expose(), "sk-dsk");
        assert_eq!(settings.base_url, "https://deepseek.example");
    }

    #[tokio::test]
    async fn a_legacy_bare_id_still_resolves_opencode_go() {
        // A11: a settings row written before the prefix existed holds a bare id.
        // It reads as OpenCode Go and keeps working.
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(keys::OPENCODE_GO_API_KEY, "sk-one".into(), true)
            .await
            .unwrap();
        store
            .set(keys::MODEL, "glm-5.3".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store, no_images());

        let settings = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap();
        assert_eq!(settings.id.as_str(), "opencode-go");
        // build_model migrates the bare id to the prefixed form.
        source
            .model(
                Arc::new(ToolRegistry::new()),
                None,
                AgentMode::Agent,
                ModeOverride::default(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_unknown_model_id_fails_to_build() {
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(keys::OPENCODE_GO_API_KEY, "sk-one".into(), true)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store, no_images());
        let error = match source
            .model(
                Arc::new(ToolRegistry::new()),
                None,
                AgentMode::Agent,
                ModeOverride {
                    model: Some("ocg_not-a-model".into()),
                    reasoning_effort: None,
                },
            )
            .await
        {
            Ok(_) => panic!("an unknown model must not build a model"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("not-a-model"),
            "the error names the model: {error}"
        );
    }

    #[tokio::test]
    async fn a_disabled_provider_is_refused() {
        let store = Arc::new(MemorySettingsStore::new());
        store
            .set(keys::OPENCODE_GO_API_KEY, "sk-one".into(), true)
            .await
            .unwrap();
        store
            .set(keys::PROVIDER_OPENCODE_GO, "off".into(), false)
            .await
            .unwrap();
        let source = SettingsModelSource::new(store, no_images());
        let error = source
            .provider_settings(AgentMode::Agent, &ModeOverride::default())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ServiceError::BadRequest("provider_opencode_go is off".into())
        );
    }

    fn service(store: Arc<dyn SettingsStore>) -> SettingsService {
        SettingsService { store }
    }

    #[tokio::test]
    async fn the_pickable_list_leaves_out_a_provider_that_is_off() {
        let store = Arc::new(MemorySettingsStore::new());
        let settings = service(store.clone() as Arc<dyn SettingsStore>);
        let all = pickable_models(&settings).await;
        assert!(
            all.iter()
                .any(|info| ProviderKind::of(info.id.as_str()) == ProviderKind::Anthropic),
            "every provider is on until one is turned off"
        );

        store
            .set(keys::PROVIDER_ANTHROPIC, "off".into(), false)
            .await
            .unwrap();
        let filtered = pickable_models(&settings).await;
        assert_eq!(filtered.len(), all.len() - filtered_anthropic(&all));
        assert!(
            filtered
                .iter()
                .all(|info| ProviderKind::of(info.id.as_str()) != ProviderKind::Anthropic),
            "an off provider is not offered"
        );
        assert!(
            filtered.iter().all(|info| info.supports_tools),
            "the list is tool-capable models only"
        );
    }

    fn filtered_anthropic(all: &[ModelInfo]) -> usize {
        all.iter()
            .filter(|info| ProviderKind::of(info.id.as_str()) == ProviderKind::Anthropic)
            .count()
    }

    #[tokio::test]
    async fn a_settings_read_that_fails_leaves_every_provider_on() {
        let settings = service(Arc::new(FailingStore));
        assert!(!pickable_models(&settings).await.is_empty());
    }

    /// Every read fails, which is the store a picker must survive.
    struct FailingStore;

    #[async_trait]
    impl SettingsStore for FailingStore {
        async fn get(
            &self,
            _key: &str,
        ) -> Result<Option<crate::domain::settings::store::Setting>, ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn set(&self, _key: &str, _value: String, _secret: bool) -> Result<(), ServiceError> {
            Err(ServiceError::Unknown)
        }

        async fn remove(&self, _key: &str) -> Result<(), ServiceError> {
            Err(ServiceError::Unknown)
        }
    }

    #[tokio::test]
    async fn the_choice_climbs_override_then_mode_then_fallback_then_default() {
        let store = Arc::new(MemorySettingsStore::new());
        let settings = service(store.clone() as Arc<dyn SettingsStore>);
        let none = ModeOverride::default();

        let bare = effective_choice(&settings, AgentMode::Agent, &none)
            .await
            .unwrap();
        assert_eq!(bare.model, keys::DEFAULT_MODEL);
        assert_eq!(bare.effort, None);

        store
            .set(keys::MODEL, "glm-5.2".into(), false)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT, "low".into(), false)
            .await
            .unwrap();
        let fallback = effective_choice(&settings, AgentMode::Agent, &none)
            .await
            .unwrap();
        assert_eq!(fallback.model, "glm-5.2");
        assert_eq!(fallback.effort.as_deref(), Some("low"));

        store
            .set(keys::MODEL_AGENT, "ant_claude".into(), false)
            .await
            .unwrap();
        store
            .set(keys::REASONING_EFFORT_AGENT, "high".into(), false)
            .await
            .unwrap();
        let mode = effective_choice(&settings, AgentMode::Agent, &none)
            .await
            .unwrap();
        assert_eq!(mode.model, "ant_claude");
        assert_eq!(mode.effort.as_deref(), Some("high"));
        // Another mode still inherits the fallback: the keys are per mode.
        let ask = effective_choice(&settings, AgentMode::Ask, &none)
            .await
            .unwrap();
        assert_eq!(ask.model, "glm-5.2");

        let override_ = ModeOverride {
            model: Some("dsk_flash".into()),
            reasoning_effort: Some("medium".into()),
        };
        let chosen = effective_choice(&settings, AgentMode::Agent, &override_)
            .await
            .unwrap();
        assert_eq!(chosen.model, "dsk_flash");
        assert_eq!(chosen.effort.as_deref(), Some("medium"));

        // An empty string is absent, which is how a cleared override reads.
        let cleared = ModeOverride {
            model: Some(String::new()),
            reasoning_effort: Some(String::new()),
        };
        let inherited = effective_choice(&settings, AgentMode::Agent, &cleared)
            .await
            .unwrap();
        assert_eq!(inherited.model, "ant_claude");
        assert_eq!(inherited.effort.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn mode_defaults_read_each_mode_separately() {
        let store = Arc::new(MemorySettingsStore::new());
        let settings = service(store.clone() as Arc<dyn SettingsStore>);
        store
            .set(keys::MODEL, "glm-5.3".into(), false)
            .await
            .unwrap();
        store
            .set(keys::MODEL_PLAN, "ant_claude".into(), false)
            .await
            .unwrap();

        let defaults = ModeDefaults::load(&settings).await.unwrap();
        assert_eq!(defaults.for_mode(AgentMode::Agent).model, "glm-5.3");
        assert_eq!(defaults.for_mode(AgentMode::Ask).model, "glm-5.3");
        assert_eq!(defaults.for_mode(AgentMode::Plan).model, "ant_claude");
        assert_eq!(
            defaults.for_mode(AgentMode::Plan).provider(),
            ProviderKind::Anthropic
        );
    }
}

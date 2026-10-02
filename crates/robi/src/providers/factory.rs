//! How a configured provider becomes a `Model`.
//!
//! This is the seam the settings layer plugs into. Nothing here reads an
//! environment variable, a file, or a keychain: a caller hands over a
//! `ProviderSettings` with the credential already loaded, and gets back something
//! the loop can drive.

use std::sync::Arc;

use robi_core::model::Model;
use robi_core::tool::ToolRegistry;

use super::catalog::ModelCatalog;
use super::config::ProviderSettings;
use super::error::ProviderError;
use super::openai::OpenAiCompatibleModel;

/// Build the model an `Agent` will drive.
///
/// Refuses a model the catalog does not list, and one that cannot call tools, so a
/// misconfiguration fails here with the model's name rather than on the first turn.
///
/// **Switching models.** A later milestone adds a `ModelRouter` here: a `Model`
/// itself that resolves a session's configured model and dispatches, so `Agent`
/// keeps holding one `Arc<dyn Model>`. `Model::generate` already receives the
/// `SessionId` that router needs, so the router is additive — see D8 in
/// `docs/design/providers-streaming.md`.
pub fn build_model(
    settings: ProviderSettings,
    tools: Arc<ToolRegistry>,
) -> Result<Arc<dyn Model>, ProviderError> {
    let catalog = Arc::new(ModelCatalog::opencode_go());
    let model = OpenAiCompatibleModel::new(settings, catalog, tools)?;
    Ok(Arc::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::ModelInfo;
    use crate::providers::config::{ApiKey, ModelId};

    fn settings_for(model: &str) -> ProviderSettings {
        ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new(model))
    }

    #[test]
    fn a_known_model_builds() {
        let model = build_model(settings_for("glm-5.3"), Arc::new(ToolRegistry::new()));
        assert!(model.is_ok());
    }

    #[test]
    fn an_unknown_model_is_refused_at_construction() {
        let error = build_model(settings_for("not-a-model"), Arc::new(ToolRegistry::new()))
            .err()
            .expect("the catalog does not list it");
        assert!(matches!(error, ProviderError::UnknownModel(_)), "{error:?}");
        assert!(
            error.to_string().contains("not-a-model"),
            "the error names the model: {error}"
        );
    }

    #[test]
    fn a_model_without_tool_support_is_refused_at_construction() {
        // The catalog only lists tool-capable models, so this path is exercised
        // with a catalog that does, which is what a bad regeneration would produce.
        let catalog = Arc::new(ModelCatalog::from_models(vec![ModelInfo {
            id: ModelId::new("text-only"),
            display_name: "Text Only".to_owned(),
            context_window: 8_192,
            max_output: 1_024,
            supports_tools: false,
            supports_reasoning: false,
        }]));

        let error = OpenAiCompatibleModel::new(
            settings_for("text-only"),
            catalog,
            Arc::new(ToolRegistry::new()),
        )
        .expect_err("a tool-less model cannot drive the loop");
        assert!(
            matches!(error, ProviderError::ToolLessModel(_)),
            "{error:?}"
        );
    }
}

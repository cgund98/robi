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
use super::images::ImageSource;
use super::openai::OpenAiCompatibleModel;

/// Build the model an `Agent` will drive.
///
/// Refuses a model the catalog does not list, and one that cannot call tools, so a
/// misconfiguration fails here with the model's name rather than on the first turn.
///
/// **Switching models.** The session actor resolves the choice before it calls
/// this, and keeps the model for that execution. See D8 in
/// `docs/design/providers-streaming.md`.
///
/// `images` provides the bytes behind a user message's attachments (D11), the
/// same way `tools` provides the request's tool definitions: neither is given to
/// `Model::generate`, so both are wired in at construction.
pub fn build_model(
    settings: ProviderSettings,
    tools: Arc<ToolRegistry>,
    images: Arc<dyn ImageSource>,
) -> Result<Arc<dyn Model>, ProviderError> {
    let catalog = Arc::new(ModelCatalog::opencode_go());
    let model = OpenAiCompatibleModel::new(settings, catalog, tools, images)?;
    Ok(Arc::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::providers::catalog::ModelInfo;
    use crate::agent::providers::config::{ApiKey, ModelId};
    use crate::agent::providers::images::ImageSource;
    use crate::agent::providers::ProviderError;

    fn settings_for(model: &str) -> ProviderSettings {
        ProviderSettings::opencode_go(ApiKey::new("k"), ModelId::new(model))
    }

    /// No attachment is resolved by these construction tests.
    struct NoImages;

    #[async_trait::async_trait]
    impl ImageSource for NoImages {
        async fn image(&self, _id: &str) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
            Ok(None)
        }
    }

    fn no_images() -> Arc<dyn ImageSource> {
        Arc::new(NoImages)
    }

    #[test]
    fn a_known_model_builds() {
        let model = build_model(
            settings_for("glm-5.3"),
            Arc::new(ToolRegistry::new()),
            no_images(),
        );
        assert!(model.is_ok());
    }

    #[test]
    fn an_unknown_model_is_refused_at_construction() {
        let error = build_model(
            settings_for("not-a-model"),
            Arc::new(ToolRegistry::new()),
            no_images(),
        )
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
            supports_vision: true,
        }]));

        let error = match OpenAiCompatibleModel::new(
            settings_for("text-only"),
            catalog,
            Arc::new(ToolRegistry::new()),
            no_images(),
        ) {
            Ok(_) => panic!("a tool-less model cannot drive the loop"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::ToolLessModel(_)),
            "{error:?}"
        );
    }
}

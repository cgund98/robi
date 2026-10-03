//! The model table: what a model is, and how much context it holds.
//!
//! The table is vendored rather than fetched. A startup fetch would make the app
//! fail without connectivity, and a desktop tool should not phone home to render a
//! context meter. Regenerate [`opencode_go`] from a models.dev snapshot instead of
//! hand-editing it.

mod opencode_go;

use super::config::ModelId;

/// One model a provider serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: ModelId,
    pub display_name: String,
    /// The model's own advertised window.
    ///
    /// Not a plan tier. The vendor's tiered figures change at a token threshold
    /// and are about *price*; they belong in a cost table, never in the window the
    /// context meter reads.
    pub context_window: u64,
    pub max_output: u64,
    pub supports_tools: bool,
    pub supports_reasoning: bool,
}

/// Every model one endpoint serves.
#[derive(Debug, Clone)]
pub struct ModelCatalog {
    models: Vec<ModelInfo>,
}

impl ModelCatalog {
    /// The models OpenCode Go serves.
    pub fn opencode_go() -> Self {
        Self {
            models: opencode_go::models(),
        }
    }

    pub fn from_models(models: Vec<ModelInfo>) -> Self {
        Self { models }
    }

    pub fn get(&self, id: &ModelId) -> Option<&ModelInfo> {
        self.models.iter().find(|info| info.id == *id)
    }

    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// The models that can drive this agent loop, in table order.
    pub fn tool_capable(&self) -> impl Iterator<Item = &ModelInfo> {
        self.models.iter().filter(|info| info.supports_tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_holds_the_opencode_go_table() {
        let catalog = ModelCatalog::opencode_go();
        assert_eq!(catalog.len(), 33, "the vendored snapshot has 33 models");
        assert!(catalog.get(&ModelId::new("glm-5.3")).is_some());
        assert!(catalog.get(&ModelId::new("not-a-model")).is_none());
    }

    #[test]
    fn every_listed_model_can_call_tools() {
        // D9: a model that cannot call tools cannot drive the loop, so listing one
        // would only invite a session that fails.
        let catalog = ModelCatalog::opencode_go();
        let tool_less: Vec<&str> = catalog
            .models()
            .iter()
            .filter(|info| !info.supports_tools)
            .map(|info| info.id.as_str())
            .collect();
        assert!(
            tool_less.is_empty(),
            "the catalog lists models without tool support: {tool_less:?}"
        );
        assert_eq!(catalog.tool_capable().count(), catalog.len());
    }

    #[test]
    fn a_models_output_budget_never_exceeds_its_window() {
        // Equality is legitimate: `kimi-k2.7-code` advertises a 262,144 output
        // budget against a 262,144 window. Larger would mean the table is wrong.
        for info in ModelCatalog::opencode_go().models() {
            assert!(
                info.max_output <= info.context_window,
                "{} reports a {} output budget for a {} window",
                info.id,
                info.max_output,
                info.context_window
            );
        }
    }

    #[test]
    fn model_ids_are_unique() {
        let catalog = ModelCatalog::opencode_go();
        let mut ids: Vec<&str> = catalog.models().iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "a duplicate id would shadow a model");
    }
}

//! The model table: what a model is, and how much context it holds.
//!
//! The table is vendored rather than fetched. A startup fetch would make the app
//! fail without connectivity, and a desktop tool should not phone home to render a
//! context meter. Regenerate [`opencode_go`] from a models.dev snapshot instead of
//! hand-editing it.

mod anthropic;
mod deepseek;
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
    /// Whether the model accepts an effort setting (Anthropic's `output_config`).
    ///
    /// Anthropic's Haiku 4.5 rejects it outright ("Extra inputs are not
    /// permitted"), so the flag is `false` there and the adapter never sends it
    /// (A2). Harmless and unused on a chat-completions model, where effort rides
    /// on `reasoning_effort` and is never gated.
    pub supports_effort: bool,
    /// Whether the model accepts image input. Permissive by default: a model
    /// whose snapshot does not state the flag is assumed capable, so a missing
    /// flag never blocks a working model — the provider's 400 is the fallback.
    /// The vendored table sets it explicitly on every row so a bad regeneration
    /// fails the snapshot test rather than silently taking a guess.
    pub supports_vision: bool,
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

    /// The models Anthropic serves.
    pub fn anthropic() -> Self {
        Self {
            models: anthropic::models(),
        }
    }

    /// The models DeepSeek serves.
    pub fn deepseek() -> Self {
        Self {
            models: deepseek::models(),
        }
    }

    /// Both catalogs in one flat list, OpenCode Go first then Anthropic.
    ///
    /// The dropdown renders this (A11). Ids are prefixed and disjoint, so one
    /// list is unambiguous.
    pub fn all() -> Self {
        let mut models = opencode_go::models();
        models.extend(anthropic::models());
        models.extend(deepseek::models());
        Self { models }
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
    use crate::agent::providers::config::{ANTHROPIC_PREFIX, DEEPSEEK_PREFIX, OPENCODE_GO_PREFIX};

    #[test]
    fn the_catalog_holds_the_opencode_go_table() {
        let catalog = ModelCatalog::opencode_go();
        assert_eq!(catalog.len(), 33, "the vendored snapshot has 33 models");
        assert!(catalog.get(&ModelId::new("ocg_glm-5.3")).is_some());
        assert!(catalog.get(&ModelId::new("not-a-model")).is_none());
    }

    #[test]
    fn the_catalog_holds_the_anthropic_table() {
        let catalog = ModelCatalog::anthropic();
        for id in [
            "ant_claude-fable-5-1",
            "ant_claude-opus-5-5",
            "ant_claude-sonnet-5-5",
            "ant_claude-haiku-4-5",
        ] {
            assert!(catalog.get(&ModelId::new(id)).is_some(), "missing {id}");
        }
        assert!(catalog.get(&ModelId::new("not-a-model")).is_none());
    }

    #[test]
    fn every_id_carries_its_providers_prefix() {
        // A11: the prefix is the dispatch, so every row must carry the right one.
        for info in ModelCatalog::opencode_go().models() {
            assert!(
                info.id.as_str().starts_with(OPENCODE_GO_PREFIX),
                "{} is missing the {OPENCODE_GO_PREFIX} prefix",
                info.id
            );
        }
        for info in ModelCatalog::anthropic().models() {
            assert!(
                info.id.as_str().starts_with(ANTHROPIC_PREFIX),
                "{} is missing the {ANTHROPIC_PREFIX} prefix",
                info.id
            );
        }
        for info in ModelCatalog::deepseek().models() {
            assert!(
                info.id.as_str().starts_with(DEEPSEEK_PREFIX),
                "{} is missing the {DEEPSEEK_PREFIX} prefix",
                info.id
            );
        }
    }

    #[test]
    fn the_two_catalogs_are_disjoint() {
        // A11: the union is the namespace, so an id in both would be ambiguous.
        let opencode_ids = ModelCatalog::opencode_go();
        let opencode: std::collections::HashSet<&str> = opencode_ids
            .models()
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        for info in ModelCatalog::anthropic().models() {
            assert!(
                !opencode.contains(info.id.as_str()),
                "{} appears in both catalogs",
                info.id
            );
        }
    }

    #[test]
    fn haiku_does_not_support_effort() {
        // A2: the user's constraint. Sending `output_config.effort` to Haiku is a
        // 400, so the flag must be false and every listed model must be explicit.
        let catalog = ModelCatalog::anthropic();
        for (id, supports) in [
            ("ant_claude-fable-5-1", true),
            ("ant_claude-opus-5-5", true),
            ("ant_claude-sonnet-5-5", true),
            ("ant_claude-haiku-4-5", false),
        ] {
            let info = catalog.get(&ModelId::new(id)).expect("listed");
            assert_eq!(info.supports_effort, supports, "{id}");
        }
    }

    #[test]
    fn every_listed_model_can_call_tools() {
        // D9: a model that cannot call tools cannot drive the loop, so listing one
        // would only invite a session that fails.
        for catalog in [
            ModelCatalog::opencode_go(),
            ModelCatalog::anthropic(),
            ModelCatalog::deepseek(),
        ] {
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
    }

    #[test]
    fn every_row_states_whether_it_accepts_images() {
        // D13: vision is a real capability, and gating happens in the adapter at
        // request build. The generator fills `supports_vision` on every row, so a
        // row that somehow lost the flag would be a generator bug a regeneration
        // would have to make deliberately.
        let catalog = ModelCatalog::opencode_go();
        assert_eq!(catalog.len(), 33);
        assert_eq!(
            catalog
                .models()
                .iter()
                .filter(|m| m.supports_vision)
                .count(),
            catalog.len(),
            "every vendored row must state whether it accepts image input"
        );
    }

    #[test]
    fn a_models_output_budget_never_exceeds_its_window() {
        // Equality is legitimate: `kimi-k2.7-code` advertises a 262,144 output
        // budget against a 262,144 window. Larger would mean the table is wrong.
        for catalog in [
            ModelCatalog::opencode_go(),
            ModelCatalog::anthropic(),
            ModelCatalog::all(),
        ] {
            for info in catalog.models() {
                assert!(
                    info.max_output <= info.context_window,
                    "{} reports a {} output budget for a {} window",
                    info.id,
                    info.max_output,
                    info.context_window
                );
            }
        }
    }

    #[test]
    fn model_ids_are_unique() {
        let catalog = ModelCatalog::all();
        let mut ids: Vec<&str> = catalog.models().iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "a duplicate id would shadow a model");
    }
}

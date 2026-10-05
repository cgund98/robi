//! The DeepSeek model table.
//!
//! Vendored rather than fetched, like the other tables: startup must not need the
//! network. Windows and output budgets are the figures on the vendor's pricing
//! page. `deepseek-flash` is the current name for V4.1-Flash; `deepseek-v4-pro`
//! is V4-Pro. Both call tools. Only Flash accepts images.

use super::ModelInfo;
use crate::agent::providers::config::{ModelId, DEEPSEEK_PREFIX};

/// `(bare id, display name, supports vision)`.
///
/// Both models advertise a 1,000,000-token window and a 384,000-token output
/// budget.
const MODELS: &[(&str, &str, bool)] = &[
    ("deepseek-flash", "DeepSeek Flash", true),
    ("deepseek-v4-pro", "DeepSeek V4 Pro", false),
];

pub(super) fn models() -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(|(id, display_name, supports_vision)| ModelInfo {
            id: ModelId::new(format!("{DEEPSEEK_PREFIX}{id}")),
            display_name: (*display_name).to_owned(),
            context_window: 1_000_000,
            max_output: 384_000,
            supports_tools: true,
            supports_reasoning: true,
            supports_effort: true,
            supports_vision: *supports_vision,
        })
        .collect()
}

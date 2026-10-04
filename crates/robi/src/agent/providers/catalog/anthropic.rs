//! The Anthropic model table.
//!
//! Vendored rather than fetched, like the OpenCode Go table: startup must not need
//! the network, and a desktop tool should not phone home to render a context meter.
//! The `MODELS` list holds the vendor's bare ids; [`models`] prefixes each with
//! `ant_` (A11).
//!
//! `supports_effort` is the load-bearing flag here (A2). `output_config.effort` is
//! stable on the 4.6 models, beta on Opus 4.5, and **rejected outright by Haiku
//! 4.5** — "Extra inputs are not permitted" — so Haiku carries `false` and the
//! adapter never sends the field for it.

use super::ModelInfo;
use crate::agent::providers::config::{ModelId, ANTHROPIC_PREFIX};

/// `(bare id, display name, context window, max output, supports effort)`.
///
/// Windows and output budgets are the models' own advertised figures. The API
/// requires `max_tokens`, so `max_output` is the request's cap (A5).
const MODELS: &[(&str, &str, u64, u64, bool)] = &[
    (
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        200_000,
        64_000,
        true,
    ),
    ("claude-opus-4-6", "Claude Opus 4.6", 200_000, 32_000, true),
    ("claude-opus-4-5", "Claude Opus 4.5", 200_000, 64_000, true),
    (
        "claude-haiku-4-5",
        "Claude Haiku 4.5",
        200_000,
        64_000,
        false,
    ),
];

pub(super) fn models() -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(
            |(id, display_name, context_window, max_output, supports_effort)| ModelInfo {
                id: ModelId::new(format!("{ANTHROPIC_PREFIX}{id}")),
                display_name: (*display_name).to_owned(),
                context_window: *context_window,
                max_output: *max_output,
                // Every Claude model in this table calls tools and reasons.
                supports_tools: true,
                supports_reasoning: true,
                supports_effort: *supports_effort,
                // Claude 3+ models accept image input.
                supports_vision: true,
            },
        )
        .collect()
}

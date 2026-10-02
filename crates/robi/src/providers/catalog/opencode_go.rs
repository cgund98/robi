//! The OpenCode Go model table.
//!
//! Generated from the models.dev `opencode-go` snapshot. Do not hand-edit: refresh
//! by regenerating this file, and re-run the catalog tests, which assert the count,
//! that every entry supports tool calling, and that the windows are coherent.
//!
//! The endpoint is a proxy in front of models from several labs. Context windows
//! and output budgets below are the models' own advertised figures.

use super::ModelInfo;
use crate::providers::config::ModelId;

/// `(id, display name, context window, max output)`.
const MODELS: &[(&str, &str, u64, u64)] = &[
    ("glm-5.3", "GLM-5.3", 1_000_000, 131_072),
    ("glm-5.3-flash", "GLM-5.3-Flash", 1_000_000, 131_072),
    ("glm-5.2", "GLM-5.2", 1_000_000, 131_072),
    ("kimi-k3", "Kimi K3", 1_048_576, 131_072),
    ("kimi-k2.7-code", "Kimi K2.7 Code", 262_144, 262_144),
    ("kimi-k2.6", "Kimi K2.6", 262_144, 65_536),
    ("deepseek-v4-pro", "DeepSeek V4 Pro", 1_000_000, 384_000),
    (
        "deepseek-v4.1-flash",
        "DeepSeek V4.1 Flash",
        1_000_000,
        384_000,
    ),
    ("deepseek-v4-flash", "DeepSeek V4 Flash", 1_000_000, 384_000),
    (
        "deepseek-v4-flash-vision-exp",
        "DeepSeek V4 Flash Vision Exp",
        1_000_000,
        384_000,
    ),
    ("qwen3.8-max", "Qwen3.8 Max", 1_000_000, 131_072),
    ("qwen3.8-flash", "Qwen3.8 Flash", 1_000_000, 131_072),
    ("qwen3.7-plus", "Qwen3.7 Plus", 1_000_000, 65_536),
    ("qwen3.7-max", "Qwen3.7 Max", 1_000_000, 65_536),
    ("qwen3.6-plus", "Qwen3.6 Plus", 1_000_000, 65_536),
    ("minimax-m3", "MiniMax M3", 1_000_000, 131_072),
    ("minimax-m2.7", "MiniMax M2.7", 204_800, 131_072),
    ("grok-4.7", "Grok 4.7", 500_000, 500_000),
    ("grok-4.6", "Grok 4.6", 500_000, 500_000),
    ("grok-4.5", "Grok 4.5", 500_000, 500_000),
    ("gpt-6-luna", "GPT-6 Luna", 1_050_000, 128_000),
    ("gpt-5.6-luna", "GPT-5.6 Luna", 1_050_000, 128_000),
    ("mimo-v2.6-pro", "MiMo-V2.6-Pro", 1_048_576, 131_072),
    ("mimo-v2.6-flash", "MiMo-V2.6-Flash", 1_048_576, 131_072),
    ("mimo-v2.5-pro", "MiMo V2.5 Pro", 1_048_576, 128_000),
    ("mimo-v2.5", "MiMo V2.5", 1_000_000, 128_000),
    ("longcat-2.0", "LongCat-2.0", 1_000_000, 131_072),
    (
        "longcat-2.5-preview-free",
        "LongCat 2.5 Preview",
        1_000_000,
        131_072,
    ),
    ("hy3", "Hy3", 256_000, 128_000),
    ("hy4-preview", "Hy4 preview", 1_024_000, 64_000),
    (
        "muse-spark-1.3-contributor",
        "Muse Spark 1.3 Contributor",
        1_048_576,
        131_072,
    ),
    (
        "muse-spark-1.2-contributor",
        "Muse Spark 1.2 Contributor",
        1_048_576,
        131_072,
    ),
    ("space-bunny-free", "Space Bunny Free", 1_048_576, 524_288),
];

pub(super) fn models() -> Vec<ModelInfo> {
    MODELS
        .iter()
        .map(|(id, display_name, context_window, max_output)| ModelInfo {
            id: ModelId::new(*id),
            display_name: (*display_name).to_owned(),
            context_window: *context_window,
            max_output: *max_output,
            // Every model this endpoint serves reports tool calling and reasoning.
            supports_tools: true,
            supports_reasoning: true,
        })
        .collect()
}

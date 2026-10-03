//! The I/O that runs a turn.
//!
//! `providers` reaches model APIs. `workspace` resolves paths and applies
//! session path rules. `tools` is the workspace tools. `sandbox` confines the
//! shell tool's child process. `review` diffs a session's baselines against
//! the files on disk. `prompt` assembles the system prompt from the built-in
//! text, user settings, and instruction files. `compress` shortens tool
//! output. `web` fetches and searches pages. `skills` and `mcp` load extra
//! instructions and remote tools. `lsp` and `index` answer code questions.

pub mod compress;
pub mod index;
pub mod lsp;
pub mod mcp;
pub mod prompt;
pub mod providers;
pub mod review;
pub mod sandbox;
pub mod skills;
pub mod tools;
pub mod web;
pub mod workspace;

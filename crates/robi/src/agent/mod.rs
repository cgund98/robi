//! The I/O that runs a turn.
//!
//! `providers` reaches model APIs. `workspace` resolves paths and applies
//! session path rules. `tools` is the workspace tools. `sandbox` confines the
//! shell tool's child process. `review` diffs a session's baselines against
//! the files on disk. `prompt` assembles the system prompt from the built-in
//! text, user settings, and instruction files. `compress` shortens tool
//! output. `web` fetches and searches pages. `skills` and `mcp` load extra
//! instructions and remote tools. `files` renders a user's attachments for the
//! provider. `lsp` answers code questions by line and symbol, and `index` does
//! the same by meaning when the `semantic-index` feature is on. `docs`
//! reconciles piece-meal editor saves against the file on disk.

pub mod blocking;
pub mod compact;
pub mod compress;
pub mod docs;
pub mod files;
#[cfg(feature = "semantic-index")]
pub mod index;
// The client is kept in every build; only the tools that drive it are gated.
// Much of it is unreachable without them, which is not a defect.
#[cfg_attr(not(feature = "lsp-tools"), allow(dead_code, unused_imports))]
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

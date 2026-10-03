//! The implementations that do I/O, behind the traits `robi-core` declares.
//!
//! `providers` reaches model APIs. `domain` is the HTTP API's model and
//! services, `adapters` is SQLite and the settings files, and `web_api` is the
//! Axum surface. `workspace` resolves paths and applies session path rules.
//! `tools` is the workspace tools. `sandbox` confines the shell tool's child
//! process. `review` diffs a session's baselines against
//! the files on disk. `prompt` assembles the system
//! prompt from the built-in text, user settings, and instruction files. The
//! `robi-api` binary wires them.

pub mod adapters;
pub mod domain;
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
pub mod web_api;
pub mod workspace;

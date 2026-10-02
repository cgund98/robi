//! The implementations that do I/O, behind the traits `robi-core` declares.
//!
//! `providers` reaches model APIs. `domain` is the HTTP API's model and
//! services, `adapters` is SQLite and the settings files, and `web_api` is the
//! Axum surface. `workspace` resolves paths and applies session path rules.
//! `tools` is the workspace tools. `review` diffs a session's baselines against
//! the files on disk. `prompt` assembles the system
//! prompt from the built-in text, user settings, and instruction files. The
//! `robi-api` binary wires them.

pub mod adapters;
pub mod domain;
pub mod prompt;
pub mod providers;
pub mod review;
pub mod tools;
pub mod web_api;
pub mod workspace;

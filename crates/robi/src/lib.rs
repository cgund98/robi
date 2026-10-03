//! The implementations that do I/O, behind the traits `robi-core` declares.
//!
//! `agent` runs a turn: model clients, tools, the prompt, and the workspace.
//! `domain` is the HTTP API's model and services, `adapters` is SQLite and the
//! settings files, and `web_api` is the Axum surface. The `robi-api` binary
//! wires them.

pub mod adapters;
pub mod agent;
pub mod domain;
pub mod web_api;

//! The implementations that do I/O, behind the traits `robi-core` declares.
//!
//! `agent` runs a turn: model clients, tools, the prompt, and the workspace.
//! `domain` is the HTTP API's model and services, `adapters` is SQLite and the
//! settings files, and `web_api` is the Axum surface. `bootstrap` wires them
//! for the `robi-api` binary and the Tauri process. `tui` is the terminal front
//! end, behind the `tui` feature. `mcp_tools` is the `mcp-tools` binary: one
//! MCP server's tool list, read from the same files the app reads.

pub mod adapters;
pub mod agent;
pub mod bootstrap;
pub mod domain;
pub mod logs;
pub mod mcp_tools;
#[cfg(feature = "tui")]
pub mod tui;
pub mod web_api;

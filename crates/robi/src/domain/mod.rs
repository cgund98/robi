//! Product types for the HTTP API.
//!
//! No SQLite and no Axum. `web_api` and `adapters` depend on this module; it
//! depends on neither.

pub mod chat_message;
pub mod chat_session;
pub mod error;

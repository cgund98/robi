//! Infrastructure behind the domain traits.
//!
//! Depends on `domain`. Does not depend on `web_api`.

pub mod chat_message;
pub mod chat_runtime;
pub mod chat_session;
pub mod file_change;
pub mod model_source;
pub mod session_title;
pub mod settings;
pub mod sqlite;
pub mod workspace;

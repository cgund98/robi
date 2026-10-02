//! String settings, read live and written through [`store::SettingsStore`].
//!
//! No files and no HTTP. The TOML files live in `adapters`, and the routes
//! live in `web_api`.

pub mod keys;
pub mod service;
pub mod store;

#[cfg(test)]
pub(crate) mod memory;

pub use service::SettingsService;
pub use store::{Setting, SettingsStore};

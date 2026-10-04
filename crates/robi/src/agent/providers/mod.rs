//! Model providers: settings, transport, and the wire mapping that turns a
//! transcript into a request and a stream of deltas.
//!
//! The adapter behind `robi_core::Model` lives here. `robi-core` declares the
//! trait and the delta vocabulary; nothing in this module is visible to the loop
//! except through that trait.

pub mod catalog;
pub mod config;
pub mod error;
pub mod factory;
pub mod images;
pub mod openai;
pub mod retry;

pub use catalog::{ModelCatalog, ModelInfo};
pub use config::{ApiKey, ModelId, ProviderId, ProviderSettings, ReasoningEffort};
pub use error::ProviderError;
pub use factory::build_model;
pub use images::{ImageSource, ImageStore};
pub use retry::RetryPolicy;

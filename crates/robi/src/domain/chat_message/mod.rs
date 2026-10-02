//! Submitting an instruction and reading the transcript.

pub mod runtime;
pub mod service;

pub use runtime::{ChatRuntime, SubmitOutcome};
pub use service::ChatMessageService;

//! The implementations that do I/O, behind the traits `robi-core` declares.
//!
//! `providers` reaches model APIs. `domain` is the HTTP API's model and
//! services, `adapters` is SQLite and the settings files, and `web_api` is the
//! Axum surface. The `robi-api` binary wires those three. Tools and workspace
//! follow in later milestones, each as its own module here rather than a new
//! crate.

pub mod adapters;
pub mod domain;
pub mod providers;
pub mod web_api;

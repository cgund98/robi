//! The Robi agent loop.
//!
//! This crate holds the transcript, the tool trait and registry, the turn state
//! machine, and the events it emits. It performs no I/O: it depends on traits it
//! declares, and the implementations live in other crates.

// The crate boundary in `Cargo.toml` keeps out every dependency, but `std::fs`
// and `std::net` ship with `std`. The rules are in `clippy.toml` beside this
// manifest; these attributes make a violation fail the build rather than warn.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

pub mod agent;
pub mod compress;
pub mod config;
pub mod error;
pub mod event;
pub mod ids;
pub mod message;
pub mod model;
pub mod prompt;
pub mod segments;
pub mod store;
pub mod tool;

#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;

//! Language servers for the workspace the model is editing.
//!
//! The design is [docs/design/lsp.md](../../../docs/design/lsp.md). `robi-core`
//! does not spawn a process. One hub in the API process shares a server across
//! sessions on the same workspace root.

mod catalog;
mod client;
mod convert;
mod hub;
mod position;

use std::time::Duration;

pub use client::LspError;
pub use hub::{LspHub, Unavailable};

#[cfg(test)]
pub(crate) use client::connect_fake;
pub(crate) use client::{LspClient, SyncedDoc, MAX_FILE_BYTES};
pub(crate) use convert::{diagnostic_items, location_hits, scalar_column, symbol_kind_name};
pub(crate) use position::{to_lsp, PositionError};

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub initialize: Duration,
    pub request: Duration,
    pub diagnostics: Duration,
    pub settle: Duration,
    pub idle: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(30),
            request: Duration::from_secs(10),
            diagnostics: Duration::from_secs(8),
            settle: Duration::from_millis(400),
            idle: Duration::from_secs(5 * 60),
        }
    }
}

#[cfg(test)]
impl Timing {
    pub(crate) fn fast() -> Self {
        Self {
            initialize: Duration::from_secs(2),
            request: Duration::from_secs(2),
            diagnostics: Duration::from_millis(200),
            settle: Duration::from_millis(20),
            idle: Duration::from_secs(60),
        }
    }
}

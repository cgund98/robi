//! Reconcile piece-meal editor saves with the file on disk.
//!
//! The docs editor sends CodeMirror change sets, not whole buffers. A write
//! names the base version it was built from. When that base is still what the
//! disk holds, the change set applies cleanly. When an agent edit landed in
//! between, [`merge3`] combines the two so non-overlapping work survives and
//! the user's edit wins where they collide.
//!
//! Nothing here touches the filesystem. The save handler reads and writes.

mod change;
mod merge;

pub use change::{apply_changes, version_hash, ChangeRange, DocsEditCache, DEFAULT_CACHE_ENTRIES};
pub use merge::merge3;

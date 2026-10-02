//! Diffs of the files a chat session has changed.
//!
//! The edit tools store a baseline. This module compares it with the file on
//! disk. Review will call `reject` to put a hunk back.

mod diff;
mod session;

pub use diff::{diff, reject, FileDiff, FileStatus, Hunk};
pub use session::hunks_for_session;

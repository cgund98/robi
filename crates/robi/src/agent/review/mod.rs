//! Diffs of the files a chat session has changed.
//!
//! The edit tools store a baseline. This module compares it with the file on
//! disk. `decide_review` keeps a change or puts a hunk back.

mod diff;
#[cfg(feature = "review")]
mod session;

pub use diff::{
    accept, diff, reject, review_lines, split_lines, FileDiff, FileStatus, Hunk, ReviewLine,
    ReviewLineKind,
};
#[cfg(feature = "review")]
pub use session::{
    decide_review, hunks_for_session, review_file, review_for_session, review_summaries,
    ReviewDecision, ReviewFile, ReviewSummary,
};

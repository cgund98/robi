//! Workspace path resolution, the session path filter, and the file opener.
//!
//! No database access. Callers pass a root and the rules stored on the session.

mod open;
mod path_filter;
mod paths;

pub use open::{argv_for, open, OpenError};
pub use path_filter::{directory_exclusion_globs, PathFilter};
pub use paths::{
    absolute_inside, find_binary, resolve_path, user_home, workspace_relative, PathError,
    ResolveError, ResolvedPath,
};

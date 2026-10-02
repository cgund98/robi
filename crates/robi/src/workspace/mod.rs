//! Workspace path resolution and the session path filter.
//!
//! No database access. Callers pass a root and the rules stored on the session.

mod path_filter;
mod paths;

pub use path_filter::{directory_exclusion_globs, PathFilter};
pub use paths::{resolve_path, user_home, workspace_relative, ResolveError, ResolvedPath};

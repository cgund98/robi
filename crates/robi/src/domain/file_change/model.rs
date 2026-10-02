//! One path this chat session has changed, and the bytes from before that.

/// The file body before this session's first write or delete of `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBaseline {
    /// Workspace-relative path, `/` separators.
    pub path: String,
    /// UTF-8 body before the first change. Empty when the file did not exist.
    pub baseline: String,
    /// The file did not exist before this session changed it.
    pub created: bool,
}

//! Shared read-modify-write for anything that writes a file text.
//!
//! These are not edit-tool helpers: `write_plan`, `todos`, and the docs editor
//! use them too, so this module stays in the default build while the edit tools
//! are gated.

use std::path::Path;

use robi_core::error::ToolError;
use robi_core::ids::SessionId;

use crate::domain::file_change::repo::FileChangeRepository;

pub use super::path_lock::lock_path;

pub fn read_text(path: &Path) -> Result<Option<String>, ToolError> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| ToolError::Failed("file is not utf-8".to_owned())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ToolError::Failed(format!("read file: {err}"))),
    }
}

pub fn ensure_parent(path: &Path) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|err| ToolError::Failed(format!("create directory: {err}")))?;
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, text: &str) -> Result<(), ToolError> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let tmp = path.with_file_name(format!(".{name}.robi-tmp"));
    if let Err(err) = std::fs::write(&tmp, text.as_bytes()) {
        return Err(ToolError::Failed(format!("write file: {err}")));
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ToolError::Failed(format!("write file: {err}")));
    }
    Ok(())
}

// Only the edit tools and the docs editor record a baseline.
#[cfg_attr(
    not(any(feature = "edit-tools", feature = "docs-viewer")),
    allow(dead_code)
)]
pub async fn record_baseline(
    repo: &dyn FileChangeRepository,
    session_id: SessionId,
    path: &str,
    baseline: &str,
    created: bool,
) -> Result<(), ToolError> {
    repo.record_baseline(session_id, path, baseline, created)
        .await
        .map_err(|err| ToolError::Failed(err.to_string()))?;
    Ok(())
}

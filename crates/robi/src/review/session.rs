//! Session baselines compared with the files on disk.

use std::path::{Path, PathBuf};

use robi_core::ids::SessionId;

use crate::domain::{
    error::ServiceError,
    file_change::{model::FileBaseline, repo::FileChangeRepository},
};

use super::diff::{diff, FileDiff, FileStatus};

/// One file diff per baseline this session still tracks.
///
/// A missing file is a deletion. A file this session created, whose baseline
/// is empty and whose file is already gone, is omitted.
pub async fn hunks_for_session(
    repo: &dyn FileChangeRepository,
    root: &Path,
    session_id: SessionId,
) -> Result<Vec<FileDiff>, ServiceError> {
    let baselines = repo.list_baselines(session_id).await?;
    let mut diffs = Vec::with_capacity(baselines.len());
    for baseline in baselines {
        if let Some(file_diff) = diff_baseline(root, &baseline)? {
            diffs.push(file_diff);
        }
    }
    Ok(diffs)
}

fn diff_baseline(root: &Path, baseline: &FileBaseline) -> Result<Option<FileDiff>, ServiceError> {
    let absolute = absolute_from_relative(root, &baseline.path);
    let current = match std::fs::read(&absolute) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => {
                return Err(ServiceError::BadRequest(format!(
                    "file is not utf-8: {}",
                    baseline.path
                )));
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(ServiceError::BadRequest(format!(
                "read {}: {err}",
                baseline.path
            )));
        }
    };

    if current.is_none() && baseline.created && baseline.baseline.is_empty() {
        return Ok(None);
    }

    let current = current.unwrap_or_default();
    let mut file_diff = diff(&baseline.path, &baseline.baseline, &current);
    if current.is_empty() && !absolute.exists() {
        file_diff.status = FileStatus::Deleted;
    }
    Ok(Some(file_diff))
}

fn absolute_from_relative(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    if !relative.is_empty() && relative != "." {
        for part in relative.split('/') {
            path.push(part);
        }
    }
    path
}

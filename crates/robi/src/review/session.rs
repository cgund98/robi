//! Session baselines compared with the files on disk.

use std::path::{Path, PathBuf};

use robi_core::ids::SessionId;

use crate::domain::{
    error::ServiceError,
    file_change::{model::FileBaseline, repo::FileChangeRepository},
};

use super::diff::{accept, diff, reject, review_lines, FileDiff, FileStatus, Hunk, ReviewLine};
use super::lock::lock_path;

/// One changed path, with the lines a review shows.
pub struct ReviewFile {
    pub path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    /// File body before this session's first change of the path.
    pub baseline: String,
    /// File body on disk now. Empty when the file is gone.
    pub current: String,
    pub lines: Vec<ReviewLine>,
    pub hunks: Vec<Hunk>,
}

/// Changed paths for this session. A baseline that matches the file on disk
/// is omitted, as is a file this session created that is already gone.
pub async fn review_for_session(
    repo: &dyn FileChangeRepository,
    root: &Path,
    session_id: SessionId,
) -> Result<Vec<ReviewFile>, ServiceError> {
    let baselines = repo.list_baselines(session_id).await?;
    let mut files = Vec::with_capacity(baselines.len());
    for baseline in baselines {
        if let Some(file) = review_baseline(root, &baseline)? {
            files.push(file);
        }
    }
    Ok(files)
}

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

fn review_baseline(
    root: &Path,
    baseline: &FileBaseline,
) -> Result<Option<ReviewFile>, ServiceError> {
    let Some((current, exists)) = read_current(root, baseline)? else {
        return Ok(None);
    };
    let mut file_diff = diff(&baseline.path, &baseline.baseline, &current);
    if current.is_empty() && !exists {
        file_diff.status = FileStatus::Deleted;
    }
    if file_diff.additions == 0 && file_diff.deletions == 0 {
        return Ok(None);
    }
    Ok(Some(ReviewFile {
        path: file_diff.path,
        status: file_diff.status,
        additions: file_diff.additions,
        deletions: file_diff.deletions,
        lines: review_lines(&baseline.baseline, &current),
        baseline: baseline.baseline.clone(),
        current,
        hunks: file_diff.hunks,
    }))
}

/// Keep a change, or put it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approve,
    Reject,
}

/// Apply `decision` to one hunk, or to every hunk of `path` when `hunk_id` is absent.
///
/// Approve folds the current lines into the baseline and leaves the file.
/// Reject writes the baseline lines back. A file this session created, fully
/// put back, is removed along with its baseline.
pub async fn decide_review(
    repo: &dyn FileChangeRepository,
    root: &Path,
    session_id: SessionId,
    path: &str,
    hunk_id: Option<&str>,
    decision: ReviewDecision,
) -> Result<(), ServiceError> {
    if path.is_empty() {
        return Err(ServiceError::BadRequest("path is empty".to_owned()));
    }
    let Some(stored) = repo.get_baseline(session_id, path).await? else {
        return Err(ServiceError::NotFound(path.to_owned()));
    };
    let absolute = absolute_from_relative(root, &stored.path);
    let _guard = lock_path(&absolute).await;
    let Some((current, exists)) = read_current(root, &stored)? else {
        return Err(ServiceError::NotFound(path.to_owned()));
    };
    let file_diff = diff(&stored.path, &stored.baseline, &current);
    let hunks: Vec<&Hunk> = match hunk_id {
        None => file_diff.hunks.iter().collect(),
        Some(id) => {
            let found: Vec<&Hunk> = file_diff
                .hunks
                .iter()
                .filter(|hunk| hunk.id == id)
                .collect();
            if found.is_empty() {
                return Err(ServiceError::BadRequest(format!("unknown hunk: {id}")));
            }
            found
        }
    };
    if hunks.is_empty() {
        return Err(ServiceError::BadRequest(format!("no changes: {path}")));
    }

    match decision {
        ReviewDecision::Approve => {
            if hunk_id.is_none() {
                if exists {
                    repo.replace_baseline(session_id, &stored.path, &current)
                        .await?;
                } else {
                    repo.delete_baseline(session_id, &stored.path).await?;
                }
            } else {
                let mut next = stored.baseline.clone();
                let mut ordered = hunks;
                ordered.sort_by_key(|hunk| std::cmp::Reverse(hunk.old_start));
                for hunk in ordered {
                    next = accept(&next, hunk).map_err(ServiceError::Conflict)?;
                }
                repo.replace_baseline(session_id, &stored.path, &next)
                    .await?;
            }
        }
        ReviewDecision::Reject => {
            let mut next = current.clone();
            let mut ordered = hunks;
            ordered.sort_by_key(|hunk| std::cmp::Reverse(hunk.new_start));
            for hunk in ordered {
                next = reject(&next, hunk).map_err(ServiceError::Conflict)?;
            }
            if next.is_empty() && stored.created && stored.baseline.is_empty() {
                if exists {
                    std::fs::remove_file(&absolute).map_err(|err| {
                        ServiceError::BadRequest(format!("remove {}: {err}", stored.path))
                    })?;
                }
                repo.delete_baseline(session_id, &stored.path).await?;
            } else {
                write_text(&absolute, &next, &stored.path)?;
            }
        }
    }
    Ok(())
}

fn write_text(path: &Path, text: &str, relative: &str) -> Result<(), ServiceError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| {
                ServiceError::BadRequest(format!("create directory for {relative}: {err}"))
            })?;
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let tmp = path.with_file_name(format!(".{name}.robi-tmp"));
    std::fs::write(&tmp, text.as_bytes())
        .map_err(|err| ServiceError::BadRequest(format!("write {relative}: {err}")))?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ServiceError::BadRequest(format!("write {relative}: {err}")));
    }
    Ok(())
}

fn diff_baseline(root: &Path, baseline: &FileBaseline) -> Result<Option<FileDiff>, ServiceError> {
    let Some((current, exists)) = read_current(root, baseline)? else {
        return Ok(None);
    };
    let mut file_diff = diff(&baseline.path, &baseline.baseline, &current);
    if current.is_empty() && !exists {
        file_diff.status = FileStatus::Deleted;
    }
    Ok(Some(file_diff))
}

/// `None` when a created file is already gone. `exists` is false when the
/// path is missing and the baseline is a deletion.
fn read_current(
    root: &Path,
    baseline: &FileBaseline,
) -> Result<Option<(String, bool)>, ServiceError> {
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

    let exists = absolute.exists();
    Ok(Some((current.unwrap_or_default(), exists)))
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

#[cfg(test)]
mod tests {
    use robi_core::ids::SessionId;
    use uuid::Uuid;

    use crate::domain::file_change::memory::MemoryFileChangeRepository;

    use super::*;

    #[tokio::test]
    async fn reject_one_hunk_restores_those_lines() {
        let root = std::env::temp_dir().join(format!("robi-decide-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "one\nthree\nfour\n").unwrap();
        let repo = MemoryFileChangeRepository::new();
        let session = SessionId::from_uuid(Uuid::now_v7());
        repo.record_baseline(session, "a.txt", "one\ntwo\nfour\n", false)
            .await
            .unwrap();
        let review = review_for_session(&repo, &root, session).await.unwrap();
        let id = review[0].hunks[0].id.clone();
        decide_review(
            &repo,
            &root,
            session,
            "a.txt",
            Some(&id),
            ReviewDecision::Reject,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "one\ntwo\nfour\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn approve_one_hunk_keeps_the_file_and_updates_the_baseline() {
        let root = std::env::temp_dir().join(format!("robi-decide-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "one\nthree\n").unwrap();
        let repo = MemoryFileChangeRepository::new();
        let session = SessionId::from_uuid(Uuid::now_v7());
        repo.record_baseline(session, "a.txt", "one\ntwo\n", false)
            .await
            .unwrap();
        let review = review_for_session(&repo, &root, session).await.unwrap();
        let id = review[0].hunks[0].id.clone();
        decide_review(
            &repo,
            &root,
            session,
            "a.txt",
            Some(&id),
            ReviewDecision::Approve,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "one\nthree\n"
        );
        let stored = repo.get_baseline(session, "a.txt").await.unwrap().unwrap();
        assert_eq!(stored.baseline, "one\nthree\n");
        let _ = std::fs::remove_dir_all(&root);
    }
}

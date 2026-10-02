//! Line diff between a session baseline and the file on disk.

use std::fmt::Write as _;

use serde::Serialize;
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};

/// One changed region. Line starts are 0-based indexes into the split lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub id: String,
    pub old_start: usize,
    pub new_start: usize,
    pub old: Vec<String>,
    pub new: Vec<String>,
}

/// What happened to one path, from the baseline's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
}

/// One path's change. `patch` is a unified diff. `hunks` are the regions inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub patch: String,
    pub additions: u32,
    pub deletions: u32,
    pub status: FileStatus,
    pub hunks: Vec<Hunk>,
}

/// Diff `baseline` against `current`. Both are the full file text.
pub fn diff(path: &str, baseline: &str, current: &str) -> FileDiff {
    let text = TextDiff::from_lines(baseline, current);
    let mut patch = String::new();
    let _ = write!(
        patch,
        "{}",
        text.unified_diff()
            .header(&format!("a/{path}"), &format!("b/{path}"))
    );

    let mut hunks = Vec::new();
    let mut additions = 0u32;
    let mut deletions = 0u32;
    let mut old_index = 0usize;
    let mut new_index = 0usize;
    let mut open: Option<OpenHunk> = None;

    for change in text.iter_all_changes() {
        let line = strip_ending(change.value());
        match change.tag() {
            ChangeTag::Equal => {
                if let Some(hunk) = open.take() {
                    hunks.push(hunk.finish());
                }
                old_index += 1;
                new_index += 1;
            }
            ChangeTag::Delete => {
                let hunk = open.get_or_insert_with(|| OpenHunk::new(old_index, new_index));
                hunk.old.push(line);
                deletions += 1;
                old_index += 1;
            }
            ChangeTag::Insert => {
                let hunk = open.get_or_insert_with(|| OpenHunk::new(old_index, new_index));
                hunk.new.push(line);
                additions += 1;
                new_index += 1;
            }
        }
    }
    if let Some(hunk) = open.take() {
        hunks.push(hunk.finish());
    }

    let status = if baseline.is_empty() && !current.is_empty() {
        FileStatus::Added
    } else if !baseline.is_empty() && current.is_empty() {
        FileStatus::Deleted
    } else {
        FileStatus::Modified
    };

    FileDiff {
        path: path.to_owned(),
        patch,
        additions,
        deletions,
        status,
        hunks,
    }
}

/// Replace `hunk`'s current lines with its baseline lines.
///
/// Fails when those current lines are not in `current` at `new_start`.
pub fn reject(current: &str, hunk: &Hunk) -> Result<String, String> {
    let lines = split_lines(current);
    let end = hunk
        .new_start
        .checked_add(hunk.new.len())
        .ok_or_else(|| "hunk no longer matches".to_owned())?;
    if end > lines.len() {
        return Err("hunk no longer matches".to_owned());
    }
    for (offset, line) in hunk.new.iter().enumerate() {
        if lines[hunk.new_start + offset] != *line {
            return Err("hunk no longer matches".to_owned());
        }
    }
    let mut next = Vec::with_capacity(lines.len() - hunk.new.len() + hunk.old.len());
    next.extend_from_slice(&lines[..hunk.new_start]);
    next.extend(hunk.old.iter().cloned());
    next.extend_from_slice(&lines[end..]);
    Ok(join_lines(&next, current))
}

struct OpenHunk {
    old_start: usize,
    new_start: usize,
    old: Vec<String>,
    new: Vec<String>,
}

impl OpenHunk {
    fn new(old_start: usize, new_start: usize) -> Self {
        Self {
            old_start,
            new_start,
            old: Vec::new(),
            new: Vec::new(),
        }
    }

    fn finish(self) -> Hunk {
        Hunk {
            id: hunk_id(self.old_start, &self.old, &self.new),
            old_start: self.old_start,
            new_start: self.new_start,
            old: self.old,
            new: self.new,
        }
    }
}

fn hunk_id(old_start: usize, old_lines: &[String], new_lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(old_start.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(old_lines.join("\n").as_bytes());
    hasher.update(b"\n");
    hasher.update(new_lines.join("\n").as_bytes());
    let digest = hasher.finalize();
    hex(&digest[..8])
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

/// Lines without their terminator. A trailing newline does not add an empty line.
pub fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find('\n') {
        let (line, _) = rest.split_at(index);
        lines.push(strip_ending(line));
        rest = &rest[index + 1..];
    }
    if !rest.is_empty() {
        lines.push(strip_ending(rest));
    }
    lines
}

fn strip_ending(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_owned()
}

fn join_lines(lines: &[String], prototype: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let ending = if prototype.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = lines.join(ending);
    if prototype.is_empty() || prototype.ends_with('\n') {
        out.push_str(ending);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_is_one_hunk() {
        let diff = diff("src/a.rs", "one\ntwo\n", "one\nthree\n");
        assert_eq!(diff.status, FileStatus::Modified);
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 1);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].old, vec!["two".to_owned()]);
        assert_eq!(diff.hunks[0].new, vec!["three".to_owned()]);
        assert_eq!(diff.hunks[0].old_start, 1);
        assert_eq!(diff.hunks[0].new_start, 1);
        assert!(diff.patch.contains("-two"));
        assert!(diff.patch.contains("+three"));
    }

    #[test]
    fn reject_restores_the_baseline_lines() {
        let diff = diff("src/a.rs", "one\ntwo\n", "one\nthree\n");
        let restored = reject("one\nthree\n", &diff.hunks[0]).unwrap();
        assert_eq!(restored, "one\ntwo\n");
    }

    #[test]
    fn reject_of_a_deletion_recreates_the_file() {
        let diff = diff("src/a.rs", "one\ntwo\n", "");
        assert_eq!(diff.status, FileStatus::Deleted);
        let restored = reject("", &diff.hunks[0]).unwrap();
        assert_eq!(restored, "one\ntwo\n");
    }

    #[test]
    fn reject_fails_when_the_current_lines_moved() {
        let diff = diff("src/a.rs", "one\ntwo\n", "one\nthree\n");
        let error = reject("one\nfour\n", &diff.hunks[0]).unwrap_err();
        assert_eq!(error, "hunk no longer matches");
    }

    #[test]
    fn an_empty_baseline_is_an_addition() {
        let diff = diff("src/a.rs", "", "fn main() {}\n");
        assert_eq!(diff.status, FileStatus::Added);
        assert_eq!(diff.deletions, 0);
        assert!(diff.additions > 0);
    }

    #[test]
    fn the_hunk_id_is_stable() {
        let first = diff("a", "a\n", "b\n");
        let second = diff("a", "a\n", "b\n");
        assert_eq!(first.hunks[0].id, second.hunks[0].id);
        assert_eq!(first.hunks[0].id.len(), 16);
    }
}

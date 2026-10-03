//! Find files by path substring or glob.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::workspace::{workspace_relative, PathFilter};

use super::context::{denied, ToolContext};

const MAX_FIND_FILES: usize = 50;

pub struct Find {
    ctx: Arc<ToolContext>,
}

impl Find {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Find {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Find files by path substring, or by glob when glob is true. path is the directory to start from and may be workspace-relative, outside the workspace (../), absolute, or start with ~/. A path outside the workspace is refused until the session grants it. The walk follows ripgrep's defaults: .gitignore is honored and hidden files are skipped. Set hidden to include hidden files. Set no_ignore to include gitignored files. Denied paths are still omitted. Results stop at 50 files and say so when truncated."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Substring matched against the workspace-relative path. With glob true, a glob."},
                "path": {"type": "string", "description": "Directory to start from. Workspace-relative, absolute, or ~/. Defaults to the workspace root."},
                "glob": {"type": "boolean", "description": "Match pattern as a glob instead of a substring."},
                "hidden": {"type": "boolean", "description": "Include hidden files. Ripgrep skips them unless this is set."},
                "no_ignore": {"type": "boolean", "description": "Include gitignored files. Ripgrep skips them unless this is set."}
            },
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: FindArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        let filter = self.ctx.filter().await?;
        let resolved = self.ctx.resolve(args.path.as_deref().unwrap_or(""))?;
        if !filter.allows_read(&resolved.relative) {
            return Err(denied(&resolved));
        }
        let matcher = if args.glob {
            Some(
                Glob::new(args.pattern.as_deref().unwrap_or(""))
                    .map_err(|err| ToolError::InvalidArgs(format!("invalid glob: {err}")))?
                    .compile_matcher(),
            )
        } else {
            None
        };
        let mut files = Vec::new();
        let mut truncated = false;
        let pattern = args.pattern.unwrap_or_default();
        walk_find(
            &mut FindWalk {
                root: &self.ctx.root,
                filter: &filter,
                pattern: &pattern,
                glob: matcher.as_ref(),
                hidden: args.hidden,
                no_ignore: args.no_ignore,
                cancel: &run.cancel,
                files: &mut files,
                truncated: &mut truncated,
            },
            &resolved.absolute,
        )?;
        let mut payload = json!({
            "files": files,
            "truncated": truncated,
        });
        if truncated {
            payload["hint"] = json!(
                "Results stopped at 50 files. Narrow the path or pattern and call find again."
            );
        }
        Ok(payload)
    }
}

#[derive(Debug, Deserialize)]
struct FindArgs {
    pattern: Option<String>,
    path: Option<String>,
    #[serde(default)]
    glob: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    no_ignore: bool,
}

struct FindWalk<'a> {
    root: &'a Path,
    filter: &'a PathFilter,
    pattern: &'a str,
    glob: Option<&'a GlobMatcher>,
    hidden: bool,
    no_ignore: bool,
    cancel: &'a CancellationToken,
    files: &'a mut Vec<String>,
    truncated: &'a mut bool,
}

fn walk_find(walk: &mut FindWalk<'_>, start: &Path) -> Result<(), ToolError> {
    let mut builder = WalkBuilder::new(start);
    builder
        .hidden(!walk.hidden)
        .follow_links(false)
        .git_ignore(!walk.no_ignore)
        .git_global(!walk.no_ignore)
        .git_exclude(!walk.no_ignore)
        .ignore(!walk.no_ignore)
        .parents(!walk.no_ignore);
    let root = walk.root.to_path_buf();
    let filter = walk.filter.clone();
    builder.filter_entry(move |entry| {
        let relative = relative_slash(&root, entry.path());
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            return !filter.skip_dir(&relative);
        }
        filter.allows_read(&relative)
    });

    for entry in builder.build() {
        if walk.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if *walk.truncated {
            return Ok(());
        }
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        push_file(
            walk.root,
            entry.path(),
            walk.pattern,
            walk.glob,
            walk.files,
            walk.truncated,
        );
    }
    Ok(())
}

fn push_file(
    root: &Path,
    path: &Path,
    pattern: &str,
    glob: Option<&GlobMatcher>,
    files: &mut Vec<String>,
    truncated: &mut bool,
) {
    if files.len() >= MAX_FIND_FILES {
        *truncated = true;
        return;
    }
    let relative = relative_slash(root, path);
    let matched = match glob {
        Some(glob) => glob.is_match(&relative),
        None => pattern.is_empty() || relative.contains(pattern),
    };
    if matched {
        files.push(relative);
    }
}

fn relative_slash(root: &Path, path: &Path) -> String {
    workspace_relative(root, path)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::domain::chat_session::model::PathRules;

    fn workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "robi-find-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "notes.txt\n").unwrap();
        fs::write(root.join("notes.txt"), "gitignored\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".env"), "TOKEN=1\n").unwrap();
        root
    }

    fn listed(root: &Path, hidden: bool, no_ignore: bool) -> Vec<String> {
        let filter = PathFilter::compile(&PathRules::default()).unwrap();
        let mut files = Vec::new();
        let mut truncated = false;
        walk_find(
            &mut FindWalk {
                root,
                filter: &filter,
                pattern: "",
                glob: None,
                hidden,
                no_ignore,
                cancel: &CancellationToken::new(),
                files: &mut files,
                truncated: &mut truncated,
            },
            root,
        )
        .unwrap();
        files
    }

    #[test]
    fn find_skips_gitignored_and_hidden_files_by_default() {
        let root = workspace();
        let files = listed(&root, false, false);
        assert!(files.iter().any(|path| path == "src/main.rs"), "{files:?}");
        assert!(!files.iter().any(|path| path == "notes.txt"), "{files:?}");
        assert!(!files.iter().any(|path| path == ".env"), "{files:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_ignore_includes_gitignored_files_and_still_skips_env() {
        let root = workspace();
        let files = listed(&root, true, true);
        assert!(files.iter().any(|path| path == "notes.txt"), "{files:?}");
        assert!(!files.iter().any(|path| path == ".env"), "{files:?}");
        let _ = fs::remove_dir_all(&root);
    }
}

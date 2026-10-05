//! Ask the user to allow a path for the rest of this session.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::chat_session::model::UpdateChatSessionCommand;

use super::context::{display_path, ToolContext};

pub struct Grant {
    ctx: Arc<ToolContext>,
}

impl Grant {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Grant {
    fn name(&self) -> &str {
        "grant"
    }

    fn description(&self) -> &str {
        "Ask the user to allow a path that the session currently refuses, including a path outside the workspace such as ../gopi. path is the refused path. access is read or write. A write grant also allows reads of that path. The call always waits for the user. On approval it saves an allow for this session only. Grant the path that was refused; a parent allow does not open a more specific deny. Granting a directory allows its children. A grant of ../gopi does not open ../gopi/.git or a sibling directory."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to allow. Workspace-relative, ../ outside the workspace, absolute, or ~/."},
                "access": {"type": "string", "enum": ["read", "write"], "description": "read or write. write also allows reads."}
            },
            "required": ["path", "access"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::NeedsApproval
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: GrantArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.path.is_empty() {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        let access = match args.access.as_str() {
            "read" => Access::Read,
            "write" => Access::Write,
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "access must be read or write, got {other}"
                )));
            }
        };
        let resolved = self.ctx.resolve(&args.path)?;
        let absolute = resolved.absolute.clone();
        let directory = crate::agent::blocking::call(move || {
            std::fs::metadata(&absolute)
                .map(|meta| meta.is_dir())
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        let pattern = allow_pattern(&resolved.relative, directory);
        let session = self
            .ctx
            .sessions
            .get_chat_session(self.ctx.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let mut command = UpdateChatSessionCommand {
            id: self.ctx.session_id,
            title: None,
            allow_read: None,
            allow_write: None,
            deny_read: None,
            deny_write: None,
            allow_hosts: None,
            mcp_allows: None,
            mode: None,
            model_config: None,
        };
        match access {
            Access::Read => {
                let mut patterns = session.path_rules.allow_read;
                push_unique(&mut patterns, pattern.clone());
                command.allow_read = Some(patterns);
            }
            Access::Write => {
                let mut patterns = session.path_rules.allow_write;
                push_unique(&mut patterns, pattern.clone());
                command.allow_write = Some(patterns);
            }
        }
        self.ctx
            .sessions
            .update_chat_session(command)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        Ok(json!({
            "path": display_path(&resolved),
            "access": args.access,
            "pattern": pattern,
        }))
    }
}

enum Access {
    Read,
    Write,
}

#[derive(Debug, Deserialize)]
struct GrantArgs {
    path: String,
    access: String,
}

/// A pattern specific enough to beat a broader deny of the same path.
///
/// A directory matches itself and its children. The workspace root matches
/// every relative path, and still loses to a deny that names more literals.
/// Allow patterns for a newline-separated settings value.
///
/// Blank lines are skipped. A leading `~` or `~/` is the home directory.
/// A path that is a file is an exact allow. Any other path, including one
/// that does not exist yet, is that directory and its children.
pub fn allows_from_setting(workspace: &Path, home: &Path, value: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    for line in value.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(resolved) = crate::agent::workspace::resolve_path(workspace, line, home) else {
            continue;
        };
        let directory = !resolved.absolute.is_file();
        push_unique(&mut patterns, allow_pattern(&resolved.relative, directory));
    }
    patterns
}

pub fn allow_pattern(relative: &str, directory: bool) -> String {
    if relative.is_empty() {
        return r"^.*$".to_owned();
    }
    let escaped = regex::escape(relative);
    if directory {
        format!(r"^{escaped}(/|$)")
    } else {
        format!(r"^{escaped}$")
    }
}

fn push_unique(patterns: &mut Vec<String>, pattern: String) {
    if !patterns.iter().any(|existing| existing == &pattern) {
        patterns.push(pattern);
    }
}

#[cfg(test)]
mod tests {
    use super::{allow_pattern, allows_from_setting};

    #[test]
    fn a_file_pattern_is_an_exact_path() {
        assert_eq!(allow_pattern("src/.env", false), r"^src/\.env$");
    }

    #[test]
    fn a_directory_pattern_includes_children() {
        assert_eq!(allow_pattern("gopi/.git", true), r"^gopi/\.git(/|$)");
    }

    #[test]
    fn a_home_path_in_settings_becomes_an_absolute_allow() {
        let root = std::env::temp_dir().join(format!("robi-allow-{}", std::process::id()));
        let workspace = root.join("ws");
        let home = root.join("home");
        let pnpm = home.join("pnpm");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&pnpm).unwrap();
        let workspace = std::fs::canonicalize(&workspace).unwrap();
        let home = std::fs::canonicalize(&home).unwrap();
        let patterns = allows_from_setting(&workspace, &home, "\n~/pnpm\n\n  ~/pnpm \n");
        assert_eq!(patterns.len(), 1);
        let resolved = crate::agent::workspace::resolve_path(&workspace, "~/pnpm", &home).unwrap();
        assert!(resolved.absolute.starts_with(&home));
        assert_eq!(patterns[0], allow_pattern(&resolved.relative, true));
        assert!(patterns[0].contains("pnpm"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_outside_the_workspace_keeps_the_parent_prefix() {
        assert_eq!(allow_pattern("../gopi", true), r"^\.\./gopi(/|$)");
    }

    #[test]
    fn the_workspace_root_is_a_wildcard_that_names_nothing() {
        assert_eq!(allow_pattern("", true), r"^.*$");
    }
}

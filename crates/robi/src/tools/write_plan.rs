//! Create or update a markdown plan under `.robi/plans`.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::change::{atomic_write, ensure_parent, lock_path, read_text, record_baseline};
use super::context::{display_path, ToolContext};

const PLAN_DIR: &str = ".robi/plans";
const IGNORE_LINE: &str = ".robi/plans";
const MAX_TODOS: usize = 20;

pub struct WritePlan {
    ctx: Arc<ToolContext>,
    /// Plan mode may create a file. Agent mode only overwrites one that exists.
    create: bool,
}

impl WritePlan {
    pub fn new(ctx: Arc<ToolContext>, create: bool) -> Self {
        Self { ctx, create }
    }
}

#[derive(Deserialize)]
struct Todo {
    id: String,
    content: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct WritePlanArgs {
    #[serde(default)]
    plan_name: String,
    body: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    todos: Vec<Todo>,
}

#[async_trait]
impl Tool for WritePlan {
    fn name(&self) -> &str {
        "write_plan"
    }

    fn description(&self) -> &str {
        if self.create {
            "Create or update a markdown plan under .robi/plans. Pass path to overwrite an existing plan. Omit path to create .robi/plans/<plan_name>-<uuid>.md. Pass todos for the implementation steps. Each todo has an id, content, and status of pending, in_progress, completed, or canceled. The body is the markdown plan and does not include the todo list. Saving a plan adds .robi/plans to the workspace .gitignore when that file already exists."
        } else {
            "Update an existing markdown plan under .robi/plans. Pass path to that file, the full body, and the todos. This does not create a new plan."
        }
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan_name": {
                    "type": "string",
                    "description": "Short name for a new plan file. Required when path is omitted."
                },
                "body": {
                    "type": "string",
                    "description": "Full markdown plan, without the todo list."
                },
                "path": {
                    "type": "string",
                    "description": "Existing markdown plan under .robi/plans to overwrite."
                },
                "todos": {
                    "type": "array",
                    "description": "Implementation steps written as frontmatter.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Short stable id."},
                            "content": {"type": "string", "description": "What the step is."},
                            "status": {
                                "type": "string",
                                "description": "pending, in_progress, completed, or canceled. Defaults to pending."
                            }
                        },
                        "required": ["id", "content"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["body"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: WritePlanArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.body.trim().is_empty() {
            return Err(ToolError::InvalidArgs("body is required".into()));
        }
        let todos = normalize_todos(&args.todos)?;
        let destination = self.destination(&args)?;
        let resolved = self.ctx.resolve(&destination.relative)?;
        if !plan_file(&resolved.relative) {
            return Err(ToolError::Failed(
                "path must be a markdown file under .robi/plans".into(),
            ));
        }
        let filter = self.ctx.filter().await?;
        if !filter.allows_write(&resolved.relative) {
            return Err(ToolError::Failed(format!(
                "path is not allowed: {}",
                display_path(&resolved)
            )));
        }
        let _guard = lock_path(&resolved.absolute).await;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if std::fs::metadata(&resolved.absolute)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Err(ToolError::Failed("path is a directory".into()));
        }
        let existing = read_text(&resolved.absolute)?;
        if !self.create && existing.is_none() {
            return Err(ToolError::Failed("plan file does not exist".into()));
        }
        if destination.created && existing.is_some() {
            return Err(ToolError::Failed("plan file already exists".into()));
        }
        let existed = existing.is_some();
        let before = existing.unwrap_or_default();
        let relative = display_path(&resolved);
        let text = format_plan(&todos, &args.body);
        record_baseline(
            self.ctx.file_changes.as_ref(),
            self.ctx.session_id,
            &relative,
            &before,
            !existed,
        )
        .await?;
        ensure_parent(&resolved.absolute)?;
        atomic_write(&resolved.absolute, &text)?;
        let gitignore_updated = append_plan_ignore(&self.ctx.root)?;
        Ok(json!({
            "path": relative,
            "status": if existed { "updated" } else { "created" },
            "gitignore_updated": gitignore_updated,
        }))
    }
}

struct Destination {
    relative: String,
    created: bool,
}

impl WritePlan {
    fn destination(&self, args: &WritePlanArgs) -> Result<Destination, ToolError> {
        let path = args.path.trim();
        if !path.is_empty() {
            if !self.create {
                return Ok(Destination {
                    relative: path.to_owned(),
                    created: false,
                });
            }
            let resolved = self.ctx.resolve(path)?;
            if !plan_file(&resolved.relative) {
                return Err(ToolError::Failed(
                    "path must be a markdown file under .robi/plans".into(),
                ));
            }
            let exists = std::fs::metadata(&resolved.absolute)
                .map(|meta| meta.is_file())
                .unwrap_or(false);
            if !exists {
                return Err(ToolError::Failed("plan file does not exist".into()));
            }
            return Ok(Destination {
                relative: resolved.relative,
                created: false,
            });
        }
        if !self.create {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        let slug = plan_slug(&args.plan_name)?;
        let name = format!("{slug}-{}.md", Uuid::now_v7().simple());
        Ok(Destination {
            relative: format!("{PLAN_DIR}/{name}"),
            created: true,
        })
    }
}

fn plan_file(relative: &str) -> bool {
    let Some(rest) = relative.strip_prefix(".robi/plans/") else {
        return false;
    };
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('\\')
        && rest.ends_with(".md")
        && rest != ".md"
}

fn plan_slug(name: &str) -> Result<String, ToolError> {
    let mut slug = String::new();
    let mut dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !slug.is_empty() && !dash {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return Err(ToolError::InvalidArgs("plan_name is required".into()));
    }
    Ok(slug.to_owned())
}

fn normalize_todos(todos: &[Todo]) -> Result<Vec<Todo>, ToolError> {
    if todos.len() > MAX_TODOS {
        return Err(ToolError::InvalidArgs(format!(
            "task list is limited to {MAX_TODOS} items"
        )));
    }
    let mut seen = Vec::new();
    let mut in_progress = 0;
    let mut out = Vec::with_capacity(todos.len());
    for todo in todos {
        let id = todo.id.trim();
        let content = todo.content.trim();
        let mut status = todo.status.trim().to_owned();
        if id.is_empty() || id.chars().any(char::is_whitespace) || id.chars().count() > 64 {
            return Err(ToolError::InvalidArgs(
                "task id must be a short slug".into(),
            ));
        }
        if content.is_empty() || content.chars().count() > 200 {
            return Err(ToolError::InvalidArgs("task content is required".into()));
        }
        if status.is_empty() {
            status = "pending".to_owned();
        }
        if !matches!(
            status.as_str(),
            "pending" | "in_progress" | "completed" | "canceled"
        ) {
            return Err(ToolError::InvalidArgs(
                "task status must be pending, in_progress, completed, or canceled".into(),
            ));
        }
        if seen.iter().any(|seen_id: &String| seen_id == id) {
            return Err(ToolError::InvalidArgs(format!("duplicate task {id}")));
        }
        if status == "in_progress" {
            in_progress += 1;
        }
        seen.push(id.to_owned());
        out.push(Todo {
            id: id.to_owned(),
            content: content.to_owned(),
            status,
        });
    }
    if in_progress > 1 {
        return Err(ToolError::InvalidArgs(
            "only one task can be in progress".into(),
        ));
    }
    Ok(out)
}

fn format_plan(todos: &[Todo], body: &str) -> String {
    if todos.is_empty() {
        return body.to_owned();
    }
    let mut text = String::from("---\ntodos:\n");
    for todo in todos {
        text.push_str("  - id: ");
        text.push_str(&yaml_quote(&todo.id));
        text.push_str("\n    content: ");
        text.push_str(&yaml_quote(&todo.content));
        text.push_str("\n    status: ");
        text.push_str(&yaml_quote(&todo.status));
        text.push('\n');
    }
    text.push_str("---\n");
    text.push_str(body);
    text
}

fn yaml_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn append_plan_ignore(root: &std::path::Path) -> Result<bool, ToolError> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let path = root.join(".gitignore");
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(ToolError::Failed(format!("read .gitignore: {err}"))),
    };
    if body.lines().any(|line| line.trim() == IGNORE_LINE) {
        return Ok(false);
    }
    let mut next = body;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(IGNORE_LINE);
    next.push('\n');
    std::fs::write(&path, next)
        .map_err(|err| ToolError::Failed(format!("update .gitignore: {err}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::tools::apply_tests::harness;
    use tokio_util::sync::CancellationToken;

    fn run() -> ToolRun {
        ToolRun::new(CancellationToken::new())
    }

    #[tokio::test]
    async fn write_plan_creates_under_robi_plans_and_refuses_a_path_outside() {
        let harness = harness().await;
        std::fs::write(harness.ctx.root.join(".gitignore"), "target\n").unwrap();
        let tool = WritePlan::new(Arc::clone(&harness.ctx), true);
        let created = tool
            .execute(
                json!({
                    "plan_name": "Ship modes",
                    "body": "Add ask, plan, and agent.\n",
                    "todos": [{"id": "modes", "content": "Add the mode registry", "status": "pending"}]
                }),
                run(),
            )
            .await
            .unwrap();
        let path = created["path"].as_str().unwrap();
        assert!(path.starts_with(".robi/plans/"));
        assert!(path.ends_with(".md"));
        assert_eq!(created["status"], "created");
        assert_eq!(created["gitignore_updated"], true);
        let file = std::fs::read_to_string(harness.ctx.root.join(path)).unwrap();
        assert!(file.contains("id: \"modes\""));
        assert!(file.contains("Add ask, plan, and agent."));
        let ignore = std::fs::read_to_string(harness.ctx.root.join(".gitignore")).unwrap();
        assert!(ignore.contains(".robi/plans\n"));

        let outside = tool
            .execute(json!({"path": "src/main.rs", "body": "nope\n"}), run())
            .await
            .unwrap_err();
        assert!(outside.to_string().contains(".robi/plans"));
    }

    #[tokio::test]
    async fn agent_mode_refuses_a_missing_path() {
        let harness = harness().await;
        let tool = WritePlan::new(Arc::clone(&harness.ctx), false);
        let error = tool
            .execute(json!({"body": "A plan.\n"}), run())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("path is required"));

        let missing = tool
            .execute(
                json!({"path": ".robi/plans/missing.md", "body": "A plan.\n"}),
                run(),
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("does not exist"));
    }
}

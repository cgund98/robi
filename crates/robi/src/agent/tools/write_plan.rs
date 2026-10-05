//! Create or update a markdown plan under `~/.robi/plans/<session_id>`.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::change::{atomic_write, ensure_parent, lock_path, read_text};
use super::context::ToolContext;
use super::plan_file::{format_plan, is_session_plan_file, normalize_todos, plan_argument, Todo};
use crate::agent::workspace::user_home;

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
            "Create or update a markdown plan for this session under ~/.robi/plans/<session_id>. Pass path to overwrite an existing plan. Omit path to create ~/.robi/plans/<session_id>/<plan_name>-<uuid>.md. Pass todos for the implementation steps. Each todo has an id, content of at most 500 characters, and status of pending, in_progress, completed, or canceled. The body is the markdown plan and does not include the todo list."
        } else {
            "Update an existing markdown plan for this session under ~/.robi/plans/<session_id>. Pass path to that file, the full body, and the todos. An omitted list removes the frontmatter. Change a status with todos instead. This does not create a new plan."
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
                    "description": "Existing markdown plan for this session. A path under ~/.robi/plans/<session_id>."
                },
                "todos": {
                    "type": "array",
                    "description": "Implementation steps written as frontmatter.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string", "description": "Short stable id."},
                            "content": {"type": "string", "description": "What the step is. At most 500 characters."},
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
        let home = user_home().map_err(|err| ToolError::Failed(err.to_string()))?;
        let destination = self.destination(&args, &home).await?;
        let resolved = self.ctx.resolve(&destination.argument)?;
        if !is_session_plan_file(&resolved.absolute, &home, self.ctx.session_id) {
            return Err(ToolError::Failed(
                "path must be a markdown file under ~/.robi/plans for this session".into(),
            ));
        }
        let _guard = lock_path(&resolved.absolute).await;
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let absolute = resolved.absolute.clone();
        let (is_dir, existing) = crate::agent::blocking::call(move || {
            let is_dir = std::fs::metadata(&absolute)
                .map(|meta| meta.is_dir())
                .unwrap_or(false);
            let existing = read_text(&absolute)?;
            Ok::<_, ToolError>((is_dir, existing))
        })
        .await
        .map_err(ToolError::Failed)??;
        if is_dir {
            return Err(ToolError::Failed("path is a directory".into()));
        }
        if !self.create && existing.is_none() {
            return Err(ToolError::Failed("plan file does not exist".into()));
        }
        if destination.created && existing.is_some() {
            return Err(ToolError::Failed("plan file already exists".into()));
        }
        let existed = existing.is_some();
        let text = format_plan(&todos, &args.body);
        let stored = plan_argument(
            self.ctx.session_id,
            resolved
                .absolute
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ToolError::Failed("plan file name is missing".into()))?,
        );
        let absolute = resolved.absolute.clone();
        crate::agent::blocking::call(move || {
            ensure_parent(&absolute)?;
            atomic_write(&absolute, &text)
        })
        .await
        .map_err(ToolError::Failed)??;
        self.ctx.remember_plan(&stored).await?;
        Ok(json!({
            "path": stored,
            "status": if existed { "updated" } else { "created" },
        }))
    }
}

struct Destination {
    argument: String,
    created: bool,
}

impl WritePlan {
    async fn destination(
        &self,
        args: &WritePlanArgs,
        home: &std::path::Path,
    ) -> Result<Destination, ToolError> {
        let path = args.path.trim();
        if !path.is_empty() {
            if !self.create {
                return Ok(Destination {
                    argument: path.to_owned(),
                    created: false,
                });
            }
            let resolved = self.ctx.resolve(path)?;
            if !is_session_plan_file(&resolved.absolute, home, self.ctx.session_id) {
                return Err(ToolError::Failed(
                    "path must be a markdown file under ~/.robi/plans for this session".into(),
                ));
            }
            let absolute = resolved.absolute.clone();
            let exists = crate::agent::blocking::call(move || {
                std::fs::metadata(&absolute)
                    .map(|meta| meta.is_file())
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);
            if !exists {
                return Err(ToolError::Failed("plan file does not exist".into()));
            }
            return Ok(Destination {
                argument: path.to_owned(),
                created: false,
            });
        }
        if !self.create {
            return Err(ToolError::InvalidArgs("path is required".into()));
        }
        let slug = plan_slug(&args.plan_name)?;
        let name = format!("{slug}-{}.md", Uuid::now_v7().simple());
        Ok(Destination {
            argument: plan_argument(self.ctx.session_id, &name),
            created: true,
        })
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent::tools::apply_tests::harness;
    use tokio_util::sync::CancellationToken;

    fn run() -> ToolRun {
        ToolRun::new(CancellationToken::new())
    }

    #[tokio::test]
    async fn write_plan_creates_under_the_session_plan_dir_and_refuses_a_path_outside() {
        let harness = harness().await;
        let _cleanup = remove_session_plans(harness.session_id);
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
        assert!(path.starts_with(&format!("~/.robi/plans/{}/", harness.session_id)));
        assert!(path.ends_with(".md"));
        assert_eq!(created["status"], "created");
        let file = std::fs::read_to_string(expand_home(path)).unwrap();
        assert!(file.contains("id: \"modes\""));
        assert!(file.contains("Add ask, plan, and agent."));
        let session = harness
            .ctx
            .sessions
            .get_chat_session(harness.session_id)
            .await
            .unwrap();
        assert_eq!(session.plan_path.as_deref(), Some(path));
        let resolved = harness.ctx.resolve(path).unwrap();
        let filter = harness.ctx.filter().await.unwrap();
        assert!(filter.allows_read(&resolved.relative));
        assert!(!filter.allows_write(&resolved.relative));

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
                json!({
                    "path": format!("~/.robi/plans/{}/missing.md", harness.session_id),
                    "body": "A plan.\n"
                }),
                run(),
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn a_missing_task_is_named_and_a_long_task_names_the_limit() {
        let harness = harness().await;
        let _cleanup = remove_session_plans(harness.session_id);
        let tool = WritePlan::new(Arc::clone(&harness.ctx), true);
        let empty = tool
            .execute(
                json!({
                    "plan_name": "Docs",
                    "body": "Rewrite the docs.\n",
                    "todos": [
                        {"id": "outline", "content": "List the pages"},
                        {"id": "rewrite", "content": "  "}
                    ]
                }),
                run(),
            )
            .await
            .unwrap_err();
        assert!(empty
            .to_string()
            .contains("task rewrite content is required"));

        let long = tool
            .execute(
                json!({
                    "plan_name": "Docs",
                    "body": "Rewrite the docs.\n",
                    "todos": [{"id": "rewrite", "content": "x".repeat(501)}]
                }),
                run(),
            )
            .await
            .unwrap_err();
        let message = long.to_string();
        assert!(message.contains("task rewrite content is limited to 500 characters"));
        assert!(!message.contains("task content is required"));
    }

    fn expand_home(path: &str) -> std::path::PathBuf {
        crate::agent::workspace::user_home()
            .unwrap()
            .join(path.strip_prefix("~/").unwrap())
    }

    fn remove_session_plans(session_id: robi_core::ids::SessionId) -> RemoveSessionPlans {
        let dir = crate::agent::workspace::user_home()
            .unwrap()
            .join(".robi/plans")
            .join(session_id.to_string());
        RemoveSessionPlans(dir)
    }

    struct RemoveSessionPlans(std::path::PathBuf);

    impl Drop for RemoveSessionPlans {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

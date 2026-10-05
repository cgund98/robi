//! Patch the todo frontmatter of an existing plan.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use super::change::{atomic_write, lock_path, read_text};
use super::context::ToolContext;
use super::plan_file::{
    format_plan, is_session_plan_file, normalize_todos, plan_argument, split_plan, Todo,
};
use crate::agent::workspace::user_home;

pub struct Todos {
    ctx: Arc<ToolContext>,
}

impl Todos {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Deserialize)]
struct TodoUpdate {
    id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct TodosArgs {
    path: String,
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    update: Vec<TodoUpdate>,
    #[serde(default)]
    add: Vec<Todo>,
}

#[async_trait]
impl Tool for Todos {
    fn name(&self) -> &str {
        "todos"
    }

    fn description(&self) -> &str {
        "Patch the checklist on an existing plan for this session under ~/.robi/plans/<session_id>. path is that file. clear drops every item. remove drops ids. update changes the status or content of existing ids. add appends items, and an id that already exists is updated. A call can do more than one, applied in that order. Ids the call does not mention stay. clear plus add replaces the list. At most one item is in progress. The markdown body is left unchanged. If a todos block is already loaded, do not add those ids again. Leave unchanged ids out of the call."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Existing markdown plan for this session under ~/.robi/plans/<session_id>."
                },
                "clear": {
                    "type": "boolean",
                    "description": "Drop every current item. Combine with add to replace the list."
                },
                "remove": {
                    "type": "array",
                    "description": "Ids to drop. Ids not listed stay.",
                    "items": {"type": "string"}
                },
                "update": {
                    "type": "array",
                    "description": "Change the status or content of existing ids.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "content": {"type": "string"},
                            "status": {
                                "type": "string",
                                "description": "pending, in_progress, completed, or canceled."
                            }
                        },
                        "required": ["id"],
                        "additionalProperties": false
                    }
                },
                "add": {
                    "type": "array",
                    "description": "Items to append. An id that already exists is updated.",
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
            "required": ["path"],
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
        let args: TodosArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if !args.clear && args.remove.is_empty() && args.update.is_empty() && args.add.is_empty() {
            return Err(ToolError::InvalidArgs(
                "add, update, remove, or clear a task".into(),
            ));
        }
        let home = user_home().map_err(|err| ToolError::Failed(err.to_string()))?;
        let resolved = self.ctx.resolve(args.path.trim())?;
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
        let Some(before) = crate::agent::blocking::call(move || read_text(&absolute))
            .await
            .map_err(ToolError::Failed)??
        else {
            return Err(ToolError::Failed("plan file does not exist".into()));
        };
        let (items, body) = split_plan(&before)?;
        let next = patch_todos(&items, &args)?;
        let text = format_plan(&next, &body);
        let stored = plan_argument(
            self.ctx.session_id,
            resolved
                .absolute
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ToolError::Failed("plan file name is missing".into()))?,
        );
        let absolute = resolved.absolute.clone();
        crate::agent::blocking::call(move || atomic_write(&absolute, &text))
            .await
            .map_err(ToolError::Failed)??;
        self.ctx.remember_plan(&stored).await?;
        Ok(json!({
            "path": stored,
            "items": next.iter().map(|todo| json!({
                "id": todo.id,
                "content": todo.content,
                "status": todo.status,
            })).collect::<Vec<_>>(),
        }))
    }
}

fn patch_todos(items: &[Todo], args: &TodosArgs) -> Result<Vec<Todo>, ToolError> {
    unique_ids(args.remove.iter().map(|id| id.trim()))?;
    unique_ids(args.update.iter().map(|update| update.id.trim()))?;
    unique_ids(args.add.iter().map(|todo| todo.id.trim()))?;

    let mut next = if args.clear {
        Vec::new()
    } else {
        items.to_vec()
    };
    for id in &args.remove {
        let id = id.trim();
        let Some(index) = position(&next, id) else {
            return Err(ToolError::InvalidArgs(format!("unknown task {id}")));
        };
        next.remove(index);
    }
    for update in &args.update {
        let id = update.id.trim();
        let content = update.content.trim();
        let status = update.status.trim();
        if content.is_empty() && status.is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "update {id} needs content or status"
            )));
        }
        let Some(index) = position(&next, id) else {
            return Err(ToolError::InvalidArgs(format!("unknown task {id}")));
        };
        if !content.is_empty() {
            next[index].content = content.to_owned();
        }
        if !status.is_empty() {
            next[index].status = status.to_owned();
        }
    }
    for item in &args.add {
        let id = item.id.trim();
        if let Some(index) = position(&next, id) {
            if !item.content.trim().is_empty() {
                next[index].content = item.content.trim().to_owned();
            }
            if !item.status.trim().is_empty() {
                next[index].status = item.status.trim().to_owned();
            }
            continue;
        }
        next.push(item.clone());
    }
    normalize_todos(&next)
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), ToolError> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.iter().any(|seen_id: &String| seen_id == id) {
            return Err(ToolError::InvalidArgs(format!("duplicate task {id}")));
        }
        seen.push(id.to_owned());
    }
    Ok(())
}

fn position(items: &[Todo], id: &str) -> Option<usize> {
    items.iter().position(|todo| todo.id == id)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::agent::tools::apply_tests::harness;
    use crate::agent::tools::write_plan::WritePlan;
    use tokio_util::sync::CancellationToken;

    fn run() -> ToolRun {
        ToolRun::new(CancellationToken::new())
    }

    async fn plan(harness: &crate::agent::tools::apply_tests::Harness, body: &str) -> String {
        let created = WritePlan::new(Arc::clone(&harness.ctx), true)
            .execute(
                json!({
                    "plan_name": "Ship",
                    "body": body,
                    "todos": [
                        {"id": "modes", "content": "Add the mode registry", "status": "pending"},
                        {"id": "wire", "content": "Register the tool", "status": "pending"}
                    ]
                }),
                run(),
            )
            .await
            .unwrap();
        created["path"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn a_status_patch_leaves_the_body_and_updates_frontmatter() {
        let harness = harness().await;
        let _cleanup = remove_session_plans(harness.session_id);
        let body = "# Ship\n\nDo the thing.\n";
        let path = plan(&harness, body).await;
        let tool = Todos::new(Arc::clone(&harness.ctx));
        let result = tool
            .execute(
                json!({
                    "path": path,
                    "update": [{"id": "wire", "status": "in_progress"}]
                }),
                run(),
            )
            .await
            .unwrap();
        assert_eq!(result["path"], path);
        assert_eq!(result["items"][1]["status"], "in_progress");
        assert_eq!(result["items"][0]["status"], "pending");
        let file = std::fs::read_to_string(expand_home(&path)).unwrap();
        let (items, prose) = split_plan(&file).unwrap();
        assert_eq!(prose, body);
        assert_eq!(items[1].status, "in_progress");
        assert!(file.ends_with(body));
        let session = harness
            .ctx
            .sessions
            .get_chat_session(harness.session_id)
            .await
            .unwrap();
        assert_eq!(session.plan_path.as_deref(), Some(path.as_str()));
    }

    #[tokio::test]
    async fn unknown_duplicate_and_two_in_progress_fail() {
        let harness = harness().await;
        let _cleanup = remove_session_plans(harness.session_id);
        let path = plan(&harness, "Body.\n").await;
        let tool = Todos::new(Arc::clone(&harness.ctx));
        let unknown = tool
            .execute(
                json!({"path": path, "update": [{"id": "missing", "status": "completed"}]}),
                run(),
            )
            .await
            .unwrap_err();
        assert!(unknown.to_string().contains("unknown task missing"));

        let duplicate = tool
            .execute(
                json!({
                    "path": path,
                    "add": [
                        {"id": "extra", "content": "One"},
                        {"id": "extra", "content": "Two"}
                    ]
                }),
                run(),
            )
            .await
            .unwrap_err();
        assert!(duplicate.to_string().contains("duplicate task extra"));

        tool.execute(
            json!({"path": path, "update": [{"id": "modes", "status": "in_progress"}]}),
            run(),
        )
        .await
        .unwrap();
        let second = tool
            .execute(
                json!({"path": path, "update": [{"id": "wire", "status": "in_progress"}]}),
                run(),
            )
            .await
            .unwrap_err();
        assert!(second
            .to_string()
            .contains("only one task can be in progress"));
        let file = std::fs::read_to_string(expand_home(&path)).unwrap();
        assert!(file.contains("status: \"pending\""));
        assert!(file.ends_with("Body.\n"));
    }

    #[tokio::test]
    async fn a_path_outside_plans_is_refused() {
        let harness = harness().await;
        let error = Todos::new(Arc::clone(&harness.ctx))
            .execute(
                json!({"path": "src/main.rs", "add": [{"id": "a", "content": "Nope"}]}),
                run(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains(".robi/plans"));
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

    fn expand_home(path: &str) -> std::path::PathBuf {
        crate::agent::workspace::user_home()
            .unwrap()
            .join(path.strip_prefix("~/").unwrap())
    }
}

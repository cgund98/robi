//! Load one skill by id.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use super::context::ToolContext;

pub struct Skill {
    ctx: Arc<ToolContext>,
}

impl Skill {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Skill {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Load a skill by id before following a procedure listed in <skills>. An @id already in the user message is already loaded. name is the skill id. A skill marked manual-only must be requested by the user with @id."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Skill id."}
            },
            "required": ["name"],
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
        let args: SkillArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        let name = args.name.trim();
        if name.is_empty() {
            return Err(ToolError::InvalidArgs("name is required".into()));
        }
        let skills = self.ctx.skills();
        let Some(skill) = skills.into_iter().find(|skill| skill.id == name) else {
            return Err(ToolError::Failed(format!("unknown skill: {name}")));
        };
        if !skill.model_invocable {
            return Err(ToolError::Failed(format!(
                "skill `{name}` is manual-only. Ask the user to mention @{name}. Do not carry the procedure out on your own."
            )));
        }
        let load = skill.load();
        let mut payload = json!({
            "name": load.id,
            "description": load.description,
            "directory": load.directory,
            "files": load.files,
            "body": load.body,
        });
        if skill.files_truncated {
            payload["truncated"] = json!(true);
        }
        Ok(payload)
    }
}

#[derive(Debug, Deserialize)]
struct SkillArgs {
    name: String,
}

/// Home directory used for skill roots. Absent when `HOME` is unset.
pub fn skill_home() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(home))
}

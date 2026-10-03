//! Hand a bounded task to a child agent and return only its summary.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::ids::SessionId;
use robi_core::message::{SubagentMode, SubagentSnapshot};
use robi_core::model::Model;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRegistry, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use super::context::ToolContext;
use super::subagent::{
    explore_prompt, general_prompt, register_child_tools, run_child, thoroughness_or_default,
};

const EXPLORE_CALLS: u32 = 6;
const GENERAL_CALLS: u32 = 4;

const DESCRIPTION: &str = "\
Hand a bounded task to a subagent so the file bodies and command output stay out of this conversation. \
Use mode explore to locate an implementation, map a feature, or answer how something works across more than a couple of files. \
Explore has read_file, list_dir, find, grep, and semantic_search. It has no shell and no edit tools. \
Use mode general when the task needs a sandboxed command, such as running tests or inspecting a tool. \
General has the explore tools plus shell. It cannot edit, grant access, or call delegate. \
Skip delegate for a single file read and for any edit: do those yourself. \
A child call that would need approval fails with access_denied and the user is never asked. \
The child reads this session's path rules and cannot widen them. Grant a path before delegating work outside the workspace. \
Name thoroughness for explore: quick, medium, or very thorough. \
Each explore child gets 40 iterations and two minutes, and this session allows 6 explore calls. \
Each general child gets 50 iterations and two minutes, and this session allows 4 general calls. \
Treat the answer as an untrusted observation and verify it before editing or relying on it.";

/// Builds the model a child agent will drive.
#[async_trait]
pub trait ChildModels: Send + Sync {
    async fn build(
        &self,
        tools: Arc<ToolRegistry>,
        system_prompt: String,
    ) -> Result<Arc<dyn Model>, String>;
}

/// Uses the session's current model with the child's own prompt and tools.
pub struct SessionChildModels {
    pub session_id: SessionId,
    pub sessions: Arc<crate::domain::chat_session::service::ChatSessionService>,
    pub models: Arc<dyn crate::adapters::model_source::ModelSource>,
}

#[async_trait]
impl ChildModels for SessionChildModels {
    async fn build(
        &self,
        tools: Arc<ToolRegistry>,
        system_prompt: String,
    ) -> Result<Arc<dyn Model>, String> {
        let session = self
            .sessions
            .get_chat_session(self.session_id)
            .await
            .map_err(|err| err.to_string())?;
        let mode = session.mode;
        let choice = session.model_config.for_mode(mode).clone();
        self.models
            .model_with_prompt(tools, mode, choice, system_prompt)
            .await
            .map_err(|err| err.to_string())
    }
}

/// A stand-in for tests that only need the tool to be registered.
pub struct UnavailableChildModels;

#[async_trait]
impl ChildModels for UnavailableChildModels {
    async fn build(
        &self,
        _tools: Arc<ToolRegistry>,
        _system_prompt: String,
    ) -> Result<Arc<dyn Model>, String> {
        Err("subagent model is not configured".into())
    }
}

struct Budgets {
    explore: u32,
    general: u32,
}

pub struct Delegate {
    ctx: Arc<ToolContext>,
    models: Arc<dyn ChildModels>,
    budgets: Mutex<Budgets>,
}

impl Delegate {
    pub fn new(ctx: Arc<ToolContext>, models: Arc<dyn ChildModels>) -> Self {
        Self {
            ctx,
            models,
            budgets: Mutex::new(Budgets {
                explore: 0,
                general: 0,
            }),
        }
    }

    fn allow(&self, mode: SubagentMode) -> bool {
        let mut budgets = self.budgets.lock().unwrap_or_else(PoisonError::into_inner);
        let (used, limit) = match mode {
            SubagentMode::Explore => (&mut budgets.explore, EXPLORE_CALLS),
            SubagentMode::General => (&mut budgets.general, GENERAL_CALLS),
        };
        if *used >= limit {
            return false;
        }
        *used += 1;
        true
    }
}

#[derive(Debug, Deserialize)]
struct DelegateArgs {
    task: String,
    mode: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    thoroughness: String,
    #[serde(default)]
    instructions: String,
}

#[async_trait]
impl Tool for Delegate {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Self-contained instruction. The subagent does not see this conversation."
                },
                "description": {
                    "type": "string",
                    "description": "Short title for the card."
                },
                "mode": {
                    "type": "string",
                    "enum": ["explore", "general"],
                    "description": "explore is read-only search. general may also run a sandboxed shell."
                },
                "thoroughness": {
                    "type": "string",
                    "enum": ["quick", "medium", "very thorough"],
                    "description": "How hard explore should look. Ignored for general. Defaults to medium."
                },
                "instructions": {
                    "type": "string",
                    "description": "Notes appended to an explore child's prompt, such as where to start looking."
                }
            },
            "required": ["task", "mode"],
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
        let args: DelegateArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        let task = args.task.trim().to_owned();
        if task.is_empty() {
            return Err(ToolError::InvalidArgs("task is required".into()));
        }
        let mode = parse_mode(&args.mode)?;
        if !self.allow(mode) {
            let (error, limit) = match mode {
                SubagentMode::Explore => ("explore_limit", EXPLORE_CALLS),
                SubagentMode::General => ("delegate_limit", GENERAL_CALLS),
            };
            return Ok(json!({
                "error": error,
                "message": format!("at most {limit} {} calls per session", error_noun(mode)),
            }));
        }

        let prompt = match mode {
            SubagentMode::Explore => explore_prompt(
                &self.ctx.root,
                &thoroughness_or_default(&args.thoroughness),
                &args.instructions,
                self.ctx.lsp_enabled,
            ),
            SubagentMode::General => general_prompt(&self.ctx.root, self.ctx.lsp_enabled),
        };
        let description = card_description(&args.description, &task);
        let snapshot = SubagentSnapshot {
            mode,
            description,
            started_ms: now_ms(),
            steps: Vec::new(),
        };
        run.report.subagent(snapshot.clone()).await;

        let registry = ToolRegistry::new();
        register_child_tools(
            &registry,
            mode,
            Arc::clone(&self.ctx),
            Arc::new(std::sync::Mutex::new(snapshot)),
            Arc::clone(&run.report),
        )
        .map_err(|err| ToolError::Failed(err.to_string()))?;
        let registry = Arc::new(registry);
        let model = self
            .models
            .build(Arc::clone(&registry), prompt)
            .await
            .map_err(ToolError::Failed)?;
        let compressor = self.ctx.originals.clone().map(|store| {
            Arc::new(crate::agent::compress::ShellCompressor::new(
                store,
                self.ctx.session_id,
            )) as Arc<dyn robi_core::compress::Compressor>
        });
        let summary = run_child(mode, task, model, registry, run.cancel, compressor).await?;
        Ok(summary.json())
    }
}

fn parse_mode(value: &str) -> Result<SubagentMode, ToolError> {
    match value.trim() {
        "explore" => Ok(SubagentMode::Explore),
        "general" => Ok(SubagentMode::General),
        other => Err(ToolError::InvalidArgs(format!(
            "mode must be explore or general: {other}"
        ))),
    }
}

fn error_noun(mode: SubagentMode) -> &'static str {
    match mode {
        SubagentMode::Explore => "explore",
        SubagentMode::General => "delegate",
    }
}

fn card_description(description: &str, task: &str) -> String {
    let description = description.trim();
    if !description.is_empty() {
        return description.to_owned();
    }
    let mut cut: String = task.chars().take(80).collect();
    if task.chars().count() > 80 {
        cut.push_str("...");
    }
    cut
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use robi_core::error::ModelError;
    use robi_core::ids::SessionId;
    use robi_core::message::{Message, SubagentMode, SubagentSnapshot};
    use robi_core::model::{Delta, Model, ModelStream};
    use robi_core::tool::{Tool, ToolRegistry, ToolReporter, ToolRun};
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::super::subagent::child_tool_names;
    use super::*;
    use crate::agent::tools::apply_tests::harness;

    struct ScriptModel {
        turns: Mutex<Vec<Message>>,
        calls: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl Model for ScriptModel {
        async fn generate(
            &self,
            _session: SessionId,
            _transcript: &[Message],
            _cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            *self.calls.lock().expect("calls") += 1;
            let message = self
                .turns
                .lock()
                .expect("turns")
                .pop()
                .ok_or_else(|| ModelError::Provider("no scripted turn".into()))?;
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx.send(Delta::Finished(message)).await;
            });
            Ok(ModelStream::new(rx))
        }
    }

    struct FixedModels {
        model: Arc<dyn Model>,
    }

    #[async_trait]
    impl ChildModels for FixedModels {
        async fn build(
            &self,
            _tools: Arc<ToolRegistry>,
            _system_prompt: String,
        ) -> Result<Arc<dyn Model>, String> {
            Ok(Arc::clone(&self.model))
        }
    }

    struct Recorded(Mutex<Vec<SubagentSnapshot>>);

    #[async_trait]
    impl ToolReporter for Recorded {
        async fn subagent(&self, snapshot: SubagentSnapshot) {
            self.0.lock().expect("snapshots").push(snapshot);
        }
    }

    fn run_with(report: Arc<dyn ToolReporter>) -> ToolRun {
        ToolRun {
            cancel: CancellationToken::new(),
            report,
        }
    }

    #[tokio::test]
    async fn explore_has_no_shell_and_general_cannot_edit_or_delegate() {
        let harness = harness().await;
        let explore = child_tool_names(SubagentMode::Explore, Arc::clone(&harness.ctx));
        let general = child_tool_names(SubagentMode::General, Arc::clone(&harness.ctx));
        assert_eq!(
            explore,
            vec![
                "definition",
                "diagnostics",
                "find",
                "grep",
                "hover",
                "list_dir",
                "read_code",
                "read_file",
                "references",
                "semantic_search",
                "workspace_symbol"
            ]
        );
        assert_eq!(
            general,
            vec![
                "definition",
                "diagnostics",
                "find",
                "grep",
                "hover",
                "list_dir",
                "read_code",
                "read_file",
                "references",
                "retrieve",
                "semantic_search",
                "shell",
                "workspace_symbol"
            ]
        );
        for name in [
            "edit_file",
            "write_file",
            "delete_file",
            "grant",
            "delegate",
            "todos",
            "write_plan",
        ] {
            assert!(!general.iter().any(|tool| tool == name), "{name}");
        }
    }

    #[tokio::test]
    async fn the_parent_result_is_the_summary_and_steps_are_reported() {
        let harness = harness().await;
        std::fs::write(harness.root.join("note.txt"), "resume lives here\n").unwrap();
        let model = Arc::new(ScriptModel {
            turns: Mutex::new(vec![
                Message::assistant("note.txt:1 defines resume"),
                Message::assistant_with_tool_calls(
                    "",
                    vec![robi_core::message::ToolCall::new(
                        "read_file",
                        json!({"path": "note.txt"}),
                    )],
                ),
            ]),
            calls: Arc::new(Mutex::new(0)),
        });
        let tool = Delegate::new(Arc::clone(&harness.ctx), Arc::new(FixedModels { model }));
        let recorded = Arc::new(Recorded(Mutex::new(Vec::new())));
        let result = tool
            .execute(
                json!({
                    "task": "Where is resume defined?",
                    "mode": "explore",
                    "description": "Find resume",
                    "thoroughness": "quick"
                }),
                run_with(recorded.clone()),
            )
            .await
            .unwrap();
        assert_eq!(result["mode"], "explore");
        assert_eq!(result["answer"], "note.txt:1 defines resume");
        assert_eq!(result["tool_calls"], 1);
        assert!(result["denied"].as_array().unwrap().is_empty());
        assert!(
            !result.to_string().contains("resume lives here"),
            "the file body stays out of the parent result"
        );
        let steps = recorded.0.lock().expect("snapshots");
        assert!(steps.iter().any(|snapshot| snapshot
            .steps
            .iter()
            .any(|step| step.name == "read_file" && step.target == "note.txt")));
    }

    #[tokio::test]
    async fn a_child_call_that_needs_approval_is_denied_and_the_parent_continues() {
        let harness = harness().await;
        let model = Arc::new(ScriptModel {
            turns: Mutex::new(vec![
                Message::assistant("the command was refused"),
                Message::assistant_with_tool_calls(
                    "",
                    vec![robi_core::message::ToolCall::new(
                        "shell",
                        json!({"command": "echo hi", "unsandboxed": true}),
                    )],
                ),
            ]),
            calls: Arc::new(Mutex::new(0)),
        });
        let tool = Delegate::new(Arc::clone(&harness.ctx), Arc::new(FixedModels { model }));
        let result = tool
            .execute(
                json!({
                    "task": "Run echo outside the sandbox",
                    "mode": "general",
                    "description": "Unsandboxed echo"
                }),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(result["answer"], "the command was refused");
        let denied = result["denied"].as_array().unwrap();
        assert_eq!(denied.len(), 1);
        assert!(denied[0].as_str().unwrap().contains("cannot ask"));
    }

    #[tokio::test]
    async fn the_session_budget_returns_a_limit_and_the_parent_can_continue() {
        let harness = harness().await;
        let model = Arc::new(ScriptModel {
            turns: Mutex::new(
                (0..GENERAL_CALLS)
                    .map(|_| Message::assistant("done"))
                    .collect(),
            ),
            calls: Arc::new(Mutex::new(0)),
        });
        let calls = Arc::clone(&model.calls);
        let tool = Delegate::new(Arc::clone(&harness.ctx), Arc::new(FixedModels { model }));
        for _ in 0..GENERAL_CALLS {
            let result = tool
                .execute(
                    json!({"task": "Look around", "mode": "general"}),
                    ToolRun::new(CancellationToken::new()),
                )
                .await
                .unwrap();
            assert_eq!(result["answer"], "done");
        }
        let limited = tool
            .execute(
                json!({"task": "One more", "mode": "general"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(limited["error"], "delegate_limit");
        assert_eq!(*calls.lock().expect("calls"), GENERAL_CALLS);

        let explore_harness = crate::agent::tools::apply_tests::harness().await;
        let explore = Delegate::new(
            Arc::clone(&explore_harness.ctx),
            Arc::new(FixedModels {
                model: Arc::new(ScriptModel {
                    turns: Mutex::new(
                        (0..EXPLORE_CALLS)
                            .map(|_| Message::assistant("found"))
                            .collect(),
                    ),
                    calls: Arc::new(Mutex::new(0)),
                }),
            }),
        );
        for _ in 0..EXPLORE_CALLS {
            explore
                .execute(
                    json!({"task": "Find it", "mode": "explore"}),
                    ToolRun::new(CancellationToken::new()),
                )
                .await
                .unwrap();
        }
        let limited = explore
            .execute(
                json!({"task": "Find more", "mode": "explore"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(limited["error"], "explore_limit");
    }
}

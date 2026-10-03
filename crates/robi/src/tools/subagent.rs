//! One child agent: a fresh transcript, a smaller tool set, and a summary.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use robi_core::agent::Agent;
use robi_core::config::LoopConfig;
use robi_core::error::{AgentError, ToolError, TurnOutcome};
use robi_core::event::NopSink;
use robi_core::ids::WorkspaceId;
use robi_core::message::{
    Message, Role, SubagentMode, SubagentSnapshot, SubagentStep, SubagentStepStatus,
};
use robi_core::model::Model;
use robi_core::store::MessageStore;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRegistry, ToolReporter, ToolRun};
use serde_json::{json, Value};

use super::context::ToolContext;
use super::find::Find;
use super::grep::Grep;
use super::list_dir::ListDir;
use super::memory_store::MemoryStore;
use super::read_file::ReadFile;
use super::shell::Shell;

pub(crate) const EXPLORE_ITERATIONS: u32 = 40;
pub(crate) const GENERAL_ITERATIONS: u32 = 50;
pub(crate) const CHILD_TIMEOUT: Duration = Duration::from_secs(120);

/// What the parent model is allowed to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildSummary {
    pub mode: SubagentMode,
    pub answer: String,
    pub tool_calls: u32,
    pub denied: Vec<String>,
}

impl ChildSummary {
    pub(crate) fn json(&self) -> Value {
        json!({
            "mode": self.mode,
            "answer": self.answer,
            "tool_calls": self.tool_calls,
            "denied": self.denied,
        })
    }
}

/// Run the child and return only its summary.
pub(crate) async fn run_child(
    mode: SubagentMode,
    task: String,
    model: Arc<dyn Model>,
    registry: Arc<ToolRegistry>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<ChildSummary, ToolError> {
    let store = Arc::new(MemoryStore::default());
    let iterations = match mode {
        SubagentMode::Explore => EXPLORE_ITERATIONS,
        SubagentMode::General => GENERAL_ITERATIONS,
    };
    let agent = Agent::new(
        store.clone(),
        Arc::new(NopSink),
        model,
        registry,
        LoopConfig::default().with_max_iterations(iterations),
    );
    let session = agent.new_chat(WorkspaceId::new());
    let child_cancel = cancel.child_token();
    let run_cancel = child_cancel.clone();
    let outcome = tokio::select! {
        biased;
        () = child_cancel.cancelled() => return Err(ToolError::Cancelled),
        () = tokio::time::sleep(CHILD_TIMEOUT) => {
            child_cancel.cancel();
            return Err(ToolError::TimedOut);
        }
        outcome = agent.user_input(session, &task, run_cancel) => outcome,
    };
    let messages = store
        .messages(session)
        .await
        .map_err(|err| ToolError::Failed(err.to_string()))?;
    match outcome {
        TurnOutcome::Complete | TurnOutcome::Failed(AgentError::MaxIterations(_)) => {
            Ok(summarize(mode, &messages))
        }
        TurnOutcome::Paused => Err(ToolError::Failed("subagent paused for approval".into())),
        TurnOutcome::Cancelled => Err(ToolError::Cancelled),
        TurnOutcome::Failed(error) => Err(ToolError::Failed(error.to_string())),
    }
}

pub(crate) fn register_child_tools(
    registry: &ToolRegistry,
    mode: SubagentMode,
    ctx: Arc<ToolContext>,
    state: Arc<Mutex<SubagentSnapshot>>,
    report: Arc<dyn ToolReporter>,
) -> Result<(), robi_core::error::RegistryError> {
    let read = |tool: Arc<dyn Tool>| wrap(tool, Arc::clone(&state), Arc::clone(&report));
    registry.register(read(Arc::new(ReadFile::new(Arc::clone(&ctx)))))?;
    registry.register(read(Arc::new(ListDir::new(Arc::clone(&ctx)))))?;
    registry.register(read(Arc::new(Find::new(Arc::clone(&ctx)))))?;
    registry.register(read(Arc::new(Grep::new(Arc::clone(&ctx)))))?;
    if mode == SubagentMode::General {
        registry.register(read(Arc::new(Shell::new(ctx))))?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn child_tool_names(mode: SubagentMode, ctx: Arc<ToolContext>) -> Vec<String> {
    let registry = ToolRegistry::new();
    let snapshot = SubagentSnapshot {
        mode,
        description: String::new(),
        started_ms: 0,
        steps: Vec::new(),
    };
    register_child_tools(
        &registry,
        mode,
        ctx,
        Arc::new(Mutex::new(snapshot)),
        Arc::new(robi_core::tool::NopReporter),
    )
    .expect("child tool names are unique");
    registry.names()
}

fn wrap(
    tool: Arc<dyn Tool>,
    state: Arc<Mutex<SubagentSnapshot>>,
    report: Arc<dyn ToolReporter>,
) -> Arc<dyn Tool> {
    Arc::new(Reporting {
        inner: Arc::new(FailClosed { inner: tool }),
        state,
        report,
    })
}

/// A call that would ask the user returns `access_denied` instead.
struct FailClosed {
    inner: Arc<dyn Tool>,
}

#[async_trait]
impl Tool for FailClosed {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn concurrency(&self) -> Concurrency {
        self.inner.concurrency()
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if self.inner.requires_approval(&args).await == ApprovalDecision::NeedsApproval {
            return Ok(json!({
                "error": "access_denied",
                "message": "this call needs approval, and a subagent cannot ask",
            }));
        }
        self.inner.execute(args, run).await
    }
}

/// Publishes each child call on the parent card, then runs it.
struct Reporting {
    inner: Arc<dyn Tool>,
    state: Arc<Mutex<SubagentSnapshot>>,
    report: Arc<dyn ToolReporter>,
}

impl Reporting {
    async fn publish(&self, snapshot: SubagentSnapshot) {
        self.report.subagent(snapshot).await;
    }

    fn mutate(&self, f: impl FnOnce(&mut SubagentSnapshot)) -> SubagentSnapshot {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard);
        guard.clone()
    }
}

#[async_trait]
impl Tool for Reporting {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn concurrency(&self) -> Concurrency {
        self.inner.concurrency()
    }

    async fn requires_approval(&self, args: &Value) -> ApprovalDecision {
        self.inner.requires_approval(args).await
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        let name = self.inner.name().to_owned();
        let target = step_target(&name, &args);
        let snapshot = self.mutate(|state| {
            state.steps.push(SubagentStep {
                name: name.clone(),
                target: target.clone(),
                status: SubagentStepStatus::Running,
            });
        });
        self.publish(snapshot).await;
        let result = self.inner.execute(args, run).await;
        let status = step_status(&result);
        let snapshot = self.mutate(|state| {
            if let Some(step) = state.steps.iter_mut().rev().find(|step| {
                step.name == name
                    && step.target == target
                    && step.status == SubagentStepStatus::Running
            }) {
                step.status = status;
            }
        });
        self.publish(snapshot).await;
        result
    }
}

fn step_status(result: &Result<Value, ToolError>) -> SubagentStepStatus {
    match result {
        Err(_) => SubagentStepStatus::Failed,
        Ok(value) if value.get("error").and_then(Value::as_str) == Some("access_denied") => {
            SubagentStepStatus::Denied
        }
        Ok(_) => SubagentStepStatus::Ok,
    }
}

pub(crate) fn step_target(name: &str, args: &Value) -> String {
    let field = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let path = field("path");
    let pattern = field("pattern");
    let command = field("command");
    let raw = match name {
        "read_file" => path,
        "list_dir" => {
            if path.is_empty() {
                ".".to_owned()
            } else {
                path
            }
        }
        "grep" => {
            if pattern.is_empty() {
                path
            } else {
                pattern
            }
        }
        "find" => {
            if !pattern.is_empty() {
                pattern
            } else if path.is_empty() {
                ".".to_owned()
            } else {
                path
            }
        }
        "shell" => command,
        other => other.to_owned(),
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn summarize(mode: SubagentMode, messages: &[Message]) -> ChildSummary {
    let mut answer = String::new();
    let mut tool_calls = 0u32;
    let mut denied = Vec::new();
    for message in messages {
        if message.role == Role::Tool {
            tool_calls += 1;
            if let Some(line) = denied_line(&message.content) {
                denied.push(line);
            }
        }
        if message.role == Role::Assistant
            && !message.content.is_empty()
            && message.tool_calls.is_empty()
        {
            answer = message.content.clone();
        }
    }
    if answer.is_empty() {
        answer = "stopped before a final answer".to_owned();
    }
    ChildSummary {
        mode,
        answer,
        tool_calls,
        denied,
    }
}

fn denied_line(content: &str) -> Option<String> {
    let value: Value = serde_json::from_str(content).ok()?;
    if value.get("error").and_then(Value::as_str) != Some("access_denied") {
        return None;
    }
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("access denied");
    let path = value.get("path").and_then(Value::as_str).unwrap_or("");
    if path.is_empty() {
        Some(message.to_owned())
    } else {
        Some(format!("{path}: {message}"))
    }
}

pub(crate) fn explore_prompt(root: &Path, thoroughness: &str, instructions: &str) -> String {
    let mut prompt = format!(
        "\
You are a file search specialist. You explore an unfamiliar codebase and report
what you found. You do not change anything.

<tools>
- find: list files whose paths match a substring. Use it for path patterns and to
  learn a directory's shape.
- grep: search file contents. The pattern is a literal substring unless you set
  regex. Start broad, then narrow with path.
- list_dir: list one directory.
- read_file: read a file when you know the path, or a line window with offset and
  limit. Continue from next_offset when truncated is true.
</tools>

<rules>
- Honor the thoroughness the caller names. \"quick\" is a handful of searches;
  \"medium\" is the obvious paths and naming conventions; \"very thorough\" follows
  every naming convention, plural, and abbreviation you can think of.
- Report evidence, not impressions. Give workspace-relative paths with line
  numbers and the exact identifier, function, or string you found.
- If a search comes back empty, say so and say what you tried. A negative result
  is a finding; do not guess at a location you did not verify.
- Read only what the task needs. The caller pays context for every line you
  return.
- Return your findings as your final message. It is the only thing the caller
  receives, so make it self-contained.
- Answer in the fewest words that carry the finding.
- You cannot edit and you cannot run commands. A file outside the workspace, a
  protected path such as .env, or a directory the caller has not granted fails
  closed. The user is never asked. Report the denial instead of working around it.
</rules>

<thoroughness>
{thoroughness}
</thoroughness>

The workspace root is {root}.
",
        thoroughness = thoroughness,
        root = root.display(),
    );
    let notes = instructions.trim();
    if !notes.is_empty() {
        prompt.push_str("\n<caller_notes>\n");
        prompt.push_str(notes);
        prompt.push_str("\n</caller_notes>\n");
    }
    prompt
}

pub(crate) fn general_prompt(root: &Path) -> String {
    format!(
        "\
You are an investigator. You read, search, and run sandboxed commands, then
report what you found. You do not change files.

<tools>
- find, grep, list_dir, and read_file inspect the workspace.
- shell runs a sandboxed command. It can read and write the workspace, and it
  cannot read the home directory, secret files, or the network. A call that
  would need approval fails with access_denied. Do not set unsandboxed,
  read_paths, write_paths, or network to unrestricted.
</tools>

<rules>
- The task is the whole job. You do not see the conversation that sent it.
- Return a self-contained summary as your final message. It is the only thing
  the caller receives. Include the paths, the commands, and what they showed.
  Do not paste long file bodies.
- A denial is a finding. Report it. Do not work around it.
- You cannot edit, grant access, or start another subagent.
</rules>

The workspace root is {}.
",
        root.display()
    )
}

pub(crate) fn thoroughness_or_default(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "quick" => "quick".to_owned(),
        "" | "medium" => "medium".to_owned(),
        "very thorough" | "very_thorough" | "verythorough" | "deep" => "very thorough".to_owned(),
        other => other.to_owned(),
    }
}

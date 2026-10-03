//! A minimal end-to-end entrypoint: build a model and an agent from the
//! environment, then drive a conversation through the core loop.
//!
//! This is the smallest program that exercises the whole stack against a real
//! provider — the adapter, the transcript, the tool registry, and the approval
//! path — and it is meant to be run by hand. Its shape follows `examples/simple`
//! in `../gogent`, which serves the same purpose there.
//!
//! What it proves, in order:
//!
//! 1. A turn whose tool needs no approval runs to completion on its own.
//! 2. A turn whose tool needs approval **pauses** instead of running it, leaving
//!    the call pending in the transcript.
//! 3. `approve` settles that call, and `resume` carries the turn to a final
//!    assistant message that has read the tool result.
//!
//! Run it with `cargo run -p robi --example simple`.
//!
//! ```sh
//! export OPENCODE_GO_API_KEY=...
//! cargo run -p robi --example simple
//! cargo run -p robi --example simple -- "Add 40 and 2"
//! ```

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use robi::agent::providers::{build_model, ApiKey, ModelId, ProviderSettings, ReasoningEffort};
use robi_core::agent::Agent;
use robi_core::config::LoopConfig;
use robi_core::error::{StoreError, ToolError, TurnOutcome};
use robi_core::event::NopSink;
use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::{Message, Role};
use robi_core::store::MessageStore;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRegistry, ToolRun};
use tokio_util::sync::CancellationToken;

/// The system prompt this entrypoint always sends.
///
/// It forbids mental arithmetic on purpose. A model that answers from memory will
/// produce a correct number without calling a tool, and then the tool path, the
/// approval pause, and the provider's tool-call id round trip all go untested.
const SYSTEM_PROMPT: &str = "You are a calculator assistant used to test the Robi \
agent loop. Use the provided tools for every arithmetic result. Never compute an \
answer yourself, and never guess a result you did not get from a tool.";

/// The turns this entrypoint runs when the caller passes no prompt.
///
/// The first needs no approval and must complete in one call. The second needs
/// approval and must pause.
const SCRIPTED_TURNS: [&str; 2] = ["Add 2 and 3.", "Now multiply 454 by 543."];

/// Everything, so a failure can be reported in one place.
///
/// A single-threaded runtime: the loop never needs more, and it keeps the
/// approval prompt's blocking read from competing with work on other threads.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("\nerror: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let settings = settings_from_env()?;
    let system_prompt = settings.system_prompt.clone();

    // The registry is shared with the model, because the request body carries the
    // tool definitions and `Model::generate` is handed only the transcript.
    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(TwoNumberTool::addition()))?;
    tools.register(Arc::new(TwoNumberTool::multiplication()))?;

    let model = build_model(settings, tools.clone())?;
    let store = Arc::new(MemoryStore::default());

    println!("tools: {:?}", tools.names());
    println!("registered {} tools", tools.len());

    let agent = Agent::new(
        store.clone(),
        Arc::new(NopSink),
        model,
        tools.clone(),
        LoopConfig::default(),
    );
    let session = agent.new_chat(WorkspaceId::new());
    let cancel = CancellationToken::new();

    println!("--- system prompt ---\n{system_prompt}\n");

    // A prompt on the command line replaces the scripted turns.
    let turns: Vec<String> = match std::env::args().nth(1) {
        Some(prompt) => vec![prompt],
        None => SCRIPTED_TURNS.iter().map(|s| (*s).to_owned()).collect(),
    };

    for turn in turns {
        println!("=== user ===\n{turn}\n");

        let mut outcome = agent.user_input(session, &turn, cancel.clone()).await;
        println!("outcome: {outcome:?}");

        // A paused turn is the point of this entrypoint: the model asked for a tool
        // that needs a decision, and the loop stopped before running it.
        if outcome == TurnOutcome::Paused {
            outcome = resolve_pending(&agent, session, cancel.clone()).await?;
        }

        if let TurnOutcome::Failed(error) = &outcome {
            return Err(format!("the turn failed: {error}").into());
        }
        println!();
    }

    println!("--- transcript ---");
    print_transcript(&agent.messages(session).await?);
    Ok(())
}

/// Show the calls waiting on a decision, ask about each, then carry the turn on.
async fn resolve_pending(
    agent: &Agent,
    session: SessionId,
    cancel: CancellationToken,
) -> Result<TurnOutcome, Box<dyn Error>> {
    let pending = agent.pending_tool_calls(session).await?;
    println!("\n--- approval required ---");
    for call in &pending {
        println!("  {} {}", call.name, call.args);
    }

    for call in pending {
        // `approve` settles the call and nothing else; the turn continues on the
        // next `resume`. That split is what lets a paused turn survive a restart.
        if ask(&format!("run {} {}?", call.name, call.args))? {
            agent.approve(session, call.id).await?;
            println!("approved {}", call.name);
        } else {
            let reason = "the user declined this call";
            agent.reject(session, call.id, reason).await?;
            println!("rejected {} ({reason})", call.name);
        }
    }

    Ok(agent.resume(session, cancel).await)
}

/// Ask a yes/no question on the terminal. Defaults to no.
fn ask(question: &str) -> Result<bool, Box<dyn Error>> {
    use std::io::Write;

    print!("{question} [y/N] ");
    std::io::stdout().flush()?;

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        // No input available, so nothing was approved. Failing closed is the only
        // safe default for a call that asked for a decision.
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn print_transcript(messages: &[Message]) {
    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        println!("[{role}] {}", message.content);

        for call in &message.tool_calls {
            println!(
                "  -> {} {} approval={:?} execution={:?}",
                call.name, call.args, call.approval_status, call.execution_status
            );
            match (&call.result, &call.error, &call.args_error) {
                (_, _, Some(error)) => println!("     arguments: {error}"),
                (_, Some(error), _) => println!("     error: {error}"),
                (Some(result), _, _) => println!("     result: {result}"),
                (None, None, None) => {}
            }
        }
        if let Some(usage) = &message.usage {
            println!(
                "  (tokens in={} out={} cached={})",
                usage.input, usage.output, usage.cached
            );
        }
    }
}

/// Configuration, read from the environment.
///
/// Only the credential is required. `ProviderSettings` reads no variable itself —
/// this function is the layer that does, and it is deliberately the entrypoint's
/// job rather than the model's.
fn settings_from_env() -> Result<ProviderSettings, Box<dyn Error>> {
    let key = std::env::var("OPENCODE_GO_API_KEY").map_err(|_| {
        "set OPENCODE_GO_API_KEY to an OpenCode Go key (and optionally ROBI_MODEL, \
         ROBI_BASE_URL, ROBI_EFFORT)"
    })?;
    if key.trim().is_empty() {
        return Err("OPENCODE_GO_API_KEY is empty".into());
    }

    let model = std::env::var("ROBI_MODEL").unwrap_or_else(|_| "glm-5.3".to_owned());
    let mut settings =
        ProviderSettings::opencode_go(ApiKey::new(key), ModelId::new(model.as_str()));
    settings.system_prompt = SYSTEM_PROMPT.to_owned();

    if let Ok(base_url) = std::env::var("ROBI_BASE_URL") {
        settings.base_url = base_url;
    }

    // A hostname that is not the vendor's is how the fake server in
    // `tests/provider.rs` is pointed at, and it is useful by hand too.
    if let Ok(effort) = std::env::var("ROBI_EFFORT") {
        settings.reasoning_effort = Some(match effort.to_ascii_lowercase().as_str() {
            "low" => ReasoningEffort::Low,
            "medium" => ReasoningEffort::Medium,
            "high" => ReasoningEffort::High,
            other => {
                return Err(format!("ROBI_EFFORT must be low, medium, or high: {other}").into())
            }
        });
    }

    println!("model: {}", settings.model);
    println!("endpoint: {}", settings.chat_completions_url());
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// A tool over two integers, so the two tools below differ only in what they do
/// and whether they need a decision.
struct TwoNumberTool {
    name: &'static str,
    description: &'static str,
    op: fn(i64, i64) -> i64,
    needs_approval: bool,
}

impl TwoNumberTool {
    fn addition() -> Self {
        Self {
            name: "addition",
            description: "Add two integers and return their sum.",
            op: |a, b| a + b,
            needs_approval: false,
        }
    }

    fn multiplication() -> Self {
        Self {
            name: "multiplication",
            description: "Multiply two integers and return their product.",
            op: |a, b| a * b,
            // The example's whole point: this one pauses the turn.
            needs_approval: true,
        }
    }

    /// Both tools take the same shape, so the schema is built once.
    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "a": {"type": "integer", "description": "The first integer"},
                "b": {"type": "integer", "description": "The second integer"},
            },
            "required": ["a", "b"],
        })
    }

    fn operands(&self, args: &serde_json::Value) -> Result<(i64, i64), ToolError> {
        let field = |name: &str| {
            args.get(name)
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| {
                    ToolError::InvalidArgs(format!(
                        "{}.{name} must be an integer, got {}",
                        self.name, args
                    ))
                })
        };
        Ok((field("a")?, field("b")?))
    }
}

#[async_trait]
impl Tool for TwoNumberTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters(&self) -> serde_json::Value {
        Self::schema()
    }

    fn concurrency(&self) -> Concurrency {
        // Pure arithmetic, so several calls may overlap.
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &serde_json::Value) -> ApprovalDecision {
        if self.needs_approval {
            ApprovalDecision::NeedsApproval
        } else {
            ApprovalDecision::AllowImmediately
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _run: ToolRun,
    ) -> Result<serde_json::Value, ToolError> {
        let (a, b) = self.operands(&args)?;
        let result = (self.op)(a, b);
        println!("  [{}] {a} and {b} -> {result}", self.name);
        Ok(serde_json::json!({ "result": result }))
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// A transcript in memory, for one process.
///
/// `robi-core`'s in-memory store is `#[cfg(test)]`, so it is not reachable from
/// here, and M2's `store` module is where a real one lands. This is the minimum a
/// single conversation needs.
#[derive(Default)]
struct MemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<Message>>>,
}

impl MemoryStore {
    fn with_session<T>(
        &self,
        session: SessionId,
        f: impl FnOnce(&mut Vec<Message>) -> T,
    ) -> Result<T, StoreError> {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let messages = sessions
            .get_mut(&session)
            .ok_or(StoreError::SessionNotFound(session))?;
        Ok(f(messages))
    }
}

#[async_trait]
impl MessageStore for MemoryStore {
    fn create_session(&self, _workspace: WorkspaceId) -> SessionId {
        let session = SessionId::new();
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session, Vec::new());
        session
    }

    fn has_session(&self, session: SessionId) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&session)
    }

    async fn messages(&self, session: SessionId) -> Result<Vec<Message>, StoreError> {
        self.with_session(session, |messages| messages.clone())
    }

    async fn message(
        &self,
        session: SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        self.with_session(session, |messages| {
            messages.iter().find(|message| message.id == id).cloned()
        })
    }

    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        self.with_session(session, |messages| messages.push(message))
    }

    /// Replace by id. A message the loop only ever appends a status to still has
    /// to be findable, because a resumed turn rewrites the call it settled.
    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        self.with_session(session, |messages| {
            match messages
                .iter_mut()
                .find(|existing| existing.id == message.id)
            {
                Some(existing) => *existing = message,
                None => messages.push(message),
            }
        })
    }
}

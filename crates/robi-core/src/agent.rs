//! The agent loop.
//!
//! The loop is a repeat-until-settled function: settle any unresolved turn, take
//! a model turn, resolve the tool calls it asked for, and repeat while the model
//! keeps asking. Every state it acts on is derived from the stored transcript,
//! so a restart needs nothing else reconstructed.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{oneshot, Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::config::LoopConfig;
use crate::error::{AgentError, ModelError, StoreError, ToolError, TurnOutcome};
use crate::event::{Event, EventSink};
use crate::ids::{MessageId, SessionId, ToolCallId, WorkspaceId};
use crate::message::{
    unresolved_turn, ApprovalStatus, ExecutionStatus, Message, SubagentSnapshot, ToolCall,
    Truncation, Truncator,
};
use crate::model::{Delta, Model};
use crate::segments::{segment_by, Segment};
use crate::store::MessageStore;
use crate::tool::{Tool, ToolRegistry, ToolReporter, ToolRun};

/// What settling an unresolved turn found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Settle {
    /// A call still needs the user's decision.
    Paused,
    /// Tool calls ran, so the model owes a reading of their results.
    Executed,
    /// Nothing was outstanding.
    NothingOutstanding,
}

/// The result of resolving one turn's tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Paused,
    Done,
}

/// Why a turn stopped inside the loop.
#[derive(Debug, Clone)]
enum TurnError {
    Cancelled,
    Failed(AgentError),
}

impl From<AgentError> for TurnError {
    fn from(error: AgentError) -> Self {
        TurnError::Failed(error)
    }
}

impl From<StoreError> for TurnError {
    fn from(error: StoreError) -> Self {
        TurnError::Failed(error.into())
    }
}

impl From<ModelError> for TurnError {
    fn from(error: ModelError) -> Self {
        TurnError::Failed(error.into())
    }
}

/// What one call in the turn is for, decided before anything runs.
enum CallPlan {
    Run {
        tool: Arc<dyn Tool>,
        args: serde_json::Value,
    },
    /// The user rejected it. It never runs.
    Reject { reason: String },
    /// It cannot run: unknown tool, or arguments that did not parse.
    Fail(ToolError),
    /// Resolved in an earlier pass. Left exactly as it is, so a second settle
    /// cannot re-run it or duplicate its result message.
    Settled,
}

/// A call that will run, with its position in the turn.
struct RunItem {
    position: usize,
    call_id: ToolCallId,
    tool: Arc<dyn Tool>,
    args: serde_json::Value,
}

/// Writes a child snapshot onto one parent call and tells the UI.
///
/// The message mutex is held across the store write so two delegates in the
/// same turn cannot replace each other's steps.
struct CallReporter {
    session: SessionId,
    message_id: MessageId,
    call_id: ToolCallId,
    shared: Arc<Mutex<Message>>,
    store: Arc<dyn MessageStore>,
    events: Arc<dyn EventSink>,
}

#[async_trait]
impl ToolReporter for CallReporter {
    async fn subagent(&self, snapshot: SubagentSnapshot) {
        let mut guard = self.shared.lock().await;
        let Some(call) = guard.call_mut(self.call_id) else {
            return;
        };
        if call.execution_status == ExecutionStatus::NotStarted {
            call.execution_status = ExecutionStatus::Running;
        }
        call.subagent = Some(snapshot);
        let message = guard.clone();
        if self.store.update(self.session, message).await.is_err() {
            return;
        }
        drop(guard);
        self.events
            .emit(Event::ToolCallUpdated {
                session: self.session,
                message: self.message_id,
                call: self.call_id,
            })
            .await;
    }
}

/// Drives turns over a transcript.
pub struct Agent {
    store: Arc<dyn MessageStore>,
    events: Arc<dyn EventSink>,
    model: Arc<dyn Model>,
    tools: Arc<ToolRegistry>,
    config: LoopConfig,
}

impl Agent {
    pub fn new(
        store: Arc<dyn MessageStore>,
        events: Arc<dyn EventSink>,
        model: Arc<dyn Model>,
        tools: Arc<ToolRegistry>,
        config: LoopConfig,
    ) -> Self {
        Self {
            store,
            events,
            model,
            tools,
            config,
        }
    }

    pub fn config(&self) -> LoopConfig {
        self.config
    }

    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    /// Open a session in a workspace.
    pub fn new_chat(&self, workspace: WorkspaceId) -> SessionId {
        self.store.create_session(workspace)
    }

    /// The whole transcript, in order.
    pub async fn messages(&self, session: SessionId) -> Result<Vec<Message>, AgentError> {
        Ok(self.store.messages(session).await?)
    }

    /// The calls waiting on a decision, in transcript order.
    ///
    /// A call that has already run is not waiting on anything. A call the tool
    /// would run immediately is stored as `Approved` once the loop has decided,
    /// so it is not offered while a sibling call is paused. Filtering on approval
    /// alone would still report a call the user approved that has not run yet.
    pub async fn pending_tool_calls(
        &self,
        session: SessionId,
    ) -> Result<Vec<ToolCall>, AgentError> {
        let messages = self.store.messages(session).await?;
        Ok(messages
            .iter()
            .flat_map(|message| message.tool_calls.iter())
            .filter(|call| call.is_pending_approval() && call.needs_execution())
            .cloned()
            .collect())
    }

    /// Approve one call.
    ///
    /// This settles the call and nothing else. The turn continues when the caller
    /// next calls [`Agent::resume`] or [`Agent::user_input`], which is what lets a
    /// paused turn survive a restart.
    pub async fn approve(&self, session: SessionId, call: ToolCallId) -> Result<(), AgentError> {
        self.settle_call(session, call, None).await
    }

    /// Reject one call, with a reason the model reads.
    pub async fn reject(
        &self,
        session: SessionId,
        call: ToolCallId,
        reason: &str,
    ) -> Result<(), AgentError> {
        self.settle_call(session, call, Some(reason.to_owned()))
            .await
    }

    /// Add a user message and run.
    ///
    /// Settles any unresolved turn first. If the turn is still paused after
    /// settling, the text is not appended: user input never runs ahead of an
    /// unresolved turn.
    pub async fn user_input(
        &self,
        session: SessionId,
        text: &str,
        cancel: CancellationToken,
    ) -> TurnOutcome {
        match self.settle_unresolved(&session, &cancel).await {
            Ok(Settle::Paused) => return TurnOutcome::Paused,
            Ok(_) => {}
            Err(error) => return TurnOutcome::Failed(error),
        }

        let message = Message::user(text);
        if let Err(error) = self.store.append(session, message.clone()).await {
            return TurnOutcome::Failed(error.into());
        }
        self.emit(Event::MessageAdded {
            session,
            message: message.id,
        })
        .await;

        self.run(session, true, cancel).await
    }

    /// Settle whatever is outstanding, then continue.
    ///
    /// A resume that settled tool calls takes a model turn so the model can read
    /// their results. A resume that settles nothing is a no-op.
    pub async fn resume(&self, session: SessionId, cancel: CancellationToken) -> TurnOutcome {
        self.run(session, false, cancel).await
    }

    async fn run(
        &self,
        session: SessionId,
        invoke_model: bool,
        cancel: CancellationToken,
    ) -> TurnOutcome {
        self.emit(Event::TurnStarted { session }).await;
        let outcome = self.run_inner(&session, invoke_model, &cancel).await;
        self.emit(Event::TurnFinished {
            session,
            outcome: outcome.clone(),
        })
        .await;
        outcome
    }

    async fn run_inner(
        &self,
        session: &SessionId,
        invoke_model: bool,
        cancel: &CancellationToken,
    ) -> TurnOutcome {
        let mut invoke = invoke_model;
        let mut model_turns = 0u32;

        loop {
            // 1. Nothing runs ahead of an unresolved turn. Settle it first.
            match self.settle_unresolved(session, cancel).await {
                Ok(Settle::Paused) => return TurnOutcome::Paused,
                Ok(Settle::Executed) => invoke = true,
                Ok(Settle::NothingOutstanding) => {}
                Err(error) => return TurnOutcome::Failed(error),
            }

            // 2. A resume with nothing outstanding and no new input is done.
            if !invoke {
                return TurnOutcome::Complete;
            }

            if cancel.is_cancelled() {
                return TurnOutcome::Cancelled;
            }

            // 3. The cap counts model turns, not tool calls.
            if model_turns >= self.config.max_iterations {
                return TurnOutcome::Failed(AgentError::MaxIterations(self.config.max_iterations));
            }

            // 4. One model turn. The message is appended inside `model_turn`.
            let assistant = match self.model_turn(session, cancel).await {
                Ok(assistant) => assistant,
                Err(TurnError::Cancelled) => return TurnOutcome::Cancelled,
                Err(TurnError::Failed(error)) => return TurnOutcome::Failed(error),
            };
            model_turns += 1;

            // 5. No tool calls means the turn is settled.
            if !assistant.has_tool_calls() {
                return TurnOutcome::Complete;
            }

            // 6. Resolve this turn's calls. May pause; a tool error never ends it.
            match self.process_tool_calls(session, &assistant, cancel).await {
                Ok(Phase::Done) => {}
                Ok(Phase::Paused) => return TurnOutcome::Paused,
                Err(TurnError::Cancelled) => return TurnOutcome::Cancelled,
                Err(TurnError::Failed(error)) => return TurnOutcome::Failed(error),
            }

            // 7. Loop so the model can read the results.
            invoke = true;
        }
    }

    /// Settle the newest unresolved turn, if there is one.
    async fn settle_unresolved(
        &self,
        session: &SessionId,
        cancel: &CancellationToken,
    ) -> Result<Settle, AgentError> {
        let messages = self.store.messages(*session).await?;
        let Some(index) = unresolved_turn(&messages) else {
            return Ok(Settle::NothingOutstanding);
        };
        let assistant = messages[index].clone();

        // Ask each tool again. A call is stored `Pending` the moment the model
        // asks for it, including one that needs no decision, so the status alone
        // cannot pause the turn. A restart between the model turn and the tool
        // round is this path.
        match self.process_tool_calls(session, &assistant, cancel).await {
            Ok(Phase::Done) => Ok(Settle::Executed),
            Ok(Phase::Paused) => Ok(Settle::Paused),
            Err(TurnError::Cancelled) => Ok(Settle::Paused),
            Err(TurnError::Failed(error)) => Err(error),
        }
    }

    /// One model turn: drain the stream to the sink, append the message.
    async fn model_turn(
        &self,
        session: &SessionId,
        cancel: &CancellationToken,
    ) -> Result<Message, TurnError> {
        let transcript = self.store.messages(*session).await?;

        // Cancellation during connection setup must report `Cancelled`, not a
        // failure: the adapter cannot express "cancelled" through `ModelError`, and
        // a turn the user stopped is not an error to surface. Selecting here also
        // drops the in-flight request.
        let stream = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(TurnError::Cancelled),
            stream = self.model.generate(*session, &transcript, cancel.clone()) => stream?,
        };
        let mut stream = stream;
        // The id the delta events will name, fixed before the first delta.
        let message_id = stream.message_id();

        loop {
            let delta = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(TurnError::Cancelled),
                delta = stream.next() => delta,
            };

            match delta {
                None => return Err(ModelError::StreamClosed.into()),
                Some(Delta::Finished(mut message)) => {
                    // A cancelled turn does not append, and neither does a stream
                    // that ends without a finished message.
                    message.id = message_id;
                    self.store.append(*session, message.clone()).await?;
                    self.emit(Event::MessageAdded {
                        session: *session,
                        message: message.id,
                    })
                    .await;
                    return Ok(message);
                }
                Some(Delta::Failed(error)) => return Err(error.into()),
                Some(delta) => {
                    self.emit(Event::MessageDelta {
                        session: *session,
                        message: message_id,
                        delta,
                    })
                    .await;
                }
            }
        }
    }

    /// Resolve one turn's tool calls: decide, execute, assemble.
    async fn process_tool_calls(
        &self,
        session: &SessionId,
        assistant: &Message,
        cancel: &CancellationToken,
    ) -> Result<Phase, TurnError> {
        // Phase 1 — decide. Sequential and side-effect free, over the whole turn.
        let mut assistant = assistant.clone();
        let mut plans = Vec::with_capacity(assistant.tool_calls.len());
        let mut waiting = Vec::new();
        let mut cleared = Vec::new();

        for call in &mut assistant.tool_calls {
            // Already resolved. Checked first, so a second settle over the same
            // message is a no-op rather than a re-run.
            if call.execution_status.is_terminal() {
                plans.push(CallPlan::Settled);
                continue;
            }

            if call.approval_status == ApprovalStatus::Rejected {
                plans.push(CallPlan::Reject {
                    reason: call
                        .error
                        .clone()
                        .unwrap_or_else(|| "rejected by the user".to_owned()),
                });
                continue;
            }

            if let Some(error) = &call.args_error {
                plans.push(CallPlan::Fail(ToolError::InvalidArgs(error.clone())));
                continue;
            }

            let Some(tool) = self.tools.get(&call.name) else {
                plans.push(CallPlan::Fail(ToolError::NotFound(call.name.clone())));
                continue;
            };

            if self.tools.awaits_user_decision(call).await {
                waiting.push(call.id);
            } else if call.is_pending_approval() {
                // Recorded so a sibling that does need a decision does not leave
                // this call looking like one. It still does not run until the
                // whole turn is decided.
                call.approval_status = ApprovalStatus::Approved;
                cleared.push(call.id);
            }

            plans.push(CallPlan::Run {
                tool,
                args: call.args.clone(),
            });
        }

        if !waiting.is_empty() {
            // Nothing in this turn has run. The calls that need a person stay
            // pending. The others are already marked approved on `assistant`.
            if !cleared.is_empty() {
                let message_id = assistant.id;
                self.store.update(*session, assistant.clone()).await?;
                self.emit(Event::MessageUpdated {
                    session: *session,
                    message: message_id,
                })
                .await;
                for call in cleared {
                    self.emit(Event::ToolCallUpdated {
                        session: *session,
                        message: message_id,
                        call,
                    })
                    .await;
                }
            }
            for call in waiting {
                self.emit(Event::AwaitingApproval {
                    session: *session,
                    call,
                })
                .await;
            }
            return Ok(Phase::Paused);
        }

        // Phase 2 — execute. Model order, in segments.
        let runnable: Vec<RunItem> = plans
            .iter()
            .enumerate()
            .filter_map(|(position, plan)| match plan {
                CallPlan::Run { tool, args } => Some(RunItem {
                    position,
                    call_id: assistant.tool_calls[position].id,
                    tool: tool.clone(),
                    args: args.clone(),
                }),
                _ => None,
            })
            .collect();

        // Reporters and the final write share this copy, so a snapshot published
        // mid-call is still there when the result is stored.
        let shared = Arc::new(Mutex::new(assistant.clone()));

        let serial = self.config.serial_tools;
        let segments = segment_by(runnable, |item| {
            serial || item.tool.concurrency().is_exclusive()
        });

        let mut outcomes: Vec<Option<Result<serde_json::Value, ToolError>>> =
            (0..plans.len()).map(|_| None).collect();

        for segment in segments {
            match segment {
                Segment::Batch(items) => {
                    // A batch is bounded by the in-flight limit.
                    let limit = self.config.max_concurrent_tools;
                    for (position, result) in self
                        .run_items(items, limit, cancel, session, assistant.id, &shared)
                        .await
                    {
                        outcomes[position] = Some(result);
                    }
                }
                Segment::Solo(item) => {
                    // A solo call runs alone: a limit of one is the barrier.
                    for (position, result) in self
                        .run_items(vec![item], 1, cancel, session, assistant.id, &shared)
                        .await
                    {
                        outcomes[position] = Some(result);
                    }
                }
            }
        }

        // Phase 3 — assemble. Model order, not completion order.
        // Start from the shared copy so child snapshots survive this write.
        let mut updated = shared.lock().await.clone();
        let mut processed: Vec<usize> = Vec::new();

        for (position, plan) in plans.iter().enumerate() {
            let Some(call) = updated.tool_calls.get_mut(position) else {
                continue;
            };
            match plan {
                CallPlan::Settled => continue,
                CallPlan::Reject { reason } => {
                    call.execution_status = ExecutionStatus::Failed;
                    call.error = Some(format!("rejected by the user: {reason}"));
                    call.result = Some(rejection_payload(reason));
                }
                CallPlan::Fail(error) => {
                    call.execution_status = status_for(error);
                    call.error = Some(error.to_string());
                    call.result = Some(error_payload(error));
                }
                CallPlan::Run { .. } => {
                    // A plan with no outcome means the call never ran, which is how
                    // a cancellation mid-batch looks once the results are assembled.
                    let outcome = outcomes[position]
                        .take()
                        .unwrap_or(Err(ToolError::Cancelled));
                    match outcome {
                        Ok(value) => {
                            let (value, truncated) = self.enforce_result_ceiling(value);
                            call.execution_status = ExecutionStatus::Succeeded;
                            call.result = Some(value);
                            if truncated {
                                call.truncation = Some(Truncation {
                                    by: Truncator::Core,
                                    limit_bytes: self.config.max_tool_result_bytes,
                                });
                            }
                        }
                        Err(error) => {
                            call.execution_status = status_for(&error);
                            call.error = Some(error.to_string());
                            call.result = Some(error_payload(&error));
                        }
                    }
                }
            }
            processed.push(position);
        }

        if processed.is_empty() {
            return Ok(Phase::Done);
        }

        self.store.update(*session, updated.clone()).await?;
        self.emit(Event::MessageUpdated {
            session: *session,
            message: updated.id,
        })
        .await;

        // One result message per call resolved in this pass, in model order.
        for position in &processed {
            let call = &updated.tool_calls[*position];
            let message = Message::tool_result(call.id, tool_result_content(call));
            self.store.append(*session, message.clone()).await?;
            self.emit(Event::MessageAdded {
                session: *session,
                message: message.id,
            })
            .await;
            self.emit(Event::ToolCallUpdated {
                session: *session,
                message: updated.id,
                call: call.id,
            })
            .await;
        }

        Ok(Phase::Done)
    }

    /// Run calls with at most `limit` in flight, returning results by position.
    ///
    /// Results come back in the order given, never in completion order.
    async fn run_items(
        &self,
        items: Vec<RunItem>,
        limit: usize,
        cancel: &CancellationToken,
        session: &SessionId,
        message_id: MessageId,
        shared: &Arc<Mutex<Message>>,
    ) -> Vec<(usize, Result<serde_json::Value, ToolError>)> {
        let semaphore = Arc::new(Semaphore::new(limit.max(1)));
        let mut handles = Vec::with_capacity(items.len());

        for item in items {
            // Taking the permit before spawning bounds the number of live tasks.
            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                continue;
            };
            let tool = item.tool;
            let args = item.args;
            let cancel = cancel.clone();
            let report = Arc::new(CallReporter {
                session: *session,
                message_id,
                call_id: item.call_id,
                shared: Arc::clone(shared),
                store: Arc::clone(&self.store),
                events: Arc::clone(&self.events),
            });
            let (tx, rx) = oneshot::channel();

            // Spawned rather than inlined: an unwinding tool task becomes a failed
            // call instead of taking the process with it.
            tokio::spawn(async move {
                let _permit = permit;
                let run = ToolRun { cancel, report };
                let _ = tx.send(tool.execute(args, run).await);
            });

            handles.push((item.position, rx));
        }

        let mut results = Vec::with_capacity(handles.len());
        for (position, rx) in handles {
            let result = match rx.await {
                Ok(result) => result,
                // The sender drops when the task unwinds, so a panic arrives here.
                Err(_) => Err(ToolError::Panicked),
            };
            results.push((position, result));
        }
        results
    }

    /// Cut a tool result that exceeds the backstop.
    fn enforce_result_ceiling(&self, value: serde_json::Value) -> (serde_json::Value, bool) {
        let limit = self.config.max_tool_result_bytes;
        let Ok(serialized) = serde_json::to_string(&value) else {
            return (value, false);
        };
        if serialized.len() <= limit {
            return (value, false);
        }

        let mut cut = limit.min(serialized.len());
        while cut > 0 && !serialized.is_char_boundary(cut) {
            cut -= 1;
        }

        let truncated = serde_json::json!({
            "truncated_by": "core",
            "limit_bytes": limit,
            "original_bytes": serialized.len(),
            "note": "the tool did not bound its own output, so only a prefix is shown",
            "prefix": &serialized[..cut],
        });
        (truncated, true)
    }

    /// Settle one call. Idempotent: a call already decided is left alone.
    async fn settle_call(
        &self,
        session: SessionId,
        call_id: ToolCallId,
        rejection: Option<String>,
    ) -> Result<(), AgentError> {
        let messages = self.store.messages(session).await?;
        let Some(index) = messages
            .iter()
            .rposition(|message| message.tool_calls.iter().any(|call| call.id == call_id))
        else {
            return Ok(());
        };

        let mut message = messages[index].clone();
        let message_id = message.id;
        let Some(call) = message.call_mut(call_id) else {
            return Ok(());
        };
        if !call.is_pending_approval() {
            return Ok(());
        }

        match rejection {
            Some(reason) => {
                call.approval_status = ApprovalStatus::Rejected;
                call.error = Some(reason);
            }
            None => {
                call.approval_status = ApprovalStatus::Approved;
                call.error = None;
            }
        }

        self.store.update(session, message).await?;
        self.emit(Event::ToolCallUpdated {
            session,
            message: message_id,
            call: call_id,
        })
        .await;
        Ok(())
    }

    async fn emit(&self, event: Event) {
        self.events.emit(event).await;
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("config", &self.config)
            .field("tools", &self.tools.names())
            .finish_non_exhaustive()
    }
}

/// How a failure was recorded.
fn status_for(error: &ToolError) -> ExecutionStatus {
    match error {
        ToolError::TimedOut => ExecutionStatus::TimedOut,
        ToolError::Cancelled => ExecutionStatus::Cancelled,
        _ => ExecutionStatus::Failed,
    }
}

/// The payload the model reads for a failed call.
fn error_payload(error: &ToolError) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "kind": error.kind(),
        "error": error.to_string(),
    })
}

/// The payload the model reads for a rejected call.
fn rejection_payload(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "rejected": true,
        "reason": reason,
    })
}

/// What goes into the tool-result message for a call.
fn tool_result_content(call: &ToolCall) -> String {
    match &call.result {
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()),
        None => "{}".to_owned(),
    }
}

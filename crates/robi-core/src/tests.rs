//! The loop's required test cases, from `docs/design/agent-loop.md`.
//!
//! Every case here runs against a stub model and an in-memory store: no network,
//! no filesystem, no Tauri.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::agent::Agent;
use crate::compact::{CompactError, CompactOutcome, CompactTrigger, Compactor};
use crate::config::LoopConfig;
use crate::error::{AgentError, ModelError, RegistryError, ToolError, TurnOutcome};
use crate::event::Event;
use crate::ids::{SessionId, ToolCallId, WorkspaceId};
use crate::message::{
    unresolved_turn, ApprovalStatus, ExecutionStatus, Message, Role, SubagentMode,
    SubagentSnapshot, SubagentStep, SubagentStepStatus, ToolCall,
};
use crate::model::Delta;
use crate::store::MessageStore;
use crate::testkit::{
    assistant_asking_for, huge_result, FunctionTool, InMemoryStore, Probe, RecordingSink, Script,
    StubModel, Timeline,
};
use crate::tool::{Concurrency, Tool, ToolRegistry, ToolRun};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    agent: Agent,
    store: Arc<InMemoryStore>,
    sink: Arc<RecordingSink>,
    timeline: Arc<Timeline>,
    model: Arc<StubModel>,
    session: SessionId,
}

impl Harness {
    fn transcript(&self) -> Vec<Message> {
        self.store.transcript(self.session)
    }

    /// The ids of the calls on the newest assistant message.
    fn last_call_ids(&self) -> Vec<ToolCallId> {
        self.transcript()
            .iter()
            .rev()
            .find(|message| message.has_tool_calls())
            .map(|message| message.tool_calls.iter().map(|call| call.id).collect())
            .unwrap_or_default()
    }

    fn calls(&self) -> Vec<ToolCall> {
        self.transcript()
            .iter()
            .rev()
            .find(|message| message.has_tool_calls())
            .map(|message| message.tool_calls.clone())
            .unwrap_or_default()
    }

    /// `(role, content)` for each message, for comparing two runs.
    fn projection(&self) -> Vec<(Role, String)> {
        self.transcript()
            .iter()
            .map(|message| (message.role, message.content.clone()))
            .collect()
    }

    /// The names of the tool calls each tool-result message answers, in order.
    fn result_names(&self) -> Vec<String> {
        let mut names: HashMap<ToolCallId, String> = HashMap::new();
        for message in self.transcript() {
            for call in &message.tool_calls {
                names.insert(call.id, call.name.clone());
            }
        }
        self.transcript()
            .iter()
            .filter(|message| message.role == Role::Tool)
            .filter_map(|message| message.tool_call_id)
            .filter_map(|id| names.get(&id).cloned())
            .collect()
    }
}

fn build(
    model: StubModel,
    tools: Vec<Arc<dyn Tool>>,
    config: LoopConfig,
    sink: Arc<RecordingSink>,
) -> Harness {
    // The store shares the sink's timeline, so write-then-emit order is observable.
    let timeline = sink.timeline();
    let store = Arc::new(InMemoryStore::new(timeline.clone()));
    let model = Arc::new(model);
    let registry = Arc::new(ToolRegistry::new());
    for tool in tools {
        registry
            .register(tool)
            .expect("test tools have unique names");
    }

    let agent = Agent::new(store.clone(), sink.clone(), model.clone(), registry, config);
    let session = agent.new_chat(WorkspaceId::new());

    Harness {
        agent,
        store,
        sink,
        timeline,
        model,
        session,
    }
}

fn harness(model: StubModel, tools: Vec<Arc<dyn Tool>>, config: LoopConfig) -> Harness {
    let timeline = Arc::new(Timeline::default());
    let sink = Arc::new(RecordingSink::new(timeline));
    build(model, tools, config, sink)
}

/// A call with no arguments.
fn call(name: &str) -> ToolCall {
    ToolCall::new(name, serde_json::json!({}))
}

fn no_cancel() -> CancellationToken {
    CancellationToken::new()
}

/// A model script that asks for these calls, then answers with `then`.
fn asking_then(names: &[&str], then: &str) -> StubModel {
    StubModel::new(vec![
        Script::Message(assistant_asking_for(
            names.iter().map(|name| call(name)).collect(),
        )),
        Script::Message(Message::assistant(then)),
    ])
}

// ---------------------------------------------------------------------------
// The turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_session_id_reaches_the_model() {
    // A provider needs the session for its routing hint and, later, to pick a
    // model per session. The loop already holds it, so it hands it over.
    let model = StubModel::saying("hello");
    let h = harness(model, vec![], LoopConfig::default());

    h.agent.user_input(h.session, "hi", no_cancel()).await;

    assert_eq!(h.model.sessions_seen(), vec![h.session]);
}

#[tokio::test]
async fn every_model_turn_in_a_conversation_carries_one_session_id() {
    let model = asking_then(&["read"], "done");
    let tools = vec![FunctionTool::new("read").arc()];
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;
    h.agent.user_input(h.session, "again", no_cancel()).await;

    let seen = h.model.sessions_seen();
    assert!(seen.len() > 1, "the tool loop takes several model turns");
    assert!(
        seen.iter().all(|session| *session == h.session),
        "one conversation keeps one session id across every turn"
    );
}

#[tokio::test]
async fn a_single_turn_without_tools_completes() {
    let model = StubModel::saying("hello");
    let h = harness(model, vec![], LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "hi", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(h.model.call_count(), 1);
    assert_eq!(h.transcript().len(), 2);
    assert_eq!(h.transcript()[0].role, Role::User);
    assert_eq!(h.transcript()[1].role, Role::Assistant);
    assert_eq!(h.transcript()[1].content, "hello");
}

#[tokio::test]
async fn a_tool_loop_reaches_a_second_model_turn() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("read").probed(probe.clone()).arc()];
    let model = asking_then(&["read"], "done");
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(h.model.call_count(), 2, "the model reads the tool results");
    assert!(probe.ran("read"));
    // user, assistant(call), tool(result), assistant(done)
    assert_eq!(h.transcript().len(), 4);
    assert_eq!(h.transcript()[2].role, Role::Tool);
    assert_eq!(h.transcript()[3].content, "done");
}

#[tokio::test]
async fn the_cap_counts_model_turns_not_tool_calls() {
    let tools = vec![FunctionTool::new("noop").arc()];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("noop")])),
        Script::Message(assistant_asking_for(vec![call("noop")])),
        Script::Message(assistant_asking_for(vec![call("noop")])),
    ]);
    let config = LoopConfig::default().with_max_iterations(2);
    let h = harness(model, tools, config);

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Failed(AgentError::MaxIterations(2)));
    assert_eq!(
        h.model.call_count(),
        2,
        "the cap counts model turns, so exactly two ran"
    );
}

#[tokio::test]
async fn a_turn_paused_by_the_cap_leaves_its_transcript_intact() {
    let tools = vec![FunctionTool::new("noop").arc()];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("noop")])),
        Script::Message(assistant_asking_for(vec![call("noop")])),
    ]);
    let config = LoopConfig::default().with_max_iterations(1);
    let h = harness(model, tools, config);

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Failed(AgentError::MaxIterations(1)));
    // The tool result for the first turn is still there.
    assert!(h.transcript().iter().any(|m| m.role == Role::Tool));
    assert!(unresolved_turn(&h.transcript()).is_none());
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[tokio::test]
async fn text_and_reasoning_deltas_reach_the_sink() {
    let model = StubModel::new(vec![Script::Deltas {
        deltas: vec![
            Delta::Reasoning("thinking".to_owned()),
            Delta::Text("answer".to_owned()),
            Delta::Finished(Message::assistant("answer")),
        ],
        gap: Duration::ZERO,
    }]);
    let h = harness(model, vec![], LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    let deltas: Vec<Delta> = h
        .sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::MessageDelta { delta, .. } => Some(*delta),
            _ => None,
        })
        .collect();
    assert_eq!(
        deltas,
        vec![
            Delta::Reasoning("thinking".to_owned()),
            Delta::Text("answer".to_owned())
        ],
        "reasoning and text both reach the sink, in order"
    );
}

#[tokio::test]
async fn every_delta_event_names_the_message_the_loop_will_append() {
    let model = StubModel::new(vec![Script::Deltas {
        deltas: vec![
            Delta::Text("a".to_owned()),
            Delta::Finished(Message::assistant("a")),
        ],
        gap: Duration::ZERO,
    }]);
    let h = harness(model, vec![], LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    let delta_ids: Vec<_> = h
        .sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::MessageDelta { message, .. } => Some(message),
            _ => None,
        })
        .collect();
    let appended = h.transcript()[1].id;
    assert!(delta_ids.iter().all(|id| *id == appended));
}

#[tokio::test]
async fn a_model_error_fails_the_turn_without_appending() {
    let model = StubModel::new(vec![Script::Error(ModelError::Provider(
        "rate limited".to_owned(),
    ))]);
    let h = harness(model, vec![], LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(
        outcome,
        TurnOutcome::Failed(AgentError::Model(ModelError::Provider(
            "rate limited".to_owned()
        )))
    );
    assert!(
        h.transcript()
            .iter()
            .all(|message| message.role != Role::Assistant),
        "a failed model turn is not appended"
    );
}

#[tokio::test]
async fn a_stream_that_ends_without_finishing_fails_the_turn() {
    let model = StubModel::new(vec![Script::Deltas {
        deltas: vec![Delta::Text("cut off".to_owned())],
        gap: Duration::ZERO,
    }]);
    let h = harness(model, vec![], LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(
        outcome,
        TurnOutcome::Failed(AgentError::Model(ModelError::StreamClosed))
    );
    assert!(h
        .transcript()
        .iter()
        .all(|message| message.role != Role::Assistant));
}

// ---------------------------------------------------------------------------
// Failures that do not end the run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_tool_error_does_not_end_the_run() {
    let tools = vec![FunctionTool::new("boom")
        .fails(ToolError::Failed("it broke".to_owned()))
        .arc()];
    let model = asking_then(&["boom"], "recovered");
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    let call = &h.calls()[0];
    assert_eq!(call.execution_status, ExecutionStatus::Failed);
    assert_eq!(call.error.as_deref(), Some("it broke"));

    // The model was shown a structured error it can act on.
    let second = &h.model.seen()[1];
    let tool_result = second
        .iter()
        .find(|message| message.role == Role::Tool)
        .expect("a tool result was recorded");
    assert!(tool_result.content.contains("\"ok\":false"));
    assert!(tool_result.content.contains("it broke"));
}

#[tokio::test]
async fn a_panicking_tool_becomes_a_failed_call() {
    let tools = vec![FunctionTool::new("panics").panics().arc()];
    let model = asking_then(&["panics"], "carried on");
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(h.calls()[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(
        h.transcript().last().map(|m| m.content.as_str()),
        Some("carried on")
    );
}

#[tokio::test]
async fn an_unknown_tool_fails_the_call_and_the_run_continues() {
    let model = asking_then(&["nope"], "carried on");
    let h = harness(model, vec![], LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    let call = &h.calls()[0];
    assert_eq!(call.execution_status, ExecutionStatus::Failed);
    assert_eq!(call.error.as_deref(), Some("tool not found: nope"));
    assert!(h.calls()[0]
        .result
        .as_ref()
        .and_then(|result| result.get("kind"))
        .and_then(|kind| kind.as_str())
        .is_some_and(|kind| kind == "not_found"));
}

#[tokio::test]
async fn unparseable_arguments_fail_the_call_without_asking() {
    // Deviation 3: a call whose arguments did not parse fails rather than running
    // on defaults, and it does not prompt for approval first.
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("read").probed(probe.clone()).arc()];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![ToolCall::with_args_error(
            "read",
            "expected a JSON object",
        )])),
        Script::Message(Message::assistant("carried on")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert!(!probe.ran("read"), "the tool never ran");
    assert_eq!(h.calls()[0].execution_status, ExecutionStatus::Failed);
    assert_eq!(
        h.calls()[0].error.as_deref(),
        Some("invalid arguments: expected a JSON object")
    );
}

#[tokio::test]
async fn an_oversized_result_is_truncated_by_the_core() {
    let tools = vec![FunctionTool::new("huge").returns(huge_result(4096)).arc()];
    let model = asking_then(&["huge"], "done");
    let config = LoopConfig::default().with_max_tool_result_bytes(512);
    let h = harness(model, tools, config);

    h.agent.user_input(h.session, "go", no_cancel()).await;

    let call = &h.calls()[0];
    assert_eq!(call.execution_status, ExecutionStatus::Succeeded);
    let truncation = call.truncation.expect("the core truncated the result");
    assert_eq!(truncation.by, crate::message::Truncator::Core);
    assert_eq!(truncation.limit_bytes, 512);

    let result = call.result.as_ref().expect("a result was recorded");
    assert_eq!(
        result.get("truncated_by").and_then(|v| v.as_str()),
        Some("core"),
        "the core's marker is distinguishable from a tool's own"
    );
    assert!(result.get("note").is_some());
}

#[tokio::test]
async fn a_result_within_the_ceiling_is_left_alone() {
    let tools = vec![FunctionTool::new("small")
        .returns(serde_json::json!({"ok": true}))
        .arc()];
    let model = asking_then(&["small"], "done");
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert!(h.calls()[0].truncation.is_none());
}

// ---------------------------------------------------------------------------
// Approval
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_call_needing_approval_pauses_the_turn() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("write")
        .needs_approval()
        .probed(probe.clone())
        .arc()];
    let model = asking_then(&["write"], "done");
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Paused);
    assert!(!probe.ran("write"), "nothing ran before the decision");
    assert_eq!(
        h.agent.pending_tool_calls(h.session).await.unwrap().len(),
        1
    );
    assert!(h
        .sink
        .events()
        .iter()
        .any(|event| matches!(event, Event::AwaitingApproval { .. })));
}

#[tokio::test]
async fn a_call_that_needed_no_approval_is_not_reported_as_pending() {
    // A call that needed no approval is stored as approved once the loop has
    // decided, and it has run. It must not be offered for a decision.
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("read").probed(probe.clone()).arc()];
    let model = asking_then(&["read"], "done");
    let h = harness(model, tools, LoopConfig::default());

    assert_eq!(
        h.agent.user_input(h.session, "go", no_cancel()).await,
        TurnOutcome::Complete
    );
    assert!(probe.ran("read"), "the tool ran without a decision");

    assert!(
        h.agent
            .pending_tool_calls(h.session)
            .await
            .unwrap()
            .is_empty(),
        "a call that already ran is not waiting on a decision"
    );
}

#[tokio::test]
async fn an_executed_call_is_not_pending_while_a_later_call_waits() {
    // The shape a real session produces: within one turn, the model calls a tool
    // that needs no approval, reads its result, then calls one that does. Only the
    // second call may be offered for a decision.
    let probe = Probe::new();
    let tools = vec![
        FunctionTool::new("read").probed(probe.clone()).arc(),
        FunctionTool::new("write")
            .needs_approval()
            .probed(probe.clone())
            .arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("read")])),
        Script::Message(assistant_asking_for(vec![call("write")])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    assert_eq!(
        h.agent
            .user_input(h.session, "read then write", no_cancel())
            .await,
        TurnOutcome::Paused
    );
    assert!(probe.ran("read"), "the auto-approved call ran");
    assert!(!probe.ran("write"), "the call awaiting a decision did not");

    let pending = h.agent.pending_tool_calls(h.session).await.unwrap();
    assert_eq!(pending.len(), 1, "only the call awaiting a decision");
    assert_eq!(pending[0].name, "write");
}

#[tokio::test]
async fn approving_then_resuming_completes_the_turn() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("write")
        .needs_approval()
        .probed(probe.clone())
        .arc()];
    let model = asking_then(&["write"], "done");
    let h = harness(model, tools, LoopConfig::default());

    assert_eq!(
        h.agent.user_input(h.session, "go", no_cancel()).await,
        TurnOutcome::Paused
    );

    let call_id = h.last_call_ids()[0];
    h.agent.approve(h.session, call_id).await.unwrap();
    assert!(h
        .agent
        .pending_tool_calls(h.session)
        .await
        .unwrap()
        .is_empty());

    // Deviation 2: settling runs the tool and the model then reads its results.
    let outcome = h.agent.resume(h.session, no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert!(probe.ran("write"));
    assert_eq!(h.model.call_count(), 2);
    assert_eq!(h.transcript().last().unwrap().content, "done");
}

#[tokio::test]
async fn rejecting_a_call_reaches_the_model_as_a_rejection() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("write")
        .needs_approval()
        .probed(probe.clone())
        .arc()];
    let model = asking_then(&["write"], "understood");
    let h = harness(model, tools, LoopConfig::default());

    assert_eq!(
        h.agent.user_input(h.session, "go", no_cancel()).await,
        TurnOutcome::Paused
    );
    let call_id = h.last_call_ids()[0];
    h.agent
        .reject(h.session, call_id, "the file is not yours to touch")
        .await
        .unwrap();

    let outcome = h.agent.resume(h.session, no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert!(!probe.ran("write"), "a rejected call never runs");
    let call = &h.calls()[0];
    assert_eq!(call.approval_status, ApprovalStatus::Rejected);
    assert_eq!(call.execution_status, ExecutionStatus::Failed);

    let second = &h.model.seen()[1];
    let tool_result = second
        .iter()
        .find(|message| message.role == Role::Tool)
        .expect("the rejection reached the model");
    assert!(tool_result.content.contains("\"rejected\":true"));
    assert!(tool_result
        .content
        .contains("the file is not yours to touch"));
}

#[tokio::test]
async fn settling_a_call_twice_is_a_no_op() {
    let tools = vec![FunctionTool::new("write").needs_approval().arc()];
    let model = asking_then(&["write"], "done");
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;
    let call_id = h.last_call_ids()[0];

    h.agent.approve(h.session, call_id).await.unwrap();
    // A second decision must not reverse the first, so a double click cannot
    // reject a call that was already approved.
    h.agent
        .reject(h.session, call_id, "changed my mind")
        .await
        .unwrap();

    assert_eq!(h.calls()[0].approval_status, ApprovalStatus::Approved);
}

#[tokio::test]
async fn user_input_while_paused_rejects_the_pending_calls() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("write")
        .needs_approval()
        .probed(probe.clone())
        .arc()];
    let model = asking_then(&["write"], "done");
    let h = harness(model, tools, LoopConfig::default());

    assert_eq!(
        h.agent.user_input(h.session, "go", no_cancel()).await,
        TurnOutcome::Paused
    );

    let outcome = h
        .agent
        .user_input(h.session, "are you there?", no_cancel())
        .await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert!(!probe.ran("write"), "a rejected call never runs");
    let call = &h.calls()[0];
    assert_eq!(call.approval_status, ApprovalStatus::Rejected);
    assert!(call
        .error
        .as_deref()
        .is_some_and(|reason| reason.contains("new message")));
    let transcript = h.transcript();
    let user_at = transcript
        .iter()
        .position(|message| message.content == "are you there?")
        .expect("the text was appended");
    let tool_at = transcript
        .iter()
        .position(|message| message.role == Role::Tool)
        .expect("the rejection was recorded");
    assert!(tool_at < user_at, "the rejection precedes the new message");
}

#[tokio::test]
async fn an_interrupted_call_that_needs_no_approval_runs_on_settle() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("diagnostics").probed(probe.clone()).arc()];
    let model = StubModel::saying("done");
    let h = harness(model, tools, LoopConfig::default());
    h.store
        .append(
            h.session,
            Message::assistant_with_tool_calls("check", vec![call("diagnostics")]),
        )
        .await
        .unwrap();

    let outcome = h.agent.resume(h.session, no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert!(probe.ran("diagnostics"));
    assert!(
        !h.sink
            .events()
            .iter()
            .any(|event| matches!(event, Event::AwaitingApproval { .. })),
        "a call the tool would run immediately is not a decision"
    );
}

#[tokio::test]
async fn an_interrupted_call_that_needs_approval_still_pauses() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("write")
        .needs_approval()
        .probed(probe.clone())
        .arc()];
    let model = StubModel::saying("done");
    let h = harness(model, tools, LoopConfig::default());
    h.store
        .append(
            h.session,
            Message::assistant_with_tool_calls("edit", vec![call("write")]),
        )
        .await
        .unwrap();

    assert_eq!(
        h.agent.resume(h.session, no_cancel()).await,
        TurnOutcome::Paused
    );
    assert!(!probe.ran("write"));
    assert_eq!(
        h.agent.pending_tool_calls(h.session).await.unwrap().len(),
        1
    );

    // A later message is a refusal of that decision, same as a pause the user
    // already saw. The rejection is recorded before the text, then the turn
    // continues.
    assert_eq!(
        h.agent
            .user_input(h.session, "are you there?", no_cancel())
            .await,
        TurnOutcome::Complete
    );
    assert!(!probe.ran("write"), "a rejected call never runs");
    let call = &h.calls()[0];
    assert_eq!(call.approval_status, ApprovalStatus::Rejected);
    assert!(call
        .error
        .as_deref()
        .is_some_and(|reason| reason.contains("new message")));
    let transcript = h.transcript();
    let user_at = transcript
        .iter()
        .position(|message| message.content == "are you there?")
        .expect("the text was appended");
    let tool_at = transcript
        .iter()
        .position(|message| message.role == Role::Tool)
        .expect("the rejection was recorded");
    assert!(tool_at < user_at, "the rejection precedes the new message");
}

#[tokio::test]
async fn a_sibling_that_needs_no_approval_is_not_offered_while_the_turn_is_paused() {
    let probe = Probe::new();
    let tools = vec![
        FunctionTool::new("diagnostics").probed(probe.clone()).arc(),
        FunctionTool::new("write")
            .needs_approval()
            .probed(probe.clone())
            .arc(),
    ];
    let model = StubModel::saying("done");
    let h = harness(model, tools, LoopConfig::default());
    let seeded =
        Message::assistant_with_tool_calls("check", vec![call("diagnostics"), call("write")]);
    let write_id = seeded.tool_calls[1].id;
    h.store.append(h.session, seeded).await.unwrap();

    assert_eq!(
        h.agent.resume(h.session, no_cancel()).await,
        TurnOutcome::Paused
    );
    assert!(
        !probe.ran("diagnostics"),
        "nothing runs while a call is undecided"
    );
    assert!(!probe.ran("write"));

    let calls = h.calls();
    assert_eq!(calls[0].approval_status, ApprovalStatus::Approved);
    assert_eq!(calls[0].execution_status, ExecutionStatus::NotStarted);
    assert_eq!(calls[1].approval_status, ApprovalStatus::Pending);

    let offered: Vec<_> = h
        .sink
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::AwaitingApproval { call, .. } => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(offered, vec![write_id]);
}

#[tokio::test]
async fn deciding_is_whole_turn_so_nothing_runs_when_a_late_call_waits() {
    let probe = Probe::new();
    let tools = vec![
        FunctionTool::new("read").probed(probe.clone()).arc(),
        FunctionTool::new("write")
            .needs_approval()
            .probed(probe.clone())
            .arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("read"), call("write")])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Paused);
    assert!(
        !probe.ran("read"),
        "the approved call must not run before the undecided one is settled"
    );
    assert!(
        !h.transcript()
            .iter()
            .any(|message| message.role == Role::Tool),
        "no side effect landed"
    );
}

// ---------------------------------------------------------------------------
// Resume semantics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resuming_with_nothing_outstanding_is_a_no_op() {
    let model = StubModel::saying("hello");
    let h = harness(model, vec![], LoopConfig::default());

    h.agent.user_input(h.session, "hi", no_cancel()).await;
    let before = h.transcript().len();

    let outcome = h.agent.resume(h.session, no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(h.transcript().len(), before);
    assert_eq!(h.model.call_count(), 1, "no extra model turn was taken");
}

// ---------------------------------------------------------------------------
// Concurrency and ordering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_exclusive_call_is_a_barrier() {
    let probe = Probe::new();
    let slow = Duration::from_millis(20);
    let tools = vec![
        FunctionTool::new("read-a")
            .slow(slow)
            .probed(probe.clone())
            .arc(),
        FunctionTool::new("read-c")
            .slow(slow)
            .probed(probe.clone())
            .arc(),
        FunctionTool::new("write")
            .exclusive()
            .slow(slow)
            .probed(probe.clone())
            .arc(),
        FunctionTool::new("read-d")
            .slow(slow)
            .probed(probe.clone())
            .arc(),
        FunctionTool::new("read-e")
            .slow(slow)
            .probed(probe.clone())
            .arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![
            call("read-a"),
            call("read-c"),
            call("write"),
            call("read-d"),
            call("read-e"),
        ])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(
        probe.exclusive_overlaps(),
        0,
        "the exclusive call overlapped nothing"
    );
    assert_eq!(
        probe.max_inflight(),
        2,
        "the two reads before the barrier overlapped, and nothing crossed it"
    );
}

#[tokio::test]
async fn execution_follows_model_order_around_an_exclusive_call() {
    let probe = Probe::new();
    let gap = Duration::from_millis(15);
    let tools = vec![
        FunctionTool::new("a").slow(gap).probed(probe.clone()).arc(),
        FunctionTool::new("b")
            .exclusive()
            .slow(gap)
            .probed(probe.clone())
            .arc(),
        FunctionTool::new("c").slow(gap).probed(probe.clone()).arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("a"), call("b"), call("c")])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(
        probe.ends(),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        "execution order matches model order"
    );
    assert_eq!(
        probe.starts(),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        "and so does start order"
    );
    assert_eq!(h.result_names(), vec!["a", "b", "c"]);
}

#[tokio::test]
async fn reads_before_an_exclusive_call_still_batch() {
    let probe = Probe::new();
    let gap = Duration::from_millis(20);
    let tools = vec![
        FunctionTool::new("a").slow(gap).probed(probe.clone()).arc(),
        FunctionTool::new("c").slow(gap).probed(probe.clone()).arc(),
        FunctionTool::new("b")
            .exclusive()
            .slow(gap)
            .probed(probe.clone())
            .arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("a"), call("c"), call("b")])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(probe.max_inflight(), 2, "a and c overlapped");
    assert_eq!(h.result_names(), vec!["a", "c", "b"]);
}

#[tokio::test]
async fn the_in_flight_limit_is_respected() {
    let probe = Probe::new();
    let gap = Duration::from_millis(20);
    let tools = (0..4)
        .map(|index| {
            FunctionTool::new(format!("t{index}"))
                .slow(gap)
                .probed(probe.clone())
                .arc()
        })
        .collect();
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![
            call("t0"),
            call("t1"),
            call("t2"),
            call("t3"),
        ])),
        Script::Message(Message::assistant("done")),
    ]);
    let config = LoopConfig::default().with_max_concurrent_tools(2);
    let h = harness(model, tools, config);

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(probe.max_inflight(), 2, "never more than the limit");
}

#[tokio::test]
async fn results_are_in_model_order_when_the_first_tool_is_slowest() {
    let tools = vec![
        FunctionTool::new("slow")
            .slow(Duration::from_millis(40))
            .arc(),
        FunctionTool::new("fast")
            .slow(Duration::from_millis(1))
            .arc(),
    ];
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![call("slow"), call("fast")])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(
        h.result_names(),
        vec!["slow", "fast"],
        "results are committed in model order, not completion order"
    );
}

#[tokio::test]
async fn serial_mode_produces_the_same_transcript_as_concurrent_mode() {
    async fn run(config: LoopConfig) -> Vec<(Role, String)> {
        let tools = vec![
            FunctionTool::new("a").slow(Duration::from_millis(5)).arc(),
            FunctionTool::new("b")
                .exclusive()
                .slow(Duration::from_millis(5))
                .arc(),
            FunctionTool::new("c").slow(Duration::from_millis(5)).arc(),
        ];
        let model = StubModel::new(vec![
            Script::Message(assistant_asking_for(vec![call("a"), call("b"), call("c")])),
            Script::Message(Message::assistant("done")),
        ]);
        let h = harness(model, tools, config);
        h.agent.user_input(h.session, "go", no_cancel()).await;
        h.projection()
    }

    let concurrent = run(LoopConfig::default()).await;
    let serial = run(LoopConfig::serial()).await;

    assert_eq!(
        concurrent, serial,
        "serial mode changes only the parallelism, not the transcript"
    );
}

#[tokio::test]
async fn serial_mode_runs_one_call_at_a_time() {
    let probe = Probe::new();
    let tools = (0..3)
        .map(|index| {
            FunctionTool::new(format!("t{index}"))
                .slow(Duration::from_millis(10))
                .probed(probe.clone())
                .arc()
        })
        .collect();
    let model = StubModel::new(vec![
        Script::Message(assistant_asking_for(vec![
            call("t0"),
            call("t1"),
            call("t2"),
        ])),
        Script::Message(Message::assistant("done")),
    ]);
    let h = harness(model, tools, LoopConfig::serial());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    assert_eq!(probe.max_inflight(), 1);
    assert_eq!(
        probe.ends(),
        vec!["t0".to_owned(), "t1".to_owned(), "t2".to_owned()],
        "and still in model order"
    );
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancellation_before_the_model_turn_does_not_start_one() {
    let model = StubModel::saying("hello");
    let h = harness(model, vec![], LoopConfig::default());
    let cancel = CancellationToken::new();
    cancel.cancel();

    let outcome = h.agent.user_input(h.session, "go", cancel).await;

    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert_eq!(h.model.call_count(), 0);
    assert!(h
        .transcript()
        .iter()
        .all(|message| message.role != Role::Assistant));
    // The transcript is resolvable: a later resume finds nothing outstanding.
    assert!(unresolved_turn(&h.transcript()).is_none());
}

#[tokio::test]
async fn cancellation_mid_stream_does_not_append_a_partial_message() {
    let cancel = CancellationToken::new();
    let timeline = Arc::new(Timeline::default());
    let sink = Arc::new(RecordingSink::new(timeline).cancelling_on_delta(cancel.clone()));
    let model = StubModel::new(vec![Script::Deltas {
        deltas: vec![
            Delta::Text("part".to_owned()),
            Delta::Text("never sent".to_owned()),
        ],
        gap: Duration::from_millis(50),
    }]);
    let h = build(model, vec![], LoopConfig::default(), sink);

    let outcome = h.agent.user_input(h.session, "go", cancel).await;

    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert!(h.sink.delta_count() >= 1, "a delta did arrive first");
    assert!(
        h.transcript()
            .iter()
            .all(|message| message.role != Role::Assistant),
        "a half-written assistant message must not be committed"
    );
    assert!(unresolved_turn(&h.transcript()).is_none());
}

#[tokio::test]
async fn cancellation_mid_tool_records_the_call_as_cancelled() {
    let probe = Probe::new();
    let tools = vec![FunctionTool::new("slow")
        .slow(Duration::from_millis(500))
        .probed(probe.clone())
        .arc()];
    let model = asking_then(&["slow"], "done");
    let h = harness(model, tools, LoopConfig::default());

    let cancel = CancellationToken::new();
    let killer = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        killer.cancel();
    });

    let outcome = h.agent.user_input(h.session, "go", cancel).await;

    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert_eq!(h.calls()[0].execution_status, ExecutionStatus::Cancelled);
    assert!(
        unresolved_turn(&h.transcript()).is_none(),
        "the interrupted call is recorded, so the turn is resolvable"
    );
}

// ---------------------------------------------------------------------------
// Events and the registry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_event_follows_the_write_it_describes() {
    let tools = vec![FunctionTool::new("read").arc()];
    let model = asking_then(&["read"], "done");
    let h = harness(model, tools, LoopConfig::default());

    h.agent.user_input(h.session, "go", no_cancel()).await;

    h.timeline.assert_events_follow_writes();
}

#[tokio::test]
async fn a_turn_reports_its_start_and_finish() {
    let model = StubModel::saying("hello");
    let h = harness(model, vec![], LoopConfig::default());

    h.agent.user_input(h.session, "hi", no_cancel()).await;

    assert_eq!(
        h.sink
            .count_where(|e| matches!(e, Event::TurnStarted { .. })),
        1
    );
    assert_eq!(
        h.sink.outcomes(),
        vec![TurnOutcome::Complete],
        "the outcome is reported once"
    );
}

struct ReportingTool;

#[async_trait]
impl Tool for ReportingTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "reports a child step"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn requires_approval(&self, _args: &serde_json::Value) -> crate::tool::ApprovalDecision {
        crate::tool::ApprovalDecision::AllowImmediately
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        run: ToolRun,
    ) -> Result<serde_json::Value, ToolError> {
        run.report
            .subagent(SubagentSnapshot {
                mode: SubagentMode::Explore,
                description: "find resume".into(),
                started_ms: 1,
                steps: vec![SubagentStep {
                    name: "grep".into(),
                    target: "resume".into(),
                    status: SubagentStepStatus::Running,
                }],
            })
            .await;
        run.report
            .subagent(SubagentSnapshot {
                mode: SubagentMode::Explore,
                description: "find resume".into(),
                started_ms: 1,
                steps: vec![SubagentStep {
                    name: "grep".into(),
                    target: "resume".into(),
                    status: SubagentStepStatus::Ok,
                }],
            })
            .await;
        Ok(serde_json::json!({
            "mode": "explore",
            "answer": "found it",
            "tool_calls": 1,
            "denied": []
        }))
    }
}

#[tokio::test]
async fn a_reporter_records_child_steps_and_the_model_does_not_see_them() {
    let h = harness(
        StubModel::new(vec![
            Script::Message(assistant_asking_for(vec![ToolCall::new(
                "delegate",
                serde_json::json!({}),
            )
            .approved()])),
            Script::Message(Message::assistant("done")),
        ]),
        vec![Arc::new(ReportingTool)],
        LoopConfig::default(),
    );

    let outcome = h.agent.user_input(h.session, "go", no_cancel()).await;
    assert_eq!(outcome, TurnOutcome::Complete);

    let assistant = h
        .transcript()
        .into_iter()
        .find(|message| message.has_tool_calls())
        .expect("the assistant asked for a tool");
    let call = &assistant.tool_calls[0];
    let snapshot = call.subagent.as_ref().expect("the child steps were stored");
    assert_eq!(snapshot.steps.len(), 1);
    assert_eq!(snapshot.steps[0].target, "resume");
    assert_eq!(snapshot.steps[0].status, SubagentStepStatus::Ok);

    let tool_message = h
        .transcript()
        .into_iter()
        .find(|message| message.role == Role::Tool)
        .expect("the model receives a tool result");
    assert!(tool_message.content.contains("found it"));
    assert!(
        !tool_message.content.contains("resume"),
        "the tool result is the summary, not the child steps: {}",
        tool_message.content
    );
    assert!(
        h.sink
            .count_where(|event| matches!(event, Event::ToolCallUpdated { .. }))
            >= 2,
        "a step is published before the call finishes"
    );
}

#[tokio::test]
async fn a_tool_registered_after_the_agent_is_built_is_the_one_execute_finds() {
    let timeline = Arc::new(Timeline::default());
    let sink = Arc::new(RecordingSink::new(timeline.clone()));
    let store = Arc::new(InMemoryStore::new(timeline));
    let registry = Arc::new(ToolRegistry::new());
    let probe = Probe::new();
    let agent = Agent::new(
        store,
        sink,
        Arc::new(asking_then(&["later"], "done")),
        Arc::clone(&registry),
        LoopConfig::default(),
    );
    let session = agent.new_chat(WorkspaceId::new());
    registry
        .register(FunctionTool::new("later").probed(probe.clone()).arc())
        .expect("a late registration succeeds");

    assert_eq!(
        agent.user_input(session, "go", no_cancel()).await,
        TurnOutcome::Complete
    );
    assert!(probe.ran("later"));
    assert!(registry.remove("later"));
    assert!(registry.get("later").is_none());
    assert!(!registry.remove("later"));
}

#[test]
fn the_registry_refuses_a_duplicate_name() {
    let registry = ToolRegistry::new();
    registry
        .register(FunctionTool::new("read").arc())
        .expect("the first registration succeeds");

    let error = registry
        .register(FunctionTool::new("read").arc())
        .expect_err("the second is refused");

    assert_eq!(error, RegistryError::DuplicateName("read".to_owned()));
}

#[test]
fn the_registry_lists_tools_by_name_and_states_every_concurrency() {
    // The pattern the real tool set adopts at M3: the snapshot is asserted
    // exactly, so adding a tool fails this test until its author states a
    // setting. A defaulted `Concurrent` is the omission this catches.
    let registry = ToolRegistry::new();
    for tool in [
        FunctionTool::new("read").arc(),
        FunctionTool::new("grep").arc(),
        FunctionTool::new("write").exclusive().arc(),
    ] {
        registry.register(tool).unwrap();
    }

    assert_eq!(registry.names(), vec!["grep", "read", "write"]);

    let snapshot = registry.concurrency_snapshot();
    assert_eq!(snapshot.get("read"), Some(&Concurrency::Concurrent));
    assert_eq!(snapshot.get("grep"), Some(&Concurrency::Concurrent));
    assert_eq!(snapshot.get("write"), Some(&Concurrency::Exclusive));
    assert_eq!(snapshot.len(), 3, "every tool states a concurrency");
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

/// Records every `compact` call and returns a scripted result.
struct StubCompactor {
    calls: Mutex<Vec<(SessionId, Option<u64>, CompactTrigger)>>,
    fail: bool,
}

impl StubCompactor {
    fn new(fail: bool) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail,
        }
    }

    fn triggers(&self) -> Vec<CompactTrigger> {
        self.calls
            .lock()
            .expect("compactor calls")
            .iter()
            .map(|(_, _, trigger)| *trigger)
            .collect()
    }
}

#[async_trait]
impl Compactor for StubCompactor {
    async fn compact(
        &self,
        session: SessionId,
        window: Option<u64>,
        trigger: CompactTrigger,
        _cancel: &CancellationToken,
    ) -> Result<CompactOutcome, CompactError> {
        self.calls
            .lock()
            .expect("compactor calls")
            .push((session, window, trigger));
        if self.fail {
            Err(CompactError("the summary call failed".into()))
        } else {
            Ok(CompactOutcome::BelowThreshold)
        }
    }
}

fn agent_with_compactor(compactor: Arc<dyn Compactor>) -> (Agent, Arc<InMemoryStore>, SessionId) {
    let timeline = Arc::new(Timeline::default());
    let sink = Arc::new(RecordingSink::new(timeline.clone()));
    let store = Arc::new(InMemoryStore::new(timeline));
    let agent = Agent::new(
        store.clone(),
        sink,
        Arc::new(StubModel::saying("done")),
        Arc::new(ToolRegistry::new()),
        LoopConfig::default(),
    )
    .with_compactor(compactor);
    let session = agent.new_chat(WorkspaceId::new());
    (agent, store, session)
}

#[tokio::test]
async fn auto_compaction_runs_once_at_the_start_of_a_user_turn() {
    let compactor = Arc::new(StubCompactor::new(false));
    let (agent, _store, session) = agent_with_compactor(compactor.clone());

    let outcome = agent.user_input(session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(compactor.triggers(), vec![CompactTrigger::Auto]);
    assert_eq!(compactor.calls.lock().unwrap()[0].0, session);
}

#[tokio::test]
async fn a_failing_auto_compaction_still_runs_the_turn() {
    let compactor = Arc::new(StubCompactor::new(true));
    let (agent, store, session) = agent_with_compactor(compactor.clone());

    let outcome = agent.user_input(session, "go", no_cancel()).await;

    assert_eq!(outcome, TurnOutcome::Complete);
    assert_eq!(compactor.triggers(), vec![CompactTrigger::Auto]);
    // The user message and the assistant reply are both stored.
    assert_eq!(store.transcript(session).len(), 2);
}

#[tokio::test]
async fn resume_does_not_compact() {
    let compactor = Arc::new(StubCompactor::new(false));
    let (agent, _store, session) = agent_with_compactor(compactor.clone());

    agent.user_input(session, "go", no_cancel()).await;
    agent.resume(session, no_cancel()).await;

    assert_eq!(
        compactor.triggers(),
        vec![CompactTrigger::Auto],
        "compaction happens on a user turn, not on a resume"
    );
}

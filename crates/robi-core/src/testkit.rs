//! Test doubles for the loop.
//!
//! M0 has no provider and no store, so these are what prove the loop. They are
//! compiled only for tests.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::{ModelError, StoreError, ToolError};
use crate::event::{Event, EventSink};
use crate::ids::{MessageId, SessionId, WorkspaceId};
use crate::message::Message;
use crate::model::{Delta, Model, ModelStream};
use crate::store::MessageStore;
use crate::tool::{Concurrency, ApprovalDecision, Tool};

// ---------------------------------------------------------------------------
// Timeline: a shared record of writes and emits, to check their order.
// ---------------------------------------------------------------------------

/// One thing that happened, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Append(MessageId),
    Update(MessageId),
    Emit {
        kind: &'static str,
        message: Option<MessageId>,
    },
}

/// Where a store write and an event emit are recorded together.
#[derive(Default, Debug)]
pub struct Timeline {
    steps: Mutex<Vec<Step>>,
}

impl Timeline {
    pub fn push(&self, step: Step) {
        self.steps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(step);
    }

    pub fn steps(&self) -> Vec<Step> {
        self.steps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every `message_added` or `message_updated` event must come after the write
    /// it describes, so a consumer that reloads on the event sees the message.
    ///
    /// Delta events are excluded: they describe a message that does not exist yet,
    /// which is the point of them.
    pub fn assert_events_follow_writes(&self) {
        let mut written: Vec<MessageId> = Vec::new();
        for step in self.steps() {
            match step {
                Step::Append(id) | Step::Update(id) => written.push(id),
                Step::Emit {
                    kind,
                    message: Some(id),
                } if matches!(kind, "message_added" | "message_updated") => {
                    assert!(
                        written.contains(&id),
                        "the event {kind} for {id} was emitted before the write for it",
                    );
                }
                Step::Emit { .. } => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// A transcript in memory, recording every write to a timeline.
pub struct InMemoryStore {
    sessions: Mutex<HashMap<SessionId, Vec<Message>>>,
    timeline: Arc<Timeline>,
}

impl InMemoryStore {
    pub fn new(timeline: Arc<Timeline>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            timeline,
        }
    }

    pub fn transcript(&self, session: SessionId) -> Vec<Message> {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&session)
            .cloned()
            .unwrap_or_default()
    }

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
impl MessageStore for InMemoryStore {
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

    async fn append(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        let id = message.id;
        self.with_session(session, |messages| messages.push(message))?;
        self.timeline.push(Step::Append(id));
        Ok(())
    }

    async fn update(&self, session: SessionId, message: Message) -> Result<(), StoreError> {
        let id = message.id;
        self.with_session(session, |messages| {
            match messages
                .iter_mut()
                .find(|existing| existing.id == message.id)
            {
                Some(existing) => *existing = message,
                None => messages.push(message),
            }
        })?;
        self.timeline.push(Step::Update(id));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Event sink
// ---------------------------------------------------------------------------

/// Records every event, and can cancel a token on the first delta.
pub struct RecordingSink {
    events: Mutex<Vec<Event>>,
    timeline: Arc<Timeline>,
    cancel_on_delta: Option<CancellationToken>,
}

impl RecordingSink {
    pub fn new(timeline: Arc<Timeline>) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            timeline,
            cancel_on_delta: None,
        }
    }

    /// Cancel this token as soon as a partial message arrives, to test a cancel
    /// that lands mid-stream.
    pub fn cancelling_on_delta(mut self, cancel: CancellationToken) -> Self {
        self.cancel_on_delta = Some(cancel);
        self
    }

    pub fn events(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The timeline this sink records into, so a store can share it.
    pub fn timeline(&self) -> Arc<Timeline> {
        self.timeline.clone()
    }

    pub fn count_where(&self, predicate: impl Fn(&Event) -> bool) -> usize {
        self.events()
            .iter()
            .filter(|event| predicate(event))
            .count()
    }

    pub fn delta_count(&self) -> usize {
        self.count_where(|event| matches!(event, Event::MessageDelta { .. }))
    }

    pub fn outcomes(&self) -> Vec<crate::error::TurnOutcome> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::TurnFinished { outcome, .. } => Some(outcome),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl EventSink for RecordingSink {
    async fn emit(&self, event: Event) {
        let (kind, message) = describe(&event);
        self.timeline.push(Step::Emit { kind, message });
        if matches!(event, Event::MessageDelta { .. }) {
            if let Some(cancel) = &self.cancel_on_delta {
                cancel.cancel();
            }
        }
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }
}

fn describe(event: &Event) -> (&'static str, Option<MessageId>) {
    match event {
        Event::TurnStarted { .. } => ("turn_started", None),
        Event::MessageAdded { message, .. } => ("message_added", Some(*message)),
        Event::MessageUpdated { message, .. } => ("message_updated", Some(*message)),
        Event::MessageDelta { message, .. } => ("message_delta", Some(*message)),
        Event::ToolCallUpdated { .. } => ("tool_call_updated", None),
        Event::AwaitingApproval { .. } => ("awaiting_approval", None),
        Event::TurnFinished { .. } => ("turn_finished", None),
    }
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// One scripted model turn.
pub enum Script {
    /// One finished assistant message.
    Message(Message),
    /// Deltas delivered in order, with a gap between them.
    Deltas { deltas: Vec<Delta>, gap: Duration },
    /// The provider fails before producing anything.
    Error(ModelError),
}

/// A model that replays a script, and records what it was shown.
pub struct StubModel {
    scripts: Mutex<VecDeque<Script>>,
    seen: Mutex<Vec<Vec<Message>>>,
}

impl StubModel {
    pub fn new(scripts: Vec<Script>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// A model that answers with plain text, once.
    pub fn saying(text: &str) -> Self {
        Self::new(vec![Script::Message(Message::assistant(text))])
    }

    /// The transcripts the loop handed the model, one per call.
    pub fn seen(&self) -> Vec<Vec<Message>> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn call_count(&self) -> usize {
        self.seen().len()
    }
}

#[async_trait]
impl Model for StubModel {
    async fn generate(
        &self,
        transcript: &[Message],
        _cancel: CancellationToken,
    ) -> Result<ModelStream, ModelError> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(transcript.to_vec());

        let script = self
            .scripts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| ModelError::Provider("the stub has no scripted turn left".to_owned()))?;

        match script {
            Script::Error(error) => Err(error),
            Script::Message(message) => {
                let (tx, rx) = mpsc::channel(1);
                tokio::spawn(async move {
                    let _ = tx.send(Delta::Finished(message)).await;
                });
                Ok(ModelStream::new(rx))
            }
            Script::Deltas { deltas, gap } => {
                let (tx, rx) = mpsc::channel(64);
                tokio::spawn(async move {
                    for delta in deltas {
                        if tx.send(delta).await.is_err() {
                            return;
                        }
                        if !gap.is_zero() {
                            tokio::time::sleep(gap).await;
                        }
                    }
                });
                Ok(ModelStream::new(rx))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// What a probe observed about overlapping calls.
#[derive(Default, Debug)]
pub struct Probe {
    inflight: AtomicUsize,
    max_inflight: AtomicUsize,
    exclusive_overlaps: AtomicUsize,
    starts: Mutex<Vec<String>>,
    ends: Mutex<Vec<String>>,
}

impl Probe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn enter(&self, name: &str, exclusive: bool) {
        let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        if exclusive && now > 1 {
            self.exclusive_overlaps.fetch_add(1, Ordering::SeqCst);
        }
        self.max_inflight.fetch_max(now, Ordering::SeqCst);
        self.starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
    }

    fn leave(&self, name: &str) {
        self.inflight.fetch_sub(1, Ordering::SeqCst);
        self.ends
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
    }

    /// The most calls ever in flight at once.
    pub fn max_inflight(&self) -> usize {
        self.max_inflight.load(Ordering::SeqCst)
    }

    /// Times an exclusive call started while another call was in flight.
    pub fn exclusive_overlaps(&self) -> usize {
        self.exclusive_overlaps.load(Ordering::SeqCst)
    }

    pub fn starts(&self) -> Vec<String> {
        self.starts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn ends(&self) -> Vec<String> {
        self.ends
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn ran(&self, name: &str) -> bool {
        self.ends().iter().any(|ended| ended == name)
    }
}

/// How a `FunctionTool` answers.
enum Behavior {
    Return(serde_json::Value),
    Fail(ToolError),
    Panic,
}

/// A configurable tool, covering every failure mode the tests need.
pub struct FunctionTool {
    name: String,
    description: String,
    concurrency: Concurrency,
    decision: ApprovalDecision,
    delay: Duration,
    probe: Option<Arc<Probe>>,
    behavior: Behavior,
}

impl FunctionTool {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: "a test tool".to_owned(),
            concurrency: Concurrency::Concurrent,
            decision: ApprovalDecision::AllowImmediately,
            delay: Duration::ZERO,
            probe: None,
            behavior: Behavior::Return(serde_json::json!({"ok": true})),
        }
    }

    pub fn returns(mut self, value: serde_json::Value) -> Self {
        self.behavior = Behavior::Return(value);
        self
    }

    pub fn fails(mut self, error: ToolError) -> Self {
        self.behavior = Behavior::Fail(error);
        self
    }

    pub fn panics(mut self) -> Self {
        self.behavior = Behavior::Panic;
        self
    }

    pub fn exclusive(mut self) -> Self {
        self.concurrency = Concurrency::Exclusive;
        self
    }

    pub fn needs_approval(mut self) -> Self {
        self.decision = ApprovalDecision::NeedsApproval;
        self
    }

    pub fn slow(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn probed(mut self, probe: Arc<Probe>) -> Self {
        self.probe = Some(probe);
        self
    }

    pub fn arc(self) -> Arc<dyn Tool> {
        Arc::new(self)
    }
}

#[async_trait]
impl Tool for FunctionTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    fn concurrency(&self) -> Concurrency {
        self.concurrency
    }

    async fn requires_approval(&self, _args: &serde_json::Value) -> ApprovalDecision {
        self.decision
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<serde_json::Value, ToolError> {
        let exclusive = self.concurrency.is_exclusive();
        if let Some(probe) = &self.probe {
            probe.enter(&self.name, exclusive);
        }

        if !self.delay.is_zero() {
            // A cooperating tool gives up when cancelled, which is what makes a
            // cancelled call resolvable.
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    if let Some(probe) = &self.probe {
                        probe.leave(&self.name);
                    }
                    return Err(ToolError::Cancelled);
                }
                () = tokio::time::sleep(self.delay) => {}
            }
        }

        if let Some(probe) = &self.probe {
            probe.leave(&self.name);
        }

        match &self.behavior {
            Behavior::Return(value) => Ok(value.clone()),
            Behavior::Fail(error) => Err(error.clone()),
            Behavior::Panic => panic!("a tool panicked"),
        }
    }
}

/// A tool that returns an oversized result, to trip the core's backstop.
pub fn huge_result(bytes: usize) -> serde_json::Value {
    serde_json::json!({ "blob": "x".repeat(bytes) })
}

/// An assistant message that asks for these calls.
pub fn assistant_asking_for(calls: Vec<crate::message::ToolCall>) -> Message {
    Message::assistant_with_tool_calls("", calls)
}

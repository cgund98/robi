//! Summarize an older prefix and rewrite the transcript.
//!
//! The loop decides *when* to compact (see `robi_core::compact`). This is the
//! I/O side: one model call with no tools, then a transactional rewrite.
//! Specified in `docs/src/design/workspace/context-management.md`.

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::compact::{
    over_auto_threshold, plan_cut, plan_last_turn, CompactError, CompactOutcome, CompactTrigger,
    Compactor, Cut,
};
use robi_core::event::{Event, EventSink};
use robi_core::ids::{MessageId, SessionId};
use robi_core::message::{Message, Role};
use robi_core::model::{Delta, Model};
use robi_core::store::MessageStore;
use robi_core::tool::ToolRegistry;
use tokio_util::sync::CancellationToken;

use crate::agent::tools::ChildModels;
use crate::domain::events::{EventBus, EventEnvelope};

/// A short instruction that the model continues a coding session from the
/// summary. The system prompt and the plan checklist are rebuilt every request,
/// so they are not in the prefix and not summarized.
const SUMMARY_SYSTEM_PROMPT: &str = "\
You summarize a coding session so the next turn can continue from your summary. \
Cover, in order: decisions made, files read or changed, tool outcomes that still \
matter, and constraints the user set. Be specific: name paths, identifiers, and \
commands. Do not include a preamble or a closing remark.";

/// Headroom left under the summary model's window for the reply. The rendered
/// prefix sits below the window so the model can still answer.
const SUMMARY_REPLY_HEADROOM_TOKENS: u64 = 8_192;

/// Cap the rendered prefix when the model advertises no window.
const DEFAULT_RENDER_CAP_CHARS: usize = 200_000;

/// Why a manual compact cannot start, decided before the actor is spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactRefusal {
    /// The transcript is paused on a tool decision.
    AwaitingApproval,
    /// There is no older prefix to summarize.
    Nothing,
    /// The transcript could not be read.
    Unavailable,
}

impl CompactRefusal {
    /// The `409` message for a refusal, or `None` when it is not a conflict.
    pub fn conflict_message(self) -> Option<&'static str> {
        match self {
            CompactRefusal::AwaitingApproval => Some("chat session is awaiting approval"),
            CompactRefusal::Nothing => Some("nothing to compact"),
            CompactRefusal::Unavailable => None,
        }
    }
}

/// Summarizes an older prefix of one session's transcript.
pub struct SessionCompactor {
    session: SessionId,
    store: Arc<dyn MessageStore>,
    events: Arc<dyn EventSink>,
    bus: Arc<EventBus>,
    models: Arc<dyn ChildModels>,
}

impl SessionCompactor {
    pub fn new(
        session: SessionId,
        store: Arc<dyn MessageStore>,
        events: Arc<dyn EventSink>,
        bus: Arc<EventBus>,
        models: Arc<dyn ChildModels>,
    ) -> Self {
        Self {
            session,
            store,
            events,
            bus,
            models,
        }
    }

    /// Whether a manual compact can start, without spawning an actor.
    pub async fn feasibility(&self, window: Option<u64>) -> Result<(), CompactRefusal> {
        let messages = self
            .store
            .messages(self.session)
            .await
            .map_err(|_| CompactRefusal::Unavailable)?;
        if has_pending_approval(&messages) {
            return Err(CompactRefusal::AwaitingApproval);
        }
        if self.plan(&messages, window).is_none() {
            return Err(CompactRefusal::Nothing);
        }
        Ok(())
    }

    /// The cut for this transcript and window: `plan_cut` when the window is
    /// known, `plan_last_turn` when the manual trigger has no figure to target.
    fn plan(&self, messages: &[Message], window: Option<u64>) -> Option<Cut> {
        match window {
            Some(window) => plan_cut(messages, window),
            None => plan_last_turn(messages),
        }
    }
}

#[async_trait]
impl Compactor for SessionCompactor {
    async fn compact(
        &self,
        session: SessionId,
        window: Option<u64>,
        trigger: CompactTrigger,
        cancel: &CancellationToken,
    ) -> Result<CompactOutcome, CompactError> {
        let messages = self
            .store
            .messages(session)
            .await
            .map_err(|error| CompactError(error.to_string()))?;

        // A pending approval means the turn is mid-decision; wait for it.
        if has_pending_approval(&messages) {
            return Err(CompactError(
                "the transcript is waiting on approval".to_owned(),
            ));
        }

        // Auto only fires at the threshold. Manual ignores it.
        if trigger == CompactTrigger::Auto && !over_auto_threshold(&messages, window) {
            return Ok(CompactOutcome::BelowThreshold);
        }

        let Some(cut) = self.plan(&messages, window) else {
            return Ok(CompactOutcome::Nothing);
        };

        let model = self
            .models
            .build(
                Arc::new(ToolRegistry::new()),
                SUMMARY_SYSTEM_PROMPT.to_owned(),
            )
            .await
            .map_err(CompactError)?;

        let text = match summarize(model.as_ref(), &cut.prefix, window, cancel).await {
            Ok(text) => text,
            Err(error) => {
                // A failure leaves the transcript untouched and surfaces the
                // reason on the session error event.
                self.bus
                    .publish(EventEnvelope::user_error(error.to_string()));
                return Err(error);
            }
        };

        let summary = Message::summary(text);
        let summary_id = summary.id;
        let delete: Vec<MessageId> = cut.prefix.iter().map(|message| message.id).collect();
        self.store
            .replace_prefix(session, &delete, summary)
            .await
            .map_err(|error| {
                let error = CompactError(error.to_string());
                self.bus
                    .publish(EventEnvelope::user_error(error.to_string()));
                error
            })?;

        self.events
            .emit(Event::MessageAdded {
                session,
                message: summary_id,
            })
            .await;
        self.bus
            .publish(EventEnvelope::transcript_compacted(session, summary_id));

        Ok(CompactOutcome::Compacted {
            message: summary_id,
        })
    }
}

/// Whether any call is still waiting on a decision.
fn has_pending_approval(messages: &[Message]) -> bool {
    messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .any(|call| call.is_pending_approval() && call.needs_execution())
}

/// One model call, no tools, over the rendered prefix.
async fn summarize(
    model: &dyn Model,
    prefix: &[Message],
    window: Option<u64>,
    cancel: &CancellationToken,
) -> Result<String, CompactError> {
    let request = Message::user(render_prompt(prefix, window));
    let mut stream = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(CompactError("the summary call was cancelled".into())),
        stream = model.generate(SessionId::new(), std::slice::from_ref(&request), cancel.clone()) => {
            stream.map_err(|error| CompactError(error.to_string()))?
        }
    };

    let mut text = String::new();
    loop {
        let delta = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(CompactError("the summary call was cancelled".into())),
            delta = stream.next() => delta,
        };
        match delta {
            Some(Delta::Text(chunk)) => text.push_str(&chunk),
            Some(Delta::Finished(message)) => {
                text = message.content;
                break;
            }
            Some(Delta::Failed(error)) => return Err(CompactError(error.to_string())),
            Some(_) => {}
            None => break,
        }
    }

    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(CompactError("the summary call returned nothing".into()));
    }
    Ok(text)
}

/// The prefix as `role: content` lines, then the instruction. Bounded so a
/// full prefix cannot exceed the summary model's window.
fn render_prompt(prefix: &[Message], window: Option<u64>) -> String {
    let mut body = render_prefix(prefix, window);
    body.push_str(
        "\nSummarize the session above so the next turn can continue from it. \
         Reply with the summary and nothing else.",
    );
    body
}

fn render_prefix(prefix: &[Message], window: Option<u64>) -> String {
    let cap = render_cap(window);
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut dropped = 0usize;
    // Keep the newest messages whole; drop the oldest when the cap is reached.
    for (index, message) in prefix.iter().rev().enumerate() {
        let line = render_line(message);
        if used + line.len() > cap {
            dropped = prefix.len() - index;
            break;
        }
        used += line.len();
        kept.push(line);
    }
    kept.reverse();

    let mut body = String::new();
    if dropped > 0 {
        body.push_str(&format!("[{dropped} earlier messages omitted]\n"));
    }
    for line in kept {
        body.push_str(&line);
    }
    body
}

fn render_line(message: &Message) -> String {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut line = format!("{role}: {}\n", message.content);
    for call in &message.tool_calls {
        if let Ok(args) = serde_json::to_string(&call.args) {
            line.push_str(&format!("tool_call {} {args}\n", call.name));
        }
    }
    line
}

/// Characters the rendered prefix may use, leaving headroom for the reply.
fn render_cap(window: Option<u64>) -> usize {
    match window {
        Some(window) => {
            let chars = window.saturating_mul(4);
            let headroom = SUMMARY_REPLY_HEADROOM_TOKENS.saturating_mul(4);
            // Never drop more than half the window's worth of characters, so a
            // small advertised window still renders a usable prefix.
            usize::try_from(chars.saturating_sub(headroom))
                .unwrap_or(usize::MAX)
                .max(usize::try_from(chars / 2).unwrap_or(0))
        }
        None => DEFAULT_RENDER_CAP_CHARS,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use robi_core::error::ModelError;
    use robi_core::event::NopSink;
    use robi_core::message::ToolCall;
    use robi_core::model::{Model, ModelStream};
    use robi_core::store::MessageStore;
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;
    use crate::agent::tools::memory_store::MemoryStore;

    struct ScriptModel {
        reply: Mutex<String>,
        fail: bool,
        seen: Mutex<Vec<String>>,
    }

    impl ScriptModel {
        fn replying(reply: &str) -> Self {
            Self {
                reply: Mutex::new(reply.to_owned()),
                fail: false,
                seen: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                reply: Mutex::new(String::new()),
                fail: true,
                seen: Mutex::new(Vec::new()),
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.seen.lock().expect("seen").clone()
        }
    }

    #[async_trait]
    impl Model for ScriptModel {
        async fn generate(
            &self,
            _session: SessionId,
            transcript: &[Message],
            _cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            self.seen.lock().expect("seen").push(
                transcript
                    .iter()
                    .map(|message| message.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            if self.fail {
                return Err(ModelError::Provider("the provider failed".into()));
            }
            let (tx, rx) = mpsc::channel(1);
            let reply = self.reply.lock().expect("reply").clone();
            tokio::spawn(async move {
                let _ = tx.send(Delta::Finished(Message::assistant(reply))).await;
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

    fn seeded(_prefix_turns: usize) -> (Arc<MemoryStore>, SessionId) {
        let store = Arc::new(MemoryStore::default());
        let session = store.create_session(robi_core::ids::WorkspaceId::new());
        (store, session)
    }

    /// An in-memory session with `turns` pairs of 400-char messages already in it.
    async fn seeded_async(prefix_turns: usize) -> (Arc<MemoryStore>, SessionId) {
        let (store, session) = seeded(prefix_turns);
        let block = "x".repeat(400);
        for _ in 0..prefix_turns {
            store
                .append(session, Message::user(block.clone()))
                .await
                .unwrap();
            store
                .append(session, Message::assistant(block.clone()))
                .await
                .unwrap();
        }
        (store, session)
    }

    fn bus_with_error() -> (Arc<EventBus>, crate::domain::events::EventSubscription) {
        let bus = Arc::new(EventBus::new());
        let subscription = bus.subscribe();
        (bus, subscription)
    }

    #[tokio::test]
    async fn a_manual_compact_summarizes_the_prefix_and_rewrites_the_transcript() {
        let (store, session) = seeded_async(3).await;
        let (bus, mut subscription) = bus_with_error();
        let model = Arc::new(ScriptModel::replying("the summary"));
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels {
                model: model.clone(),
            }),
        );

        // window 500 tokens: half is 250, each turn is ~200 tokens, so the
        // newest turn is kept and the two older turns are summarized.
        let outcome = compactor
            .compact(
                session,
                Some(500),
                CompactTrigger::Manual,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let CompactOutcome::Compacted { message } = outcome else {
            panic!("a compact happened: {outcome:?}");
        };

        let messages = store.messages(session).await.unwrap();
        assert_eq!(messages[0].id, message);
        assert!(messages[0].compaction, "the summary is marked");
        assert_eq!(messages[0].content, "the summary");
        assert_eq!(messages.len(), 3, "summary + the kept turn");

        let envelope = subscription.recv().await.unwrap();
        assert_eq!(
            envelope.event_type,
            crate::domain::events::TRANSCRIPT_COMPACTED
        );
        assert_eq!(envelope.data["message_id"], message.to_string());

        // The prompt showed the older turns.
        assert!(model.prompts()[0].contains(&"x".repeat(400)));
    }

    #[tokio::test]
    async fn auto_skips_below_the_threshold() {
        let (store, session) = seeded_async(2).await;
        let bus = Arc::new(EventBus::new());
        let model = Arc::new(ScriptModel::replying("unused"));
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels {
                model: model.clone(),
            }),
        );

        let outcome = compactor
            .compact(
                session,
                Some(10_000_000),
                CompactTrigger::Auto,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, CompactOutcome::BelowThreshold);
        assert!(
            model.prompts().is_empty(),
            "no model call below the threshold"
        );
        assert_eq!(store.messages(session).await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn a_summary_failure_leaves_the_transcript_unchanged() {
        let (store, session) = seeded_async(3).await;
        let (bus, mut subscription) = bus_with_error();
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels {
                model: Arc::new(ScriptModel::failing()),
            }),
        );

        let error = compactor
            .compact(
                session,
                Some(500),
                CompactTrigger::Manual,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("the provider failed"));
        assert_eq!(store.messages(session).await.unwrap().len(), 6);

        let envelope = subscription.recv().await.unwrap();
        assert_eq!(envelope.event_type, crate::domain::events::APP_ERROR);
    }

    #[tokio::test]
    async fn a_manual_compact_without_a_window_keeps_the_current_turn() {
        let (store, session) = seeded_async(2).await;
        let bus = Arc::new(EventBus::new());
        let model = Arc::new(ScriptModel::replying("the summary"));
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels { model }),
        );

        compactor
            .compact(
                session,
                None,
                CompactTrigger::Manual,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let messages = store.messages(session).await.unwrap();
        assert_eq!(messages.len(), 3, "summary + the last turn");
        assert!(messages[0].compaction);
    }

    #[tokio::test]
    async fn nothing_to_compact_is_reported() {
        // One turn only: the prefix is empty.
        let (store, session) = seeded_async(1).await;
        let bus = Arc::new(EventBus::new());
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels {
                model: Arc::new(ScriptModel::replying("unused")),
            }),
        );

        let outcome = compactor
            .compact(
                session,
                Some(10),
                CompactTrigger::Manual,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, CompactOutcome::Nothing);
        assert_eq!(
            compactor.feasibility(Some(10)).await,
            Err(CompactRefusal::Nothing)
        );
    }

    #[tokio::test]
    async fn a_pending_approval_blocks_compaction() {
        let store = Arc::new(MemoryStore::default());
        let session = store.create_session(robi_core::ids::WorkspaceId::new());
        store.append(session, Message::user("hi")).await.unwrap();
        store
            .append(
                session,
                Message::assistant_with_tool_calls("", vec![ToolCall::new("read_file", json!({}))]),
            )
            .await
            .unwrap();
        store.append(session, Message::user("next")).await.unwrap();
        let bus = Arc::new(EventBus::new());
        let compactor = SessionCompactor::new(
            session,
            store.clone(),
            Arc::new(NopSink),
            bus,
            Arc::new(FixedModels {
                model: Arc::new(ScriptModel::replying("unused")),
            }),
        );

        assert_eq!(
            compactor.feasibility(Some(10)).await,
            Err(CompactRefusal::AwaitingApproval)
        );
        assert!(compactor
            .compact(
                session,
                Some(10),
                CompactTrigger::Manual,
                &CancellationToken::new()
            )
            .await
            .is_err());
        assert_eq!(store.messages(session).await.unwrap().len(), 3);
    }
}

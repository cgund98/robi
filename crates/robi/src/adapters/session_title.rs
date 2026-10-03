//! Ask the model for a chat session title without writing that call into the transcript.

use std::sync::Arc;

use robi_core::ids::SessionId;
use robi_core::message::{Message, Role};
use robi_core::model::{Delta, Model};
use robi_core::store::MessageStore;
use tokio_util::sync::CancellationToken;

use crate::domain::{
    chat_session::{normalize_generated_title, service::ChatSessionService},
    events::{EventBus, EventEnvelope},
};

const EXCERPT_CHARS: usize = 1_500;

/// After a completed turn, name the session when its title is still null.
///
/// The model call uses a fresh session id so it does not join the chat's
/// provider conversation. The prompt and the reply are not appended.
pub async fn title_completed_turn(
    session: SessionId,
    model: Arc<dyn Model>,
    store: Arc<dyn MessageStore>,
    sessions: Arc<ChatSessionService>,
    bus: Option<Arc<EventBus>>,
) {
    let current = match sessions.get_chat_session(session).await {
        Ok(session) => session,
        Err(error) => {
            tracing::warn!(%session, %error, "session title skipped");
            return;
        }
    };
    if current.title.is_some() {
        return;
    }

    let messages = match store.messages(session).await {
        Ok(messages) => messages,
        Err(error) => {
            tracing::warn!(%session, %error, "session title skipped");
            return;
        }
    };

    let Some(title) = estimate_session_title(model.as_ref(), &messages).await else {
        return;
    };

    match sessions.set_title_if_unset(session, title.clone()).await {
        Ok(Some(_)) => {
            tracing::info!(%session, %title, "session title stored");
            if let Some(bus) = bus {
                bus.publish(EventEnvelope::session_updated(session));
            }
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%session, %error, "session title was not stored");
        }
    }
}

/// One model reply, reduced to a title. `None` when the transcript has no user
/// text, the call fails, or the reply is empty.
pub async fn estimate_session_title(model: &dyn Model, transcript: &[Message]) -> Option<String> {
    let request = title_request(transcript)?;
    let mut stream = match model
        .generate(
            SessionId::new(),
            std::slice::from_ref(&request),
            CancellationToken::new(),
        )
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(%error, "session title estimate failed");
            return None;
        }
    };

    let mut text = String::new();
    while let Some(delta) = stream.next().await {
        match delta {
            Delta::Text(chunk) => text.push_str(&chunk),
            Delta::Finished(message) => {
                text = message.content;
                break;
            }
            Delta::Failed(error) => {
                tracing::warn!(%error, "session title estimate failed");
                return None;
            }
            _ => {}
        }
    }
    normalize_generated_title(&text)
}

fn title_request(transcript: &[Message]) -> Option<Message> {
    let user = transcript
        .iter()
        .rev()
        .find(|message| message.role == Role::User && !message.content.trim().is_empty())?;
    let assistant = transcript
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant && !message.content.trim().is_empty());

    let mut body = String::from(
        "Name this conversation for a sidebar. Reply with a title of at most six words and nothing else.\n\nUser: ",
    );
    body.push_str(&excerpt(&user.content));
    if let Some(assistant) = assistant {
        body.push_str("\n\nAssistant: ");
        body.push_str(&excerpt(&assistant.content));
    }
    Some(Message::user(body))
}

fn excerpt(text: &str) -> String {
    text.chars().take(EXCERPT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use robi_core::error::ModelError;
    use robi_core::model::ModelStream;
    use tokio::sync::mpsc;

    use super::*;

    struct ScriptModel {
        reply: Mutex<String>,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Model for ScriptModel {
        async fn generate(
            &self,
            _session: SessionId,
            transcript: &[Message],
            _cancel: CancellationToken,
        ) -> Result<ModelStream, ModelError> {
            let prompt = transcript
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            self.seen.lock().expect("seen").push(prompt);
            let (tx, rx) = mpsc::channel(1);
            let reply = self.reply.lock().expect("reply").clone();
            let _ = tx.send(Delta::Finished(Message::assistant(reply))).await;
            Ok(ModelStream::new(rx))
        }
    }

    #[tokio::test]
    async fn estimate_uses_the_turn_and_normalizes_the_reply() {
        let model = ScriptModel {
            reply: Mutex::new("\"Parser cleanup.\"".into()),
            seen: Mutex::new(Vec::new()),
        };
        let transcript = vec![
            Message::user("rename the parser"),
            Message::assistant("I renamed the parser entry point."),
        ];

        let title = estimate_session_title(&model, &transcript).await.unwrap();
        assert_eq!(title, "Parser cleanup");
        let seen = model.seen.lock().expect("seen");
        assert_eq!(seen.len(), 1);
        assert!(seen[0].contains("rename the parser"));
        assert!(seen[0].contains("I renamed the parser entry point."));
    }

    #[tokio::test]
    async fn estimate_skips_a_transcript_with_no_user_text() {
        let model = ScriptModel {
            reply: Mutex::new("unused".into()),
            seen: Mutex::new(Vec::new()),
        };
        assert!(estimate_session_title(&model, &[]).await.is_none());
        assert!(model.seen.lock().expect("seen").is_empty());
    }
}

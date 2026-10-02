//! The OpenAI-compatible chat-completions adapter.
//!
//! One client for every endpoint that speaks this wire format: OpenCode Go today,
//! and Kimi, DeepSeek, or OpenRouter when they are configured, since each is the
//! same request shape with a different base URL, credential, and model table.
//!
//! `wire` builds the request, `sse` frames the response, `stream` turns chunks into
//! the delta vocabulary `robi-core` fixed, and this module owns the transport:
//! timeouts, the retry boundary, and cancellation.

pub mod sse;
pub mod stream;
pub mod wire;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use robi_core::error::ModelError;
use robi_core::ids::SessionId;
use robi_core::message::Message;
use robi_core::model::{Delta, Model, ModelStream};
use robi_core::tool::ToolRegistry;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::catalog::ModelCatalog;
use super::config::ProviderSettings;
use super::error::{truncate_body, ProviderError};
use super::retry::{classify, Retry};
use sse::SseDecoder;
use stream::Assembler;

/// A model served over the OpenAI chat-completions wire format.
#[derive(Debug)]
pub struct OpenAiCompatibleModel {
    client: reqwest::Client,
    settings: ProviderSettings,
    catalog: Arc<ModelCatalog>,
    /// A shared handle rather than a snapshot, so a tool registered after this
    /// model was built is still offered to it.
    tools: Arc<ToolRegistry>,
}

impl OpenAiCompatibleModel {
    /// Build an adapter, refusing a model that cannot drive the loop.
    ///
    /// The check is here rather than at the first turn so a misconfiguration is a
    /// startup error that names the model, not a failed conversation.
    pub fn new(
        settings: ProviderSettings,
        catalog: Arc<ModelCatalog>,
        tools: Arc<ToolRegistry>,
    ) -> Result<Self, ProviderError> {
        let info = catalog
            .get(&settings.model)
            .ok_or_else(|| ProviderError::UnknownModel(settings.model.to_string()))?;
        if !info.supports_tools {
            return Err(ProviderError::ToolLessModel(settings.model.to_string()));
        }

        let client = reqwest::Client::builder()
            .user_agent(settings.user_agent.clone())
            .build()
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        Ok(Self {
            client,
            settings,
            catalog,
            tools,
        })
    }

    pub fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    /// The key that identifies the conversation to the provider.
    ///
    /// The loop's `SessionId` is minted once per session and persisted, so it is
    /// stable across every turn and across a restart — which is what a prompt cache
    /// needs.
    fn session_key(&self, session: SessionId) -> String {
        self.settings
            .session_key_override
            .clone()
            .unwrap_or_else(|| session.to_string())
    }
}

#[async_trait]
impl Model for OpenAiCompatibleModel {
    async fn generate(
        &self,
        session: SessionId,
        transcript: &[Message],
        cancel: CancellationToken,
    ) -> Result<ModelStream, ModelError> {
        let tools = self.tools.tools();
        let request = wire::build_request(&self.settings, &tools, transcript);
        let body = serde_json::to_vec(&request)
            .map_err(|error| ProviderError::Malformed(error.to_string()).into_model_error())?;
        let session_key = self.session_key(session);

        // Retrying is confined to this call: nothing has been emitted yet.
        let response = self
            .send_with_retry(&body, &session_key, &cancel)
            .await
            .map_err(ProviderError::into_model_error)?;

        let (tx, rx) = mpsc::channel(64);
        let chunk_timeout = self.settings.chunk_timeout;
        tokio::spawn(async move {
            pump(response, tx, cancel, chunk_timeout).await;
        });

        Ok(ModelStream::new(rx))
    }
}

impl OpenAiCompatibleModel {
    /// Send until a response arrives, a retry is refused, or the attempts run out.
    ///
    /// Only a failure *before* any delta reaches the caller is retried, which is
    /// what keeps a retry from rendering a duplicated partial message.
    async fn send_with_retry(
        &self,
        body: &[u8],
        session_key: &str,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, ProviderError> {
        let url = self.settings.chat_completions_url();
        let mut attempt = 0u32;

        loop {
            attempt += 1;

            match self.send_once(&url, body, session_key).await {
                Ok(response) => return Ok(response),
                Err((error, retry)) => {
                    let Retry::Yes { after } = retry else {
                        return Err(error);
                    };
                    if attempt >= self.settings.retry.max_attempts {
                        return Err(error);
                    }

                    let delay = after.unwrap_or_else(|| self.settings.retry.backoff(attempt));
                    tracing::debug!(attempt, ?delay, "retrying a provider request");

                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                        () = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    /// One attempt. A response that is not a success becomes an error plus the
    /// retry decision, so the caller does not have to re-inspect a consumed body.
    async fn send_once(
        &self,
        url: &str,
        body: &[u8],
        session_key: &str,
    ) -> Result<reqwest::Response, (ProviderError, Retry)> {
        let mut request = self
            .client
            .post(url)
            .bearer_auth(self.settings.api_key.expose())
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::ACCEPT, "text/event-stream")
            .body(body.to_vec());

        if let Some(header) = &self.settings.session_header {
            request = request.header(header, session_key);
        }

        // A timeout on the request as a whole would also bound the stream, so the
        // wait for response headers is bounded here instead.
        let response = match tokio::time::timeout(self.settings.header_timeout, request.send())
            .await
        {
            Err(_) => {
                return Err((
                    ProviderError::Transport("timed out waiting for response headers".to_owned()),
                    Retry::Yes { after: None },
                ))
            }
            Ok(Err(error)) => {
                return Err((
                    ProviderError::Transport(error.to_string()),
                    Retry::Yes { after: None },
                ))
            }
            Ok(Ok(response)) => response,
        };

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let retry = classify(status, response.headers());
        let body = truncate_body(&response.text().await.unwrap_or_default());
        let status = status.as_u16();
        let error = match status {
            401 | 403 => {
                tracing::debug!(status, body, "the credential was refused");
                ProviderError::Unauthorized { status }
            }
            429 => {
                tracing::debug!(status, body, "the provider is rate limiting");
                ProviderError::RateLimited { status }
            }
            other => ProviderError::Http {
                status: other,
                body,
            },
        };
        Err((error, retry))
    }
}

/// Read the response body and forward deltas until the message finishes, the
/// stream stalls, or the caller cancels.
async fn pump(
    response: reqwest::Response,
    tx: mpsc::Sender<Delta>,
    cancel: CancellationToken,
    chunk_timeout: Duration,
) {
    let mut decoder = SseDecoder::new();
    let mut assembler = Assembler::new();
    let mut body = response.bytes_stream();

    loop {
        let chunk = tokio::select! {
            biased;
            // A cancelled turn sends nothing further. The loop already owns the
            // cancelled outcome, so a failed delta here would be noise.
            () = cancel.cancelled() => return,
            next = tokio::time::timeout(chunk_timeout, body.next()) => match next {
                // A gap longer than the timeout cannot be resumed in place, and
                // it is not retryable: a retry would duplicate what streamed.
                Err(_elapsed) => {
                    let error = ProviderError::Transport(format!(
                        "no chunk arrived within {chunk_timeout:?}"
                    ));
                    let _ = tx.send(Delta::Failed(error.into_model_error())).await;
                    return;
                }
                Ok(None) => {
                    // A last line may have arrived without a terminating blank
                    // line, so flush the decoder before deciding the stream ended.
                    if absorb(&tx, &mut assembler, decoder.finish()).await {
                        return;
                    }
                    let outcome = match assembler.on_eof() {
                        Ok(deltas) => send_all(&tx, deltas).await,
                        Err(error) => {
                            tx.send(Delta::Failed(error.into_model_error())).await.is_ok()
                        }
                    };
                    let _ = outcome;
                    return;
                }
                Ok(Some(Err(error))) => {
                    let error = ProviderError::Transport(error.to_string());
                    let _ = tx.send(Delta::Failed(error.into_model_error())).await;
                    return;
                }
                Ok(Some(Ok(bytes))) => bytes,
            },
        };

        if absorb(&tx, &mut assembler, decoder.push(&chunk)).await {
            return;
        }
    }
}

/// Feed payloads to the assembler, forwarding deltas.
///
/// Returns `true` when the stream is finished, either because the message is
/// complete or because the receiver is gone.
async fn absorb(
    tx: &mpsc::Sender<Delta>,
    assembler: &mut Assembler,
    payloads: Vec<String>,
) -> bool {
    for payload in payloads {
        match assembler.on_payload(&payload) {
            Ok(deltas) => {
                let finished = deltas
                    .iter()
                    .any(|delta| matches!(delta, Delta::Finished(_)));
                if !send_all(tx, deltas).await {
                    return true;
                }
                if finished {
                    return true;
                }
            }
            Err(error) => {
                let _ = tx.send(Delta::Failed(error.into_model_error())).await;
                return true;
            }
        }
    }
    false
}

/// Forward deltas, stopping if the loop stopped listening.
async fn send_all(tx: &mpsc::Sender<Delta>, deltas: Vec<Delta>) -> bool {
    for delta in deltas {
        if tx.send(delta).await.is_err() {
            return false;
        }
    }
    true
}

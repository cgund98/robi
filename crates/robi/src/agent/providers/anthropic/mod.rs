//! The Anthropic Messages adapter.
//!
//! `wire` builds the request, `stream` turns typed SSE events into the delta
//! vocabulary `robi-core` fixed, and this module owns the transport: timeouts, the
//! retry boundary (D7), and cancellation. The SSE framing is shared with the
//! OpenAI adapter (`providers::sse`); only the payload handling differs (A1).

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

use crate::agent::providers::catalog::ModelCatalog;
use crate::agent::providers::config::ProviderSettings;
use crate::agent::providers::error::{truncate_body, ProviderError};
use crate::agent::providers::images::ImageSource;
use crate::agent::providers::retry::{classify, Retry};
use crate::agent::providers::sse::SseDecoder;
use stream::Assembler;

/// A model served over Anthropic's Messages wire format.
pub struct AnthropicModel {
    client: reqwest::Client,
    settings: ProviderSettings,
    catalog: Arc<ModelCatalog>,
    /// A shared handle rather than a snapshot, so a tool registered after this
    /// model was built is still offered to it.
    tools: Arc<ToolRegistry>,
    /// Resolves a user message's attachment ids to bytes at request build.
    images: Arc<dyn ImageSource>,
}

impl AnthropicModel {
    /// Build an adapter, refusing a model that cannot drive the loop, one that
    /// cannot call tools, and a configured effort the model rejects (A2).
    pub fn new(
        settings: ProviderSettings,
        catalog: Arc<ModelCatalog>,
        tools: Arc<ToolRegistry>,
        images: Arc<dyn ImageSource>,
    ) -> Result<Self, ProviderError> {
        let info = catalog
            .get(&settings.model)
            .ok_or_else(|| ProviderError::UnknownModel(settings.model.to_string()))?;
        if !info.supports_tools {
            return Err(ProviderError::ToolLessModel(settings.model.to_string()));
        }
        // A2: Haiku 4.5 returns a 400 for `output_config`. Refuse the
        // configuration here rather than discover it on the first turn.
        if settings.reasoning_effort.is_some() && !info.supports_effort {
            return Err(ProviderError::UnsupportedEffort {
                model: settings.model.to_string(),
            });
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
            images,
        })
    }

    pub fn settings(&self) -> &ProviderSettings {
        &self.settings
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    /// Whether the configured model accepts `output_config.effort` (A2).
    fn supports_effort(&self) -> bool {
        self.catalog
            .get(&self.settings.model)
            .map(|info| info.supports_effort)
            .unwrap_or(false)
    }

    /// The beta header Opus 4.5 needs before it accepts `output_config.effort`.
    fn effort_beta(&self) -> Option<&'static str> {
        if self.settings.reasoning_effort.is_some()
            && self.settings.model.wire_id() == "claude-opus-4-5"
        {
            Some(ProviderSettings::ANTHROPIC_EFFORT_BETA)
        } else {
            None
        }
    }
}

#[async_trait]
impl Model for AnthropicModel {
    async fn generate(
        &self,
        _session: SessionId,
        transcript: &[Message],
        cancel: CancellationToken,
    ) -> Result<ModelStream, ModelError> {
        let tools = self.tools.tools();
        let images = self.resolve_images(transcript).await?;
        let request = wire::build_request(
            &self.settings,
            &tools,
            transcript,
            &images,
            self.supports_effort(),
        )
        .map_err(|error| ProviderError::from(error).into_model_error())?;
        let body = serde_json::to_vec(&request)
            .map_err(|error| ProviderError::Malformed(error.to_string()).into_model_error())?;

        // Retrying is confined to this call: nothing has been emitted yet.
        let response = self
            .send_with_retry(&body, &cancel)
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

impl AnthropicModel {
    /// Resolve every attachment id the transcript references to its stored bytes.
    ///
    /// Rejects the turn when the model does not accept images (D13) or when a row
    /// is missing (D11).
    async fn resolve_images(
        &self,
        transcript: &[Message],
    ) -> Result<wire::ResolvedImages, ModelError> {
        let any_images = transcript.iter().any(|m| !m.images.is_empty());
        if !any_images {
            return Ok(wire::ResolvedImages::default());
        }

        let info = self
            .catalog
            .get(&self.settings.model)
            .expect("construction validated the model is in the catalog");
        if !info.supports_vision {
            return Err(ProviderError::NoVision {
                model: self.settings.model.to_string(),
            }
            .into_model_error());
        }

        let mut resolved = wire::ResolvedImages::default();
        for message in transcript {
            for attachment in &message.images {
                let Some((media_type, bytes)) = self
                    .images
                    .image(&attachment.id)
                    .await
                    .map_err(ProviderError::into_model_error)?
                else {
                    return Err(ProviderError::MissingImage {
                        id: attachment.id.clone(),
                    }
                    .into_model_error());
                };
                resolved.insert(attachment.id.clone(), (media_type, bytes));
            }
        }
        Ok(resolved)
    }

    /// Send until a response arrives, a retry is refused, or the attempts run out.
    async fn send_with_retry(
        &self,
        body: &[u8],
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, ProviderError> {
        let url = self.settings.messages_url();
        let mut attempt = 0u32;

        loop {
            attempt += 1;

            match self.send_once(&url, body).await {
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

    /// One attempt.
    async fn send_once(
        &self,
        url: &str,
        body: &[u8],
    ) -> Result<reqwest::Response, (ProviderError, Retry)> {
        let mut request = self
            .client
            .post(url)
            .header("x-api-key", self.settings.api_key.expose())
            .header("anthropic-version", ProviderSettings::ANTHROPIC_VERSION)
            .header(http::header::CONTENT_TYPE, "application/json")
            .header(http::header::ACCEPT, "text/event-stream")
            .body(body.to_vec());

        if let Some(beta) = self.effort_beta() {
            request = request.header("anthropic-beta", beta);
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
            tracing::info!(
                model = %self.settings.model,
                status = status.as_u16(),
                "provider accepted the request"
            );
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
            () = cancel.cancelled() => return,
            next = tokio::time::timeout(chunk_timeout, body.next()) => match next {
                Err(_elapsed) => {
                    let error = ProviderError::Transport(format!(
                        "no chunk arrived within {chunk_timeout:?}"
                    ));
                    tracing::warn!(?chunk_timeout, "provider stream stalled");
                    let _ = tx.send(Delta::Failed(error.into_model_error())).await;
                    return;
                }
                Ok(None) => {
                    if absorb(&tx, &mut assembler, decoder.finish()).await {
                        return;
                    }
                    let outcome = match assembler.on_eof() {
                        Ok(deltas) => {
                            note_stop_reason(&assembler);
                            send_all(&tx, deltas).await
                        }
                        Err(error) => {
                            tracing::warn!(%error, "provider stream failed");
                            tx.send(Delta::Failed(error.into_model_error())).await.is_ok()
                        }
                    };
                    let _ = outcome;
                    return;
                }
                Ok(Some(Err(error))) => {
                    tracing::warn!(%error, "provider stream failed");
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
                    note_stop_reason(assembler);
                    tracing::info!("provider stream finished");
                    return true;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "provider stream failed");
                let _ = tx.send(Delta::Failed(error.into_model_error())).await;
                return true;
            }
        }
    }
    false
}

/// A `max_tokens` stop still settles the message, so the turn reads as complete.
/// Log it; otherwise a truncated reply is indistinguishable from a finished one.
fn note_stop_reason(assembler: &Assembler) {
    if assembler.stop_reason() == Some("max_tokens") {
        tracing::warn!("provider truncated the message at the output limit");
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::providers::catalog::ModelInfo;
    use crate::agent::providers::config::{ApiKey, ModelId, ReasoningEffort};
    use crate::agent::providers::ProviderError;
    use robi_core::message::ImageAttachment;

    fn settings_for(model: &str) -> ProviderSettings {
        ProviderSettings::anthropic(ApiKey::new("k"), ModelId::new(model))
    }

    fn catalog() -> ModelCatalog {
        ModelCatalog::from_models(vec![
            ModelInfo {
                id: ModelId::new("ant_vision"),
                display_name: "Vision".into(),
                context_window: 200_000,
                max_output: 64_000,
                supports_tools: true,
                supports_reasoning: true,
                supports_effort: true,
                supports_vision: true,
            },
            ModelInfo {
                id: ModelId::new("ant_visionless"),
                display_name: "Visionless".into(),
                context_window: 200_000,
                max_output: 64_000,
                supports_tools: true,
                supports_reasoning: true,
                supports_effort: true,
                supports_vision: false,
            },
            ModelInfo {
                id: ModelId::new("ant_no_effort"),
                display_name: "No Effort".into(),
                context_window: 200_000,
                max_output: 64_000,
                supports_tools: true,
                supports_reasoning: true,
                supports_effort: false,
                supports_vision: true,
            },
        ])
    }

    struct MapSource(std::collections::HashMap<String, (String, Vec<u8>)>);

    #[async_trait]
    impl ImageSource for MapSource {
        async fn image(&self, id: &str) -> Result<Option<(String, Vec<u8>)>, ProviderError> {
            Ok(self.0.get(id).cloned())
        }
    }

    fn source(rows: &[(&str, &str, &[u8])]) -> Arc<dyn ImageSource> {
        Arc::new(MapSource(
            rows.iter()
                .map(|(id, mt, bytes)| (id.to_string(), (mt.to_string(), bytes.to_vec())))
                .collect(),
        ))
    }

    fn model(model: &str, catalog: ModelCatalog, images: Arc<dyn ImageSource>) -> AnthropicModel {
        AnthropicModel::new(
            settings_for(model),
            Arc::new(catalog),
            Arc::new(ToolRegistry::new()),
            images,
        )
        .expect("builds")
    }

    fn image_message() -> Message {
        Message::user_with_images(
            "describe",
            vec![ImageAttachment {
                id: "img".to_owned(),
                media_type: "image/png".to_owned(),
            }],
        )
    }

    #[test]
    fn a_model_that_rejects_effort_is_refused_naming_itself() {
        let settings = settings_for("ant_no_effort").with_reasoning_effort(ReasoningEffort::High);
        let error = match AnthropicModel::new(
            settings,
            Arc::new(catalog()),
            Arc::new(ToolRegistry::new()),
            source(&[]),
        ) {
            Ok(_) => panic!("the model rejects effort"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ProviderError::UnsupportedEffort { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("ant_no_effort"));
    }

    #[tokio::test]
    async fn a_non_vision_model_rejects_the_turn_naming_itself() {
        let m = model("ant_visionless", catalog(), source(&[]));
        let error = m
            .resolve_images(&[image_message()])
            .await
            .expect_err("a vision-less model cannot take images");
        let ModelError::Provider(text) = error else {
            panic!("a vision refusal is a provider error");
        };
        assert!(text.contains("ant_visionless"), "{text}");
    }

    #[tokio::test]
    async fn a_vision_model_resolves_the_attachment_bytes() {
        let m = model(
            "ant_vision",
            catalog(),
            source(&[("img", "image/png", b"\x89PNG\r\n")]),
        );
        let resolved = m
            .resolve_images(&[image_message()])
            .await
            .expect("resolves");
        assert_eq!(
            resolved.get("img"),
            Some(&("image/png".to_owned(), b"\x89PNG\r\n".to_vec()))
        );
    }

    #[tokio::test]
    async fn a_missing_row_fails_the_turn() {
        let m = model("ant_vision", catalog(), source(&[]));
        let error = m
            .resolve_images(&[image_message()])
            .await
            .expect_err("the row is missing");
        let ModelError::Provider(text) = error else {
            panic!("a missing image is a provider error");
        };
        assert!(text.contains("img"), "{text}");
    }

    #[tokio::test]
    async fn a_transcript_without_images_skips_the_store_and_vision_check() {
        let m = model("ant_visionless", catalog(), source(&[]));
        let resolved = m
            .resolve_images(&[Message::user("plain")])
            .await
            .expect("text-only turns resolve to an empty map");
        assert!(resolved.is_empty());
    }

    #[test]
    fn the_effort_beta_header_is_sent_only_for_opus_4_5() {
        let opus = model("ant_vision", catalog(), source(&[]));
        // The test catalog model is not opus, so no beta header.
        assert!(opus.effort_beta().is_none());

        let opus = AnthropicModel::new(
            settings_for("ant_claude-opus-4-5").with_reasoning_effort(ReasoningEffort::High),
            Arc::new(ModelCatalog::anthropic()),
            Arc::new(ToolRegistry::new()),
            source(&[]),
        )
        .expect("builds");
        assert_eq!(opus.effort_beta(), Some("effort-2025-11-24"));

        let sonnet = AnthropicModel::new(
            settings_for("ant_claude-sonnet-4-6").with_reasoning_effort(ReasoningEffort::High),
            Arc::new(ModelCatalog::anthropic()),
            Arc::new(ToolRegistry::new()),
            source(&[]),
        )
        .expect("builds");
        assert!(
            sonnet.effort_beta().is_none(),
            "4.6 models need no beta header"
        );
    }
}

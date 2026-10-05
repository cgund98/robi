use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::error::IndexError;

pub const MODEL_ID: &str = "nomic-ai/nomic-embed-text-v1.5";
pub const DIMENSIONS: usize = 768;
pub const DOCUMENT_PREFIX: &str = "search_document: ";
pub const QUERY_PREFIX: &str = "search_query: ";

#[async_trait]
pub trait Embedder: Send + Sync {
    fn model_id(&self) -> &str;
    fn dimensions(&self) -> usize;
    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError>;
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, IndexError>;

    /// Load weights. The fake embedder does nothing.
    async fn warmup(&self) -> Result<(), IndexError> {
        Ok(())
    }
}

/// Fixed vectors from the text hash. Tests never download a model.
pub struct FakeEmbedder {
    pub documents: AtomicUsize,
    dimensions: usize,
}

impl FakeEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self {
            documents: AtomicUsize::new(0),
            dimensions,
        }
    }

    pub fn document_calls(&self) -> usize {
        self.documents.load(Ordering::SeqCst)
    }
}

impl Default for FakeEmbedder {
    fn default() -> Self {
        Self::new(DIMENSIONS)
    }
}

#[async_trait]
impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError> {
        self.documents.fetch_add(texts.len(), Ordering::SeqCst);
        Ok(texts
            .iter()
            .map(|text| hash_vector(&format!("{DOCUMENT_PREFIX}{text}"), self.dimensions))
            .collect())
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, IndexError> {
        Ok(hash_vector(
            &format!("{QUERY_PREFIX}{text}"),
            self.dimensions,
        ))
    }
}

fn hash_vector(text: &str, dimensions: usize) -> Vec<f32> {
    let digest = Sha256::digest(text.as_bytes());
    (0..dimensions)
        .map(|index| {
            let byte = digest[index % digest.len()];
            (byte as f32 / 255.0) - 0.5
        })
        .collect()
}

#[cfg(feature = "local-embed")]
mod local {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

    use super::{DOCUMENT_PREFIX, MODEL_ID, QUERY_PREFIX};
    use crate::error::IndexError;

    /// Intra-op threads for the ONNX session. The index walk and query
    /// embeds share this session, so both stay on at most two cores.
    const EMBED_THREADS: usize = 2;

    /// On-device Nomic embeddings. Prefixes are applied here, once.
    ///
    /// The session lives behind an `Arc` so load and inference can run on
    /// Tokio's blocking pool. Running them on a runtime worker deadlocks that
    /// pool: ONNX waits for worker threads that are themselves blocked inside
    /// ONNX, and the HTTP server on the same runtime stops accepting.
    pub struct LocalEmbedder {
        cache_dir: PathBuf,
        model: Arc<Mutex<Option<TextEmbedding>>>,
    }

    impl LocalEmbedder {
        pub fn new(cache_dir: PathBuf) -> Self {
            Self {
                cache_dir,
                model: Arc::new(Mutex::new(None)),
            }
        }
    }

    fn load_model(cache_dir: &std::path::Path) -> Result<TextEmbedding, IndexError> {
        TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::NomicEmbedTextV15)
                .with_cache_dir(cache_dir.to_path_buf())
                .with_show_download_progress(false)
                .with_intra_threads(EMBED_THREADS)
                .with_session_config("session.intra_op.allow_spinning", "0")
                .with_session_config("session.inter_op.allow_spinning", "0"),
        )
        .map_err(|err| IndexError::Message(format!("load embedding model: {err}")))
    }

    fn with_model<T>(
        cache_dir: &std::path::Path,
        model: &Mutex<Option<TextEmbedding>>,
        body: impl FnOnce(&mut TextEmbedding) -> Result<T, IndexError>,
    ) -> Result<T, IndexError> {
        let mut guard = model
            .lock()
            .map_err(|_| IndexError::Message("embedder lock poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(load_model(cache_dir)?);
        }
        let loaded = guard
            .as_mut()
            .ok_or_else(|| IndexError::Message("embedding model is not loaded".into()))?;
        body(loaded)
    }

    async fn on_blocking<T>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, IndexError>
    where
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|err| IndexError::Message(format!("embedding task failed: {err}")))
    }

    #[async_trait]
    impl super::Embedder for LocalEmbedder {
        fn model_id(&self) -> &str {
            MODEL_ID
        }

        fn dimensions(&self) -> usize {
            super::DIMENSIONS
        }

        async fn warmup(&self) -> Result<(), IndexError> {
            let cache_dir = self.cache_dir.clone();
            let model = Arc::clone(&self.model);
            on_blocking(move || with_model(&cache_dir, &model, |_| Ok(()))).await?
        }

        async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, IndexError> {
            let texts: Vec<String> = texts
                .iter()
                .map(|text| format!("{DOCUMENT_PREFIX}{text}"))
                .collect();
            let cache_dir = self.cache_dir.clone();
            let model = Arc::clone(&self.model);
            on_blocking(move || {
                with_model(&cache_dir, &model, |loaded| {
                    loaded
                        .embed(texts, None)
                        .map_err(|err| IndexError::Message(format!("embed documents: {err}")))
                })
            })
            .await?
        }

        async fn embed_query(&self, text: &str) -> Result<Vec<f32>, IndexError> {
            let query = format!("{QUERY_PREFIX}{text}");
            let cache_dir = self.cache_dir.clone();
            let model = Arc::clone(&self.model);
            on_blocking(move || {
                with_model(&cache_dir, &model, |loaded| {
                    let mut vectors = loaded
                        .embed(vec![query], None)
                        .map_err(|err| IndexError::Message(format!("embed query: {err}")))?;
                    vectors
                        .pop()
                        .ok_or_else(|| IndexError::Message("embed query returned no vector".into()))
                })
            })
            .await?
        }
    }
}

#[cfg(feature = "local-embed")]
pub use local::LocalEmbedder;

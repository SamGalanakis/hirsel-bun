//! OpenRouter-backed embedding + contextualisation client.
//!
//! Used by the chunk worker (Phase 4D) and by `search_context`'s hybrid
//! query path (Phase 4E). There is no local fallback: if the OpenRouter
//! API key is unset, every call returns an error and the worker parks.
//! The frontend surfaces a banner when `embeddings_ready=false` so the
//! user knows to add the key.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::backend::credentials::CredentialStore;
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

pub const CONTEXTUALIZER_VERSION: &str = "contextualizer-gemini-3-flash-v1";
pub const EMBEDDING_VERSION: &str = "pplx-embed-v1-0.6b-v1";

fn read_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

async fn load_api_key() -> Result<String, String> {
    if let Some(v) = read_env("OPENROUTER_API_KEY") {
        return Ok(v);
    }
    let store = CredentialStore::open()
        .await
        .map_err(|e| format!("credential store: {e}"))?;
    match store.load_openrouter_api_key().await {
        Ok(Some(v)) if !v.trim().is_empty() => Ok(v),
        _ => Err("OpenRouter API key is required for embeddings. Set OPENROUTER_API_KEY or save a key in Settings.".to_string()),
    }
}

fn base_url() -> String {
    read_env("OPENROUTER_BASE_URL")
        .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Client against OpenRouter's chat completions + embeddings endpoints.
#[derive(Clone)]
pub struct EmbeddingClient {
    http: Client,
    api_key: String,
    base_url: String,
    concurrency: Arc<Semaphore>,
}

impl EmbeddingClient {
    /// Build a client from the current credential state. Errors if the
    /// OpenRouter key is missing — embeddings are hard-required.
    pub async fn from_credentials() -> Result<Self, String> {
        let api_key = load_api_key().await?;
        let max_concurrent =
            RuntimeSettings::get_or(keys::EMBED_MAX_CONCURRENT, Defaults::EMBED_MAX_CONCURRENT)
                .await
                .max(1);
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("reqwest build: {e}"))?;
        Ok(Self {
            http,
            api_key,
            base_url: base_url(),
            concurrency: Arc::new(Semaphore::new(max_concurrent)),
        })
    }

    /// Check whether an OpenRouter key is currently available. Does not
    /// make a network call.
    pub async fn is_configured() -> bool {
        load_api_key().await.is_ok()
    }

    /// Embed a batch of texts. Respects the `EMBED_BATCH_MAX_*` budgets
    /// and the `EMBED_MAX_CONCURRENT` semaphore. Preserves input order.
    pub async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let model =
            RuntimeSettings::get_or(keys::EMBEDDING_MODEL, Defaults::EMBEDDING_MODEL.to_string())
                .await;
        let dimensions =
            RuntimeSettings::get_or(keys::EMBEDDING_DIMENSIONS, Defaults::EMBEDDING_DIMENSIONS)
                .await;
        let max_items =
            RuntimeSettings::get_or(keys::EMBED_BATCH_MAX_ITEMS, Defaults::EMBED_BATCH_MAX_ITEMS)
                .await
                .max(1);
        let max_tokens = RuntimeSettings::get_or(
            keys::EMBED_BATCH_MAX_TOKENS,
            Defaults::EMBED_BATCH_MAX_TOKENS,
        )
        .await
        .max(1);

        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        let mut cursor = 0;
        while cursor < texts.len() {
            let mut end = cursor;
            let mut tokens_in_batch = 0usize;
            while end < texts.len() && (end - cursor) < max_items {
                let est = estimate_tokens(&texts[end]);
                if end > cursor && tokens_in_batch + est > max_tokens {
                    break;
                }
                tokens_in_batch += est;
                end += 1;
            }
            let batch = &texts[cursor..end];
            let embeddings = self.embed_batch(&model, dimensions, batch).await?;
            out.extend(embeddings);
            cursor = end;
        }
        Ok(out)
    }

    async fn embed_batch(
        &self,
        model: &str,
        expected_dims: usize,
        batch: &[String],
    ) -> Result<Vec<Vec<f32>>, String> {
        let _permit = self
            .concurrency
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("concurrency acquire: {e}"))?;
        let body = json!({ "model": model, "input": batch });
        let response = self
            .http
            .post(format!("{}/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("embeddings request: {e}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("embeddings {status}: {text}"));
        }
        let parsed: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| format!("embeddings decode: {e}"))?;
        let mut data = parsed.data;
        data.sort_by_key(|item| item.index);
        let embeddings: Vec<Vec<f32>> = data.into_iter().map(|item| item.embedding).collect();
        if embeddings.len() != batch.len() {
            return Err(format!(
                "embeddings returned {} vectors for {} inputs",
                embeddings.len(),
                batch.len()
            ));
        }
        for e in &embeddings {
            if e.len() != expected_dims {
                return Err(format!(
                    "embedding dim mismatch: got {}, expected {}",
                    e.len(),
                    expected_dims
                ));
            }
        }
        Ok(embeddings)
    }

    /// Single-query embedding convenience.
    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, String> {
        let mut out = self.embed_texts(&[query.to_string()]).await?;
        out.pop()
            .ok_or_else(|| "embed_query: provider returned no vector".to_string())
    }

    /// Produce a 2-sentence context blurb for a chunk, to be prepended to
    /// the chunk before it is embedded. Falls back to a deterministic
    /// string if the LLM call fails, so indexing never hard-blocks.
    pub async fn contextualize_chunk(
        &self,
        document_context: &str,
        chunk_text: &str,
        chunk_path: &[String],
    ) -> Result<String, String> {
        let _permit = self
            .concurrency
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("concurrency acquire: {e}"))?;
        let model = RuntimeSettings::get_or(
            keys::EMBEDDING_CONTEXT_MODEL,
            Defaults::EMBEDDING_CONTEXT_MODEL.to_string(),
        )
        .await;
        let summary_threshold = RuntimeSettings::get_or(
            keys::CHUNK_GLOBAL_SUMMARY_THRESHOLD,
            Defaults::CHUNK_GLOBAL_SUMMARY_THRESHOLD,
        )
        .await;
        let source_context = trim_to_token_estimate(document_context, summary_threshold.min(8_000));
        let prompt = format!(
            "Write 2 concise sentences that explain what this chunk is about in the larger source. \
             Do not quote long passages.\n\nSource context:\n{source_context}\n\nChunk:\n{chunk_text}"
        );
        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": "You write retrieval context for document chunks."},
                {"role": "user", "content": prompt}
            ],
            "max_tokens": 256,
            "temperature": 0.2
        });
        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("contextualize request: {e}"))?;
        if !response.status().is_success() {
            return Ok(fallback_context(chunk_path));
        }
        let parsed: ChatResponse = response
            .json()
            .await
            .map_err(|e| format!("contextualize decode: {e}"))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .unwrap_or_default();
        if content.is_empty() {
            Ok(fallback_context(chunk_path))
        } else {
            Ok(content)
        }
    }
}

fn fallback_context(chunk_path: &[String]) -> String {
    if chunk_path.is_empty() {
        "This chunk comes from the indexed source document.".to_string()
    } else {
        format!("This chunk appears under {}.", chunk_path.join(" / "))
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

/// Same token heuristic figments uses. ~1 token = 3/4 word.
pub fn estimate_tokens(text: &str) -> usize {
    (text.split_whitespace().count() * 4 / 3)
        .max(text.len() / 5)
        .max(1)
}

fn trim_to_token_estimate(text: &str, max_tokens: usize) -> String {
    if estimate_tokens(text) <= max_tokens {
        return text.to_string();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let split = ((max_tokens * 3) / 4).min(words.len()).max(1);
    words[..split].join(" ")
}

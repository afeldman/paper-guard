//! Embedding provider abstraction for Review Memory.
//!
//! Review Memory needs semantic retrieval: an embedding is computed for each
//! approved memory entry (once), and a query embedding is computed at
//! retrieval time. To keep provider independence (mock, local, remote) the
//! rest of Paper Guard only ever sees the [`EmbeddingProvider`] trait.
//!
//! The first concrete providers are:
//!   * [`MockEmbeddingProvider`] — a deterministic, offline provider used by
//!     standalone mode and tests. It does not fabricate a real semantic space,
//!     but it is stable and exercises the full pipeline offline.
//!   * [`OpenAICompatibleEmbeddingProvider`] — talks to any OpenAI-compatible
//!     `/embeddings` endpoint, which includes Ollama's embedding API. The
//!     embedding model is configurable and never hard-coded.
//!
//! A memory entry is embedded by its [`ReviewMemoryEntry::embedding_text()`] —
//! a deterministic representation of the **review experience** (category,
//! claim/evidence context, finding, human decision, human feedback), never the
//! raw manuscript text.

use std::time::Duration;

/// A dense embedding vector.
pub type Embedding = Vec<f32>;

/// An embedding provider. Must be cheap; embeddings are computed once per
/// memory entry and once per retrieval query.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text into a dense vector.
    async fn embed(&self, input: &str) -> anyhow::Result<Embedding>;
}

/// A deterministic, offline embedding provider.
///
/// It hashes token frequencies into a small, stable vector. It is **not** a
/// real semantic space, but it is reproducible, dependency-free, and lets the
/// full retrieval pipeline (including scope/approval filtering) run in
/// standalone mode and in the offline test suite.
pub struct MockEmbeddingProvider {
    dimensions: usize,
}

impl MockEmbeddingProvider {
    pub fn new() -> Self {
        MockEmbeddingProvider { dimensions: 64 }
    }
}

impl Default for MockEmbeddingProvider {
    fn default() -> Self {
        MockEmbeddingProvider::new()
    }
}

/// Embed text by hashing token ids into a fixed-dim vector (bag-of-tokens).
fn hash_embed(text: &str, dims: usize) -> Embedding {
    let mut vec = vec![0.0f32; dims];
    for token in text.split_whitespace() {
        let token = token.to_lowercase();
        // Stable per-token hash into the vector's bucket dimension.
        let mut h: u64 = 1469598103934665603;
        for b in token.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        let idx = (h % dims as u64) as usize;
        // Sign based on a second hash to spread dimensions.
        let mut h2: u64 = 31;
        for b in token.as_bytes() {
            h2 = h2.wrapping_mul(31).wrapping_add(*b as u64);
        }
        let val = if h2.is_multiple_of(2) { 1.0 } else { -1.0 };
        vec[idx] += val;
    }
    // L2-normalize so cosine similarity is meaningful on the mock vectors.
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    }
    vec
}

#[async_trait::async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, input: &str) -> anyhow::Result<Embedding> {
        Ok(hash_embed(input, self.dimensions))
    }
}

/// Wrapper so the mock can also be constructed via a radius parameter for tests.
impl MockEmbeddingProvider {
    pub fn with_dimensions(dims: usize) -> Self {
        MockEmbeddingProvider {
            dimensions: dims.max(4),
        }
    }
}

/// Configuration for the OpenAI-compatible (incl. Ollama) embedding provider.
#[derive(Debug, Clone)]
pub struct EmbeddingProviderConfig {
    /// Base URL of the OpenAI-compatible endpoint, e.g.
    /// `https://api.openai.com/v1` or Ollama's `http://localhost:11434/v1`.
    pub base_url: String,
    /// The embedding model name (e.g. `text-embedding-3-small` for OpenAI,
    /// `all-minilm` or `nomic-embed-text` for Ollama). Configurable, never
    /// hard-coded.
    pub model: String,
    /// Name of an environment variable holding an optional API key. When
    /// absent/empty, requests are sent without an Authorization header (which
    /// is what Ollama needs).
    pub api_key_env: Option<String>,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
}

/// An OpenAI-compatible `/embeddings` provider (covers Ollama's embedding API).
///
/// The endpoint is called with a single text and returns a dense vector from
/// `data[0].embedding`.
pub struct OpenAICompatibleEmbeddingProvider {
    config: EmbeddingProviderConfig,
    client: reqwest::Client,
    api_key: Option<String>,
}

impl OpenAICompatibleEmbeddingProvider {
    /// Construct a provider. Reads the API key (if any) from the environment
    /// once up front so it is never stored or re-read/re-logged.
    pub fn new(config: EmbeddingProviderConfig) -> anyhow::Result<Self> {
        let api_key = match &config.api_key_env {
            Some(var) if !var.trim().is_empty() => {
                std::env::var(var).ok().filter(|v| !v.trim().is_empty())
            }
            _ => None,
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
            .build()?;
        Ok(OpenAICompatibleEmbeddingProvider {
            config,
            client,
            api_key,
        })
    }

    fn embeddings_url(&self) -> String {
        format!("{}/embeddings", self.config.base_url.trim_end_matches('/'))
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAICompatibleEmbeddingProvider {
    async fn embed(&self, input: &str) -> anyhow::Result<Embedding> {
        let body = serde_json::json!({
            "model": self.config.model,
            "input": input,
        });
        let mut req = self.client.post(self.embeddings_url()).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        let resp = req.send().await.map_err(|e| {
            anyhow::anyhow!(
                "embedding provider unavailable at {}: {e}",
                self.config.base_url
            )
        })?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "embedding provider error: HTTP {}",
                resp.status()
            ));
        }
        let text = resp.text().await.unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("embedding provider malformed JSON: {e}"))?;
        let vector = value
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|d| d.first())
            .and_then(|x| x.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| {
                anyhow::anyhow!("embedding provider response missing `data[0].embedding`")
            })?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<_>>();
        if vector.is_empty() {
            return Err(anyhow::anyhow!(
                "embedding provider returned an empty embedding vector"
            ));
        }
        Ok(vector)
    }
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let mag = na.sqrt() * nb.sqrt();
    if mag <= 1e-8 {
        0.0
    } else {
        dot / mag
    }
}

/// Compute cosine similarity if the embedding dimension matches; otherwise 0.
pub fn try_cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    cosine_similarity(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_provider_is_deterministic() {
        let p = MockEmbeddingProvider::new();
        let a = p.embed("the method reduces latency").await.unwrap();
        let b = p.embed("the method reduces latency").await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[tokio::test]
    async fn mock_similar_text_has_higher_similarity() {
        let p = MockEmbeddingProvider::new();
        let a = p.embed("claim lacks supporting evidence").await.unwrap();
        let b = p
            .embed("claim lacks supporting evidence here")
            .await
            .unwrap();
        let c = p.embed("the weather is nice today").await.unwrap();
        let sim_ab = cosine_similarity(&a, &b);
        let sim_ac = cosine_similarity(&a, &c);
        assert!(
            sim_ab > sim_ac,
            "related > unrelated ({sim_ab} vs {sim_ac})"
        );
    }

    #[test]
    fn cosine_is_normalized_behaviour() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![0.0, 1.0];
        assert_eq!(cosine_similarity(&v1, &v2), 0.0);
        let v3 = vec![1.0, 0.0];
        assert!((cosine_similarity(&v1, &v3) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mismatched_dims_return_zero() {
        assert_eq!(try_cosine(&[1.0], &[1.0, 0.0]), 0.0);
    }
}

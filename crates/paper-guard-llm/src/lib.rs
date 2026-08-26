//! # Paper Guard LLM
//!
//! Provider-agnostic LLM abstraction. Reviewers and the judge never talk to a
//! concrete provider; they talk to [`LlmProvider`]. Concrete implementations
//! (OpenAI, Anthropic, OpenAI-compatible, local) are selected via configuration
//! and instantiated behind this trait.
//!
//! A deterministic [`MockLlmProvider`] is provided so the entire core pipeline
//! can be exercised without any external API.

mod mock;
mod openai_compatible;
mod provider;

pub use mock::{
    hash_bytes, with_schema, MockLlmFactory, MockLlmRequest, MockLlmScenario, MockOutcome,
    MockProvider,
};
pub use openai_compatible::{OpenAICompatibleConfig, OpenAICompatibleProvider, RetryPolicy};
pub use provider::{
    LlmContent, LlmImage, LlmProvider, ModelConfig, ProviderCapabilities, ProviderCapabilityError,
    ProviderError, ProviderKind, TransientKind,
};

use paper_guard_core::ContentHash;

/// Types of content that can be part of a request.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentPart {
    /// Plain text content.
    Text(String),
    /// An image reference (base64 data or path) passed to multimodal models.
    Image(LlmImage),
}

/// A request to an LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmRequest {
    /// The model name (provider-specific).
    pub model: String,
    /// System prompt.
    pub system: String,
    /// User content.
    pub user: Vec<ContentPart>,
    /// Determinism seed if the provider supports it.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Temperature.
    #[serde(default)]
    pub temperature: f32,
    /// Maximum output tokens.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// A stable hash of the full request, for reproducibility.
    #[serde(skip)]
    pub request_hash: ContentHash,
    /// The prompt version, for reproducibility.
    pub prompt_version: String,
}

impl LlmRequest {
    /// Build a new text-only request, computing a stable content hash.
    pub fn new(
        model: impl Into<String>,
        system: impl Into<String>,
        user_text: impl Into<String>,
        prompt_version: impl Into<String>,
    ) -> Self {
        let user = vec![ContentPart::Text(user_text.into())];
        let mut req = LlmRequest {
            model: model.into(),
            system: system.into(),
            user,
            seed: None,
            temperature: 0.0,
            max_tokens: None,
            request_hash: ContentHash("0".into()),
            prompt_version: prompt_version.into(),
        };
        req.request_hash = ContentHash::compute(&req);
        req
    }

    /// Set a multimodal image part.
    pub fn with_image(mut self, image: LlmImage) -> Self {
        self.user.push(ContentPart::Image(image));
        self.request_hash = ContentHash::compute(&self);
        self
    }

    /// Set a seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// The full text of the user prompt.
    pub fn user_text(&self) -> String {
        self.user
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.clone()),
                ContentPart::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether this request carries at least one image part (requires a
    /// multimodal / vision-capable provider).
    pub fn needs_image_capability(&self) -> bool {
        self.user.iter().any(|p| matches!(p, ContentPart::Image(_)))
    }
}

/// A response from an LLM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmResponse {
    /// The generated text.
    pub text: String,
    /// Token usage, if reported.
    pub usage: Option<LlmUsage>,
}

/// Token usage.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl LlmUsage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_returns_scripted_and_fallback() {
        let scenario = MockLlmScenario::new("test")
            .on("evidence-missing", r#"[{"finding":"missing"}]"#)
            .fallback("[]");
        let provider = MockProvider::new("model-x", scenario);
        let req = LlmRequest::new("model-x", "sys", "evidence-missing here", "v1");
        let resp = provider.generate(req).await.unwrap();
        assert!(resp.text.contains("missing"));

        let req2 = LlmRequest::new("model-x", "sys", "nothing matching", "v1");
        let resp2 = provider.generate(req2).await.unwrap();
        assert_eq!(resp2.text, "[]");
    }

    #[test]
    fn request_hash_is_stable() {
        let a = LlmRequest::new("m", "s", "hello", "v1");
        let b = LlmRequest::new("m", "s", "hello", "v1");
        assert_eq!(a.request_hash, b.request_hash);
        let c = LlmRequest::new("m", "s", "hello world", "v1");
        assert_ne!(a.request_hash, c.request_hash);
    }

    #[test]
    fn multimodal_request_keeps_images() {
        let req = LlmRequest::new("m", "s", "look", "v1").with_image(LlmImage {
            media_type: "image/png".into(),
            base64: "aGVsbG8=".into(),
        });
        assert_eq!(req.user.len(), 2);
        assert_eq!(req.user_text(), "look");
    }

    #[test]
    fn provider_kind_from_str() {
        assert_eq!(
            "openai".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenAi
        );
        assert_eq!(
            "anthropic".parse::<ProviderKind>().unwrap(),
            ProviderKind::Anthropic
        );
        assert!("bogus".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn model_config_hash_changes_on_temperature() {
        let a = ModelConfig {
            provider: ProviderKind::OpenAi,
            model: "gpt".into(),
            base_url: None,
            seed: None,
            temperature: 0.0,
            max_tokens: None,
        };
        let mut b = a.clone();
        b.temperature = 0.7;
        assert_ne!(a.config_hash(), b.config_hash());
    }
}

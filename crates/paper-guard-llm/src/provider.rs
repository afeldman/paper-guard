//! Core types for the LLM provider abstraction.

use paper_guard_core::ContentHash;

/// The kind of LLM provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    OpenAiCompatible,
    Local,
    Mock,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ProviderKind::OpenAi => "openai",
                ProviderKind::Anthropic => "anthropic",
                ProviderKind::OpenAiCompatible => "openai_compatible",
                ProviderKind::Local => "local",
                ProviderKind::Mock => "mock",
            }
        )
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "openai" => Ok(ProviderKind::OpenAi),
            "anthropic" => Ok(ProviderKind::Anthropic),
            "openai_compatible" | "openai-compatible" => Ok(ProviderKind::OpenAiCompatible),
            "local" => Ok(ProviderKind::Local),
            "mock" => Ok(ProviderKind::Mock),
            other => Err(format!("unknown provider kind: {other}")),
        }
    }
}

/// Configuration for a single model assignment.
///
/// Reviewers are assigned a provider + model via configuration; this struct
/// captures that assignment plus reproducibility-relevant parameters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    /// Provider kind.
    pub provider: ProviderKind,
    /// Model identifier (provider-specific).
    pub model: String,
    /// Base URL for OpenAI-compatible / local endpoints.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Determinism seed if supported.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Temperature.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// Max output tokens.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

fn default_temperature() -> f32 {
    0.0
}

impl ModelConfig {
    /// A stable hash of the full configuration, for reproducibility.
    pub fn config_hash(&self) -> ContentHash {
        ContentHash::compute(self)
    }
}

/// An image part for multimodal requests.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LlmImage {
    /// Media type, e.g. `image/png`.
    pub media_type: String,
    /// Base64-encoded image bytes.
    pub base64: String,
}

/// A piece of an LLM response (kept for forward compatibility).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LlmContent {
    /// Text content.
    Text(String),
    /// An image in the response (unused in v1).
    Image(String),
}

/// The async provider trait. Implementations belong behind this trait so that
/// reviewers never depend on a concrete provider.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Generate a completion for the request.
    async fn generate(&self, request: crate::LlmRequest) -> anyhow::Result<crate::LlmResponse>;

    /// The provider kind, for logging.
    fn kind(&self) -> ProviderKind;

    /// The capabilities this provider/endpoint/model actually supports.
    ///
    /// When a requested capability (e.g. vision) is not supported, the
    /// provider *must* fail explicitly rather than silently dropping content.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::TEXT_AND_STRUCTURED
    }
}

/// Capabilities a provider/endpoint/model may expose.
///
/// A capability is *claimed* only when the configured endpoint really supports
/// it. A provider must never pretend to have reviewed a modality (e.g. an
/// image) that it was not able to send. If a reviewer requires a capability
/// the configured endpoint does not provide, the provider fails with an
/// explicit capability error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilities {
    /// Plain text input / output.
    pub text: bool,
    /// Structured JSON output (`response_format`).
    pub structured_output: bool,
    /// Multimodal image input.
    pub vision: bool,
}

impl ProviderCapabilities {
    pub const TEXT_ONLY: Self = ProviderCapabilities {
        text: true,
        structured_output: false,
        vision: false,
    };
    pub const TEXT_AND_STRUCTURED: Self = ProviderCapabilities {
        text: true,
        structured_output: true,
        vision: false,
    };
    pub const TEXT_STRUCTURED_AND_VISION: Self = ProviderCapabilities {
        text: true,
        structured_output: true,
        vision: true,
    };

    /// Whether every requested capability is present.
    pub fn supports(&self, required: ProviderCapabilities) -> bool {
        (!required.text || self.text)
            && (!required.structured_output || self.structured_output)
            && (!required.vision || self.vision)
    }
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        ProviderCapabilities::TEXT_AND_STRUCTURED
    }
}

/// A capability that a requested operation required but the provider does not
/// support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderCapabilityError {
    #[error("this provider does not support required structured JSON output")]
    StructuredOutputUnsupported,
    #[error("this provider does not support required multimodal vision input")]
    VisionUnsupported,
}

/// Errors surfaced from a provider.
///
/// The error is classified as [`ProviderError::Transient`] when a retry
/// could plausibly succeed (timeout, connection error, rate limit, temporary
/// provider 5xx). Permanent errors (auth, invalid request, invalid config,
/// malformed schema) must never be retried. A transient error carries a
/// bounded retry count so the policy cannot create retry storms.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    /// A retryable failure (timeout / connection / 429 / 5xx).
    #[error("transient provider error: {kind}: {message}")]
    Transient {
        kind: TransientKind,
        message: String,
    },
    /// Authentication or authorization failure (`401` / `403` / missing key).
    #[error("authentication error: {0}")]
    Auth(String),
    /// The request was rejected as invalid by the endpoint (e.g. `400`).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The endpoint returned JSON we could not decode.
    #[error("malformed provider response: {0}")]
    MalformedResponse(String),
    /// The requested capability is not supported by the configured endpoint.
    #[error("capability not supported: {0}")]
    Capability(ProviderCapabilityError),
    /// Configuration problem (missing base URL, unknown env var, etc.).
    #[error("provider configuration error: {0}")]
    Config(String),
    /// The request context window was exceeded.
    #[error("context length exceeded: {0}")]
    ContextLength(String),
    /// A permanent, non-retryable error from the provider.
    #[error("provider error: {0}")]
    Other(String),
}

impl ProviderError {
    /// Whether this error is plausibly transient and safe to retry.
    pub fn is_transient(&self) -> bool {
        matches!(self, ProviderError::Transient { .. })
    }
}

/// The kind of a transient error (used for classification / logging).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientKind {
    Timeout,
    Connection,
    RateLimit,
    ServerError,
}

impl std::fmt::Display for TransientKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                TransientKind::Timeout => "timeout",
                TransientKind::Connection => "connection",
                TransientKind::RateLimit => "rate_limit",
                TransientKind::ServerError => "server_error",
            }
        )
    }
}

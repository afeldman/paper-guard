//! An OpenAI-compatible chat-completions provider.
//!
//! This is the single production provider used to talk to any OpenAI-compatible
//! endpoint (OpenAI, Mammoth.ai, local servers, or any other compatible
//! service). The difference between those backends is *configuration* only —
//! an endpoint's `base_url`, `model`, and API key environment variable — never
//! code. The rest of Paper Guard only ever sees the [`crate::LlmProvider`]
//! trait.
//!
//! The implementation is a thin, dependency-light HTTP client against the
//! `/chat/completions` endpoint. No SDK is required, which keeps the provider
//! completely isolated inside `paper-guard-llm`.

use std::time::Duration;

use reqwest::StatusCode;

use crate::{
    ContentPart, LlmProvider, LlmRequest, LlmResponse, LlmUsage, ModelConfig, ProviderCapabilities,
    ProviderError, ProviderKind, StructuredOutputMode, TransientKind,
};

/// Retry / backoff policy for the OpenAI-compatible provider.
///
/// Only [`ProviderError`]s classified as transient (timeout, connection,
/// rate-limit, provider 5xx) are retried, up to a bounded number of attempts
/// with exponential backoff. Permanent errors (auth, invalid request, invalid
/// schema, config) are never retried.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retries (in addition to the initial attempt).
    pub max_retries: u32,
    /// Base backoff delay before the first retry (seconds).
    pub base_backoff_seconds: u64,
    /// Multiplier applied to the backoff per retry.
    pub backoff_multiplier: f64,
    /// Cap on a single backoff delay (seconds).
    pub max_backoff_seconds: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_retries: 2,
            base_backoff_seconds: 1,
            backoff_multiplier: 2.0,
            max_backoff_seconds: 8,
        }
    }
}

/// Configuration for an [`OpenAICompatibleProvider`].
///
/// Secrets (the API key) are never stored here; only the name of the
/// environment variable that holds them is recorded.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenAICompatibleConfig {
    /// The endpoint base URL, e.g. `https://api.openai.com/v1` or a Mammoth.ai
    /// OpenAI-compatible endpoint.
    pub base_url: String,
    /// The environment variable that holds the API key, e.g. `OPENAI_API_KEY`.
    /// If empty / absent, requests are sent without an Authorization header
    /// (suitable for local endpoints that do not require a key).
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// The model identifier (configuration-driven, never hard-coded).
    pub model: String,
    /// Temperature (0..=2).
    #[serde(default)]
    pub temperature: f32,
    /// Request timeout.
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    /// Retry / backoff policy.
    #[serde(default)]
    pub retry: RetryPolicy,
    /// Maximum output tokens, if any.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Capabilities the configured endpoint/model actually supports.
    #[serde(default)]
    pub capabilities: ProviderCapabilities,
    /// How this OpenAI-compatible endpoint constrains its structured output
    /// at the transport layer. Chosen by configuration; the provider never
    /// silently downgrades from a stricter mode to a looser one. The endpoint
    /// must support the chosen mode or the provider fails explicitly.
    #[serde(default = "default_structured_mode")]
    pub structured_output: StructuredOutputMode,
}

fn default_timeout_seconds() -> u64 {
    120
}

fn default_structured_mode() -> StructuredOutputMode {
    StructuredOutputMode::JsonObject
}

impl Default for OpenAICompatibleConfig {
    fn default() -> Self {
        OpenAICompatibleConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key_env: Some("OPENAI_API_KEY".into()),
            model: "gpt-4o-mini".into(),
            temperature: 0.0,
            timeout_seconds: default_timeout_seconds(),
            retry: RetryPolicy::default(),
            max_tokens: None,
            capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
            structured_output: default_structured_mode(),
        }
    }
}

impl OpenAICompatibleConfig {
    /// Build a config from a [`ModelConfig`] plus a base URL / key env / retry.
    ///
    /// The `model` and capability-relevant parameters come from the model
    /// assignment; the endpoint-global parameters (base URL, key env, retry,
    /// timeout) come from the `[providers.openai-compatible]` section.
    pub fn from_model_config(
        model: &ModelConfig,
        base_url: &str,
        api_key_env: Option<String>,
        retry: RetryPolicy,
        timeout_seconds: u64,
        capabilities: ProviderCapabilities,
        structured_output: StructuredOutputMode,
    ) -> Self {
        OpenAICompatibleConfig {
            base_url: base_url.to_string(),
            api_key_env,
            model: model.model.clone(),
            temperature: model.temperature,
            timeout_seconds,
            retry,
            max_tokens: model.max_tokens,
            capabilities,
            structured_output,
        }
    }
}

/// An HTTP client for an OpenAI-compatible chat-completions endpoint.
#[derive(Debug, Clone)]
pub struct OpenAICompatibleProvider {
    config: OpenAICompatibleConfig,
    client: reqwest::Client,
    /// Cached API key pulled from the environment at construction so it is
    /// never stored in a committed config or re-read (and re-logged) per call.
    api_key: Option<String>,
}

impl OpenAICompatibleProvider {
    /// Construct a provider from configuration. Reads the API key from the
    /// configured environment variable (if any) once, up front.
    pub fn new(config: OpenAICompatibleConfig) -> anyhow::Result<Self> {
        let api_key = match &config.api_key_env {
            Some(var) if !var.trim().is_empty() => {
                let value = std::env::var(var).map_err(|_| {
                    ProviderError::Config(format!(
                        "required environment variable `{var}` is not set; refusing to construct \
                         an OpenAI-compatible provider without its API key"
                    ))
                })?;
                Some(value)
            }
            _ => None,
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.max(1)))
            .build()
            .map_err(|e| ProviderError::Config(format!("failed to build HTTP client: {e}")))?;

        Ok(OpenAICompatibleProvider {
            config,
            client,
            api_key,
        })
    }

    /// Read the current config (for introspection / logging safely — it never
    /// contains the API key itself, only the env var name).
    pub fn config(&self) -> &OpenAICompatibleConfig {
        &self.config
    }

    /// The chat-completions URL for the configured base URL.
    fn chat_completions_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    /// Build the request body for a single attempt.
    fn build_request_body(&self, request: &LlmRequest) -> Result<serde_json::Value, ProviderError> {
        let mut messages = Vec::new();
        if !request.system.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": request.system,
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": build_user_content(&request.user),
        }));

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
        });
        // The provider may supply a default max_tokens when the request does
        // not prescribe one (configuration-driven parameters).
        if let Some(max_tokens) = request.max_tokens.or(self.config.max_tokens) {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if request.seed.is_some() {
            // Many OpenAI-compatible endpoints accept `seed`; harmless when
            // ignored, and recorded for reproducibility when honoured.
            body["seed"] = serde_json::json!(request.seed);
        }
        // Structured output: encode the configured mode. This never silently
        // downgrades — a mode that cannot be satisfied returns an explicit
        // capability/config error instead.
        self.add_structured_output(request, &mut body)?;
        Ok(body)
    }

    /// Encode `response_format` according to the configured mode.
    ///
    /// * `JsonObject` emits `{"type":"json_object"}`.
    /// * `JsonSchema` emits `{"type":"json_schema", ...}` using the schema from
    ///   the incoming [`LlmRequest`]; if none is attached, that is a capability
    ///   error (we never downgrade to unconstrained generation).
    /// * `Off` sends no `response_format` (free-form text) — the historical
    ///   `structured_output = false` behaviour.
    fn add_structured_output(
        &self,
        request: &LlmRequest,
        body: &mut serde_json::Value,
    ) -> Result<(), ProviderError> {
        if !self.config.capabilities.structured_output {
            // The endpoint/model does not support structural JSON at all. If the
            // operator explicitly asked for it, fail clearly rather than degrade.
            if self.config.structured_output != StructuredOutputMode::Off {
                return Err(ProviderError::Capability(
                    crate::ProviderCapabilityError::StructuredOutputUnsupported,
                ));
            }
            return Ok(());
        }
        match self.config.structured_output {
            StructuredOutputMode::Off => Ok(()),
            StructuredOutputMode::JsonObject => {
                body["response_format"] = serde_json::json!({"type": "json_object"});
                Ok(())
            }
            StructuredOutputMode::JsonSchema => {
                let spec = request.structured_output.as_ref().ok_or({
                    ProviderError::Capability(
                        crate::ProviderCapabilityError::StructuredOutputUnsupported,
                    )
                })?;
                body["response_format"] = serde_json::json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": spec.name,
                        "strict": spec.strict,
                        "schema": spec.schema,
                    },
                });
                Ok(())
            }
        }
    }

    /// Perform a single HTTP call and classify the result.
    async fn attempt(&self, request: &LlmRequest) -> Result<LlmResponse, ProviderError> {
        let url = self.chat_completions_url();
        let body = self.build_request_body(request)?;

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await.map_err(|e| classify_send_error(&e))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if status.is_success() {
            return parse_success(&text);
        }

        Err(classify_status(status, &text))
    }
}

/// Build the `content` array for the user message, mixing text and images.
fn build_user_content(parts: &[ContentPart]) -> serde_json::Value {
    // If everything is plain text, send a single string (simplest, most widely
    // compatible).
    if parts.iter().all(|p| matches!(p, ContentPart::Text(_))) {
        let text = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.clone()),
                ContentPart::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        return serde_json::Value::String(text);
    }

    // Mixed text + images: use the OpenAI multimodal content array.
    let mut content = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text(t) => content.push(serde_json::json!({
                "type": "text",
                "text": t,
            })),
            ContentPart::Image(img) => content.push(serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{mime};base64,{b64}", mime = img.media_type, b64 = img.base64),
                },
            })),
        }
    }
    serde_json::Value::Array(content)
}

/// Parse a successful 2xx response body into an [`LlmResponse`].
fn parse_success(body: &str) -> Result<LlmResponse, ProviderError> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        ProviderError::MalformedResponse(format!("invalid JSON in success body: {e}"))
    })?;

    let choices = value
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            ProviderError::MalformedResponse("response missing `choices` array".into())
        })?;
    let choice = choices.first().ok_or_else(|| {
        ProviderError::MalformedResponse("response has an empty `choices` array".into())
    })?;

    // Support both `choices[0].message.content` (string) and newer
    // `choices[0].message.content` as an array of parts.
    let text = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .map(extract_content_text)
        .ok_or_else(|| {
            ProviderError::MalformedResponse("response message missing `content`".into())
        })?;

    let usage = value.get("usage").map(|u| LlmUsage {
        prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        completion_tokens: u
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    });

    Ok(LlmResponse { text, usage })
}

/// Extract text out of a message content value (string or array of parts).
fn extract_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Classify a low-level HTTP send error (connection / timeout).
fn classify_send_error(e: &reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        return ProviderError::Transient {
            kind: TransientKind::Timeout,
            message: e.to_string(),
        };
    }
    if e.is_connect() {
        return ProviderError::Transient {
            kind: TransientKind::Connection,
            message: e.to_string(),
        };
    }
    ProviderError::Transient {
        kind: TransientKind::Connection,
        message: e.to_string(),
    }
}

/// Classify a non-2xx HTTP status into a provider error.
fn classify_status(status: StatusCode, body: &str) -> ProviderError {
    let code = status.as_u16();
    // Rate limit and server errors are transient.
    if code == 429 {
        return ProviderError::Transient {
            kind: TransientKind::RateLimit,
            message: sanitize(body),
        };
    }
    if code >= 500 {
        return ProviderError::Transient {
            kind: TransientKind::ServerError,
            message: sanitize(body),
        };
    }
    match code {
        401 | 403 => ProviderError::Auth(format!("HTTP {code}: {}", sanitize(body))),
        400 if body.to_lowercase().contains("context") || body.to_lowercase().contains("token") => {
            ProviderError::ContextLength(sanitize(body))
        }
        400 => ProviderError::InvalidRequest(format!("HTTP {code}: {}", sanitize(body))),
        other => ProviderError::Other(format!("HTTP {other}: {}", sanitize(body))),
    }
}

/// Sanitize a server response body for inclusion in an error message: strip
/// anything that looks like an embedded secret and cap its length.
fn sanitize(body: &str) -> String {
    let mut out = body.trim().to_string();
    const MAX: usize = 400;
    if out.len() > MAX {
        out.truncate(MAX);
        out.push('…');
    }
    // Best-effort: avoid echoing a full bearer key if the server echoes it.
    if let Some(pos) = out.find("sk-") {
        out = format!("{}[redacted]", &out[..pos]);
    }
    out
}

#[async_trait::async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn generate(&self, request: LlmRequest) -> anyhow::Result<LlmResponse> {
        // Capability gate: a request that cannot be honoured must fail
        // explicitly, never silently drop content.
        if request.needs_image_capability() && !self.config.capabilities.vision {
            return Err(ProviderError::Capability(
                crate::ProviderCapabilityError::VisionUnsupported,
            )
            .into());
        }

        let max_retries = self.config.retry.max_retries;
        let mut attempt_no = 0u32;

        loop {
            match self.attempt(&request).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    if !err.is_transient() || attempt_no >= max_retries {
                        return Err(err.into());
                    }
                    attempt_no += 1;
                    let backoff = backoff_for(&self.config.retry, attempt_no);
                    log_retry(&request, &err, attempt_no, backoff, max_retries);
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.config.capabilities
    }
}

/// Compute the backoff delay for a given retry attempt (1-indexed).
fn backoff_for(policy: &RetryPolicy, attempt: u32) -> Duration {
    let base =
        policy.base_backoff_seconds as f64 * policy.backoff_multiplier.powf((attempt - 1) as f64);
    Duration::from_secs(base.round().min(policy.max_backoff_seconds as f64).max(1.0) as u64)
}

/// Minimal retry logging that never includes API keys, secrets, or the full
/// request payload. Only the run context, provider, model, error category, and
/// retry count are surfaced.
fn log_retry(request: &LlmRequest, err: &ProviderError, attempt: u32, backoff: Duration, max: u32) {
    let msg = format!(
        "provider transient error; model={} retry={}/{} backoff_ms={} error_category={}",
        request.model,
        attempt,
        max,
        backoff.as_millis(),
        transient_category_name(err),
    );
    // Emit via the standard tracing facade so rust_loguru integrations capture
    // it; never implies the raw request or API key.
    tracing::info!("{msg}");
}

fn transient_category_name(e: &ProviderError) -> String {
    match e {
        ProviderError::Transient { kind, .. } => format!("{kind}"),
        _ => "none".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_defaults_are_bounded() {
        let p = RetryPolicy::default();
        assert!(p.max_retries >= 1);
        assert!(p.max_retries <= 5);
        assert!(p.backoff_multiplier >= 1.0);
    }

    #[test]
    fn backoff_grows_and_caps() {
        let p = RetryPolicy {
            max_retries: 5,
            base_backoff_seconds: 1,
            backoff_multiplier: 2.0,
            max_backoff_seconds: 8,
        };
        assert_eq!(backoff_for(&p, 1).as_secs(), 1);
        assert_eq!(backoff_for(&p, 2).as_secs(), 2);
        assert_eq!(backoff_for(&p, 3).as_secs(), 4);
        // Capped.
        assert_eq!(backoff_for(&p, 5).as_secs(), 8);
    }

    #[test]
    fn classify_429_transient_rate_limit() {
        let e = classify_status(StatusCode::TOO_MANY_REQUESTS, "slow down");
        assert!(e.is_transient());
    }

    #[test]
    fn classify_500_transient_server() {
        let e = classify_status(StatusCode::INTERNAL_SERVER_ERROR, "oops");
        assert!(e.is_transient());
    }

    #[test]
    fn classify_401_auth_not_retryable() {
        let e = classify_status(StatusCode::UNAUTHORIZED, "bad key");
        assert!(!e.is_transient());
        assert!(matches!(e, ProviderError::Auth(_)));
    }

    #[test]
    fn capability_error_is_not_transient() {
        let e = ProviderError::Capability(crate::ProviderCapabilityError::VisionUnsupported);
        assert!(!e.is_transient());
    }

    #[test]
    fn default_config_never_contains_a_secret() {
        let cfg = OpenAICompatibleConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("sk-"));
        // Only the env-var NAME is stored, never the key.
        assert!(json.contains("OPENAI_API_KEY"));
    }
}

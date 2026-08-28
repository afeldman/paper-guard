//! LM Studio compatibility contract tests.
//!
//! LM Studio exposes an OpenAI-compatible `/v1` chat-completions endpoint
//! (default port 1234) and typically needs NO API key. Paper Guard routes it
//! through the SAME `OpenAICompatibleProvider` as OpenAI / Mammoth.ai / Ollama.
//!
//! These tests use a mocked OpenAI-compatible endpoint (wiremock) and never
//! require a running LM Studio instance or internet access. They pin the
//! exact first real E2E configuration (base_url `http://localhost:1234/v1`,
//! model `qwen/qwen3.5-9b`, `api_key_env = None`) to prove the provider
//! reaches that endpoint without ever requesting `OPENAI_API_KEY`.

use paper_guard_llm::{
    LlmProvider, LlmRequest, OpenAICompatibleConfig, OpenAICompatibleProvider,
    ProviderCapabilities, RetryPolicy, StructuredOutputMode,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The exact model identifier used by the local LM Studio instance.
const LS_MODEL: &str = "qwen/qwen3.5-9b";

/// Build a provider configured exactly like the LM Studio endpoint:
/// base_url on port 1234, keyless (`api_key_env = None`).
fn lmstudio_provider(server: &MockServer) -> OpenAICompatibleProvider {
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", server.uri()), // LM Studio's OpenAI-compatible prefix
        api_key_env: None,                        // LM Studio needs no key
        model: LS_MODEL.into(),
        temperature: 0.0,
        timeout_seconds: 5,
        retry: RetryPolicy {
            max_retries: 2,
            base_backoff_seconds: 0,
            backoff_multiplier: 1.0,
            max_backoff_seconds: 0,
        },
        max_tokens: Some(1024),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonObject,
    };
    OpenAICompatibleProvider::new(cfg).unwrap()
}

fn ok_payload(payload: &str) -> ResponseTemplate {
    let body = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": payload}}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 6}
    });
    ResponseTemplate::new(200).set_body_json(&body)
}

#[tokio::test]
async fn lmstudio_construction_requires_no_env_key() {
    // Because api_key_env is None, constructing the provider must succeed even
    // with no environment variable set. This is the core guarantee for the
    // real LM Studio E2E run: it must never request OPENAI_API_KEY.
    let cfg = OpenAICompatibleConfig {
        base_url: "http://localhost:1234/v1".into(),
        api_key_env: None,
        model: LS_MODEL.into(),
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();
    // Introspect safely: the env-var name is absent, so there is no key path.
    assert!(provider.config().api_key_env.is_none());
}

#[tokio::test]
async fn lmstudio_is_reached_without_an_authorization_header() {
    let mock_server = MockServer::start().await;
    let provider = lmstudio_provider(&mock_server);
    let payload = r#"[{"finding_id":"PG-LS","finding":"local finding"}]"#;

    // The request must hit LM Studio's `/v1/chat/completions` with the exact
    // local model identifier and carry NO Authorization header.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": LS_MODEL,
            "max_tokens": 1024,
            "messages": [
                {"role": "system", "content": "integrity preamble"},
                {"role": "user", "content": "review this"}
            ]
        })))
        .respond_with(ok_payload(payload))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new(LS_MODEL, "integrity preamble", "review this", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, payload);
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 12);
    assert_eq!(usage.completion_tokens, 6);

    // Because api_key_env is None, the outgoing request must carry NO
    // Authorization header.
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert!(received[0].headers.get("authorization").is_none());
}

//! Provider contract tests for [`OpenAICompatibleProvider`] using a local
//! mock HTTP server (wiremock). These tests are entirely offline: no real API
//! key or network is required.

use paper_guard_llm::{
    LlmImage, LlmProvider, LlmRequest, OpenAICompatibleConfig, OpenAICompatibleProvider,
    ProviderCapabilities, ProviderError, ProviderKind, RetryPolicy, StructuredOutputMode,
    TransientKind,
};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET_KEY: &str = "sk-test-secret-value-123456";

/// Generate a unique env-var name per test so parallel tests never race on the
/// process-global environment.
fn unique_env() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("PAPER_GUARD_TEST_KEY_{n}")
}

/// Set the given environment key. Returns its env-var name.
fn new_env_var(secret: &str) -> String {
    let name = unique_env();
    std::env::set_var(&name, secret);
    name
}

/// Build a provider pointed at a mock server with the given retry policy and a
/// fresh, unique API-key environment variable.
fn provider_for(server: &MockServer, retry: RetryPolicy) -> OpenAICompatibleProvider {
    let key_env = new_env_var(SECRET_KEY);
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", server.uri()),
        api_key_env: Some(key_env),
        model: "test-model".into(),
        temperature: 0.0,
        timeout_seconds: 5,
        retry,
        max_tokens: Some(128),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonObject,
    };
    OpenAICompatibleProvider::new(cfg).unwrap()
}

fn ok_payload(payload: &str) -> ResponseTemplate {
    // Build the OpenAI-shaped success body with serde so the inner payload is
    // correctly escaped as a JSON string value.
    let body = serde_json::json!({
        "choices": [
            {"message": {"role": "assistant", "content": payload}}
        ],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5}
    });
    ResponseTemplate::new(200).set_body_json(&body)
}

#[tokio::test]
async fn request_construction_matches_contract() {
    let mock_server = MockServer::start().await;
    let provider = provider_for(&mock_server, RetryPolicy::default());

    let payload = r#"[{"finding_id":"PG-1","finding":"x"}]"#;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("Authorization", format!("Bearer {SECRET_KEY}")))
        .and(body_partial_json(serde_json::json!({
            "model": "test-model",
            "temperature": 0.0,
            "max_tokens": 128,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": "system prompt"},
                {"role": "user", "content": "user content"},
            ],
        })))
        .respond_with(ok_payload(payload))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "system prompt", "user content", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, payload);
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);
}

#[tokio::test]
async fn missing_api_key_produces_config_error() {
    // A unique variable name that is guaranteed never to be set in this test
    // suite => construction must fail with a config error.
    let missing = unique_env();
    std::env::remove_var(&missing);
    let cfg = OpenAICompatibleConfig {
        base_url: "https://example.invalid/v1".into(),
        api_key_env: Some(missing),
        model: "m".into(),
        ..Default::default()
    };
    let err = OpenAICompatibleProvider::new(cfg).unwrap_err();
    let provider_err = err.downcast_ref::<ProviderError>();
    assert!(matches!(provider_err, Some(ProviderError::Config(_))));
}

#[tokio::test]
async fn malformed_success_body_is_rejected() {
    let mock_server = MockServer::start().await;
    let provider = provider_for(&mock_server, RetryPolicy::default());

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "s", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::MalformedResponse(_)));
}

#[tokio::test]
async fn auth_error_is_returned_and_not_retried() {
    let mock_server = MockServer::start().await;
    let retry = RetryPolicy {
        max_retries: 3,
        ..Default::default()
    };
    let provider = provider_for(&mock_server, retry);

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("{\"error\":\"bad key\"}"))
        .expect(1) // auth is NOT retried
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "s", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::Auth(_)));
    assert!(!err.is_transient());
}

#[tokio::test]
async fn rate_limit_is_retried_then_fails_after_limit() {
    let mock_server = MockServer::start().await;
    let retry = RetryPolicy {
        max_retries: 2,
        base_backoff_seconds: 0,
        backoff_multiplier: 1.0,
        max_backoff_seconds: 0,
    };
    let provider = provider_for(&mock_server, retry);

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("too many"))
        .expect(3) // 1 initial + 2 retries
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "s", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(err.is_transient());
    if let ProviderError::Transient { kind, .. } = err {
        assert_eq!(kind, TransientKind::RateLimit);
    } else {
        panic!("expected transient rate-limit error");
    }
}

#[tokio::test]
async fn server_error_is_retried_then_succeeds() {
    let mock_server = MockServer::start().await;
    let retry = RetryPolicy {
        max_retries: 2,
        base_backoff_seconds: 0,
        backoff_multiplier: 1.0,
        max_backoff_seconds: 0,
    };
    let provider = provider_for(&mock_server, retry);
    let payload = r#"[]"#;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_string("boom")
                .set_delay(std::time::Duration::from_millis(10)),
        )
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok_payload(payload))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "s", "u", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, payload);
}

#[tokio::test]
async fn permanent_invalid_request_is_not_retried() {
    let mock_server = MockServer::start().await;
    let retry = RetryPolicy {
        max_retries: 5,
        ..Default::default()
    };
    let provider = provider_for(&mock_server, retry);

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad param"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("test-model", "s", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::InvalidRequest(_)));
}

#[tokio::test]
async fn vision_request_without_capability_fails_explicitly() {
    let mock_server = MockServer::start().await;
    let provider = provider_for(&mock_server, RetryPolicy::default());

    // Capability failure is a pre-request gate: no HTTP call is expected.
    let req = LlmRequest::new("test-model", "s", "look", "v1").with_image(LlmImage {
        media_type: "image/png".into(),
        base64: "aGVsbG8=".into(),
    });
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::Capability(_)));
    assert!(!err.is_transient());
}

#[tokio::test]
async fn provider_kind_and_capabilities() {
    let mock_server = MockServer::start().await;
    let provider = provider_for(&mock_server, RetryPolicy::default());
    assert_eq!(provider.kind(), ProviderKind::OpenAiCompatible);
    assert!(provider.capabilities().text);
    assert!(provider.capabilities().structured_output);
    assert!(!provider.capabilities().vision);
}

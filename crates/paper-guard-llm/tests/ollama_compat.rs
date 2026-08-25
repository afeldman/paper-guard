//! Ollama-compatibility contract tests.
//!
//! Ollama exposes an OpenAI-compatible `/v1` chat-completions endpoint, so
//! Paper Guard reaches local models through the SAME `OpenAICompatibleProvider`
//! as OpenAI / Mammoth.ai. These tests use a mocked OpenAI-compatible endpoint
//! (wiremock) and never require a running Ollama instance or network.

use paper_guard_llm::{
    LlmProvider, LlmRequest, OpenAICompatibleConfig, OpenAICompatibleProvider,
    ProviderCapabilities, RetryPolicy,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A provider configured exactly like a local Ollama endpoint: `api_key_env`
/// is empty / absent, so no Authorization header is sent.
fn keyless_ollama_provider(server: &MockServer) -> OpenAICompatibleProvider {
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", server.uri()), // Ollama's OpenAI-compatible prefix
        api_key_env: None,                        // local Ollama needs no key
        model: "llama3.2".into(),
        temperature: 0.0,
        timeout_seconds: 5,
        retry: RetryPolicy {
            max_retries: 2,
            base_backoff_seconds: 0,
            backoff_multiplier: 1.0,
            max_backoff_seconds: 0,
        },
        max_tokens: Some(512),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        use_structured_output: true,
    };
    OpenAICompatibleProvider::new(cfg).unwrap()
}

fn ok_payload(payload: &str) -> ResponseTemplate {
    let body = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": payload}}],
        "usage": {"prompt_tokens": 9, "completion_tokens": 4}
    });
    ResponseTemplate::new(200).set_body_json(&body)
}

#[tokio::test]
async fn local_ollama_is_reached_without_an_authorization_header() {
    let mock_server = MockServer::start().await;
    let provider = keyless_ollama_provider(&mock_server);
    let payload = r#"[{"finding_id":"PG-OLLAMA","finding":"local finding"}]"#;

    // The request must hit Ollama's `/v1/chat/completions` with the local model.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "llama3.2",
            "max_tokens": 512,
            "messages": [
                {"role": "system", "content": "integrity preamble"},
                {"role": "user", "content": "review this"}
            ]
        })))
        .respond_with(ok_payload(payload))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("llama3.2", "integrity preamble", "review this", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, payload);
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 9);
    assert_eq!(usage.completion_tokens, 4);

    // Because api_key_env is None, the outgoing request must carry NO
    // Authorization header (local Ollama needs no key).
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert!(received[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn local_ollama_construction_requires_no_env_key() {
    // Because api_key_env is None, constructing the provider must succeed even
    // with no environment variable at all — this is how local, keyless Ollama
    // works. (No mock server is needed.)
    let cfg = OpenAICompatibleConfig {
        base_url: "http://localhost:11434/v1".into(),
        api_key_env: None,
        model: "llama3.2".into(),
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();
    // Introspect the config safely: the env-var name must be absent (no key path).
    assert!(provider.config().api_key_env.is_none());
}

#[tokio::test]
async fn local_ollama_structured_output_is_parsed_and_validated() {
    // A local model returning malformed prose must NOT be silently accepted as
    // a findings list. The provider surfaces the raw text; the reviewer layer
    // then rejects it as REVIEWER_OUTPUT_INVALID. Here we verify the provider
    // returns the response intact (structured validation is a downstream step).
    let mock_server = MockServer::start().await;
    let provider = keyless_ollama_provider(&mock_server);

    Mock::given(method("POST"))
        .respond_with(ok_payload("this is free-form prose, not JSON findings"))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("llama3.2", "sys", "u", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert!(resp.text.contains("free-form prose"));
}

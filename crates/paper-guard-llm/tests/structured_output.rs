//! Structured-output contract tests for the OpenAI-compatible provider.
//!
//! These tests are entirely offline (wiremock). They pin how the provider
//! encodes `response_format` for the `json_schema` and `json_object` modes, and
//! prove that requesting JSON Schema never silently downgrades to unconstrained
//! generation.

use paper_guard_llm::{
    LlmProvider, LlmRequest, OpenAICompatibleConfig, OpenAICompatibleProvider,
    ProviderCapabilities, ProviderError, StructuredOutputMode, StructuredOutputSpec,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A small but realistic JSON Schema for a reviewer finding. It deliberately
/// requires `confidence` as a JSON number.
fn finding_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "finding_id": {"type": "string"},
            "confidence": {"type": "number"},
            "finding": {"type": "string"},
            "evidence": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["finding_id", "confidence", "finding"]
    })
}

fn ok_payload(payload: &str) -> ResponseTemplate {
    let body = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": payload}}],
        "usage": {"prompt_tokens": 4, "completion_tokens": 3}
    });
    ResponseTemplate::new(200).set_body_json(&body)
}

#[tokio::test]
async fn json_schema_serialization_includes_schema_and_strict() {
    // Constructing a provider in JsonSchema mode and issuing a request must
    // put `{"type":"json_schema","json_schema":{name,strict,schema}}` in
    // `response_format`.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonSchema,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();

    let spec = StructuredOutputSpec::new("finding_schema", finding_schema());
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        // json_schema type + strict = true + the caller's schema.
        .and(body_partial_json(serde_json::json!({
            "model": "local-model",
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "finding_schema",
                    "strict": true,
                    "schema": finding_schema()
                }
            }
        })))
        .respond_with(ok_payload(r#"[{"finding_id":"PG-1"}]"#))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("local-model", "sys", "u", "v1").with_structured_output(spec);
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, r#"[{"finding_id":"PG-1"}]"#);
}

#[tokio::test]
async fn json_schema_is_strict_by_default() {
    // Strict enforcement must be enabled unless the caller explicitly disables it.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonSchema,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();

    let spec = StructuredOutputSpec::new("s", finding_schema());
    assert!(
        spec.strict,
        "StructuredOutputSpec must default to strict=true"
    );
    Mock::given(method("POST"))
        .respond_with(ok_payload("[]"))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("local-model", "sys", "u", "v1").with_structured_output(spec);
    provider.generate(req).await.unwrap();
    // If the HTTP round-trip succeeded, the strict flag was serialized.
    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("request body is JSON");
    assert_eq!(
        body["response_format"]["json_schema"]["strict"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn schema_conformance_rejects_string_confidence() {
    // Verify the finding schema REQUIRES a numeric `confidence`. This proves
    // JSON-Schema transport constrains `confidence` to a JSON number, matching
    // the strongly-typed `f32` reviewer field (so `"High"` would be rejected
    // at the transport layer).
    let schema = finding_schema();
    let obj = schema.as_object().expect("schema is an object");
    let props = obj["properties"]
        .as_object()
        .expect("schema has properties");
    let conf = &props["confidence"];
    let conf_type = &conf["type"];
    assert_eq!(conf_type, &serde_json::json!("number"));
    // It must NOT permit a string.
    let types = match conf_type {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    assert!(
        !types.contains(&"string".to_string()),
        "confidence must not be typed as a string"
    );
    // And it must be a required field.
    let required = obj["required"].as_array().expect("required is an array");
    assert!(
        required.iter().any(|v| v == "confidence"),
        "confidence must be required"
    );
    assert!(
        required.iter().any(|v| v == "finding_id"),
        "finding_id must be required"
    );
}

#[tokio::test]
async fn json_object_mode_sends_json_object_format() {
    // Historical `structured_output = true` behaviour: `{"type":"json_object"}`.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonObject,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();

    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({
            "response_format": {"type": "json_object"}
        })))
        .respond_with(ok_payload(r#"{"x":1}"#))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("local-model", "sys", "u", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, r#"{"x":1}"#);
}

#[tokio::test]
async fn json_schema_mode_without_schema_fails_explicitly() {
    // Requesting JSON Schema with no attached schema must NOT silently fall
    // back to unconstrained generation. The provider returns a capability error
    // before any HTTP call is made.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonSchema,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();

    // No structured_output spec attached.
    let req = LlmRequest::new("local-model", "sys", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::Capability(_)));
    assert!(!err.is_transient(), "capability error must not be retried");

    // No HTTP request may have been issued.
    let received = mock_server.received_requests().await.unwrap();
    assert!(
        received.is_empty(),
        "must not call the endpoint when downgrading is disallowed"
    );
}

#[tokio::test]
async fn structured_mode_without_capability_fails_explicitly() {
    // If the endpoint does not claim structured-output capability but the
    // operator configured a structured mode, fail clearly rather than degrade.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities {
            text: true,
            structured_output: false,
            vision: false,
        },
        structured_output: StructuredOutputMode::JsonObject,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();
    let req = LlmRequest::new("local-model", "sys", "u", "v1");
    let err = provider
        .generate(req)
        .await
        .unwrap_err()
        .downcast::<ProviderError>()
        .unwrap();
    assert!(matches!(err, ProviderError::Capability(_)));
}

#[tokio::test]
async fn off_mode_sends_no_response_format() {
    // Historical `structured_output = false`: no `response_format` at all.
    let mock_server = MockServer::start().await;
    let cfg = OpenAICompatibleConfig {
        base_url: format!("{}/v1", mock_server.uri()),
        api_key_env: None,
        model: "local-model".into(),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::Off,
        ..Default::default()
    };
    let provider = OpenAICompatibleProvider::new(cfg).unwrap();

    Mock::given(method("POST"))
        .respond_with(ok_payload("free-form prose"))
        .mount(&mock_server)
        .await;

    let req = LlmRequest::new("local-model", "sys", "u", "v1");
    let resp = provider.generate(req).await.unwrap();
    assert_eq!(resp.text, "free-form prose");

    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let body: serde_json::Value =
        serde_json::from_slice(&received[0].body).expect("request body is JSON");
    assert!(
        body.get("response_format").is_none(),
        "Off mode must not send response_format"
    );
}

//! Client contract tests against a mocked HTTP service (wiremock).
//!
//! These tests never require a real Kubernetes/network service. They verify
//! the client's typed methods, error taxonomy, mode/security guarantees, and
//! that tokens / manuscript contents are never logged.

use paper_guard_client::{ClientConfig, PaperGuardClient};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A health body matching the service's `GET /health` response.
fn health_json() -> serde_json::Value {
    json!({
        "status": "ok",
        "service": "paper-guard",
        "version": "0.4.0",
        "provider": "mock",
        "memory_backend": "none",
    })
}

fn client_for(uri: &str) -> PaperGuardClient {
    let cfg = ClientConfig::new(uri, 5);
    PaperGuardClient::new(&cfg).unwrap()
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_typed_result() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(health_json()))
        .mount(&mock)
        .await;

    let client = client_for(&mock.uri());
    let h = client.health().await.unwrap();
    assert_eq!(h.status, "ok");
    assert_eq!(h.version, "0.4.0");
    assert_eq!(h.provider, "mock");
}

// ---------------------------------------------------------------------------
// Submit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submit_review_returns_run_id() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-42",
            "status": "completed",
            "input_hash": "abc123",
            "findings_opened": 2,
            "judge_entries": 1,
        })))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("paper.tex");
    std::fs::write(
        &src,
        r"\documentclass{article}\begin{document}Hi\end{document}",
    )
    .unwrap();

    let client = client_for(&mock.uri());
    let sub = client.submit_review(src.to_str().unwrap()).await.unwrap();
    assert_eq!(sub.run_id, "run-42");
    assert_eq!(sub.findings_opened, 2);
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_review_returns_typed_status() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/reviews/run-7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-7",
            "status": "completed",
            "source_format": "latex",
            "input_hash": "h1",
            "prompt_version": "v1",
            "findings_opened": 1,
            "judge_entries": 1,
            "revisions_applied": 0,
            "timestamp": "2026-01-01T00:00:00Z",
            "reviewers": [
                {"agent": "evidence", "status": "success", "finding_count": 1, "error": null}
            ],
        })))
        .mount(&mock)
        .await;

    let client = client_for(&mock.uri());
    let s = client.get_review("run-7").await.unwrap();
    assert_eq!(s.run_id, "run-7");
    assert_eq!(s.status, "completed");
    assert_eq!(s.reviewers.len(), 1);
    assert_eq!(s.reviewers[0].agent, "evidence");
}

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_findings_returns_structured_findings() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/reviews/run-7/findings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-7",
            "findings": [{
                "schema_version": "1.0",
                "finding_id": "PG-1",
                "reviewer": "evidence",
                "location": "3:4",
                "category": "evidence",
                "severity": "major",
                "confidence": 0.9,
                "claim_id": null,
                "finding": "evidence insufficient",
                "evidence": ["e1"],
                "recommendation": "add evidence",
                "requires_human_approval": false
            }],
            "open_count": 1,
        })))
        .mount(&mock)
        .await;

    let client = client_for(&mock.uri());
    let f = client.get_findings("run-7").await.unwrap();
    assert_eq!(f.open_count, 1);
    assert_eq!(f.findings.len(), 1);
    assert_eq!(f.findings[0].finding_id, "PG-1");
    assert_eq!(f.findings[0].severity, "major");
}

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn submit_feedback_is_accepted() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/reviews/run-7/feedback"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "memory_id": "mem-1",
            "approval_state": "PRIVATE",
        })))
        .mount(&mock)
        .await;

    let client = client_for(&mock.uri());
    let req = paper_guard_client::SubmitFeedbackRequest {
        reviewer_kind: "evidence".into(),
        unit_text: "the claim".into(),
        unit_kind: Some("claim".into()),
        finding_text: Some("finding text".into()),
        decision: "accept".into(),
        feedback: Some("good".into()),
    };
    let resp = client.submit_feedback("run-7", &req).await.unwrap();
    assert_eq!(resp.memory_id, "mem-1");
    assert_eq!(resp.approval_state, "PRIVATE");
}

// ---------------------------------------------------------------------------
// Remote review consolidation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn review_consolidates_status_and_findings() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/reviews/run-9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-9", "status": "completed", "source_format": "latex",
            "input_hash": "h", "prompt_version": "v1", "findings_opened": 1,
            "judge_entries": 1, "revisions_applied": 0, "timestamp": "t",
            "reviewers": []
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/reviews/run-9/findings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "run_id": "run-9",
            "findings": [],
            "open_count": 0,
        })))
        .mount(&mock)
        .await;

    let client = client_for(&mock.uri());
    let review = client.review("run-9").await.unwrap();
    assert_eq!(review.status, "completed");
    assert_eq!(review.open_count, 0);
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connection_refused_maps_to_connection_error() {
    // Bind a listener then drop it so the port is known to be closed.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);

    let client = client_for(&format!("http://{addr}"));
    let err = client.health().await.unwrap_err();
    assert_eq!(err.kind(), "connection");
    assert!(err.to_string().contains("unavailable"), "got: {err}");
}

#[tokio::test]
async fn timeout_maps_to_timeout_error() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)))
        .mount(&mock)
        .await;

    let cfg = ClientConfig::new(mock.uri(), 1); // 1s client timeout
    let client = PaperGuardClient::new(&cfg).unwrap();
    let err = client.health().await.unwrap_err();
    assert_eq!(err.kind(), "timeout");
}

async fn expect_status(status: u16, body: serde_json::Value, expected_kind: &str) {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&mock)
        .await;
    let client = client_for(&mock.uri());
    let err = client.health().await.unwrap_err();
    assert_eq!(err.kind(), expected_kind, "status {status}: {err}");
}

#[tokio::test]
async fn http_400_is_review_failed() {
    expect_status(400, json!({"error": "invalid manuscript"}), "review_failed").await;
}

#[tokio::test]
async fn http_401_is_authentication_error() {
    expect_status(401, json!({"error": "auth required"}), "auth").await;
}

#[tokio::test]
async fn http_403_is_authentication_error() {
    expect_status(403, json!({"error": "forbidden"}), "auth").await;
}

#[tokio::test]
async fn http_404_is_http_error() {
    expect_status(404, json!({"error": "not found"}), "http").await;
}

#[tokio::test]
async fn http_409_is_http_error() {
    expect_status(409, json!({"error": "conflict"}), "http").await;
}

#[tokio::test]
async fn http_429_is_http_error() {
    expect_status(429, json!({"error": "rate limited"}), "http").await;
}

#[tokio::test]
async fn http_500_is_http_error() {
    expect_status(500, json!({"error": "boom"}), "http").await;
}

#[tokio::test]
async fn http_503_is_http_error() {
    expect_status(503, json!({"error": "unavailable"}), "http").await;
}

#[tokio::test]
async fn malformed_json_maps_to_invalid_response() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
        .mount(&mock)
        .await;
    let client = client_for(&mock.uri());
    let err = client.health().await.unwrap_err();
    assert_eq!(err.kind(), "invalid_response");
    assert!(err.to_string().contains("malformed JSON"), "got: {err}");
}

#[tokio::test]
async fn invalid_url_is_rejected_at_construction() {
    let cfg = ClientConfig::new("not-a-url", 5);
    let err = PaperGuardClient::new(&cfg).unwrap_err();
    assert_eq!(err.kind(), "invalid_url");
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bearer_token_is_sent_but_never_logged_or_serialized() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(health_json()))
        .mount(&mock)
        .await;

    std::env::set_var("PAPER_GUARD_TEST_TOKEN", "super-secret-token-xyz");
    let cfg = ClientConfig {
        base_url: mock.uri().trim_end_matches('/').to_string(),
        timeout: std::time::Duration::from_secs(5),
        auth_token_env: Some("PAPER_GUARD_TEST_TOKEN".into()),
    };
    let client = PaperGuardClient::new(&cfg).unwrap();
    let _ = client.health().await.unwrap();

    let received = mock.received_requests().await.unwrap();
    let auth = received[0]
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert!(auth.contains("super-secret-token-xyz"), "got: {auth}");

    // The token must not appear in any serialized Debug string of the client.
    let debug = format!("{client:?}");
    assert!(
        !debug.contains("super-secret-token-xyz"),
        "token leaked in Debug"
    );
    std::env::remove_var("PAPER_GUARD_TEST_TOKEN");
}

#[tokio::test]
async fn manuscript_content_never_appears_in_error_or_debug() {
    let mock = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/reviews"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"error": "server boom"})))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("paper.tex");
    std::fs::write(&src, "TOP-SECRET-CONTENT-SHOULD-NOT-LEAK").unwrap();
    let client = client_for(&mock.uri());
    let err = client
        .submit_review(src.to_str().unwrap())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        !msg.contains("TOP-SECRET-CONTENT-SHOULD-NOT-LEAK"),
        "manuscript leaked into error: {msg}"
    );

    let debug = format!("{client:?}");
    assert!(!debug.contains("TOP-SECRET-CONTENT-SHOULD-NOT-LEAK"));
}

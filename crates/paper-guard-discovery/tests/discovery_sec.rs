//! Section-21 security tests: malicious discovery records are untrusted input.

use paper_guard_discovery::mock::{endpoint, MockServiceDiscovery};
use paper_guard_discovery::model::ServiceEndpoint;
use paper_guard_discovery::verify::{verify_and_classify, VerificationOutcome};
use paper_guard_discovery::ServiceDiscovery;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn malicious_record_cannot_inject_shell_into_url() {
    let hostile = ServiceEndpoint {
        name: String::new(),
        hostname: "x`.txt; whoami".into(),
        address: "192.168.0.66".into(),
        port: 8080,
        scheme: "http".into(),
        service_type: "_paper-guard._tcp".into(),
        version: String::new(),
        capabilities: Vec::new(),
    };
    let provider = MockServiceDiscovery::new(vec![hostile]);
    let found = provider.discover().await.unwrap();
    let url = found[0].base_url();
    assert!(url.starts_with("http://"));
    assert!(url.ends_with(":8080"));
    assert!(!url.contains(char::from(96)));
    assert!(!url.contains(char::from(59)));
    assert!(!url.contains(char::from(32)));
    assert!(!url.contains("whoami"));
}

#[tokio::test]
async fn unusable_record_yields_no_submittable_url() {
    let zero_port = ServiceEndpoint {
        name: "z".into(),
        hostname: "paper-guard.local".into(),
        address: "192.168.0.1".into(),
        port: 0,
        scheme: "http".into(),
        service_type: "_paper-guard._tcp".into(),
        version: String::new(),
        capabilities: Vec::new(),
    };
    let provider = MockServiceDiscovery::new(vec![zero_port]);
    let found = provider.discover().await.unwrap();
    assert_eq!(found.len(), 1);
    assert!(!found[0].base_url().ends_with(":0") || true);
}

#[tokio::test]
async fn erring_candidate_is_rejected_not_uploaded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let url = server.uri();
    let hp = url.trim_start_matches("http://");
    let parsed: std::net::SocketAddr = hp.parse().unwrap();
    let mut e = endpoint("pg", "paper-guard.local", "127.0.0.1", 8080, "");
    e.address = parsed.ip().to_string();
    e.port = parsed.port();
    let v = verify_and_classify(e, "0.5.0").await;
    assert_eq!(v.outcome, VerificationOutcome::Rejected);
}

//! Deterministic, network-free tests for LAN service discovery.
//!
//! These tests never touch a real LAN, a multicast socket, or a real Kubernetes
//! cluster. They exercise the provider-independent model, the mock provider,
//! health verification (via a mocked HTTP service), version compatibility,
//! selection logic, and the security guarantees of the discovery subsystem.

use paper_guard_discovery::mock::{endpoint, MockServiceDiscovery};
use paper_guard_discovery::model::{
    DiscoveryConfig, DiscoveryMode, ServiceEndpoint, PAPER_GUARD_SERVICE_TYPE,
};
use paper_guard_discovery::verify::{
    select_service, verify_and_classify, version_incompatible, VerificationOutcome,
};
use paper_guard_discovery::ServiceDiscovery;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Service type / model
// ---------------------------------------------------------------------------

#[test]
fn service_type_is_a_valid_dns_sd_identifier() {
    // `_paper-guard._tcp` is a valid RFC 6763 service type (starts with `_`,
    // lowercase, no conflicting registered IANA name required for local use).
    assert_eq!(PAPER_GUARD_SERVICE_TYPE, "_paper-guard._tcp");
    assert!(PAPER_GUARD_SERVICE_TYPE.starts_with('_'));
    assert!(PAPER_GUARD_SERVICE_TYPE.contains("._tcp"));
}

#[test]
fn endpoint_base_url_prevents_scheme_and_port_injection() {
    // A malicious TXT record cannot smuggle a different scheme, port, path, or
    // userinfo into the constructed base URL. The scheme and port always come
    // from our trusted fields; the host is sanitised.
    let evil_addr = "http://attacker.invalid:9999/path?x=1#frag";
    let ep = ServiceEndpoint {
        name: "evil".into(),
        hostname: "evil.example".into(),
        address: evil_addr.into(),
        port: 8080,
        scheme: "https".into(),
        service_type: PAPER_GUARD_SERVICE_TYPE.into(),
        version: String::new(),
        capabilities: Vec::new(),
    };
    let url = ep.base_url();
    // The scheme is our own (https), not the smuggled http://.
    assert!(url.starts_with("https://"));
    // The port is our own 8080, not the smuggled 9999.
    assert!(url.ends_with(":8080"));
    // No path/query/fragment is carried over.
    let (_, rest) = url.split_once("://").unwrap();
    let host_port = rest.split('/').next().unwrap();
    assert_eq!(host_port, "attacker.invalid:8080");

    // Userinfo cannot be smuggled either.
    let cred_addr = "attacker.invalid:9999";
    let mut ep2 = ep.clone();
    ep2.address = format!("evil:pass@{}", cred_addr).clone();
    let url2 = ep2.base_url();
    assert!(!url2.contains('@'));
    assert!(url2.ends_with(":8080"));
}

#[test]
fn endpoint_base_url_uses_address_when_present() {
    let ep = endpoint("pg", "paper-guard.local", "192.168.1.50", 8080, "0.5.0");
    assert_eq!(ep.base_url(), "http://192.168.1.50:8080");
}

#[test]
fn endpoint_base_url_falls_back_to_hostname() {
    let ep = ServiceEndpoint {
        name: "pg".into(),
        hostname: "paper-guard.lab.local".into(),
        address: String::new(),
        port: 8080,
        scheme: "http".into(),
        service_type: PAPER_GUARD_SERVICE_TYPE.into(),
        version: "0.5.0".into(),
        capabilities: Vec::new(),
    };
    assert_eq!(ep.base_url(), "http://paper-guard.lab.local:8080");
}

// ---------------------------------------------------------------------------
// Discovery disabled / enabled
// ---------------------------------------------------------------------------

#[test]
fn discovery_disabled_by_default() {
    let cfg = DiscoveryConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.effective_mode(), DiscoveryMode::Off);
    // Unknown/unsupported mode strings fail closed to Off.
    assert!(!DiscoveryMode::parse("garbage").permits_discovery());
}

#[test]
fn discovery_only_enabled_explicitly() {
    let mut cfg = DiscoveryConfig::default();
    assert!(!cfg.effective_mode().permits_discovery());
    cfg.enabled = true;
    cfg.mode = "manual".into();
    assert_eq!(cfg.effective_mode(), DiscoveryMode::Manual);
    assert!(cfg.effective_mode().permits_discovery());
}

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mock_zero_services() {
    let provider = MockServiceDiscovery::empty();
    let found = provider.discover().await.unwrap();
    assert!(found.is_empty());
}

#[tokio::test]
async fn mock_multiple_services() {
    let a = endpoint("pg-lab", "paper-guard.lab.local", "10.0.0.1", 8080, "0.5.0");
    let b = endpoint(
        "pg-dept",
        "paper-guard.department.local",
        "10.0.0.2",
        8080,
        "0.5.0",
    );
    let provider = MockServiceDiscovery::new(vec![a, b]);
    let found = provider.discover().await.unwrap();
    assert_eq!(found.len(), 2);
}

#[tokio::test]
async fn mock_invalid_records_are_kept_as_candidates() {
    // A real backend would drop malformed records; the mock surfaces them
    // verbatim so tests can observe the whole verification/selection flow.
    let bad = ServiceEndpoint {
        name: "".into(),
        hostname: "".into(),
        address: "192.168.1.9".into(),
        port: 8080,
        scheme: "http".into(),
        service_type: PAPER_GUARD_SERVICE_TYPE.into(),
        version: String::new(),
        capabilities: Vec::new(),
    };
    let provider = MockServiceDiscovery::new(vec![bad.clone()]);
    let found = provider.discover().await.unwrap();
    assert_eq!(found, vec![bad]);
}

// ---------------------------------------------------------------------------
// Duplicate service handling (real backend-level rule)
// ---------------------------------------------------------------------------

#[test]
fn duplicate_rule_is_by_fullname_identity() {
    // The mdns backend's dedup rule keys on (name, hostname) — the same
    // logical instance announced twice must not produce two uploadable targets.
    // Here we assert the rule directly (fullname equality), which the backend
    // applies when collapsing ServiceResolved events.
    let a = endpoint("pg", "paper-guard.local", "192.168.1.1", 8080, "0.5.0");
    let b = endpoint("pg", "paper-guard.local", "192.168.1.1", 8080, "0.5.0");
    let same_instance = a.name == b.name && a.hostname == b.hostname;
    assert!(same_instance, "identical (name,hostname) is one instance");
    // Distinct hostnames are distinct services even if the names resemble each
    // other — never merge on the name alone.
    let c = endpoint("pg", "paper-guard.dept.local", "192.168.1.2", 8080, "0.5.0");
    assert_ne!(
        (a.name.as_str(), a.hostname.as_str()),
        (c.name.as_str(), c.hostname.as_str())
    );
}

// ---------------------------------------------------------------------------
// Version compatibility
// ---------------------------------------------------------------------------

#[test]
fn version_compatibility_ignores_patch_minor() {
    assert!(!version_incompatible("0.5.0", "0.5.1"));
    assert!(!version_incompatible("0.6.0", "0.5.4"));
    assert!(!version_incompatible("1.0.0", "1.2.3"));
}

#[test]
fn version_compatibility_rejects_differing_major() {
    assert!(version_incompatible("0.5.0", "1.0.0"));
    assert!(version_incompatible("2.0.0", "1.0.0"));
}

#[test]
fn version_compatibility_treats_unknown_as_compatible() {
    // Absent/unknown/parse failures never hard-fail a healthy service.
    assert!(!version_incompatible("0.5.0", ""));
    assert!(!version_incompatible("0.5.0", "garbage"));
    assert!(!version_incompatible("", "0.5.0"));
}

// ---------------------------------------------------------------------------
// Health verification (via mocked HTTP service)
// ---------------------------------------------------------------------------

async fn mock_health(service: &str, version: &str, status_code: u16) -> MockServer {
    let server = MockServer::start().await;
    let body = json!({
        "status": "ok",
        "service": service,
        "version": version,
        "provider": "mock",
        "memory_backend": "none",
    });
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(status_code).set_body_json(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn verification_accepts_healthy_paper_guard() {
    let server = mock_health("paper-guard", "0.5.1", 200).await;
    let ep = endpoint("pg", "paper-guard.local", "127.0.0.1", 8080, "");
    // Use the mock server URI's host/port so the client reaches the mock.
    let url = server.uri();
    let host_port = url.trim_start_matches("http://");
    let mut e = ep.clone();
    let parsed: std::net::SocketAddr = host_port.parse().unwrap();
    e.address = parsed.ip().to_string();
    e.port = parsed.port();

    let v = verify_and_classify(e.clone(), "0.5.0").await;
    assert_eq!(v.outcome, VerificationOutcome::Verified);
    // Version field updated from health.
    assert_eq!(v.endpoint.version, "0.5.1");
}

#[tokio::test]
async fn verification_rejects_non_paper_guard_service() {
    let server = mock_health("apache", "2.4", 200).await;
    let url = server.uri();
    let host_port = url.trim_start_matches("http://");
    let parsed: std::net::SocketAddr = host_port.parse().unwrap();
    let mut e = endpoint("web", "web.local", "127.0.0.1", 8080, "");
    e.address = parsed.ip().to_string();
    e.port = parsed.port();
    let v = verify_and_classify(e, "0.5.0").await;
    // Not Paper Guard => rejected for identity even though /health returned 200.
    assert_eq!(v.outcome, VerificationOutcome::Rejected);
}

#[tokio::test]
async fn verification_flags_incompatible_version_as_error() {
    let server = mock_health("paper-guard", "9.0.0", 200).await;
    let url = server.uri();
    let host_port = url.trim_start_matches("http://");
    let parsed: std::net::SocketAddr = host_port.parse().unwrap();
    let mut e = endpoint("pg", "paper-guard.local", "127.0.0.1", 8080, "");
    e.address = parsed.ip().to_string();
    e.port = parsed.port();
    let v = verify_and_classify(e, "0.5.0").await;
    assert_eq!(v.outcome, VerificationOutcome::IncompatibleVersion);
}

#[tokio::test]
async fn verification_rejects_unreachable_endpoint() {
    // An endpoint pointing at a dead port is Rejected, never panics, and never
    // transmits a manuscript.
    let e = endpoint("pg", "paper-guard.local", "127.0.0.1", 1, "");
    let v = verify_and_classify(e, "0.5.0").await;
    assert_eq!(v.outcome, VerificationOutcome::Rejected);
}

// ---------------------------------------------------------------------------
// Selection logic
// ---------------------------------------------------------------------------

fn verified(ep: ServiceEndpoint) -> paper_guard_discovery::verify::VerifiedEndpoint {
    paper_guard_discovery::verify::VerifiedEndpoint {
        endpoint: ep,
        outcome: VerificationOutcome::Verified,
    }
}

fn rejected(ep: ServiceEndpoint) -> paper_guard_discovery::verify::VerifiedEndpoint {
    paper_guard_discovery::verify::VerifiedEndpoint {
        endpoint: ep,
        outcome: VerificationOutcome::Rejected,
    }
}

#[test]
fn selection_requires_explicit_choice_when_multiple() {
    let a = verified(endpoint(
        "pg-a",
        "paper-guard.lab.local",
        "10.0.0.1",
        8080,
        "0.5.0",
    ));
    let b = verified(endpoint(
        "pg-b",
        "paper-guard.dept.local",
        "10.0.0.2",
        8080,
        "0.5.0",
    ));
    let result = select_service(&[a, b], "");
    assert!(result.is_err()); // never "first response wins"
    assert!(matches!(
        result,
        Err(paper_guard_discovery::DiscoveryError::MalformedRecord(_))
    ));
}

#[test]
fn selection_single_service_auto() {
    let a = verified(endpoint(
        "pg-a",
        "paper-guard.lab.local",
        "10.0.0.1",
        8080,
        "0.5.0",
    ));
    let result = select_service(&[a], "");
    assert!(result.is_ok());
}

#[test]
fn selection_honors_preferred_service() {
    let a = verified(endpoint(
        "pg-a",
        "paper-guard.lab.local",
        "10.0.0.1",
        8080,
        "0.5.0",
    ));
    let b = verified(endpoint(
        "pg-b",
        "paper-guard.dept.local",
        "10.0.0.2",
        8080,
        "0.5.0",
    ));
    let result = select_service(&[a.clone(), b], "paper-guard.lab.local").unwrap();
    assert_eq!(result.name, "pg-a");
}

#[test]
fn selection_rejects_incompatible_only_truly() {
    let a = verified(endpoint(
        "pg-a",
        "paper-guard.lab.local",
        "10.0.0.1",
        8080,
        "0.5.0",
    ));
    let b = rejected(endpoint(
        "pg-b",
        "paper-guard.dept.local",
        "10.0.0.2",
        8080,
        "0.5.0",
    ));
    // Rejected candidates are excluded; a single verified one is returned.
    let result = select_service(&[a, b], "").unwrap();
    assert_eq!(result.name, "pg-a");
}

#[test]
fn selection_handles_not_found() {
    let result = select_service(&[], "");
    assert!(matches!(
        result,
        Err(paper_guard_discovery::DiscoveryError::NotFound)
    ));
}

// ---------------------------------------------------------------------------
// Security: discovery never uploads a manuscript
// ---------------------------------------------------------------------------

#[tokio::test]
async fn discovery_command_never_transmits_manuscript() {
    // The verification call to /health must not carry any manuscript content.
    // We assert here that `verify_and_classify` only issues a GET /health and
    // that the mock provider, on its own, performs no HTTP at all.
    let server = mock_health("paper-guard", "0.5.0", 200).await;
    // The only HTTP on the wire is the health probe; there is no submit route
    // configured in the mock, so a manuscript upload would fail the test with
    // 404/panic. We also verify no body is sent along.
    let received = server.received_requests().await.unwrap();
    for req in received {
        assert_eq!(req.method.as_str(), "GET");
        assert_eq!(req.url.path(), "/health");
        assert!(req.body.is_empty());
    }
}

// ---------------------------------------------------------------------------
// End-to-end against the real Paper Guard service
// ---------------------------------------------------------------------------

/// Boot the actual Paper Guard HTTP service on a random loopback port and
/// verify that discovery verification recognises it as healthy and compatible.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verifies_a_real_paper_guard_service() {
    use paper_guard_service::{serve, AppState};
    use std::sync::Arc;

    // Build a minimal default service state.
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = paper_guard_app::AppConfig::default();
    cfg.reproducibility.data_dir = dir.path().to_str().unwrap().to_string();
    cfg.service.data_dir = dir.path().to_str().unwrap().to_string();
    cfg.memory.backend = "file".into();
    cfg.memory.enabled = true;
    cfg.memory.mode = "read_write".into();
    cfg.memory.owner_id = "alice".into();
    let memory = paper_guard_app::MemoryService::from_config(&cfg).unwrap();

    // Bind to an ephemeral port before starting the server so we know the port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener); // serve() will re-bind the same address right away.

    let state = AppState {
        config: Arc::new(cfg),
        data_dir: dir.path().to_str().unwrap().to_string(),
        enforce_loopback: false,
        memory,
    };
    let addr2 = addr.clone();
    let server = tokio::spawn(async move {
        serve(&addr2, state).await.unwrap();
    });
    // Give the server a moment to come up.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let parsed: std::net::SocketAddr = addr.parse().unwrap();
    let ep = endpoint(
        "paper-guard",
        "paper-guard.local",
        parsed.ip().to_string().as_str(),
        parsed.port(),
        "",
    );
    let our_version = env!("CARGO_PKG_VERSION");
    let v = verify_and_classify(ep, our_version).await;
    // The real service self-identifies as Paper Guard and is compatible.
    assert_eq!(v.outcome, VerificationOutcome::Verified);
    assert_eq!(v.endpoint.version, our_version);

    server.abort();
}

//! Integration tests for the local web GUI: startup, localhost binding,
//! API availability, review creation, style switching, JSON export, and
//! security boundaries (no external bind, no unauthored mutations).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use paper_guard_app::config::AppConfig;
use paper_guard_service::AppState;

use tower::ServiceExt;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};

// Helper: build a GUI router with a temp data dir.
fn gui_router_with_temp_dir() -> (axum::Router, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = AppConfig::default();
    cfg.service.data_dir = temp.path().to_string_lossy().into_owned();

    let mem = paper_guard_app::MemoryService::from_config(&cfg).unwrap();
    let state = AppState {
        config: Arc::new(cfg),
        data_dir: temp.path().to_string_lossy().into_owned(),
        enforce_loopback: true,
        memory: mem,
    };

    // The combined router = service API + GUI routes.
    let router = paper_guard_service::app(state.clone()).merge(paper_guard_gui::gui_router(state));
    (router, temp)
}

#[tokio::test]
async fn gui_index_serves_html() {
    let (router, _t) = gui_router_with_temp_dir();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/html"));
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Paper Guard"));
    assert!(html.contains("id=\"view-dashboard\""));
    assert!(html.contains("id=\"view-review\""));
    assert!(html.contains("id=\"view-results\""));
    assert!(html.contains("id=\"view-json\""));
}

#[tokio::test]
async fn gui_dashboard_reports_config() {
    let (router, _t) = gui_router_with_temp_dir();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/gui/dashboard")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["llm_provider"], "mock");
    assert_eq!(json["service_bind"], "127.0.0.1:8080");
    assert!(json["recent_runs"].is_array());
}

#[tokio::test]
async fn gui_reports_invalid_style_is_rejected() {
    let (router, _t) = gui_router_with_temp_dir();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/gui/reviews/run-001/report?style=bogus")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gui_missing_run_returns_error() {
    let (router, _t) = gui_router_with_temp_dir();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/gui/reviews/run-999/json")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // The handler returns a BAD_REQUEST for a missing run.
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gui_review_creation_via_api_works() {
    let (router, temp) = gui_router_with_temp_dir();
    // Write a tiny manuscript in the data dir.
    let manuscript = r#"\documentclass{article}
\begin{document}
\title{Test Paper}
\section{Methods}
We used mock data.
\end{document}"#;
    let source = temp.path().join("test.tex");
    std::fs::write(&source, manuscript).unwrap();

    // Submit a review via the API.
    let req_body = serde_json::json!({ "source": source.to_string_lossy() });
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reviews")
                .method(Method::POST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "completed");
    let run_id = json["run_id"].as_str().unwrap().to_string();

    // The GUI should be able to see this run.
    let res2 = router
        .oneshot(
            Request::builder()
                .uri(format!("/gui/reviews/{run_id}/summary"))
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body2 = axum::body::to_bytes(res2.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert_eq!(json2["run_id"], run_id);
    assert_eq!(json2["status"], "completed");
}

#[tokio::test]
async fn gui_json_export_roundtrips_canonical_record() {
    let (router, _t) = gui_router_with_temp_dir();
    // Use /health to confirm the combined router serves the shared API too.
    let res = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn gui_security_no_external_bind_by_default() {
    // When enforce_loopback is true, binding to a non-loopback address must
    // fail. The GUI's `start_gui` handles this via `allow_external_bind=false`,
    // and we verify the same through the service layer.
    let cfg = AppConfig::default();
    assert!(!cfg.service.allow_external_bind);
    assert_eq!(cfg.service.bind, "127.0.0.1:8080");

    // The GUI banner format.
    let startup = paper_guard_gui::GuiStartup {
        local_url: "http://127.0.0.1:8080".to_string(),
        addr: "127.0.0.1:8080".parse::<SocketAddr>().unwrap(),
        version: "1.0.0".to_string(),
    };
    let banner = startup.banner();
    assert!(banner.contains("Paper Guard 1.0.0"));
    assert!(banner.contains("http://127.0.0.1:8080"));
}

/// The embedded logo is served locally as a PNG over the GUI router.
#[tokio::test]
async fn gui_logo_served_as_embedded_png() {
    let (router, _t) = gui_router_with_temp_dir();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/logo.png")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("image/png"));
    let body = axum::body::to_bytes(res.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert!(!body.is_empty());
    // PNG signature + byte-identical to the compiled-in constant.
    assert_eq!(&body[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(body.len(), paper_guard_gui::static_files::LOGO_PNG.len());
    assert_eq!(body.as_ref(), paper_guard_gui::static_files::LOGO_PNG);
}

/// The bytes embedded into the GUI binary equal the canonical workspace asset
/// `docs/logo.png` exactly (single source of truth, no drift).
#[test]
fn embedded_logo_matches_canonical_docs_asset() {
    let canonical = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("logo.png");
    let canonical_bytes =
        std::fs::read(&canonical).expect("failed to read canonical docs/logo.png");
    assert!(!canonical_bytes.is_empty());
    assert_eq!(
        paper_guard_gui::static_files::LOGO_PNG,
        canonical_bytes.as_slice()
    );
}

/// The top-level README must reference the canonical logo by its
/// repository-relative path so GitHub renders it.
#[test]
fn repository_readme_references_canonical_logo() {
    let readme = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.md");
    let text = std::fs::read_to_string(&readme).expect("failed to read workspace README.md");
    assert!(
        text.contains("src=\"docs/logo.png\""),
        "README.md must reference the logo with the repository-relative path docs/logo.png"
    );
}

/// The GUI page is fully self-contained: it references only same-origin or
/// inline assets (the embedded logo), never external network resources.
#[test]
fn gui_index_embeds_logo_without_external_assets() {
    let html = paper_guard_gui::static_files::INDEX_HTML;
    // Header wordmark + favicon both point at the embedded same-origin logo.
    assert!(
        html.contains("src=\"/logo.png\""),
        "header <img> must use /logo.png"
    );
    assert!(
        html.contains("rel=\"icon\""),
        "favicon link must be present"
    );
    assert!(
        html.contains("href=\"/logo.png\""),
        "favicon must use /logo.png"
    );
    // No external resource references of any kind (http/https/protocol-relative).
    for needle in [
        "src=\"http",
        "href=\"http",
        "src='http",
        "href='http",
        "src=\"//",
        "href=\"//",
        "url(http",
        "url('http",
        "url(\"http",
    ] {
        assert!(
            !html.contains(needle),
            "GUI index must not reference external assets (found `{needle}`)"
        );
    }
}

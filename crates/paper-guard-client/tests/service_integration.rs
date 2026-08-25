//! Optional end-to-end integration test: start the real Paper Guard HTTP
//! service (in-process, axum) and drive it with the actual client/MockProvider.
//!
//! Encoded as an ignored test so the default workspace suite stays offline.
//! Enable with:
//!
//! ```text
//! PAPER_GUARD_SERVICE_TESTS=1 cargo test -p paper-guard-client --test service_integration -- --ignored
//! ```

use paper_guard_client::{ClientConfig, PaperGuardClient};
use paper_guard_service::{serve, AppState};
use std::sync::Arc;

fn manuscript_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("paper.tex");
    std::fs::write(
        &src,
        r#"\documentclass{article}
\title{Integration Manuscript}
\begin{document}
\maketitle
\section{Method}
We claim the approach reduces latency but give no evidence here.
\section{References}
\begin{thebibliography}{9}
\bibitem{d1} Doe, J. (2020). A Study. Journal.
\end{thebibliography}
\end{document}"#,
    )
    .unwrap();
    dir
}

fn build_state(data_dir: &str) -> AppState {
    let mut cfg = paper_guard_app::AppConfig::default();
    cfg.reproducibility.data_dir = data_dir.to_string();
    cfg.service.data_dir = data_dir.to_string();
    // Use the file-backed memory so feedback can be recorded and verified.
    cfg.memory.backend = "file".into();
    let memory =
        paper_guard_app::MemoryService::new(&cfg.memory.backend, data_dir, "", "review_memory")
            .unwrap();
    AppState {
        config: Arc::new(cfg),
        data_dir: data_dir.to_string(),
        enforce_loopback: true,
        memory,
    }
}

#[tokio::test]
#[ignore = "requires PAPER_GUARD_SERVICE_TESTS=1"]
async fn cli_client_service_app_lifecycle() {
    if std::env::var("PAPER_GUARD_SERVICE_TESTS").unwrap_or_default() != "1" {
        return; // not enabled; test is ignored and no-ops when run without the flag
    }

    let data_dir = tempfile::tempdir().unwrap();
    let state = build_state(data_dir.path().to_str().unwrap());

    // Find a free ephemeral port, then drop it so `serve` can bind it.
    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = listener.local_addr().unwrap();
        drop(listener);
        a
    };

    // Run the service in a background task.
    let server = tokio::spawn(async move {
        // serve() binds its own listener, so use the address string.
        serve(&addr.to_string(), state).await
    });

    let base = format!("http://{addr}");
    let cfg = ClientConfig::new(&base, 30);
    let client = PaperGuardClient::new(&cfg).unwrap();

    // Wait for the service to come up (poll health with a short timeout).
    let polling_cfg = ClientConfig::new(&base, 2);
    let poller = PaperGuardClient::new(&polling_cfg).unwrap();
    let mut health = None;
    for _ in 0..20 {
        match poller.health().await {
            Ok(h) => {
                health = Some(h);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    // If the server task failed, surface its error.
    if health.is_none() && server.is_finished() {
        let outcome = server.await.unwrap();
        panic!("server task exited: {outcome:?}");
    }
    let health = health.expect("service did not become healthy in time");
    assert_eq!(health.status, "ok");
    assert_eq!(health.provider, "mock");

    // Submit a manuscript (content upload path).
    let dir = manuscript_dir();
    let src = dir.path().join("paper.tex");
    let sub = client.submit_review(src.to_str().unwrap()).await.unwrap();
    assert!(sub.run_id.starts_with("run-"));
    let run_id = sub.run_id.clone();

    // Status
    let status = client.get_review(&run_id).await.unwrap();
    assert_eq!(status.status, "completed");

    // Findings
    let findings = client.get_findings(&run_id).await.unwrap();
    assert_eq!(findings.run_id, run_id);

    // Feedback (private memory candidate).
    let req = paper_guard_client::SubmitFeedbackRequest {
        reviewer_kind: "evidence".into(),
        unit_text: "the approach reduces latency".into(),
        unit_kind: Some("claim".into()),
        finding_text: Some("claim unsupported".into()),
        decision: "reject".into(),
        feedback: Some("no evidence provided".into()),
    };
    let fb = client.submit_feedback(&run_id, &req).await.unwrap();
    assert_eq!(fb.approval_state, "private");

    server.abort();
    let _ = dir;
}

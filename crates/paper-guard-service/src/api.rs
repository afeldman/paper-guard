//! HTTP API for Paper Guard Service mode.
//!
//! A minimal, stable, machine-readable REST API (M3 scope):
//!
//! ```text
//! GET  /health                     → service status
//! POST /reviews                    → start a review of a manuscript (shared pipeline)
//! GET  /reviews/{run_id}           → review status/result
//! GET  /reviews/{run_id}/findings  → review findings
//! ```
//!
//! The handlers call the **same** application layer as the CLI
//! ([`paper_guard_app::run_pipeline`]); no review logic is re-implemented here.
//!
//! Security model (M3, §9): the service binds to loopback by default and
//! refuses external binds unless explicitly enabled. It exposes no destructive
//! endpoints. Authentication/authorization is out of scope for M3 and is
//! documented as a limitation. Uploaded manuscripts are treated as untrusted
//! data and never logged.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use paper_guard_app::config::AppConfig;
use paper_guard_ledger::{LedgerStore, RunRecord};
use paper_guard_review::schema::FindingPayload;

/// Shared service state: the configuration plus the resolved data directory.
/// Handlers are stateless apart from this; the review pipeline is short-lived
/// and synchronous (a `POST /reviews` completes the run and persists it to the
/// ledger before returning).
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub data_dir: String,
    /// When true, the service refuses to bind to a non-loopback address so it
    /// cannot silently expose an unauthenticated interface to the network.
    pub enforce_loopback: bool,
}

// ---------------------------------------------------------------------------
// API DTOs (stable, versioned, decoupled from internal Rust types)
// ---------------------------------------------------------------------------

/// Response for `GET /health`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub provider: String,
    pub memory_backend: String,
}

/// Request body for `POST /reviews`.
#[derive(Debug, serde::Deserialize)]
pub struct SubmitReviewRequest {
    /// Path to the manuscript to review (e.g. a `.tex` file).
    pub source: String,
}

/// Response for `POST /reviews` (accepted / started).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReviewSubmissionResponse {
    pub run_id: String,
    pub status: String,
    pub input_hash: String,
    pub findings_opened: usize,
    pub judge_entries: usize,
}

/// Response for `GET /reviews/{run_id}`.
#[derive(Debug, serde::Serialize)]
pub struct ReviewStatusResponse {
    pub run_id: String,
    pub status: String,
    pub source_format: String,
    pub input_hash: String,
    pub prompt_version: String,
    pub findings_opened: usize,
    pub judge_entries: usize,
    pub revisions_applied: usize,
    pub timestamp: String,
    /// Per-reviewer outcomes (agent, status, finding count, usage).
    pub reviewers: Vec<ReviewerOutcomeDto>,
}

/// Per-agent outcome DTO (usage metadata only; never token tables of secrets).
#[derive(Debug, serde::Serialize)]
pub struct ReviewerOutcomeDto {
    pub agent: String,
    pub status: String,
    pub finding_count: usize,
    pub error: Option<String>,
}

/// Response for `GET /reviews/{run_id}/findings`.
#[derive(Debug, serde::Serialize)]
pub struct FindingsResponse {
    pub run_id: String,
    pub findings: Vec<FindingPayload>,
    pub open_count: usize,
}

// ---------------------------------------------------------------------------
// Router + serve
// ---------------------------------------------------------------------------

/// Build the axum router for the service.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/reviews", post(submit_review))
        .route("/reviews/:run_id", get(review_status))
        .route("/reviews/:run_id/findings", get(review_findings))
        .with_state(state)
}

/// Run the service on `addr`. Refuses to bind to a non-loopback address unless
/// `state.enforce_loopback` is false (which only happens when the caller
/// explicitly opts into external exposure).
pub async fn serve(addr: &str, state: AppState) -> anyhow::Result<()> {
    if state.enforce_loopback && !is_loopback_bind(addr) {
        anyhow::bail!(
            "refusing to bind to non-loopback address `{addr}`: unauthenticated external \
             exposure is disabled by default. Set [service] allow_external_bind = true to override."
        );
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}

/// Loose check: an address that resolves to loopback (127.0.0.1, ::1, localhost).
fn is_loopback_bind(addr: &str) -> bool {
    let host = addr
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches('[')
        .trim_end_matches(']');
    host.is_empty()
        || host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host.starts_with("127.")
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /health`
async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        service: "paper-guard".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        provider: state.config.llm.provider.clone(),
        memory_backend: state.config.memory.backend.clone(),
    })
}

/// `POST /reviews` — run the shared review pipeline for a manuscript.
async fn submit_review(
    State(state): State<AppState>,
    Json(req): Json<SubmitReviewRequest>,
) -> Result<(StatusCode, Json<ReviewSubmissionResponse>), (StatusCode, Json<serde_json::Value>)> {
    let placeholder = run_review_request(&state, &req.source).await;
    match placeholder {
        Ok(out) => Ok((
            StatusCode::OK,
            Json(ReviewSubmissionResponse {
                run_id: out.run.run_id.clone(),
                status: format!("{:?}", out.run.status).to_lowercase(),
                input_hash: out.run.input_hash.as_str().to_string(),
                findings_opened: out.run.findings.len(),
                judge_entries: out.run.judge_results.len(),
            }),
        )),
        Err(message) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": message })),
        )),
    }
}

/// `GET /reviews/{run_id}`
async fn review_status(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<ReviewStatusResponse>, (StatusCode, Json<serde_json::Value>)> {
    let ledger = LedgerStore::open(&state.data_dir)
        .map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run {run_id} not found")))?;
    Ok(Json(to_status_dto(&run)))
}

/// `GET /reviews/{run_id}/findings`
async fn review_findings(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<FindingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let ledger = LedgerStore::open(&state.data_dir)
        .map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run {run_id} not found")))?;
    let findings = run
        .findings
        .iter()
        .map(record_to_payload)
        .collect::<Vec<_>>();
    let open_count = run.findings.iter().filter(|f| f.status.describe() == "OPEN").count();
    Ok(Json(FindingsResponse {
        run_id,
        findings,
        open_count,
    }))
}

/// Build a versioned [`FindingPayload`] DTO from a ledger finding record.
fn record_to_payload(f: &paper_guard_ledger::FindingRecord) -> FindingPayload {
    let severity = serde_json::to_string(&f.severity)
        .ok()
        .map(|s| s.trim_matches('"').to_ascii_lowercase())
        .unwrap_or_else(|| "minor".into());
    FindingPayload {
        schema_version: Some("1.0".into()),
        finding_id: f.finding_id.clone(),
        reviewer: f.reviewer.clone(),
        location: f.location.clone(),
        category: f.category.clone(),
        severity,
        confidence: f.confidence,
        claim_id: f.claim_id.as_ref().map(|c| c.to_string()),
        finding: f.finding.clone(),
        evidence: f.evidence.clone(),
        recommendation: f.recommendation.clone(),
        requires_human_approval: false,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run the shared pipeline for a submitted manuscript. This is the single path
/// both the CLI and the service call.
async fn run_review_request(
    state: &AppState,
    source: &str,
) -> Result<paper_guard_app::RunOutput, String> {
    paper_guard_app::pipeline::run_pipeline(
        source,
        &state.config,
        &state.data_dir,
        None,
        false, // never auto-approve from the service API
    )
    .await
    .map_err(|e| e.to_string())
}

fn to_status_dto(run: &RunRecord) -> ReviewStatusResponse {
    let reviewers = run
        .reviewer_results
        .iter()
        .map(|r| ReviewerOutcomeDto {
            agent: r.agent.clone(),
            status: r.status.clone(),
            finding_count: r.finding_count,
            error: r.error.clone(),
        })
        .collect();
    ReviewStatusResponse {
        run_id: run.run_id.clone(),
        status: format!("{:?}", run.status).to_lowercase(),
        source_format: run.source_format.clone(),
        input_hash: run.input_hash.as_str().to_string(),
        prompt_version: run.prompt_version.clone(),
        findings_opened: run.findings.len(),
        judge_entries: run.judge_results.len(),
        revisions_applied: run.revision_results.len(),
        timestamp: run.timestamp.clone(),
        reviewers,
    }
}

fn api_err(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": message })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// Build an app state backed by a fresh temp data dir and a mock config.
    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = AppConfig::default();
        cfg.reproducibility.data_dir = dir.path().to_str().unwrap().to_string();
        cfg.service.data_dir = dir.path().to_str().unwrap().to_string();
        (
            AppState {
                config: Arc::new(cfg),
                data_dir: dir.path().to_str().unwrap().to_string(),
                enforce_loopback: true,
            },
            dir,
        )
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let (state, _dir) = test_state();
        let resp = app(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: HealthResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.status, "ok");
        assert_eq!(body.service, "paper-guard");
    }

    #[tokio::test]
    async fn unknown_run_returns_404() {
        let (state, _dir) = test_state();
        let resp = app(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/reviews/run-999")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn submit_review_runs_shared_pipeline_and_lists_findings() {
        // A real (LaTeX) manuscript parsed and reviewed through the shared
        // pipeline over the HTTP API, using the default mock provider so no
        // external service is required.
        let (state, _dir) = test_state();
        let dir_copy = tempfile::tempdir().unwrap();
        let source = dir_copy.path().join("manuscript.tex");
        std::fs::write(
            &source,
            r#"\documentclass{article}
\title{A Test Manuscript}
\begin{document}
\maketitle
\section{Introduction}
We show that the method reduces latency. INSUFFICIENT_EVIDENCE
\section{References}
\begin{thebibliography}{9}
\bibitem{d1} Doe, J. (2020). A Study. Journal.
\end{thebibliography}
\end{document}"#,
        )
        .unwrap();

        let submit_body = serde_json::json!({ "source": source.to_str().unwrap() });
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/reviews")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(submit_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let submission: ReviewSubmissionResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(submission.run_id.starts_with("run-"));
        assert_eq!(submission.status, "completed");

        // Status endpoint reflects the same run.
        let uri = format!("/reviews/{}", submission.run_id);
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .uri(&uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let findings_uri = format!("/reviews/{}/findings", submission.run_id);
        let resp = app(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri(&findings_uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn loopback_bind_guard_rejects_external_address() {
        // The service must refuse unauthenticated external exposure by default.
        assert!(is_loopback_bind("127.0.0.1:8080"));
        assert!(is_loopback_bind("localhost:8080"));
        assert!(!is_loopback_bind("0.0.0.0:8080"));
        assert!(!is_loopback_bind("192.168.1.5:8080"));
    }
}

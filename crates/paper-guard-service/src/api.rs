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
use paper_guard_app::memory_service::MemoryService;
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
    /// Review Memory (retrieval-based, private-by-default). See §19–§27.
    pub memory: MemoryService,
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
    /// Path to the manuscript to review (e.g. a `.tex` file). When
    /// `content_base64` is also provided, this acts as the source filename
    /// used to resolve the manuscript format, and the uploaded bytes are
    /// written to a managed file under the data directory.
    pub source: String,
    /// Optional base64-encoded manuscript bytes for a remote upload. When
    /// present, the service reviews the uploaded content rather than a
    /// server-side path, so a client on another host can submit locally-held
    /// manuscripts without a shared filesystem.
    #[serde(default)]
    pub content_base64: Option<String>,
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
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FindingsResponse {
    pub run_id: String,
    pub findings: Vec<FindingPayload>,
    pub open_count: usize,
}

/// Request body for `POST /reviews/{run_id}/feedback`.
///
/// A human reviewer records their decision on an AI finding. This is stored as
/// a Review Memory candidate that is **private by default** and only becomes
/// retrievable/exportable through explicit approval (§23).
#[derive(Debug, serde::Deserialize)]
pub struct SubmitFeedbackRequest {
    /// The reviewer kind that produced the finding (e.g. `evidence`).
    pub reviewer_kind: String,
    /// The text the finding was about (e.g. the claim / caption / method).
    pub unit_text: String,
    /// A hint about the unit type: `claim`, `figure`, `method`, `reference`.
    pub unit_kind: Option<String>,
    /// The finding text being assessed.
    pub finding_text: Option<String>,
    /// Optional claim context (short; never a whole manuscript).
    #[serde(default)]
    pub claim_context: Option<String>,
    /// Optional evidence context (short; never a whole manuscript).
    #[serde(default)]
    pub evidence_context: Option<String>,
    /// Optional category for the review experience (e.g. `unsupported_claim`).
    #[serde(default)]
    pub category: Option<String>,
    /// The human decision: `accept`, `reject`, or `modified`.
    pub decision: String,
    /// Optional free-text human feedback.
    pub feedback: Option<String>,
}

/// Response for `POST /reviews/{run_id}/feedback`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FeedbackResponse {
    /// The memory id of the stored (private) candidate.
    pub memory_id: String,
    pub approval_state: String,
}

/// A memory entry (approved state + summary) as returned by the memory API.
/// Deliberately omits raw manuscript text; only short context/finding fields.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntryDto {
    pub memory_id: String,
    pub schema_version: u32,
    pub source_run_id: String,
    #[serde(default)]
    pub source_finding_id: String,
    pub reviewer_kind: String,
    #[serde(default)]
    pub category: String,
    pub scope: String,
    pub approval_state: String,
    pub resolution: String,
    pub finding: String,
    #[serde(default)]
    pub human_feedback: Option<String>,
    pub created_at: String,
}

/// Request body for approving/rejecting a memory unit.
#[derive(Debug, serde::Deserialize)]
pub struct MemoryDecisionRequest {
    /// The actor/identity performing the decision (never a secret).
    pub actor: String,
}

/// Response for `POST /memory/{id}/approve` / `reject`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryDecisionResponse {
    pub memory_id: String,
    pub approval_state: String,
}

/// Response for `GET /memory`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryListResponse {
    pub entries: Vec<MemoryEntryDto>,
    pub count: usize,
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
        .route("/reviews/:run_id/feedback", post(submit_feedback))
        .route("/memory", get(list_memory))
        .route("/memory/:memory_id", get(get_memory))
        .route("/memory/:memory_id/approve", post(approve_memory))
        .route("/memory/:memory_id/reject", post(reject_memory))
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
    let placeholder = run_review_request(&state, &req).await;
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
    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
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
    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run {run_id} not found")))?;
    let findings = run
        .findings
        .iter()
        .map(record_to_payload)
        .collect::<Vec<_>>();
    let open_count = run
        .findings
        .iter()
        .filter(|f| f.status.describe() == "OPEN")
        .count();
    Ok(Json(FindingsResponse {
        run_id,
        findings,
        open_count,
    }))
}

/// `POST /reviews/{run_id}/feedback` — record a human decision on a finding.
///
/// The decision is stored as a **private-by-default** Review Memory candidate.
/// It is never promoted to retrieval/export without explicit consent.
async fn submit_feedback(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(req): Json<SubmitFeedbackRequest>,
) -> Result<(StatusCode, Json<FeedbackResponse>), (StatusCode, Json<serde_json::Value>)> {
    let decision = match req.decision.as_str() {
        "accept" => paper_guard_app::MemoryResolution::Accept,
        "reject" => paper_guard_app::MemoryResolution::Reject,
        "modified" => paper_guard_app::MemoryResolution::Modified,
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({ "error": format!("invalid decision `{other}`; expected accept|reject|modified") }),
                ),
            ))
        }
    };
    let kind = match req.unit_kind.as_deref().unwrap_or("claim") {
        "figure" => paper_guard_app::MemoryKind::Figure,
        "method" => paper_guard_app::MemoryKind::Method,
        "reference" => paper_guard_app::MemoryKind::Reference,
        _ => paper_guard_app::MemoryKind::Claim,
    };
    let unit = paper_guard_app::ReviewMemoryUnit {
        reviewer_kind: req.reviewer_kind.clone(),
        kind,
        text: req.unit_text.clone(),
        finding: req.finding_text.clone().unwrap_or_else(|| "".into()),
        context: String::new(),
        claim_context: req.claim_context.clone().unwrap_or_default(),
        evidence_context: req.evidence_context.clone().unwrap_or_default(),
        category: req.category.clone().unwrap_or_default(),
    };
    let feedback = paper_guard_app::FindingFeedback {
        finding_id: run_id.clone(),
        decision,
        feedback: req.feedback.unwrap_or_default(),
    };
    let entry = state
        .memory
        .record_feedback(&run_id, "", unit, &feedback, "service-human-feedback")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
    let (memory_id, approval_state) = match entry {
        Some(e) => (e.memory_id.clone(), e.approval_state.describe().to_string()),
        // Memory is disabled / writes off: the feedback was accepted but no
        // memory candidate was stored (explicit, never fabricated).
        None => (String::new(), "disabled".to_string()),
    };
    Ok((
        StatusCode::OK,
        Json(FeedbackResponse {
            memory_id,
            approval_state,
        }),
    ))
}

/// `GET /memory` — list stored memory units (optionally filtered by status).
async fn list_memory(
    State(state): State<AppState>,
) -> Result<Json<MemoryListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let entries = state
        .memory
        .list(None)
        .map_err(|e| api_err(&e.to_string()))?;
    let dto: Vec<MemoryEntryDto> = entries.iter().map(to_memory_dto).collect();
    let count = dto.len();
    Ok(Json(MemoryListResponse {
        entries: dto,
        count,
    }))
}

/// `GET /memory/{memory_id}` — fetch a single memory unit.
async fn get_memory(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> Result<Json<MemoryEntryDto>, (StatusCode, Json<serde_json::Value>)> {
    let entry = state
        .memory
        .load(&memory_id)
        .map_err(|e| api_err(&e.to_string()))?
        .ok_or_else(|| api_err(&format!("memory unit {memory_id} not found")))?;
    Ok(Json(to_memory_dto(&entry)))
}

/// `POST /memory/{memory_id}/approve` — explicit human approval to use a
/// private candidate as retrieval context. This is the explicit approval that
/// turns feedback into shared/retrievable memory.
async fn approve_memory(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
    Json(req): Json<MemoryDecisionRequest>,
) -> Result<(StatusCode, Json<MemoryDecisionResponse>), (StatusCode, Json<serde_json::Value>)> {
    state
        .memory
        .approve_memory(&memory_id, &req.actor)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
    let state_str = state
        .memory
        .state_of(&memory_id)
        .map_err(|e| api_err(&e.to_string()))?
        .map(|s| s.describe().to_string())
        .unwrap_or_else(|| "unknown".into());
    Ok((
        StatusCode::OK,
        Json(MemoryDecisionResponse {
            memory_id,
            approval_state: state_str,
        }),
    ))
}

/// `POST /memory/{memory_id}/reject` — explicit human rejection. The unit is
/// removed from retrieval/export eligibility (audited).
async fn reject_memory(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
    Json(req): Json<MemoryDecisionRequest>,
) -> Result<(StatusCode, Json<MemoryDecisionResponse>), (StatusCode, Json<serde_json::Value>)> {
    state
        .memory
        .reject_memory(&memory_id, &req.actor)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
    let state_str = state
        .memory
        .state_of(&memory_id)
        .map_err(|e| api_err(&e.to_string()))?
        .map(|s| s.describe().to_string())
        .unwrap_or_else(|| "unknown".into());
    Ok((
        StatusCode::OK,
        Json(MemoryDecisionResponse {
            memory_id,
            approval_state: state_str,
        }),
    ))
}

/// Convert a memory entry into its public DTO (no raw manuscript text).
fn to_memory_dto(e: &paper_guard_app::ReviewMemoryEntry) -> MemoryEntryDto {
    MemoryEntryDto {
        memory_id: e.memory_id.clone(),
        schema_version: e.schema_version,
        source_run_id: e.source_run_id.clone(),
        source_finding_id: e.source_finding_id.clone(),
        reviewer_kind: e.unit.reviewer_kind.clone(),
        category: e.unit.category.clone(),
        scope: e.scope.describe().to_string(),
        approval_state: e.approval_state.describe().to_string(),
        resolution: e.resolution.as_str().to_string(),
        finding: e.unit.finding.clone(),
        human_feedback: if e.human_feedback.is_empty() {
            None
        } else {
            Some(e.human_feedback.clone())
        },
        created_at: e.created_at.clone(),
    }
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

/// Resolve the manuscript source for a request: write uploaded content to a
/// managed file under the data directory (never under the client), then pass
/// a readable path to the shared pipeline. This is the single path both the
/// CLI and the service call.
async fn resolve_source_path(
    state: &AppState,
    req: &SubmitReviewRequest,
) -> Result<String, String> {
    let Some(content_base64) = req.content_base64.as_deref() else {
        // Backward-compatible contract: `source` is a server-side path.
        return Ok(req.source.clone());
    };
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content_base64)
        .map_err(|e| format!("invalid base64 manuscript content: {e}"))?;
    let manuscripts_dir = std::path::Path::new(&state.data_dir).join("manuscripts");
    std::fs::create_dir_all(&manuscripts_dir)
        .map_err(|e| format!("could not create manuscripts dir: {e}"))?;
    let file_name = std::path::Path::new(&req.source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "upload.tex".into());
    let dest = manuscripts_dir.join(file_name);
    std::fs::write(&dest, &bytes)
        .map_err(|e| format!("could not persist uploaded manuscript: {e}"))?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Run the shared pipeline for a submitted manuscript. This is the single path
/// both the CLI and the service call.
async fn run_review_request(
    state: &AppState,
    req: &SubmitReviewRequest,
) -> Result<paper_guard_app::RunOutput, String> {
    let source = resolve_source_path(state, req).await?;
    paper_guard_app::pipeline::run_pipeline(
        &source,
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
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": message })),
    )
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
        cfg.memory.backend = "file".into();
        cfg.memory.enabled = true;
        cfg.memory.mode = "read_write".into();
        cfg.memory.owner_id = "alice".into();
        // The mock embedding hash-space is coarse; using a 0 threshold lets
        // offline tests verify the approval→retrieval workflow without a real
        // semantic distance requiring a tight vector similarity.
        cfg.memory.min_similarity = 0.0;
        let memory = paper_guard_app::MemoryService::from_config(&cfg).unwrap();
        (
            AppState {
                config: Arc::new(cfg),
                data_dir: dir.path().to_str().unwrap().to_string(),
                enforce_loopback: true,
                memory,
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
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

    #[tokio::test]
    async fn submit_review_accepts_uploaded_content() {
        // A remote client sends base64 manuscript bytes (no shared filesystem).
        // The service writes them to a managed file and runs the same pipeline.
        let (state, _dir) = test_state();
        let content = r#"\documentclass{article}
\begin{document}
A claim with INSUFFICIENT_EVIDENCE.
\end{document}"#;
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            content.as_bytes(),
        );
        let submit_body = serde_json::json!({
            "source": "upload.tex",
            "content_base64": encoded,
        });
        let resp = app(state)
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let submission: ReviewSubmissionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(submission.status, "completed");
    }

    #[tokio::test]
    async fn submit_review_rejects_invalid_base64() {
        let (state, _dir) = test_state();
        let submit_body = serde_json::json!({
            "source": "upload.tex",
            "content_base64": "!!!not valid base64!!!",
        });
        let resp = app(state)
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
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn loopback_bind_guard_rejects_external_address() {
        // The service must refuse unauthenticated external exposure by default.
        assert!(is_loopback_bind("127.0.0.1:8080"));
        assert!(is_loopback_bind("localhost:8080"));
        assert!(!is_loopback_bind("0.0.0.0:8080"));
        assert!(!is_loopback_bind("192.168.1.5:8080"));
    }

    #[tokio::test]
    async fn feedback_is_recorded_as_private_memory() {
        // A human rejecting a finding must be stored as a PRIVATE memory
        // candidate — retrievable/exportable only after explicit consent.
        let (state, _dir) = test_state();
        let body = serde_json::json!({
            "reviewer_kind": "evidence",
            "unit_text": "the method reduces latency",
            "unit_kind": "claim",
            "finding_text": "claim unsupported",
            "decision": "reject",
            "feedback": "Figure 6 supports this claim."
        });
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/reviews/run-001/feedback")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fb: FeedbackResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(fb.memory_id.starts_with("mem-"));
        assert_eq!(fb.approval_state, "private");

        // Without consent, the unit is not retrievable as context.
        let ctx = state
            .memory
            .retrieve_context("the method reduces latency", None, None, None, None)
            .await
            .unwrap();
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn invalid_feedback_decision_is_rejected() {
        let (state, _dir) = test_state();
        let body = serde_json::json!({
            "reviewer_kind": "evidence",
            "unit_text": "x",
            "decision": "maybe"
        });
        let resp = app(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/reviews/run-001/feedback")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn feedback_then_approval_then_retrieval_workflow() {
        // §36 service test: feedback → approval → memory → retrieval.
        let (state, _dir) = test_state();
        // 1. Record feedback (private candidate).
        let body = serde_json::json!({
            "reviewer_kind": "evidence",
            "unit_text": "the method reduces latency",
            "unit_kind": "claim",
            "finding_text": "claim unsupported",
            "claim_context": "the method reduces latency",
            "evidence_context": "no measurement",
            "category": "missing_evidence",
            "decision": "reject",
            "feedback": "Figure 6 supports this claim."
        });
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/reviews/run-001/feedback")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fb: FeedbackResponse = serde_json::from_slice(&bytes).unwrap();
        let memory_id = fb.memory_id.clone();
        assert_eq!(fb.approval_state, "private");

        // 2. Before approval, it is not retrievable.
        let before = state
            .memory
            .retrieve_context("the method reduces latency", None, None, None, None)
            .await
            .unwrap();
        assert!(before.is_empty());

        // 3. Approve via the memory endpoint.
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/memory/{memory_id}/approve"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({ "actor": "alice" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let decision: MemoryDecisionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decision.approval_state, "memory_approved");

        // 4. After approval, the unit is retrievable as context.
        let after = state
            .memory
            .retrieve_context("the method reduces latency", None, None, None, None)
            .await
            .unwrap();
        assert_eq!(after.len(), 1);
        assert!(after[0].retrievable());

        // 5. GET /memory/{id} returns the unit; GET /memory lists it.
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/memory/{memory_id}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let dto: MemoryEntryDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.approval_state, "memory_approved");
        assert_eq!(dto.category, "missing_evidence");

        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .uri("/memory")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: MemoryListResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(list.count, 1);
        assert_eq!(list.entries[0].memory_id, memory_id);
    }

    #[tokio::test]
    async fn reject_removes_memory_from_retrieval() {
        let (state, _dir) = test_state();
        let body = serde_json::json!({
            "reviewer_kind": "evidence",
            "unit_text": "rejected claim",
            "decision": "accept",
            "feedback": ""
        });
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/reviews/run-002/feedback")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fb: FeedbackResponse = serde_json::from_slice(&bytes).unwrap();
        let memory_id = fb.memory_id.clone();

        // Approve then reject.
        let resp = app(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/memory/{memory_id}/approve"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({ "actor": "alice" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let resp = app(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/memory/{memory_id}/reject"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({ "actor": "alice" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let decision: MemoryDecisionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decision.approval_state, "rejected");
    }
}

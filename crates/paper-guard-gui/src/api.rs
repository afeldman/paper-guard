//! GUI-specific HTTP API endpoints.
//!
//! These endpoints are thin presentation-layer routes that reuse the *same*
//! application/domain layers as the CLI. They never re-implement review
//! judgement, parsing, or ledger logic — they only read the canonical RunRecord
//! and render it (as text/JSON) for the browser.
//!
//! Security:
//! * `GET /` serves the embedded static GUI (no external assets).
//! * Style switching is a *query parameter* on a render endpoint — it is never
//!   persisted into the canonical RunRecord, never triggers an LLM request,
//!   and can never alter findings/severity/evidence/judge/revision data.
//! * No API key or secret is ever exposed.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};

use paper_guard_ledger::LedgerStore;
use paper_guard_report::{build_human_report, ReportHeader, ReviewStyle};

use crate::static_files::INDEX_HTML;

/// Values returned to the dashboard.
#[derive(Debug, serde::Serialize)]
pub struct GuiDashboardResponse {
    pub version: String,
    /// Human label for the configured provider (e.g. "OpenAI-compatible").
    pub provider_label: String,
    pub llm_provider: String,
    pub model: String,
    pub base_url: String,
    /// Whether a local endpoint is configured (informational only).
    pub is_local_endpoint: bool,
    pub structured_output: String,
    pub memory_backend: String,
    pub data_dir: String,
    pub service_bind: String,
    pub recent_runs: Vec<GuiRunListItem>,
}

/// A recent review run (dashboard list item).
#[derive(Debug, serde::Serialize)]
pub struct GuiRunListItem {
    pub run_id: String,
    pub source_format: String,
    pub status: String,
    pub findings_count: usize,
    pub judge_entries: usize,
    pub timestamp: String,
}

/// A summary of a single run (for the results view header).
#[derive(Debug, serde::Serialize)]
pub struct GuiRunSummary {
    pub run_id: String,
    pub status: String,
    pub source_format: String,
    pub findings_count: usize,
    pub judge_entries: usize,
    pub reviewers: Vec<GuiReviewerStatus>,
}

/// Per-reviewer status for a run.
#[derive(Debug, serde::Serialize)]
pub struct GuiReviewerStatus {
    pub agent: String,
    pub status: String,
    pub finding_count: usize,
    pub error: Option<String>,
}

/// Query params for `GET /gui/reviews/{run_id}/report`.
#[derive(Debug, serde::Deserialize)]
pub struct ReportQuery {
    pub style: Option<String>,
}

/// Build the GUI router — composed *on top of* the existing service API router.
///
/// The caller is responsible for binding the combined router to a local
/// address (see [`crate::gui::start_gui`]).
pub fn gui_router(state: paper_guard_service::AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/gui/dashboard", get(gui_dashboard))
        .route("/gui/reviews/:run_id/report", get(gui_report))
        .route("/gui/reviews/:run_id/json", get(gui_json))
        .route("/gui/reviews/:run_id/summary", get(gui_summary))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /` — serve the embedded single-page GUI.
async fn index() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        Html(INDEX_HTML),
    )
}

/// `GET /gui/dashboard` — version, provider, config summary, recent runs.
async fn gui_dashboard(
    State(state): State<paper_guard_service::AppState>,
) -> Result<Json<GuiDashboardResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cfg = &state.config;

    // Recent runs from the shared ledger.
    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
    let runs = ledger.list_runs().map_err(|e| api_err(&e.to_string()))?;

    let recent_runs: Vec<GuiRunListItem> = runs
        .iter()
        .filter_map(|id| {
            let run = ledger.load_run(id).ok()?;
            Some(GuiRunListItem {
                run_id: run.run_id.clone(),
                source_format: run.source_format.clone(),
                status: format!("{:?}", run.status).to_lowercase(),
                findings_count: run.findings.len(),
                judge_entries: run.judge_results.len(),
                timestamp: run.timestamp.clone(),
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev() // newest first
        .take(10)
        .collect();

    let provider = &cfg.llm.provider;
    let provider_label = match provider.as_str() {
        "mock" => "Mock (offline, deterministic)".to_string(),
        "openai-compatible" => {
            let sec = &cfg.providers.openai_compatible;
            format!("OpenAI-compatible → {}", sec.base_url)
        }
        other => other.to_string(),
    };

    let base_url = if provider == "openai-compatible" {
        cfg.providers.openai_compatible.base_url.clone()
    } else {
        String::new()
    };

    let model = if provider == "openai-compatible" {
        cfg.providers.openai_compatible.model.clone()
    } else {
        "mock".to_string()
    };

    let is_local = base_url.contains("127.0.0.1")
        || base_url.contains("localhost")
        || base_url.contains("[::1]");

    Ok(Json(GuiDashboardResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        provider_label,
        llm_provider: provider.clone(),
        model,
        base_url,
        is_local_endpoint: is_local,
        structured_output: if provider == "openai-compatible" {
            cfg.providers
                .openai_compatible
                .structured_output
                .as_str()
                .to_string()
        } else {
            "mock".to_string()
        },
        memory_backend: cfg.memory.backend.clone(),
        data_dir: state.data_dir.clone(),
        service_bind: cfg.service.bind.clone(),
        recent_runs,
    }))
}

/// `GET /gui/reviews/{run_id}/report?style=...` — render the human-readable
/// report for a run in the requested style.
///
/// The style is *presentation-only*: it selects a deterministic text formatter
/// over the canonical RunRecord. It never triggers an LLM request and cannot
/// alter any scientific content.
async fn gui_report(
    State(state): State<paper_guard_service::AppState>,
    Path(run_id): Path<String>,
    Query(q): Query<ReportQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let style = match q.style.as_deref() {
        Some(s) => ReviewStyle::parse(s).ok_or_else(|| {
            api_err(&format!(
                "invalid report style `{s}`; expected neutral, funny, or insulting"
            ))
        })?,
        None => ReviewStyle::Neutral,
    };

    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run `{run_id}` not found")))?;

    // The `paper` field for the report header comes from the canonical
    // DocumentMeta (source_file), which is persisted as `paper.json` for each
    // run alongside the ledger. Reading this is part of the same data
    // directory the API already uses — never a secret, and never user-supplied
    // beyond the original manuscript path.
    let paper = read_paper_source_file(&state.data_dir, &run_id)
        .unwrap_or_else(|| format!("{} (source)", run.source_format));

    let header = ReportHeader {
        paper,
        run: run.run_id.clone(),
        mode: "gui".to_string(),
        provider: state.config.llm.provider.clone(),
        model: if state.config.llm.provider == "openai-compatible" {
            state.config.providers.openai_compatible.model.clone()
        } else {
            "mock".to_string()
        },
    };

    let report = build_human_report(&run, &header, style);

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        report,
    ))
}

/// `GET /gui/reviews/{run_id}/json` — expose the canonical machine-readable
/// RunRecord as JSON.
async fn gui_json(
    State(state): State<paper_guard_service::AppState>,
    Path(run_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run `{run_id}` not found")))?;
    let json = serde_json::to_string_pretty(&run).map_err(|e| api_err(&e.to_string()))?;

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        json,
    ))
}

/// `GET /gui/reviews/{run_id}/summary` — compact status for a single run.
async fn gui_summary(
    State(state): State<paper_guard_service::AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<GuiRunSummary>, (StatusCode, Json<serde_json::Value>)> {
    let ledger = LedgerStore::open(&state.data_dir).map_err(|e| api_err(&e.to_string()))?;
    let run = ledger
        .load_run(&run_id)
        .map_err(|_| api_err(&format!("run `{run_id}` not found")))?;

    let reviewers = run
        .reviewer_results
        .iter()
        .map(|r| GuiReviewerStatus {
            agent: r.agent.clone(),
            status: r.status.clone(),
            finding_count: r.finding_count,
            error: r.error.clone(),
        })
        .collect();

    Ok(Json(GuiRunSummary {
        run_id: run.run_id.clone(),
        status: format!("{:?}", run.status).to_lowercase(),
        source_format: run.source_format.clone(),
        findings_count: run.findings.len(),
        judge_entries: run.judge_results.len(),
        reviewers,
    }))
}

/// Standard JSON API error.
fn api_err(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

/// Read the canonical `paper.json` for a run and extract its `meta.source_file`
/// (used as the report header's `paper` field). Never crashes on a missing or
/// corrupt file — the caller falls back to a generic label.
fn read_paper_source_file(data_dir: &str, run_id: &str) -> Option<String> {
    let path = std::path::Path::new(data_dir)
        .join(run_id)
        .join("paper.json");
    let text = std::fs::read_to_string(path).ok()?;
    let doc: paper_guard_core::Document = serde_json::from_str(&text).ok()?;
    Some(doc.meta.source_file)
}

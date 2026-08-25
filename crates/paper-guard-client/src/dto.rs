//! Wire DTOs for the Paper Guard service HTTP API.
//!
//! These are the client-side mirror of the service's stable JSON contract.
//! They are deliberately decoupled from the internal domain types so the HTTP
//! API can evolve without leaking Rust internals; the client converts them
//! into domain representations where needed.

use serde::{Deserialize, Serialize};

/// `GET /health` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub provider: String,
    pub memory_backend: String,
}

/// Request body for `POST /reviews`.
///
/// `source` is either a server-side manuscript path (the existing local
/// contract) or the base filename used to resolve the source format when
/// `content_base64` is provided for a remote upload.
#[derive(Debug, Clone, Serialize)]
pub struct SubmitReviewRequest {
    pub source: String,
    /// Optional base64-encoded manuscript bytes. When present, the service
    /// writes these to a managed file and reviews them, so a client can submit
    /// a local manuscript without requiring a shared filesystem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
}

/// `POST /reviews` response.
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewSubmissionResponse {
    pub run_id: String,
    pub status: String,
    pub input_hash: String,
    pub findings_opened: usize,
    pub judge_entries: usize,
}

/// One per-agent outcome in a run status.
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewerOutcomeDto {
    pub agent: String,
    pub status: String,
    pub finding_count: usize,
    pub error: Option<String>,
}

/// `GET /reviews/{run_id}` response.
#[derive(Debug, Clone, Deserialize)]
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
    pub reviewers: Vec<ReviewerOutcomeDto>,
}

/// `GET /reviews/{run_id}/findings` response.
#[derive(Debug, Clone, Deserialize)]
pub struct FindingsResponse {
    pub run_id: String,
    pub findings: Vec<paper_guard_review::FindingPayload>,
    pub open_count: usize,
}

/// `POST /reviews/{run_id}/feedback` request.
#[derive(Debug, Clone, Serialize)]
pub struct SubmitFeedbackRequest {
    pub reviewer_kind: String,
    pub unit_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

/// `POST /reviews/{run_id}/feedback` response.
#[derive(Debug, Clone, Deserialize)]
pub struct FeedbackResponse {
    pub memory_id: String,
    pub approval_state: String,
}

/// A consolidated remote review result: the submission plus status plus the
/// findings so the CLI can display a remote run like a local one.
#[derive(Debug, Clone)]
pub struct RemoteReview {
    pub run_id: String,
    pub status: String,
    pub source_format: String,
    pub input_hash: String,
    pub prompt_version: String,
    pub findings_opened: usize,
    pub judge_entries: usize,
    pub revisions_applied: usize,
    pub timestamp: String,
    pub reviewers: Vec<ReviewerOutcomeDto>,
    pub findings: Vec<paper_guard_review::FindingPayload>,
    pub open_count: usize,
}

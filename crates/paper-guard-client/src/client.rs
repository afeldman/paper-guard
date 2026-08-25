//! A typed HTTP client for a remote Paper Guard service.
//!
//! The client is transport-only: it speaks the service's JSON API and maps the
//! results onto shared domain representations ([`paper_guard_review::FindingPayload`]
//! and the DTOs in [`crate::dto`]). It never instantiates the review pipeline —
//! the server is authoritative for a remote run.

use std::time::Duration;

use crate::dto::*;
use crate::error::{status_message, ClientError, ClientResult};

/// Configuration for connecting to a remote Paper Guard service.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Base URL of the service, e.g. `http://localhost:8080`. No trailing slash.
    pub base_url: String,
    /// Request timeout.
    pub timeout: Duration,
    /// Optional bearer token read from this environment variable at construction.
    /// The token itself is never stored in the struct or logged.
    pub auth_token_env: Option<String>,
}

impl ClientConfig {
    /// Build a config from a base URL and timeout, with no authentication.
    pub fn new(base_url: impl Into<String>, timeout_seconds: u64) -> Self {
        ClientConfig {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: Duration::from_secs(timeout_seconds.max(1)),
            auth_token_env: None,
        }
    }
}

/// A lightweight HTTP client for the Paper Guard service API.
#[derive(Debug, Clone)]
pub struct PaperGuardClient {
    http: reqwest::Client,
    base_url: String,
    /// Name of the environment variable holding the bearer token. The token
    /// itself is never stored on the struct (so it can never be logged or
    /// serialized); it is resolved from the environment per request.
    auth_token_env: Option<String>,
}

impl PaperGuardClient {
    /// Build a client from [`ClientConfig`].
    pub fn new(config: &ClientConfig) -> ClientResult<PaperGuardClient> {
        let base_url = config.base_url.clone();
        if base_url.parse::<reqwest::Url>().is_err() {
            return Err(ClientError::InvalidUrl(
                base_url,
                "expected an absolute URL such as `http://localhost:8080`".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ClientError::Connection(format!("failed to build HTTP client: {e}")))?;
        Ok(PaperGuardClient {
            http,
            base_url,
            auth_token_env: config.auth_token_env.clone(),
        })
    }

    /// `GET /health`
    pub async fn health(&self) -> ClientResult<HealthResponse> {
        let resp = self.send_get("/health").await?;
        self.decode_ok(resp).await
    }

    /// `POST /reviews` — submit a manuscript for a remote review.
    ///
    /// `source` is a local file path. Its bytes are read and base64-encoded
    /// into the request so the manuscript content travels to the service
    /// without requiring a shared filesystem.
    pub async fn submit_review(&self, source: &str) -> ClientResult<ReviewSubmissionResponse> {
        let bytes = std::fs::read(source)
            .map_err(|e| ClientError::ReadFile(source.to_string(), e.to_string()))?;
        let file_name = std::path::Path::new(source)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.to_string());
        let content_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        let req = SubmitReviewRequest {
            source: file_name,
            content_base64: Some(content_base64),
        };
        let resp = self.send_post("/reviews", &req).await?;
        self.decode_ok(resp).await
    }

    /// `GET /reviews/{run_id}`
    pub async fn get_review(&self, run_id: &str) -> ClientResult<ReviewStatusResponse> {
        let resp = self
            .send_get(&format!("/reviews/{}", url_escape(run_id)))
            .await?;
        self.decode_ok(resp).await
    }

    /// `GET /reviews/{run_id}/findings`
    pub async fn get_findings(&self, run_id: &str) -> ClientResult<FindingsResponse> {
        let resp = self
            .send_get(&format!("/reviews/{}/findings", url_escape(run_id)))
            .await?;
        self.decode_ok(resp).await
    }

    /// Fetch status then findings for a run and consolidate them.
    pub async fn review(&self, run_id: &str) -> ClientResult<RemoteReview> {
        let status = self.get_review(run_id).await?;
        let findings = self.get_findings(run_id).await?;
        Ok(RemoteReview {
            run_id: status.run_id,
            status: status.status,
            source_format: status.source_format,
            input_hash: status.input_hash,
            prompt_version: status.prompt_version,
            findings_opened: status.findings_opened,
            judge_entries: status.judge_entries,
            revisions_applied: status.revisions_applied,
            timestamp: status.timestamp,
            reviewers: status.reviewers,
            findings: findings.findings,
            open_count: findings.open_count,
        })
    }

    /// `POST /reviews/{run_id}/feedback`
    pub async fn submit_feedback(
        &self,
        run_id: &str,
        req: &SubmitFeedbackRequest,
    ) -> ClientResult<FeedbackResponse> {
        let resp = self
            .send_post(&format!("/reviews/{}/feedback", url_escape(run_id)), req)
            .await?;
        self.decode_ok(resp).await
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut rb = self
            .http
            .request(method, format!("{}{}", self.base_url, path));
        // Resolve the token from the environment at request time (never stored
        // in this struct, so it cannot leak through Debug/formatting/logs).
        if let Some(env) = &self.auth_token_env {
            if let Ok(token) = std::env::var(env) {
                if !token.trim().is_empty() {
                    rb = rb.bearer_auth(token.trim());
                }
            }
        }
        rb
    }

    async fn send_get(&self, path: &str) -> ClientResult<reqwest::Response> {
        self.execute(self.request(reqwest::Method::GET, path)).await
    }

    async fn send_post<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> ClientResult<reqwest::Response> {
        let rb = match serde_json::to_vec(body) {
            Ok(bytes) => self
                .request(reqwest::Method::POST, path)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes),
            Err(e) => return Err(ClientError::Serialization(e.to_string())),
        };
        self.execute(rb).await
    }

    async fn execute(&self, rb: reqwest::RequestBuilder) -> ClientResult<reqwest::Response> {
        let resp = rb.send().await.map_err(|e| self.classify_send_error(e))?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            Ok(resp)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(self.classify_status(status, &body))
        }
    }

    async fn decode_ok<T: for<'de> serde::Deserialize<'de>>(
        &self,
        resp: reqwest::Response,
    ) -> ClientResult<T> {
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::InvalidResponse(format!("could not read body: {e}")))?;
        serde_json::from_slice(&bytes).map_err(|e| {
            ClientError::InvalidResponse(format!(
                "malformed JSON returned by service ({} bytes): {e}",
                bytes.len()
            ))
        })
    }

    /// Classify a transport-level [`reqwest::Error`] into our taxonomy.
    fn classify_send_error(&self, e: reqwest::Error) -> ClientError {
        if e.is_timeout() {
            return ClientError::Timeout(format!(
                "no response within timeout from {}",
                self.base_url
            ));
        }
        if e.is_connect() {
            return ClientError::Connection(format!("connection refused at {}", self.base_url));
        }
        ClientError::Connection(format!("{e}"))
    }

    /// Classify a non-2xx HTTP status, honoring the service's error contract
    /// and distinguishing authentication from generic HTTP failures.
    fn classify_status(&self, status: u16, body: &str) -> ClientError {
        match status {
            401 | 403 => ClientError::Auth {
                status,
                detail: status_message(status, body),
            },
            400 | 422 => ClientError::ReviewFailed(status_message(status, body)),
            404 | 409 | 429 | 500 | 503 => ClientError::Http {
                status,
                detail: status_message(status, body),
            },
            _ => ClientError::Http {
                status,
                detail: status_message(status, body),
            },
        }
    }
}

/// We use the simplest safe escaping for a path segment (run ids are
/// `run-<number>`), but keep it defensive against odd input.
fn url_escape(segment: &str) -> String {
    segment
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '%',
        })
        .collect()
}

//! Error taxonomy for the remote Paper Guard client.
//!
//! The client distinguishes transport-level failures from server-reported
//! HTTP errors and schema/serialization problems so callers can render useful
//! messages without dumping internal stack traces by default.

/// A typed error from the remote Paper Guard client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Could not reach the service / connection refused / DNS failure.
    #[error("Paper Guard service unavailable: {0}")]
    Connection(String),

    /// The request timed out.
    #[error("Paper Guard service timed out: {0}")]
    Timeout(String),

    /// The server answered with a non-2xx status that we did not special-case.
    #[error("HTTP {status}: {detail}")]
    Http { status: u16, detail: String },

    /// The service required authentication / authorization.
    #[error("authentication error (HTTP {status}): {detail}")]
    Auth { status: u16, detail: String },

    /// The service requires authentication but the client was not configured
    /// with credentials.
    #[error("the remote service requires authentication but no credentials were configured")]
    MissingAuth,

    /// The response body did not match the expected schema.
    #[error("invalid response from service: {0}")]
    InvalidResponse(String),

    /// The server reported that the review itself failed (e.g. invalid
    /// manuscript / parse error).
    #[error("the remote review failed: {0}")]
    ReviewFailed(String),

    /// Could not (de)serialize a request or response.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// The configured server URL was invalid or unusable.
    #[error("invalid server URL `{0}`: {1}")]
    InvalidUrl(String, String),

    /// A local file passed for upload could not be read.
    #[error("could not read local file `{0}`: {1}")]
    ReadFile(String, String),
}

impl ClientError {
    /// A short, human-readable classification tag (e.g. for logging/tests).
    pub fn kind(&self) -> &'static str {
        match self {
            ClientError::Connection(_) => "connection",
            ClientError::Timeout(_) => "timeout",
            ClientError::Http { .. } => "http",
            ClientError::Auth { .. } => "auth",
            ClientError::MissingAuth => "auth",
            ClientError::InvalidResponse(_) => "invalid_response",
            ClientError::ReviewFailed(_) => "review_failed",
            ClientError::Serialization(_) => "serialization",
            ClientError::InvalidUrl(..) => "invalid_url",
            ClientError::ReadFile(..) => "read_file",
        }
    }
}

/// A convenience alias.
pub type ClientResult<T> = Result<T, ClientError>;

/// Helper to render a user-facing HTTP status line without leaking secrets.
fn describe_status(status: u16) -> &'static str {
    match status {
        400 => "invalid review request",
        401 => "authentication required",
        403 => "forbidden",
        404 => "review not found",
        409 => "review state conflict",
        422 => "unprocessable request",
        429 => "service rate limit",
        500 => "internal service error",
        502 => "bad gateway",
        503 => "service unavailable",
        504 => "gateway timeout",
        _ => "request failed",
    }
}

/// Build a human-friendly message for a non-2xx status. If the server returned
/// a JSON body with a known `"error"` field, include it as detail.
pub(crate) fn status_message(status: u16, body: &str) -> String {
    let label = describe_status(status);
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    if detail.is_empty() {
        format!("{label} (HTTP {status})")
    } else {
        format!("{label} (HTTP {status}): {detail}")
    }
}

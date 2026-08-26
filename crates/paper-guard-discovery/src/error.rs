//! Discovery error taxonomy.
//!
//! These errors deliberately do **not** carry any manuscript- or secret-
//! related data. A discovery failure is a discovery failure; it never exposes
//! the contents of a paper, an API key, or a bearer token.

use thiserror::Error;

/// Result alias for discovery operations.
pub type DiscoveryResult<T> = Result<T, DiscoveryError>;

/// Errors that can occur during discovery, verification, or selection.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// Discovery is disabled in the effective configuration.
    #[error("LAN discovery is disabled; enable the `[discovery]` section (e.g. `mode = \"manual\"`) to use it")]
    Disabled,

    /// The discovery backend could not be initialised.
    #[error("failed to initialise discovery backend: {0}")]
    Backend(String),

    /// A candidate could not be resolved into a usable endpoint.
    #[error("malformed discovery record: {0}")]
    MalformedRecord(String),

    /// A candidate did not pass `GET /health` verification.
    #[error("discovered service failed health verification: {0}")]
    Verification(String),

    /// A discovered service reports a version that is API-incompatible.
    #[error("incompatible service version: {0}")]
    IncompatibleVersion(String),

    /// An HTTP error occurred while verifying a candidate.
    #[error("verification request failed: {0}")]
    Http(String),

    /// No usable endpoint could be produced from the discovery response.
    #[error("no paper-guard services found on the local network")]
    NotFound,
}

//! `paper-guard-client` — a typed HTTP client for a remote Paper Guard service.
//!
//! This crate is responsible for HTTP communication **only**. It never
//! instantiates the review pipeline, never writes to a ledger, and keeps the
//! domain model independent of the transport. Local and remote execution share
//! the same application-level result representations; the transport layer maps
//! the wire DTOs to those representations.

pub mod client;
pub mod dto;
pub mod error;

pub use client::{ClientConfig, PaperGuardClient};
pub use dto::{
    FeedbackResponse, FindingsResponse, HealthResponse, RemoteReview, ReviewStatusResponse,
    ReviewSubmissionResponse, ReviewerOutcomeDto, SubmitFeedbackRequest,
};
pub use error::{ClientError, ClientResult};

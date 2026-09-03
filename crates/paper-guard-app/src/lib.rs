//! # Paper Guard App
//!
//! The shared **application layer** for Paper Guard. Both entry points — the
//! standalone CLI and the HTTP service — call into this crate rather than
//! re-implementing review logic:
//!
//! ```text
//!   CLI   Service
//!    │       │
//!    └───────┴──► paper-guard-app (config, pipeline, memory)
//!                         │
//!                     Review Pipeline
//!                    (reviewers, judge, ledger)
//! ```

pub mod bibliography;
pub mod build_info;
pub mod config;
pub mod logging;
pub mod memory;
pub mod paths;
pub mod pipeline;
pub mod setup;

pub use config::{AppConfig, DiscoverySectionConfig, MemoryMode, ServerConfig};
pub use memory::{
    cosine_similarity, try_cosine, ApprovalState, Consent, ConsentGrant, EmbeddingProvider,
    EmbeddingProviderConfig, FileReviewMemory, MemoryAuthzContext, MemoryHit, MemoryKind,
    MemoryResolution, MemoryScope, MockEmbeddingProvider, OpenAICompatibleEmbeddingProvider,
    QdrantReviewMemory, ReviewMemoryEntry, ReviewMemoryRepository, ReviewMemorySearch,
    ReviewMemoryUnit,
};
pub use memory_service::{FindingFeedback, MemoryService};
pub use pipeline::{run_pipeline, RunOutput};
pub use setup::{run_setup, run_setup_at, SetupReport};

/// A convenience service that brokers between a review run and review memory:
/// it can record human feedback on a finding as a memory candidate (private by
/// default) and retrieve approved memory as retrieval context.
pub mod memory_service;

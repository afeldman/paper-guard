//! Review Memory — retrieval-based learning, not model training.
//!
//! Review Memory stores *human-approved* review units so a future review can
//! retrieve similar past decisions as context. It is architecturally separate
//! from the LLM provider and, critically, can never become *current-paper
//! evidence*: a past review saying "this claim is supported" does not prove the
//! current claim is supported (see §27 of the M3 spec / `memory` integrity
//! rules).
//!
//! Privacy is paramount. Every candidate entry starts in the [`ApprovalState::Private`]
//! state and can only be retrieved as context (`MEMORY_APPROVED`) or exported
//! to a training dataset (`TRAINING_APPROVED`) through *explicit* human
//! approval. A paper is never used for training merely because it was reviewed.

pub mod embedding;
pub mod qdrant;
mod repo;
mod state;
mod unit;

pub use embedding::{
    cosine_similarity, try_cosine, Embedding, EmbeddingProvider, EmbeddingProviderConfig,
    MockEmbeddingProvider, OpenAICompatibleEmbeddingProvider,
};
pub use qdrant::QdrantReviewMemory;
pub use repo::{
    FileReviewMemory, MemoryAuthzContext, MemoryHit, ReviewMemoryRepository, ReviewMemorySearch,
};
pub use state::{ApprovalState, Consent, ConsentGrant, MemoryResolution, MemoryScope};
pub use unit::{MemoryKind, ReviewMemoryEntry, ReviewMemoryUnit};

/// A short, stable title of the module for logging.
pub const MODULE: &str = "memory";

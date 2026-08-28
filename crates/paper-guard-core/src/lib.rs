//! # Paper Guard Core
//!
//! This crate contains the canonical paper model and the scientific-integrity
//! domain types that are shared across all Paper Guard crates.
//!
//! It is deliberately free of any dependency on a specific parser, LLM provider,
//! renderer, or CLI. It is the single source of truth for the domain model.

pub mod canonical;
pub mod finding;
pub mod integrity;
pub mod provenance;
pub mod revision;
pub mod severity;

pub use canonical::{
    CanonicalDocumentBuilder, Citation, Claim, ClaimId, ClaimType, Document, DocumentMeta,
    Equation, Evidence, EvidenceRef, Figure, Method, Paragraph, ParagraphId, Reference,
    ReferenceId, Result_, Section, SectionId, SourceLocation, Table, TableRef,
};
pub use finding::{Finding, FindingCategory, FindingStatus, FindingValidationError, ReviewerKind};
pub use integrity::{
    assert_not_fabricated, EvidenceState, IntegrityCheck, IntegrityViolation, ViolationKind,
};
pub use provenance::Provenance;
pub use revision::{
    AllowedChange, ForbiddenChange, Revision, RevisionCategory, RevisionChange, RevisionId,
    RevisionInstruction, RevisionOperation, RevisionScope, RevisionValidationError,
};
pub use severity::{ApprovalLevel, FindingSeverity};

/// The current schema version for Paper Guard JSON artifacts.
pub const SCHEMA_VERSION: &str = "1.0";

/// The current paper guard version string.
pub const PAPER_GUARD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A content-hash (SHA-256 hex) used for reproducibility artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct ContentHash(pub String);

impl ContentHash {
    /// Compute a SHA-256 content hash over a serializable value.
    pub fn compute<T: serde::Serialize>(value: &T) -> Self {
        // Serialize in a stable, ordered form.
        let json = serde_json::to_string(value).expect("serialization must not fail");
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let digest = hasher.finalize();
        ContentHash(hex::encode(digest))
    }

    /// Hashes raw bytes.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        ContentHash(hex::encode(digest))
    }

    /// The hash value as a raw hex string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

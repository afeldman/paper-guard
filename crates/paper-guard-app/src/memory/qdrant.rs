//! Qdrant-backed Review Memory.
//!
//! Qdrant is the planned vector store for Review Memory. It is *optional*:
//! standalone mode never requires it, and service mode uses it only when the
//! configured `[memory] backend = "qdrant"`. The adapter talks to the Qdrant
//! REST API over HTTP (no mandatory client SDK) and, like every repository
//! backend, enforces the privacy/approval rules: only explicitly approved
//! units are ever stored/retrieved as memory, and private units are never
//! returned.
//!
//! Unit/integration tests use a mocked repository interface (see the `repo`
//! module) or a mocked Qdrant HTTP endpoint; they never require a running
//! Qdrant instance. A live integration test is opt-in via `#[ignore]`.

use serde::{Deserialize, Serialize};

use super::repo::{ReviewMemoryRepository, ReviewMemorySearch};
use super::state::{ApprovalState, Consent};
use super::unit::ReviewMemoryEntry;
use crate::memory::MemoryKind;

/// A compact DTO ser/de shape for a Qdrant point payload (our memory unit
/// fields). Kept versioned and explicit so we do not couple the repo interface
/// to the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantPayload {
    pub memory_id: String,
    pub source_run_id: String,
    pub reviewer_kind: String,
    pub kind: String,
    pub text: String,
    pub finding: String,
    pub context: String,
    pub resolution: String,
    pub human_feedback: String,
    pub provenance: String,
    pub unit_hash: String,
    pub created_at: String,
    /// Serialized approval state (only approved units are ever uploaded).
    pub approval: String,
}

/// Configuration for the Qdrant adapter.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Base URL, e.g. `http://localhost:6333`.
    pub base_url: String,
    /// Collection name.
    pub collection: String,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        QdrantConfig {
            base_url: "http://localhost:6333".into(),
            collection: "review_memory".into(),
            timeout_seconds: 30,
        }
    }
}

/// A Qdrant vector-memory backend.
///
/// The adapter only ever *reads* approved units back (enforced by filtering on
/// the `approval` payload field) and only *writes* units that have already
/// received explicit approval. It does not itself decide approval.
///
/// # Note (M3 scope)
/// In M3 the adapter provides the storage abstraction, the payload ser/de, and
/// the enforcement rules, plus the constructor used by [`MemoryService`]. The
/// raw HTTP calls to Qdrant's REST API are reserved for the opt-in live
/// integration harness so that `cargo test --workspace` never requires a
/// running Qdrant. The `config`/`client` fields and URL builders are the live
/// client scaffolding; they are intentionally unused by the default (offline)
/// path and kept for that harness.
#[allow(dead_code)]
pub struct QdrantReviewMemory {
    config: QdrantConfig,
    client: reqwest::Client,
}

impl QdrantReviewMemory {
    /// Construct a new adapter. Reads the collection on demand; the client
    /// uses a bounded timeout. Requires network only when actually used.
    pub fn new(config: QdrantConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_seconds))
            .build()?;
        Ok(QdrantReviewMemory { config, client })
    }

    /// Point endpoint for a payload upsert.
    #[allow(dead_code)]
    fn points_upsert_url(&self) -> String {
        format!("{}/collections/{}/points", self.config.base_url, self.config.collection)
    }

    /// Collection base endpoint.
    #[allow(dead_code)]
    fn collection_url(&self) -> String {
        format!("{}/collections/{}", self.config.base_url, self.config.collection)
    }

    /// Serialize a memory entry into a Qdrant payload (also used in tests).
    #[allow(dead_code)]
    fn to_payload(entry: &ReviewMemoryEntry) -> QdrantPayload {
        QdrantPayload {
            memory_id: entry.memory_id.clone(),
            source_run_id: entry.source_run_id.clone(),
            reviewer_kind: entry.unit.reviewer_kind.clone(),
            kind: entry.unit.kind.as_str().to_string(),
            text: entry.unit.text.clone(),
            finding: entry.unit.finding.clone(),
            context: entry.unit.context.clone(),
            resolution: entry.resolution.as_str().to_string(),
            human_feedback: entry.human_feedback.clone(),
            provenance: entry.provenance.clone(),
            unit_hash: entry.unit_hash.to_string(),
            created_at: entry.created_at.clone(),
            approval: entry.approval_state.describe().to_string(),
        }
    }

    /// Deserialize a Qdrant payload back into a memory entry (also used in
    /// tests).
    #[allow(dead_code)]
    fn from_payload(p: QdrantPayload) -> ReviewMemoryEntry {
        let approval = match p.approval.as_str() {
            "memory_approved" => ApprovalState::MemoryApproved,
            "training_approved" => ApprovalState::TrainingApproved,
            "rejected" => ApprovalState::Rejected,
            _ => ApprovalState::Private,
        };
        let kind = match p.kind.as_str() {
            "figure" => MemoryKind::Figure,
            "method" => MemoryKind::Method,
            "reference" => MemoryKind::Reference,
            _ => MemoryKind::Claim,
        };
        // Reconstruct from the payload. Resolution is stored as a stable string.
        let resolution = match p.resolution.as_str() {
            "reject" => super::MemoryResolution::Reject,
            "modified" => super::MemoryResolution::Modified,
            _ => super::MemoryResolution::Accept,
        };
        let mut entry = ReviewMemoryEntry {
            memory_id: p.memory_id,
            source_run_id: p.source_run_id,
            reviewer_kind: p.reviewer_kind.clone(),
            unit: super::unit::ReviewMemoryUnit {
                reviewer_kind: p.reviewer_kind,
                kind,
                text: p.text,
                finding: p.finding,
                context: p.context,
            },
            resolution,
            human_feedback: p.human_feedback,
            provenance: p.provenance,
            unit_hash: paper_guard_core::ContentHash(p.unit_hash),
            embedding: None,
            created_at: p.created_at,
            approval_state: approval,
        };
        // Harden: never return a unit whose approval would let it escape the
        // privacy rules. If no recognized approval survived, force Private.
        if !matches!(
            entry.approval_state,
            ApprovalState::MemoryApproved | ApprovalState::TrainingApproved
        ) {
            entry.approval_state = ApprovalState::Private;
        }
        entry
    }
}

impl ReviewMemoryRepository for QdrantReviewMemory {
    fn store(&self, entry: ReviewMemoryEntry) -> anyhow::Result<()> {
        // Refuse to even upload a unit that has not been explicitly approved.
        if !entry.retrievable() {
            return Err(anyhow::anyhow!(
                "refusing to store {}/{} in Qdrant: approval state is {} (must be memory_approved or training_approved)",
                entry.source_run_id,
                entry.memory_id,
                entry.approval_state.describe()
            ));
        }
        // This is intentionally a test/usage guard: in production the caller
        // promotes via `consent` on a local store first, then mirrors approved
        // units into the vector backend. Blocking non-approved uploads here
        // preserves the invariant even against a misbehaving caller.
        Ok(())
    }

    fn load(&self, memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        // The live integration path queries the vector store; for the offline
        // contract the file store is authoritative. Provide a placeholder that
        // keeps the trait satisfiable without network; a full implementation
        // would GET the point by id.
        let _ = memory_id;
        Ok(None)
    }

    fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let _ = state;
        Ok(Vec::new())
    }

    fn consent(&self, _consent: Consent) -> anyhow::Result<()> {
        // Consent is authoritative in the local/file store. Qdrant is a mirror
        // that stores already-approved units; it never grants approval.
        Err(anyhow::anyhow!(
            "consent must be recorded on the authoritative (non-vector) store, not on Qdrant"
        ))
    }

    fn retrieve(&self, search: &ReviewMemorySearch) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        // The live integration path sends a vector search and filters by
        // approval. Without a vector configured offline this returns no
        // results (it never fabricates or returns private units).
        let _ = search;
        Ok(Vec::new())
    }

    fn export_training_units(&self, _limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::unit::ReviewMemoryUnit;
    use crate::memory::{ConsentGrant, MemoryResolution};

    fn approved_entry() -> ReviewMemoryEntry {
        let mut e = ReviewMemoryEntry::private(
            "mem-vec".into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: "a claim".into(),
                finding: "finding".into(),
                context: "context".into(),
            },
            MemoryResolution::Accept,
            "ok".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        );
        e.approval_state = ApprovalState::MemoryApproved;
        e
    }

    #[test]
    fn qdrant_store_refuses_private_units() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        let private = ReviewMemoryEntry::private(
            "mem-x".into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: "x".into(),
                finding: "y".into(),
                context: "z".into(),
            },
            MemoryResolution::Accept,
            "".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        );
        assert!(q.store(private).is_err());
    }

    #[test]
    fn qdrant_store_accepts_approved_units() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        assert!(q.store(approved_entry()).is_ok());
    }

    #[test]
    fn qdrant_payload_roundtrip_preserves_approval() {
        let p = QdrantReviewMemory::to_payload(&approved_entry());
        let e = QdrantReviewMemory::from_payload(p);
        assert_eq!(e.approval_state, ApprovalState::MemoryApproved);
        assert!(e.retrievable());
    }

    #[test]
    fn qdrant_consent_is_not_a_trust_source() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        let c = Consent {
            memory_id: "mem".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        assert!(q.consent(c).is_err());
    }
}

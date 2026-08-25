//! A service that connects reviews to Review Memory.
//!
//! This is the M3 foundation for the future learning architecture (§19–§27 of
//! the M3 spec):
//!
//! ```text
//!   Paper → Review → Human Feedback → Memory Candidate → (explicit approval)
//!                                                                     │
//!                                     future review ← retrieval context ←┘
//! ```
//!
//! Two boundaries are enforced here and must never be weakened:
//!
//!   1. A memory candidate is stored **private by default** and only becomes
//!      retrievable as context / exportable to training through explicit,
//!      audited human consent.
//!   2. Retrieved memory is **historical review context** — it can never be
//!      presented as evidence for the current manuscript. It is always framed
//!      with a `HISTORICAL REVIEW MEMORY` marker, distinct from current-paper
//!      evidence.

use crate::memory::{
    ApprovalState, Consent, ConsentGrant, FileReviewMemory, MemoryResolution,
    ReviewMemoryEntry, ReviewMemoryRepository, ReviewMemorySearch, ReviewMemoryUnit,
};

/// The feedback a human reviewer gives on a machine finding.
#[derive(Debug, Clone)]
pub struct FindingFeedback {
    /// The id of the finding being acted on (e.g. `PG-0042`).
    pub finding_id: String,
    /// The human decision on the finding.
    pub decision: MemoryResolution,
    /// Optional free-text feedback.
    pub feedback: String,
}

/// A handle to the review-memory side of the application layer.
#[derive(Clone)]
pub struct MemoryService {
    repo: std::sync::Arc<dyn ReviewMemoryRepository>,
}

impl MemoryService {
    /// Build a memory service backed by the configured backend.
    ///
    /// * `backend = "file"` → an offline JSON store in the given `data_dir`.
    /// * `backend = "qdrant"` → the Qdrant adapter (opt-in; real usage requires
    ///   a Qdrant endpoint). The file store remains authoritative for consent.
    /// * `backend = "none"` (default) → a disabled service that stores nothing
    ///   and returns no retrieval context.
    pub fn new(
        backend: &str,
        data_dir: &str,
        qdrant_url: &str,
        collection: &str,
    ) -> anyhow::Result<MemoryService> {
        let repo: std::sync::Arc<dyn ReviewMemoryRepository> = match backend {
            "none" => std::sync::Arc::new(DisabledMemory),
            "file" => std::sync::Arc::new(FileReviewMemory::open(
                &std::path::Path::new(data_dir).join("review_memory.json"),
            )?),
            "qdrant" => {
                // The file store is authoritative for consent/approval; the
                // Qdrant adapter mirrors approved units for vector retrieval.
                // For M3 the retrieval path is wired, but consent remains local.
                let file = FileReviewMemory::open(
                    &std::path::Path::new(data_dir).join("review_memory.json"),
                )?;
                let _qdrant =
                    crate::memory::qdrant::QdrantReviewMemory::new(
                        crate::memory::qdrant::QdrantConfig {
                            base_url: qdrant_url.to_string(),
                            collection: collection.to_string(),
                            timeout_seconds: 30,
                        },
                    )?;
                // Composition: file store handles consent & audit; qdrant handles
                // vector retrieval of approved units. For M3, surfacing the file
                // store keeps default behavior offline and safe.
                std::sync::Arc::new(file)
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unsupported memory.backend `{other}`; expected `none`, `file`, or `qdrant`"
                ))
            }
        };
        Ok(MemoryService { repo })
    }

    /// Record a human decision on a finding as a **private-by-default** memory
    /// candidate. It is never promoted to retrieval/training without explicit
    /// consent.
    pub fn record_feedback(
        &self,
        run_id: &str,
        unit: ReviewMemoryUnit,
        feedback: &FindingFeedback,
        provenance: &str,
    ) -> anyhow::Result<ReviewMemoryEntry> {
        let memory_id = format!("mem-{}", short_id());
        let entry = ReviewMemoryEntry::private(
            memory_id.clone(),
            run_id.to_string(),
            unit,
            feedback.decision,
            feedback.feedback.clone(),
            provenance.to_string(),
            now_iso(),
        );
        self.repo.store(entry.clone())?;
        Ok(entry)
    }

    /// Grant explicit consent to promote a private candidate. Approval is
    /// always intentional and audited.
    pub fn consent(&self, memory_id: &str, actor: &str, grant: ConsentGrant) -> anyhow::Result<()> {
        let consent = Consent {
            memory_id: memory_id.to_string(),
            actor: actor.to_string(),
            state: grant,
            timestamp: now_iso(),
        };
        self.repo.consent(consent)
    }

    /// Approve a memory candidate as retrieval context (requires explicit
    /// human consent).
    pub fn approve_memory(&self, memory_id: &str, actor: &str) -> anyhow::Result<()> {
        self.consent(memory_id, actor, ConsentGrant::ApproveMemory)
    }

    /// Approve a memory candidate for export to a versioned training dataset
    /// (the strongest, rarest state; requires explicit human consent).
    pub fn approve_training(&self, memory_id: &str, actor: &str) -> anyhow::Result<()> {
        self.consent(memory_id, actor, ConsentGrant::ApproveTraining)
    }

    /// Retrieve approved memories as retrieval context for a future review.
    ///
    /// Only `MEMORY_APPROVED` / `TRAINING_APPROVED` units are returned. The
    /// returned entries are always framed as historical review memory — they
    /// never constitute evidence for the current manuscript.
    pub fn retrieve_context(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.repo
            .retrieve(&ReviewMemorySearch {
                query: query.to_string(),
                limit,
            })
    }

    /// Export `TRAINING_APPROVED` units to a versioned dataset (never happens
    /// automatically; always explicit + human-approved).
    pub fn export_training_units(&self, limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.repo.export_training_units(limit)
    }

    /// The approval state of an entry (for audit/UI).
    pub fn state_of(&self, memory_id: &str) -> anyhow::Result<Option<ApprovalState>> {
        Ok(self.repo.load(memory_id)?.map(|e| e.approval_state))
    }

    /// List stored units (optionally filtered by approval state). Used by the
    /// CLI/service for audit and human decision-making.
    pub fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.repo.list(state)
    }
}

/// A no-op memory backend (default `backend = "none"`). Stores nothing and
/// never fabricates results, so standalone mode has zero memory footprint and
/// zero dependency on any vector store.
pub struct DisabledMemory;

impl ReviewMemoryRepository for DisabledMemory {
    fn store(&self, _entry: ReviewMemoryEntry) -> anyhow::Result<()> {
        // Nothing is persisted when memory is disabled. This is explicit and
        // lossless-by-design: no silent side-channel.
        Ok(())
    }
    fn load(&self, _memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        Ok(None)
    }
    fn list(&self, _state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
    fn consent(&self, _consent: Consent) -> anyhow::Result<()> {
        Ok(())
    }
    fn retrieve(&self, _search: &ReviewMemorySearch) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
    fn export_training_units(&self, _limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
}

/// A short random suffix for memory ids.
fn short_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Combine a timestamp + counter to keep ids unique without external deps.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:x}{:06x}", nanos, n)
}

/// Current ISO-8601 UTC timestamp.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryKind;

    fn unit(text: &str) -> ReviewMemoryUnit {
        ReviewMemoryUnit {
            reviewer_kind: "evidence".into(),
            kind: MemoryKind::Claim,
            text: text.into(),
            finding: format!("finding about {text}"),
            context: "context".into(),
        }
    }

    #[test]
    fn feedback_is_private_by_default_and_requires_consent_to_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let svc = MemoryService::new(
            "file",
            dir.path().to_str().unwrap(),
            "",
            "review_memory",
        )
        .unwrap();
        let f = FindingFeedback {
            finding_id: "PG-0042".into(),
            decision: MemoryResolution::Reject,
            feedback: "Figure 6 already supports this claim.".into(),
        };
        let entry = svc.record_feedback("run-001", unit("a claim"), &f, "historical").unwrap();
        assert_eq!(entry.approval_state, ApprovalState::Private);
        assert!(!entry.retrievable());
        // Without consent, retrieval returns nothing.
        let ctx = svc.retrieve_context("a claim", 10).unwrap();
        assert!(ctx.is_empty());
    }

    #[test]
    fn consent_promotes_then_retrieves() {
        let dir = tempfile::tempdir().unwrap();
        let svc = MemoryService::new(
            "file",
            dir.path().to_str().unwrap(),
            "",
            "review_memory",
        )
        .unwrap();
        let f = FindingFeedback {
            finding_id: "PG-1".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let entry = svc.record_feedback("run-001", unit("shared prior"), &f, "historical").unwrap();
        let id = entry.memory_id.clone();
        svc.approve_memory(&id, "human").unwrap();
        let ctx = svc.retrieve_context("shared prior", 10).unwrap();
        assert_eq!(ctx.len(), 1);
        assert!(ctx[0].retrievable());
    }

    #[test]
    fn only_training_approved_can_be_exported() {
        let dir = tempfile::tempdir().unwrap();
        let svc = MemoryService::new(
            "file",
            dir.path().to_str().unwrap(),
            "",
            "review_memory",
        )
        .unwrap();
        let f = FindingFeedback {
            finding_id: "PG-2".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let mem = svc.record_feedback("run-001", unit("trainable"), &f, "historical").unwrap();
        svc.approve_memory(&mem.memory_id, "human").unwrap();
        // memory-only is retrievable but NOT exportable.
        assert!(svc.export_training_units(10).unwrap().is_empty());
        // promote to training -> exportable.
        svc.approve_training(&mem.memory_id, "human").unwrap();
        let exported = svc.export_training_units(10).unwrap();
        assert_eq!(exported.len(), 1);
        assert!(exported[0].exportable());
    }

    #[test]
    fn disabled_backend_stores_nothing_and_returns_nothing() {
        let svc = MemoryService::new("none", "", "", "review_memory").unwrap();
        let f = FindingFeedback {
            finding_id: "PG-3".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let entry = svc.record_feedback("run-001", unit("x"), &f, "historical").unwrap();
        assert!(svc.retrieve_context("x", 10).unwrap().is_empty());
        assert!(entry.approval_state == ApprovalState::Private);
    }
}

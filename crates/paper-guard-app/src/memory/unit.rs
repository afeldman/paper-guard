//! The review-memory domain model: meaningful review units, not whole papers.

use paper_guard_core::ContentHash;
use serde::{Deserialize, Serialize};

use super::state::{ApprovalState, MemoryResolution};

/// What kind of scientific unit a memory entry captures.
///
/// Review Memory stores *meaningful units* (claim, figure + caption, method,
/// reference + citation context) rather than copying entire papers into the
/// vector store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Claim,
    Figure,
    Method,
    Reference,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Claim => "claim",
            MemoryKind::Figure => "figure",
            MemoryKind::Method => "method",
            MemoryKind::Reference => "reference",
        }
    }
}

/// A single stored review unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMemoryUnit {
    /// The reviewer kind that produced the original finding (e.g. "evidence").
    pub reviewer_kind: String,
    /// The type of unit (claim/figure/method/reference).
    pub kind: MemoryKind,
    /// Short text describing the unit (e.g. the claim text or figure caption).
    pub text: String,
    /// The original finding text.
    pub finding: String,
    /// Optional surrounding context (kept short; never a whole paper).
    #[serde(default)]
    pub context: String,
}

/// A full review-memory entry as persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMemoryEntry {
    /// Stable memory id (e.g. `mem-uuid`).
    pub memory_id: String,
    /// The run id that produced the original review.
    pub source_run_id: String,
    /// The reviewer kind that produced the finding.
    pub reviewer_kind: String,
    /// The unit of memory being stored.
    pub unit: ReviewMemoryUnit,
    /// The human decision on the finding (accept/reject/modify).
    pub resolution: MemoryResolution,
    /// Optional human feedback text.
    #[serde(default)]
    pub human_feedback: String,
    /// Provenance scope marker (always a historical-review tag, never current).
    pub provenance: String,
    /// A content hash of the unit for dedup/audit.
    pub unit_hash: ContentHash,
    /// An optional embedding vector (filled by the vector backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    /// When this entry was created (ISO-8601).
    pub created_at: String,
    /// The current approval state (defaults to [`ApprovalState::Private`]).
    pub approval_state: ApprovalState,
}

impl ReviewMemoryEntry {
    /// Create a new entry in the default [`ApprovalState::Private`] state.
    pub fn private(
        memory_id: String,
        source_run_id: String,
        unit: ReviewMemoryUnit,
        resolution: MemoryResolution,
        human_feedback: String,
        provenance: String,
        created_at: String,
    ) -> Self {
        let unit_hash = ContentHash::compute(&format!(
            "{}|{}|{}|{}",
            unit.reviewer_kind,
            unit.kind.as_str(),
            unit.text,
            resolution.as_str()
        ));
        ReviewMemoryEntry {
            memory_id,
            source_run_id,
            reviewer_kind: unit.reviewer_kind.clone(),
            unit,
            resolution,
            human_feedback,
            provenance,
            unit_hash,
            embedding: None,
            created_at,
            approval_state: ApprovalState::Private,
        }
    }

    /// Build the retrieval text used when this unit is injected as context.
    ///
    /// Memory is always framed as **historical review experience**, never as
    /// evidence for the current manuscript.
    pub fn context_text(&self) -> String {
        format!(
            "[HISTORICAL REVIEW MEMORY {}] reviewer={} unit={} finding={} human_decision={}",
            self.approval_state.describe(),
            self.unit.reviewer_kind,
            self.unit.kind.as_str(),
            self.unit.finding,
            self.resolution.as_str()
        )
    }

    /// Whether this entry may be used as retrieval context.
    pub fn retrievable(&self) -> bool {
        self.approval_state.retrievable_as_context()
    }

    /// Whether this entry may be exported to a training dataset.
    pub fn exportable(&self) -> bool {
        self.approval_state.exportable_to_training()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryKind;

    fn sample() -> ReviewMemoryEntry {
        ReviewMemoryEntry::private(
            "mem-1".into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: "the method reduces latency".into(),
                finding: "claim lacks supporting evidence".into(),
                context: "INSUFFICIENT_EVIDENCE".into(),
            },
            MemoryResolution::Accept,
            "accepted by human".into(),
            "historical-review".into(),
            "2026-01-01T00:00:00Z".into(),
        )
    }

    #[test]
    fn new_entry_is_private_and_not_retrievable() {
        let e = sample();
        assert_eq!(e.approval_state, ApprovalState::Private);
        assert!(!e.retrievable());
        assert!(!e.exportable());
    }

    #[test]
    fn context_text_is_framed_as_historical_memory_not_evidence() {
        let e = sample();
        let t = e.context_text();
        assert!(t.contains("HISTORICAL REVIEW MEMORY"));
        assert!(t.contains("private"));
    }

    #[test]
    fn unit_hash_is_stable_for_identical_units() {
        let a = sample();
        let b = sample();
        assert_eq!(a.unit_hash, b.unit_hash);
    }
}

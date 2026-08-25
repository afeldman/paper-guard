//! The review-memory domain model: meaningful review units, not whole papers.

use paper_guard_core::ContentHash;
use serde::{Deserialize, Serialize};

use super::state::{ApprovalState, MemoryResolution, MemoryScope};

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
    /// The claim context the finding was about (short; never a whole paper).
    #[serde(default)]
    pub claim_context: String,
    /// The evidence context relevant to the finding (short; never raw paper
    /// text beyond what is needed to reason about the finding).
    #[serde(default)]
    pub evidence_context: String,
    /// An explicit category for the review experience (e.g. `unsupported_claim`,
    /// `missing_evidence`, `overclaim`, `citation_issue`). Used for retrieval
    /// filtering and prompt framing.
    #[serde(default)]
    pub category: String,
}

/// A full review-memory entry as persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMemoryEntry {
    /// Stable schema version of this memory record (for forward/backward
    /// compatible migration and auditable provenance).
    pub schema_version: u32,
    /// Stable memory id (e.g. `mem-uuid`).
    pub memory_id: String,
    /// The run id that produced the original review.
    pub source_run_id: String,
    /// The id of the specific finding in the source run (if known), for
    /// precise provenance.
    #[serde(default)]
    pub source_finding_id: String,
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
    /// Ownership/sharing scope of this unit.
    #[serde(default)]
    pub scope: MemoryScope,
    /// The owner of this unit (a lightweight identity tag; never a secret).
    #[serde(default)]
    pub owner_id: String,
    /// The team this unit belongs to when [`scope`](MemoryScope::Team). Empty
    /// means no team.
    #[serde(default)]
    pub team_id: String,
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

/// The current schema version of [`ReviewMemoryEntry`].
pub const MEMORY_SCHEMA_VERSION: u32 = 1;

impl ReviewMemoryEntry {
    /// Create a new entry in the default `PRIVATE` / `PRIVATE`-scope state.
    pub fn private(
        memory_id: String,
        source_run_id: String,
        unit: ReviewMemoryUnit,
        resolution: MemoryResolution,
        human_feedback: String,
        provenance: String,
        created_at: String,
    ) -> Self {
        ReviewMemoryEntry::private_for_owner(
            memory_id,
            source_run_id,
            unit,
            resolution,
            human_feedback,
            provenance,
            String::new(),
            created_at,
        )
    }

    /// Create a new private entry attributed to a specific owner (M4 team
    /// foundation). Scope defaults to [`MemoryScope::Private`].
    #[allow(clippy::too_many_arguments)]
    pub fn private_for_owner(
        memory_id: String,
        source_run_id: String,
        unit: ReviewMemoryUnit,
        resolution: MemoryResolution,
        human_feedback: String,
        provenance: String,
        owner_id: String,
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
            schema_version: MEMORY_SCHEMA_VERSION,
            memory_id,
            source_run_id,
            source_finding_id: String::new(),
            reviewer_kind: unit.reviewer_kind.clone(),
            unit,
            resolution,
            human_feedback,
            provenance,
            scope: MemoryScope::Private,
            owner_id,
            team_id: String::new(),
            unit_hash,
            embedding: None,
            created_at,
            approval_state: ApprovalState::Private,
        }
    }

    /// Whether a unit with the given scope is accessible to a caller carrying
    /// `caller_owner` (and optionally `caller_team`).
    ///
    /// Access rule: PRIVATE memory is visible only to its owner; TEAM memory is
    /// visible to any member of the owning team. This is the single source of
    /// truth used by every retrieval path (file + vector) so no backend can
    /// accidentally leak a unit outside its authorization boundary.
    pub fn accessible_to(&self, caller_owner: &str, caller_team: Option<&str>) -> bool {
        match self.scope {
            MemoryScope::Private => !self.owner_id.is_empty() && self.owner_id == caller_owner,
            MemoryScope::Team => {
                // Team memory is accessible to any member of the owning team.
                // The caller must provide a matching team id.
                !self.team_id.is_empty() && caller_team.map(|t| t == self.team_id).unwrap_or(false)
            }
        }
    }

    /// Grant an ownership/sharing scope to this unit (used at promotion time
    /// to make an approved unit shareable with a team).
    pub fn with_scope(mut self, scope: MemoryScope, team_id: String) -> Self {
        self.scope = scope;
        self.team_id = team_id;
        self
    }

    /// Build the embedding text used for vector similarity. This is a
    /// deterministic representation of the **review experience** (not raw
    /// manuscript text), including the human decision and feedback.
    pub fn embedding_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.unit.category.is_empty() {
            parts.push(format!("Category: {}", self.unit.category));
        }
        parts.push(format!("Reviewer: {}", self.unit.reviewer_kind));
        if !self.unit.claim_context.is_empty() {
            parts.push(format!("Claim context: {}", self.unit.claim_context));
        }
        if !self.unit.evidence_context.is_empty() {
            parts.push(format!("Evidence context: {}", self.unit.evidence_context));
        }
        parts.push(format!("Finding: {}", self.unit.finding));
        parts.push(format!("Human decision: {}", self.resolution.as_str()));
        if !self.human_feedback.is_empty() {
            parts.push(format!("Human feedback: {}", self.human_feedback));
        }
        parts.join("\n")
    }

    /// Build the retrieval text used when this unit is injected as context.
    ///
    /// Memory is always framed as **historical review experience**, never as
    /// evidence for the current manuscript.
    pub fn context_text(&self) -> String {
        format!(
            "[HISTORICAL REVIEW MEMORY {} scope={}] reviewer={} unit={} finding={} human_decision={}{}",
            self.approval_state.describe(),
            self.scope.describe(),
            self.unit.reviewer_kind,
            self.unit.kind.as_str(),
            self.unit.finding,
            self.resolution.as_str(),
            if self.human_feedback.is_empty() {
                String::new()
            } else {
                format!(" human_feedback={}", self.human_feedback)
            }
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
                claim_context: "the method reduces latency".into(),
                evidence_context: "no measurement reported".into(),
                category: "missing_evidence".into(),
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

    #[test]
    fn embedding_text_represents_review_experience_not_raw_paper() {
        let e = sample();
        let t = e.embedding_text();
        assert!(t.contains("Category: missing_evidence"));
        assert!(t.contains("Reviewer: evidence"));
        assert!(t.contains("Finding: claim lacks supporting evidence"));
        assert!(t.contains("Human decision: accept"));
        assert!(t.contains("Human feedback: accepted by human"));
    }

    #[test]
    fn private_scope_requires_same_owner() {
        let e = sample();
        // No owner => not accessible to anyone.
        assert!(!e.accessible_to("alice", Some("team-a")));
        // Attach an owner; only that owner has access.
        let mut owned = e.clone();
        owned.owner_id = "alice".into();
        assert!(owned.accessible_to("alice", Some("team-a")));
        assert!(!owned.accessible_to("bob", Some("team-a")));
    }

    #[test]
    fn team_scope_is_accessible_to_any_team_member() {
        let e = sample().with_scope(crate::memory::MemoryScope::Team, "team-a".into());
        assert!(!e.accessible_to("alice", None));
        assert!(e.accessible_to("alice", Some("team-a")));
        assert!(e.accessible_to("bob", Some("team-a")));
        assert!(!e.accessible_to("carol", Some("team-b")));
    }

    #[test]
    fn schema_version_is_set() {
        let e = sample();
        assert_eq!(e.schema_version, MEMORY_SCHEMA_VERSION);
        assert_eq!(MEMORY_SCHEMA_VERSION, 1);
    }
}

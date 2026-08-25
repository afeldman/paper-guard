//! Privacy and approval states for Review Memory.
//!
//! The default state is [`ApprovalState::Private`]. A memory entry only becomes
//! eligible to be retrieved as context (`MemoryApproved`) or exported to a
//! training dataset (`TrainingApproved`) through explicit human consent. This
//! guarantees that a paper is never used for anything beyond its own review
//! unless a person explicitly approves it.

use serde::{Deserialize, Serialize};

/// The approval state of a review-memory unit.
///
/// These are serialized to stable strings so approval can be audited across
/// runs and exported datasets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    /// Default. The unit exists but cannot be retrieved as memory context or
    /// used for training. It is kept in the repository so it *can* later be
    /// promoted if a human approves it, but it is inert until then.
    Private,
    /// The unit may be retrieved as retrieval context for future reviews.
    MemoryApproved,
    /// The unit may be exported to a versioned training dataset (and also be
    /// retrieved as context).
    TrainingApproved,
    /// The unit was reviewed and explicitly rejected by a human.
    Rejected,
}

impl ApprovalState {
    /// Whether this unit may be used as retrieval context for a future review.
    pub fn retrievable_as_context(&self) -> bool {
        matches!(
            self,
            ApprovalState::MemoryApproved | ApprovalState::TrainingApproved
        )
    }

    /// Whether this unit may be exported to a (versioned, human-approved)
    /// training dataset.
    pub fn exportable_to_training(&self) -> bool {
        matches!(self, ApprovalState::TrainingApproved)
    }

    /// A stable, human-readable description.
    pub fn describe(&self) -> &'static str {
        match self {
            ApprovalState::Private => "private",
            ApprovalState::MemoryApproved => "memory_approved",
            ApprovalState::TrainingApproved => "training_approved",
            ApprovalState::Rejected => "rejected",
        }
    }
}

/// An explicit consent decision recorded for auditability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    /// The memory id this consent applies to.
    pub memory_id: String,
    /// The approving actor / reviewer identity (never a secret).
    pub actor: String,
    /// The new approval state granted by this consent.
    pub state: ConsentGrant,
    /// When the consent was recorded (ISO-8601).
    pub timestamp: String,
}

/// What a person is consenting to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentGrant {
    /// Promote to memory-context eligibility.
    ApproveMemory,
    /// Promote to training-dataset eligibility (implies memory eligibility).
    ApproveTraining,
    /// Explicitly reject the unit (it is removed from any retrieval/export).
    Reject,
}

/// The resolution of a human decision on a finding (the human-feedback layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryResolution {
    /// The human accepted the finding as correct.
    Accept,
    /// The human rejected the finding.
    Reject,
    /// The human modified the finding before accepting it.
    Modified,
}

/// The ownership / sharing scope of a review-memory unit.
///
/// M4 introduces a simple two-level scope so a team can share an approved
/// review memory without building a full enterprise identity system:
///
/// * [`MemoryScope::Private`] — visible only to its owner (the default; matches
///   the M3 `PRIVATE` approval-state default).
/// * [`MemoryScope::Team`] — visible to any member of the owning team.
///
/// Scope is orthogonal to *approval state*: a unit may be `MEMORY_APPROVED`
/// (retrievable) but still `Private`-scoped. Retrieval always enforces the
/// intersection of (approval state == approved) AND (scope grants access).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    /// Visible only to the owner of the unit. Never retrieved for anyone else.
    #[default]
    Private,
    /// Visible to members of the owning team (when the caller carries a
    /// matching `team_id`).
    Team,
}

impl MemoryScope {
    pub fn describe(&self) -> &'static str {
        match self {
            MemoryScope::Private => "private",
            MemoryScope::Team => "team",
        }
    }
}

impl MemoryResolution {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryResolution::Accept => "accept",
            MemoryResolution::Reject => "reject",
            MemoryResolution::Modified => "modified",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_private_and_inert() {
        // The fundamental guarantee: nothing starts retrievable or exportable.
        assert_eq!(ApprovalState::Private.describe(), "private");
        assert!(!ApprovalState::Private.retrievable_as_context());
        assert!(!ApprovalState::Private.exportable_to_training());
    }

    #[test]
    fn approval_grants_are_audited() {
        let consent = Consent {
            memory_id: "mem-1".into(),
            actor: "human-reviewer".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(consent.memory_id, "mem-1");
        assert_eq!(consent.state, ConsentGrant::ApproveMemory);
    }
}

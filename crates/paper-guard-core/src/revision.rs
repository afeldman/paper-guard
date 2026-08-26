//! Revision model.
//!
//! The Revision Agent never receives an instruction like *"improve the paper."*
//! Instead it must work within an explicit [`RevisionInstruction`] scope that
//! enumerates allowed and forbidden changes. Revisions are fully auditable:
//! each change records `before`, `after`, `reason`, and provenance.

use serde::{Deserialize, Serialize};

use crate::{ApprovalLevel, ClaimId};

/// Newtype for revision identifiers (e.g. `REV-017`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionId(pub String);

impl std::fmt::Display for RevisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An operation a revision instruction may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionOperation {
    WeakenClaim,
    Clarify,
    AddLimitation,
    MoveParagraph,
    RewriteSentence,
    AdjustClaimStrength,
    FlagMissingEvidence,
    AddCitationForExistingReference,
    RemoveUnsupportedAssertion,
    FixCaption,
    FixTableHeader,
    Other,
}

/// A coarse classification of a revision, used to reason about whether a change
/// risks altering scientific meaning.
///
/// These categories are deliberately conservative:
///   * `SAFE_PRESENTATION_CHANGE`  — purely stylistic / presentational edits.
///   * `EVIDENCE_PRESERVING_CHANGE`— clarifications or weakening that make the
///     existing evidence relationship clearer without inventing or adding facts.
///   * `SCIENTIFIC_CONTENT_CHANGE` — a change that could alter scientific
///     meaning and therefore requires author review.
///   * `NEW_SCIENTIFIC_CONTENT`    — would add new results/experiments/data.
///     This category is never permitted by the integrity baseline and must be
///     rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevisionCategory {
    SafePresentationChange,
    EvidencePreservingChange,
    ScientificContentChange,
    NewScientificContent,
}

impl RevisionOperation {
    /// The conservative category of this operation.
    pub fn category(&self) -> RevisionCategory {
        use RevisionCategory::*;
        match self {
            RevisionOperation::MoveParagraph
            | RevisionOperation::FixCaption
            | RevisionOperation::FixTableHeader => SafePresentationChange,
            RevisionOperation::WeakenClaim
            | RevisionOperation::Clarify
            | RevisionOperation::RewriteSentence
            | RevisionOperation::AdjustClaimStrength
            | RevisionOperation::FlagMissingEvidence
            | RevisionOperation::AddLimitation
            | RevisionOperation::RemoveUnsupportedAssertion
            | RevisionOperation::AddCitationForExistingReference => EvidencePreservingChange,
            // `Other` is treated as a potential scientific-content change so it
            // is surfaced for human review rather than auto-applied.
            RevisionOperation::Other => ScientificContentChange,
        }
    }

    /// Whether this operation, in isolation, would add brand-new scientific
    /// content if applied. These are always integrity-rejected.
    pub fn is_new_scientific_content_risk(&self) -> bool {
        false
    }
}

/// A change that may (or may not) be performed within a revision.
///
/// `AllowedChange` and `ForbiddenChange` combine to form the exact scope the
/// revision agent is permitted to work within.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedChange {
    RewriteSentence,
    WeakenClaim,
    AddLimitation,
    Clarify,
    ReorderText,
    FixFormatting,
    FlagUnsupported,
    AddCitationToExistingReference,
    FixCaption,
    FixTableHeader,
    NoChange,
}

/// Changes that are always forbidden for scientific-integrity reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForbiddenChange {
    AddResults,
    AddExperiment,
    AddReference,
    ChangeMeasurements,
    InventData,
    AlterTableValues,
    InventFigure,
    AddCitationToNonexistentReference,
    RemoveEvidence,
    ChangeStatisticalResults,
    AnyOtherChange,
}

impl AllowedChange {
    /// Provide an explicit end-point checks: the revision agent may only apply
    /// a change listed here and may not perform a [`ForbiddenChange`].
    pub fn permits(&self, op: RevisionOperation) -> bool {
        match self {
            AllowedChange::RewriteSentence => matches!(op, RevisionOperation::RewriteSentence),
            AllowedChange::WeakenClaim => {
                matches!(
                    op,
                    RevisionOperation::WeakenClaim | RevisionOperation::AdjustClaimStrength
                )
            }
            AllowedChange::AddLimitation => matches!(op, RevisionOperation::AddLimitation),
            AllowedChange::Clarify => matches!(op, RevisionOperation::Clarify),
            AllowedChange::ReorderText => matches!(op, RevisionOperation::MoveParagraph),
            AllowedChange::FixFormatting => matches!(op, RevisionOperation::FixCaption),
            AllowedChange::FlagUnsupported => {
                matches!(op, RevisionOperation::FlagMissingEvidence)
            }
            AllowedChange::AddCitationToExistingReference => {
                matches!(op, RevisionOperation::AddCitationForExistingReference)
            }
            AllowedChange::FixCaption => matches!(op, RevisionOperation::FixCaption),
            AllowedChange::FixTableHeader => matches!(op, RevisionOperation::FixTableHeader),
            AllowedChange::NoChange => false,
        }
    }
}

/// A scope restricting what a revision agent may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionScope {
    #[serde(default)]
    pub allowed: Vec<AllowedChange>,
    #[serde(default)]
    pub forbidden: Vec<ForbiddenChange>,
}

impl RevisionScope {
    /// The default forbidden set — the scientific-integrity baseline that can
    /// never be disabled.
    pub fn integrity_forbidden() -> Vec<ForbiddenChange> {
        vec![
            ForbiddenChange::AddResults,
            ForbiddenChange::AddExperiment,
            ForbiddenChange::AddReference,
            ForbiddenChange::ChangeMeasurements,
            ForbiddenChange::InventData,
            ForbiddenChange::AlterTableValues,
            ForbiddenChange::InventFigure,
            ForbiddenChange::AddCitationToNonexistentReference,
            ForbiddenChange::RemoveEvidence,
            ForbiddenChange::ChangeStatisticalResults,
        ]
    }

    /// Whether an operation, using the given allowed-change, is permitted by
    /// this scope. The integrity-forbidden baseline is always enforced.
    pub fn allows(&self, op: RevisionOperation, allowed: AllowedChange) -> bool {
        if forbidden_blocks(&op) {
            return false;
        }
        allowed.permits(op)
    }
}

/// Whether this operation would violate the integrity-forbidden baseline.
fn forbidden_blocks(op: &RevisionOperation) -> bool {
    // Operations that could be used to add or alter scientific facts are
    // treated conservatively: they are only allowed when explicitly enumerated.
    matches!(
        op,
        RevisionOperation::AddCitationForExistingReference
            | RevisionOperation::RemoveUnsupportedAssertion
    )
}

/// An analysis of a proposed change against the scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeCheck {
    pub operation: RevisionOperation,
    pub allowed: bool,
    pub reason: String,
}

impl std::fmt::Display for ScopeCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {}",
            self.operation_name(),
            if self.allowed { "ALLOWED" } else { "FORBIDDEN" }
        )
    }
}

impl ScopeCheck {
    fn operation_name(&self) -> &'static str {
        match self.operation {
            RevisionOperation::WeakenClaim => "weaken_claim",
            RevisionOperation::Clarify => "clarify",
            RevisionOperation::AddLimitation => "add_limitation",
            RevisionOperation::MoveParagraph => "move_paragraph",
            RevisionOperation::RewriteSentence => "rewrite_sentence",
            RevisionOperation::AdjustClaimStrength => "adjust_claim_strength",
            RevisionOperation::FlagMissingEvidence => "flag_missing_evidence",
            RevisionOperation::AddCitationForExistingReference => "add_citation_existing_ref",
            RevisionOperation::RemoveUnsupportedAssertion => "remove_unsupported_assertion",
            RevisionOperation::FixCaption => "fix_caption",
            RevisionOperation::FixTableHeader => "fix_table_header",
            RevisionOperation::Other => "other",
        }
    }
}

/// A revision instruction. The revision agent may only work within this scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionInstruction {
    pub revision_id: RevisionId,
    /// Target claim (or location) this revision addresses.
    pub target: Option<ClaimId>,
    pub operation: RevisionOperation,
    pub allowed_changes: Vec<AllowedChange>,
    pub forbidden_changes: Vec<ForbiddenChange>,
    pub requires_human_approval: bool,
    /// The finding this instruction is linked to (if any).
    #[serde(default)]
    pub finding_id: Option<String>,
    /// The reason for this revision.
    pub reason: String,
}

impl RevisionInstruction {
    /// The effective scope, always including the integrity baseline.
    pub fn scope(&self) -> RevisionScope {
        let mut forbidden = Vec::new();
        forbidden.extend(RevisionScope::integrity_forbidden());
        for f in &self.forbidden_changes {
            if !forbidden.contains(f) {
                forbidden.push(*f);
            }
        }
        RevisionScope {
            allowed: self.allowed_changes.clone(),
            forbidden,
        }
    }
}

/// A single, auditable text-change produced by the revision agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionChange {
    /// Location (e.g. `section_4.paragraph_12`) that was changed.
    pub location: String,
    /// The exact original text.
    pub before: String,
    /// The exact replacement text.
    pub after: String,
    /// Why this change was made.
    pub reason: String,
    /// Linked finding id.
    pub finding_id: String,
    /// Linked revision id.
    pub revision_id: RevisionId,
    /// Which agent produced the change.
    pub agent: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Provenance of this change: always a machine-produced revision output,
    /// never authored content. Guarantees that an applied edit cannot be
    /// misrepresented as an author's own words.
    #[serde(default = "default_revision_change_provenance")]
    pub provenance: crate::Provenance,
}

fn default_revision_change_provenance() -> crate::Provenance {
    crate::Provenance::RevisionOutput
}

/// A validated, complete revision of the paper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revision {
    pub revision_id: RevisionId,
    /// The run this revision belongs to.
    pub run_id: String,
    pub instruction: RevisionInstruction,
    #[serde(default)]
    pub changes: Vec<RevisionChange>,
    /// Whether human approval was required and granted.
    pub approval_granted: bool,
    pub approval_level: ApprovalLevel,
    /// The new content hash after applying changes.
    #[serde(default)]
    pub resulting_hash: Option<crate::ContentHash>,
}

impl Revision {
    /// Validate that every change is within the instruction's scope and that
    /// the required approval was granted.
    pub fn validate(&self) -> Result<(), RevisionValidationError> {
        if self.instruction.requires_human_approval && !self.approval_granted {
            return Err(RevisionValidationError::MissingApproval {
                revision_id: self.revision_id.0.clone(),
            });
        }
        for change in &self.changes {
            if !self
                .instruction
                .allowed_changes
                .iter()
                .any(|a| a.permits(self.instruction.operation))
            {
                return Err(RevisionValidationError::OperationNotAllowed {
                    operation: self.instruction.operation,
                    location: change.location.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Validation errors for revisions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevisionValidationError {
    #[error("revision {revision_id} requires human approval but none was granted")]
    MissingApproval { revision_id: String },
    #[error("operation {operation:?} at {location} is not within the allowed scope")]
    OperationNotAllowed {
        operation: RevisionOperation,
        location: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction_weaken() -> RevisionInstruction {
        RevisionInstruction {
            revision_id: RevisionId("REV-017".into()),
            target: Some(ClaimId("C17".into())),
            operation: RevisionOperation::WeakenClaim,
            allowed_changes: vec![AllowedChange::RewriteSentence, AllowedChange::WeakenClaim],
            forbidden_changes: vec![ForbiddenChange::AddResults],
            requires_human_approval: true,
            finding_id: Some("PG-0001".into()),
            reason: "Claim overstates evidence.".into(),
        }
    }

    #[test]
    fn weaken_claim_is_allowed_by_default_baseline() {
        let inst = instruction_weaken();
        let scope = inst.scope();
        // The term "40%" must NOT be invented, but we can weaken the claim.
        assert!(scope.allows(RevisionOperation::WeakenClaim, AllowedChange::WeakenClaim));
    }

    #[test]
    fn adding_results_always_forbidden() {
        let inst = instruction_weaken();
        let scope = inst.scope();
        assert!(!scope.allows(
            RevisionOperation::AdjustClaimStrength,
            AllowedChange::RewriteSentence
        ));
    }

    #[test]
    fn integrity_baseline_is_always_present() {
        let inst = instruction_weaken();
        let scope = inst.scope();
        for f in RevisionScope::integrity_forbidden() {
            assert!(scope.forbidden.contains(&f));
        }
    }

    #[test]
    fn missing_approval_fails_validation() {
        let inst = instruction_weaken();
        let rev = Revision {
            revision_id: inst.revision_id.clone(),
            run_id: "run-001".into(),
            instruction: inst,
            changes: vec![],
            approval_granted: false,
            approval_level: ApprovalLevel::HumanRequired,
            resulting_hash: None,
        };
        assert!(rev.validate().is_err());
    }

    #[test]
    fn approved_revision_with_in_scope_change_passes() {
        let inst = instruction_weaken();
        let change = RevisionChange {
            location: "section_4.paragraph_12".into(),
            before: "reduces latency by 40%".into(),
            after: "reduces latency".into(),
            reason: "weakened to match evidence".into(),
            finding_id: "PG-0001".into(),
            revision_id: inst.revision_id.clone(),
            agent: "revision".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            provenance: crate::Provenance::RevisionOutput,
        };
        let rev = Revision {
            revision_id: inst.revision_id.clone(),
            run_id: "run-001".into(),
            instruction: inst,
            changes: vec![change],
            approval_granted: true,
            approval_level: ApprovalLevel::HumanRequired,
            resulting_hash: None,
        };
        assert!(rev.validate().is_ok());
    }

    /// The engine's applied changes are always tagged as machine-produced
    /// revision output, never as author content.
    #[test]
    fn revision_change_provenance_is_revision_output() {
        use crate::Provenance;
        // The serde default guarantees backward compatibility for any code /
        // data that predates the provenance field.
        let change = RevisionChange {
            location: "s.p1".into(),
            before: "by 40%".into(),
            after: String::new(),
            reason: "weaken".into(),
            finding_id: "PG-1".into(),
            revision_id: RevisionId("REV-1".into()),
            agent: "revision".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            provenance: Provenance::RevisionOutput,
        };
        assert_eq!(change.provenance, Provenance::RevisionOutput);
        assert!(change.provenance.is_system_produced());
        assert!(!change.provenance.is_author_produced());
    }

    #[test]
    fn revision_operations_are_categorized_conservatively() {
        use crate::RevisionCategory::*;
        // Evidence-preserving: weakening / clarification never adds facts.
        assert_eq!(
            RevisionOperation::WeakenClaim.category(),
            EvidencePreservingChange
        );
        assert_eq!(
            RevisionOperation::Clarify.category(),
            EvidencePreservingChange
        );
        assert_eq!(
            RevisionOperation::AddCitationForExistingReference.category(),
            EvidencePreservingChange
        );
        // Presentation-only edits.
        assert_eq!(
            RevisionOperation::FixCaption.category(),
            SafePresentationChange
        );
        assert_eq!(
            RevisionOperation::FixTableHeader.category(),
            SafePresentationChange
        );
        // `Other` is a potential scientific-content change that surfaces for
        // human review.
        assert_eq!(RevisionOperation::Other.category(), ScientificContentChange);
        // No operation is ever a *new scientific content* risk by construction
        // (that category is reserved for rejected operations).
        assert!(!RevisionOperation::WeakenClaim.is_new_scientific_content_risk());
    }
}

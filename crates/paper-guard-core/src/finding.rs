//! Review finding model.
//!
//! Every reviewer returns findings in a shared, structured schema. Findings are
//! the currency exchanged between reviewers, the judge, and the ledger.

use serde::{Deserialize, Serialize};

use crate::{ClaimId, FindingSeverity};

/// The category of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    UnsupportedClaim,
    WeakEvidence,
    Overclaiming,
    Contradiction,
    MissingControl,
    Confounder,
    Bias,
    StatisticalWeakness,
    Leakage,
    Reproducibility,
    RefactoredLogic,
    LogicalGap,
    InterpretationError,
    Limitation,
    ReferenceError,
    HallucinatedReference,
    MissingReference,
    CitationMismatch,
    FigureIssue,
    TableIssue,
    Inconsistency,
    Methodology,
    PromptInjection,
    Other,
}

/// A named reviewer that produced a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerKind {
    Scientific,
    Adversarial,
    Evidence,
    References,
    Figures,
    Judge,
}

impl ReviewerKind {
    /// Canonical string used in ledger and finding `reviewer` field.
    pub fn name(&self) -> &'static str {
        match self {
            ReviewerKind::Scientific => "scientific",
            ReviewerKind::Adversarial => "adversarial",
            ReviewerKind::Evidence => "evidence",
            ReviewerKind::References => "references",
            ReviewerKind::Figures => "figures",
            ReviewerKind::Judge => "judge",
        }
    }
}

impl std::fmt::Display for ReviewerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Lifecycle status of a finding across iterations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FindingStatus {
    /// Newly reported, not yet addressed.
    Open,
    /// Acknowledged by the workflow (e.g. a revision assigned).
    Acknowledged,
    /// A proposed revision was approved.
    Approved,
    /// Addressed by an approved revision.
    Revised,
    /// Re-verified as resolved in a later run.
    Resolved,
    /// The proposed resolution was rejected.
    Rejected,
    /// A previously resolved problem was reintroduced.
    Regressed,
}

impl FindingStatus {
    pub fn describe(&self) -> &'static str {
        match self {
            FindingStatus::Open => "OPEN",
            FindingStatus::Acknowledged => "ACKNOWLEDGED",
            FindingStatus::Approved => "APPROVED",
            FindingStatus::Revised => "REVISED",
            FindingStatus::Resolved => "RESOLVED",
            FindingStatus::Rejected => "REJECTED",
            FindingStatus::Regressed => "REGRESSED",
        }
    }
}

/// A finding produced by a reviewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Stable identifier (e.g. `PG-0042`).
    pub finding_id: String,
    /// The reviewer that produced it.
    pub reviewer: ReviewerKind,
    /// Location in the document (e.g. `section_4.paragraph_12`).
    pub location: String,
    pub category: FindingCategory,
    pub severity: FindingSeverity,
    /// Confidence 0..=1.
    pub confidence: f32,
    /// Optional linked claim.
    #[serde(default)]
    pub claim_id: Option<ClaimId>,
    /// Human-readable finding description.
    pub finding: String,
    /// Evidence artifact ids referenced.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Optional recommended change.
    pub recommendation: String,
    /// Whether this finding needs human approval to revise.
    pub requires_human_approval: bool,
}

impl Finding {
    /// A compact one-line summary.
    pub fn summary(&self) -> String {
        format!(
            "{} [{}] {} {}: {}",
            self.finding_id,
            self.reviewer.name(),
            self.severity.priority(),
            self.location,
            self.finding
        )
    }

    /// Validate structural invariants of a finding.
    pub fn validate(&self) -> Result<(), FindingValidationError> {
        if self.confidence < 0.0 || self.confidence > 1.0 {
            return Err(FindingValidationError::ConfidenceOutOfRange(self.confidence));
        }
        if self.finding_id.trim().is_empty() {
            return Err(FindingValidationError::EmptyFindingId);
        }
        if self.finding.trim().is_empty() {
            return Err(FindingValidationError::EmptyFindingText);
        }
        Ok(())
    }
}

/// A validation error for a finding.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FindingValidationError {
    #[error("finding confidence {0} is outside [0,1]")]
    ConfidenceOutOfRange(f32),
    #[error("finding id is empty")]
    EmptyFindingId,
    #[error("finding text is empty")]
    EmptyFindingText,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Finding {
        Finding {
            finding_id: "PG-0001".into(),
            reviewer: ReviewerKind::Adversarial,
            location: "section_4.paragraph_12".into(),
            category: FindingCategory::UnsupportedClaim,
            severity: FindingSeverity::Major,
            confidence: 0.94,
            claim_id: Some(ClaimId("C17".into())),
            finding: "The claim lacks supporting evidence.".into(),
            evidence: vec!["F6".into(), "R12".into()],
            recommendation: "Add supporting evidence.".into(),
            requires_human_approval: true,
        }
    }

    #[test]
    fn valid_finding_passes() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn confidence_out_of_range() {
        let mut f = sample();
        f.confidence = 1.5;
        assert!(matches!(
            f.validate(),
            Err(FindingValidationError::ConfidenceOutOfRange(1.5))
        ));
    }

    #[test]
    fn empty_fields_rejected() {
        let mut f = sample();
        f.finding_id = "".into();
        assert!(f.validate().is_err());
        let mut f = sample();
        f.finding = "".into();
        assert!(f.validate().is_err());
    }

    #[test]
    fn category_and_status_serialize_stably() {
        let cat = FindingCategory::Overclaiming;
        let s = serde_json::to_string(&cat).unwrap();
        assert_eq!(s, "\"overclaiming\"");
        let st = FindingStatus::Open;
        let s2 = serde_json::to_string(&st).unwrap();
        assert_eq!(s2, "\"OPEN\"");
    }
}

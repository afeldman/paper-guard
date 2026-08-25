//! Scientific-integrity domain types.
//!
//! This module encodes the system's central rule:
//!
//! > **Paper Guard must never invent scientific facts.**
//!
//! The [`EvidenceState`] type provides a first-class representation of the
//! absence of verifiable evidence (`INSUFFICIENT_EVIDENCE`, `NOT_VERIFIED`),
//! and the [`IntegrityCheck`] / [`IntegrityViolation`] types let callers
//! explicitly detect and reject attempts to fabricate results, references,
//! measurements, or experiments.

use serde::{Deserialize, Serialize};

/// The state of evidence backing a claim, result, or reference.
///
/// Crucially, this type has **no** "fabricated" variant. When evidence is
/// missing, agents and checkers must mark it as insufficient or unverified —
/// they are structurally prevented from reporting plausible-but-unreal evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EvidenceState {
    /// Evidence exists and directly supports the claim.
    Supported,
    /// Evidence partially supports the claim.
    PartiallySupported,
    /// Evidence only weakly supports the claim.
    WeaklySupported,
    /// No evidence was located / the claim is not backed by the paper.
    Unsupported,
    /// The paper itself does not contain enough information to judge.
    InsufficientEvidence,
    /// The available evidence contradicts the claim.
    Contradicted,
    /// A reference's existence could not be verified against an authoritative
    /// source (this is *not* a claim of existence or non-existence).
    #[default]
    NotVerified,
}

impl EvidenceState {
    /// Whether this state represents a failure to confirm support.
    pub fn is_unconfirmed(&self) -> bool {
        matches!(
            self,
            EvidenceState::InsufficientEvidence
                | EvidenceState::Unsupported
                | EvidenceState::NotVerified
        )
    }

    /// A stable, machine-readable tag (as used in the spec examples).
    pub fn tag(&self) -> &'static str {
        match self {
            EvidenceState::Supported => "SUPPORTED",
            EvidenceState::PartiallySupported => "PARTIALLY_SUPPORTED",
            EvidenceState::WeaklySupported => "WEAKLY_SUPPORTED",
            EvidenceState::Unsupported => "UNSUPPORTED",
            EvidenceState::InsufficientEvidence => "INSUFFICIENT_EVIDENCE",
            EvidenceState::Contradicted => "CONTRADICTED",
            EvidenceState::NotVerified => "NOT_VERIFIED",
        }
    }
}

impl std::fmt::Display for EvidenceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.tag())
    }
}

/// The kinds of scientific fabrication that Paper Guard forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationKind {
    /// A result was invented.
    InventedResult,
    /// A measurement was invented or altered.
    InventedMeasurement,
    /// An experiment was invented.
    InventedExperiment,
    /// A dataset was invented.
    InventedDataset,
    /// A reference was invented.
    InventedReference,
    /// A citation was invented.
    InventedCitation,
    /// A figure was invented.
    InventedFigure,
    /// A table value was altered.
    AlteredTableValue,
    /// A statistical result was invented.
    InventedStatistic,
    /// Evidence that does not exist was represented as real.
    FabricatedEvidence,
    /// A claim was asserted as *proven* without supporting data.
    UnsupportedAssertionAsProven,
}

impl ViolationKind {
    /// A human-readable description.
    pub fn describe(&self) -> &'static str {
        match self {
            ViolationKind::InventedResult => "inventing a result",
            ViolationKind::InventedMeasurement => "inventing or altering a measurement",
            ViolationKind::InventedExperiment => "inventing an experiment",
            ViolationKind::InventedDataset => "inventing a dataset",
            ViolationKind::InventedReference => "inventing a reference",
            ViolationKind::InventedCitation => "inventing a citation",
            ViolationKind::InventedFigure => "inventing a figure",
            ViolationKind::AlteredTableValue => "altering a table value",
            ViolationKind::InventedStatistic => "inventing a statistical result",
            ViolationKind::FabricatedEvidence => "representing absent evidence as real",
            ViolationKind::UnsupportedAssertionAsProven => {
                "asserting an unsupported claim as proven"
            }
        }
    }
}

/// A detected violation of the scientific-integrity rule.
///
/// When any agent or component detects that the system is about to invent a
/// scientific fact, it records (at minimum) an [`IntegrityViolation`]. The
/// system treats such an attempt as an error and refuses to persist it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityViolation {
    /// A stable kind identifier.
    pub kind: ViolationKind,
    /// Where the attempted violation originated (e.g. `agent:evidence`).
    pub origin: String,
    /// A short human-readable description.
    pub message: String,
}

/// A result of an integrity check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityCheck {
    /// Set when an integrity violation was detected.
    pub violation: Option<IntegrityViolation>,
    /// The evidence-support state for the audited claim/result.
    pub evidence_state: EvidenceState,
    /// True when the check passed without any detected violation.
    pub passed: bool,
}

impl IntegrityCheck {
    /// A passing check with the given evidence state.
    pub fn ok(evidence_state: EvidenceState) -> Self {
        IntegrityCheck {
            violation: None,
            evidence_state,
            passed: true,
        }
    }

    /// A failing check due to a violation.
    pub fn violation(
        kind: ViolationKind,
        origin: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        IntegrityCheck {
            violation: Some(IntegrityViolation {
                kind,
                origin: origin.into(),
                message: message.into(),
            }),
            evidence_state: EvidenceState::InsufficientEvidence,
            passed: false,
        }
    }
}

/// Reject any evidence state that claims support without actual evidence.
///
/// This guard is used before persisting a support assertion. Callers must prove
/// the `Supported` / `PartiallySupported` / `WeaklySupported` states genuinely
/// correspond to artifacts; otherwise they may not be reported.
pub fn assert_not_fabricated(origin: &str, has_real_artifacts: bool, state: EvidenceState) -> IntegrityCheck {
    use EvidenceState::*;
    match (has_real_artifacts, state) {
        (true, Supported | PartiallySupported | WeaklySupported) => IntegrityCheck::ok(state),
        (false, Supported | PartiallySupported | WeaklySupported) => IntegrityCheck::violation(
            ViolationKind::FabricatedEvidence,
            origin,
            format!("attempted to report support ({state}) without a backing artifact"),
        ),
        _ => IntegrityCheck::ok(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_state_tags() {
        assert_eq!(
            EvidenceState::InsufficientEvidence.tag(),
            "INSUFFICIENT_EVIDENCE"
        );
        assert_eq!(EvidenceState::NotVerified.tag(), "NOT_VERIFIED");
        assert_eq!(EvidenceState::Supported.tag(), "SUPPORTED");
        assert_eq!(EvidenceState::Contradicted.tag(), "CONTRADICTED");
    }

    #[test]
    fn unconfirmed_states() {
        assert!(EvidenceState::InsufficientEvidence.is_unconfirmed());
        assert!(EvidenceState::Unsupported.is_unconfirmed());
        assert!(EvidenceState::NotVerified.is_unconfirmed());
        assert!(!EvidenceState::Supported.is_unconfirmed());
    }

    #[test]
    fn support_without_artifact_is_fabrication() {
        let check = assert_not_fabricated("test", false, EvidenceState::Supported);
        assert!(!check.passed);
        assert_eq!(
            check.violation.unwrap().kind,
            ViolationKind::FabricatedEvidence
        );
    }

    #[test]
    fn honest_unconfirmed_state_is_not_fabrication() {
        for state in [
            EvidenceState::InsufficientEvidence,
            EvidenceState::Unsupported,
            EvidenceState::NotVerified,
        ] {
            let check = assert_not_fabricated("test", false, state);
            assert!(check.passed, "state {state:?} should pass");
        }
    }

    #[test]
    fn genuine_support_passes() {
        for state in [
            EvidenceState::Supported,
            EvidenceState::PartiallySupported,
            EvidenceState::WeaklySupported,
        ] {
            let check = assert_not_fabricated("test", true, state);
            assert!(check.passed, "state {state:?} should pass");
        }
    }
}

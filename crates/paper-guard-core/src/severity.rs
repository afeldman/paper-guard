//! Severity and approval-level types.

use serde::{Deserialize, Serialize};

/// Severity of a finding.
///
/// The Judge agent assigns a severity, which maps directly to a priority and to
/// an [`ApprovalLevel`] governing whether a human must approve a revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FindingSeverity {
    /// Blocking issue; must be addressed before submission.
    Critical,
    /// A major issue that materially weakens the paper.
    Major,
    /// A moderate issue worth addressing.
    Moderate,
    /// A minor, cosmetic or low-impact issue.
    Minor,
}

impl FindingSeverity {
    /// Priority label used by the judge.
    pub fn priority(&self) -> &'static str {
        match self {
            FindingSeverity::Critical => "P0",
            FindingSeverity::Major => "P1",
            FindingSeverity::Moderate => "P2",
            FindingSeverity::Minor => "P3",
        }
    }

    /// The minimum human-approval level required for a revision targeting a
    /// finding of this severity (unless overridden by configuration).
    pub fn default_approval_level(&self) -> ApprovalLevel {
        match self {
            FindingSeverity::Critical => ApprovalLevel::HumanRequired,
            FindingSeverity::Major => ApprovalLevel::HumanRequired,
            FindingSeverity::Moderate => ApprovalLevel::Configurable,
            FindingSeverity::Minor => ApprovalLevel::Automatic,
        }
    }
}

/// Whether a revision requires human approval.
///
/// `Configurable` resolves to either `Automatic` or `HumanRequired` based on
/// the project configuration (e.g. `require_human_approval_for_major`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalLevel {
    /// May be applied automatically.
    Automatic,
    /// Whether approval is required is configurable.
    Configurable,
    /// Human approval is required.
    HumanRequired,
    /// Human approval is always required, no matter what.
    HumanRequiredCritical,
}

impl ApprovalLevel {
    /// Whether this level, combined with a configuration flag, requires human
    /// approval.
    pub fn requires_human(&self, configurable_default: bool) -> bool {
        match self {
            ApprovalLevel::Automatic => false,
            ApprovalLevel::Configurable => configurable_default,
            ApprovalLevel::HumanRequired => true,
            ApprovalLevel::HumanRequiredCritical => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_priority_mapping() {
        assert_eq!(FindingSeverity::Critical.priority(), "P0");
        assert_eq!(FindingSeverity::Major.priority(), "P1");
        assert_eq!(FindingSeverity::Moderate.priority(), "P2");
        assert_eq!(FindingSeverity::Minor.priority(), "P3");
    }

    #[test]
    fn approval_levels() {
        assert!(!ApprovalLevel::Automatic.requires_human(true));
        assert!(ApprovalLevel::Configurable.requires_human(true));
        assert!(!ApprovalLevel::Configurable.requires_human(false));
        assert!(ApprovalLevel::HumanRequired.requires_human(false));
        assert!(ApprovalLevel::HumanRequiredCritical.requires_human(false));
    }
}

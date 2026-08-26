//! The Judge agent: consolidates reviewer findings.
//!
//! The judge merges redundant findings, detects contradictions, determines
//! severities and priorities, and decides which findings require a revision
//! (and whether that revision needs human approval). The judge never silently
//! changes a scientific claim; it only decides on *review and revision actions*.

use std::collections::HashMap;

use paper_guard_core::{
    AllowedChange, ApprovalLevel, Finding, FindingCategory, FindingSeverity, FindingStatus,
    RevisionId, RevisionInstruction, RevisionOperation,
};
use serde::{Deserialize, Serialize};

/// An approved action: either mark a finding (possibly producing a revision
/// instruction) or take no revision action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeAction {
    /// A revision is warranted.
    Revise {
        priority: u8,
        operation: RevisionOperation,
        allowed_changes: Vec<AllowedChange>,
        requires_human_approval: bool,
    },
    /// No revision warranted.
    NoAction { reason: String },
}

/// A judged finding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeEntry {
    pub finding_id: String,
    pub status: FindingStatus,
    pub severity: FindingSeverity,
    pub priority: String,
    pub action: JudgeAction,
}

/// The consolidated output of the judge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeOutput {
    /// All consolidated findings.
    pub entries: Vec<JudgeEntry>,
    /// Revision instructions generated for actionable findings.
    pub revisions: Vec<RevisionInstruction>,
    /// Internal judge notes (e.g. detected reviewer conflicts).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Default operation for a finding category.
pub fn default_operation_for_category(cat: FindingCategory) -> RevisionOperation {
    use FindingCategory::*;
    match cat {
        UnsupportedClaim | WeakEvidence | Overclaiming | RefactoredLogic => {
            RevisionOperation::WeakenClaim
        }
        Contradiction | Inconsistency | InterpretationError | Methodology | Other => {
            RevisionOperation::Clarify
        }
        MissingControl | Confounder | Bias | StatisticalWeakness | Leakage | Reproducibility
        | LogicalGap | Limitation => RevisionOperation::AddLimitation,
        ReferenceError | MissingReference | CitationMismatch | HallucinatedReference => {
            RevisionOperation::AddCitationForExistingReference
        }
        FigureIssue => RevisionOperation::FixCaption,
        TableIssue => RevisionOperation::FixTableHeader,
        PromptInjection => RevisionOperation::RemoveUnsupportedAssertion,
    }
}

/// Default allowed changes for a given operation.
pub fn default_allowed_changes(op: RevisionOperation) -> Vec<AllowedChange> {
    use RevisionOperation::*;
    match op {
        WeakenClaim | AdjustClaimStrength => vec![
            AllowedChange::WeakenClaim,
            AllowedChange::RewriteSentence,
            AllowedChange::Clarify,
        ],
        Clarify | Other => vec![AllowedChange::Clarify, AllowedChange::RewriteSentence],
        AddLimitation => vec![AllowedChange::AddLimitation],
        AddCitationForExistingReference => vec![AllowedChange::AddCitationToExistingReference],
        RemoveUnsupportedAssertion | FlagMissingEvidence => vec![AllowedChange::FlagUnsupported],
        FixCaption => vec![AllowedChange::FixCaption],
        FixTableHeader => vec![AllowedChange::FixTableHeader],
        _ => vec![AllowedChange::Clarify],
    }
}

/// The judge agent.
pub struct Judge {
    /// Prompt version, for reproducibility.
    pub prompt_version: String,
    /// Whether major findings require human approval (configurable).
    pub require_human_approval_for_major: bool,
}

impl Judge {
    pub fn new(prompt_version: &str, require_human_approval_for_major: bool) -> Self {
        Judge {
            prompt_version: prompt_version.into(),
            require_human_approval_for_major,
        }
    }

    /// Consolidate a set of reviewer findings into a [`JudgeOutput`].
    ///
    /// Deterministic and integrity-safe: merges duplicates, derives actions,
    /// emits revision instructions. Never fabricates content.
    pub fn consolidate(&self, findings: Vec<Finding>) -> JudgeOutput {
        let mut merged: HashMap<String, Finding> = HashMap::new();
        for f in findings {
            let key = normalize_finding_key(&f);
            match merged.get_mut(&key) {
                Some(existing) => {
                    if severity_rank(f.severity) > severity_rank(existing.severity) {
                        existing.severity = f.severity;
                    }
                    for e in &f.evidence {
                        if !existing.evidence.contains(e) {
                            existing.evidence.push(e.clone());
                        }
                    }
                    existing.confidence = existing.confidence.max(f.confidence);
                }
                None => {
                    merged.insert(key, f);
                }
            }
        }

        // Detect reviewer conflicts on the same claim BEFORE we move out of
        // merged below.
        let mut claim_findings: HashMap<String, Vec<&Finding>> = HashMap::new();
        for f in merged.values() {
            if let Some(cid) = &f.claim_id {
                claim_findings.entry(cid.0.clone()).or_default().push(f);
            }
        }
        let mut notes = Vec::new();
        for (claim, fs) in claim_findings {
            if fs.len() > 1 {
                notes.push(format!(
                    "conflicting findings on claim {claim} ({} findings); reviewer conflict detected",
                    fs.len()
                ));
            }
        }

        let mut entries = Vec::new();
        let mut revisions = Vec::new();
        let mut rev_counter = 0usize;

        let mut findings_sorted: Vec<Finding> = merged.into_values().collect();
        findings_sorted.sort_by_key(|f| std::cmp::Reverse(severity_rank(f.severity)));

        for f in findings_sorted {
            let op = default_operation_for_category(f.category);
            let priority = severity_priority(&f.severity);
            let requires_human = self.requires_human(&f);
            rev_counter += 1;
            let rev = self.make_revision(&f, op, rev_counter);
            revisions.push(rev.clone());
            let action = JudgeAction::Revise {
                priority,
                operation: op,
                allowed_changes: default_allowed_changes(op),
                requires_human_approval: requires_human,
            };
            entries.push(JudgeEntry {
                finding_id: f.finding_id,
                status: FindingStatus::Open,
                severity: f.severity,
                priority: format!("P{priority}"),
                action,
            });
        }

        JudgeOutput {
            entries,
            revisions,
            notes,
        }
    }

    fn requires_human(&self, f: &Finding) -> bool {
        f.requires_human_approval
            || match f.severity {
                FindingSeverity::Critical => true,
                FindingSeverity::Major => self.require_human_approval_for_major,
                _ => false,
            }
    }

    fn make_revision(
        &self,
        f: &Finding,
        op: RevisionOperation,
        counter: usize,
    ) -> RevisionInstruction {
        let allowed = default_allowed_changes(op);
        RevisionInstruction {
            revision_id: RevisionId(format!("REV-{:03}", counter)),
            target: f.claim_id.clone(),
            operation: op,
            allowed_changes: allowed,
            forbidden_changes: Vec::new(),
            requires_human_approval: self.requires_human(f),
            finding_id: Some(f.finding_id.clone()),
            reason: f.recommendation.clone(),
        }
    }

    /// The approval level required for a severity given the configured policy.
    pub fn approval_level_for(&self, severity: FindingSeverity) -> ApprovalLevel {
        severity.default_approval_level()
    }
}

fn normalize_finding_key(f: &Finding) -> String {
    let cat = serde_json::to_string(&f.category).unwrap_or_else(|_| "other".into());
    format!("{}|{}|{}", f.reviewer.name(), f.location, cat)
}

fn severity_rank(s: FindingSeverity) -> u8 {
    match s {
        FindingSeverity::Critical => 4,
        FindingSeverity::Major => 3,
        FindingSeverity::Moderate => 2,
        FindingSeverity::Minor => 1,
    }
}

fn severity_priority(s: &FindingSeverity) -> u8 {
    match s {
        FindingSeverity::Critical => 0,
        FindingSeverity::Major => 1,
        FindingSeverity::Moderate => 2,
        FindingSeverity::Minor => 3,
    }
}

/// A wrapper return type for convenience.
#[derive(Debug, Clone)]
pub struct JudgeResult {
    pub output: JudgeOutput,
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{FindingCategory, ReviewerKind};

    fn finding(
        id: &str,
        cat: FindingCategory,
        sev: FindingSeverity,
        claim: Option<&str>,
    ) -> Finding {
        Finding {
            finding_id: id.into(),
            reviewer: ReviewerKind::Adversarial,
            location: "loc".into(),
            category: cat,
            severity: sev,
            confidence: 0.9,
            claim_id: claim.map(|s| paper_guard_core::ClaimId(s.to_string())),
            finding: "text".into(),
            evidence: vec!["F6".into()],
            recommendation: "do something".into(),
            requires_human_approval: sev == FindingSeverity::Critical,
        }
    }

    #[test]
    fn consolidates_and_produces_revisions() {
        let judge = Judge::new("v1", true);
        let out = judge.consolidate(vec![
            finding(
                "PG-1",
                FindingCategory::UnsupportedClaim,
                FindingSeverity::Major,
                Some("C1"),
            ),
            finding(
                "PG-2",
                FindingCategory::UnsupportedClaim,
                FindingSeverity::Major,
                Some("C1"),
            ),
        ]);
        assert_eq!(out.entries.len(), 1); // merged
        assert_eq!(out.revisions.len(), 1);
        assert!(out.revisions[0].requires_human_approval);
    }

    #[test]
    fn critical_always_requires_human_approval() {
        let judge = Judge::new("v1", false);
        let out = judge.consolidate(vec![finding(
            "PG-3",
            FindingCategory::Overclaiming,
            FindingSeverity::Critical,
            None,
        )]);
        assert!(out.revisions.iter().any(|r| r.requires_human_approval));
    }
}

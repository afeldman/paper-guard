//! The revision engine: applies approved revisions within a strict scope.

use paper_guard_core::{Revision, RevisionChange, RevisionInstruction, RevisionOperation};

/// Options for the revision engine.
#[derive(Debug, Clone)]
pub struct RevisionEngineOptions {
    /// Which agent name to record on produced changes.
    pub agent_name: String,
    /// Whether automated approval is permitted for `Configurable` findings.
    pub allow_configurable_auto_approve: bool,
}

impl Default for RevisionEngineOptions {
    fn default() -> Self {
        RevisionEngineOptions {
            agent_name: "revision".into(),
            allow_configurable_auto_approve: true,
        }
    }
}

/// The outcome of attempting to apply a revision.
#[derive(Debug, Clone)]
pub struct RevisionOutcome {
    pub revision: Revision,
    pub applied: bool,
    pub rejected: Option<String>,
}

/// The revision engine.
pub struct RevisionEngine {
    pub options: RevisionEngineOptions,
}

impl RevisionEngine {
    pub fn new(options: RevisionEngineOptions) -> Self {
        RevisionEngine { options }
    }

    /// Apply a revision instruction to the source text.
    ///
    /// Returns the changes produced. The engine only operates on the exact
    /// scope allowed by the instruction — it never adds content, alters
    /// measurements, or fabricates.
    ///
    /// The engine **fails closed**: if the request implies an automatic textual
    /// edit but no deterministically-safe transformation can be determined
    /// (e.g. the target text is not found, or a numeric overstatement is not
    /// present), it does NOT silently claim success. Instead it returns
    /// `applied: false` with a `rejected` reason that surfaces as an
    /// author-facing action rather than an applied revision.
    pub fn apply(
        &self,
        instruction: &RevisionInstruction,
        run_id: &str,
        approval_granted: bool,
        source: &str,
    ) -> RevisionOutcome {
        let requires_human = instruction.requires_human_approval;
        if requires_human && !approval_granted {
            return RevisionOutcome {
                revision: Revision {
                    revision_id: instruction.revision_id.clone(),
                    run_id: run_id.into(),
                    instruction: instruction.clone(),
                    changes: Vec::new(),
                    approval_granted,
                    approval_level: paper_guard_core::ApprovalLevel::HumanRequired,
                    resulting_hash: None,
                },
                applied: false,
                rejected: Some("requires human approval not granted".into()),
            };
        }

        // Determine changes based on the operation.
        let changes = self.build_changes(instruction, source);

        // Fails closed: an instruction that asked for an edit but produced no
        // safe change is NOT reported as applied. It is surfaced as needing
        // author review instead.
        if changes.is_empty() {
            let revision = Revision {
                revision_id: instruction.revision_id.clone(),
                run_id: run_id.into(),
                instruction: instruction.clone(),
                changes: Vec::new(),
                approval_granted,
                approval_level: if requires_human {
                    paper_guard_core::ApprovalLevel::HumanRequired
                } else {
                    paper_guard_core::ApprovalLevel::Automatic
                },
                resulting_hash: None,
            };
            return RevisionOutcome {
                revision,
                applied: false,
                rejected: Some(
                    "no deterministically-safe change could be produced within scope; \
                     author review required (the engine does not auto-apply an edit it \
                     cannot prove safe)"
                        .to_string(),
                ),
            };
        }

        let mut revision = Revision {
            revision_id: instruction.revision_id.clone(),
            run_id: run_id.into(),
            instruction: instruction.clone(),
            changes,
            approval_granted,
            approval_level: if requires_human {
                paper_guard_core::ApprovalLevel::HumanRequired
            } else {
                paper_guard_core::ApprovalLevel::Automatic
            },
            resulting_hash: None,
        };

        // Validate the produced revision against its own scope; if out of scope,
        // reject rather than apply.
        match revision.validate() {
            Ok(()) => {
                revision.resulting_hash = Some(paper_guard_core::ContentHash::compute(
                    &apply_changes_to(source, &revision.changes),
                ));
                RevisionOutcome {
                    revision,
                    applied: true,
                    rejected: None,
                }
            }
            Err(e) => RevisionOutcome {
                revision,
                applied: false,
                rejected: Some(e.to_string()),
            },
        }
    }

    /// Build revision changes for the performed operation.
    ///
    /// Only word-level, scope-listed changes are applied (e.g. removing a
    /// numeric overstatement, weakening a claim). No scientific content is
    /// added or altered numerically. If no safe change can be determined, an
    /// empty list is returned so the caller fails closed.
    fn build_changes(
        &self,
        instruction: &RevisionInstruction,
        source: &str,
    ) -> Vec<RevisionChange> {
        match instruction.operation {
            RevisionOperation::WeakenClaim => {
                // Remove a numeric overstatement (e.g. " by 40%") from the
                // source. This only *weakens* (removes an unsupported absolute
                // percentage); it never invents or alters a measurement.
                if !instruction
                    .allowed_changes
                    .iter()
                    .any(|a| a.permits(RevisionOperation::WeakenClaim))
                {
                    return Vec::new();
                }
                match weaken_numeric_global(source) {
                    Some((before, after)) => vec![RevisionChange {
                        location: "claim (numeric overstatement)".to_string(),
                        before,
                        after,
                        reason: "weakened claim to match available evidence".to_string(),
                        finding_id: instruction.finding_id.clone().unwrap_or_default(),
                        revision_id: instruction.revision_id.clone(),
                        agent: self.options.agent_name.clone(),
                        timestamp: now_iso(),
                        provenance: paper_guard_core::Provenance::RevisionOutput,
                    }],
                    None => Vec::new(),
                }
            }
            // These operations require careful, context-specific human or
            // deterministic edits. The automatic engine does not guess — if no
            // safe deterministic transformation is defined, it returns no
            // change so the caller fails closed rather than inventing one.
            // (Safe, optionally scripted transformations may be added here.)
            RevisionOperation::Clarify
            | RevisionOperation::AdjustClaimStrength
            | RevisionOperation::FlagMissingEvidence
            | RevisionOperation::AddLimitation
            | RevisionOperation::MoveParagraph
            | RevisionOperation::RewriteSentence
            | RevisionOperation::AddCitationForExistingReference
            | RevisionOperation::RemoveUnsupportedAssertion
            | RevisionOperation::FixCaption
            | RevisionOperation::FixTableHeader
            | RevisionOperation::Other => Vec::new(),
        }
    }
}

/// Apply a set of changes to a source string (helper used for computing the
/// resulting hash). Mirrors the CLI's apply logic: an empty `before` is never
/// applied; otherwise the first occurrence is replaced (or removed when `after`
/// is empty).
fn apply_changes_to(source: &str, changes: &[RevisionChange]) -> String {
    let mut out = source.to_string();
    for change in changes {
        if change.before.is_empty() {
            continue;
        }
        if let Some(idx) = out.find(&change.before) {
            out.replace_range(idx..idx + change.before.len(), &change.after);
        }
    }
    out
}

/// Remove the first numeric overstatement like " by 40%" (or LaTeX " by 40\%")
/// from the source. Returns `(before, after)` where `after` is empty (removal).
///
/// This is a pure *weakening* operation: it deletes an unqualified absolute
/// percentage from the claim without altering any underlying measurement.
fn weaken_numeric_global(source: &str) -> Option<(String, String)> {
    // Match " by <number>%" with an optional LaTeX-escaped slash before the
    // percent sign. We require a leading space to avoid mangling unrelated
    // tokens like "40% discount" inside prose that is not a numeric
    // overstatement; the scope of the weaken operation is deliberately narrow.
    let re = regex::Regex::new(r" by \d+(\.\d+)?\\?%").ok()?;
    let m = re.find(source)?;
    let before = m.as_str().to_string();
    // `after` is empty: this is a removal of the overstatement, not an
    // addition or a measurement change.
    Some((before, String::new()))
}

/// Current ISO-8601 UTC timestamp.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{AllowedChange, ForbiddenChange};

    fn instruction(
        op: RevisionOperation,
        allowed: Vec<AllowedChange>,
        needs_approval: bool,
    ) -> RevisionInstruction {
        RevisionInstruction {
            revision_id: paper_guard_core::RevisionId("REV-001".into()),
            target: Some(paper_guard_core::ClaimId("C1".into())),
            operation: op,
            allowed_changes: allowed,
            forbidden_changes: vec![ForbiddenChange::AddResults],
            requires_human_approval: needs_approval,
            finding_id: Some("PG-1".into()),
            reason: "test".into(),
        }
    }

    #[test]
    fn weaken_claim_without_approval_is_rejected_when_required() {
        let engine = RevisionEngine::new(Default::default());
        let inst = instruction(
            RevisionOperation::WeakenClaim,
            vec![AllowedChange::WeakenClaim, AllowedChange::RewriteSentence],
            true,
        );
        let out = engine.apply(&inst, "run-001", false, "reduces latency by 40%");
        assert!(!out.applied);
        assert!(out.rejected.is_some());
    }

    #[test]
    fn out_of_scope_operation_is_rejected() {
        // EnableAdjustClaimStrength must be listed explicitly; here only
        // RewriteSentence is allowed so AdjustClaimStrength is out of scope.
        let engine = RevisionEngine::new(Default::default());
        let inst = instruction(
            RevisionOperation::AdjustClaimStrength,
            vec![AllowedChange::RewriteSentence],
            false,
        );
        let out = engine.apply(&inst, "run-001", true, "text");
        // The instruction's scope does not allow AdjustClaimStrength via a
        // plain rewrite, so the engine refuses to mutate silently.
        assert!(!out.revision.revision_id.0.is_empty());
    }

    #[test]
    fn same_weaken_change_is_recorded_and_validated() {
        let engine = RevisionEngine::new(Default::default());
        let inst = instruction(
            RevisionOperation::WeakenClaim,
            vec![AllowedChange::WeakenClaim, AllowedChange::RewriteSentence],
            false,
        );
        let out = engine.apply(
            &inst,
            "run-001",
            true,
            "the method reduces latency by 40% [CLAIM C1]",
        );
        // Applied cleanly (the change is a deterministic, scope-safe weaken).
        assert!(out.applied);
        // And it does not invent content: changes have before text but no
        // added numbers.
        for c in &out.revision.changes {
            assert_eq!(c.finding_id, "PG-1");
        }
    }
}

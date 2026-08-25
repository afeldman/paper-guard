//! Shared structured schema for review findings.
//!
//! The model here mirrors the spec's example JSON so that LLM outputs and the
//! review ledger stay stable and versioned.

use paper_guard_core::{
    ClaimId, ContentHash, Finding, FindingCategory as CoreCategory,
    FindingSeverity as CoreSeverity, FindingStatus,
};

/// Re-exported convenience aliases.
pub use paper_guard_core::{FindingCategory, FindingSeverity, ReviewerKind};

/// A lightweight, already-unauthorized-filtered summary of a single historical
/// review-memory entry, used to build the reviewer's memory context block.
///
/// It intentionally omits raw manuscript text and any owner/team identifiers;
/// the pipeline builds these from approved, authorized memory units only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryBrief {
    /// The review-experience category (e.g. `unsupported_claim`).
    pub category: String,
    /// The finding text.
    pub finding: String,
    /// The human decision (`accept`/`reject`/`modified`).
    pub decision: String,
    /// The human's feedback (if any), kept short.
    #[serde(default)]
    pub human_feedback: String,
}

impl MemoryBrief {
    /// Build a brief from a memory unit (used by the pipeline). `finding`,
    /// `decision`, and `feedback` are drawn from the approved memory entry.
    pub fn new(
        category: String,
        finding: String,
        decision: String,
        human_feedback: String,
    ) -> Self {
        MemoryBrief {
            category,
            finding,
            decision,
            human_feedback,
        }
    }
}

/// A parsed finding as it appears in the JSON schema (spec-compatible).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FindingPayload {
    pub schema_version: Option<String>,
    pub finding_id: String,
    pub reviewer: String,
    pub location: String,
    pub category: String,
    pub severity: String,
    pub confidence: f32,
    #[serde(default)]
    pub claim_id: Option<String>,
    pub finding: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub recommendation: String,
    #[serde(default)]
    pub requires_human_approval: bool,
}

impl FindingPayload {
    /// Convert this payload into a validated domain [`Finding`].
    pub fn into_finding(self) -> anyhow::Result<Finding> {
        finding_from_payload(self)
    }
}

/// Convert a domain [`Finding`] into its JSON payload form.
pub fn finding_to_payload(f: &Finding) -> FindingPayload {
    FindingPayload {
        schema_version: Some("1.0".to_string()),
        finding_id: f.finding_id.clone(),
        reviewer: f.reviewer.name().to_string(),
        location: f.location.clone(),
        category: serde_json::to_value(f.category)
            .map(|v| v.as_str().unwrap_or("other").to_string())
            .unwrap_or_else(|_| "other".to_string()),
        severity: serde_json::to_value(f.severity)
            .map(|v| v.as_str().unwrap_or("minor").to_string())
            .unwrap_or_else(|_| "minor".to_string()),
        confidence: f.confidence,
        claim_id: f.claim_id.as_ref().map(|c| c.0.clone()),
        finding: f.finding.clone(),
        evidence: f.evidence.clone(),
        recommendation: f.recommendation.clone(),
        requires_human_approval: f.requires_human_approval,
    }
}

/// Parse a domain [`Finding`] from its JSON payload, mapping the string keys
/// to typed enums. Unknown categories/severities map to safe defaults so a
/// malformed agent response degrades gracefully instead of aborting the run.
pub fn finding_from_payload(p: FindingPayload) -> anyhow::Result<Finding> {
    let reviewer = parse_reviewer(&p.reviewer);
    let category = parse_category(&p.category);
    let severity = parse_severity(&p.severity);
    let claim_id = p.claim_id.map(ClaimId);
    let confidence = p.confidence.clamp(0.0, 1.0);
    let requires_human = p.requires_human_approval
        || severity == CoreSeverity::Critical
        || severity == CoreSeverity::Major;

    let f = Finding {
        finding_id: p.finding_id,
        reviewer,
        location: p.location,
        category,
        severity,
        confidence,
        claim_id,
        finding: p.finding,
        evidence: p.evidence,
        recommendation: p.recommendation,
        requires_human_approval: requires_human,
    };
    f.validate()?;
    Ok(f)
}

fn parse_reviewer(s: &str) -> ReviewerKind {
    match s.trim().to_ascii_lowercase().as_str() {
        "scientific" => ReviewerKind::Scientific,
        "adversarial" => ReviewerKind::Adversarial,
        "evidence" => ReviewerKind::Evidence,
        "references" => ReviewerKind::References,
        "figures" => ReviewerKind::Figures,
        "judge" => ReviewerKind::Judge,
        _ => ReviewerKind::Judge,
    }
}

fn parse_category(s: &str) -> CoreCategory {
    use CoreCategory::*;
    match s.trim().to_ascii_lowercase().as_str() {
        "unsupported_claim" => UnsupportedClaim,
        "weak_evidence" => WeakEvidence,
        "overclaiming" => Overclaiming,
        "contradiction" => Contradiction,
        "missing_control" => MissingControl,
        "confounder" => Confounder,
        "bias" => Bias,
        "statistical_weakness" => StatisticalWeakness,
        "leakage" => Leakage,
        "reproducibility" => Reproducibility,
        "logical_gap" => LogicalGap,
        "interpretation_error" => InterpretationError,
        "limitation" => Limitation,
        "reference_error" => ReferenceError,
        "hallucinated_reference" => HallucinatedReference,
        "missing_reference" => MissingReference,
        "citation_mismatch" => CitationMismatch,
        "figure_issue" => FigureIssue,
        "table_issue" => TableIssue,
        "inconsistency" => Inconsistency,
        "methodology" => Methodology,
        "prompt_injection" => PromptInjection,
        "refactored_logic" => RefactoredLogic,
        _ => Other,
    }
}

fn parse_severity(s: &str) -> CoreSeverity {
    use CoreSeverity::*;
    match s.trim().to_ascii_uppercase().as_str() {
        "CRITICAL" => Critical,
        "MAJOR" => Major,
        "MODERATE" => Moderate,
        "MINOR" => Minor,
        _ => Minor,
    }
}

/// A stable hash to include in ledger artifacts (kept for reproducibility).
#[allow(dead_code)]
pub fn schema_hash() -> ContentHash {
    ContentHash::compute(&paper_guard_core::SCHEMA_VERSION.to_string())
}

/// The full set of status labels, exported for completeness.
#[allow(dead_code)]
pub const FINDING_STATUSES: &[FindingStatus] = &[
    FindingStatus::Open,
    FindingStatus::Acknowledged,
    FindingStatus::Approved,
    FindingStatus::Revised,
    FindingStatus::Resolved,
    FindingStatus::Rejected,
    FindingStatus::Regressed,
];

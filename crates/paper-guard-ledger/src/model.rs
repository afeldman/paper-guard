//! Ledger data model.

use paper_guard_core::{ClaimId, ContentHash, FindingSeverity, FindingStatus};
use serde::{Deserialize, Serialize};

/// The lifecycle status of a RunRecord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// A run was started but not completed (crash / interruption).
    InProgress,
    /// The full pipeline completed.
    Completed,
    /// The pipeline failed partway through.
    Failed,
}

/// The outcome of a single agent in a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutcome {
    pub agent: String,
    pub status: String,
    pub error: Option<String>,
    /// Summary counts (e.g. number of findings) when successful.
    #[serde(default)]
    pub finding_count: usize,
    /// Provider usage metadata (tokens, provider, model) when available.
    ///
    /// This is generic — it never couples the ledger to a specific provider
    /// (OpenAI, Mammoth.ai, etc.); it merely records what an [`crate::LlmProvider`]
    /// reported. Secrets are never stored here.
    #[serde(default)]
    pub provider_usage: Option<ProviderUsage>,
}

/// Generic provider usage metadata recorded in the ledger for auditability.
///
/// It is intentionally provider-agnostic: any OpenAI-compatible endpoint (or
/// the mock) can populate it. No API key or secret is ever stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    /// The provider kind that produced this agent's result (e.g. "mock",
    /// "openai_compatible").
    pub provider: String,
    /// The model identifier used for this agent.
    pub model: String,
    /// Prompt (input) tokens, if reported.
    pub input_tokens: u32,
    /// Completion (output) tokens, if reported.
    pub output_tokens: u32,
}

impl ProviderUsage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// A tracked finding with its lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingRecord {
    pub finding_id: String,
    pub reviewer: String,
    pub location: String,
    pub category: String,
    pub severity: FindingSeverity,
    pub confidence: f32,
    pub claim_id: Option<ClaimId>,
    pub finding: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub recommendation: String,
    pub status: FindingStatus,
    /// The run id where this finding was originally opened.
    pub opened_in: String,
    /// The run id that resolved/regressed it (if any).
    #[serde(default)]
    pub resolved_in: Option<String>,
}

impl FindingRecord {
    /// Build a record from a payload-like set of fields with an initial state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        finding_id: String,
        reviewer: String,
        location: String,
        category: String,
        severity: FindingSeverity,
        confidence: f32,
        claim_id: Option<ClaimId>,
        finding: String,
        evidence: Vec<String>,
        recommendation: String,
        opened_in: String,
    ) -> Self {
        FindingRecord {
            finding_id,
            reviewer,
            location,
            category,
            severity,
            confidence,
            claim_id,
            finding,
            evidence,
            recommendation,
            // A freshly opened finding always starts OPEN; the severity no
            // longer branches (the branch was a no-op) and any severity is
            // surfaced the same way so the judge's required-approval decision
            // drives next steps.
            status: FindingStatus::Open,
            opened_in,
            resolved_in: None,
        }
    }
}

/// A judged finding entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgedRecord {
    pub finding_id: String,
    pub status: FindingStatus,
    pub severity: FindingSeverity,
    pub priority: String,
    pub action: String,
    pub requires_human_approval: bool,
    pub revision_id: Option<String>,
}

/// A validation outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRecord {
    pub stage: String,
    pub passed: bool,
    #[serde(default)]
    pub issues: Vec<String>,
}

/// A single run record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema_version: String,
    pub run_id: String,
    #[serde(default)]
    pub parent_run: Option<String>,
    pub input_hash: ContentHash,
    pub source_format: String,
    pub parser_version: String,
    pub paper_guard_version: String,
    pub configuration_hash: ContentHash,
    pub model_configuration: String,
    #[serde(default)]
    pub prompt_version: String,
    pub reviewer_results: Vec<AgentOutcome>,
    #[serde(default)]
    pub findings: Vec<FindingRecord>,
    #[serde(default)]
    pub judge_results: Vec<JudgedRecord>,
    #[serde(default)]
    pub revision_results: Vec<String>,
    #[serde(default)]
    pub validation_results: Vec<ValidationRecord>,
    pub timestamp: String,
    pub status: RunStatus,
}

impl RunRecord {
    /// Create a shell record for a new run.
    #[allow(clippy::too_many_arguments)]
    pub fn shell(
        run_id: String,
        parent_run: Option<String>,
        input_hash: ContentHash,
        source_format: &str,
        parser_version: &str,
        paper_guard_version: &str,
        configuration_hash: ContentHash,
        model_configuration: &str,
        prompt_version: &str,
        timestamp: &str,
    ) -> Self {
        RunRecord {
            schema_version: "1.0".to_string(),
            run_id,
            parent_run,
            input_hash,
            source_format: source_format.to_string(),
            parser_version: parser_version.to_string(),
            paper_guard_version: paper_guard_version.to_string(),
            configuration_hash,
            model_configuration: model_configuration.to_string(),
            prompt_version: prompt_version.to_string(),
            reviewer_results: Vec::new(),
            findings: Vec::new(),
            judge_results: Vec::new(),
            revision_results: Vec::new(),
            validation_results: Vec::new(),
            timestamp: timestamp.to_string(),
            status: RunStatus::InProgress,
        }
    }

    /// Mark the run completed.
    pub fn mark_completed(&mut self) {
        self.status = RunStatus::Completed;
    }

    /// Mark the run failed.
    pub fn mark_failed(&mut self) {
        self.status = RunStatus::Failed;
    }

    /// Whether this run's input matches another by content hash.
    #[allow(dead_code)]
    pub fn same_input_as(&self, hash: &ContentHash) -> bool {
        self.input_hash == *hash
    }
}

/// Instantiate an empty FindingRecord in tests without heavy deps.
#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{ContentHash, EvidenceState};

    #[test]
    fn finding_record_status_defaults_to_open() {
        let r = FindingRecord::new(
            "PG-1".into(),
            "adversarial".into(),
            "loc".into(),
            "unsupported_claim".into(),
            FindingSeverity::Major,
            0.9,
            None,
            "text".into(),
            vec![],
            "rec".into(),
            "run-001".into(),
        );
        assert_eq!(r.status, FindingStatus::Open);
        assert_eq!(r.opened_in, "run-001");
    }

    #[test]
    fn run_shell_marks_same_input() {
        let hash = ContentHash::compute(&"abc");
        let run = RunRecord::shell(
            "run-001".into(),
            None,
            hash.clone(),
            "latex",
            "0.1.0",
            "0.1.0",
            ContentHash::default(),
            "{}",
            "v1",
            "2026-01-01T00:00:00Z",
        );
        assert!(run.same_input_as(&hash));
        assert_eq!(run.status, RunStatus::InProgress);
        let _ = EvidenceState::default();
    }
}

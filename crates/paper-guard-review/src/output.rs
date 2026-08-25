//! Reviewer output containers.
//!
//! A reviewer returns a [`ReviewerOutput`] which wraps the raw LLM artifact
//! (for the ledger) plus the parsed findings. Structural validation is strict:
//! a response that cannot be parsed unambiguously into findings is rejected as
//! [`ReviewOutputError::Invalid`] (surfaced as `REVIEWER_OUTPUT_INVALID`),
//! rather than silently degraded into "empty", which could hide a malformed or
//! partially-fabricated reviewer reply.

use paper_guard_core::Finding;

use crate::schema::FindingPayload;

/// The sentinel reason string recorded for a reviewer whose output could not
/// be validated into structured findings.
pub const REVIEWER_OUTPUT_INVALID: &str = "REVIEWER_OUTPUT_INVALID";

/// Errors produced while turning raw reviewer output into domain findings.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReviewOutputError {
    /// The raw response was not parseable into a findings structure.
    #[error("REVIEWER_OUTPUT_INVALID: {0}")]
    Invalid(String),
}

/// The structured output of a single reviewer run.
#[derive(Debug, Clone)]
pub struct ReviewerOutput {
    /// The reviewer's kind name.
    pub reviewer: String,
    /// The raw LLM response text (zero-copy preserved as a reproducibility
    /// artifact).
    pub raw_response: String,
    /// Findings parsed from the response.
    pub findings: Vec<Finding>,
    /// The request's content hash (for reproducibility).
    pub request_hash: Option<String>,
    /// Report of the provider usage (tokens) for this agent, if the provider
    /// reported any. Intentionally provider-agnostic.
    pub usage: Option<paper_guard_llm::LlmUsage>,
}

impl ReviewerOutput {
    /// Build a reviewer output that failed gracefully: an error message is
    /// preserved but no fabricated findings are produced.
    pub fn failed(
        reviewer: &str,
        request_hash: Option<String>,
        error: &dyn std::fmt::Display,
    ) -> Self {
        ReviewerOutput {
            reviewer: reviewer.to_string(),
            raw_response: format!("AGENT_FAILED: {error}"),
            findings: Vec::new(),
            request_hash,
            usage: None,
        }
    }

    /// Build an output from raw response text, deferring finding parsing.
    pub fn from_raw(
        reviewer: &str,
        raw_response: String,
        request_hash: Option<String>,
    ) -> Self {
        ReviewerOutput {
            reviewer: reviewer.to_string(),
            raw_response,
            findings: Vec::new(),
            request_hash,
            usage: None,
        }
    }

    /// Parse findings from a payload string.
    pub fn parse_findings(mut self, payload: &str) -> anyhow::Result<Self> {
        self.findings = resolve_findings(payload)?;
        Ok(self)
    }

    /// Attach provider usage metadata (tokens) reported for this agent.
    pub fn with_usage(mut self, usage: Option<paper_guard_llm::LlmUsage>) -> Self {
        self.usage = usage;
        self
    }
}

/// Strictly parse a list of findings from a JSON payload.
///
/// Fail-closed behaviour: only a well-formed findings array/object is accepted.
/// A payload that is empty, a bare non-array value, or unparseable JSON yields
/// a [`ReviewOutputError::Invalid`] rather than an empty finding list — the
/// alternative would let a malformed or partially-fabricated reviewer reply
/// silently disappear.
pub fn resolve_findings(payload: &str) -> anyhow::Result<Vec<Finding>> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return Err(ReviewOutputError::Invalid(
            "empty reviewer response (no JSON)".to_string(),
        )
        .into());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
        ReviewOutputError::Invalid(format!(
            "response is not valid JSON: {e} (raw: {})",
            snippet(trimmed)
        ))
    })?;
    match value {
        serde_json::Value::Array(items) => {
            let mut out = Vec::new();
            for (idx, item) in items.into_iter().enumerate() {
                let raw = serde_json::to_string(&item).unwrap_or_else(|_| "<unprintable>".into());
                let p: FindingPayload = serde_json::from_value(item).map_err(|e| {
                    ReviewOutputError::Invalid(format!(
                        "finding #{idx} does not match the schema: {e} (raw: {})",
                        snippet(&raw)
                    ))
                })?;
                out.push(p.into_finding().map_err(|e| {
                    ReviewOutputError::Invalid(format!("finding #{idx} failed validation: {e}"))
                })?);
            }
            Ok(out)
        }
        serde_json::Value::Object(_) => {
            let p: FindingPayload = serde_json::from_value(value).map_err(|e| {
                ReviewOutputError::Invalid(format!(
                    "single finding does not match the schema: {e}"
                ))
            })?;
            Ok(vec![p.into_finding().map_err(|e| {
                ReviewOutputError::Invalid(format!("single finding failed validation: {e}"))
            })?])
        }
        other => Err(ReviewOutputError::Invalid(format!(
            "response is {} not a findings array/object (raw: {})",
            json_kind(&other),
            snippet(trimmed)
        ))
        .into()),
    }
}

/// A short, whitespace-collapsed excerpt of a payload for error messages.
fn snippet(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 160 {
        format!("{}…", &collapsed[..160])
    } else {
        collapsed
    }
}

fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

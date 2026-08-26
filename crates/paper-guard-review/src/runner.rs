//! The parallel reviewer runner.
//!
//! Independent reviewers run concurrently. A failure in one reviewer produces
//! a `failed` [`ReviewerOutput`] (recorded as a failed-agent status in the
//! ledger) but does not abort the run.

use std::sync::Arc;

use paper_guard_core::Finding;
use tokio::sync::Semaphore;

use crate::output::ReviewerOutput;
use crate::reviewer::{Reviewer, ReviewerContext};
use crate::schema::{FindingPayload, ReviewerKind};

/// A per-agent run outcome.
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub agent: ReviewerKind,
    pub status: AgentStatus,
    pub output: Option<ReviewerOutput>,
    pub error: Option<String>,
}

/// A status for a single agent in the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Success,
    Failed,
    Disabled,
}

/// A boxed reviewer along with its settings-provider mapping.
type BoxedReviewer = Box<dyn Reviewer>;

/// The review runner: runs a set of reviewers in parallel via tokio.
pub struct ReviewRunner {
    max_concurrent: usize,
}

impl ReviewRunner {
    pub fn new(max_concurrent: usize) -> Self {
        ReviewRunner {
            max_concurrent: if max_concurrent == 0 {
                4
            } else {
                max_concurrent
            },
        }
    }

    /// Run all provided reviewers concurrently over the same context.
    pub async fn run(
        &self,
        ctx: &ReviewerContext,
        reviewers: Vec<BoxedReviewer>,
        provider: Arc<dyn paper_guard_llm::LlmProvider>,
    ) -> Vec<AgentRunResult> {
        if reviewers.is_empty() {
            return Vec::new();
        }
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut handles = Vec::new();
        for reviewer in reviewers {
            let ctx = ctx.clone();
            let semaphore = semaphore.clone();
            let provider = provider.clone();
            // Capture the reviewer's identity up front so that a panicked task
            // can still be attributed to the correct agent (not mislabelled as
            // the judge or as some other reviewer).
            let agent = reviewer.kind();
            let expected_agent = agent;
            handles.push((
                expected_agent,
                tokio::spawn(async move {
                    // Take a permit to bound concurrency. If disabled, mark as such.
                    let _permit = semaphore.acquire().await;
                    run_one(reviewer, ctx, provider, agent).await
                }),
            ));
        }
        let mut results = Vec::with_capacity(handles.len());
        for (expected_agent, h) in handles {
            match h.await {
                Ok(res) => results.push(res),
                Err(e) => {
                    // Task panicked — treat as a failed agent, never fabricate,
                    // and preserve WHICH agent failed.
                    results.push(AgentRunResult {
                        agent: expected_agent,
                        status: AgentStatus::Failed,
                        output: None,
                        error: Some(format!("agent task panicked: {e}")),
                    });
                }
            }
        }
        results
    }
}

/// Run a single reviewer and classify the outcome.
async fn run_one(
    reviewer: BoxedReviewer,
    ctx: ReviewerContext,
    provider: Arc<dyn paper_guard_llm::LlmProvider>,
    agent: ReviewerKind,
) -> AgentRunResult {
    let enabled = reviewer.settings().enabled;
    if !enabled {
        return AgentRunResult {
            agent,
            status: AgentStatus::Disabled,
            output: None,
            error: None,
        };
    }
    match reviewer.run(&ctx, provider.as_ref()).await {
        Ok(output) => AgentRunResult {
            agent,
            status: AgentStatus::Success,
            output: Some(output),
            error: None,
        },
        Err(e) => {
            // Failed agent: no findings, but the failure is surfaced, not
            // masked with fabricated findings.
            AgentRunResult {
                agent,
                status: AgentStatus::Failed,
                output: Some(ReviewerOutput::failed(agent.name(), None, &e)),
                error: Some(e.to_string()),
            }
        }
    }
}

/// Collect all findings from successful agent results.
pub fn collect_findings(results: &[AgentRunResult]) -> Vec<Finding> {
    results
        .iter()
        .filter(|r| r.status == AgentStatus::Success)
        .filter_map(|r| r.output.as_ref())
        .flat_map(|o| o.findings.clone())
        .collect()
}

/// Convert findings into payloads (for ledger serialization).
pub fn findings_to_payloads(findings: &[Finding]) -> Vec<FindingPayload> {
    findings.iter().map(crate::finding_to_payload).collect()
}

/// Convenience: build a provider-pinned set of reviewer kinds.
pub fn reviewable_kinds() -> &'static [ReviewerKind] {
    &[
        ReviewerKind::Scientific,
        ReviewerKind::Adversarial,
        ReviewerKind::Evidence,
        ReviewerKind::References,
        ReviewerKind::Figures,
    ]
}

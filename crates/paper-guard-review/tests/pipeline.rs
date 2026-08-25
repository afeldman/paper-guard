//! Integration tests: LaTeX -> canonical -> review -> judge -> revision.

use paper_guard_core::RevisionOperation;
use paper_guard_llm::{MockLlmScenario, MockProvider};
use paper_guard_parser::Parser;
use paper_guard_review::{
    collect_findings, ReviewRunner, ReviewerContext, ReviewerKind, ReviewerSettings,
};

fn sample_latex() -> &'static str {
    r#"\documentclass{article}
\title{T}
\begin{document}
\section{Intro}
We show that our method significantly reduces latency by 40\%.

There is no experiment or dataset to back this claim.
\end{document}"#
}

async fn build_review_ctx() -> ReviewerContext {
    let parser = paper_guard_parser::LatexParser;
    let parsed = parser
        .parse("main.tex", sample_latex().as_bytes())
        .await
        .unwrap();
    ReviewerContext {
        document: parsed.document,
        prompt_version: "v1".into(),
        run_id: "run-001".into(),
        memory_context: String::new(),
    }
}

#[tokio::test]
async fn full_pipeline_with_fixture_finding() {
    // The mock produces a finding only when the prompt shows the strong claim.
    let scenario = MockLlmScenario::new("adversarial").on(
        "significantly reduces latency",
        r#"[{"finding_id":"PG-0001","reviewer":"adversarial","location":"section_1.paragraph_1","category":"overclaiming","severity":"major","confidence":0.9,"claim_id":"C1","finding":"strength exceeds the evidence","evidence":[],"recommendation":"weaken the claim to match the available evidence","requires_human_approval":false}]"#,
    );
    let provider: std::sync::Arc<dyn paper_guard_llm::LlmProvider> =
        std::sync::Arc::new(MockProvider::new("mock", scenario));

    let ctx = build_review_ctx().await;
    let reviewers: Vec<Box<dyn paper_guard_review::Reviewer>> =
        vec![Box::new(paper_guard_review::AdversarialReviewer {
            settings: ReviewerSettings::default_with_model(ReviewerKind::Adversarial, "mock"),
        })];
    let runner = ReviewRunner::new(4);
    let results = runner.run(&ctx, reviewers, provider).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, paper_guard_review::AgentStatus::Success);

    let findings = collect_findings(&results);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].finding_id, "PG-0001");

    // Integrity: the finding must not reference invented evidence.
    assert!(findings[0].evidence.is_empty());

    // Judge consolidates into a revision instruction that weakens the claim.
    let judge = paper_guard_review::Judge::new("v1", true);
    let judged = judge.consolidate(findings);
    assert!(!judged.revisions.is_empty());
    assert_eq!(
        judged.revisions[0].operation,
        RevisionOperation::WeakenClaim
    );

    // The revision agent weakens the claim deterministically without adding
    // any content.
    let engine = paper_guard_agents::RevisionEngine::new(Default::default());
    let out = engine.apply(&judged.revisions[0], "run-001", true, sample_latex());
    assert!(out.applied);
    // No forbidden add-content change is present: before is a " by N%" phrase
    // and after is empty (weakening, not inventing).
    assert!(out
        .revision
        .changes
        .iter()
        .all(|c| c.after.is_empty() || c.before.contains('%')));

    // The applied change must actually mutate the source text (removing the
    // numeric overstatement), matching how the CLI applies a revision. A change
    // that is recorded but never reflected in the source would break the audit
    // trail: the ledger would claim a revision happened when the text is
    // unchanged.
    let before_text = sample_latex();
    let after_text = apply_as_cli(before_text, &out);
    assert!(
        after_text != before_text,
        "the weaken revision must materially change the source text"
    );
    assert!(
        !after_text.contains("by 40"),
        "the numeric overstatement should be removed from the re-rendered source"
    );
    // The change is tagged as machine-produced revision output.
    for c in &out.revision.changes {
        assert_eq!(c.provenance, paper_guard_core::Provenance::RevisionOutput);
    }
}

/// Replicate the CLI's `apply_changes` behaviour: replace/remove the first
/// occurrence of each change's `before` with `after` (empty = removal).
fn apply_as_cli(source: &str, out: &paper_guard_agents::RevisionOutcome) -> String {
    let mut s = source.to_string();
    for c in &out.revision.changes {
        if c.before.is_empty() {
            continue;
        }
        if let Some(idx) = s.find(&c.before) {
            s.replace_range(idx..idx + c.before.len(), &c.after);
        }
    }
    s
}

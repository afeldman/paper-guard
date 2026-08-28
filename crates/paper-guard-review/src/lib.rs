//! # Paper Guard Review
//!
//! The reviewer and judge agents. Independent reviewers examine the canonical
//! document through an [`LlmProvider`] and return structured [`Finding`]s.
//! Findings travel in a shared JSON schema (see [`schema`]) that mirrors the
//! spec's example payload. The judge then consolidates the findings.
//!
//! Reviewers run concurrently; a failure in one reviewer does not abort the
//! run — it is tracked as a failed agent status in the ledger.

pub mod judge;
pub mod output;
pub mod reviewer;
pub mod reviewers;
pub mod runner;
pub mod schema;

pub use judge::{Judge, JudgeAction, JudgeOutput, JudgeResult};
pub use output::{resolve_findings, ReviewOutputError, ReviewerOutput, REVIEWER_OUTPUT_INVALID};
pub use reviewer::{
    render_document_for_prompt, render_memory_context, AdversarialReviewer, EvidenceReviewer,
    FigureReviewer, ReferenceReviewer, Reviewer, ReviewerContext, ReviewerSettings,
    ScientificReviewer, MEMORY_UNTRUSTED_PREAMBLE,
};
pub use reviewers::INTEGRITY_PREAMBLE;
pub use runner::{collect_findings, AgentRunResult, AgentStatus, ReviewRunner};
pub use schema::{
    finding_from_payload, finding_schema_value, finding_to_payload, reviewer_schema_spec,
    FindingCategory, FindingPayload, FindingSeverity, MemoryBrief, ReviewerKind,
    REVIEWER_SCHEMA_NAME,
};

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{DocumentMeta, Paragraph, ParagraphId, Section, SectionId};
    use paper_guard_llm::{MockLlmScenario, MockProvider};

    fn tiny_doc() -> paper_guard_core::Document {
        paper_guard_core::Document {
            document_id: "doc-test".into(),
            meta: DocumentMeta {
                title: Some("Test".into()),
                authors: vec![],
                abstract_text: None,
                source_format: "latex".into(),
                source_file: "main.tex".into(),
            },
            sections: vec![Section {
                id: SectionId("section_1".into()),
                title: "Intro".into(),
                paragraphs: vec![Paragraph {
                    id: ParagraphId("section_1.paragraph_1".into()),
                    text: "We show the method reduces latency. INSUFFICIENT_EVIDENCE".into(),
                }],
            }],
            bibliography: vec![],
            citations: vec![],
            claims: vec![],
            evidence: vec![],
            results: vec![],
            methods: vec![],
            figures: vec![],
            tables: vec![],
            equations: vec![],
            source_hash: Default::default(),
        }
    }

    #[tokio::test]
    async fn mock_reviewer_and_judge_end_to_end() {
        // A mock that reacts to the INSUFFICIENT_EVIDENCE marker and returns a
        // finding in the shared schema.
        let scenario = MockLlmScenario::new("adversarial").on(
            "INSUFFICIENT_EVIDENCE",
            r#"[{"finding_id":"PG-0001","reviewer":"adversarial","location":"section_1.paragraph_1","category":"unsupported_claim","severity":"major","confidence":0.9,"finding":"claim lacks evidence","evidence":[],"recommendation":"weaken claim","requires_human_approval":true}]"#,
        );
        let provider: std::sync::Arc<dyn paper_guard_llm::LlmProvider> =
            std::sync::Arc::new(MockProvider::new("model-x", scenario));

        let ctx = ReviewerContext {
            document: tiny_doc(),
            prompt_version: "v1".into(),
            run_id: "run-001".into(),
            memory_context: String::new(),
        };

        let reviewers: Vec<Box<dyn Reviewer>> = vec![Box::new(AdversarialReviewer {
            settings: ReviewerSettings::default_with_model(ReviewerKind::Adversarial, "model-x"),
        })];

        let runner = ReviewRunner::new(4);
        let results = runner.run(&ctx, reviewers, provider).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, AgentStatus::Success);

        let findings = collect_findings(&results);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_id, "PG-0001");
        // Critical integrity: the finding references NO invented evidence.
        assert!(findings[0].evidence.is_empty());

        let judge = Judge::new("v1", true);
        let out = judge.consolidate(findings);
        assert!(!out.revisions.is_empty());
        assert!(out.revisions[0].requires_human_approval);
    }

    #[tokio::test]
    async fn failed_agent_does_not_fabricate() {
        // A reviewer pointing at a model with no scenario and no triggers
        // returns "[]" (fallback) => no fabricated findings.
        let scenario = MockLlmScenario::new("empty").fallback("[]");
        let provider: std::sync::Arc<dyn paper_guard_llm::LlmProvider> =
            std::sync::Arc::new(MockProvider::new("empty-model", scenario));

        let ctx = ReviewerContext {
            document: tiny_doc(),
            prompt_version: "v1".into(),
            run_id: "run-002".into(),
            memory_context: String::new(),
        };
        let reviewers: Vec<Box<dyn Reviewer>> = vec![Box::new(ScientificReviewer {
            settings: ReviewerSettings::default_with_model(ReviewerKind::Scientific, "empty-model"),
        })];
        let runner = ReviewRunner::new(4);
        let results = runner.run(&ctx, reviewers, provider).await;
        let findings = collect_findings(&results);
        assert!(findings.is_empty());
    }
}

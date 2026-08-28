//! End-to-end structured-output tests for the reviewer layer.
//!
//! These verify the documented contract:
//!
//! * The reviewer attaches a JSON Schema (derived from the strongly-typed
//!   [`FindingPayload`]) to every `LlmRequest`. This is a *transport* concern:
//!   it never weakens the reviewer-side domain validation.
//! * A valid reviewer response matching the schema reaches the normal
//!   reviewer/domain validation path and becomes a proper [`Finding`].
//! * Malformed / semantically-invalid reviewer output (e.g. `"High"` for the
//!   numeric `confidence`, or a missing required field) is rejected as
//!   `REVIEWER_OUTPUT_INVALID` and never coerced or silently repaired.
//!
//! All tests are offline.

use paper_guard_core::{ClaimId, DocumentMeta, Paragraph, ParagraphId, Section, SectionId};
use paper_guard_llm::{MockLlmScenario, MockProvider};
use paper_guard_review::{
    resolve_findings, reviewer_schema_spec, AdversarialReviewer, AgentStatus, FindingPayload,
    ReviewRunner, Reviewer, ReviewerContext, ReviewerKind, ReviewerSettings,
};

fn tiny_doc() -> paper_guard_core::Document {
    paper_guard_core::Document {
        document_id: "doc-e2e".into(),
        meta: DocumentMeta {
            title: Some("T".into()),
            authors: vec![],
            abstract_text: None,
            source_format: "latex".into(),
            source_file: "main.tex".into(),
        },
        sections: vec![Section {
            id: SectionId("section_1".into()),
            title: "Methods".into(),
            paragraphs: vec![Paragraph {
                id: ParagraphId("section_1.paragraph_1".into()),
                text: "We measured the effect size via a controlled experiment.".into(),
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

fn valid_finding_json() -> &'static str {
    // Matches the FindingPayload schema: numeric confidence, non-empty ids.
    r#"[{
        "schema_version": "1.0",
        "finding_id": "PG-777",
        "reviewer": "adversarial",
        "location": "section_sec-1.paragraph_p-1",
        "category": "weak_evidence",
        "severity": "moderate",
        "confidence": 0.75,
        "claim_id": "CL-1",
        "finding": "The conclusion overstates the effect size relative to the presented data.",
        "evidence": ["p-1"],
        "recommendation": "Add a clear limitation and temper the claim.",
        "requires_human_approval": false
    }]"#
}

fn ctx() -> ReviewerContext {
    ReviewerContext {
        document: tiny_doc(),
        prompt_version: "v1".into(),
        run_id: "run-e2e".into(),
        memory_context: String::new(),
    }
}

fn adversarial() -> Box<dyn Reviewer> {
    Box::new(AdversarialReviewer {
        settings: ReviewerSettings::default_with_model(ReviewerKind::Adversarial, "model-a"),
    })
}

#[test]
fn reviewer_schema_derives_numeric_confidence() {
    // The schema that the reviewer attaches to the request must constrain
    // `confidence` to a JSON number (matching the `f32` domain field), not a
    // string like `"High"`.
    let spec = reviewer_schema_spec();
    // Array wrapper with the finding schema as its items.
    let schema = &spec.schema;
    assert_eq!(schema["type"], serde_json::json!("array"));
    let item = &schema["items"];
    assert_eq!(item["type"], serde_json::json!("object"));
    assert_eq!(
        item["properties"]["confidence"]["type"],
        serde_json::json!("number")
    );
    let required = item["required"].as_array().expect("required is an array");
    assert!(
        required.iter().any(|v| v == "confidence"),
        "confidence must be required"
    );
    assert!(
        required.iter().any(|v| v == "finding_id"),
        "finding_id must be required"
    );
    // The name is stable and used as the schema name in the transport request.
    assert_eq!(spec.name, paper_guard_review::REVIEWER_SCHEMA_NAME);
    // Strict by default.
    assert!(spec.strict);
}

#[tokio::test]
async fn valid_response_reaches_domain_validation() {
    // A reviewer whose model returns a schema-conforming findings array must
    // reach the reviewer/domain validation path and yield a real Finding.
    let scenario = MockLlmScenario::new("valid").fallback(valid_finding_json().to_string());
    let provider: std::sync::Arc<dyn paper_guard_llm::LlmProvider> =
        std::sync::Arc::new(MockProvider::new("model-a", scenario));
    let runner = ReviewRunner::new(4);
    let results = runner.run(&ctx(), vec![adversarial()], provider).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, AgentStatus::Success);
    let out = results[0].output.as_ref().expect("successful output");
    assert_eq!(out.findings.len(), 1);
    let f = &out.findings[0];
    assert_eq!(f.finding_id, "PG-777");
    assert!((f.confidence - 0.75).abs() < f64::EPSILON as f32);
    assert!(f.evidence.contains(&"p-1".to_string()));
    assert_eq!(f.claim_id, Some(ClaimId("CL-1".to_string())));
}

#[tokio::test]
async fn string_confidence_is_rejected_as_invalid() {
    // The exact failure observed in the first LM Studio E2E run: the model
    // returned `"confidence": "High"` (a string). Paper Guard must NOT coerce
    // it; it must fail closed as REVIEWER_OUTPUT_INVALID.
    let payload = r#"[{
        "finding_id": "PG-BAD",
        "reviewer": "adversarial",
        "location": "x",
        "category": "weak_evidence",
        "severity": "moderate",
        "confidence": "High",
        "finding": "some claim",
        "recommendation": "fix",
        "requires_human_approval": false
    }]"#;
    let err = resolve_findings(payload).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("REVIEWER_OUTPUT_INVALID"), "actual: {msg}");
    assert!(
        msg.contains("confidence"),
        "should mention the offending field: {msg}"
    );
}

#[tokio::test]
async fn missing_required_field_is_rejected_as_invalid() {
    // Missing `finding_id` (a required field per the schema) must be rejected.
    let payload = r#"[{
        "reviewer": "adversarial",
        "location": "x",
        "category": "weak_evidence",
        "severity": "moderate",
        "confidence": 0.5,
        "finding": "some claim",
        "recommendation": "fix"
    }]"#;
    let err = resolve_findings(payload).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("REVIEWER_OUTPUT_INVALID"), "actual: {msg}");
    assert!(
        msg.contains("finding_id"),
        "should mention the missing field: {msg}"
    );
}

#[tokio::test]
async fn finding_payload_parses_and_round_trips() {
    // A schema-conforming payload deserializes into the domain FindingPayload
    // and converts to a validated Finding.
    // The reviewer output is a JSON array of FindingPayload; deserialize the
    // items (as resolve_findings does) and convert to validated Findings.
    let items: Vec<FindingPayload> = serde_json::from_str(valid_finding_json()).unwrap();
    assert_eq!(items.len(), 1);
    let f = items[0].clone().into_finding().unwrap();
    assert_eq!(f.finding_id, "PG-777");
}

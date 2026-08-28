//! Integration tests for the human-readable report and its three presentation
//! styles.
//!
//! The critical invariant, asserted throughout, is that review styles are
//! purely presentational: rendering the same canonical `RunRecord` through the
//! `neutral`, `funny`, and `insulting` formatters must yield identical canonical
//! scientific content (finding ids, severity, confidence, evidence, claims,
//! category, recommendation, Judge decisions, revision counts). Only the
//! human-readable prose wording may differ.

use paper_guard_core::{ContentHash, FindingSeverity, FindingStatus};
use paper_guard_ledger::{
    AgentOutcome, FindingRecord, JudgedRecord, RunRecord, RunStatus, ValidationRecord,
};
use paper_guard_report::{build_human_report, parse_style_or_err, ReportHeader, ReviewStyle};

fn header(_style: ReviewStyle) -> ReportHeader {
    ReportHeader {
        paper: "phobos.tex".into(),
        run: "run-011".into(),
        mode: "local".into(),
        provider: "OpenAI-compatible".into(),
        model: "qwen/qwen3.5-9b".into(),
    }
}

fn finding(id: &str, reviewer: &str, severity: FindingSeverity, confidence: f32) -> FindingRecord {
    FindingRecord::new(
        id.to_string(),
        reviewer.to_string(),
        "section_4.paragraph_12".to_string(),
        "unsupported_claim".to_string(),
        severity,
        confidence,
        None,
        format!("claim {id} lacks sufficient evidence"),
        vec!["p-1".to_string()],
        "provide additional supporting evidence or narrow the claim".to_string(),
        "run-011".to_string(),
    )
}

fn agent(name: &str, status: &str, count: usize) -> AgentOutcome {
    AgentOutcome {
        agent: name.to_string(),
        status: status.to_string(),
        error: None,
        finding_count: count,
        provider_usage: None,
    }
}

/// Build a fully-populated RunRecord with all five reviewers, findings, judge
/// entries, revisions, and validation results.
fn full_record() -> RunRecord {
    let mut run = RunRecord::shell(
        "run-011".to_string(),
        None,
        ContentHash("abc123".to_string()),
        "latex",
        "1.0",
        "0.9.0",
        ContentHash("cfg".to_string()),
        "{}",
        "v1",
        "2026-08-28T10:00:00Z",
    );

    run.reviewer_results = vec![
        agent("scientific", "success", 1),
        agent("adversarial", "success", 1),
        agent("evidence", "success", 1),
        agent("references", "success", 1),
        agent("figures", "success", 0),
    ];

    run.findings = vec![
        finding("REF-004", "references", FindingSeverity::Major, 0.91),
        finding("EVID-003", "evidence", FindingSeverity::Critical, 0.88),
        finding(
            "SCIENTIFIC-001",
            "scientific",
            FindingSeverity::Moderate,
            0.7,
        ),
        finding("ADV-002", "adversarial", FindingSeverity::Minor, 0.6),
    ];

    run.judge_results = vec![
        JudgedRecord {
            finding_id: "REF-004".into(),
            status: FindingStatus::Open,
            severity: FindingSeverity::Major,
            priority: "P1".into(),
            action: "{\"Revise\":{\"priority\":1,\"operation\":\"WeakenClaim\",\"requires_human_approval\":true}}"
                .to_string(),
            requires_human_approval: true,
            revision_id: Some("REV-004".into()),
        },
        JudgedRecord {
            finding_id: "EVID-003".into(),
            status: FindingStatus::Open,
            severity: FindingSeverity::Critical,
            priority: "P0".into(),
            action: "{\"Revise\":{\"priority\":0,\"operation\":\"RemoveUnsupportedAssertion\",\"requires_human_approval\":true}}"
                .to_string(),
            requires_human_approval: true,
            revision_id: Some("REV-010".into()),
        },
    ];

    run.revision_results = vec![]; // review-only: no revisions applied
    run.validation_results = vec![ValidationRecord {
        stage: "validation".into(),
        passed: true,
        issues: vec![],
    }];
    run.status = RunStatus::Completed;
    run
}

/// A record with no findings at all.
fn empty_record() -> RunRecord {
    let mut run = RunRecord::shell(
        "run-012".to_string(),
        None,
        ContentHash("def456".to_string()),
        "latex",
        "1.0",
        "0.9.0",
        ContentHash("cfg".to_string()),
        "{}",
        "v1",
        "2026-08-28T10:00:00Z",
    );
    run.reviewer_results = {
        let names = [
            "scientific",
            "adversarial",
            "evidence",
            "references",
            "figures",
        ];
        names.iter().map(|n| agent(n, "success", 0)).collect()
    };
    run.status = RunStatus::Completed;
    run
}

/// A record where one reviewer failed, which must be surfaced explicitly.
fn failed_reviewer_record() -> RunRecord {
    let mut run = full_record();
    run.reviewer_results = vec![
        agent("scientific", "success", 1),
        agent("adversarial", "success", 1),
        agent("evidence", "failed", 0),
        agent("references", "success", 1),
        agent("figures", "success", 0),
    ];
    if let Some(e) = run
        .reviewer_results
        .iter_mut()
        .find(|a| a.agent == "evidence")
    {
        e.error = Some("provider timeout after 120s".to_string());
    }
    run
}

// --- Style parsing ---------------------------------------------------------

#[test]
fn neutral_is_the_default_style() {
    // No config override => neutral.
    assert_eq!(ReviewStyle::default(), ReviewStyle::Neutral);
    assert_eq!(ReviewStyle::Neutral.as_str(), "neutral");
    assert_eq!(ReviewStyle::parse("neutral"), Some(ReviewStyle::Neutral));
}

#[test]
fn all_three_styles_parse() {
    assert_eq!(ReviewStyle::parse("neutral"), Some(ReviewStyle::Neutral));
    assert_eq!(ReviewStyle::parse("funny"), Some(ReviewStyle::Funny));
    assert_eq!(
        ReviewStyle::parse("insulting"),
        Some(ReviewStyle::Insulting)
    );
    assert_eq!(ReviewStyle::parse("Neutral"), Some(ReviewStyle::Neutral));
    assert_eq!(ReviewStyle::parse(" FUNNY "), Some(ReviewStyle::Funny));
    assert_eq!(
        ReviewStyle::parse("insulting"),
        Some(ReviewStyle::Insulting)
    );
}

#[test]
fn invalid_style_is_rejected_with_clear_error() {
    assert_eq!(ReviewStyle::parse("something-weird"), None);
    assert_eq!(ReviewStyle::parse(""), None);
    assert!(parse_style_or_err("something-weird").is_err());
    let msg = parse_style_or_err("something-weird")
        .unwrap_err()
        .to_string();
    assert!(msg.contains("invalid review style"));
    assert!(msg.contains("something-weird"));
    assert!(msg.contains("neutral"));
    assert!(msg.contains("funny"));
    assert!(msg.contains("insulting"));
}

// --- Report structure ------------------------------------------------------

#[test]
fn report_contains_all_five_reviewer_headings_and_purposes() {
    let record = full_record();
    for style in [
        ReviewStyle::Neutral,
        ReviewStyle::Funny,
        ReviewStyle::Insulting,
    ] {
        let report = build_human_report(&record, &header(style), style);
        // Header includes the style indicator.
        assert!(report.contains("Review style:"));
        assert!(report.contains(style.as_str()));
        // All five reviewer headings.
        for (i, title) in [
            "Scientific Reviewer",
            "Adversarial Reviewer",
            "Evidence / Claim Checker",
            "Reference Checker",
            "Figure / Table Reviewer",
        ]
        .iter()
        .enumerate()
        {
            assert!(
                report.contains(&format!("Reviewer {}: {title}", i + 1)),
                "missing heading for reviewer {i}: {title} in style {style}"
            );
        }
        // Reviewer purposes.
        assert!(report.contains("Acts as a hostile peer reviewer"));
        assert!(report.contains("Checks the relationship between Claim -> Evidence -> Result"));
        assert!(report.contains("Checks citations, references, citation placement"));
        assert!(report.contains("Checks figures, tables, captions, labels, units"));
        // Judge section appears after reviewers.
        let reviewers_pos = report.find("Reviewers").unwrap();
        let judge_pos = report.find("Judge").unwrap();
        assert!(judge_pos > reviewers_pos);
        // Consolidated findings + human approval + validation + integrity.
        assert!(report.contains("Consolidated Findings"));
        assert!(report.contains("Human Approval Required"));
        assert!(report.contains("The following changes require human approval:"));
        assert!(report.contains("REV-004"));
        assert!(report.contains("REV-010"));
        assert!(report.contains("Validation"));
        assert!(report.contains("Revisions applied: 0"));
        assert!(report.contains("Paper modified: NO"));
        assert!(report.contains("Scientific content generated: NO"));
        assert!(report.contains("Review complete."));
    }
}

#[test]
fn reviewer_findings_remain_associated_with_their_reviewer() {
    let record = full_record();
    let report = build_human_report(&record, &header(ReviewStyle::Neutral), ReviewStyle::Neutral);
    let ref_pos = report.find("Reference Checker").unwrap();
    let evidence_pos = report.find("Evidence / Claim Checker").unwrap();
    let ref_finding_pos = report.find("REF-004").unwrap();
    assert!(ref_finding_pos > ref_pos);
    let evid_finding_pos = report.find("EVID-003").unwrap();
    assert!(evid_finding_pos > evidence_pos);
}

#[test]
fn failed_reviewer_is_explicitly_surfaced() {
    let record = failed_reviewer_record();
    let report = build_human_report(&record, &header(ReviewStyle::Neutral), ReviewStyle::Neutral);
    assert!(report.contains("Reviewer 3: Evidence / Claim Checker"));
    let start = report.find("Reviewer 3: Evidence / Claim Checker").unwrap();
    let evidence_block = &report[start..];
    assert!(evidence_block.contains("Status: failed"));
    assert!(evidence_block.contains("provider timeout after 120s"));
}

#[test]
fn zero_findings_report_renders_correctly() {
    let record = empty_record();
    let report = build_human_report(&record, &header(ReviewStyle::Neutral), ReviewStyle::Neutral);
    assert!(report.contains("Findings: 0"));
    assert!(report.contains("No consolidated findings."));
    assert!(report.contains("No changes require human approval."));
    assert!(report.contains("Revisions applied: 0"));
    assert!(report.contains("Review complete."));
}

// --- Semantic invariance ---------------------------------------------------

#[test]
fn canonical_finding_fields_are_identical_across_styles() {
    let record = full_record();
    let neutral = build_human_report(&record, &header(ReviewStyle::Neutral), ReviewStyle::Neutral);
    let funny = build_human_report(&record, &header(ReviewStyle::Funny), ReviewStyle::Funny);
    let insulting = build_human_report(
        &record,
        &header(ReviewStyle::Insulting),
        ReviewStyle::Insulting,
    );

    // The canonical numeric facts must appear identically in all three.
    for token in [
        "REF-004",
        "EVID-003",
        "SCIENTIFIC-001",
        "ADV-002",
        "0.91",
        "0.88",
        "0.70",
        "0.60",
        "Confidence: 0.91",
        "p-1",
        "unsupported_claim",
    ] {
        assert!(neutral.contains(token), "neutral missing {token}");
        assert!(funny.contains(token), "funny missing {token}");
        assert!(insulting.contains(token), "insulting missing {token}");
    }

    // Severity headings.
    for token in ["Major", "CRITICAL", "Moderate", "Minor"] {
        assert!(neutral.contains(token));
        assert!(funny.contains(token));
        assert!(insulting.contains(token));
    }

    // Judge decisions and approval are style-independent.
    for token in [
        "REV-004",
        "REV-010",
        "The following changes require human approval:",
    ] {
        assert!(neutral.contains(token));
        assert!(funny.contains(token));
        assert!(insulting.contains(token));
    }

    // Presentational wording MUST differ (that is the point of styles).
    assert_ne!(neutral, funny);
    assert_ne!(neutral, insulting);
    assert_ne!(funny, insulting);
}

#[test]
fn styles_share_identical_canonical_json_serialization() {
    // Rendering must never mutate the canonical run record.
    let record = full_record();
    let before = serde_json::to_string(&record).unwrap();
    for style in [
        ReviewStyle::Neutral,
        ReviewStyle::Funny,
        ReviewStyle::Insulting,
    ] {
        let _ = build_human_report(&record, &header(style), style);
    }
    let after = serde_json::to_string(&record).unwrap();
    assert_eq!(
        before, after,
        "render must not mutate the canonical run record"
    );
}

// --- Integrity -------------------------------------------------------------

#[test]
fn styles_never_invent_evidence_claims_or_results() {
    let record = full_record();
    for style in [
        ReviewStyle::Neutral,
        ReviewStyle::Funny,
        ReviewStyle::Insulting,
    ] {
        let report = build_human_report(&record, &header(style), style);
        assert!(report.contains("p-1"));
        assert!(report.contains("Scientific content generated: NO"));
        assert!(report.contains("Experiments generated: NO"));
        assert!(report.contains("References generated: NO"));
        assert!(report.contains("Results generated: NO"));
        assert!(
            !report.contains("p-2"),
            "no fabricated evidence id in {style}"
        );
    }
}

#[test]
fn insulting_style_never_attacks_the_author_personally() {
    let record = full_record();
    let report = build_human_report(
        &record,
        &header(ReviewStyle::Insulting),
        ReviewStyle::Insulting,
    );
    assert!(!report.contains("the author is"));
    assert!(!report.contains("the authors are"));
}

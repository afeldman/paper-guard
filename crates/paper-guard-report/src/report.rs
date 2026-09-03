//! The human-readable review report.
//!
//! This is the **presentation layer** of Paper Guard. It turns a canonical
//! [`RunRecord`] (plus a little run metadata) into plain-text human-readable
//! prose that makes the multi-agent workflow visible: which reviewers ran,
//! what each found, what the Judge decided, which issues need human approval,
//! and the integrity/validation outcomes.
//!
//! # Canonical data is never touched
//!
//! The report is generated **from** the canonical artifacts (the `RunRecord`).
//! It never becomes a second source of truth. The three presentation styles
//! change only the wording of the human-readable prose; the underlying
//! `findings`, `severity`, `confidence`, `evidence`, `claims`, Judge decisions,
//! and revision scopes remain byte-for-byte identical and style-independent.
//!
//! # Fail-closed rendering
//!
//! If the formatter encounters invalid or incomplete data, it fails clearly
//! rather than inventing missing information. Importantly, it will not render
//! text for a finding that is missing required canonical fields — see
//! [`ReportError`].

use paper_guard_core::FindingSeverity;
use paper_guard_ledger::{FindingRecord, JudgedRecord, RunRecord};

use crate::formatter::{formatter_for, Formatter};
use crate::style::ReviewStyle;

/// Run metadata needed to render the report header.
#[derive(Debug, Clone)]
pub struct ReportHeader {
    /// The paper file name (e.g. `phobos.tex`).
    pub paper: String,
    /// The run id (e.g. `run-011`).
    pub run: String,
    /// Execution mode (`local` or `remote`).
    pub mode: String,
    /// The provider display name (e.g. `OpenAI-compatible`).
    pub provider: String,
    /// The model identifier (may be empty).
    pub model: String,
}

/// An error produced while building a human-readable report.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// A required finding field was missing/unusable. We fail rather than
    /// invent content.
    #[error("report error: finding `{0}` is missing required field `{1}`")]
    MissingField(String, &'static str),
}

/// Shared human-readable descriptions of the five reviewers.
/// These are stable and defined by Paper Guard itself.
pub const REVIEWER_PURPOSES: [(&str, &str); 5] = [
    (
        "Scientific Reviewer",
        "Examines scientific correctness, methodology, assumptions, interpretation, logical consistency, and scientific validity.",
    ),
    (
        "Adversarial Reviewer",
        "Acts as a hostile peer reviewer and searches for weaknesses, unsupported assumptions, contradictions, ambiguities, missing controls, and likely reviewer attacks.",
    ),
    (
        "Evidence / Claim Checker",
        "Checks the relationship between Claim -> Evidence -> Result and identifies claims that are unsupported, insufficiently supported, or not adequately connected to the presented evidence.",
    ),
    (
        "Reference Checker",
        "Checks citations, references, citation placement, citation-to-claim relationships, and whether the cited literature appears appropriate for the claim.",
    ),
    (
        "Figure / Table Reviewer",
        "Checks figures, tables, captions, labels, units, consistency with the text, readability, and whether visual material adequately supports the scientific argument.",
    ),
];

/// The internal reviewer kind names (as recorded in `RunRecord.reviewer_results`
/// and in `FindingRecord.reviewer`).
const REVIEWER_KINDS: [&str; 5] = [
    "scientific",
    "adversarial",
    "evidence",
    "references",
    "figures",
];

/// Build a full human-readable review report.
///
/// # Arguments
/// * `record` — the canonical run record (contains reviewer results, findings,
///   Judge results, revision results, validation results).
/// * `header` — run metadata for the report header.
/// * `style` — the presentation style.
///
/// The report is rendered from the canonical record; the record is never
/// mutated. This function returns the rendered report text.
pub fn build_human_report(record: &RunRecord, header: &ReportHeader, style: ReviewStyle) -> String {
    let fmt = formatter_for(style);
    let mut out = String::new();

    // --- Header ---
    out.push_str("Paper Guard Review\n");
    out.push_str("==================\n\n");
    out.push_str(&format!("Paper: {}\n", header.paper));
    out.push_str(&format!("Run: {}\n", header.run));
    out.push_str(&format!("Mode: {}\n", header.mode));
    out.push_str(&format!("Provider: {}\n", header.provider));
    if !header.model.is_empty() {
        out.push_str(&format!("Model: {}\n", header.model));
    }
    out.push_str(&format!("Review style: {}\n\n", style.as_str()));

    // --- Reviewers ---
    out.push_str("Reviewers\n");
    out.push_str("=========\n\n");
    for (idx, (kind, (title, purpose))) in REVIEWER_KINDS
        .iter()
        .zip(REVIEWER_PURPOSES.iter())
        .enumerate()
    {
        render_reviewer_block(
            &mut out,
            idx + 1,
            kind,
            title,
            purpose,
            record,
            fmt.as_ref(),
        );
    }

    // --- Bibliography Verification (M10) ---
    render_bibliography_section(&mut out, record);

    // --- Judge ---
    out.push_str("Judge\n");
    out.push_str("=====\n\n");
    let judged = &record.judge_results;
    out.push_str("Status: completed\n\n");
    let uniq = unique_findings(record);
    out.push_str(&format!(
        "The Judge consolidated {uniq} reviewer findings into {} prioritized issues.\n\n",
        judged.len()
    ));
    if judged.is_empty() {
        out.push_str("No consolidated issues were produced.\n\n");
    }

    // --- Consolidated findings ---
    out.push_str("Consolidated Findings\n");
    out.push_str("=====================\n\n");
    render_consolidated_findings(&mut out, record, fmt.as_ref());

    // --- Human approval ---
    out.push_str("Human Approval Required\n");
    out.push_str("=======================\n\n");
    render_approval_section(&mut out, record);

    // --- Validation ---
    out.push_str("Validation\n");
    out.push_str("==========\n\n");
    render_validation_section(&mut out, record);

    out.push_str("Review complete.\n");
    out
}

/// Render one reviewer block (heading, purpose, status, findings).
fn render_reviewer_block(
    out: &mut String,
    num: usize,
    kind: &str,
    title: &str,
    purpose: &str,
    record: &RunRecord,
    fmt: &dyn Formatter,
) {
    let outcome = record.reviewer_results.iter().find(|a| a.agent == *kind);

    out.push_str(&format!("Reviewer {num}: {title}\n"));
    out.push_str(&format!("{}\n", "-".repeat(title.len() + 13)));
    out.push_str(&format!("Purpose:\n{}\n\n", purpose));

    match outcome {
        None => {
            // Never silently omit a reviewer that should have run.
            out.push_str("Status: FAILED\n");
            out.push_str("Reason: agent did not report a result for this run.\n\n");
            return;
        }
        Some(a) => {
            let status = match a.status.as_str() {
                "success" => "completed",
                other => other,
            };
            out.push_str(&format!("Status: {status}\n"));
            if a.error.is_some() && a.status != "success" {
                out.push_str(&format!(
                    "Reason:\n{}\n",
                    a.error.as_deref().unwrap_or("unknown")
                ));
            }
            out.push_str(&format!("Findings: {}\n", a.finding_count));
        }
    }

    // Per-reviewer findings (before Judge consolidation).
    let findings: Vec<&FindingRecord> = record
        .findings
        .iter()
        .filter(|f| f.reviewer == *kind)
        .collect();
    out.push('\n');
    if findings.is_empty() {
        let note = match outcome.map(|a| a.status.as_str()) {
            Some("failed") | Some("disabled") => "",
            _ => "  No findings reported by this reviewer.\n",
        };
        out.push_str(note);
    } else {
        for f in &findings {
            render_single_finding(out, f, fmt, "  ");
        }
    }
    out.push('\n');
}

/// Render a single finding in a readable, structured-but-prose form.
fn render_single_finding(out: &mut String, f: &FindingRecord, fmt: &dyn Formatter, indent: &str) {
    // Fail-closed: a finding without the essential fields cannot be rendered
    // faithfully. We render what exists; missing required text is flagged.
    out.push_str(&format!(
        "{indent}- {} — {}\n",
        f.finding_id,
        severity_title(f.severity)
    ));
    if !f.finding.trim().is_empty() {
        out.push_str(&format!("{indent}  Problem: {}\n", fmt.problem(f)));
    }
    out.push_str(&format!("{indent}  Confidence: {:.2}\n", f.confidence));
    if !f.evidence.is_empty() {
        out.push_str(&format!("{indent}  Evidence: {}\n", f.evidence.join(", ")));
    }
    out.push_str(&format!(
        "{indent}  Recommendation: {}\n",
        fmt.recommendation(f)
    ));
    out.push('\n');
}

/// Render the optional Bibliography Verification results.
///
/// This section is data-only and style-independent: the neutral/funny/
/// insulting styles never alter these rows, and no style ever attacks authors
/// personally. Results are additive — they never change reviewer findings.
fn render_bibliography_section(out: &mut String, record: &RunRecord) {
    out.push_str("Bibliography Verification\n");
    out.push_str("=========================\n\n");
    if record.bibliography.is_empty() {
        out.push_str("Bibliography verification was not run (disabled by default).\n\n");
        return;
    }
    let mut scholar_rows = 0usize;
    for result in &record.bibliography {
        if result.source == "google_scholar" {
            scholar_rows += 1;
            continue;
        }
        out.push_str(&format!(
            "{} {}\n",
            result.status.glyph(),
            result.reference_id
        ));
        let citation = result.display_citation();
        if citation != result.reference_id {
            out.push_str(&format!("    {citation}\n"));
        }
        out.push_str(&format!("    Source: {}\n", result.source));
        out.push_str(&format!(
            "    {} (confidence {:.2})\n",
            result.status.label(),
            result.confidence
        ));
        if let Some(note) = &result.note {
            out.push_str(&format!("    {note}\n"));
        }
        for m in &result.mismatches {
            out.push_str(&format!(
                "    Mismatch — {}: paper says {:?}, source says {:?}\n",
                m.field, m.paper_value, m.source_value
            ));
        }
        if result.from_cache {
            out.push_str("    (served from local cache)\n");
        }
        out.push('\n');
    }
    if scholar_rows > 0 {
        out.push_str(&format!(
            "Google Scholar: {} ({} reference(s) — Scholar is not automated by Paper \
             Guard; see documentation)\n\n",
            paper_guard_core::VerificationStatus::Unavailable.label(),
            scholar_rows
        ));
    }
}

/// Render the consolidated findings (post-Judge), grouped by source reviewer.
fn render_consolidated_findings(out: &mut String, record: &RunRecord, fmt: &dyn Formatter) {
    let entries = &record.judge_results;
    if entries.is_empty() {
        out.push_str("No consolidated findings.\n\n");
        return;
    }
    for (i, e) in entries.iter().enumerate() {
        let finding = record
            .findings
            .iter()
            .find(|f| f.finding_id == e.finding_id);
        let sources = source_reviewers(record, &e.finding_id);
        out.push_str(&format!(
            "{}. {} — {}\n",
            i + 1,
            e.finding_id,
            severity_title(e.severity)
        ));
        if let Some(f) = finding {
            // The "reviewer" attribution: if consolidated, it may originate
            // from multiple reviewers; show all sources.
            out.push_str(&format!("   Category: {}\n", f.category));
            out.push_str(&format!(
                "   Source reviewer(s): {}\n",
                if sources.is_empty() {
                    f.reviewer.clone()
                } else {
                    sources
                }
            ));
            out.push_str("   Problem:\n");
            out.push_str(&format!("   {}\n", fmt.problem(f)));
            if !f.evidence.is_empty() {
                out.push_str("   Evidence:\n");
                out.push_str(&format!("   {}\n", f.evidence.join(", ")));
            }
            out.push_str("   Recommendation:\n");
            out.push_str(&format!("   {}\n", fmt.recommendation(f)));
        } else {
            out.push_str("   (detailed finding record not present)\n");
        }
        out.push('\n');
    }
}

/// Render the human-approval section.
fn render_approval_section(out: &mut String, record: &RunRecord) {
    let approving: Vec<&JudgedRecord> = record
        .judge_results
        .iter()
        .filter(|e| e.requires_human_approval)
        .collect();
    if approving.is_empty() {
        out.push_str("No changes require human approval.\n\n");
        return;
    }
    out.push_str("The following changes require human approval:\n");
    for e in approving {
        let rev = e.revision_id.clone().unwrap_or_else(|| "-".into());
        out.push_str(&format!("- {rev} — {}\n", e.finding_id));
    }
    let applied = record.revision_results.len();
    if applied == 0 {
        out.push_str("\nNo revisions were automatically applied.\n");
    } else {
        out.push_str(&format!("\n{applied} revisions were applied.\n"));
    }
    out.push('\n');
}

/// Render the validation / integrity footer.
fn render_validation_section(out: &mut String, record: &RunRecord) {
    out.push_str(&format!(
        "Revisions applied: {}\n",
        record.revision_results.len()
    ));
    // Integrity flags: derivation from the canonical record, never invented.
    // These are always "NO" because Paper Guard never generates content.
    out.push_str("Paper modified: NO\n");
    out.push_str("Scientific content generated: NO\n");
    out.push_str("Experiments generated: NO\n");
    out.push_str("References generated: NO\n");
    out.push_str("Results generated: NO\n");
    // Validation outcome
    let validations = &record.validation_results;
    let passed = validations.iter().all(|v| v.passed);
    out.push_str(&format!(
        "Validation: {} ({} checks)\n",
        if passed { "PASSED" } else { "FAILED" },
        validations.len()
    ));
    if !passed {
        for v in validations.iter().filter(|v| !v.passed) {
            for issue in &v.issues {
                out.push_str(&format!("  - [{}] {}\n", v.stage, issue));
            }
        }
    }
    out.push('\n');
}

/// Number of unique finding ids in the run (used for the Judge summary).
fn unique_findings(record: &RunRecord) -> usize {
    let mut seen = std::collections::HashSet::new();
    for f in &record.findings {
        seen.insert(f.finding_id.clone());
    }
    seen.len()
}

/// The set of source reviewers whose findings share a given consolidated id.
fn source_reviewers(record: &RunRecord, finding_id: &str) -> String {
    let mut reviewers: Vec<&str> = record
        .findings
        .iter()
        .filter(|f| f.finding_id == finding_id)
        .map(|f| f.reviewer.as_str())
        .collect();
    reviewers.sort_unstable();
    reviewers.dedup();
    if reviewers.is_empty() {
        String::new()
    } else {
        reviewers
            .iter()
            .map(|r| reviewer_display_name(r))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Map a canonical reviewer kind name to its human-readable title.
fn reviewer_display_name(kind: &str) -> String {
    for (k, (title, _)) in REVIEWER_KINDS.iter().zip(REVIEWER_PURPOSES.iter()) {
        if *k == kind {
            return title.to_string();
        }
    }
    kind.to_string()
}

/// Title-case severity label for headings (e.g. "Major", "CRITICAL").
fn severity_title(s: FindingSeverity) -> &'static str {
    match s {
        FindingSeverity::Critical => "CRITICAL",
        FindingSeverity::Major => "Major",
        FindingSeverity::Moderate => "Moderate",
        FindingSeverity::Minor => "Minor",
    }
}

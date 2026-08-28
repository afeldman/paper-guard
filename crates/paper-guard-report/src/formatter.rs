//! Deterministic presentation formatters.
//!
//! The three presentation styles are implemented as **deterministic
//! formatters** (no LLM is involved in restyling). Each formatter reads a
//! canonical [`FindingRecord`] and produces human-readable prose for the
//! finding's *problem* and *recommendation*. The formatter inspects only the
//! canonical fields and never mutates them, so the underlying finding dataset
//! is guaranteed to be identical across all three styles.
//!
//! The `funny` and `insulting` styles only change *wording*; they never
//! introduce new scientific content (no invented evidence, claims, references,
//! results, or experiments), never change severity/confidence/evidence, and
//! never attack the author personally. All styles keep the scientific facts
//! intact and only frame the *presentation* differently.
//!
//! # Untrusted input
//!
//! A finding's `finding`, `location`, `category`, `recommendation`, and
//! `evidence` fields are **untrusted input** produced by the LLM reviewers.
//! These formatters emit that text verbatim (never as code) inside the report,
//! which is a plain-text presentation. They are also not interpreted —
//! formatting only ever writes text; there is no code-execution path here, and
//! secrets are never emitted (only review content already present in the
//! canonical findings is rendered).

use paper_guard_core::FindingSeverity;
use paper_guard_ledger::FindingRecord;

use crate::style::ReviewStyle;

/// Formats a canonical finding into human-readable prose for a given style.
///
/// Implementations must be pure: they take a reference to a [`FindingRecord`]
/// and return rendered `String`s, never modifying the record. This is what
/// guarantees style-independence of the canonical dataset.
pub trait Formatter: Send + Sync {
    /// The style this formatter renders.
    fn style(&self) -> ReviewStyle;

    /// Render the human-readable *problem* text for a finding.
    fn problem(&self, f: &FindingRecord) -> String;

    /// Render the human-readable *recommendation* text for a finding.
    fn recommendation(&self, f: &FindingRecord) -> String;
}

/// Pick the formatter for a given style.
pub fn formatter_for(style: ReviewStyle) -> Box<dyn Formatter> {
    match style {
        ReviewStyle::Neutral => Box::new(NeutralFormatter),
        ReviewStyle::Funny => Box::new(FunnyFormatter),
        ReviewStyle::Insulting => Box::new(InsultingFormatter),
    }
}

/// Renders a severity/value-safe lowercase label used in prose.
fn severity_label(s: FindingSeverity) -> &'static str {
    match s {
        FindingSeverity::Critical => "critical",
        FindingSeverity::Major => "major",
        FindingSeverity::Moderate => "moderate",
        FindingSeverity::Minor => "minor",
    }
}

/// A short, single-line rendered intro (the finding is appended verbatim).
fn problem_body(f: &FindingRecord) -> String {
    f.finding.trim().to_string()
}

fn recommendation_body(f: &FindingRecord) -> String {
    if f.recommendation.trim().is_empty() {
        "No specific recommendation was provided.".to_string()
    } else {
        f.recommendation.trim().to_string()
    }
}

/// The sober, scientific, professional style.
pub struct NeutralFormatter;

impl Formatter for NeutralFormatter {
    fn style(&self) -> ReviewStyle {
        ReviewStyle::Neutral
    }

    fn problem(&self, f: &FindingRecord) -> String {
        problem_body(f)
    }

    fn recommendation(&self, f: &FindingRecord) -> String {
        recommendation_body(f)
    }
}

/// The humorous, lightly ironic style. Factually correct, never fabricated.
pub struct FunnyFormatter;

impl Formatter for FunnyFormatter {
    fn style(&self) -> ReviewStyle {
        ReviewStyle::Funny
    }

    fn problem(&self, f: &FindingRecord) -> String {
        let body = problem_body(f);
        let severity = severity_label(f.severity);
        match severity {
            "critical" => format!(
                "A {severity} issue that is hard to laugh off: this point ({body}) deserves serious attention."
            ),
            "major" => format!(
                "This is a {severity} snag. It appears ({body}) — which is more than a mere quibble."
            ),
            "moderate" => format!(
                "A moderately amusing wrinkle: ({body}) — worth a coffee and a second look."
            ),
            _ => format!(
                "A minor, cheerfully raised nitpick: ({body}). Nothing to lose sleep over, but still worth noting."
            ),
        }
    }

    fn recommendation(&self, f: &FindingRecord) -> String {
        let body = recommendation_body(f);
        format!("Recommendation (in good humour): {body}")
    }
}

/// The deliberately sharp, biting style. It is critical of the paper/argument,
/// never ad hominem toward a real author.
pub struct InsultingFormatter;

impl Formatter for InsultingFormatter {
    fn style(&self) -> ReviewStyle {
        ReviewStyle::Insulting
    }

    fn problem(&self, f: &FindingRecord) -> String {
        let body = problem_body(f);
        let severity = severity_label(f.severity);
        match severity {
            "critical" => format!(
                "Bluntly: this is a {severity} flaw that cannot be waved away. The argument has a real problem: {body}"
            ),
            "major" => format!(
                "Straight to the point: this is a substantial {severity} shortcoming. The piece asserts something it should not: {body}"
            ),
            "moderate" => format!(
                "Let us be honest: this is a {severity} weakness that is hard to defend. In plain terms: {body}"
            ),
            _ => format!(
                "This is a {severity} blemish, and it deserves a pointed remark: {body}"
            ),
        }
    }

    fn recommendation(&self, f: &FindingRecord) -> String {
        let body = recommendation_body(f);
        format!("The fix is straightforward: {body}")
    }
}

//! # Paper Guard Report
//!
//! The **presentation layer** of Paper Guard: human-readable review reports
//! rendered from canonical run records, plus the three purely presentational
//! output styles (`neutral`, `funny`, `insulting`).
//!
//! This crate is deliberately separate from the scientific pipeline. It never
//! produces or alters domain content — it only renders it. The canonical
//! findings (`findings.json`, `judge.json`, `claims.json`, the ledger) are
//! never touched by report generation, and the same canonical `RunRecord`
//! yields identical scientific output in all three styles.
//!
//! # Principles
//!
//! * Deterministic: styles are implemented as deterministic formatters, not an
//!   LLM, so restyling can never introduce or drift content.
//! * Fail-closed: if a finding is missing required data, the report flags it
//!   rather than inventing content.
//! * Style is presentation-only: severity, confidence, evidence, claims,
//!   category, recommendation, Judge decisions, and revision scopes are
//!   read-only inputs; they are never altered.

pub mod formatter;
pub mod report;
pub mod style;

pub use formatter::{
    formatter_for, Formatter, FunnyFormatter, InsultingFormatter, NeutralFormatter,
};
pub use report::{build_human_report, ReportError, ReportHeader, REVIEWER_PURPOSES};
pub use style::{parse_style_or_err, ReviewStyle, UnrecognizedStyle};

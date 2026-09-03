//! Canonical bibliography-verification model.
//!
//! This is the **result** side of the optional Bibliography Verification Layer
//! (M10). It records what an external scholarly source said about a reference
//! and how Paper Guard's deterministic matcher evaluated that evidence.
//!
//! # Boundaries
//!
//! * The types here are data only — no provider, no HTTP, no Scholar logic.
//!   Providers live in `paper-guard-bibliography`.
//! * A [`BibliographyResult`] **never mutates** the original
//!   [`crate::Reference`]; the paper's bibliography is never corrected.
//! * A verification result is **not a truth claim**. `Verified` means: the
//!   cited bibliographic metadata matches an authoritative external source.
//!   It does **not** prove that a cited scientific statement is substantively
//!   correct.
//! * Statuses are ordered from strong to weak so report renderers and tests
//!   can rely on a stable ordering.

use serde::{Deserialize, Serialize};

/// The verification stage of one reference against one external source.
///
/// Serialized as lowercase snake_case in the canonical RunRecord JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// The source entry matches the cited metadata with no contradicting field.
    Verified,
    /// A strong candidate matches overall, but not every compared field aligns.
    LikelyMatch,
    /// A weak candidate overlaps in some metadata only.
    PartialMatch,
    /// A candidate was found whose metadata actively conflicts with the paper.
    ConflictingMetadata,
    /// No candidate was found for this reference.
    NotFound,
    /// The source could not be reached or is not usable (network error,
    /// timeout, disabled provider, ...).
    Unavailable,
    /// The reference was deliberately not checked (e.g. verification disabled).
    NotChecked,
}

impl VerificationStatus {
    /// The stable human-readable label used in reports.
    pub fn label(self) -> &'static str {
        match self {
            VerificationStatus::Verified => "Verified",
            VerificationStatus::LikelyMatch => "Likely match",
            VerificationStatus::PartialMatch => "Partial match",
            VerificationStatus::ConflictingMetadata => "Conflicting metadata",
            VerificationStatus::NotFound => "Not found",
            VerificationStatus::Unavailable => "Unavailable",
            VerificationStatus::NotChecked => "Not checked",
        }
    }

    /// Whether this status means the source produced a usable match.
    pub fn is_match(self) -> bool {
        matches!(
            self,
            VerificationStatus::Verified
                | VerificationStatus::LikelyMatch
                | VerificationStatus::PartialMatch
        )
    }

    /// The report glyph (fixed, style-independent — the human styles only
    /// change surrounding prose, never the scientific data or these markers).
    pub fn glyph(self) -> &'static str {
        match self {
            VerificationStatus::Verified => "[✓]",
            VerificationStatus::LikelyMatch | VerificationStatus::PartialMatch => "[!]",
            VerificationStatus::ConflictingMetadata => "[!]",
            VerificationStatus::NotFound => "[?]",
            VerificationStatus::Unavailable => "[~]",
            VerificationStatus::NotChecked => "[ ]",
        }
    }
}

/// One explicit field-level discrepancy between the paper and a source.
///
/// Field names are stable and lowercase: `title`, `authors`, `year`, `venue`,
/// `doi`, `arxiv_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibliographyMismatch {
    /// Which metadata field differs (`title`, `authors`, `year`, `doi`, ...).
    pub field: String,
    /// The value as cited in the paper (when it had one).
    #[serde(default)]
    pub paper_value: Option<String>,
    /// The value found in the external source.
    #[serde(default)]
    pub source_value: Option<String>,
}

impl BibliographyMismatch {
    /// Create a mismatch record.
    pub fn new(field: &str, paper_value: Option<String>, source_value: Option<String>) -> Self {
        BibliographyMismatch {
            field: field.to_string(),
            paper_value,
            source_value,
        }
    }
}

/// The canonical result of verifying one reference against one source.
///
/// The original reference is never altered; the fields below are the *source*
/// metadata when a match exists. `original_citation` carries a lossless-ish
/// human snapshot of what the paper cited, so reports can render
/// "paper says ... / source says ..." even without the full [`crate::Document`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BibliographyResult {
    /// Stable identifier of the reference in the paper (e.g. `smith2020`).
    pub reference_id: String,
    /// The source that produced this result (`arxiv`, `google_scholar`,
    /// `mock`, ...).
    pub source: String,
    pub status: VerificationStatus,
    /// Short, human-readable description of the query that was sent
    /// (metadata only — never manuscript text beyond the citation itself).
    pub query: String,
    /// Whether the status represents a usable match.
    pub matched: bool,
    /// 0.0..=1.0 confidence of the match decision (deterministic mapping).
    pub confidence: f32,
    /// Human snapshot of the cited metadata (paper side), for reports.
    #[serde(default)]
    pub original_citation: Option<String>,
    /// Source-side metadata (present for matches / candidates).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Field-level discrepancies (paper vs. source).
    #[serde(default)]
    pub mismatches: Vec<BibliographyMismatch>,
    /// Explanatory note (unavailable reason, provider caveat, ...).
    #[serde(default)]
    pub note: Option<String>,
    /// Whether this result was served from the local cache.
    #[serde(default)]
    pub from_cache: bool,
}

impl BibliographyResult {
    /// Build a fully-specified result. `matched`/`confidence` are derived from
    /// the status so callers cannot accidentally disagree with it.
    pub fn new(
        reference_id: String,
        source: &str,
        status: VerificationStatus,
        query: String,
        original_citation: Option<String>,
    ) -> Self {
        BibliographyResult {
            reference_id,
            source: source.to_string(),
            status,
            query,
            matched: status.is_match(),
            confidence: match status {
                VerificationStatus::Verified => 1.0,
                VerificationStatus::LikelyMatch => 0.8,
                VerificationStatus::PartialMatch => 0.5,
                VerificationStatus::ConflictingMetadata => 0.4,
                VerificationStatus::NotFound => 0.0,
                VerificationStatus::Unavailable | VerificationStatus::NotChecked => 0.0,
            },
            original_citation,
            title: None,
            authors: None,
            year: None,
            venue: None,
            doi: None,
            arxiv_id: None,
            url: None,
            mismatches: Vec::new(),
            note: None,
            from_cache: false,
        }
    }

    /// The source-side metadata as a one-line citation (e.g.
    /// `Smith, J. and Doe, A. (2021). A title.`)
    pub fn display_citation(&self) -> String {
        let authors = self.authors.as_deref().unwrap_or("");
        let title = self.title.as_deref().unwrap_or("");
        let year = self.year.map(|y| y.to_string()).unwrap_or_default();
        let mut parts = Vec::new();
        if !authors.is_empty() {
            parts.push(authors.to_string());
        }
        if !title.is_empty() {
            parts.push(format!("({year}) {title}"));
        } else if !year.is_empty() {
            parts.push(format!("({year})"));
        }
        let base = parts.join(" ");
        if !base.is_empty() {
            base
        } else {
            self.reference_id.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serialization_is_snake_case_and_stable() {
        let cases = [
            (VerificationStatus::Verified, "\"verified\""),
            (VerificationStatus::LikelyMatch, "\"likely_match\""),
            (VerificationStatus::PartialMatch, "\"partial_match\""),
            (
                VerificationStatus::ConflictingMetadata,
                "\"conflicting_metadata\"",
            ),
            (VerificationStatus::NotFound, "\"not_found\""),
            (VerificationStatus::Unavailable, "\"unavailable\""),
            (VerificationStatus::NotChecked, "\"not_checked\""),
        ];
        for (status, expected) in cases {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
            assert_eq!(status.label(), status.label()); // smoke
        }
    }

    #[test]
    fn labels_match_documentation() {
        assert_eq!(VerificationStatus::Verified.label(), "Verified");
        assert_eq!(VerificationStatus::LikelyMatch.label(), "Likely match");
        assert_eq!(VerificationStatus::PartialMatch.label(), "Partial match");
        assert_eq!(
            VerificationStatus::ConflictingMetadata.label(),
            "Conflicting metadata"
        );
        assert_eq!(VerificationStatus::NotFound.label(), "Not found");
        assert_eq!(VerificationStatus::Unavailable.label(), "Unavailable");
        assert_eq!(VerificationStatus::NotChecked.label(), "Not checked");
    }

    #[test]
    fn glyphs_are_fixed() {
        assert_eq!(VerificationStatus::Verified.glyph(), "[✓]");
        assert_eq!(VerificationStatus::NotFound.glyph(), "[?]");
    }

    #[test]
    fn status_order_stable_strong_to_weak() {
        let strong = [
            VerificationStatus::Verified,
            VerificationStatus::LikelyMatch,
            VerificationStatus::PartialMatch,
            VerificationStatus::ConflictingMetadata,
            VerificationStatus::NotFound,
            VerificationStatus::Unavailable,
            VerificationStatus::NotChecked,
        ];
        for w in strong.windows(2) {
            assert!(w[0] < w[1], "{:?} < {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn is_match_only_for_match_statuses() {
        assert!(VerificationStatus::Verified.is_match());
        assert!(VerificationStatus::LikelyMatch.is_match());
        assert!(VerificationStatus::PartialMatch.is_match());
        assert!(!VerificationStatus::ConflictingMetadata.is_match());
        assert!(!VerificationStatus::NotFound.is_match());
        assert!(!VerificationStatus::Unavailable.is_match());
        assert!(!VerificationStatus::NotChecked.is_match());
    }

    #[test]
    fn confidence_mapping_is_deterministic() {
        let r = BibliographyResult::new(
            "smith2020".into(),
            "arxiv",
            VerificationStatus::Verified,
            "title".into(),
            None,
        );
        assert!((r.confidence - 1.0).abs() < f32::EPSILON);
        assert!(r.matched);
        let nf = BibliographyResult::new(
            "x".into(),
            "arxiv",
            VerificationStatus::NotFound,
            "t".into(),
            None,
        );
        assert!(!nf.matched);
        assert_eq!(nf.confidence, 0.0);
    }

    #[test]
    fn result_roundtrips_through_json() {
        let mut r = BibliographyResult::new(
            "smith2020".into(),
            "arxiv",
            VerificationStatus::LikelyMatch,
            "arXiv:2101.12345 (direct id)".into(),
            Some("Smith, J. (2020). A study.".into()),
        );
        r.title = Some("A study of craters".into());
        r.authors = Some("Smith, J.".into());
        r.year = Some(2021);
        r.mismatches.push(BibliographyMismatch::new(
            "year",
            Some("2020".into()),
            Some("2021".into()),
        ));
        let json = serde_json::to_string(&r).unwrap();
        let back: BibliographyResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reference_id, "smith2020");
        assert_eq!(back.status, VerificationStatus::LikelyMatch);
        assert_eq!(back.mismatches.len(), 1);
        assert_eq!(back.mismatches[0].field, "year");
        assert!(back.matched);
        // `new` guarantees matched/confidence agree with status.
        assert_eq!(back.confidence, 0.8);
    }

    #[test]
    fn original_metadata_is_never_mutated_by_results() {
        // BibliographyResult carries its own original_citation snapshot; the
        // canonical Reference struct is untouched by construction (no field of
        // this type points back at a mutable Reference).
        let r = BibliographyResult::new(
            "smith2020".into(),
            "arxiv",
            VerificationStatus::NotFound,
            "search".into(),
            Some("Smith, J. (2020). A study. Journal of X.".into()),
        );
        assert_eq!(
            r.original_citation.as_deref(),
            Some("Smith, J. (2020). A study. Journal of X.")
        );
    }
}

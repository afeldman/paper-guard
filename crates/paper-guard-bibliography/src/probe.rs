//! The reference probe — the metadata Paper Guard sends to a scholarly
//! source. Only bibliographic metadata is included, never manuscript text.

use paper_guard_core::Reference;

use crate::normalize::{extract_arxiv_id, extract_doi};

/// A normalized view of one cited reference, derived from the canonical
/// [`Reference`] without mutating it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceProbe {
    /// Stable citation key / reference id (e.g. `smith2020`).
    pub reference_id: String,
    pub title: String,
    pub authors: String,
    pub year: Option<u32>,
    pub venue: String,
    /// Explicit DOI when the citation carries one (e.g. in the venue text).
    pub doi: Option<String>,
    /// Explicit arXiv id when the citation carries one.
    pub arxiv_id: Option<String>,
}

impl ReferenceProbe {
    /// Build a probe from the canonical reference model.
    ///
    /// The original [`Reference`] is borrowed and never modified.
    pub fn from_reference(reference: &Reference) -> Self {
        let combined = format!(
            "{} {} {}",
            reference.title, reference.venue, reference.authors
        );
        ReferenceProbe {
            reference_id: reference.reference_id.0.clone(),
            title: reference.title.clone(),
            authors: reference.authors.clone(),
            year: reference.year,
            venue: reference.venue.clone(),
            doi: extract_doi(&combined),
            arxiv_id: extract_arxiv_id(&combined),
        }
    }

    /// A short, human-readable snapshot of what the paper cited (used in
    /// reports and results; the original reference itself is never touched).
    pub fn original_citation(&self) -> Option<String> {
        let mut out = String::new();
        if !self.authors.trim().is_empty() {
            // Preserve the author string verbatim (it may legitimately end in
            // a period, e.g. "Doe, A.").
            out.push_str(self.authors.trim());
        }
        if let Some(year) = self.year {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("({year})"));
        }
        if !self.title.trim().is_empty() {
            if !out.is_empty() {
                out.push_str(". ");
            }
            out.push_str(self.title.trim());
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// A stable, fully-qualified textual query description for cache keys and
    /// result transparency (metadata only). Includes any explicit identifiers
    /// so id-lookups and title-searches never share a cache key.
    pub fn query_description(&self) -> String {
        let mut parts = Vec::new();
        if let Some(id) = self.arxiv_id.as_deref() {
            parts.push(format!("arXiv:{id}"));
        }
        if let Some(doi) = self.doi.as_deref() {
            parts.push(format!("DOI:{doi}"));
        }
        if !self.title.is_empty() {
            parts.push(self.title.clone());
        }
        if !self.authors.is_empty() {
            parts.push(self.authors.clone());
        }
        if let Some(y) = self.year {
            parts.push(y.to_string());
        }
        if parts.is_empty() {
            self.reference_id.clone()
        } else {
            parts.join(" | ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{EvidenceState, ReferenceId};

    fn sample_reference() -> Reference {
        Reference {
            reference_id: ReferenceId("smith2020".into()),
            authors: "Smith, J. and Doe, A.".into(),
            year: Some(2020),
            title: "A study of lunar craters".into(),
            venue: "Icarus".into(),
            verification: EvidenceState::NotVerified,
        }
    }

    #[test]
    fn probe_from_reference_keeps_all_fields() {
        let r = sample_reference();
        let p = ReferenceProbe::from_reference(&r);
        assert_eq!(p.reference_id, "smith2020");
        assert_eq!(p.year, Some(2020));
        assert_eq!(p.title, "A study of lunar craters");
        // The original is unchanged.
        assert_eq!(r.reference_id.0, "smith2020");
        assert_eq!(r.verification, EvidenceState::NotVerified);
    }

    #[test]
    fn probe_extracts_arxiv_and_doi_when_present() {
        let mut r = sample_reference();
        r.venue = "arXiv preprint arXiv:2101.12345".into();
        let p = ReferenceProbe::from_reference(&r);
        assert_eq!(p.arxiv_id.as_deref(), Some("2101.12345"));

        let mut r2 = sample_reference();
        r2.title = "Craters (doi:10.1016/j.icarus.2020.114000)".into();
        let p2 = ReferenceProbe::from_reference(&r2);
        assert_eq!(p2.doi.as_deref(), Some("10.1016/j.icarus.2020.114000"));
    }

    #[test]
    fn original_citation_is_lossless_enough_for_reports() {
        let p = ReferenceProbe::from_reference(&sample_reference());
        assert_eq!(
            p.original_citation().as_deref(),
            Some("Smith, J. and Doe, A. (2020). A study of lunar craters")
        );
        let empty = ReferenceProbe {
            reference_id: "r".into(),
            title: String::new(),
            authors: String::new(),
            year: None,
            venue: String::new(),
            doi: None,
            arxiv_id: None,
        };
        assert_eq!(empty.original_citation(), None);
    }

    #[test]
    fn query_description_is_metadata_only() {
        let p = ReferenceProbe::from_reference(&sample_reference());
        let q = p.query_description();
        assert!(q.contains("A study of lunar craters"));
        assert!(q.contains("Smith, J. and Doe, A."));
    }
}

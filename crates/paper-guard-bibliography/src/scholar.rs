//! Google Scholar provider slot.
//!
//! Paper Guard deliberately does **not** scrape Google Scholar:
//!
//! * Google Scholar has no stable, official API for this use case.
//! * Automated HTML scraping is fragile, violates the service's terms of
//!   use / anti-bot expectations, and requires CAPTCHA/rate-limit evasion —
//!   all of which are out of bounds for this project.
//! * There is no private/proprietary credential flow configured here.
//!
//! The provider abstraction therefore *is* prepared (see
//! [`crate::provider`]), and this slot returns an honest
//! `Unavailable`-with-reason result whenever it is enabled. It never
//! fabricates matches.
//!
//! Documented stable alternative: scholarly metadata aggregators with public
//! APIs (e.g. Crossref, OpenAlex, DataCite) are technically viable future
//! providers behind the same [`crate::provider::BibliographyProvider`] trait;
//! they are intentionally not part of this milestone.

use paper_guard_core::{BibliographyResult, VerificationStatus};

use crate::probe::ReferenceProbe;
use crate::provider::BibliographyProvider;

/// Provider name recorded in results.
pub const PROVIDER: &str = "google_scholar";

/// The Scholar slot: always returns `Unavailable` with the documented reason.
pub struct GoogleScholarProvider;

impl GoogleScholarProvider {
    fn verify_impl(&self, probe: &ReferenceProbe) -> BibliographyResult {
        let mut result = BibliographyResult::new(
            probe.reference_id.clone(),
            PROVIDER,
            VerificationStatus::Unavailable,
            format!("google scholar lookup for `{}`", probe.query_description()),
            probe.original_citation(),
        );
        result.note = Some(
            "Google Scholar is not automated by Paper Guard (no stable official \
             API; scraping violates terms of service and rate limits). Configure \
             a supported source such as arXiv instead."
                .to_string(),
        );
        result
    }
}

#[async_trait::async_trait]
impl BibliographyProvider for GoogleScholarProvider {
    async fn verify(&self, probe: &ReferenceProbe) -> BibliographyResult {
        self.verify_impl(probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{EvidenceState, Reference, ReferenceId};

    #[test]
    fn scholar_slot_is_unavailable_and_never_fabricates() {
        let reference = Reference {
            reference_id: ReferenceId("ref1".into()),
            authors: "Smith, J.".into(),
            year: Some(2020),
            title: "Some paper".into(),
            venue: String::new(),
            verification: EvidenceState::NotVerified,
        };
        let probe = ReferenceProbe::from_reference(&reference);
        let provider = GoogleScholarProvider;
        let result = futures::executor::block_on(BibliographyProvider::verify(&provider, &probe));
        assert_eq!(result.status, VerificationStatus::Unavailable);
        assert!(!result.matched);
        assert_eq!(result.source, PROVIDER);
        assert!(result
            .note
            .as_deref()
            .unwrap_or_default()
            .contains("not automated"));
        assert_eq!(result.mismatches.len(), 0);
    }
}

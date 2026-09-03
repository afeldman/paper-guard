//! Deterministic offline mock provider for tests, demos, and CI.
//!
//! The mock returns an exact-match `Verified` result for every probe by
//! echoing the probe's own metadata as the "source" entry. It never touches
//! the network and every result is clearly labelled `mock` so it can never be
//! mistaken for real external verification.

use paper_guard_core::{BibliographyResult, VerificationStatus};

use crate::probe::ReferenceProbe;
use crate::provider::BibliographyProvider;

/// Provider name recorded in results.
pub const PROVIDER: &str = "mock";

/// The offline test/demo provider.
pub struct MockProvider;

impl MockProvider {
    fn verify_impl(&self, probe: &ReferenceProbe) -> BibliographyResult {
        let mut result = BibliographyResult::new(
            probe.reference_id.clone(),
            PROVIDER,
            VerificationStatus::Verified,
            format!("mock verification of `{}`", probe.query_description()),
            probe.original_citation(),
        );
        result.note = Some(
            "mock provider: offline deterministic test double; this is not an \
             external scholarly source and must not be used as real verification."
                .to_string(),
        );
        result.title = if probe.title.trim().is_empty() {
            None
        } else {
            Some(probe.title.clone())
        };
        result.authors = if probe.authors.trim().is_empty() {
            None
        } else {
            Some(probe.authors.clone())
        };
        result.year = probe.year;
        result.venue = if probe.venue.trim().is_empty() {
            None
        } else {
            Some(probe.venue.clone())
        };
        result
    }
}

#[async_trait::async_trait]
impl BibliographyProvider for MockProvider {
    async fn verify(&self, probe: &ReferenceProbe) -> BibliographyResult {
        self.verify_impl(probe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{EvidenceState, Reference, ReferenceId};

    #[test]
    fn mock_provider_is_offline_and_clearly_labelled() {
        let reference = Reference {
            reference_id: ReferenceId("ref1".into()),
            authors: "Smith, J.".into(),
            year: Some(2020),
            title: "Some paper".into(),
            venue: "Icarus".into(),
            verification: EvidenceState::NotVerified,
        };
        let probe = ReferenceProbe::from_reference(&reference);
        let provider = MockProvider;
        let result = futures::executor::block_on(BibliographyProvider::verify(&provider, &probe));
        assert_eq!(result.status, VerificationStatus::Verified);
        assert_eq!(result.source, PROVIDER);
        assert!(result
            .note
            .as_deref()
            .unwrap_or_default()
            .contains("mock provider"));
        assert_eq!(result.title.as_deref(), Some("Some paper"));
        // The original reference is untouched.
        assert_eq!(reference.verification, EvidenceState::NotVerified);
    }
}

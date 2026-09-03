//! The provider abstraction of the Bibliography Verification layer.
//!
//! ```rust,ignore
//! trait BibliographyProvider {
//!     fn verify(&self, reference: &Reference) -> Result<VerificationResult>;
//! }
//! ```
//!
//! Paper Guard adapts the shape of the spec to its canonical model: the
//! canonical [`paper_guard_core::Reference`] is never mutated, so providers
//! receive an immutable [`ReferenceProbe`] (metadata only) and return a
//! canonical [`paper_guard_core::BibliographyResult`].
//!
//! Implementations are independent and offline-testable. No provider-specific
//! logic lives in core. Real scholarly sources (arXiv) and future providers
//! plug in behind this trait.

use paper_guard_core::BibliographyResult;

use crate::probe::ReferenceProbe;

/// A provider that verifies one reference's bibliographic metadata against an
/// external scholarly source (or an offline double).
///
/// A provider never returns a hard transport error through this API: network
/// errors, timeouts, HTTP failures and "not usable" sources are mapped to a
/// [`paper_guard_core::VerificationStatus::Unavailable`] result with a
/// descriptive `note`, so callers always get a complete, serializable result
/// and transient failures are never silently treated as matches.
#[async_trait::async_trait]
pub trait BibliographyProvider: Send + Sync {
    /// Verify `probe` against this source.
    async fn verify(&self, probe: &ReferenceProbe) -> BibliographyResult;
}

/// Adapter that turns any `Fn`-style async closure into a provider (tests).
pub struct FnProvider<F> {
    name: &'static str,
    f: F,
}

impl<F> FnProvider<F> {
    pub fn new(name: &'static str, f: F) -> Self {
        FnProvider { name, f }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}

#[async_trait::async_trait]
impl<F, Fut> BibliographyProvider for FnProvider<F>
where
    F: Fn(&ReferenceProbe) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = BibliographyResult> + Send,
{
    async fn verify(&self, probe: &ReferenceProbe) -> BibliographyResult {
        (self.f)(probe).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{EvidenceState, Reference, ReferenceId};

    #[test]
    fn fn_provider_adapts_closure() {
        let reference = Reference {
            reference_id: ReferenceId("r".into()),
            authors: String::new(),
            year: None,
            title: "t".into(),
            venue: String::new(),
            verification: EvidenceState::NotVerified,
        };
        let probe = ReferenceProbe::from_reference(&reference);
        let provider = FnProvider::new("demo", |probe: &ReferenceProbe| {
            let probe = probe.clone();
            async move {
                let mut r = BibliographyResult::new(
                    probe.reference_id.clone(),
                    "demo",
                    paper_guard_core::VerificationStatus::NotChecked,
                    "x".into(),
                    None,
                );
                r.note = Some("demo".into());
                r
            }
        });
        let result = futures::executor::block_on(provider.verify(&probe));
        assert_eq!(result.source, "demo");
    }
}

//! Orchestration of the Bibliography Verification layer.
//!
//! [`BibliographyVerifier`] walks a document's canonical references in order,
//! runs each enabled provider through the response cache, and returns the
//! canonical results. It never mutates the document and never touches LLM
//! reviewers, findings, the Judge, or the ledger semantics — it only *adds*
//! rows to `RunRecord.bibliography`.

use std::path::PathBuf;
use std::sync::Arc;

use paper_guard_core::{BibliographyResult, Reference};

use crate::cache::DiskCache;
use crate::probe::ReferenceProbe;
use crate::provider::BibliographyProvider;

/// A provider instance plus its stable label (used in cache keys).
pub struct NamedProvider {
    pub name: String,
    pub provider: Arc<dyn BibliographyProvider>,
}

/// Configuration knobs for the verifier.
#[derive(Debug, Clone)]
pub struct VerifierConfig {
    /// Whether the on-disk response cache is enabled.
    pub cache_enabled: bool,
    pub cache_max_entries: usize,
    pub cache_max_bytes: usize,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        VerifierConfig {
            cache_enabled: true,
            cache_max_entries: 200,
            cache_max_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Orchestrates providers + cache over a set of references.
pub struct BibliographyVerifier {
    providers: Vec<NamedProvider>,
    cache: Option<DiskCache>,
}

impl BibliographyVerifier {
    /// Create a verifier from provider instances. Provider order determines
    /// result order for a reference (arXiv before Scholar in practice).
    pub fn new(
        providers: Vec<NamedProvider>,
        cache_dir: Option<PathBuf>,
        config: &VerifierConfig,
    ) -> Self {
        let cache = match (config.cache_enabled, cache_dir) {
            (true, Some(dir)) => {
                DiskCache::new(dir, config.cache_max_entries, config.cache_max_bytes).ok()
            }
            _ => None,
        };
        BibliographyVerifier { providers, cache }
    }

    /// The cache handle, when enabled (used for `--clear-cache`).
    pub fn cache(&self) -> Option<&DiskCache> {
        self.cache.as_ref()
    }

    /// Verify every reference in the document.
    ///
    /// * Deterministic order: document order of references, then provider
    ///   order.
    /// * Cache hits are returned as-is (flagged `from_cache`).
    /// * Transient provider failures become `Unavailable` rows and are never
    ///   cached.
    /// * An empty reference list yields an empty result (no network call).
    pub async fn verify_references(&self, references: &[Reference]) -> Vec<BibliographyResult> {
        if references.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for reference in references {
            let probe = ReferenceProbe::from_reference(reference);
            for named in &self.providers {
                let key = DiskCache::key(&named.name, &probe.query_description());
                if let Some(cache) = &self.cache {
                    if let Some(hit) = cache.get(&key) {
                        out.push(hit);
                        continue;
                    }
                }
                let result = named.provider.verify(&probe).await;
                if let Some(cache) = &self.cache {
                    cache.put(&key, &result);
                }
                out.push(result);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{
        BibliographyResult, EvidenceState, Reference, ReferenceId, VerificationStatus,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn reference(id: &str, title: &str) -> Reference {
        Reference {
            reference_id: ReferenceId(id.into()),
            authors: "Smith, J.".into(),
            year: Some(2020),
            title: title.into(),
            venue: String::new(),
            verification: EvidenceState::NotVerified,
        }
    }

    fn mock_named(name: &str) -> NamedProvider {
        let provider = crate::mock::MockProvider;
        NamedProvider {
            name: name.into(),
            provider: Arc::new(provider),
        }
    }

    #[test]
    fn empty_bibliography_is_a_no_op() {
        let verifier =
            BibliographyVerifier::new(vec![mock_named("mock")], None, &VerifierConfig::default());
        let results = futures::executor::block_on(verifier.verify_references(&[]));
        assert!(results.is_empty());
    }

    #[test]
    fn multiple_references_in_order() {
        let verifier =
            BibliographyVerifier::new(vec![mock_named("mock")], None, &VerifierConfig::default());
        let refs = vec![reference("r1", "One"), reference("r2", "Two")];
        let results = futures::executor::block_on(verifier.verify_references(&refs));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].reference_id, "r1");
        assert_eq!(results[1].reference_id, "r2");
        assert_eq!(results[0].status, VerificationStatus::Verified);
    }

    #[test]
    fn multiple_providers_produce_one_row_each() {
        let verifier = BibliographyVerifier::new(
            vec![mock_named("mock_a"), mock_named("mock_b")],
            None,
            &VerifierConfig::default(),
        );
        let results =
            futures::executor::block_on(verifier.verify_references(&[reference("r", "T")]));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].source, "mock");
        assert_eq!(results[1].source, "mock");
    }

    #[test]
    fn cache_hit_avoids_provider_and_marks_from_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let provider = crate::provider::FnProvider::new(
            "counting",
            move |probe: &crate::probe::ReferenceProbe| {
                let calls = calls2.clone();
                let probe = probe.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let mut r = BibliographyResult::new(
                        probe.reference_id.clone(),
                        "counting",
                        VerificationStatus::Verified,
                        "q".into(),
                        None,
                    );
                    r.year = Some(2020);
                    r
                }
            },
        );
        let verifier = BibliographyVerifier::new(
            vec![NamedProvider {
                name: "counting".into(),
                provider: Arc::new(provider),
            }],
            Some(tmp.path().join("cache")),
            &VerifierConfig::default(),
        );
        let refs = vec![reference("r", "Same title")];
        let first = futures::executor::block_on(verifier.verify_references(&refs));
        let second = futures::executor::block_on(verifier.verify_references(&refs));
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(!first[0].from_cache);
        assert!(second[0].from_cache);
        // The provider ran exactly once across the two calls.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_miss_when_provider_result_is_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let provider = crate::provider::FnProvider::new(
            "flaky",
            move |probe: &crate::probe::ReferenceProbe| {
                let calls = calls2.clone();
                let probe = probe.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let mut r = BibliographyResult::new(
                        probe.reference_id.clone(),
                        "flaky",
                        VerificationStatus::Unavailable,
                        "q".into(),
                        None,
                    );
                    r.note = Some("down".into());
                    r
                }
            },
        );
        let verifier = BibliographyVerifier::new(
            vec![NamedProvider {
                name: "flaky".into(),
                provider: Arc::new(provider),
            }],
            Some(tmp.path().join("cache")),
            &VerifierConfig::default(),
        );
        let refs = vec![reference("r", "Same title")];
        let _ = futures::executor::block_on(verifier.verify_references(&refs));
        let _ = futures::executor::block_on(verifier.verify_references(&refs));
        // Unavailable is never cached, so the provider runs every time.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn original_references_never_mutated() {
        let verifier =
            BibliographyVerifier::new(vec![mock_named("mock")], None, &VerifierConfig::default());
        let refs = vec![reference("r", "T")];
        let before = refs[0].clone();
        let _ = futures::executor::block_on(verifier.verify_references(&refs));
        assert_eq!(refs[0].reference_id, before.reference_id);
        assert_eq!(refs[0].authors, before.authors);
        assert_eq!(refs[0].year, before.year);
        assert_eq!(refs[0].title, before.title);
        assert_eq!(refs[0].venue, before.venue);
        assert_eq!(refs[0].verification, before.verification);
    }
}

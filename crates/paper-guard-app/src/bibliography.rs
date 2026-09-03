//! Application wiring for the Bibliography Verification layer (M10).
//!
//! Builds the configured provider set, runs the verifier over a parsed
//! document's canonical references, and stores results in the canonical
//! RunRecord (`run.bibliography`). The layer is **additive**: reviewer
//! findings, Judge decisions, evidence, confidence, severity, and the
//! manuscript are never touched.

use std::path::PathBuf;
use std::sync::Arc;

use paper_guard_bibliography::{
    ArxivProvider, BibliographyVerifier, DiskCache, GoogleScholarProvider, MockProvider,
    NamedProvider, ReqwestArxivClient, VerifierConfig,
};
use paper_guard_core::{BibliographyResult, Document};

use crate::config::AppConfig;

/// The cache directory for one data directory
/// (`<data_dir>/bibliography-cache`).
pub fn bibliography_cache_dir(data_dir: &str) -> PathBuf {
    PathBuf::from(data_dir).join("bibliography-cache")
}

/// Run bibliography verification for a document when enabled.
///
/// Returns an empty vector when the layer is disabled or the document has no
/// references. Never returns a hard error for provider/network problems —
/// those surface as `Unavailable` results.
pub async fn run_bibliography_verification(
    config: &AppConfig,
    data_dir: &str,
    document: &Document,
) -> anyhow::Result<Vec<BibliographyResult>> {
    let section = &config.bibliography;
    if !section.effective_enabled() {
        return Ok(Vec::new());
    }
    if document.bibliography.is_empty() {
        return Ok(Vec::new());
    }

    let providers = build_providers(section)?;
    if providers.is_empty() {
        return Ok(Vec::new());
    }

    let cache_enabled = section.cache.enabled;
    let cache_dir = if cache_enabled {
        Some(bibliography_cache_dir(data_dir))
    } else {
        None
    };
    let verifier_config = VerifierConfig {
        cache_enabled,
        cache_max_entries: section.cache.max_entries,
        cache_max_bytes: section.cache.max_bytes,
    };
    let verifier = BibliographyVerifier::new(providers, cache_dir, &verifier_config);
    Ok(verifier.verify_references(&document.bibliography).await)
}

/// Parse a source path and verify its bibliography (standalone CLI path).
///
/// Returns the results plus the number of references parsed (0 when the
/// source carries no bibliography).
pub async fn verify_source(
    source_path: &str,
    config: &AppConfig,
) -> anyhow::Result<(Vec<BibliographyResult>, usize)> {
    let source = paper_guard_parser::parse_source_path(source_path).await?;
    let document = source.parsed.document;
    let count = document.bibliography.len();
    let data_dir = config.effective_data_dir();
    let results = run_bibliography_verification(config, data_dir, &document).await?;
    Ok((results, count))
}

/// Delete the bibliography response cache for a data directory.
pub fn clear_bibliography_cache(data_dir: &str) -> std::io::Result<()> {
    let cache = DiskCache::new(bibliography_cache_dir(data_dir), 1, 1024)?;
    cache.clear()
}

/// Build the provider list from the `[bibliography]` configuration.
fn build_providers(
    section: &crate::config::BibliographyConfig,
) -> anyhow::Result<Vec<NamedProvider>> {
    let mut providers = Vec::new();
    match section.provider.as_str() {
        // The mock engine is an offline test/demo double; its results are
        // clearly labelled `mock` and never a real verification.
        "mock" => providers.push(NamedProvider {
            name: "mock".into(),
            provider: Arc::new(MockProvider),
        }),
        // The arXiv engine is the only real network provider in M10. The
        // endpoint is fixed inside the bibliography crate (no SSRF surface).
        "arxiv" => {
            if section.arxiv.enabled {
                let client = ReqwestArxivClient::new(section.timeout_seconds)?;
                providers.push(NamedProvider {
                    name: "arxiv".into(),
                    provider: Arc::new(ArxivProvider::new(Arc::new(client))),
                });
            }
            // Scholar slot: honest `Unavailable` results only (never scraped).
            if section.google_scholar.enabled {
                providers.push(NamedProvider {
                    name: "google_scholar".into(),
                    provider: Arc::new(GoogleScholarProvider),
                });
            }
        }
        other => anyhow::bail!(
            "unsupported [bibliography] provider `{other}`; expected `arxiv` or `mock`"
        ),
    }
    Ok(providers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use paper_guard_core::{
        ContentHash, DocumentMeta, EvidenceState, Reference, ReferenceId, VerificationStatus,
    };

    fn doc_with_reference() -> Document {
        Document {
            document_id: "test".into(),
            meta: DocumentMeta {
                title: None,
                authors: vec![],
                abstract_text: None,
                source_format: "latex".into(),
                source_file: "test.tex".into(),
            },
            sections: vec![],
            bibliography: vec![Reference {
                reference_id: ReferenceId("smith2020".into()),
                authors: "Smith, J.".into(),
                year: Some(2020),
                title: "Some paper".into(),
                venue: String::new(),
                verification: EvidenceState::NotVerified,
            }],
            citations: vec![],
            claims: vec![],
            evidence: vec![],
            results: vec![],
            methods: vec![],
            figures: vec![],
            tables: vec![],
            equations: vec![],
            source_hash: ContentHash::default(),
        }
    }

    fn empty_doc() -> Document {
        let mut doc = doc_with_reference();
        doc.bibliography = vec![];
        doc
    }

    #[tokio::test]
    async fn disabled_by_default_returns_empty() {
        let config = AppConfig::default();
        assert!(!config.bibliography.enabled);
        let results =
            run_bibliography_verification(&config, "/tmp/nonexistent", &doc_with_reference())
                .await
                .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn mock_provider_runs_offline_and_clearly_labelled() {
        let mut config = AppConfig::default();
        config.bibliography.enabled = true;
        config.bibliography.provider = "mock".into();
        let tmp = tempfile::tempdir().unwrap();
        let results = run_bibliography_verification(
            &config,
            tmp.path().to_str().unwrap(),
            &doc_with_reference(),
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "mock");
        assert_eq!(results[0].status, VerificationStatus::Verified);
        assert!(results[0].note.as_deref().unwrap().contains("mock"));
        // The cache directory was created and used.
        assert!(bibliography_cache_dir(tmp.path().to_str().unwrap()).exists());
    }

    #[tokio::test]
    async fn empty_document_is_a_no_op() {
        let mut config = AppConfig::default();
        config.bibliography.enabled = true;
        config.bibliography.provider = "mock".into();
        let tmp = tempfile::tempdir().unwrap();
        let results =
            run_bibliography_verification(&config, tmp.path().to_str().unwrap(), &empty_doc())
                .await
                .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn unsupported_provider_is_rejected() {
        let mut config = AppConfig::default();
        config.bibliography.enabled = true;
        config.bibliography.provider = "scholar-scraper".into();
        let err = build_providers(&config.bibliography)
            .err()
            .expect("unsupported provider must be rejected");
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn clear_cache_removes_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_str().unwrap();
        // Populate a fake cache entry, then clear.
        let cache_dir = bibliography_cache_dir(dir);
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("x.json"), "{}").unwrap();
        clear_bibliography_cache(dir).unwrap();
        assert_eq!(std::fs::read_dir(&cache_dir).unwrap().count(), 0);
    }
}

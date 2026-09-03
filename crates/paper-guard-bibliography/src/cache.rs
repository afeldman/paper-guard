//! Bounded, transparent on-disk cache for bibliography responses.
//!
//! # Guarantees
//!
//! * **Bounded** — at most `max_entries` files totaling at most `max_bytes`;
//!   the oldest entries are evicted first.
//! * **Reproducible** — a cache entry is a pure function of its key
//!   (`sha256(source | query)`), so identical metadata always hits.
//! * **Deletable** — [`DiskCache::clear`] removes the whole cache directory;
//!   individual entries are plain JSON files.
//! * **Transparent** — every hit is flagged `from_cache = true` on the result.
//! * **No scientific semantics** — the cache stores raw provider outcomes as
//!   opaque bytes. It never interprets, filters, or re-orders them, and it is
//!   not a source of truth (the canonical RunRecord JSON is).
//!
//! Transient failures (`Unavailable`) are never cached.

use std::path::{Path, PathBuf};

use paper_guard_core::{BibliographyResult, VerificationStatus};
use sha2::{Digest, Sha256};

/// The on-disk response cache.
pub struct DiskCache {
    dir: PathBuf,
    max_entries: usize,
    max_bytes: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    /// Unix timestamp (seconds) — only used for bounded eviction.
    stored_at: u64,
    result: BibliographyResult,
}

impl DiskCache {
    /// Open (creating) a cache directory with the given bounds.
    pub fn new(dir: PathBuf, max_entries: usize, max_bytes: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(DiskCache {
            dir,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1024),
        })
    }

    /// The cache directory (for transparency / manual deletion).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Deterministic key from provider + query description.
    pub fn key(provider: &str, query: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(provider.as_bytes());
        hasher.update([0u8]);
        hasher.update(query.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// Look up a result. Returns `None` on miss or on any corrupt entry (the
    /// corrupt file is removed so it cannot wedge the cache).
    pub fn get(&self, key: &str) -> Option<BibliographyResult> {
        let path = self.path_for(key);
        let raw = std::fs::read_to_string(&path).ok()?;
        let entry: CacheEntry = match serde_json::from_str(&raw) {
            Ok(entry) => entry,
            Err(_) => {
                let _ = std::fs::remove_file(&path);
                return None;
            }
        };
        // Only confirmed outcomes are cached; a defensive re-check keeps an
        // old `Unavailable` entry from ever being served.
        if entry.result.status == VerificationStatus::Unavailable
            || entry.result.status == VerificationStatus::NotChecked
        {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        let mut result = entry.result;
        result.from_cache = true;
        Some(result)
    }

    /// Store a result. `Unavailable`/`NotChecked` results are never cached.
    /// Insertion enforces the entry and byte budgets (oldest evicted first).
    pub fn put(&self, key: &str, result: &BibliographyResult) {
        if result.status == VerificationStatus::Unavailable
            || result.status == VerificationStatus::NotChecked
        {
            return;
        }
        let path = self.path_for(key);
        let stored_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = CacheEntry {
            stored_at,
            result: result.clone(),
        };
        if let Ok(raw) = serde_json::to_vec(&entry) {
            let _ = std::fs::write(&path, raw);
        }
        self.enforce_bounds();
    }

    /// Delete every cached entry (and the directory contents).
    pub fn clear(&self) -> std::io::Result<()> {
        if self.dir.exists() {
            for entry in std::fs::read_dir(&self.dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        Ok(())
    }

    /// Number of cached entries (diagnostics/tests).
    pub fn len(&self) -> usize {
        std::fs::read_dir(&self.dir)
            .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count())
            .unwrap_or(0)
    }

    /// Whether the cache currently holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Current on-disk size in bytes (diagnostics/tests).
    pub fn size_bytes(&self) -> u64 {
        std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0)
    }

    fn enforce_bounds(&self) {
        // Entry budget.
        let mut entries: Vec<(PathBuf, u64)> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| {
                        let m = e.metadata().ok()?;
                        let modified = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        Some((e.path(), modified))
                    })
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by_key(|(_, modified)| *modified);
        while entries.len() > self.max_entries {
            if let Some((path, _)) = entries.first() {
                let _ = std::fs::remove_file(path);
                entries.remove(0);
            }
        }
        // Byte budget (approximate — based on file metadata after eviction).
        let mut total: u64 = entries
            .iter()
            .filter_map(|(p, _)| p.metadata().map(|m| m.len()).ok())
            .sum();
        let mut idx = 0;
        while total > self.max_bytes as u64 && idx < entries.len() {
            if let Ok(meta) = entries[idx].0.metadata() {
                total = total.saturating_sub(meta.len());
                let _ = std::fs::remove_file(&entries[idx].0);
            }
            idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{
        BibliographyMismatch, EvidenceState, Reference, ReferenceId, VerificationStatus,
    };

    fn sample_result(status: VerificationStatus) -> BibliographyResult {
        let mut r = BibliographyResult::new(
            "ref1".into(),
            "arxiv",
            status,
            "query".into(),
            Some("Smith, J. (2020). Paper.".into()),
        );
        r.mismatches.push(BibliographyMismatch::new(
            "year",
            Some("2020".into()),
            Some("2021".into()),
        ));
        r
    }

    fn temp_cache() -> (tempfile::TempDir, DiskCache) {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(tmp.path().join("cache"), 10, 1024 * 1024).unwrap();
        (tmp, cache)
    }

    #[test]
    fn cache_miss_then_hit() {
        let (_tmp, cache) = temp_cache();
        let key = DiskCache::key("arxiv", "query");
        assert!(cache.get(&key).is_none());
        let result = sample_result(VerificationStatus::Verified);
        cache.put(&key, &result);
        let hit = cache.get(&key).unwrap();
        assert!(hit.from_cache);
        assert_eq!(hit.status, VerificationStatus::Verified);
        assert_eq!(hit.reference_id, "ref1");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_key_is_deterministic_and_source_scoped() {
        let a = DiskCache::key("arxiv", "some query");
        let b = DiskCache::key("arxiv", "some query");
        let c = DiskCache::key("mock", "some query");
        let d = DiskCache::key("arxiv", "other query");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn unavailable_never_cached() {
        let (_tmp, cache) = temp_cache();
        let key = DiskCache::key("arxiv", "q");
        cache.put(&key, &sample_result(VerificationStatus::Unavailable));
        assert_eq!(cache.len(), 0);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn not_found_is_cached_but_flagged() {
        let (_tmp, cache) = temp_cache();
        let key = DiskCache::key("arxiv", "q");
        cache.put(&key, &sample_result(VerificationStatus::NotFound));
        let hit = cache.get(&key).unwrap();
        assert!(hit.from_cache);
        assert_eq!(hit.status, VerificationStatus::NotFound);
    }

    #[test]
    fn corrupt_entry_is_removed_not_served() {
        let (_tmp, cache) = temp_cache();
        let key = DiskCache::key("arxiv", "q");
        std::fs::write(cache.path_for(&key), "not json").unwrap();
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_is_bounded_by_entries_and_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(tmp.path().join("c"), 3, 4 * 1024 * 1024).unwrap();
        for i in 0..5 {
            let mut r = sample_result(VerificationStatus::Verified);
            r.reference_id = format!("ref{i}");
            let k = DiskCache::key("arxiv", &format!("query-{i}"));
            cache.put(&k, &r);
        }
        assert!(cache.len() <= 3, "len={}", cache.len());
        // The oldest key was evicted first.
        assert!(cache.get(&DiskCache::key("arxiv", "query-0")).is_none());
        assert!(cache.get(&DiskCache::key("arxiv", "query-4")).is_some());

        let tiny = DiskCache::new(tmp.path().join("tiny"), 100, 2048).unwrap();
        for i in 0..8 {
            let r = sample_result(VerificationStatus::Verified);
            let k = DiskCache::key("arxiv", &format!("big-{i}"));
            tiny.put(&k, &r);
        }
        assert!(
            tiny.size_bytes() <= 2048 + 4096,
            "size={}",
            tiny.size_bytes()
        );
    }

    #[test]
    fn clear_removes_everything() {
        let (_tmp, cache) = temp_cache();
        for i in 0..3 {
            cache.put(
                &DiskCache::key("arxiv", &format!("q{i}")),
                &sample_result(VerificationStatus::Verified),
            );
        }
        assert_eq!(cache.len(), 3);
        cache.clear().unwrap();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn reference_probe_roundtrip_cache_semantics() {
        // Cache entries are plain JSON: the canonical RunRecord remains the
        // single source of truth and the cache never interprets semantics.
        use crate::provider::BibliographyProvider;
        let (_tmp, cache) = temp_cache();
        let reference = Reference {
            reference_id: ReferenceId("smith2020".into()),
            authors: "Smith, J.".into(),
            year: Some(2020),
            title: "Paper".into(),
            venue: String::new(),
            verification: EvidenceState::NotVerified,
        };
        let probe = crate::probe::ReferenceProbe::from_reference(&reference);
        let provider = crate::mock::MockProvider;
        let result = futures::executor::block_on(BibliographyProvider::verify(&provider, &probe));
        let key = DiskCache::key("mock", &probe.query_description());
        cache.put(&key, &result);
        assert!(cache.get(&key).is_some());
    }
}

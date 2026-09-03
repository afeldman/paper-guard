//! Bounded, transparent on-disk cache for bibliography responses.
//!
//! # Guarantees
//!
//! * **Bounded** — at most `max_entries` files totaling at most `max_bytes`;
//!   the oldest entries (by canonical insertion timestamp) are evicted first,
//!   deterministically and independent of filesystem enumeration order.
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
    /// Canonical insertion timestamp: nanoseconds since the Unix epoch, read
    /// from the wall clock at `put` time. Bounded eviction orders entries by
    /// this value (oldest first); ties are broken by the deterministic
    /// filename ordering (see [`DiskCache::enforce_bounds`]). Entries written
    /// before the unit change carry second-precision values, which sort as
    /// older than any nanosecond-precision entry — an upgrade therefore
    /// evicts legacy entries first, which is safe for a disposable cache.
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
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
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
        // Enumerate every file once. Eviction ordering uses each entry's
        // canonical insertion timestamp (`stored_at`, captured at `put` time)
        // — never the filesystem mtime and never `read_dir` order. Both are
        // platform-dependent and routinely tie for entries written within the
        // same clock tick, which previously made "oldest first" eviction
        // nondeterministic (the stable sort then fell back to filesystem
        // enumeration order). Corrupt or unreadable files cannot be parsed
        // and sort as the oldest entries (`stored_at = 0`), so they are the
        // first eviction candidates and can never wedge the cache; `get`
        // additionally removes them on access (documented behavior).
        let mut entries: Vec<(PathBuf, u64, u64)> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| {
                        let path = e.path();
                        match std::fs::read_to_string(&path) {
                            Ok(raw) => {
                                let stored_at = serde_json::from_str::<CacheEntry>(&raw)
                                    .map(|entry| entry.stored_at)
                                    .unwrap_or(0);
                                (path, stored_at, raw.len() as u64)
                            }
                            Err(_) => {
                                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                                (path, 0, size)
                            }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Deterministic total order: canonical insertion time first; the
        // content-addressed filename second as a stable, platform-independent
        // tie-break. Entries written within the same clock tick are
        // indistinguishable by timestamp alone; the filename keeps eviction
        // reproducible across filesystems without pretending to reconstruct
        // sub-tick insertion order.
        entries.sort_by(|a, b| (a.1, a.0.file_name()).cmp(&(b.1, b.0.file_name())));

        // Entry budget: evict the oldest entries until the count fits.
        while entries.len() > self.max_entries {
            if let Some((path, _, _)) = entries.first() {
                let _ = std::fs::remove_file(path);
                entries.remove(0);
            }
        }
        // Byte budget (approximate): evict the oldest remaining entries until
        // the total serialized size fits.
        let mut total: u64 = entries.iter().map(|(_, _, size)| *size).sum();
        let mut idx = 0;
        while total > self.max_bytes as u64 && idx < entries.len() {
            total = total.saturating_sub(entries[idx].2);
            let _ = std::fs::remove_file(&entries[idx].0);
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

    /// Write an entry directly with a caller-controlled `stored_at`, bypassing
    /// `put` (which would assign the current wall clock and auto-enforce).
    fn write_entry_at(cache: &DiskCache, key: &str, stored_at: u64) {
        let entry = CacheEntry {
            stored_at,
            result: sample_result(VerificationStatus::Verified),
        };
        std::fs::write(cache.path_for(key), serde_json::to_vec(&entry).unwrap()).unwrap();
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
    fn entry_budget_evicts_oldest_by_stored_at_not_by_name_or_fs_order() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(tmp.path().join("c"), 3, 4 * 1024 * 1024).unwrap();
        // stored_at increases strictly with the key index; filenames (sha256)
        // are unrelated to that order. Eviction can only keep the newest three
        // if it uses the canonical stored_at — never mtime, read_dir order,
        // or the filename.
        let keys: Vec<String> = (0..5u64)
            .map(|i| {
                let k = DiskCache::key("arxiv", &format!("query-{i}"));
                write_entry_at(&cache, &k, 1_000 + i);
                k
            })
            .collect();
        cache.enforce_bounds();
        assert_eq!(cache.len(), 3);
        assert!(cache.get(&keys[0]).is_none(), "oldest entry not evicted");
        assert!(cache.get(&keys[1]).is_none(), "second-oldest not evicted");
        for k in &keys[2..] {
            assert!(cache.get(k).is_some(), "youngest entry {k} evicted");
        }
    }

    #[test]
    fn eviction_timestamp_ties_break_deterministically_by_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let make = |name: &str| {
            let cache = DiskCache::new(tmp.path().join(name), 3, 4 * 1024 * 1024).unwrap();
            let mut keys: Vec<String> = (0..5)
                .map(|i| DiskCache::key("arxiv", &format!("query-{i}")))
                .collect();
            for k in &keys {
                // Identical stored_at: a legitimate clock-tick collision.
                write_entry_at(&cache, k, 7_777);
            }
            keys.sort(); // filename order == documented tie-break order
            cache.enforce_bounds();
            (cache, keys)
        };
        // Two caches with equivalent contents must evict identically, and the
        // survivor set must be the exact deterministic one (no dependence on
        // filesystem enumeration order).
        let (a, keys_a) = make("a");
        let (b, keys_b) = make("b");
        assert_eq!(a.len(), 3);
        assert_eq!(b.len(), 3);
        for (cache, keys) in [(&a, &keys_a), (&b, &keys_b)] {
            for k in &keys[..2] {
                assert!(
                    cache.get(k).is_none(),
                    "entry {k} should have been evicted (tie-break evicts smallest filenames first)"
                );
            }
            for k in &keys[2..] {
                assert!(
                    cache.get(k).is_some(),
                    "entry {k} should have survived (tie-break keeps largest filenames)"
                );
            }
        }
    }

    #[test]
    fn byte_budget_evicts_oldest_entries_first() {
        let tmp = tempfile::tempdir().unwrap();
        // Generous entry budget; byte budget fits only ~2-3 equal-size entries.
        let cache = DiskCache::new(tmp.path().join("c"), 100, 1024).unwrap();
        let keys: Vec<String> = (0..8u64)
            .map(|i| {
                let k = DiskCache::key("arxiv", &format!("big-{i}"));
                write_entry_at(&cache, &k, 10_000 + i);
                k
            })
            .collect();
        cache.enforce_bounds();
        // All entries have identical serialized size (same content), so the
        // byte budget evicts a strict prefix of the insertion order and the
        // survivors form a contiguous suffix.
        let present: Vec<usize> = (0..8).filter(|&i| cache.get(&keys[i]).is_some()).collect();
        assert!(!present.is_empty(), "byte budget evicted every entry");
        assert!(present.len() < 8, "byte budget did not evict anything");
        let first_survivor = present[0];
        assert!(
            present.iter().copied().eq(first_survivor..8),
            "survivors {present:?} are not the contiguous suffix starting at {first_survivor}"
        );
        for (j, k) in keys.iter().enumerate().take(first_survivor) {
            assert!(
                cache.get(k).is_none(),
                "entry {j} (older than the oldest survivor) was not evicted"
            );
        }
        assert!(
            cache.size_bytes() <= 1024 + 4096,
            "size={}",
            cache.size_bytes()
        );
    }

    #[test]
    fn corrupt_entry_is_evicted_first_when_over_budget() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(tmp.path().join("c"), 4, 4 * 1024 * 1024).unwrap();
        let corrupt_key = DiskCache::key("arxiv", "corrupt");
        std::fs::write(cache.path_for(&corrupt_key), "not json").unwrap();
        for i in 0..4u64 {
            let k = DiskCache::key("arxiv", &format!("valid-{i}"));
            write_entry_at(&cache, &k, 20_000 + i);
        }
        // 5 files against a budget of 4: exactly one eviction must happen, and
        // it must remove the corrupt entry (unparseable => stored_at = 0 =>
        // oldest) rather than a valid one.
        cache.enforce_bounds();
        assert_eq!(cache.len(), 4);
        assert!(
            !cache.path_for(&corrupt_key).exists(),
            "corrupt entry was not the eviction candidate"
        );
        for i in 0..4u64 {
            let k = DiskCache::key("arxiv", &format!("valid-{i}"));
            assert!(cache.get(&k).is_some(), "valid entry {k} was evicted");
        }
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

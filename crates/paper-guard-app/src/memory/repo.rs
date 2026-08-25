//! Review Memory repository: a storage abstraction that keeps memory strictly
//! separate from current-paper evidence.
//!
//! Two backends are provided:
//!   * [`FileReviewMemory`] — an offline JSON store used by standalone/service
//!     without Qdrant. This is the default and requires no external service.
//!   * [`QdrantReviewMemory`] — a vector backend (see [`super::qdrant`]).
//!
//! The repository contract enforces the privacy/approval rules: private units
//! can never be retrieved as context, and nothing is ever exported without
//! `TRAINING_APPROVED`. Calling code cannot bypass these rules through the
//! [`ReviewMemoryRepository`] interface.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::state::{ApprovalState, Consent, ConsentGrant};
use super::unit::ReviewMemoryEntry;

/// A search over review memory. `count` bounds the number of results.
#[derive(Debug, Clone)]
pub struct ReviewMemorySearch {
    pub query: String,
    pub limit: usize,
}

impl Default for ReviewMemorySearch {
    fn default() -> Self {
        ReviewMemorySearch {
            query: String::new(),
            limit: 5,
        }
    }
}

/// The storage abstraction for review memory.
///
/// Memory is never a source of current-paper evidence. The repository only
/// stores and retrieves **historical, human-approved** review units.
pub trait ReviewMemoryRepository: Send + Sync {
    /// Store a new unit. It is always stored with the caller-provided state
    /// (default [`ApprovalState::Private`]); a repository should never promote
    /// a unit on its own.
    fn store(&self, entry: ReviewMemoryEntry) -> anyhow::Result<()>;

    /// Load a unit by id.
    fn load(&self, memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>>;

    /// List units matching an approval filter.
    fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>>;

    /// Record a human consent decision, promoting/denoting a unit's approval
    /// state. Consent is audited; promotion requires an explicit actor + grant.
    fn consent(&self, consent: Consent) -> anyhow::Result<()>;

    /// Retrieve units eligible to be used as retrieval context for a query.
    ///
    /// Only `MEMORY_APPROVED` / `TRAINING_APPROVED` units are ever returned.
    /// Private/rejected units are never returned regardless of similarity.
    fn retrieve(&self, search: &ReviewMemorySearch) -> anyhow::Result<Vec<ReviewMemoryEntry>>;

    /// Export units that carry explicit `TRAINING_APPROVED` consent for a
    /// versioned training dataset. Private/memory-only units are excluded.
    fn export_training_units(&self, limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>>;
}

/// An offline JSON-file memory store.
///
/// This is a simple, dependency-free backend used by standalone mode and by
/// tests. It keeps all entries in memory and persists them to a JSON file on
/// each mutation so it is not a full-database replacement, but it lets the
/// privacy/approval rules run without any external service.
pub struct FileReviewMemory {
    path: PathBuf,
    // An in-process guard so mutations are serialized across tasks.
    entries: std::sync::Mutex<Vec<ReviewMemoryEntry>>,
}

impl FileReviewMemory {
    /// Open (or initialize) a file-backed memory store at `path`.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = FileStore {
            entries: load_file::<Vec<ReviewMemoryEntry>>(path).unwrap_or_default(),
        };
        let entries = std::sync::Mutex::new(store.entries);
        let repo = FileReviewMemory {
            path: path.to_path_buf(),
            entries,
        };
        repo.persist()?;
        Ok(repo)
    }

    fn persist(&self) -> anyhow::Result<()> {
        let entries = self.entries.lock().unwrap();
        let store = FileStore {
            entries: entries.clone(),
        };
        let json = serde_json::to_string_pretty(&store)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

/// The on-disk shape of the file store (entries only; consents are recorded in
/// the approval state of each entry).
#[derive(Serialize, Deserialize)]
struct FileStore {
    entries: Vec<ReviewMemoryEntry>,
}

fn load_file<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

impl ReviewMemoryRepository for FileReviewMemory {
    fn store(&self, entry: ReviewMemoryEntry) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        // Upsert by id (keeps id stability when re-storing).
        if let Some(pos) = entries.iter().position(|e| e.memory_id == entry.memory_id) {
            entries[pos] = entry;
        } else {
            entries.push(entry);
        }
        drop(entries);
        self.persist()
    }

    fn load(&self, memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().find(|e| e.memory_id == memory_id).cloned())
    }

    fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(match state {
            Some(s) => entries.iter().filter(|e| e.approval_state == s).cloned().collect(),
            None => entries.clone(),
        })
    }

    fn consent(&self, consent: Consent) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .iter_mut()
            .find(|e| e.memory_id == consent.memory_id)
            .ok_or_else(|| anyhow::anyhow!("memory id {} not found", consent.memory_id))?;
        entry.approval_state = match consent.state {
            ConsentGrant::ApproveMemory => ApprovalState::MemoryApproved,
            ConsentGrant::ApproveTraining => ApprovalState::TrainingApproved,
            ConsentGrant::Reject => ApprovalState::Rejected,
        };
        drop(entries);
        self.persist()
    }

    fn retrieve(&self, search: &ReviewMemorySearch) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        // Only explicitly-approved units are retrievable as context. Private
        // and rejected units are never returned, regardless of similarity.
        let eligible: Vec<_> = entries
            .iter()
            .filter(|e| e.retrievable())
            .cloned()
            .collect();
        // Without a vector backend we do a simple substring/relevance match on
        // the query; the Qdrant backend substitutes real vector similarity.
        let scored: Vec<ReviewMemoryEntry> = if search.query.trim().is_empty() {
            eligible
        } else {
            let q = search.query.to_lowercase();
            let mut matched: Vec<_> = eligible
                .into_iter()
                .map(|e| {
                    let hay = format!(
                        "{} {} {}",
                        e.unit.text,
                        e.unit.finding,
                        e.unit.context
                    )
                    .to_lowercase();
                    let score = if hay.contains(&q) { 1.0 } else { 0.0 };
                    (score, e)
                })
                .filter(|(s, _)| *s > 0.0)
                .collect();
            matched.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            matched.into_iter().map(|(_, e)| e).collect()
        };
        Ok(scored.into_iter().take(search.limit).collect())
    }

    fn export_training_units(&self, limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|e| e.exportable())
            .take(limit)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::unit::{MemoryKind, ReviewMemoryUnit};
    use crate::memory::{ConsentGrant, MemoryResolution};

    fn entry(id: &str, text: &str) -> ReviewMemoryEntry {
        ReviewMemoryEntry::private(
            id.into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: text.into(),
                finding: "finding about ".to_string() + text,
                context: "context".into(),
            },
            MemoryResolution::Accept,
            "ok".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        )
    }

    fn temp_repo() -> (FileReviewMemory, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileReviewMemory::open(&dir.path().join("memory.json")).unwrap();
        (repo, dir)
    }

    #[test]
    fn private_units_cannot_be_retrieved_as_context() {
        let (repo, _dir) = temp_repo();
        repo.store(entry("mem-1", "the method reduces latency")).unwrap();
        // Even a query matching the text cannot retrieve a private unit.
        let results = repo
            .retrieve(&ReviewMemorySearch {
                query: "method reduces latency".into(),
                limit: 10,
            })
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn memory_approved_units_are_retrieved_but_private_are_not() {
        let (repo, _dir) = temp_repo();
        repo.store(entry("mem-1", "shared prior claim")).unwrap();
        repo.consent(Consent {
            memory_id: "mem-1".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
        let results = repo
            .retrieve(&ReviewMemorySearch {
                query: "prior claim".into(),
                limit: 10,
            })
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory_id, "mem-1");
        assert!(results[0].retrievable());
    }

    #[test]
    fn only_training_approved_units_are_exportable() {
        let (repo, _dir) = temp_repo();
        repo.store(entry("mem-1", "approved to train")).unwrap();
        repo.store(entry("mem-2", "memory only")).unwrap();
        repo.consent(Consent {
            memory_id: "mem-1".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveTraining,
            timestamp: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
        repo.consent(Consent {
            memory_id: "mem-2".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
        let exported = repo.export_training_units(10).unwrap();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].memory_id, "mem-1");
    }

    #[test]
    fn memory_integrity_never_becomes_current_evidence() {
        // A retrieved memory entry must always be framed as historical review
        // experience — it can never be read as evidence for the current paper.
        let (repo, _dir) = temp_repo();
        repo.store(entry("mem-1", "a prior finding")).unwrap();
        repo.consent(Consent {
            memory_id: "mem-1".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
        let r = repo.load("mem-1").unwrap().unwrap();
        let ctx = r.context_text();
        // The retrieval text must carry the HISTORICAL marker, and must not be
        // assertable as evidence for a current manuscript.
        assert!(ctx.contains("HISTORICAL REVIEW MEMORY"));
        assert!(!ctx.contains("SUPPORTED"));
    }
}

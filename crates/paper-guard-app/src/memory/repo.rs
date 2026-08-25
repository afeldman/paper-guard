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

use super::embedding::{try_cosine, EmbeddingProvider};
use super::state::{ApprovalState, Consent, ConsentGrant};
use super::unit::ReviewMemoryEntry;

/// The authorization context of a retrieval call.
///
/// Memory access is scope-aware (see [`ReviewMemoryEntry::accessible_to`]).
/// A retrieval never returns a unit the caller could not access even if the
/// vector similarity would otherwise match.
#[derive(Debug, Clone, Default)]
pub struct MemoryAuthzContext {
    /// The caller's owner identity (never a secret). Empty means "no owner",
    /// which grants access to nothing (a PRIVATE-scope unit needs its owner;
    /// a TEAM-scope unit needs a matching team).
    pub owner: String,
    /// The caller's team identity, if any. Only TEAM-scope units with a
    /// matching `team_id` are accessible.
    pub team: Option<String>,
}

impl MemoryAuthzContext {
    /// A fully-open context (used by the CLI when operating on the *local*
    /// store as the owner of all its own entries).
    pub fn owner_of(owner: &str) -> Self {
        MemoryAuthzContext {
            owner: owner.to_string(),
            team: None,
        }
    }
}

/// A search over review memory. `limit` bounds the number of results.
#[derive(Debug, Clone)]
pub struct ReviewMemorySearch {
    pub query: String,
    pub limit: usize,
    /// Minimum cosine similarity (0..=1) for vector retrieval. 0 disables the
    /// threshold (everything above 0 is eligible).
    pub min_similarity: f32,
    /// Optional category filter (e.g. `unsupported_claim`). Empty = all.
    pub category: String,
    /// Optional reviewer-kind filter (e.g. `evidence`). Empty = all.
    pub reviewer_kind: String,
    /// The authorization context used to filter access.
    pub authz: MemoryAuthzContext,
}

impl Default for ReviewMemorySearch {
    fn default() -> Self {
        ReviewMemorySearch {
            query: String::new(),
            limit: 5,
            min_similarity: 0.0,
            category: String::new(),
            reviewer_kind: String::new(),
            authz: MemoryAuthzContext::default(),
        }
    }
}

/// A scored retrieval result (entry + similarity). The score is informational;
/// it never changes the entry's evidence status.
#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub entry: ReviewMemoryEntry,
    pub similarity: f32,
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
    /// Only `MEMORY_APPROVED` / `TRAINING_APPROVED` units that the caller is
    /// **authorized to access** are ever returned. Private/rejected units are
    /// never returned regardless of similarity, and a unit outside the
    /// caller's scope (owner/team) is never returned either.
    fn retrieve(&self, search: &ReviewMemorySearch) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(self
            .retrieve_scored(search, None)?
            .into_iter()
            .map(|h| h.entry)
            .collect())
    }

    /// Retrieve units with their similarity score (informational).
    ///
    /// `query_embedding` is the caller-provided embedding of the retrieval
    /// query (via the configured embedding provider). Passing an embedding
    /// enables true semantic scoring on backends that store vectors; passing
    /// `None` falls back to a benign relevance/substring match on the file
    /// backend and returns the authorized approved units (never fabricated).
    fn retrieve_scored(
        &self,
        search: &ReviewMemorySearch,
        query_embedding: Option<&[f32]>,
    ) -> anyhow::Result<Vec<MemoryHit>>;

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
        let entries = load_file::<FileStore>(path)
            .map(|store| store.entries)
            .unwrap_or_default();
        let repo = FileReviewMemory {
            path: path.to_path_buf(),
            entries: std::sync::Mutex::new(entries),
        };
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
            Some(s) => entries
                .iter()
                .filter(|e| e.approval_state == s)
                .cloned()
                .collect(),
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

    fn retrieve_scored(
        &self,
        search: &ReviewMemorySearch,
        query_embedding: Option<&[f32]>,
    ) -> anyhow::Result<Vec<MemoryHit>> {
        let entries = self.entries.lock().unwrap();
        // Filter by approval AND authorization BEFORE any scoring, so no unit
        // a caller cannot access can be returned. Private/rejected units are
        // never retrievable regardless of similarity.
        let eligible: Vec<ReviewMemoryEntry> = entries
            .iter()
            .filter(|e| e.retrievable())
            .filter(|e| e.accessible_to(&search.authz.owner, search.authz.team.as_deref()))
            .filter(|e| search.category.is_empty() || e.unit.category == search.category)
            .filter(|e| {
                search.reviewer_kind.is_empty() || e.unit.reviewer_kind == search.reviewer_kind
            })
            .cloned()
            .collect();

        if search.query.trim().is_empty() {
            // No query: stable order, bounded by limit.
            return Ok(eligible
                .into_iter()
                .take(search.limit)
                .map(|entry| MemoryHit {
                    entry,
                    similarity: 0.0,
                })
                .collect());
        }

        // Semantic scoring: use the caller-provided query embedding when given
        // (the configured embedding provider's output); otherwise fall back to
        // a benign substring match for stores that predate vectorization.
        let query_vec = query_embedding.map(|v| v.to_vec()).or_else(|| {
            let q = search.query.to_lowercase();
            if q.trim().is_empty() {
                None
            } else {
                // Deterministic mock hash-space vector when no provider vector
                // was supplied (covers offline stores + older tests).
                futures::executor::block_on(
                    super::embedding::MockEmbeddingProvider::new().embed(&search.query),
                )
                .ok()
            }
        });

        let mut scored: Vec<MemoryHit> = eligible
            .into_iter()
            .map(|entry| {
                let similarity = match (entry.embedding.as_deref(), query_vec.as_deref()) {
                    (Some(emb), Some(qv)) => try_cosine(emb, qv),
                    _ => {
                        // No vector path: benign substring relevance.
                        let hay = format!(
                            "{} {} {}",
                            entry.unit.text, entry.unit.finding, entry.unit.context
                        )
                        .to_lowercase();
                        let q = search.query.to_lowercase();
                        if hay.contains(&q) {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };
                MemoryHit { entry, similarity }
            })
            .filter(|h| h.similarity >= search.min_similarity.max(0.0))
            .filter(|h| h.similarity > 0.0)
            .collect();
        scored.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(search.limit);
        Ok(scored)
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
        let mut e = ReviewMemoryEntry::private(
            id.into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: text.into(),
                finding: "finding about ".to_string() + text,
                context: "context".into(),
                claim_context: text.into(),
                evidence_context: String::new(),
                category: "missing_evidence".into(),
            },
            MemoryResolution::Accept,
            "ok".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        );
        e.owner_id = "alice".into();
        e
    }

    fn temp_repo() -> (FileReviewMemory, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileReviewMemory::open(&dir.path().join("memory.json")).unwrap();
        (repo, dir)
    }

    /// A search scoped to the owner of the test entries.
    fn authz_search(query: &str, limit: usize) -> ReviewMemorySearch {
        ReviewMemorySearch {
            query: query.into(),
            limit,
            authz: MemoryAuthzContext::owner_of("alice"),
            ..Default::default()
        }
    }

    #[test]
    fn private_units_cannot_be_retrieved_as_context() {
        let (repo, _dir) = temp_repo();
        repo.store(entry("mem-1", "the method reduces latency"))
            .unwrap();
        // Even a query matching the text cannot retrieve a private unit.
        let results = repo
            .retrieve(&authz_search("method reduces latency", 10))
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
        let results = repo.retrieve(&authz_search("prior claim", 10)).unwrap();
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

    #[test]
    fn reopening_a_file_store_preserves_entries() {
        // Regression: reopening an existing file-backed store must NOT lose
        // entries. The on-disk shape is a {entries:[...]} object; loading it as
        // a bare Vec would drop everything on the next persist.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.json");
        {
            let repo = FileReviewMemory::open(&path).unwrap();
            repo.store(entry("mem-persist", "persisted claim")).unwrap();
            repo.consent(Consent {
                memory_id: "mem-persist".into(),
                actor: "human".into(),
                state: ConsentGrant::ApproveMemory,
                timestamp: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
        }
        // Reopen in a fresh instance (simulates the CLI reading what the
        // service persisted, or a restart) and confirm the entry survives.
        let repo = FileReviewMemory::open(&path).unwrap();
        let entries = repo.list(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].memory_id, "mem-persist");
        assert_eq!(entries[0].approval_state, ApprovalState::MemoryApproved);
        // And it is retrievable as context.
        let found = repo.retrieve(&authz_search("persisted claim", 10)).unwrap();
        assert_eq!(found.len(), 1);
    }
}

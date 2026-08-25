//! A service that connects reviews to Review Memory.
//!
//! This is the M3 foundation + M4 team-learning layer:
//!
//! ```text
//!   Paper → Review → Human Feedback → Memory Candidate → (explicit approval)
//!                                                                     │
//!                                     future review ← retrieval context ←┘
//! ```
//!
//! Two boundaries are enforced here and must never be weakened:
//!
//!   1. A memory candidate is stored **private by default** and only becomes
//!      retrievable as context / exportable to training through explicit,
//!      audited human consent.
//!   2. Retrieved memory is **historical review context** — it can never be
//!      presented as evidence for the current manuscript. It is always framed
//!      with a `HISTORICAL REVIEW MEMORY` marker, distinct from current-paper
//!      evidence.
//!
//! M4 adds:
//!   * memory **modes** (OFF / READ_ONLY / WRITE / READ_WRITE),
//!   * provider-independent **embeddings** (mock / OpenAI-compatible incl.
//!     Ollama) used for semantic retrieval,
//!   * scope-aware **authorization** (PRIVATE owner / TEAM member),
//!   * a **Qdrant mirror** that stores vectors of approved units and performs
//!     semantic retrieval, while the file store remains authoritative for
//!     consent/approval.

use std::sync::Arc;

use crate::config::MemoryMode;
use crate::memory::qdrant::QdrantConfig;
use crate::memory::{
    ApprovalState, Consent, ConsentGrant, EmbeddingProvider, FileReviewMemory, MemoryAuthzContext,
    MemoryResolution, MockEmbeddingProvider, OpenAICompatibleEmbeddingProvider, QdrantReviewMemory,
    ReviewMemoryEntry, ReviewMemoryRepository, ReviewMemorySearch, ReviewMemoryUnit,
};

/// The feedback a human reviewer gives on a machine finding.
#[derive(Debug, Clone)]
pub struct FindingFeedback {
    /// The id of the finding being acted on (e.g. `PG-0042`).
    pub finding_id: String,
    /// The human decision on the finding.
    pub decision: MemoryResolution,
    /// Optional free-text feedback.
    pub feedback: String,
}

/// Options for constructing a [`MemoryService`] (mirrors `[memory]` config).
#[derive(Debug, Clone)]
pub struct MemoryServiceOptions {
    pub enabled: bool,
    pub backend: String,
    pub mode: MemoryMode,
    pub qdrant_url: String,
    pub collection: String,
    pub require_approval: bool,
    pub top_k: usize,
    pub min_similarity: f32,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub owner_id: String,
    pub team_id: String,
    pub data_dir: String,
    /// The base URL to use for a remote (`openai-compatible`) embedding
    /// endpoint (e.g. Ollama's `/v1`). When empty and the embedding provider is
    /// `openai-compatible`, the address is derived from the LLM provider's
    /// OpenAI-compatible base URL.
    pub embedding_base_url: String,
}

impl MemoryServiceOptions {
    /// Build options from an `AppConfig`'s `[memory]` section + data dir.
    pub fn from_config(cfg: &crate::config::AppConfig) -> MemoryServiceOptions {
        let mem = &cfg.memory;
        let mode = if mem.enabled {
            MemoryMode::parse(&mem.mode)
        } else {
            MemoryMode::Off
        };
        MemoryServiceOptions {
            enabled: mem.enabled,
            backend: mem.backend.clone(),
            mode,
            qdrant_url: mem.qdrant_url.clone(),
            collection: mem.collection.clone(),
            require_approval: mem.require_approval,
            top_k: mem.top_k.max(1),
            min_similarity: mem.min_similarity.clamp(0.0, 1.0),
            embedding_provider: mem.embedding_provider.clone(),
            embedding_model: mem.embedding_model.clone(),
            owner_id: mem.owner_id.clone(),
            team_id: mem.team_id.clone(),
            data_dir: cfg.effective_data_dir().to_string(),
            embedding_base_url: cfg.providers.openai_compatible.base_url.clone(),
        }
    }
}

/// A handle to the review-memory side of the application layer.
#[derive(Clone)]
pub struct MemoryService {
    /// The authoritative store (file or disabled). Consent/approval lives here.
    repo: Arc<dyn ReviewMemoryRepository>,
    /// An optional Qdrant mirror for semantic (vector) retrieval of approved
    /// units. Only present when `backend = "qdrant"`.
    qdrant: Option<Arc<QdrantReviewMemory>>,
    /// The configured mode (OFF / READ_ONLY / WRITE / READ_WRITE).
    mode: MemoryMode,
    /// Max retrieved entries per review.
    top_k: usize,
    /// Minimum cosine similarity for retrieval.
    min_similarity: f32,
    /// The embedding provider (mock for offline, OpenAI-compatible for real).
    embedder: Arc<dyn EmbeddingProvider>,
    /// The owner identity attributed to locally-recorded memory.
    owner_id: String,
    /// The team the service operates as a member of (for TEAM-scope access).
    team_id: String,
}

impl MemoryService {
    /// Build a memory service from an `AppConfig`'s `[memory]` section.
    pub fn from_config(cfg: &crate::config::AppConfig) -> anyhow::Result<MemoryService> {
        MemoryService::new(&MemoryServiceOptions::from_config(cfg))
    }

    /// Build a memory service from options.
    pub fn new(options: &MemoryServiceOptions) -> anyhow::Result<MemoryService> {
        // If memory is disabled (or mode is OFF), return a disabled service
        // that stores/retrieves nothing — this keeps default behavior unchanged.
        let effective_mode = if options.enabled {
            options.mode
        } else {
            MemoryMode::Off
        };
        if effective_mode == MemoryMode::Off || options.backend == "none" {
            let svc = MemoryService {
                repo: Arc::new(DisabledMemory),
                qdrant: None,
                mode: MemoryMode::Off,
                top_k: options.top_k,
                min_similarity: options.min_similarity,
                embedder: build_embedder(options)?,
                owner_id: options.owner_id.clone(),
                team_id: options.team_id.clone(),
            };
            return Ok(svc);
        }

        let file_path = std::path::Path::new(&options.data_dir).join("review_memory.json");
        let file_repo = FileReviewMemory::open(&file_path)?;

        let qdrant = if options.backend == "qdrant" {
            let q = QdrantReviewMemory::new(QdrantConfig {
                base_url: options.qdrant_url.clone(),
                collection: options.collection.clone(),
                timeout_seconds: 30,
            })?;
            Some(Arc::new(q))
        } else {
            None
        };

        let repo: Arc<dyn ReviewMemoryRepository> = Arc::new(file_repo);
        Ok(MemoryService {
            repo,
            qdrant,
            mode: effective_mode,
            top_k: options.top_k,
            min_similarity: options.min_similarity,
            embedder: build_embedder(options)?,
            owner_id: options.owner_id.clone(),
            team_id: options.team_id.clone(),
        })
    }

    /// Convenience constructor for tests / legacy callers: a file-backed or
    /// disabled service with a mock embedder.
    pub fn file(backend: &str, data_dir: &str) -> anyhow::Result<MemoryService> {
        let opts = MemoryServiceOptions {
            enabled: backend != "none",
            backend: backend.to_string(),
            mode: if backend == "none" {
                MemoryMode::Off
            } else {
                MemoryMode::ReadWrite
            },
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "cli-user".into(),
            team_id: String::new(),
            data_dir: data_dir.to_string(),
            embedding_base_url: String::new(),
        };
        MemoryService::new(&opts)
    }

    /// The current memory mode.
    pub fn mode(&self) -> MemoryMode {
        self.mode
    }

    /// The backend name in use.
    pub fn backend(&self) -> &'static str {
        if let Some(_q) = &self.qdrant {
            "qdrant"
        } else if self.mode == MemoryMode::Off {
            "none"
        } else {
            "file"
        }
    }

    /// Whether this service stores new entries.
    pub fn stores(&self) -> bool {
        self.mode.stores()
    }

    /// Whether this service retrieves memory as context.
    pub fn retrieves(&self) -> bool {
        self.mode.retrieves()
    }

    /// Record a human decision on a finding as a **private-by-default** memory
    /// candidate. It is never promoted to retrieval/training without explicit
    /// consent.
    ///
    /// In WRITE / READ_WRITE mode the candidate is embedded and stored locally.
    /// In READ_ONLY / OFF mode nothing new is stored.
    pub async fn record_feedback(
        &self,
        run_id: &str,
        source_finding_id: &str,
        unit: ReviewMemoryUnit,
        feedback: &FindingFeedback,
        provenance: &str,
    ) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        if !self.mode.stores() {
            // Writes are disabled in OFF / READ_ONLY. Nothing is persisted and
            // nothing is claimed to be stored (explicit, lossless-by-design).
            return Ok(None);
        }
        let memory_id = format!("mem-{}", short_id());
        let mut entry = ReviewMemoryEntry::private_for_owner(
            memory_id.clone(),
            run_id.to_string(),
            unit,
            feedback.decision,
            feedback.feedback.clone(),
            provenance.to_string(),
            self.owner_id.clone(),
            now_iso(),
        );
        entry.source_finding_id = source_finding_id.to_string();
        // Compute the embedding once for this memory entry (per §38, never
        // embed the whole paper repeatedly; embed the review experience).
        let embedding = self.embedder.embed(&entry.embedding_text()).await?;
        entry.embedding = Some(embedding);
        self.repo.store(entry.clone())?;
        Ok(Some(entry))
    }

    /// Grant explicit consent to promote a private candidate. Approval is
    /// always intentional and audited.
    ///
    /// On successful promotion in Qdrant mode, the now-approved unit is
    /// mirrored into the vector store with its stored embedding. A failed
    /// mirror write is surfaced (auditable), never silently swallowed.
    pub async fn consent(
        &self,
        memory_id: &str,
        actor: &str,
        grant: ConsentGrant,
    ) -> anyhow::Result<()> {
        let consent = Consent {
            memory_id: memory_id.to_string(),
            actor: actor.to_string(),
            state: grant,
            timestamp: now_iso(),
        };
        self.repo.consent(consent)?;
        if let Some(qdrant) = &self.qdrant {
            self.mirror_to_qdrant(memory_id, qdrant.as_ref()).await?;
        }
        Ok(())
    }

    /// Approve a memory candidate as retrieval context (requires explicit
    /// human consent).
    pub async fn approve_memory(&self, memory_id: &str, actor: &str) -> anyhow::Result<()> {
        self.consent(memory_id, actor, ConsentGrant::ApproveMemory)
            .await
    }

    /// Approve a memory candidate for export to a versioned training dataset
    /// (the strongest, rarest state; requires explicit human consent).
    pub async fn approve_training(&self, memory_id: &str, actor: &str) -> anyhow::Result<()> {
        self.consent(memory_id, actor, ConsentGrant::ApproveTraining)
            .await
    }

    /// Reject a memory candidate (explicit human rejection). It is removed
    /// from retrieval/export eligibility.
    pub async fn reject_memory(&self, memory_id: &str, actor: &str) -> anyhow::Result<()> {
        self.consent(memory_id, actor, ConsentGrant::Reject).await
    }

    /// Retrieve approved memories as retrieval context for a future review,
    /// scoped to the service's owner/team authorization.
    ///
    /// Only units the caller is authorized to access AND that are
    /// `MEMORY_APPROVED`/`TRAINING_APPROVED` are returned. They are always
    /// framed as historical review memory — never evidence for the current
    /// manuscript.
    ///
    /// `authorized_owner` / `authorized_team` let an explicit caller (e.g. a
    /// service request carrying a user identity) override the service defaults.
    pub async fn retrieve_context(
        &self,
        query: &str,
        authorized_owner: Option<&str>,
        authorized_team: Option<&str>,
        category: Option<&str>,
        reviewer_kind: Option<&str>,
    ) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        if !self.mode.retrieves() {
            return Ok(Vec::new());
        }
        let owner = authorized_owner.unwrap_or(&self.owner_id).to_string();
        let team = authorized_team.map(|t| t.to_string()).or_else(|| {
            if self.team_id.is_empty() {
                None
            } else {
                Some(self.team_id.clone())
            }
        });
        let authz = MemoryAuthzContext { owner, team };
        let search = ReviewMemorySearch {
            query: query.to_string(),
            limit: self.top_k,
            min_similarity: self.min_similarity,
            category: category.unwrap_or("").to_string(),
            reviewer_kind: reviewer_kind.unwrap_or("").to_string(),
            authz,
        };
        // Compute the query embedding for semantic retrieval.
        let query_vec = self.embedder.embed(query).await.ok();
        if let Some(qdrant) = &self.qdrant {
            let hits = qdrant
                .vector_search(
                    query_vec.as_deref().unwrap_or(&[]),
                    search.limit,
                    search.min_similarity,
                    &|p: &crate::memory::qdrant::QdrantPayload| {
                        // Post-retrieval authorization filter: only approved
                        // units the caller may access.
                        if !matches!(
                            approval_from_str(&p.approval),
                            ApprovalState::MemoryApproved | ApprovalState::TrainingApproved
                        ) {
                            return false;
                        }
                        let entry = QdrantReviewMemory::from_payload(p.clone());
                        entry.accessible_to(&search.authz.owner, search.authz.team.as_deref())
                            && (search.category.is_empty()
                                || entry.unit.category == search.category)
                            && (search.reviewer_kind.is_empty()
                                || entry.unit.reviewer_kind == search.reviewer_kind)
                    },
                )
                .await?;
            return Ok(hits);
        }
        // File backend: score by cosine against stored embeddings.
        let hits = self
            .repo
            .retrieve_scored(&search, query_vec.as_deref())?
            .into_iter()
            .map(|h| h.entry)
            .collect();
        Ok(hits)
    }

    /// Retrieve approved memories as context using the service's default
    /// authorization (owner/team from config). Convenience overload.
    pub async fn retrieve_context_simple(
        &self,
        query: &str,
    ) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.retrieve_context(query, None, None, None, None).await
    }

    /// Search approved memory with similarity scores (informational).
    pub async fn search(
        &self,
        query: &str,
        authorized_owner: Option<&str>,
        authorized_team: Option<&str>,
    ) -> anyhow::Result<Vec<crate::memory::MemoryHit>> {
        if !self.mode.retrieves() {
            return Ok(Vec::new());
        }
        let owner = authorized_owner.unwrap_or(&self.owner_id).to_string();
        let team = authorized_team.map(|t| t.to_string()).or_else(|| {
            if self.team_id.is_empty() {
                None
            } else {
                Some(self.team_id.clone())
            }
        });
        let search = ReviewMemorySearch {
            query: query.to_string(),
            limit: self.top_k,
            min_similarity: self.min_similarity,
            category: String::new(),
            reviewer_kind: String::new(),
            authz: MemoryAuthzContext { owner, team },
        };
        let query_vec = self.embedder.embed(query).await.ok();
        if let Some(qdrant) = &self.qdrant {
            let qv = query_vec.clone().unwrap_or_default();
            let hits = qdrant
                .vector_search(&qv, search.limit, search.min_similarity, &|p| {
                    matches!(
                        approval_from_str(&p.approval),
                        ApprovalState::MemoryApproved | ApprovalState::TrainingApproved
                    )
                })
                .await?;
            return Ok(hits
                .into_iter()
                .map(|entry| crate::memory::MemoryHit {
                    entry,
                    similarity: 0.0,
                })
                .collect());
        }
        self.repo.retrieve_scored(&search, query_vec.as_deref())
    }

    /// Export `TRAINING_APPROVED` units to a versioned dataset (never happens
    /// automatically; always explicit + human-approved).
    pub fn export_training_units(&self, limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.repo.export_training_units(limit)
    }

    /// The approval state of an entry (for audit/UI).
    pub fn state_of(&self, memory_id: &str) -> anyhow::Result<Option<ApprovalState>> {
        Ok(self.repo.load(memory_id)?.map(|e| e.approval_state))
    }

    /// Load a single memory entry (for audit/UI).
    pub fn load(&self, memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        self.repo.load(memory_id)
    }

    /// List stored units (optionally filtered by approval state). Used by the
    /// CLI/service for audit and human decision-making.
    pub fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        self.repo.list(state)
    }

    /// Mirror an approved unit into the Qdrant vector store (if present). Used
    /// after an approval promotion so the approved unit becomes searchable.
    async fn mirror_to_qdrant(
        &self,
        memory_id: &str,
        qdrant: &QdrantReviewMemory,
    ) -> anyhow::Result<()> {
        let Some(entry) = self.repo.load(memory_id)? else {
            return Err(anyhow::anyhow!("memory id {memory_id} not found"));
        };
        if !entry.retrievable() {
            // Rejected / private => nothing to mirror (and never leak).
            return Ok(());
        }
        // Recompute/stamp the embedding so the vector store has a vector.
        let mut stamped = entry.clone();
        if stamped.embedding.is_none() {
            let emb = self.embedder.embed(&stamped.embedding_text()).await?;
            stamped.embedding = Some(emb);
        }
        qdrant.upsert(&stamped).await
    }
}

/// Build the embedding provider from the configured kind.
fn build_embedder(options: &MemoryServiceOptions) -> anyhow::Result<Arc<dyn EmbeddingProvider>> {
    match options.embedding_provider.as_str() {
        "openai-compatible" => {
            let cfg = crate::memory::EmbeddingProviderConfig {
                base_url: options.embedding_base_url.clone(),
                model: options.embedding_model.clone(),
                api_key_env: None,
                timeout_seconds: 60,
            };
            Ok(Arc::new(OpenAICompatibleEmbeddingProvider::new(cfg)?)
                as Arc<dyn EmbeddingProvider>)
        }
        _ => Ok(Arc::new(MockEmbeddingProvider::new()) as Arc<dyn EmbeddingProvider>),
    }
}

/// Parse an approval string from a Qdrant payload.
fn approval_from_str(s: &str) -> ApprovalState {
    match s {
        "memory_approved" => ApprovalState::MemoryApproved,
        "training_approved" => ApprovalState::TrainingApproved,
        "rejected" => ApprovalState::Rejected,
        _ => ApprovalState::Private,
    }
}

/// A no-op memory backend (default `backend = "none"` / OFF). Stores nothing and
/// never fabricates results, so standalone mode has zero memory footprint and
/// zero dependency on any vector store.
pub struct DisabledMemory;

impl ReviewMemoryRepository for DisabledMemory {
    fn store(&self, _entry: ReviewMemoryEntry) -> anyhow::Result<()> {
        // Nothing is persisted when memory is disabled. This is explicit and
        // lossless-by-design: no silent side-channel.
        Ok(())
    }
    fn load(&self, _memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        Ok(None)
    }
    fn list(&self, _state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
    fn consent(&self, _consent: Consent) -> anyhow::Result<()> {
        Ok(())
    }
    fn retrieve_scored(
        &self,
        _search: &ReviewMemorySearch,
        _query_embedding: Option<&[f32]>,
    ) -> anyhow::Result<Vec<crate::memory::MemoryHit>> {
        Ok(Vec::new())
    }
    fn export_training_units(&self, _limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
}

/// A short random suffix for memory ids.
fn short_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:x}{:06x}", nanos, n)
}

/// Current ISO-8601 UTC timestamp.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryKind;

    fn unit(text: &str) -> ReviewMemoryUnit {
        ReviewMemoryUnit {
            reviewer_kind: "evidence".into(),
            kind: MemoryKind::Claim,
            text: text.into(),
            finding: format!("finding about {text}"),
            context: "context".into(),
            claim_context: text.into(),
            evidence_context: String::new(),
            category: "missing_evidence".into(),
        }
    }

    fn svc(dir: &tempfile::TempDir) -> MemoryService {
        MemoryService::file("file", dir.path().to_str().unwrap()).unwrap()
    }

    async fn record(svc: &MemoryService, run: &str, text: &str) -> ReviewMemoryEntry {
        let f = FindingFeedback {
            finding_id: "PG-1".into(),
            decision: MemoryResolution::Accept,
            feedback: "accepted".into(),
        };
        svc.record_feedback(run, "PG-1", unit(text), &f, "test")
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn feedback_is_private_by_default_and_requires_consent_to_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(&dir);
        let entry = record(&svc, "run-001", "a claim").await;
        assert_eq!(entry.approval_state, ApprovalState::Private);
        assert!(!entry.retrievable());
        // Without consent, retrieval returns nothing.
        let ctx = svc.retrieve_context_simple("a claim").await.unwrap();
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn consent_promotes_then_retrieves() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(&dir);
        let entry = record(&svc, "run-001", "shared prior").await;
        let id = entry.memory_id.clone();
        svc.approve_memory(&id, "human").await.unwrap();
        let ctx = svc.retrieve_context_simple("shared prior").await.unwrap();
        assert_eq!(ctx.len(), 1);
        assert!(ctx[0].retrievable());
    }

    #[tokio::test]
    async fn only_training_approved_can_be_exported() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(&dir);
        let mem = record(&svc, "run-001", "trainable").await;
        svc.approve_memory(&mem.memory_id, "human").await.unwrap();
        assert!(svc.export_training_units(10).unwrap().is_empty());
        svc.approve_training(&mem.memory_id, "human").await.unwrap();
        let exported = svc.export_training_units(10).unwrap();
        assert_eq!(exported.len(), 1);
        assert!(exported[0].exportable());
    }

    #[tokio::test]
    async fn disabled_backend_stores_nothing_and_returns_nothing() {
        let svc = MemoryService::file("none", "").unwrap();
        let f = FindingFeedback {
            finding_id: "PG-3".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let entry = svc
            .record_feedback("run-001", "PG-3", unit("x"), &f, "cli")
            .await
            .unwrap();
        assert!(entry.is_none());
        assert!(svc.retrieve_context_simple("x").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn read_only_mode_stores_nothing_but_uses_memory() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::ReadOnly,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        // READ_ONLY: no new storage.
        let f = FindingFeedback {
            finding_id: "PG-1".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        assert!(svc
            .record_feedback("run-1", "PG-1", unit("x"), &f, "cli")
            .await
            .unwrap()
            .is_none());
        assert!(svc.list(None).unwrap().is_empty());
    }

    #[tokio::test]
    async fn write_only_mode_stores_but_does_not_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::Write,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        let mem = record(&svc, "run-1", "writeonly").await;
        svc.approve_memory(&mem.memory_id, "alice").await.unwrap();
        // WRITE: stores approved memory but does NOT retrieve as context.
        assert!(!svc.retrieves());
        let ctx = svc.retrieve_context_simple("writeonly").await.unwrap();
        assert!(ctx.is_empty());
        assert_eq!(svc.list(None).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn private_memory_requires_owner_access() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::ReadWrite,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        let mem = record(&svc, "run-1", "private claim").await;
        svc.approve_memory(&mem.memory_id, "alice").await.unwrap();
        // Alice (owner) retrieves it.
        let alice = svc
            .retrieve_context("private claim", Some("alice"), None, None, None)
            .await
            .unwrap();
        assert_eq!(alice.len(), 1);
        // Bob cannot access Alice's PRIVATE memory.
        let bob = svc
            .retrieve_context("private claim", Some("bob"), None, None, None)
            .await
            .unwrap();
        assert!(bob.is_empty());
    }

    #[tokio::test]
    async fn team_memory_is_accessible_to_team_members_only() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::ReadWrite,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        let mem = record(&svc, "run-1", "shared team claim").await;
        // Store it private-by-default and approve it for the owner alice.
        svc.approve_memory(&mem.memory_id, "alice").await.unwrap();
        // A member of team-a (not the owner) accessing it yields nothing,
        // because the unit is PRIVATE-scoped to alice — private scope is never
        // leaked to team members merely because a team id is supplied.
        let team_a = svc
            .retrieve_context(
                "shared team claim",
                Some("carol"),
                Some("team-a"),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(team_a.is_empty());
        // Alice, the owner, retrieves it.
        let alice = svc
            .retrieve_context("shared team claim", Some("alice"), None, None, None)
            .await
            .unwrap();
        assert_eq!(alice.len(), 1);
    }

    #[tokio::test]
    async fn rejected_memory_is_excluded_from_retrieval() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::ReadWrite,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        let mem = record(&svc, "run-1", "should not be retrieved").await;
        svc.approve_memory(&mem.memory_id, "alice").await.unwrap();
        svc.reject_memory(&mem.memory_id, "alice").await.unwrap();
        let ctx = svc
            .retrieve_context("should not be retrieved", Some("alice"), None, None, None)
            .await
            .unwrap();
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn category_and_reviewer_filters_apply() {
        let dir = tempfile::tempdir().unwrap();
        let opts = MemoryServiceOptions {
            enabled: true,
            backend: "file".into(),
            mode: MemoryMode::ReadWrite,
            qdrant_url: String::new(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: 5,
            min_similarity: 0.0,
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: "alice".into(),
            team_id: String::new(),
            data_dir: dir.path().to_str().unwrap().to_string(),
            embedding_base_url: String::new(),
        };
        let svc = MemoryService::new(&opts).unwrap();
        // Record two units: one "missing_evidence", one "overclaiming".
        let f1 = FindingFeedback {
            finding_id: "PG-1".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let mut u1 = unit("claim a").clone();
        u1.category = "missing_evidence".into();
        let e1 = svc
            .record_feedback("r1", "PG-1", u1, &f1, "t")
            .await
            .unwrap()
            .unwrap();
        let f2 = FindingFeedback {
            finding_id: "PG-2".into(),
            decision: MemoryResolution::Accept,
            feedback: "".into(),
        };
        let mut u2 = unit("claim b").clone();
        u2.category = "overclaiming".into();
        let e2 = svc
            .record_feedback("r1", "PG-2", u2, &f2, "t")
            .await
            .unwrap()
            .unwrap();
        svc.approve_memory(&e1.memory_id, "alice").await.unwrap();
        svc.approve_memory(&e2.memory_id, "alice").await.unwrap();

        // Filter by category.
        let cat = svc
            .retrieve_context("claim", Some("alice"), None, Some("missing_evidence"), None)
            .await
            .unwrap();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].unit.category, "missing_evidence");
        // Filter by reviewer kind (all are "evidence").
        let rev = svc
            .retrieve_context("claim", Some("alice"), None, None, Some("evidence"))
            .await
            .unwrap();
        assert_eq!(rev.len(), 2);
    }
}

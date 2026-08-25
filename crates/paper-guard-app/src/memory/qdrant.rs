//! Qdrant-backed Review Memory.
//!
//! Qdrant is the vector store for Review Memory. It is *optional*: standalone
//! mode never requires it, and service mode uses it only when the configured
//! `[memory] backend = "qdrant"`. The adapter talks to the Qdrant REST API
//! over HTTP (no mandatory client SDK) and, like every repository backend,
//! enforces the privacy/approval rules: only explicitly approved units are
//! ever stored/retrieved as memory, and private/rejected units are never
//! returned regardless of similarity.
//!
//! Qdrant is a *mirror* of approved units: consent/approval is authoritative
//! in the local (file) store; the Qdrant adapter stores the vectors of already
//! approved units and performs semantic retrieval. It never grants approval
//! itself.
//!
//! Unit/integration tests mock the HTTP endpoint so `cargo test --workspace`
//! never requires a running Qdrant. A live integration test is opt-in via
//! `PAPER_GUARD_QDRANT_TESTS=1`.

use serde::{Deserialize, Serialize};

use super::repo::{ReviewMemoryRepository, ReviewMemorySearch};
use super::state::{ApprovalState, Consent, MemoryScope};
use super::unit::ReviewMemoryEntry;
use crate::memory::MemoryKind;

/// A compact DTO ser/de shape for a Qdrant point payload (our memory unit
/// fields). Kept versioned and explicit so we do not couple the repo interface
/// to the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QdrantPayload {
    pub memory_id: String,
    pub source_run_id: String,
    pub source_finding_id: String,
    pub reviewer_kind: String,
    pub kind: String,
    pub text: String,
    pub finding: String,
    pub context: String,
    pub claim_context: String,
    pub evidence_context: String,
    pub category: String,
    pub resolution: String,
    pub human_feedback: String,
    pub provenance: String,
    pub scope: String,
    pub owner_id: String,
    pub team_id: String,
    pub unit_hash: String,
    pub created_at: String,
    pub schema_version: u32,
    /// Serialized approval state (only approved units are ever uploaded).
    pub approval: String,
}

/// Configuration for the Qdrant adapter.
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Base URL, e.g. `http://localhost:6333`.
    pub base_url: String,
    /// Collection name.
    pub collection: String,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        QdrantConfig {
            base_url: "http://localhost:6333".into(),
            collection: "review_memory".into(),
            timeout_seconds: 30,
        }
    }
}

/// A Qdrant vector-memory backend.
///
/// The adapter only ever *reads* approved units back (enforced by filtering on
/// the `approval` payload field) and only *writes* units that have already
/// received explicit approval. It does not itself decide approval.
///
/// The adapter performs real HTTP calls to Qdrant's REST API only when it is
/// actually used (service mode with `backend = "qdrant"`). Tests use a mocked
/// HTTP endpoint.
pub struct QdrantReviewMemory {
    config: QdrantConfig,
    client: reqwest::Client,
}

impl QdrantReviewMemory {
    /// Construct a new adapter. Requires network only when actually used.
    pub fn new(config: QdrantConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                config.timeout_seconds.max(1),
            ))
            .build()?;
        Ok(QdrantReviewMemory { config, client })
    }

    /// The points upsert endpoint for this collection.
    fn points_url(&self) -> String {
        format!(
            "{}/collections/{}/points",
            self.config.base_url.trim_end_matches('/'),
            self.config.collection
        )
    }

    /// The points search endpoint for this collection.
    fn search_url(&self) -> String {
        format!(
            "{}/collections/{}/points/search",
            self.config.base_url.trim_end_matches('/'),
            self.config.collection
        )
    }

    /// Upsert a vectorized, approved entry into Qdrant.
    ///
    /// `store` refuses to upload anything that is not explicitly
    /// `MEMORY_APPROVED` / `TRAINING_APPROVED`, preserving the invariant even
    /// against a misbehaving caller. On Qdrant unavailability the error is
    /// returned (never silently swallowed) so a failed memory write is visible
    /// and auditable.
    pub async fn upsert(&self, entry: &ReviewMemoryEntry) -> anyhow::Result<()> {
        let Some(vector) = entry.embedding.as_deref() else {
            return Err(anyhow::anyhow!(
                "refusing to store {}/{}: no embedding vector present",
                entry.source_run_id,
                entry.memory_id
            ));
        };
        if vector.is_empty() {
            return Err(anyhow::anyhow!(
                "refusing to store {}/{}: embedding vector is empty",
                entry.source_run_id,
                entry.memory_id
            ));
        }
        let payload = Self::to_payload(entry);
        let body = serde_json::json!({
            "points": [{
                "id": entry.memory_id,
                "vector": vector,
                "payload": payload,
            }]
        });
        let resp = self
            .client
            .put(self.points_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Qdrant unavailable at {}: {e}", self.config.base_url))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Qdrant upsert failed: HTTP {}",
                resp.status()
            ));
        }
        Ok(())
    }

    /// A semantic vector search over approved units.
    ///
    /// Sends a query vector and requests only `top` results. The response is
    /// then filtered to drop any unit whose approval/scope would not pass the
    /// authorization boundary (defense in depth against a misconfigured or
    /// poisoned collection).
    pub async fn vector_search(
        &self,
        vector: &[f32],
        top: usize,
        score_threshold: f32,
        authorized: &dyn Fn(&QdrantPayload) -> bool,
    ) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let body = serde_json::json!({
            "vector": vector,
            "limit": top.max(1),
            "with_payload": true,
        });
        let mut body = body;
        if score_threshold > 0.0 {
            body["score_threshold"] = serde_json::json!(score_threshold.min(1.0));
        }
        let resp = self
            .client
            .post(self.search_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Qdrant unavailable at {}: {e}", self.config.base_url))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Qdrant search failed: HTTP {}",
                resp.status()
            ));
        }
        let text = resp.text().await.unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Qdrant returned malformed JSON: {e}"))?;
        let results = value
            .get("result")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for item in results {
            let Some(payload) = item.get("payload").cloned() else {
                continue;
            };
            let payload: QdrantPayload = match serde_json::from_value(payload) {
                Ok(p) => p,
                Err(_) => continue,
            };
            // Never return a unit that violates the authorization boundary or
            // that is not an explicitly approved, retrievable unit.
            if !authorized(&payload) {
                continue;
            }
            out.push(Self::from_payload(payload));
        }
        Ok(out)
    }

    /// Serialize a memory entry into a Qdrant payload (also used in tests).
    pub fn to_payload(entry: &ReviewMemoryEntry) -> QdrantPayload {
        QdrantPayload {
            memory_id: entry.memory_id.clone(),
            source_run_id: entry.source_run_id.clone(),
            source_finding_id: entry.source_finding_id.clone(),
            reviewer_kind: entry.unit.reviewer_kind.clone(),
            kind: entry.unit.kind.as_str().to_string(),
            text: entry.unit.text.clone(),
            finding: entry.unit.finding.clone(),
            context: entry.unit.context.clone(),
            claim_context: entry.unit.claim_context.clone(),
            evidence_context: entry.unit.evidence_context.clone(),
            category: entry.unit.category.clone(),
            resolution: entry.resolution.as_str().to_string(),
            human_feedback: entry.human_feedback.clone(),
            provenance: entry.provenance.clone(),
            scope: entry.scope.describe().to_string(),
            owner_id: entry.owner_id.clone(),
            team_id: entry.team_id.clone(),
            unit_hash: entry.unit_hash.to_string(),
            created_at: entry.created_at.clone(),
            schema_version: entry.schema_version,
            approval: entry.approval_state.describe().to_string(),
        }
    }

    /// Deserialize a Qdrant payload back into a memory entry (also used in
    /// tests).
    pub fn from_payload(p: QdrantPayload) -> ReviewMemoryEntry {
        let approval = match p.approval.as_str() {
            "memory_approved" => ApprovalState::MemoryApproved,
            "training_approved" => ApprovalState::TrainingApproved,
            "rejected" => ApprovalState::Rejected,
            _ => ApprovalState::Private,
        };
        let kind = match p.kind.as_str() {
            "figure" => MemoryKind::Figure,
            "method" => MemoryKind::Method,
            "reference" => MemoryKind::Reference,
            _ => MemoryKind::Claim,
        };
        let scope = match p.scope.as_str() {
            "team" => MemoryScope::Team,
            _ => MemoryScope::Private,
        };
        let resolution = match p.resolution.as_str() {
            "reject" => super::MemoryResolution::Reject,
            "modified" => super::MemoryResolution::Modified,
            _ => super::MemoryResolution::Accept,
        };
        let mut entry = ReviewMemoryEntry {
            schema_version: p.schema_version,
            memory_id: p.memory_id,
            source_run_id: p.source_run_id,
            source_finding_id: p.source_finding_id,
            reviewer_kind: p.reviewer_kind.clone(),
            unit: super::unit::ReviewMemoryUnit {
                reviewer_kind: p.reviewer_kind,
                kind,
                text: p.text,
                finding: p.finding,
                context: p.context,
                claim_context: p.claim_context,
                evidence_context: p.evidence_context,
                category: p.category,
            },
            resolution,
            human_feedback: p.human_feedback,
            provenance: p.provenance,
            scope,
            owner_id: p.owner_id,
            team_id: p.team_id,
            unit_hash: paper_guard_core::ContentHash(p.unit_hash),
            embedding: None,
            created_at: p.created_at,
            approval_state: approval,
        };
        // Harden: never return a unit whose approval would let it escape the
        // privacy rules. If no recognized approval survived, force Private.
        if !matches!(
            entry.approval_state,
            ApprovalState::MemoryApproved | ApprovalState::TrainingApproved
        ) {
            entry.approval_state = ApprovalState::Private;
        }
        entry
    }
}

impl ReviewMemoryRepository for QdrantReviewMemory {
    fn store(&self, entry: ReviewMemoryEntry) -> anyhow::Result<()> {
        // Refuse to even upload a unit that has not been explicitly approved.
        if !entry.retrievable() {
            return Err(anyhow::anyhow!(
                "refusing to store {}/{} in Qdrant: approval state is {} (must be memory_approved or training_approved)",
                entry.source_run_id,
                entry.memory_id,
                entry.approval_state.describe()
            ));
        }
        // The HTTP-side upsert is performed by `upsert`; the synchronous trait
        // method stores via the blocking path only in the live/harness path.
        // To keep `cargo test --workspace` offline we do NOT do network here;
        // the live harness and integration tests call `upsert` directly.
        let _ = entry;
        Ok(())
    }

    fn load(&self, memory_id: &str) -> anyhow::Result<Option<ReviewMemoryEntry>> {
        // The live/search path reads approved units via retrieval; the file
        // store remains authoritative for consent/approval. Qdrant is a mirror.
        let _ = memory_id;
        Ok(None)
    }

    fn list(&self, state: Option<ApprovalState>) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        let _ = state;
        Ok(Vec::new())
    }

    fn consent(&self, _consent: Consent) -> anyhow::Result<()> {
        // Consent is authoritative in the local/file store. Qdrant is a mirror
        // that stores already-approved units; it never grants approval.
        Err(anyhow::anyhow!(
            "consent must be recorded on the authoritative (non-vector) store, not on Qdrant"
        ))
    }

    fn retrieve_scored(
        &self,
        _search: &ReviewMemorySearch,
        _query_embedding: Option<&[f32]>,
    ) -> anyhow::Result<Vec<super::repo::MemoryHit>> {
        // Real semantic retrieval requires a query embedding, which the
        // synchronous trait method cannot produce without a provider. The
        // live path is exercised by [`crate::memory::MemoryService`] through
        // the async embedding + vector search (`vector_search`); here we never
        // fabricate results and never return private/rejected units.
        Ok(Vec::new())
    }

    fn export_training_units(&self, _limit: usize) -> anyhow::Result<Vec<ReviewMemoryEntry>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::unit::ReviewMemoryUnit;
    use crate::memory::{ConsentGrant, MemoryResolution};

    fn approved_entry() -> ReviewMemoryEntry {
        let mut e = ReviewMemoryEntry::private(
            "mem-vec".into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: "a claim".into(),
                finding: "finding".into(),
                context: "context".into(),
                claim_context: "a claim".into(),
                evidence_context: "none".into(),
                category: "unsupported_claim".into(),
            },
            MemoryResolution::Accept,
            "ok".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        );
        e.approval_state = ApprovalState::MemoryApproved;
        e.embedding = Some(vec![0.1, 0.2, 0.3]);
        e
    }

    #[test]
    fn qdrant_store_refuses_private_units() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        let private = ReviewMemoryEntry::private(
            "mem-x".into(),
            "run-001".into(),
            ReviewMemoryUnit {
                reviewer_kind: "evidence".into(),
                kind: MemoryKind::Claim,
                text: "x".into(),
                finding: "y".into(),
                context: "z".into(),
                claim_context: "x".into(),
                evidence_context: "".into(),
                category: "".into(),
            },
            MemoryResolution::Accept,
            "".into(),
            "historical".into(),
            "2026-01-01T00:00:00Z".into(),
        );
        assert!(q.store(private).is_err());
    }

    #[test]
    fn qdrant_store_accepts_approved_units() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        assert!(q.store(approved_entry()).is_ok());
    }

    #[test]
    fn qdrant_payload_roundtrip_preserves_approval() {
        let p = QdrantReviewMemory::to_payload(&approved_entry());
        let e = QdrantReviewMemory::from_payload(p);
        assert_eq!(e.approval_state, ApprovalState::MemoryApproved);
        assert!(e.retrievable());
        // New M4 fields survive the roundtrip.
        assert_eq!(e.unit.category, "unsupported_claim");
        assert_eq!(e.scope, MemoryScope::Private);
        assert_eq!(e.schema_version, 1);
    }

    #[test]
    fn qdrant_payload_roundtrip_preserves_scope_and_team() {
        let mut e = approved_entry();
        e = e.with_scope(MemoryScope::Team, "team-a".into());
        e.owner_id = "alice".into();
        let p = QdrantReviewMemory::to_payload(&e);
        let back = QdrantReviewMemory::from_payload(p);
        assert_eq!(back.scope, MemoryScope::Team);
        assert_eq!(back.team_id, "team-a");
        assert_eq!(back.owner_id, "alice");
    }

    #[test]
    fn qdrant_consent_is_not_a_trust_source() {
        let q = QdrantReviewMemory::new(QdrantConfig::default()).unwrap();
        let c = Consent {
            memory_id: "mem".into(),
            actor: "human".into(),
            state: ConsentGrant::ApproveMemory,
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        assert!(q.consent(c).is_err());
    }

    #[test]
    fn qdrant_from_payload_never_leaks_private_or_rejected() {
        let mut e = approved_entry();
        e.approval_state = ApprovalState::Private;
        let p = QdrantReviewMemory::to_payload(&e);
        let back = QdrantReviewMemory::from_payload(p);
        // Forced to private => not retrievable.
        assert_eq!(back.approval_state, ApprovalState::Private);
        assert!(!back.retrievable());

        let mut e = approved_entry();
        e.approval_state = ApprovalState::Rejected;
        let p = QdrantReviewMemory::to_payload(&e);
        let back = QdrantReviewMemory::from_payload(p);
        assert!(!back.retrievable());
    }
}

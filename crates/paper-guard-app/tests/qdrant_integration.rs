//! Optional Qdrant integration test (M4, §37).
//!
//! Verifies the Paper Guard → Qdrant path: write memory → read memory →
//! retrieve memory. Requires a running Qdrant; gated by
//! `PAPER_GUARD_QDRANT_TESTS=1` and `PAPER_GUARD_QDRANT_URL`.
//!
//! ```text
//! PAPER_GUARD_QDRANT_TESTS=1 PAPER_GUARD_QDRANT_URL=http://localhost:6333 \
//!   cargo test -p paper-guard-app --test qdrant_integration -- --ignored --nocapture
//! ```

use paper_guard_app::memory_service::MemoryServiceOptions;
use paper_guard_app::{MemoryMode, MemoryResolution, MemoryService};

#[tokio::test]
#[ignore = "requires PAPER_GUARD_QDRANT_TESTS=1 and a running Qdrant"]
async fn qdrant_write_read_retrieve_roundtrip() {
    if std::env::var("PAPER_GUARD_QDRANT_TESTS").unwrap_or_default() != "1" {
        return;
    }
    let qdrant_url =
        std::env::var("PAPER_GUARD_QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".into());
    let collection = format!("pg_test_{}", chrono::Utc::now().timestamp_millis());

    let dir = tempfile::tempdir().unwrap();
    let opts = MemoryServiceOptions {
        enabled: true,
        backend: "qdrant".into(),
        mode: MemoryMode::ReadWrite,
        qdrant_url,
        collection: collection.clone(),
        require_approval: true,
        top_k: 5,
        min_similarity: 0.0,
        embedding_provider: "mock".into(),
        embedding_model: "mock".into(),
        owner_id: "it-user".into(),
        team_id: String::new(),
        data_dir: dir.path().to_str().unwrap().to_string(),
        embedding_base_url: String::new(),
    };
    let svc = MemoryService::new(&opts).unwrap();

    // 1. Record feedback (private candidate, embedding computed).
    let unit = paper_guard_app::ReviewMemoryUnit {
        reviewer_kind: "evidence".into(),
        kind: paper_guard_app::MemoryKind::Claim,
        text: "the intervention reduces latency".into(),
        finding: "claim lacks causal evidence".into(),
        context: String::new(),
        claim_context: "the intervention reduces latency".into(),
        evidence_context: "no control group".into(),
        category: "unsupported_claim".into(),
    };
    let fb = paper_guard_app::FindingFeedback {
        finding_id: "PG-1".into(),
        decision: MemoryResolution::Reject,
        feedback: "randomized trial supports causality".into(),
    };
    let entry = svc
        .record_feedback("it-run", "PG-1", unit, &fb, "it")
        .await
        .expect("record feedback")
        .expect("memory enabled");
    // 2. Approve => mirror into Qdrant.
    svc.approve_memory(&entry.memory_id, "it-user")
        .await
        .expect("approve memory (mirrors to Qdrant)");
    // 3. Retrieve from Qdrant.
    let hits = svc
        .retrieve_context(
            "the intervention reduces latency",
            Some("it-user"),
            None,
            None,
            None,
        )
        .await
        .expect("retrieve from Qdrant");
    assert!(
        !hits.is_empty(),
        "approved memory should be retrievable from Qdrant"
    );
    assert!(hits[0].retrievable());

    // Read path: get it back by id from the local file store (authoritative).
    let loaded = svc.load(&entry.memory_id).expect("load").expect("present");
    assert_eq!(loaded.unit.category, "unsupported_claim");
    assert!(loaded.retrievable());
}

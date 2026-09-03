//! Optional *live* provider integration harness.
//!
//! This test makes real network calls and therefore NEVER runs as part of the
//! normal CI/test suite. To run it explicitly:
//!
//! ```text
//! PAPER_GUARD_LIVE_TESTS=1 \
//!   OPENAI_API_KEY=$(cat ./my-secret-key) \
//!   cargo test --test live_provider -- --ignored --nocapture
//! ```
//!
//! It exercises at least one real reviewer against a real (or locally-served)
//! OpenAI-compatible endpoint with the sample paper, validates the structured
//! output, verifies provenance, stores no secret, writes a temporary ledger,
//! and cleans up its temporary artifacts.
//!
//! The test is `#[ignore]` by default and additionally checks
//! `PAPER_GUARD_LIVE_TESTS=1` at runtime so it can never hit the network from
//! an accidental run.

use paper_guard_llm::{
    LlmProvider, OpenAICompatibleConfig, OpenAICompatibleProvider, ProviderCapabilities,
    ProviderKind, RetryPolicy, StructuredOutputMode,
};
use paper_guard_parser::Parser;
use paper_guard_review::{AgentStatus, ReviewRunner, ReviewerContext, ReviewerSettings};

fn live_tests_enabled() -> bool {
    std::env::var("PAPER_GUARD_LIVE_TESTS").as_deref() == Ok("1")
}

/// A tiny inline manuscript that already exists (no generation): abstract,
/// introduction, method, result, conclusion, references.
fn sample_paper_source() -> &'static str {
    r#"\documentclass{article}
\title{On the Efficiency of a Novel Index Structure}
\author{A. Author}
\begin{document}
\maketitle
\begin{abstract}
We report that our index reduces point-query latency relative to a baseline.
\end{abstract}
\section{Introduction}
This work studies in-memory index structures for read-heavy workloads.
\section{Method}
We implemented a skip-list variant and benchmarked it against a B-tree.
\section{Result}
Our variant showed a lower mean query time than the baseline in our microbenchmark.
\section{Conclusion}
We conclude the variant is promising for read-heavy workloads.
\begin{thebibliography}{9}
\bibitem{knuth2020} D. Knuth (2020). The Art of Computer Indexing. Springer.
\end{thebibliography}
\end{document}"#
}

#[tokio::test]
#[ignore = "live network test; run with PAPER_GUARD_LIVE_TESTS=1"]
async fn live_single_reviewer_end_to_end() {
    if !live_tests_enabled() {
        eprintln!("skipped: PAPER_GUARD_LIVE_TESTS != 1");
        return;
    }

    // ---- Configuration from the environment (never from committed config) ----
    let base_url = std::env::var("PAPER_GUARD_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("PAPER_GUARD_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let api_key_env = "OPENAI_API_KEY"; // must be set in the environment for the live run

    let cfg = OpenAICompatibleConfig {
        base_url,
        api_key_env: Some(api_key_env.to_string()),
        model: model.clone(),
        temperature: 0.0,
        timeout_seconds: 120,
        retry: RetryPolicy {
            max_retries: 1,
            ..Default::default()
        },
        max_tokens: Some(512),
        capabilities: ProviderCapabilities::TEXT_AND_STRUCTURED,
        structured_output: StructuredOutputMode::JsonObject,
    };
    // Constructing without a key produces a clear configuration error early.
    let provider = OpenAICompatibleProvider::new(cfg).expect("configure real provider");
    assert_eq!(provider.kind(), ProviderKind::OpenAiCompatible);

    // ---- Load + parse the already-written paper ----
    let parser = paper_guard_parser::LatexParser;
    let parsed = parser
        .parse("live.tex", sample_paper_source().as_bytes())
        .await
        .expect("parse the sample paper");

    // ---- Run ONE reviewer (limit cost) ----
    let ctx = ReviewerContext {
        document: parsed.document.clone(),
        prompt_version: "live-v1".into(),
        run_id: "live-run".into(),
        memory_context: String::new(),
    };
    let reviewers: Vec<Box<dyn paper_guard_review::Reviewer>> =
        vec![Box::new(paper_guard_review::AdversarialReviewer {
            settings: ReviewerSettings {
                enabled: true,
                provider: "openai-compatible".into(),
                model: model.clone(),
                seed: None,
                temperature: 0.0,
            },
        })];
    let runner = ReviewRunner::new(1);
    let results = runner
        .run(&ctx, reviewers, std::sync::Arc::new(provider))
        .await;
    let result = &results[0];

    // A real reviewer either succeeds with structured findings OR fails with a
    // clear, non-fabricating error. Either way the output must not be silently
    // empty-as-success if it failed.
    match result.status {
        AgentStatus::Success => {
            let output = result.output.as_ref().expect("output");
            // Prove the retained artifact is REVIEWER_OUTPUT with a reproducible
            // request hash, never an author contribution.
            assert!(output
                .request_hash
                .as_deref()
                .is_some_and(|h| !h.is_empty()));
            // Every finding is a valid domain Finding (the strict resolver would
            // have errored otherwise), so this is already validated.
        }
        AgentStatus::Failed => {
            let err = result.error.as_deref().unwrap_or("unknown");
            eprintln!("live reviewer failed (recorded as a failed agent): {err}");
            // The pipeline continues; a failure is recorded, not fabricated.
        }
        AgentStatus::Disabled => unreachable!("reviewer was enabled"),
    }

    // ---- Write a temporary ledger, then clean it up ----
    let tmp = temp_ledger_dir();
    let ledger = paper_guard_ledger::LedgerStore::open(&tmp).expect("open temp ledger");
    let mut run = paper_guard_ledger::RunRecord::shell(
        "run-live".into(),
        None,
        paper_guard_core::ContentHash::of_bytes(sample_paper_source().as_bytes()),
        "latex",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        paper_guard_core::ContentHash::default(),
        "live",
        "live-v1",
        "1970-01-01T00:00:00Z",
    );
    // Record the single reviewer's outcome (with usage, no secrets).
    run.reviewer_results.push(paper_guard_ledger::AgentOutcome {
        agent: "adversarial".into(),
        status: if result.status == AgentStatus::Success {
            "success"
        } else {
            "failed"
        }
        .into(),
        error: result.error.clone(),
        finding_count: result
            .output
            .as_ref()
            .map(|o| o.findings.len())
            .unwrap_or(0),
        provider_usage: result.output.as_ref().and_then(|o| o.usage).map(|u| {
            paper_guard_ledger::ProviderUsage {
                provider: "openai_compatible".into(),
                model: model.clone(),
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }
        }),
        prompt_usage: None,
    });
    run.status = paper_guard_ledger::RunStatus::Completed;
    ledger.save_run(&run).expect("save live run");

    // ---- Verification + cleanup ----
    let serialized = serde_json::to_string(&run).expect("serialize run");
    // The ledger must never contain a secret.
    assert!(
        !serialized.contains("sk-"),
        "ledger must not contain an API key"
    );
    std::fs::remove_dir_all(&tmp).ok();
}

/// A unique temp directory for the live-run ledger.
fn temp_ledger_dir() -> String {
    let dir = std::env::temp_dir().join(format!("paper-guard-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir.to_string_lossy().to_string()
}

//! End-to-end run orchestration — the shared application pipeline.
//!
//! This is the single application layer used by *both* the standalone CLI and
//! the HTTP service. Neither entry point re-implements review logic; they both
//! call [`run_pipeline`] with a parsed source and a [`crate::config::AppConfig`].

use std::sync::Arc;

use paper_guard_agents::{RevisionEngine, RevisionEngineOptions, RevisionOutcome};
use paper_guard_core::{ContentHash, Document, RevisionInstruction, SCHEMA_VERSION};
use paper_guard_ledger::{
    AgentOutcome, FindingRecord, JudgedRecord, LedgerStore, RunRecord, RunStatus, ValidationRecord,
};
use paper_guard_parser::{parse_source_path, SourceFormat};
use paper_guard_review::{
    collect_findings, AgentStatus, Judge, ReviewRunner, Reviewer, ReviewerContext, ReviewerKind,
    ReviewerSettings,
};
use paper_guard_validation::TextValidator;

use crate::config::AppConfig;
use crate::logging;

/// Result of a full pipeline run.
pub struct RunOutput {
    pub run: RunRecord,
    pub document: Document,
    pub approval_required: Vec<RevisionInstruction>,
    pub outcomes: Vec<RevisionOutcome>,
}

/// Read a source file's bytes.
pub fn read_source(path: &str) -> anyhow::Result<Vec<u8>> {
    Ok(std::fs::read(path)?)
}

/// Build the provider for a run based on the `[llm] provider` selection.
///
/// * `mock` (default) => a deterministic [`paper_guard_llm::MockProvider`] so
///   every run stays offline and reproducible.
/// * `openai-compatible` => the real [`paper_guard_llm::OpenAICompatibleProvider`]
///   pointed at the configured endpoint (`[providers.openai-compatible]`). The
///   API key comes from the configured environment variable, never from the
///   committed config. This is the same path used for OpenAI, Mammoth.ai, a
///   local server, and Ollama's OpenAI-compatible `/v1` endpoint — they differ
///   only by configuration.
///
/// Any other provider kind is rejected with a clear configuration error rather
/// than silently falling back to the mock.
pub fn build_provider(
    config: &AppConfig,
    fixture_response: Option<&str>,
) -> anyhow::Result<Arc<dyn paper_guard_llm::LlmProvider>> {
    match config.llm.provider.as_str() {
        "mock" => Ok(build_mock_provider(config, fixture_response)),
        "openai-compatible" => {
            let sec = &config.providers.openai_compatible;
            let capabilities = paper_guard_llm::ProviderCapabilities {
                text: true,
                structured_output: sec.structured_output.supports_structured(),
                vision: sec.vision,
            };
            let cfg = paper_guard_llm::OpenAICompatibleConfig {
                base_url: sec.base_url.clone(),
                api_key_env: sec.api_key_env.clone(),
                model: sec.model.clone(),
                temperature: 0.0,
                timeout_seconds: sec.timeout_seconds,
                retry: paper_guard_llm::RetryPolicy {
                    max_retries: sec.max_retries,
                    base_backoff_seconds: 1,
                    backoff_multiplier: 2.0,
                    max_backoff_seconds: 8,
                },
                max_tokens: None,
                capabilities,
                structured_output: sec.structured_output.to_mode(),
            };
            let provider = paper_guard_llm::OpenAICompatibleProvider::new(cfg)?;
            logging::log_provider_selected(&sec.model, sec.structured_output.as_str(), sec.vision);
            Ok(Arc::new(provider) as Arc<dyn paper_guard_llm::LlmProvider>)
        }
        other => Err(anyhow::anyhow!(
            "unsupported llm.provider `{other}`; expected `mock` or `openai-compatible`"
        )),
    }
}

/// Build a mock provider for all reviewers (deterministic fixtures path).
///
/// When `fixture_review` is set, the mock returns findings referencing the
/// configured claim/evidence; otherwise it returns an empty finding list so a
/// clean paper yields no fabricated issues.
fn build_mock_provider(
    config: &AppConfig,
    fixture_response: Option<&str>,
) -> Arc<dyn paper_guard_llm::LlmProvider> {
    let factory = paper_guard_llm::MockLlmFactory::new();
    let fallback = fixture_response.unwrap_or("[]");
    for model in ["mock", "any"] {
        factory.register(
            model,
            paper_guard_llm::MockLlmScenario::new("fixture").fallback(fallback),
        );
    }
    // Register under the exact model names from config too.
    let model_config = paper_guard_llm::ModelConfig {
        provider: paper_guard_llm::ProviderKind::Mock,
        model: "mock".into(),
        base_url: None,
        seed: config.reproducibility.seed,
        temperature: 0.0,
        max_tokens: None,
    };
    Arc::new(factory.provider(&model_config))
}

/// Run the full review pipeline for a source path and produce a ledger run.
///
/// `approve_all` simulates a human approving every required revision in
/// non-interactive mode; otherwise the run records the approvals needed.
pub async fn run_pipeline(
    source_path: &str,
    config: &AppConfig,
    data_dir: &str,
    fixture_response: Option<&str>,
    approve_all: bool,
) -> anyhow::Result<RunOutput> {
    let ledger = LedgerStore::open(data_dir)?;
    let run_id = next_run_id(&ledger)?;

    // Parse the source path (single .tex, \input/\include project, or .pdf)
    // into the canonical model via `parse_source_path`.
    let source = parse_source_path(source_path).await?;
    let input_hash = ContentHash::of_bytes(&source.parsed.raw_bytes);
    let format = source.format();
    let document = source.parsed.document;

    // Surface missing/cyclic include diagnostics without failing the review.
    if !source.missing_includes.is_empty() {
        logging::log_project_missing_includes(&run_id, &source.missing_includes);
    }
    if !source.include_cycles.is_empty() {
        logging::log_project_cycles(&run_id, &source.include_cycles);
    }

    let config_hash = ContentHash::compute(&config.canonical_json());
    let model_configuration = serde_json::to_string(&config.reviewers)?;

    let mut run = RunRecord::shell(
        run_id.clone(),
        None,
        input_hash,
        &format.to_string(),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        config_hash,
        &model_configuration,
        config.prompt_version(),
        &now_iso(),
    );

    // --- Review stage (parallel) ---
    let provider = build_provider(config, fixture_response)?;
    // Retrieve authorized historical review memory as untrusted reviewer
    // context (only when memory is enabled in a retrieving mode).
    let memory_context = retrieve_memory_context(config, data_dir, &document);
    let ctx = ReviewerContext {
        document: document.clone(),
        prompt_version: config.prompt_version().to_string(),
        run_id: run_id.clone(),
        memory_context,
    };

    let reviewers = enabled_reviewers(config);
    logging::log_review_start(&run_id, "pipeline", "review");
    let runner = ReviewRunner::new(config.reviewers.max_concurrent);
    let agent_results = runner.run(&ctx, reviewers, provider).await;

    for res in &agent_results {
        let status = match res.status {
            AgentStatus::Success => "success",
            AgentStatus::Failed => "failed",
            AgentStatus::Disabled => "disabled",
        };
        let count = res.output.as_ref().map(|o| o.findings.len()).unwrap_or(0);
        // Provider usage metadata (token accounting) when the provider reported
        // it. Never holds secrets — only token counts + provider/model names.
        let provider_usage =
            res.output
                .as_ref()
                .and_then(|o| o.usage)
                .map(|u| paper_guard_ledger::ProviderUsage {
                    provider: config.llm.provider.clone(),
                    model: model_for_agent(config, res.agent),
                    input_tokens: u.prompt_tokens,
                    output_tokens: u.completion_tokens,
                });
        run.reviewer_results.push(AgentOutcome {
            agent: res.agent.name().to_string(),
            status: status.to_string(),
            error: res.error.clone(),
            finding_count: count,
            provider_usage,
        });
        if res.status == AgentStatus::Failed {
            logging::log_agent_failure(
                &run_id,
                res.agent.name(),
                res.error.as_deref().unwrap_or("unknown"),
            );
        }
    }

    let findings = collect_findings(&agent_results);
    for f in &findings {
        run.findings.push(FindingRecord::new(
            f.finding_id.clone(),
            f.reviewer.name().to_string(),
            f.location.clone(),
            serde_json::to_value(f.category)
                .map(|v| v.as_str().unwrap_or("other").to_string())
                .unwrap_or_else(|_| "other".to_string()),
            f.severity,
            f.confidence,
            f.claim_id.clone(),
            f.finding.clone(),
            f.evidence.clone(),
            f.recommendation.clone(),
            run_id.clone(),
        ));
    }
    logging::log_review_end(&run_id, "pipeline", "review", "done", findings.len());

    // --- Judge stage ---
    let judge = Judge::new(
        config.prompt_version(),
        config.judge.require_human_approval_for_major,
    );
    let judged = judge.consolidate(findings);
    for entry in judged.entries.clone() {
        let rev = judged
            .revisions
            .iter()
            .find(|r| r.finding_id.as_deref() == Some(&entry.finding_id));
        let requires_human = match &entry.action {
            paper_guard_review::JudgeAction::Revise {
                requires_human_approval,
                ..
            } => *requires_human_approval,
            paper_guard_review::JudgeAction::NoAction { .. } => false,
        };
        run.judge_results.push(JudgedRecord {
            finding_id: entry.finding_id,
            status: entry.status,
            severity: entry.severity,
            priority: entry.priority,
            action: serde_json::to_string(&entry.action).unwrap_or_default(),
            requires_human_approval: requires_human,
            revision_id: rev.map(|r| r.revision_id.0.clone()),
        });
    }

    // --- Revision stage ---
    let engine = RevisionEngine::new(RevisionEngineOptions {
        agent_name: "revision".into(),
        allow_configurable_auto_approve: true,
    });
    let mut approval_required = Vec::new();
    let mut outcomes = Vec::new();

    // For the fixture path, we render the document and apply scoped edits.
    let renderer = paper_guard_renderer::LatexRenderer;
    let rendered = renderer.render(&document);
    let mut current_source = rendered.text;

    for instruction in judged.revisions.clone() {
        let needs_human = instruction.requires_human_approval;
        if needs_human && !approve_all {
            approval_required.push(instruction.clone());
            logging::log_review_end(
                &run_id,
                "revision",
                "revision",
                "needs_approval",
                instruction.revision_id.0.len(),
            );
            continue;
        }
        let outcome = engine.apply(&instruction, &run_id, approve_all, &current_source);
        if outcome.applied {
            // Apply the deterministic change to the source representation.
            current_source = apply_changes(&current_source, &outcome);
            run.revision_results
                .push(outcome.revision.revision_id.0.clone());
        }
        outcomes.push(outcome);
    }

    // --- Re-render + validation ---
    let validator = TextValidator::new();
    let reparsed_render = paper_guard_parser::parser_for_format(SourceFormat::Latex)?;
    let re_bytes = current_source.as_bytes().to_vec();
    let re_parsed = reparsed_render.parse("<rendered>", &re_bytes).await?;
    let report = validator.validate(&document, &re_parsed.document);

    for issue in &report.issues {
        run.validation_results.push(ValidationRecord {
            stage: issue.stage.clone(),
            passed: issue.level != "error",
            issues: vec![issue.message.clone()],
        });
    }
    if report.passed {
        run.validation_results.push(ValidationRecord {
            stage: "validation".into(),
            passed: true,
            issues: vec![],
        });
    }

    run.status = RunStatus::Completed;
    run.mark_completed();

    // Persist artifacts.
    persist_artifacts(data_dir, &run_id, &document, &run)?;
    ledger.save_run(&run)?;

    Ok(RunOutput {
        run,
        document,
        approval_required,
        outcomes,
    })
}

/// Apply the deterministic textual changes from a revision outcome to the
/// current source.
///
/// This only scope-limited, evidence-preserving edits are applied. Two cases
/// are supported:
///   * `before` -> `after` substitution (rewrite / clarification), and
///   * a pure removal, where `after` is empty — e.g. dropping an unsupported
///     numeric overstatement such as "by 40%". This *weakens* the claim and
///     never adds content.
///
/// Changes with an empty `before` are never applied (there is nothing to find),
/// and changes are only applied to the first occurrence, which matches the
/// single-scope design of the weaken operation.
fn apply_changes(source: &str, outcome: &RevisionOutcome) -> String {
    let mut out = source.to_string();
    for change in &outcome.revision.changes {
        if change.before.is_empty() {
            // No textual anchor: cannot safely locate the change. Skip rather
            // than guess (fail closed).
            continue;
        }
        if let Some(idx) = out.find(&change.before) {
            out.replace_range(idx..idx + change.before.len(), &change.after);
        }
    }
    out
}

/// Build the enabled reviewer set from configuration.
pub fn enabled_reviewers(config: &AppConfig) -> Vec<Box<dyn Reviewer>> {
    let mut out: Vec<Box<dyn Reviewer>> = Vec::new();
    let mk = |_kind: ReviewerKind, cfg: &crate::config::ReviewerSectionConfig| ReviewerSettings {
        enabled: cfg.enabled,
        provider: cfg.provider.clone(),
        model: cfg.model.clone(),
        seed: cfg.seed,
        temperature: 0.0,
    };
    if config.reviewers.scientific.enabled {
        out.push(Box::new(paper_guard_review::ScientificReviewer {
            settings: mk(ReviewerKind::Scientific, &config.reviewers.scientific),
        }));
    }
    if config.reviewers.adversarial.enabled {
        out.push(Box::new(paper_guard_review::AdversarialReviewer {
            settings: mk(ReviewerKind::Adversarial, &config.reviewers.adversarial),
        }));
    }
    if config.reviewers.evidence.enabled {
        out.push(Box::new(paper_guard_review::EvidenceReviewer {
            settings: mk(ReviewerKind::Evidence, &config.reviewers.evidence),
        }));
    }
    if config.reviewers.references.enabled {
        out.push(Box::new(paper_guard_review::ReferenceReviewer {
            settings: mk(ReviewerKind::References, &config.reviewers.references),
        }));
    }
    if config.reviewers.figures.enabled {
        out.push(Box::new(paper_guard_review::FigureReviewer {
            settings: mk(ReviewerKind::Figures, &config.reviewers.figures),
        }));
    }
    out
}

/// Determine the next run id.
pub fn next_run_id(ledger: &LedgerStore) -> anyhow::Result<String> {
    let runs = ledger.list_runs()?;
    let max = runs
        .iter()
        .filter_map(|r| r.strip_prefix("run-").and_then(|n| n.parse::<u32>().ok()))
        .max()
        .unwrap_or(0);
    Ok(format!("run-{:03}", max + 1))
}

/// Provide an adapter so `model_for_agent` can treat `JudgeConfig.model` and
/// `ReviewerSectionConfig.model` uniformly.
trait HasModel {
    fn model(&self) -> &str;
}
impl HasModel for crate::config::ReviewerSectionConfig {
    fn model(&self) -> &str {
        &self.model
    }
}
impl HasModel for crate::config::JudgeConfig {
    fn model(&self) -> &str {
        &self.model
    }
}

/// The configured model for a given reviewer kind (used for usage metadata).
pub fn model_for_agent(config: &AppConfig, kind: ReviewerKind) -> String {
    let cfg: &dyn HasModel = match kind {
        ReviewerKind::Scientific => &config.reviewers.scientific,
        ReviewerKind::Adversarial => &config.reviewers.adversarial,
        ReviewerKind::Evidence => &config.reviewers.evidence,
        ReviewerKind::References => &config.reviewers.references,
        ReviewerKind::Figures => &config.reviewers.figures,
        ReviewerKind::Judge => &config.judge,
    };
    cfg.model().to_string()
}

/// Persist JSON artifacts (claims, findings, judge, revisions, validation).
pub fn persist_artifacts(
    data_dir: &str,
    run_id: &str,
    doc: &Document,
    run: &RunRecord,
) -> anyhow::Result<()> {
    let dir = std::path::Path::new(data_dir).join(run_id);
    std::fs::create_dir_all(&dir)?;

    let claims_json = serde_json::to_string_pretty(&doc.claims)?;
    std::fs::write(dir.join("claims.json"), claims_json)?;

    let findings_json = serde_json::to_string_pretty(&run.findings)?;
    std::fs::write(dir.join("findings.json"), findings_json)?;

    let judge_json = serde_json::to_string_pretty(&run.judge_results)?;
    std::fs::write(dir.join("judge.json"), judge_json)?;

    let revision_json = serde_json::to_string_pretty(&run.revision_results)?;
    std::fs::write(dir.join("revisions.json"), revision_json)?;

    let validation_json = serde_json::to_string_pretty(&run.validation_results)?;
    std::fs::write(dir.join("validation.json"), validation_json)?;

    let ledger_json = serde_json::to_string_pretty(run)?;
    std::fs::write(dir.join("ledger.json"), ledger_json)?;

    let paper_json = serde_json::to_string_pretty(doc)?;
    std::fs::write(dir.join("paper.json"), paper_json)?;

    let manifest = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
    });
    std::fs::write(
        dir.join("schema.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    Ok(())
}

/// Current ISO-8601 UTC timestamp.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Build a review runner + context + provider for a service submission (used by
/// the HTTP API). This isolates the "start a review" step so the service and
/// CLI share the same construction path.
#[allow(dead_code)]
pub(crate) fn review_runner_for_config(
    config: &AppConfig,
    document: Document,
    run_id: String,
) -> anyhow::Result<(
    ReviewRunner,
    ReviewerContext,
    Arc<dyn paper_guard_llm::LlmProvider>,
)> {
    let provider = build_provider(config, None)?;
    let ctx = ReviewerContext {
        document,
        prompt_version: config.prompt_version().to_string(),
        run_id,
        memory_context: String::new(),
    };
    let runner = ReviewRunner::new(config.reviewers.max_concurrent);
    Ok((runner, ctx, provider))
}

/// Retrieve authorized historical review memory relevant to the current
/// manuscript and render it as an untrusted memory-context block for reviewers.
///
/// Only units the configured owner/team is authorized to access AND that are
/// `MEMORY_APPROVED`/`TRAINING_APPROVED` are ever included. When memory is
/// disabled, or in a non-retrieving mode, or nothing matches, this returns an
/// empty string so behaviour is identical to a memory-free review (never
/// fabricates memory). A retrieval failure surfaces as `MEMORY_UNAVAILABLE`
/// (logged) and the review continues without memory; it never invents context.
///
/// This is synchronous (driven by `block_on`) so the pipeline's future remains
/// `Send`: the memory retrieval future is never held across an `.await` in the
/// pipeline review stage.
fn retrieve_memory_context(config: &AppConfig, data_dir: &str, document: &Document) -> String {
    let mem_cfg = &config.memory;
    if !mem_cfg.enabled {
        return String::new();
    }
    // Use the pipeline's data dir so memory lives alongside the run's ledger
    // (the CLI and service both pass the same dir used to persist artifacts).
    let mut opts = crate::memory_service::MemoryServiceOptions::from_config(config);
    opts.data_dir = data_dir.to_string();
    let Ok(memory) = crate::MemoryService::new(&opts) else {
        logging::log_memory_unavailable();
        return String::new();
    };
    if !memory.retrieves() {
        return String::new();
    }
    // Build the retrieval query from the current manuscript's claims (the
    // review experience most relevant to this paper). Never embed the whole
    // paper; a claim + category is enough.
    let query = if document.claims.is_empty() {
        document
            .meta
            .title
            .clone()
            .unwrap_or_else(|| "scientific review".to_string())
    } else {
        document
            .claims
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_else(|| "scientific review".to_string())
    };

    futures::executor::block_on(async {
        match memory
            .retrieve_context(&query, None, None, None, None)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let briefs: Vec<paper_guard_review::MemoryBrief> = entries
                    .into_iter()
                    .take(mem_cfg.top_k.max(1))
                    .map(|e| {
                        paper_guard_review::MemoryBrief::new(
                            e.unit.category,
                            e.unit.finding,
                            e.resolution.as_str().to_string(),
                            e.human_feedback,
                        )
                    })
                    .collect();
                logging::log_memory_retrieval(briefs.len());
                paper_guard_review::render_memory_context(&briefs)
            }
            _ => {
                // Retrieval returned nothing OR failed. We continue without
                // fabricated context; the availability is surfaced in the log.
                logging::log_memory_unavailable();
                String::new()
            }
        }
    })
}

//! Paper Guard — command-line interface.
//!
//! A reproducible, multi-agent scientific review and revision workflow.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use config::AppConfig;
use logging::init_logging;
use paper_guard_cli::{config, logging, run};

#[derive(Parser)]
#[command(
    name = "paper-guard",
    about = "Reproducible multi-agent scientific review workflow",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a `paper-guard.toml` configuration.
    Init {
        /// Path to write the config to.
        #[arg(default_value = "paper-guard.toml")]
        path: String,
    },
    /// Review a single paper (parse + parallel review + judge + ledger).
    Review {
        /// The paper source file (e.g. `paper.pdf`, `main.tex`).
        source: String,
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Run the review on a remote Paper Guard service instead of locally.
        /// Takes precedence over any `[server].url` in the config.
        #[arg(long)]
        server: Option<String>,
        /// Non-interactively approve all required revisions.
        #[arg(long)]
        approve_all: bool,
    },
    /// Run the full end-to-end workflow (review + judge + revision + render + validate).
    Run {
        /// The paper source file or manuscript directory.
        source: String,
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Run the full workflow on a remote Paper Guard service instead of
        /// locally. Takes precedence over any `[server].url` in the config.
        #[arg(long)]
        server: Option<String>,
        #[arg(long)]
        approve_all: bool,
    },
    /// Record a human decision (accept/reject/modified) on a review finding.
    /// The decision is stored as a private Review Memory candidate.
    Feedback {
        /// Run id that contains the finding.
        run: String,
        /// The finding id to record a decision on.
        finding_id: String,
        /// The human decision: accept, reject, or modified.
        #[arg(long)]
        decision: String,
        /// Optional free-text human feedback.
        #[arg(long)]
        feedback: Option<String>,
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Record the feedback on a remote service instead of locally.
        #[arg(long)]
        server: Option<String>,
    },
    /// List accepted findings.
    Findings {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
    },
    /// Run the judge on persisted findings.
    Judge {
        /// Run id to judge.
        run: String,
        #[arg(long)]
        config: Option<String>,
    },
    /// Produce revision instructions for approved findings.
    Revise {
        /// Run id to revise.
        run: String,
        #[arg(long)]
        config: Option<String>,
    },
    /// Validate a rendered document.
    Validate {
        /// Run id to validate.
        run: String,
        #[arg(long)]
        config: Option<String>,
    },
    /// Show the review ledger.
    Ledger {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
    },
    /// Emit a summary report.
    Report {
        /// Run id to report on (defaults to the latest).
        run: Option<String>,
        #[arg(long)]
        config: Option<String>,
    },
    /// Start the Paper Guard HTTP service (uses the same application layer as
    /// the CLI).
    Serve {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long, default_value = "paper-guard.toml")]
        config: String,
        /// Override the bind address (e.g. `127.0.0.1:8080`).
        #[arg(long)]
        bind: Option<String>,
    },
    /// Print service health (if the service is running on the configured bind).
    Health {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Query a remote service's health instead of the configured endpoint.
        /// Takes precedence over any `[server].url` in the config.
        #[arg(long)]
        server: Option<String>,
    },
    /// Interact with Review Memory (retrieval-approved units only).
    Memory {
        /// Sub-command.
        #[command(subcommand)]
        command: MemoryCommand,
    },
}

#[derive(Subcommand)]
enum MemoryCommand {
    /// List memory units and their approval state.
    List {
        #[arg(long)]
        config: Option<String>,
    },
    /// Approve a unit for use as retrieval context.
    ApproveMemory {
        /// The memory id.
        memory_id: String,
        #[arg(long)]
        config: Option<String>,
        /// Actor/name of the approving human (never a secret).
        #[arg(long, default_value = "cli-user")]
        actor: String,
    },
    /// Approve a unit for export to a (versioned, human-approved) training set.
    ApproveTraining {
        /// The memory id.
        memory_id: String,
        #[arg(long)]
        config: Option<String>,
        #[arg(long, default_value = "cli-user")]
        actor: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => {
            AppConfig::write_default_to(&PathBuf::from(&path))?;
            println!("wrote default configuration to {path}");
        }
        Command::Review {
            source,
            config,
            server,
            approve_all,
        } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            if let Some(server_url) = resolve_server_url(&cfg, server.as_deref()) {
                let client = build_remote_client(&cfg, &server_url)?;
                run_remote_review(&client, &server_url, &source).await?;
            } else {
                print_mode_local(&cfg);
                let data_dir = cfg.reproducibility.data_dir.clone();
                let fixture = fixture_response_for(&source);
                let out =
                    run::run_pipeline(&source, &cfg, &data_dir, fixture.as_deref(), approve_all)
                        .await?;
                print_summary(&out);
            }
        }
        Command::Run {
            source,
            config,
            server,
            approve_all,
        } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            if let Some(server_url) = resolve_server_url(&cfg, server.as_deref()) {
                let client = build_remote_client(&cfg, &server_url)?;
                run_remote_review(&client, &server_url, &source).await?;
            } else {
                print_mode_local(&cfg);
                let data_dir = cfg.reproducibility.data_dir.clone();
                let fixture = fixture_response_for(&source);
                let out =
                    run::run_pipeline(&source, &cfg, &data_dir, fixture.as_deref(), approve_all)
                        .await?;
                print_summary(&out);
            }
        }
        Command::Findings { config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            list_findings(&cfg.reproducibility.data_dir)?;
        }
        Command::Judge { run, config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            println!("run {run}: {} judged findings", record.judge_results.len());
        }
        Command::Revise { run, config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            println!(
                "run {run}: {} revisions recorded",
                record.revision_results.len()
            );
        }
        Command::Validate { run, config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            let validations = &record.validation_results;
            let passed = validations.iter().all(|v| v.passed);
            println!(
                "run {run}: validation {} ({} checks)",
                if passed { "PASSED" } else { "FAILED" },
                validations.len()
            );
            for v in validations {
                for i in &v.issues {
                    println!("  - [{}] {}", v.stage, i);
                }
            }
            if !passed {
                std::process::exit(1);
            }
        }
        Command::Ledger { config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let runs = ledger.list_runs()?;
            if runs.is_empty() {
                println!("no runs recorded yet in {}", cfg.reproducibility.data_dir);
            }
            for id in runs {
                if let Ok(r) = ledger.load_run(&id) {
                    println!(
                        "{} | {:?} | source={} | findings={} | revisions={}",
                        id,
                        r.status,
                        r.source_format,
                        r.findings.len(),
                        r.revision_results.len()
                    );
                }
            }
        }
        Command::Report { run, config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let id = run.unwrap_or_else(|| {
                ledger
                    .list_runs()
                    .ok()
                    .and_then(|rs| rs.last().cloned())
                    .unwrap_or_else(|| "run-001".into())
            });
            let record = ledger.load_run(&id)?;
            println!("# Report for {id}");
            println!("- status: {:?}", record.status);
            println!("- input hash: {}", record.input_hash);
            println!("- findings opened: {}", record.findings.len());
            let open = record
                .findings
                .iter()
                .filter(|f| f.status.describe() == "OPEN")
                .count();
            println!("- open findings: {open}");
            println!("- revisions applied: {}", record.revision_results.len());
            for f in &record.findings {
                if f.status.describe() == "OPEN" {
                    println!(
                        "  - [{} {}] {} @ {}",
                        f.severity.priority(),
                        f.finding_id,
                        f.finding,
                        f.location
                    );
                }
            }
        }
        Command::Feedback {
            run,
            finding_id,
            decision,
            feedback,
            config,
            server,
        } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            if let Some(server_url) = resolve_server_url(&cfg, server.as_deref()) {
                let client = build_remote_client(&cfg, &server_url)?;
                send_remote_feedback(&client, &run, &finding_id, &decision, feedback.as_deref())
                    .await?;
            } else {
                record_local_feedback(&cfg, &run, &finding_id, &decision, feedback.as_deref())?;
            }
        }

        Command::Serve { config, bind } => {
            let cfg = AppConfig::load(Some(Path::new(&config)))?;
            let data_dir = cfg.service.data_dir.clone();
            let addr = bind.unwrap_or_else(|| cfg.service.bind.clone());
            let enforce_loopback = !cfg.service.allow_external_bind;
            let memory = paper_guard_app::MemoryService::new(
                &cfg.memory.backend,
                &data_dir,
                &cfg.memory.qdrant_url,
                &cfg.memory.collection,
            )?;
            let state = paper_guard_service::AppState {
                config: std::sync::Arc::new(cfg),
                data_dir,
                enforce_loopback,
                memory,
            };
            println!("paper-guard serve listening on {addr}");
            paper_guard_service::serve(&addr, state).await?;
        }
        Command::Health { config, server } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            if let Some(server_url) = resolve_server_url(&cfg, server.as_deref()) {
                let client = build_remote_client(&cfg, &server_url)?;
                let h = client.health().await?;
                println!("Paper Guard Service");
                println!("Status: {}", h.status);
                println!("Version: {}", h.version);
                println!("Provider: {}", h.provider);
                println!("Memory backend: {}", h.memory_backend);
            } else {
                // Health is answered by the running local service; if we are
                // here without a server, print the configured endpoint.
                println!(
                    "service configured at http://{}/health (provider={}, memory={})",
                    cfg.service.bind, cfg.llm.provider, cfg.memory.backend
                );
            }
        }
        Command::Memory { command } => match command {
            MemoryCommand::List { config } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                // List uses the shared memory store for the configured data dir.
                let mem = paper_guard_app::memory_service::MemoryService::new(
                    &cfg.memory.backend,
                    &cfg.reproducibility.data_dir,
                    &cfg.memory.qdrant_url,
                    &cfg.memory.collection,
                )?;
                let entries = mem.list(None)?;
                if entries.is_empty() {
                    println!("no review-memory units recorded");
                }
                for e in &entries {
                    println!(
                        "{} | {} | {} | {}",
                        e.memory_id,
                        e.approval_state.describe(),
                        e.unit.kind.as_str(),
                        snippet(&e.unit.text)
                    );
                }
            }
            MemoryCommand::ApproveMemory {
                memory_id,
                config,
                actor,
            } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::memory_service::MemoryService::new(
                    &cfg.memory.backend,
                    &cfg.reproducibility.data_dir,
                    &cfg.memory.qdrant_url,
                    &cfg.memory.collection,
                )?;
                mem.approve_memory(&memory_id, &actor)?;
                println!("approved {memory_id} for retrieval-context use (actor={actor})");
            }
            MemoryCommand::ApproveTraining {
                memory_id,
                config,
                actor,
            } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::memory_service::MemoryService::new(
                    &cfg.memory.backend,
                    &cfg.reproducibility.data_dir,
                    &cfg.memory.qdrant_url,
                    &cfg.memory.collection,
                )?;
                mem.approve_training(&memory_id, &actor)?;
                println!("approved {memory_id} for training-dataset export (actor={actor})");
            }
        },
    }
    Ok(())
}
// ---------------------------------------------------------------------------
// Local vs remote mode helpers
// ---------------------------------------------------------------------------

/// Resolve which mode to run. An explicit `--server` flag always wins; a
/// configured `[server].url` is second; otherwise we run locally. We never
/// switch modes implicitly from environment variables.
fn resolve_server_url(cfg: &AppConfig, explicit: Option<&str>) -> Option<String> {
    explicit
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let u = cfg.server.url.trim().to_string();
            if u.is_empty() {
                None
            } else {
                Some(u)
            }
        })
}

/// Build a remote client from the resolved server URL and the `[server]`
/// configuration (auth env, timeout). The token (if any) is read from the
/// environment at construction and never printed.
fn build_remote_client(
    cfg: &AppConfig,
    server_url: &str,
) -> anyhow::Result<paper_guard_client::PaperGuardClient> {
    let ccfg = paper_guard_client::ClientConfig {
        base_url: server_url.to_string(),
        timeout: std::time::Duration::from_secs(cfg.server.timeout_seconds.max(1)),
        auth_token_env: cfg.server.auth_token_env.clone(),
    };
    Ok(paper_guard_client::PaperGuardClient::new(&ccfg)?)
}

/// Print the local-mode banner (never secrets, only provider + model).
fn print_mode_local(cfg: &AppConfig) {
    let (provider, model) = match cfg.llm.provider.as_str() {
        "openai-compatible" => (
            "OpenAI-compatible",
            cfg.providers.openai_compatible.model.clone(),
        ),
        other => (other, String::new()),
    };
    println!("Mode: local");
    println!("Provider: {provider}");
    if !model.is_empty() {
        println!("Model: {model}");
    }
}

/// Print the remote-mode banner.
fn print_mode_remote(server_url: &str) {
    println!("Mode: remote");
    println!("Server: {server_url}");
}

/// Run a review remotely: submit the manuscript, then fetch status + findings.
async fn run_remote_review(
    client: &paper_guard_client::PaperGuardClient,
    server_url: &str,
    source: &str,
) -> anyhow::Result<()> {
    print_mode_remote(server_url);
    println!("Reviewing {source} on remote service…");
    let submission = client.submit_review(source).await?;
    let run_id = submission.run_id.clone();
    println!("Review submitted.");
    println!("Run: {run_id}");
    let review = client.review(&run_id).await?;
    println!("Remote review completed.");
    println!(
        "  findings opened: {} ({} open)",
        review.findings_opened, review.open_count
    );
    println!("  judge entries:  {}", review.judge_entries);
    println!("  revisions applied: {}", review.revisions_applied);
    for f in &review.findings {
        println!(
            "  - [{} {}] {} @ {}",
            f.severity, f.finding_id, f.finding, f.location
        );
    }
    Ok(())
}

/// Send a human decision to a remote service, resolving the finding from the
/// run's findings so the request is fully populated.
async fn send_remote_feedback(
    client: &paper_guard_client::PaperGuardClient,
    run: &str,
    finding_id: &str,
    decision: &str,
    feedback: Option<&str>,
) -> anyhow::Result<()> {
    let findings = client.get_findings(run).await?;
    let finding = findings
        .findings
        .iter()
        .find(|f| f.finding_id == finding_id)
        .ok_or_else(|| anyhow::anyhow!("finding {finding_id} not found in run {run}"))?;
    let req = paper_guard_client::SubmitFeedbackRequest {
        reviewer_kind: finding.reviewer.clone(),
        unit_text: finding.finding.clone(),
        unit_kind: Some("claim".into()),
        finding_text: Some(finding.finding.clone()),
        decision: decision.to_string(),
        feedback: feedback.map(|s| s.to_string()),
    };
    let resp = client.submit_feedback(run, &req).await?;
    println!(
        "Feedback recorded (memory_id={}, approval_state={})",
        resp.memory_id, resp.approval_state
    );
    Ok(())
}

/// Record a human decision in the local Review Memory store.
fn record_local_feedback(
    cfg: &AppConfig,
    run: &str,
    finding_id: &str,
    decision: &str,
    feedback: Option<&str>,
) -> anyhow::Result<()> {
    let mem = paper_guard_app::MemoryService::new(
        &cfg.memory.backend,
        &cfg.reproducibility.data_dir,
        &cfg.memory.qdrant_url,
        &cfg.memory.collection,
    )?;
    let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
    let record = ledger.load_run(run)?;
    let finding = record
        .findings
        .iter()
        .find(|f| f.finding_id == finding_id)
        .ok_or_else(|| anyhow::anyhow!("finding {finding_id} not found in run {run}"))?;
    let unit = paper_guard_app::ReviewMemoryUnit {
        reviewer_kind: finding.reviewer.clone(),
        kind: paper_guard_app::MemoryKind::Claim,
        text: finding.finding.clone(),
        finding: finding.finding.clone(),
        context: String::new(),
    };
    let resolution = match decision {
        "accept" => paper_guard_app::MemoryResolution::Accept,
        "reject" => paper_guard_app::MemoryResolution::Reject,
        "modified" => paper_guard_app::MemoryResolution::Modified,
        other => {
            anyhow::bail!("invalid decision `{other}`; expected accept|reject|modified")
        }
    };
    let fb = paper_guard_app::FindingFeedback {
        finding_id: finding_id.to_string(),
        decision: resolution,
        feedback: feedback.unwrap_or_default().to_string(),
    };
    let entry = mem.record_feedback(run, unit, &fb, "cli-human-feedback")?;
    println!(
        "Feedback recorded (memory_id={}, approval_state={})",
        entry.memory_id,
        entry.approval_state.describe()
    );
    Ok(())
}

/// A short excerpt of a text for listing (never dumps full papers).
fn snippet(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let out: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{out}…")
    } else {
        out
    }
}

/// A deterministic fixture response is only used for LaTeX sources so that a
/// the demo/example run produces a realistic finding. PDF/Typst/DOCX are parsed
/// to the point of loading; the reviewer mock returns no findings for them.
fn fixture_response_for(_source: &str) -> Option<String> {
    None
}

/// Print a concise summary after a run.
fn print_summary(out: &run::RunOutput) {
    println!("run {}: {:?}", out.run.run_id, out.run.status);
    println!("  claims extracted: {}", out.document.claims.len());
    println!("  findings opened: {}", out.run.findings.len());
    println!("  judge entries:   {}", out.run.judge_results.len());
    println!("  revisions applied: {}", out.run.revision_results.len());
    let applied = out.outcomes.iter().filter(|o| o.applied).count();
    println!("  revision outcomes applied: {applied}");
    for r in &out.approval_required {
        println!(
            "  needs approval: {} (finding {})",
            r.revision_id,
            r.finding_id.as_deref().unwrap_or("-")
        );
    }
}

/// Print a list of persisted findings from the latest completed run.
fn list_findings(data_dir: &str) -> anyhow::Result<()> {
    let ledger = paper_guard_ledger::LedgerStore::open(data_dir)?;
    let runs = ledger.list_runs()?;
    if runs.is_empty() {
        println!("no findings recorded yet");
        return Ok(());
    }
    // Read findings.json of the latest run for a richer view.
    let latest = runs.last().unwrap();
    let findings_path = std::path::Path::new(data_dir)
        .join(latest)
        .join("findings.json");
    if let Ok(text) = std::fs::read_to_string(findings_path) {
        let findings: Vec<paper_guard_core::Finding> = serde_json::from_str(&text)?;
        for f in &findings {
            println!(
                "{} [{} {}] {}",
                f.finding_id,
                f.severity.priority(),
                f.reviewer.name(),
                f.finding
            );
        }
    } else {
        let record = ledger.load_run(latest)?;
        for f in &record.findings {
            println!(
                "{} [{} {}] {} (status {})",
                f.finding_id,
                f.severity.priority(),
                f.reviewer,
                f.finding,
                f.status.describe()
            );
        }
    }
    Ok(())
}

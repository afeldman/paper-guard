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
    /// Discover Paper Guard services on the local network via mDNS/DNS-SD.
    ///
    /// Lists and verifies candidate services through `GET /health`. This never
    /// uploads any manuscript and never selects a service automatically unless
    /// `[discovery] mode = "auto"` and `preferred_service` are configured.
    Discover {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Force discovery even when `[discovery]` is disabled in the config.
        /// When set, discovery runs in manual (list-only) mode for a single run.
        #[arg(long)]
        force: bool,
    },
    /// Print non-secret build and platform diagnostics (version, OS triple,
    /// commit, build profile, and resolved config paths). Never prints secrets
    /// or manuscript contents.
    Diagnostics {
        /// Also show the resolved platform config/data/cache/log directories.
        #[arg(long)]
        paths: bool,
    },
    /// Print version and platform identity. A stub of `--version` that also
    /// reports the build profile and commit without any review output.
    Info,

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
    /// Show the full detail of a single memory unit.
    Show {
        /// The memory id.
        memory_id: String,
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
    /// Reject a unit (explicit human rejection). It is removed from retrieval
    /// and export eligibility.
    Reject {
        /// The memory id.
        memory_id: String,
        #[arg(long)]
        config: Option<String>,
        #[arg(long, default_value = "cli-user")]
        actor: String,
    },
    /// Semantic search over approved review memory.
    Search {
        /// A natural-language query (e.g. "unsupported causal claim").
        query: String,
        #[arg(long)]
        config: Option<String>,
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
                record_local_feedback(&cfg, &run, &finding_id, &decision, feedback.as_deref())
                    .await?;
            }
        }

        Command::Serve { config, bind } => {
            let cfg = AppConfig::load(Some(Path::new(&config)))?;
            let data_dir = cfg.service.data_dir.clone();
            let addr = bind.unwrap_or_else(|| cfg.service.bind.clone());
            let enforce_loopback = !cfg.service.allow_external_bind;
            let memory = paper_guard_app::MemoryService::from_config(&cfg)?;
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
        Command::Discover { config, force } => {
            run_discover(config.as_deref(), force).await?;
        }
        Command::Diagnostics { paths } => {
            print_diagnostics(paths);
        }
        Command::Info => {
            println!(
                "Paper Guard {} ({}, {}) commit={} profile={}",
                paper_guard_app::build_info::version(),
                paper_guard_app::build_info::os_triple(),
                paper_guard_app::build_info::os_family(),
                paper_guard_app::build_info::commit(),
                paper_guard_app::build_info::build_profile()
            );
            println!(
                "config={}|data={}",
                platform_or_none(paper_guard_app::paths::config_dir()),
                platform_or_none(paper_guard_app::paths::data_dir())
            );
        }
        Command::Memory { command } => match command {
            MemoryCommand::List { config } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                let entries = mem.list(None)?;
                if entries.is_empty() {
                    println!("no review-memory units recorded");
                }
                for e in &entries {
                    println!(
                        "{} | {} ({}) | {} | {}",
                        e.memory_id,
                        e.approval_state.describe(),
                        e.scope.describe(),
                        e.unit.kind.as_str(),
                        snippet(&e.unit.text)
                    );
                }
            }
            MemoryCommand::Show { memory_id, config } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                let Some(e) = mem.load(&memory_id)? else {
                    anyhow::bail!("memory unit {memory_id} not found");
                };
                println!("memory id:    {}", e.memory_id);
                println!("schema:       {}", e.schema_version);
                println!("source run:   {}", e.source_run_id);
                println!("finding id:   {}", e.source_finding_id);
                println!("reviewer:     {}", e.unit.reviewer_kind);
                println!("kind:         {}", e.unit.kind.as_str());
                println!("category:     {}", e.unit.category);
                println!(
                    "scope:        {} (owner={}{})",
                    e.scope.describe(),
                    if e.owner_id.is_empty() {
                        "-"
                    } else {
                        &e.owner_id
                    },
                    if e.team_id.is_empty() {
                        String::new()
                    } else {
                        format!(" team={}", e.team_id)
                    }
                );
                println!("approval:     {}", e.approval_state.describe());
                println!("decision:     {}", e.resolution.as_str());
                println!("claim ctx:    {}", e.unit.claim_context);
                println!("evidence ctx: {}", e.unit.evidence_context);
                println!("finding:      {}", e.unit.finding);
                if !e.human_feedback.is_empty() {
                    println!("human fb:     {}", e.human_feedback);
                }
                println!("created:      {}", e.created_at);
            }
            MemoryCommand::ApproveMemory {
                memory_id,
                config,
                actor,
            } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.approve_memory(&memory_id, &actor).await?;
                println!("approved {memory_id} for retrieval-context use (actor={actor})");
            }
            MemoryCommand::ApproveTraining {
                memory_id,
                config,
                actor,
            } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.approve_training(&memory_id, &actor).await?;
                println!("approved {memory_id} for training-dataset export (actor={actor})");
            }
            MemoryCommand::Reject {
                memory_id,
                config,
                actor,
            } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.reject_memory(&memory_id, &actor).await?;
                println!("rejected {memory_id} (actor={actor})");
            }
            MemoryCommand::Search { query, config } => {
                let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                let hits = mem.search(&query, None, None).await?;
                if hits.is_empty() {
                    println!("no approved review memory matched");
                }
                for h in &hits {
                    println!(
                        "{} | {} ({}) | sim={:.2} | {}",
                        h.entry.memory_id,
                        h.entry.approval_state.describe(),
                        h.entry.scope.describe(),
                        h.similarity,
                        snippet(&h.entry.unit.finding)
                    );
                }
            }
        },
    }
    Ok(())
}
// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Run `paper-guard discover`: browse the LAN for Paper Guard services via
/// mDNS/DNS-SD, verify each candidate through `GET /health`, and report the
/// healthy, compatible services. **No manuscript is ever uploaded by this
/// command.**
async fn run_discover(config_path: Option<&str>, force: bool) -> anyhow::Result<()> {
    use paper_guard_discovery::model::DiscoveryConfig as DiscCfg;
    use paper_guard_discovery::verify::verify_and_classify;
    use paper_guard_discovery::ServiceDiscovery;

    let cfg = AppConfig::load(config_path.map(PathBuf::from).as_deref())?;

    let disc: DiscCfg = DiscCfg {
        enabled: cfg.discovery.enabled,
        mode: cfg.discovery.mode.clone(),
        service_type: cfg.discovery.service_type.clone(),
        timeout_ms: cfg.discovery.timeout_ms,
        preferred_service: cfg.discovery.preferred_service.clone(),
    };

    // Discovery is off unless `[discovery]` enables it, or the user forced a
    // one-shot manual browse. We never probe the network implicitly.
    if !force && !disc.enabled {
        println!("LAN discovery is disabled. Enable it in your config:");
        println!();
        println!("  [discovery]");
        println!("  enabled = true");
        println!("  mode = \"manual\"   # or \"auto\"");
        println!();
        println!("then re-run `paper-guard discover`. (Use --force to run once.)");
        return Ok(());
    }

    rust_loguru::info!("event=discovery_started | mode={:?}", disc.effective_mode());
    println!("Searching local network for Paper Guard services…");

    let provider = paper_guard_discovery::MdnsServiceDiscovery::new()
        .with_service_type(&disc.service_type)
        .with_timeout(std::time::Duration::from_millis(disc.timeout_ms));
    let candidates = provider.discover().await?;

    rust_loguru::info!(
        "event=discovery_completed | candidates={}",
        candidates.len()
    );

    if candidates.is_empty() {
        println!("No Paper Guard services found on the local network.");
        return Ok(());
    }

    // Verify each candidate through GET /health. This performs no manuscript
    // transmission and uses a short, token-free client.
    let our_version = env!("CARGO_PKG_VERSION");
    let mut healthy = 0usize;
    let mut rejected = 0usize;
    let mut incompatible = 0usize;

    println!("Found:");
    for cand in &candidates {
        rust_loguru::info!(
            "event=candidate_found | name={} | host={} | addr={} | port={}",
            cand.name,
            cand.hostname,
            cand.address,
            cand.port
        );
        let verified = verify_and_classify(cand.clone(), our_version).await;
        match verified.outcome {
            paper_guard_discovery::verify::VerificationOutcome::Verified => {
                rust_loguru::info!(
                    "event=candidate_verified | name={} | version={}",
                    verified.endpoint.name,
                    verified.endpoint.version
                );
                healthy += 1;
                print_endpoint(&verified.endpoint, "healthy");
                println!("  Status: healthy");
            }
            paper_guard_discovery::verify::VerificationOutcome::IncompatibleVersion => {
                rust_loguru::warn!(
                    "event=candidate_rejected | reason=incompatible_version | name={}",
                    verified.endpoint.name
                );
                incompatible += 1;
                print_endpoint(&verified.endpoint, "incompatible-version");
                println!("  Status: INCOMPATIBLE_SERVICE_VERSION");
            }
            paper_guard_discovery::verify::VerificationOutcome::Rejected => {
                rust_loguru::warn!(
                    "event=candidate_rejected | reason=health_failed | name={}",
                    verified.endpoint.name
                );
                rejected += 1;
                print_endpoint(&verified.endpoint, "unreachable");
                println!("  Status: unreachable / not a Paper Guard service");
            }
        }
    }

    println!();
    println!(
        "Summary: {healthy} healthy, {incompatible} incompatible, {rejected} unreachable.          Discovery never uploads a manuscript."
    );
    Ok(())
}

/// Print an endpoint's identifying fields (never secrets, never manuscript
/// contents).
fn print_endpoint(ep: &paper_guard_discovery::ServiceEndpoint, status: &str) {
    println!();
    println!("  Name:     {}", ep.name);
    println!("  Host:     {}", ep.hostname);
    println!("  Address:  {}", ep.address);
    println!("  Port:     {}", ep.port);
    if !ep.version.is_empty() {
        println!("  Version:  {}", ep.version);
    }
    if !ep.capabilities.is_empty() {
        println!("  Capabilities: {}", ep.capabilities.join(", "));
    }
    let _ = status;
}

/// Render an optional platform path as a string for diagnostics; never leaks a
/// missing directory, and never contains secret material.
fn platform_or_none(p: Option<PathBuf>) -> String {
    match p {
        Some(p) => p.to_string_lossy().into_owned(),
        None => "(unresolved)".into(),
    }
}

/// Print non-secret build/platform diagnostics. `paths` additionally shows the
/// resolved platform config/data/cache/log directories (documented locations).
fn print_diagnostics(show_paths: bool) {
    use paper_guard_app::build_info;
    use paper_guard_app::paths;
    println!(
        "Paper Guard {} ({}, {})",
        build_info::version(),
        build_info::os_triple(),
        build_info::os_family()
    );
    println!(
        "commit={} profile={}",
        build_info::commit(),
        build_info::build_profile()
    );
    if show_paths {
        println!("config_dir={}", platform_or_none(paths::config_dir()));
        println!("data_dir={}", platform_or_none(paths::data_dir()));
        println!("cache_dir={}", platform_or_none(paths::cache_dir()));
        println!("log_dir={}", platform_or_none(paths::log_dir()));
        println!(
            "default_config_path={}",
            platform_or_none(paths::default_config_path())
        );
        println!("default_data_dir={}", paths::default_data_dir());
    }
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
        claim_context: finding.claim_id.as_ref().map(|c| c.to_string()),
        evidence_context: Some(finding.evidence.clone().join("; ")).filter(|s| !s.is_empty()),
        category: Some(finding.category.clone()),
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
async fn record_local_feedback(
    cfg: &AppConfig,
    run: &str,
    finding_id: &str,
    decision: &str,
    feedback: Option<&str>,
) -> anyhow::Result<()> {
    let mem = paper_guard_app::MemoryService::from_config(cfg)?;
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
        claim_context: finding
            .claim_id
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_default(),
        evidence_context: finding.evidence.join("; "),
        category: finding.category.clone(),
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
    match mem
        .record_feedback(run, finding_id, unit, &fb, "cli-human-feedback")
        .await?
    {
        Some(entry) => println!(
            "Feedback recorded (memory_id={}, approval_state={})",
            entry.memory_id,
            entry.approval_state.describe()
        ),
        None => println!(
            "Feedback accepted; review memory is disabled or in read-only mode, no memory candidate stored"
        ),
    }
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

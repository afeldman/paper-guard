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
    /// Start the local web GUI (binds to 127.0.0.1 by default).
    #[arg(long, global = true)]
    gui: bool,

    #[command(subcommand)]
    command: Option<Command>,
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
        /// Presentation style for the human-readable report: `neutral`,
        /// `funny`, or `insulting` (defaults to the `[review] style` config,
        /// then `neutral`). Style is purely presentational and never alters
        /// the canonical findings.
        #[arg(long)]
        style: Option<String>,
        /// Output format for the human-readable report: `human` (default) or
        /// `summary`. `JSON` artifacts are always written regardless.
        #[arg(long)]
        output: Option<String>,
        /// Also run Bibliography Verification (M10) in this review. Equivalent
        /// to `[bibliography] enabled = true` in the configuration; the
        /// provider still comes from the configuration.
        #[arg(long)]
        bibliography: bool,
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
        /// Presentation style for the human-readable report: `neutral`,
        /// `funny`, or `insulting` (defaults to the `[review] style` config,
        /// then `neutral`). Style is purely presentational and never alters
        /// the canonical findings.
        #[arg(long)]
        style: Option<String>,
        /// Output format for the human-readable report: `human` (default) or
        /// `summary`. `JSON` artifacts are always written regardless.
        #[arg(long)]
        output: Option<String>,
        /// Also run Bibliography Verification (M10) in this run. Equivalent
        /// to `[bibliography] enabled = true` in the configuration; the
        /// provider still comes from the configuration.
        #[arg(long)]
        bibliography: bool,
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
    /// Inspect a source document (LaTeX project or PDF) without running a
    /// review: report how it was parsed, resolved includes, page counts, and
    /// missing/cyclic structural diagnostics.
    Inspect {
        /// The paper source to inspect (e.g. `paper.pdf`, `main.tex`).
        source: String,
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
    },
    /// Print version and platform identity. A stub of `--version` that also
    /// reports the build profile and commit without any review output.
    Info {
        /// Path to a `paper-guard.toml` (optional). When given, the prompts
        /// block reflects the prompt directory configured in that file.
        #[arg(long)]
        config: Option<String>,
    },

    /// Interact with Review Memory (retrieval-approved units only).
    Memory {
        /// Sub-command.
        #[command(subcommand)]
        command: MemoryCommand,
    },

    /// Manage external reviewer prompts (copy defaults, list prompt sources).
    ///
    /// Reviewer prompts are plain files in the prompt directory
    /// (`~/.paper-guard/config/prompts` by default). They can be edited freely —
    /// prompt changes take effect without recompiling Paper Guard.
    Prompts {
        #[command(subcommand)]
        command: PromptsCommand,
    },

    /// Create the canonical per-user directory layout
    /// (`~/.paper-guard/{config,prompts,logs,data}`), write a default
    /// `config.toml`, and export the embedded default reviewer prompts.
    ///
    /// Idempotent: existing user files are never overwritten. Setup is
    /// optional — the binary works out of the box without it.
    Setup,

    /// Verify a paper's bibliography against scholarly sources (M10).
    ///
    /// Opt-in network feature: disabled unless `[bibliography] enabled = true`
    /// in the configuration. Sends only bibliographic metadata of each
    /// reference (title, authors, year, arXiv id, DOI) — never manuscript
    /// text. arXiv is the supported source; Google Scholar is deliberately not
    /// automated and reports `Unavailable`. With `--clear-cache`, deletes the
    /// local response cache instead of running a verification.
    Bibliography {
        /// The paper source to check (e.g. `paper.pdf`, `main.tex`). Optional
        /// only when `--clear-cache` is used.
        source: Option<String>,
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
        /// Output format: `human` (default) or `json`.
        #[arg(long)]
        output: Option<String>,
        /// Delete the local bibliography response cache and exit.
        #[arg(long)]
        clear_cache: bool,
    },

    /// Inspect or edit the effective Paper Guard configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show the effective configuration (user config or built-in defaults).
    ///
    /// Secrets are never printed: API keys, tokens and credentials are
    /// redacted even if they accidentally appear in a config value.
    Show {
        /// Path to a specific `paper-guard.toml` to display instead of the
        /// resolved effective configuration.
        #[arg(long)]
        config: Option<String>,
    },
    /// Edit the user configuration (`~/.paper-guard/config/config.toml`) in
    /// the platform editor ($VISUAL/$EDITOR, or a platform default).
    Edit {
        /// Edit this file instead of the canonical user configuration.
        #[arg(long)]
        config: Option<String>,
    },
}

#[derive(Subcommand)]
enum PromptsCommand {
    /// Copy the embedded default prompts into the prompt directory.
    ///
    /// Existing files are never overwritten, so locally edited prompts are
    /// preserved.
    Init {
        /// Path to a `paper-guard.toml` (optional; default prompt directory
        /// is used when omitted).
        #[arg(long)]
        config: Option<String>,
    },
    /// Show which prompt source each role resolves to (no prompt contents).
    List {
        /// Path to a `paper-guard.toml` (optional).
        #[arg(long)]
        config: Option<String>,
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

    // `paper-guard --gui` starts the local web GUI (localhost-only by default).
    if cli.gui {
        // `--config` is handled as part of subcommands, but for the GUI we
        // look for the default config path (or a `paper-guard.toml` in the
        // current directory).
        let opts = paper_guard_gui::GuiOptions {
            config_path: std::fs::metadata("paper-guard.toml")
                .ok()
                .map(|_| "paper-guard.toml".to_string()),
            bind: None,
            open_browser: true,
        };
        paper_guard_gui::start_gui(&opts).await?;
        return Ok(());
    }

    let Some(command) = cli.command else {
        // No subcommand and no GUI flag: print usage.
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        cmd.print_help()?;
        println!();
        return Ok(());
    };

    match command {
        Command::Init { path } => {
            AppConfig::write_default_to(&PathBuf::from(&path))?;
            println!("wrote default configuration to {path}");
        }
        Command::Review {
            source,
            config,
            server,
            approve_all,
            style,
            output,
            bibliography,
        } => {
            let mut cfg = load_cfg(&config)?;
            if bibliography {
                cfg.bibliography.enabled = true;
            }
            let style = resolve_review_style(style.as_deref(), &cfg)?;
            let output_mode = resolve_output_mode(output.as_deref())?;
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
                print_run_output(&out, &source, &cfg, style, output_mode);
            }
        }
        Command::Run {
            source,
            config,
            server,
            approve_all,
            style,
            output,
            bibliography,
        } => {
            let mut cfg = load_cfg(&config)?;
            if bibliography {
                cfg.bibliography.enabled = true;
            }
            let style = resolve_review_style(style.as_deref(), &cfg)?;
            let output_mode = resolve_output_mode(output.as_deref())?;
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
                print_run_output(&out, &source, &cfg, style, output_mode);
            }
        }
        Command::Findings { config } => {
            let cfg = load_cfg(&config)?;
            list_findings(&cfg.reproducibility.data_dir)?;
        }
        Command::Judge { run, config } => {
            let cfg = load_cfg(&config)?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            println!("run {run}: {} judged findings", record.judge_results.len());
        }
        Command::Revise { run, config } => {
            let cfg = load_cfg(&config)?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            println!(
                "run {run}: {} revisions recorded",
                record.revision_results.len()
            );
        }
        Command::Validate { run, config } => {
            let cfg = load_cfg(&config)?;
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
            let cfg = load_cfg(&config)?;
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
            let cfg = load_cfg(&config)?;
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
            let cfg = load_cfg(&config)?;
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
            let cfg = load_cfg(&config)?;
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
        Command::Inspect { source, config } => {
            let cfg = load_cfg(&config)?;
            run_inspect(&cfg, &source).await?;
        }
        Command::Info { config } => {
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
            let cfg = load_cfg(&config)?;
            print_prompt_status(&cfg);
        }
        Command::Memory { command } => match command {
            MemoryCommand::List { config } => {
                let cfg = load_cfg(&config)?;
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
                let cfg = load_cfg(&config)?;
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
                let cfg = load_cfg(&config)?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.approve_memory(&memory_id, &actor).await?;
                println!("approved {memory_id} for retrieval-context use (actor={actor})");
            }
            MemoryCommand::ApproveTraining {
                memory_id,
                config,
                actor,
            } => {
                let cfg = load_cfg(&config)?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.approve_training(&memory_id, &actor).await?;
                println!("approved {memory_id} for training-dataset export (actor={actor})");
            }
            MemoryCommand::Reject {
                memory_id,
                config,
                actor,
            } => {
                let cfg = load_cfg(&config)?;
                let mem = paper_guard_app::MemoryService::from_config(&cfg)?;
                mem.reject_memory(&memory_id, &actor).await?;
                println!("rejected {memory_id} (actor={actor})");
            }
            MemoryCommand::Search { query, config } => {
                let cfg = load_cfg(&config)?;
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
        Command::Prompts { command } => match command {
            PromptsCommand::Init { config } => {
                let cfg = load_cfg(&config)?;
                let dir = cfg.prompts_dir()?;
                let (written, kept) = paper_guard_review::init_prompt_directory(&dir)?;
                println!("Initialized Paper Guard prompts in {}", dir.display());
                for role in paper_guard_review::PROMPT_ROLES {
                    println!("  {}", paper_guard_review::prompt_file_name(*role));
                }
                if written.is_empty() {
                    println!("Existing files were left unchanged.");
                } else {
                    println!(
                        "Wrote {} new file(s); existing files were left unchanged.",
                        written.len()
                    );
                }
                debug_assert_eq!(
                    kept.len() + written.len(),
                    paper_guard_review::PROMPT_ROLES.len()
                );
            }
            PromptsCommand::List { config } => {
                let cfg = load_cfg(&config)?;
                let dir = cfg.prompts_dir()?;
                let mut broken = false;
                println!("Prompts:");
                println!("  directory: {}", dir.display());
                for role in paper_guard_review::PROMPT_ROLES {
                    match paper_guard_review::resolve_prompt(&dir, *role) {
                        Ok(r) => {
                            let file = r
                                .path
                                .as_deref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default();
                            if file.is_empty() {
                                println!("  {}: {}", role.name(), r.source.label());
                            } else {
                                println!("  {}: {} ({})", role.name(), r.source.label(), file);
                            }
                        }
                        Err(e) => {
                            println!("  {}: external (unreadable: {e})", role.name());
                            broken = true;
                        }
                    }
                }
                if broken {
                    std::process::exit(1);
                }
            }
        },
        Command::Bibliography {
            source,
            config,
            output,
            clear_cache,
        } => {
            let cfg = load_cfg(&config)?;
            if clear_cache {
                let dir =
                    paper_guard_app::bibliography::bibliography_cache_dir(cfg.effective_data_dir());
                paper_guard_app::bibliography::clear_bibliography_cache(cfg.effective_data_dir())?;
                println!("cleared bibliography cache at {}", dir.display());
                return Ok(());
            }
            let Some(source) = source else {
                anyhow::bail!(
                    "`paper-guard bibliography` needs a paper source path (or use `--clear-cache`)"
                );
            };
            if !cfg.bibliography.effective_enabled() {
                println!("Bibliography verification is disabled.");
                println!("Enable it in your configuration (or `paper-guard.toml`):");
                println!("  [bibliography]");
                println!("  enabled = true");
                println!("The default provider is `arxiv`; `mock` is an offline test engine.");
                println!(
                    "Only bibliographic metadata is sent to scholarly sources — never manuscript text."
                );
                return Ok(());
            }
            let mode = match output.as_deref() {
                None | Some("human") => BibOutput::Human,
                Some("summary") => BibOutput::Summary,
                Some("json") => BibOutput::Json,
                Some(other) => anyhow::bail!(
                    "unsupported --output `{other}` for bibliography; expected human, summary, or json"
                ),
            };
            let (results, parsed) =
                paper_guard_app::bibliography::verify_source(&source, &cfg).await?;
            match mode {
                BibOutput::Human | BibOutput::Summary => {
                    println!(
                        "{}",
                        format_bibliography_report(
                            &source,
                            parsed,
                            &results,
                            mode == BibOutput::Summary
                        )
                    );
                }
                BibOutput::Json => {
                    println!("{}", serde_json::to_string_pretty(&results)?);
                }
            }
        }
        Command::Setup => {
            let report = paper_guard_app::run_setup()?;
            println!("Paper Guard Setup");
            for dir in report
                .created_dirs
                .iter()
                .chain(report.existing_dirs.iter())
            {
                println!("ok {}", dir.display());
            }
            if report.config_created {
                println!(
                    "ok {} (created with built-in defaults)",
                    report.config_path.display()
                );
            } else {
                println!(
                    "ok {} (already exists, left unchanged)",
                    report.config_path.display()
                );
            }
            let exported = report.prompts_written.len();
            if exported > 0 {
                println!(
                    "ok {exported} reviewer prompt file(s) exported to {}",
                    report.prompt_dir.display()
                );
            } else {
                println!(
                    "ok reviewer prompts already present in {} (left unchanged)",
                    report.prompt_dir.display()
                );
            }
            println!("Paper Guard is ready.");
        }
        Command::Config { command } => match command {
            ConfigCommand::Show { config } => {
                let cfg = load_cfg(&config)?;
                let toml = toml::to_string_pretty(&cfg)
                    .map_err(|e| anyhow::anyhow!("cannot serialize configuration: {e}"))?;
                println!("# Effective Paper Guard configuration");
                println!("# (values that look like secrets are redacted)");
                println!("{}", redact_secrets(&toml));
            }
            ConfigCommand::Edit { config } => {
                let path = match &config {
                    Some(p) => std::path::PathBuf::from(p),
                    None => paper_guard_app::paths::user_config_path().ok_or_else(|| {
                        anyhow::anyhow!(
                            "cannot resolve the platform home directory; no user configuration \
                             path available"
                        )
                    })?,
                };
                edit_config_file(&path)?;
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

    let cfg = AppConfig::load_user_preferred(config_path.map(PathBuf::from).as_deref())?;

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

// ---------------------------------------------------------------------------
// Inspect
// ---------------------------------------------------------------------------

/// `paper-guard inspect` — report how a source document resolves without
/// running a review. Never modifies the source; only reads it.
async fn run_inspect(cfg: &AppConfig, source: &str) -> anyhow::Result<()> {
    use paper_guard_parser::{format_from_extension, SourceFormat};

    let format = format_from_extension(source);
    println!(
        "Source: {}",
        match format {
            SourceFormat::Latex => "LaTeX".to_string(),
            SourceFormat::Pdf => "PDF".to_string(),
            SourceFormat::Typst => "Typst".to_string(),
            SourceFormat::Docx => "DOCX".to_string(),
            SourceFormat::SourceDir => "Source directory".to_string(),
        }
    );

    match format {
        SourceFormat::Pdf => {
            let bytes = std::fs::read(source)?;
            let doc = paper_guard_parser::parse_source_path(source).await?;
            // Count pages via the parsed document sections.
            let pages = doc.parsed.document.sections.len();
            let has_text = !doc.parsed.document.sections.is_empty();
            println!("Pages: {pages}");
            println!(
                "Extracted text: {}",
                if has_text { "available" } else { "unavailable" }
            );
            let _ = bytes;
        }
        SourceFormat::Latex => {
            let parsed = paper_guard_parser::parse_source_path(source).await?;
            let project_files = parsed.project_files;
            let missing = parsed.missing_includes;
            let cycles = parsed.include_cycles;

            if project_files.is_empty() {
                println!("Root: {source}");
                println!("File type: single-file manuscript");
                println!("Files resolved: 1");
            } else {
                let root_name = std::path::Path::new(source)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(source);
                println!("Root: {root_name}");
                println!("File type: LaTeX project");
                println!("Files resolved: {}", project_files.len() + 1);
                println!("Includes: {}", project_files.len());
                for f in &project_files {
                    println!("  include: {f}");
                }
            }
            println!("Missing: {}", missing.len());
            for m in &missing {
                println!("  {m}");
            }
            println!("Cycles: {}", cycles.len());
            for c in &cycles {
                println!("  {c}");
            }

            // If there are structural problems, they are surfaced without
            // failing the process (consistent with the review pipeline
            // behaviour).
            if !missing.is_empty() {
                println!();
                println!(
                    "Notice: {} missing include(s) — reviewers would see an INCOMPLETE manuscript.",
                    missing.len()
                );
            }
            if !cycles.is_empty() {
                println!(
                    "Notice: {} include cycle(s) — resolution stopped deterministically.",
                    cycles.len()
                );
            }
            let _ = cfg;
        }
        _ => {
            anyhow::bail!(
                "inspect is not yet supported for this source format; use `review` for LaTeX/PDF"
            );
        }
    }
    Ok(())
}

/// Load configuration honoring the documented resolution order:
///
/// ```text
/// CLI arguments            (applied by the caller on top of the result)
/// explicit --config        (highest file priority)
/// ~/.paper-guard/config/config.toml   (user config, when present)
/// built-in defaults
/// ```
///
/// A missing user configuration is never an error.
fn load_cfg(config: &Option<String>) -> anyhow::Result<AppConfig> {
    AppConfig::load_user_preferred(config.as_deref().map(PathBuf::from).as_deref())
}

/// Render an optional platform path as a string for diagnostics; never leaks a
/// missing directory, and never contains secret material.
fn platform_or_none(p: Option<PathBuf>) -> String {
    match p {
        Some(p) => p.to_string_lossy().into_owned(),
        None => "(unresolved)".into(),
    }
}

/// Print which prompt source each reviewer role resolves to.
///
/// Only source + directory are shown — never prompt contents. An external
/// file that exists but cannot be read is reported as unreadable rather than
/// silently presented as the embedded default.
fn print_prompt_status(cfg: &AppConfig) {
    let dir = match cfg.prompts_dir() {
        Ok(d) => d,
        Err(e) => {
            println!("Prompts:");
            println!("  directory: (unresolved: {e})");
            return;
        }
    };
    println!("Prompts:");
    println!("  directory: {}", dir.display());
    for role in paper_guard_review::PROMPT_ROLES {
        match paper_guard_review::resolve_prompt(&dir, *role) {
            Ok(r) => println!("  {}: {}", role.name(), r.source.label()),
            Err(e) => println!("  {}: external (unreadable: {e})", role.name()),
        }
    }
}

/// Marker prefixes of common credential formats. Paper Guard never *intends*
/// to store or print secrets (config holds environment-variable *names* only);
/// this scrubber is defense-in-depth for `config show` and file logs.
const SECRET_MARKERS: &[&str] = &[
    "sk-",
    "ghp_",
    "github_pat_",
    "AKIA",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "eyJ", // JWT header prefix
    "-----BEGIN",
    "Bearer ",
];

/// Replace anything that looks like a credential token with `[redacted]`.
pub(crate) fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    for marker in SECRET_MARKERS {
        while let Some(start) = out.find(marker) {
            let bytes = out.as_bytes();
            let mut end = start + marker.len();
            while end < bytes.len()
                && !matches!(
                    bytes[end],
                    b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b',' | b'}' | b'#' | b'.'
                )
            {
                end += 1;
            }
            out.replace_range(start..end, "[redacted]");
        }
    }
    out
}

/// Split an editor string (`$VISUAL`/`$EDITOR`, e.g. `code --wait`) into a
/// program plus arguments. The file path is appended by the caller.
pub(crate) fn split_editor_command(value: &str) -> Option<Vec<String>> {
    let mut parts = value.split_whitespace();
    let program = parts.next()?.to_string();
    let mut out = vec![program];
    out.extend(parts.map(|p| p.to_string()));
    Some(out)
}

/// Whether `name` is an executable on `$PATH` (no shell involved).
fn command_on_path(name: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if candidate
                        .metadata()
                        .map(|m| m.permissions().mode() & 0o111 != 0)
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = candidate;
                    return true;
                }
            }
        }
    }
    false
}

/// Resolve the platform editor. Order: `$VISUAL`, `$EDITOR`, platform
/// defaults. Returns the command (program + args, file path appended by the
/// caller) or `None` when no editor can be found.
pub(crate) fn resolve_editor() -> Option<Vec<String>> {
    for var in ["VISUAL", "EDITOR"] {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return split_editor_command(&value);
            }
        }
    }
    resolve_platform_editor()
}

/// Platform-default editor command (program + args; the file path is appended
/// by the caller).
#[cfg(target_os = "windows")]
fn resolve_platform_editor() -> Option<Vec<String>> {
    Some(vec!["notepad.exe".to_string()])
}

/// macOS: prefer a blocking terminal editor when present; otherwise `open -t`
/// opens the file in the default text editor without blocking.
#[cfg(target_os = "macos")]
fn resolve_platform_editor() -> Option<Vec<String>> {
    for candidate in ["nano", "vim", "vi"] {
        if command_on_path(candidate) {
            return Some(vec![candidate.to_string()]);
        }
    }
    Some(vec!["open".to_string(), "-t".to_string()])
}

/// Linux/BSD: sensible-editor first, then common terminal editors.
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn resolve_platform_editor() -> Option<Vec<String>> {
    for candidate in ["sensible-editor", "nano", "vim", "vi"] {
        if command_on_path(candidate) {
            return Some(vec![candidate.to_string()]);
        }
    }
    None
}

/// Open `path` in the resolved platform editor. When the file does not exist
/// it is created with built-in defaults first (never overwriting an existing
/// file). Returns a clear error when no editor is available.
fn edit_config_file(path: &std::path::Path) -> anyhow::Result<()> {
    let editor = resolve_editor().ok_or_else(|| {
        anyhow::anyhow!("no editor available: set $VISUAL or $EDITOR (e.g. `export EDITOR=nano`)")
    })?;

    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", parent.display()))?;
        }
        AppConfig::write_default_to(path).map_err(|e| {
            anyhow::anyhow!(
                "cannot create default configuration {}: {e}",
                path.display()
            )
        })?;
        println!("created {} with built-in defaults", path.display());
    }

    let mut command = editor.clone();
    command.push(path.to_string_lossy().into_owned());

    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .status()
        .map_err(|e| {
            anyhow::anyhow!(
                "cannot start editor `{}`: {e} (set $VISUAL or $EDITOR)",
                command[0]
            )
        })?;
    if !status.success() {
        anyhow::bail!("editor `{}` exited with {status}", command[0]);
    }
    println!("configuration saved at {}", path.display());
    Ok(())
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
        println!(
            "prompts_dir={}",
            platform_or_none(paths::default_prompts_dir())
        );
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

/// The output mode for the human-readable report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    /// The full human-readable report (default).
    Human,
    /// A concise terminal summary.
    Summary,
}

/// Resolve the `--output` flag. `human` is the default; `summary` prints the
/// existing concise one-liner. Any other value is rejected with a clear error.
fn resolve_output_mode(output: Option<&str>) -> anyhow::Result<OutputMode> {
    match output.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        None | Some("human") => Ok(OutputMode::Human),
        Some("summary") => Ok(OutputMode::Summary),
        Some(other) => {
            anyhow::bail!("invalid --output value `{other}`; expected `human` or `summary`")
        }
    }
}

/// Resolve the review presentation style with the documented priority:
/// CLI `--style` > `[review] style` config > `neutral` default.
/// Invalid values fail with a clear error; there is no implicit switching via
/// environment variables.
fn resolve_review_style(
    cli_style: Option<&str>,
    cfg: &AppConfig,
) -> anyhow::Result<paper_guard_report::ReviewStyle> {
    let given = cli_style
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let s = cfg.review.style.trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .unwrap_or_else(|| "neutral".to_string());
    paper_guard_report::parse_style_or_err(&given).map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Output selector for the standalone bibliography command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BibOutput {
    Human,
    Summary,
    Json,
}

/// Render the standalone bibliography verification report.
///
/// The output is a *presentation* of the canonical results. Markers and
/// status labels are fixed and style-independent; no style (neutral/funny/
/// insulting) ever alters these scientific data, and no output attacks
/// authors personally.
fn format_bibliography_report(
    source: &str,
    parsed: usize,
    results: &[paper_guard_core::BibliographyResult],
    summary: bool,
) -> String {
    let mut out = String::new();
    out.push_str("Bibliography Verification\n");
    out.push_str("=========================\n\n");
    out.push_str(&format!("Paper: {source}\n"));
    out.push_str(&format!("References parsed: {parsed}\n"));
    if results.is_empty() {
        out.push_str("\nNo references to verify.\n");
        return out;
    }

    let sources: std::collections::BTreeSet<&str> =
        results.iter().map(|r| r.source.as_str()).collect();
    out.push_str(&format!(
        "Sources: {}\n\n",
        sources.into_iter().collect::<Vec<_>>().join(", ")
    ));

    if !summary {
        let mut scholar_rows = Vec::new();
        for result in results {
            if result.source == "google_scholar" {
                scholar_rows.push(&result.reference_id);
                continue;
            }
            render_bibliography_result(&mut out, result);
        }
        if !scholar_rows.is_empty() {
            out.push_str(&format!(
                "Google Scholar:\n    {} ({} reference(s) — Scholar is not automated by \
                 Paper Guard; see documentation)\n",
                paper_guard_core::VerificationStatus::Unavailable.label(),
                scholar_rows.len()
            ));
        }
    } else {
        // Summary: status counts only (still canonical data, no styling).
        let mut counts: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        for result in results {
            *counts.entry(result.status.label()).or_insert(0) += 1;
        }
        for (label, count) in counts {
            out.push_str(&format!("  {label}: {count}\n"));
        }
    }
    out
}

fn render_bibliography_result(out: &mut String, result: &paper_guard_core::BibliographyResult) {
    out.push_str(&format!(
        "{} {}\n",
        result.status.glyph(),
        result.reference_id
    ));
    let citation = result.display_citation();
    if citation != result.reference_id {
        out.push_str(&format!("    {citation}\n"));
    }
    out.push_str(&format!("    Source: {}\n", result.source));
    out.push_str(&format!(
        "    {} (confidence {:.2})\n",
        result.status.label(),
        result.confidence
    ));
    if let Some(note) = &result.note {
        out.push_str(&format!("    {note}\n"));
    }
    for m in &result.mismatches {
        out.push_str(&format!(
            "    Mismatch — {}: paper says {:?}, source says {:?}\n",
            m.field, m.paper_value, m.source_value
        ));
    }
    if result.from_cache {
        out.push_str("    (served from local cache)\n");
    }
    out.push('\n');
}

/// Print the run output to the terminal. `human` renders the full
/// human-readable review report; `summary` prints the concise one-liner. The
/// canonical JSON artifacts are always persisted regardless of the chosen
/// presentation mode.
fn print_run_output(
    out: &run::RunOutput,
    source: &str,
    cfg: &AppConfig,
    style: paper_guard_report::ReviewStyle,
    output_mode: OutputMode,
) {
    match output_mode {
        OutputMode::Summary => print_summary(out),
        OutputMode::Human => {
            let provider = match cfg.llm.provider.as_str() {
                "openai-compatible" => "OpenAI-compatible".to_string(),
                other => other.to_string(),
            };
            let model = match cfg.llm.provider.as_str() {
                "openai-compatible" => cfg.providers.openai_compatible.model.clone(),
                _ => String::new(),
            };
            let header = paper_guard_report::ReportHeader {
                paper: source.to_string(),
                run: out.run.run_id.clone(),
                mode: "local".to_string(),
                provider,
                model,
            };
            let report = paper_guard_report::build_human_report(&out.run, &header, style);
            print!("{report}");
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a config with a given `[review] style` (or the default).
    fn cfg_with_style(style: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.review.style = style.to_string();
        cfg
    }

    #[test]
    fn style_defaults_to_neutral_when_no_cli_and_no_config() {
        // No CLI flag + default config => neutral.
        let cfg = AppConfig::default();
        assert_eq!(
            resolve_review_style(None, &cfg).unwrap(),
            paper_guard_report::ReviewStyle::Neutral
        );
    }

    #[test]
    fn cli_style_flag_is_honored() {
        let cfg = cfg_with_style("funny");
        // CLI overrides config.
        assert_eq!(
            resolve_review_style(Some("neutral"), &cfg).unwrap(),
            paper_guard_report::ReviewStyle::Neutral
        );
        assert_eq!(
            resolve_review_style(Some("funny"), &cfg).unwrap(),
            paper_guard_report::ReviewStyle::Funny
        );
        assert_eq!(
            resolve_review_style(Some("insulting"), &cfg).unwrap(),
            paper_guard_report::ReviewStyle::Insulting
        );
    }

    #[test]
    fn config_style_is_used_when_no_cli_flag() {
        // No CLI flag => config wins over default.
        assert_eq!(
            resolve_review_style(None, &cfg_with_style("funny")).unwrap(),
            paper_guard_report::ReviewStyle::Funny
        );
        assert_eq!(
            resolve_review_style(None, &cfg_with_style("insulting")).unwrap(),
            paper_guard_report::ReviewStyle::Insulting
        );
    }

    #[test]
    fn cli_style_overrides_config() {
        // Config = "funny", CLI = "neutral" => neutral wins (CLI > config).
        let cfg = cfg_with_style("funny");
        let got = resolve_review_style(Some("neutral"), &cfg).unwrap();
        assert_eq!(got, paper_guard_report::ReviewStyle::Neutral);
    }

    #[test]
    fn invalid_cli_style_is_rejected() {
        let cfg = AppConfig::default();
        assert!(resolve_review_style(Some("something-weird"), &cfg).is_err());
    }

    #[test]
    fn invalid_config_style_is_rejected() {
        let cfg = cfg_with_style("bogus");
        assert!(resolve_review_style(None, &cfg).is_err());
    }

    #[test]
    fn output_mode_resolves_human_default_and_summary() {
        assert_eq!(resolve_output_mode(None).unwrap(), OutputMode::Human);
        assert_eq!(
            resolve_output_mode(Some("human")).unwrap(),
            OutputMode::Human
        );
        assert_eq!(
            resolve_output_mode(Some("summary")).unwrap(),
            OutputMode::Summary
        );
        assert!(resolve_output_mode(Some("bogus")).is_err());
    }

    #[test]
    fn redact_secrets_masks_credential_tokens_anywhere() {
        let cases = [
            ("no secrets here", "no secrets here"),
            (
                "api_key_env = \"sk-SUPERSECRET123456\"",
                "api_key_env = \"[redacted]\"",
            ),
            (
                "token=ghp_abcdefghijklmnopqrstuvwxyz123456",
                "token=[redacted]",
            ),
            ("Bearer abcDEF123xyz", "[redacted]"),
            ("AKIAIOSFODNN7EXAMPLE", "[redacted]"),
            ("eyJhbGciOiJIUzI1NiJ9.payload", "[redacted].payload"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                redact_secrets(input),
                expected,
                "redaction mismatch for {input:?}"
            );
        }
    }

    #[test]
    fn editor_command_splits_program_and_args() {
        assert_eq!(
            split_editor_command("nano").unwrap(),
            vec!["nano".to_string()]
        );
        assert_eq!(
            split_editor_command("code --wait").unwrap(),
            vec!["code".to_string(), "--wait".to_string()]
        );
        assert!(split_editor_command("   ").is_none());
        assert!(split_editor_command("").is_none());
    }
}

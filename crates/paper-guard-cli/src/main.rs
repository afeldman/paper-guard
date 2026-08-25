//! Paper Guard — command-line interface.
//!
//! A reproducible, multi-agent scientific review and revision workflow.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use paper_guard_cli::{config, logging, run};
use config::AppConfig;
use logging::init_logging;

#[derive(Parser)]
#[command(name = "paper-guard", about = "Reproducible multi-agent scientific review workflow", version)]
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
        #[arg(long)]
        approve_all: bool,
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
            approve_all,
        } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let data_dir = cfg.reproducibility.data_dir.clone();
            let fixture = fixture_response_for(&source);
            let out = run::run_pipeline(&source, &cfg, &data_dir, fixture.as_deref(), approve_all)
                .await?;
            print_summary(&out);
        }
        Command::Run {
            source,
            config,
            approve_all,
        } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let data_dir = cfg.reproducibility.data_dir.clone();
            let fixture = fixture_response_for(&source);
            let out = run::run_pipeline(&source, &cfg, &data_dir, fixture.as_deref(), approve_all)
                .await?;
            print_summary(&out);
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
            println!("run {run}: {} revisions recorded", record.revision_results.len());
        }
        Command::Validate { run, config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            let ledger = paper_guard_ledger::LedgerStore::open(&cfg.reproducibility.data_dir)?;
            let record = ledger.load_run(&run)?;
            let validations = &record.validation_results;
            let passed = validations.iter().all(|v| v.passed);
            println!("run {run}: validation {} ({} checks)", if passed { "PASSED" } else { "FAILED" }, validations.len());
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
                    println!("{} | {:?} | source={} | findings={} | revisions={}",
                        id, r.status, r.source_format,
                        r.findings.len(), r.revision_results.len());
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
            let open = record.findings.iter().filter(|f| f.status.describe() == "OPEN").count();
            println!("- open findings: {open}");
            println!("- revisions applied: {}", record.revision_results.len());
            for f in &record.findings {
                if f.status.describe() == "OPEN" {
                    println!("  - [{} {}] {} @ {}", f.severity.priority(), f.finding_id, f.finding, f.location);
                }
            }
        }
        Command::Serve { config, bind } => {
            let cfg = AppConfig::load(Some(Path::new(&config)))?;
            let data_dir = cfg.service.data_dir.clone();
            let addr = bind.unwrap_or_else(|| cfg.service.bind.clone());
            let enforce_loopback = !cfg.service.allow_external_bind;
            let state = paper_guard_service::AppState {
                config: std::sync::Arc::new(cfg),
                data_dir,
                enforce_loopback,
            };
            println!("paper-guard serve listening on {addr}");
            paper_guard_service::serve(&addr, state).await?;
        }
        Command::Health { config } => {
            let cfg = AppConfig::load(config.as_deref().map(PathBuf::from).as_deref())?;
            // Health is answered by the running service; if we are here from the
            // CLI without a server, print the configured endpoint for tooling.
            println!(
                "service configured at http://{}/health (provider={}, memory={})",
                cfg.service.bind,
                cfg.llm.provider,
                cfg.memory.backend
            );
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

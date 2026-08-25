//! Paper Guard — command-line interface.
//!
//! A reproducible, multi-agent scientific review and revision workflow.

mod config;
mod logging;
mod run;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
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
    }
    Ok(())
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

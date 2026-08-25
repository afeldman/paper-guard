//! Structured logging via rust_loguru.
//!
//! Logs carry `run_id`, `agent`, `stage`, and other structured fields where
//! relevant. Parallel agent runs remain attributable because each log line is
//! json-structured. Secrets are never logged.

use rust_loguru::formatters::Formatter;
use rust_loguru::handler::console::ConsoleHandler;
use rust_loguru::{error, info, init, warn, LogLevel, Logger};

/// Initialize the global structured logger.
pub fn init_logging() {
    // Configure a JSON console logger at INFO level.
    use parking_lot::RwLock;
    let handler = ConsoleHandler::stdout(LogLevel::Info).with_formatter(Formatter::json());
    let mut logger = Logger::new(LogLevel::Info);
    logger.add_handler(std::sync::Arc::new(RwLock::new(handler)));
    let _ = init(logger);
}

/// Structured helpers that prefix a run id / agent.
pub fn log_review_start(run_id: &str, agent: &str, stage: &str) {
    info!(
        "{} | stage={} | agent={} | event=start",
        run_id, stage, agent
    );
}

pub fn log_review_end(run_id: &str, agent: &str, stage: &str, status: &str, findings: usize) {
    info!(
        "{} | stage={} | agent={} | event=end | status={} | findings={}",
        run_id, stage, agent, status, findings
    );
}

/// Structured error logging (available for pipeline error reporting).
#[allow(dead_code)]
pub fn log_error(run_id: &str, agent: &str, stage: &str, err: &dyn std::fmt::Display) {
    error!(
        "{} | stage={} | agent={} | event=error | error={}",
        run_id, stage, agent, err
    );
}

pub fn log_agent_failure(run_id: &str, agent: &str, err: &str) {
    warn!(
        "{} | agent={} | event=failed_agent | error={}",
        run_id, agent, err
    );
}

/// Log the selected real provider (never the API key itself, only the provider
/// kind, model, and capability flags).
pub fn log_provider_selected(model: &str, structured_output: bool, vision: bool) {
    info!(
        "{} | stage={} | agent={} | event=provider_selected | model={} | structured_output={} | vision={}",
        "pipeline", "review", "pipeline", model, structured_output, vision
    );
}

/// Log a successful memory retrieval (count only; never the memory contents).
pub fn log_memory_retrieval(count: usize) {
    info!("pipeline | stage=review | agent=pipeline | event=memory_retrieved | count={count}");
}

/// Log that memory was requested but is unavailable/failed (so a READ_ONLY run
/// continues without fabricated context).
pub fn log_memory_unavailable() {
    warn!("pipeline | stage=review | agent=pipeline | event=memory_unavailable");
}

//! Structured logging via rust_loguru.
//!
//! Logs carry `run_id`, `agent`, `stage`, and other structured fields where
//! relevant. Parallel agent runs remain attributable because each log line is
//! json-structured. Secrets are never logged — and a defense-in-depth scrubber
//! masks credential-looking tokens before anything reaches the log file.
//!
//! Technical logs are mirrored to a **rolling file** inside the canonical
//! per-user directory (`~/.paper-guard/logs/paper-guard.log`), size-rotated at
//! [`LOG_MAX_FILE_BYTES`] per file and keeping [`LOG_MAX_FILES`] rotated files
//! (oldest are deleted automatically). The console output remains unchanged,
//! so the human-readable CLI output is not affected.

use std::path::PathBuf;
use std::sync::Arc;

use rust_loguru::formatters::Formatter;
use rust_loguru::handler::console::ConsoleHandler;
use rust_loguru::handler::file::FileHandler;
use rust_loguru::{error, info, init, warn, LogLevel, Logger};

use crate::paths;

/// Maximum size of one log file before it is rotated (10 MiB).
pub const LOG_MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
/// Number of rotated log files kept (`.1` … `.N`); older files are deleted.
pub const LOG_MAX_FILES: usize = 5;
/// The base log file name inside the per-user logs directory.
pub const LOG_BASE_NAME: &str = "paper-guard.log";

/// Marker prefixes of common credential formats. Paper Guard never *intends*
/// to log secrets; this scrubber is defense-in-depth so that even a buggy log
/// call cannot write a credential-looking token to the log file.
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
pub(crate) fn scrub_secrets(text: &str) -> String {
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

/// The resolved rolling log file path: `~/.paper-guard/logs/paper-guard.log`.
/// Falls back to the platform log directory when the home directory is not
/// resolvable; returns `None` only when neither location is available.
pub fn log_file_path() -> Option<PathBuf> {
    paths::user_logs_dir()
        .or_else(paths::log_dir)
        .map(|dir| dir.join(LOG_BASE_NAME))
}

/// Initialize the global structured logger.
///
/// Console logging (JSON at INFO level, as before) plus a rolling file
/// handler mirroring the same records to `~/.paper-guard/logs/paper-guard.log`
/// (10 MiB × 5, oldest files deleted automatically). If the log directory
/// cannot be created or opened, file logging is skipped without breaking the
/// console logger (a missing log file must never crash the tool).
pub fn init_logging() {
    use parking_lot::RwLock;

    let mut logger = Logger::new(LogLevel::Info);

    let console = ConsoleHandler::stdout(LogLevel::Info).with_formatter(Formatter::json());
    logger.add_handler(Arc::new(RwLock::new(console)));

    if let Some(path) = log_file_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let scrubbed_json = Formatter::json().with_format(|record| {
            let rendered = Formatter::json().format(record);
            format!("{}\n", scrub_secrets(&rendered))
        });
        if let Ok(handler) = FileHandler::new(&path).map(|h| {
            h.with_level(LogLevel::Info)
                .with_rotation(LOG_MAX_FILE_BYTES, LOG_MAX_FILES)
                .with_formatter(scrubbed_json)
                .with_colors(false)
        }) {
            logger.add_handler(Arc::new(RwLock::new(handler)));
        }
    }

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
pub fn log_provider_selected(model: &str, structured_output: &str, vision: bool) {
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

/// Log LaTeX project missing-include diagnostics (structural, never content).
pub fn log_project_missing_includes(run_id: &str, missing: &[String]) {
    info!(
        "{} | stage=parse | agent=pipeline | event=latex_missing_includes | count={} | missing={:?}",
        run_id,
        missing.len(),
        missing
    );
}

/// Log LaTeX project include-cycle diagnostics (structural, never content).
pub fn log_project_cycles(run_id: &str, cycles: &[String]) {
    warn!(
        "{} | stage=parse | agent=pipeline | event=latex_include_cycle | count={} | cycles={:?}",
        run_id,
        cycles.len(),
        cycles
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_loguru::handler::Handler;
    use rust_loguru::record::Record;

    #[test]
    fn log_path_points_into_user_logs_dir_when_home_resolvable() {
        if let Some(path) = log_file_path() {
            let s = path.to_string_lossy();
            assert!(!s.contains('~'), "resolved path must not contain ~: {s}");
            assert!(
                s.ends_with("paper-guard.log"),
                "log file must end in paper-guard.log: {s}"
            );
        }
    }

    #[test]
    fn scrub_secrets_masks_credential_tokens() {
        assert_eq!(scrub_secrets("no secrets here"), "no secrets here");
        assert_eq!(
            scrub_secrets("key=sk-SUPERSECRET123456 end"),
            "key=[redacted] end"
        );
        assert!(!scrub_secrets("ghp_abcdefghijklmnopqrstuvwxyz123456").contains("ghp_"));
        assert!(!scrub_secrets("Bearer abcDEF123xyz").contains("Bearer "));
        assert!(!scrub_secrets("AKIAIOSFODNN7EXAMPLE").contains("AKIA"));
        assert!(!scrub_secrets("eyJhbGciOiJIUzI1NiJ9.payload").contains("eyJ"));
    }

    /// The rotation contract of the file handler we configure: bounded file
    /// count, oldest file removed, bounded disk usage. Uses tiny limits.
    #[test]
    fn file_handler_rotation_removes_oldest_and_stays_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");
        let handler = FileHandler::new(&path)
            .unwrap()
            .with_rotation(256, 3)
            .with_colors(false);

        // ~1 KiB of records → several rotations.
        let big = "A".repeat(200);
        let record = Record::new(LogLevel::Info, big.as_str(), Some("m".into()), None, None);
        for _ in 0..40 {
            handler.handle(&record).unwrap();
        }

        // Current file + at most 3 rotated files exist.
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.retain(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("test.log")
        });
        assert!(files.len() <= 4, "too many log files: {files:?}");
        assert!(
            files.iter().all(|p| p.exists()),
            "all listed files must exist"
        );
        // A `.4` would be the oldest rotated file; with max_files=3 it must
        // never exist (only `.1`, `.2`, `.3` are kept).
        assert!(!dir.path().join("test.log.4").exists());
        let total: u64 = files
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().len())
            .sum();
        // 4 files, each <= ~256 bytes + one oversized record allowance.
        assert!(
            total < 2_500,
            "total log usage must stay bounded, got {total} bytes"
        );
        handler.flush().unwrap();
    }
}

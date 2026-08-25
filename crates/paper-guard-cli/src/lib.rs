//! The CLI binary crate.
//!
//! The command-line interface is a thin shell: command parsing and user-facing
//! summary output live here, while the actual pipeline orchestration and
//! configuration live in the shared [`paper_guard_app`] application layer. The
//! HTTP service (`paper-guard serve`) uses the *same* application layer, so
//! the CLI and the service can never diverge in review behaviour.

pub use paper_guard_app as app;
pub use paper_guard_app::config;
pub use paper_guard_app::logging;

/// Re-export the pipeline module under `run` so `run::RunOutput` and
/// `run::run_pipeline` resolve as before, but from the shared layer.
pub use paper_guard_app::pipeline as run;

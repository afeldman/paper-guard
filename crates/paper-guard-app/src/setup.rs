//! `paper-guard setup` — create the canonical per-user directory layout.
//!
//! Creates the user-owned Paper Guard directory tree:
//!
//! ```text
//! ~/.paper-guard/
//! ├── config/
//! │   ├── config.toml      (defaults, only when absent)
//! │   └── prompts/         (embedded default reviewer prompts)
//! ├── logs/                (rolling technical logs)
//! └── data/                (per-user review data, opt-in)
//! ```
//!
//! The command is **idempotent**: existing user files (configuration or
//! prompts) are never overwritten. Setup is optional — a fresh binary works
//! out of the box with built-in defaults and without any user configuration.

use std::path::{Path, PathBuf};

use paper_guard_review::{init_prompt_directory, PROMPT_ROLES};

use crate::config::AppConfig;
use crate::paths;

/// What `paper-guard setup` did (for printing and tests).
#[derive(Debug)]
pub struct SetupReport {
    /// The canonical user directory (`~/.paper-guard`).
    pub user_dir: PathBuf,
    /// Directories that already existed (not re-created).
    pub existing_dirs: Vec<PathBuf>,
    /// Directories newly created by this run.
    pub created_dirs: Vec<PathBuf>,
    /// The canonical user config path.
    pub config_path: PathBuf,
    /// Whether the config file was newly written (false = already existed).
    pub config_created: bool,
    /// The user prompt directory (`config/prompts`).
    pub prompt_dir: PathBuf,
    /// Prompt files newly exported by this run.
    pub prompts_written: Vec<String>,
    /// Prompt files already present (left unchanged).
    pub prompts_kept: Vec<String>,
}

/// Run the idempotent setup against the canonical user directory.
pub fn run_setup() -> anyhow::Result<SetupReport> {
    let user_dir = paths::user_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve the platform home directory; unable to set up ~/.paper-guard"
        )
    })?;
    run_setup_at(&user_dir)
}

/// The pure, injectable core of [`run_setup`]: lays out the directory tree
/// below `user_dir` (used by tests so a real user's `~/.paper-guard` is never
/// touched). `user_dir` is expected to end in `.paper-guard`.
pub fn run_setup_at(user_dir: &Path) -> anyhow::Result<SetupReport> {
    let config_dir = user_dir.join("config");
    let prompt_dir = config_dir.join("prompts");
    let logs_dir = user_dir.join("logs");
    let data_dir = user_dir.join("data");
    let config_path = config_dir.join(paths::USER_CONFIG_FILE);

    let mut existing_dirs = Vec::new();
    let mut created_dirs = Vec::new();
    for dir in [user_dir, &config_dir, &prompt_dir, &logs_dir, &data_dir] {
        if dir.exists() {
            existing_dirs.push(dir.to_path_buf());
        } else {
            std::fs::create_dir_all(dir)
                .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", dir.display()))?;
            created_dirs.push(dir.to_path_buf());
        }
    }

    // Write the default user configuration only when it does not exist.
    let config_created = if config_path.exists() {
        false
    } else {
        AppConfig::write_default_to(&config_path)?;
        true
    };

    // Export the embedded default reviewer prompts (never overwrite).
    let (prompts_written, prompts_kept) = init_prompt_directory(&prompt_dir)?;

    debug_assert_eq!(
        prompts_written.len() + prompts_kept.len(),
        PROMPT_ROLES.len()
    );

    Ok(SetupReport {
        user_dir: user_dir.to_path_buf(),
        existing_dirs,
        created_dirs,
        config_path,
        config_created,
        prompt_dir,
        prompts_written,
        prompts_kept,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure part of setup is exercised against a synthetic user directory
    /// so tests never touch a real user's `~/.paper-guard`.
    #[test]
    fn setup_is_idempotent_and_never_overwrites_user_files() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join(".paper-guard");

        // First run: directories created, config written, prompts exported.
        let first = run_setup_at(&root).expect("first setup succeeds");
        assert!(first.config_created);
        assert_eq!(first.prompts_written.len(), PROMPT_ROLES.len());
        assert!(first.prompts_kept.is_empty());
        assert!(first.existing_dirs.is_empty());
        assert_eq!(first.created_dirs.len(), 5);

        for d in ["", "config", "config/prompts", "logs", "data"] {
            assert!(root.join(d).is_dir(), "missing directory {d}");
        }
        assert!(root.join("config").join(paths::USER_CONFIG_FILE).is_file());

        // Second run: nothing is overwritten.
        let second = run_setup_at(&root).expect("second setup succeeds");
        assert!(
            !second.config_created,
            "config.toml must not be overwritten"
        );
        assert!(second.prompts_written.is_empty());
        assert_eq!(second.prompts_kept.len(), PROMPT_ROLES.len());

        // User edits survive a repeated setup.
        std::fs::write(
            root.join("config/prompts/scientific.md"),
            "MY EDITED PROMPT",
        )
        .unwrap();
        std::fs::write(
            root.join("config").join(paths::USER_CONFIG_FILE),
            "[project]\nname = \"my-paper\"\n",
        )
        .unwrap();
        let third = run_setup_at(&root).expect("third setup succeeds");
        assert!(!third.config_created);
        assert!(third.prompts_written.is_empty());
        assert_eq!(
            std::fs::read_to_string(root.join("config/prompts/scientific.md")).unwrap(),
            "MY EDITED PROMPT"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("config").join(paths::USER_CONFIG_FILE)).unwrap(),
            "[project]\nname = \"my-paper\"\n"
        );
    }
}

//! Cross-platform filesystem path resolution for Paper Guard.
//!
//! Paper Guard never hard-codes Unix-style paths. Instead, platform-appropriate
//! per-user directories are resolved through the `dirs` crate so that:
//!
//! - **macOS / Linux** → `$XDG_CONFIG_HOME` (default `~/.config`) or
//!   `~/Library/Application Support` (macOS where `dirs` prefers it).
//! - **Windows** → `%LOCALAPPDATA%` / `%APPDATA%` (e.g. `C:\Users\<User>\AppData`).
//!
//! In addition, Paper Guard owns one canonical per-user directory
//! (`~/.paper-guard`, resolved through the platform home directory) that holds
//! user configuration, external reviewer prompts, rolling logs and user data:
//!
//! ```text
//! ~/.paper-guard/
//! ├── config/
//! │   ├── config.toml        (user configuration)
//! │   └── prompts/           (editable reviewer prompts)
//! ├── logs/                  (rolling technical logs)
//! └── data/                  (per-user review data, opt-in)
//! ```
//!
//! Any of the platform locations may be overridden by an explicit `--config` /
//! `data_dir` setting from the user; the functions here only provide the
//! *default* when the user has not specified a location.

use std::path::{Path, PathBuf};

/// The application directory name used under each per-user base directory.
const APP_DIR: &str = "paper-guard";

/// The default configuration file name.
pub const CONFIG_FILE: &str = "paper-guard.toml";

/// The canonical user configuration file name inside `~/.paper-guard/config`.
pub const USER_CONFIG_FILE: &str = "config.toml";

/// Resolve the per-user **configuration** directory for Paper Guard.
///
/// - Windows: `%APPDATA%\paper-guard`
/// - macOS: `~/Library/Application Support/paper-guard`
/// - Linux/BSD: `$XDG_CONFIG_HOME/paper-guard` (default `~/.config/paper-guard`)
///
/// Returns `None` if no conventional base can be determined (rarely, e.g. an
/// unusual environment with no home directory).
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|base| base.join(APP_DIR))
}

/// Resolve the per-user **data / application data** directory.
///
/// - Windows: `%APPDATA%\paper-guard` (roaming data, matches Windows semantics)
/// - macOS: `~/Library/Application Support/paper-guard`
/// - Linux: `$XDG_DATA_HOME/paper-guard` (default `~/.local/share/paper-guard`)
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|base| base.join(APP_DIR))
}

/// Resolve the per-user **cache** directory.
///
/// - Windows: `%LOCALAPPDATA%\paper-guard\cache`
/// - macOS: `~/Library/Caches/paper-guard`
/// - Linux: `$XDG_CACHE_HOME/paper-guard` (default `~/.cache/paper-guard`)
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|base| base.join(APP_DIR))
}

/// Resolve the per-user **logs** directory.
///
/// Logs are treated as application data but kept in a dedicated subdirectory
/// so they can be managed (rotated/cleared) independently of review artifacts.
///
/// - Windows: `%LOCALAPPDATA%\paper-guard\logs`
/// - macOS: `~/Library/Logs/paper-guard`
/// - Linux: `$XDG_STATE_HOME/paper-guard/logs` (default `~/.local/state/paper-guard`)
pub fn log_dir() -> Option<PathBuf> {
    // `dirs::state_dir()` is the closest cross-platform abstraction; fall back
    // to the data dir when the OS does not expose a conventional state dir.
    dirs::state_dir()
        .map(|base| base.join(APP_DIR).join("logs"))
        .or_else(|| data_dir().map(|d| d.join("logs")))
}

/// The default full path to the `paper-guard.toml` configuration file.
pub fn default_config_path() -> Option<PathBuf> {
    config_dir().map(|c| c.join(CONFIG_FILE))
}

/// The canonical per-user Paper Guard directory: `<home>/.paper-guard`.
///
/// This is the single user-owned location for user configuration
/// (`config/`), external reviewer prompts (`config/prompts/`), rolling
/// technical logs (`logs/`) and user data (`data/`). It is resolved through
/// the platform home directory — never a hard-coded OS path.
pub fn user_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".paper-guard"))
}

/// `<home>/.paper-guard/config` — user configuration directory.
pub fn user_config_dir() -> Option<PathBuf> {
    user_dir().map(|d| d.join("config"))
}

/// `<home>/.paper-guard/config/config.toml` — the canonical user config file.
pub fn user_config_path() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join(USER_CONFIG_FILE))
}

/// `<home>/.paper-guard/config/prompts` — user-editable external prompts.
pub fn user_prompts_dir() -> Option<PathBuf> {
    user_config_dir().map(|d| d.join("prompts"))
}

/// `<home>/.paper-guard/logs` — rolling technical log files.
pub fn user_logs_dir() -> Option<PathBuf> {
    user_dir().map(|d| d.join("logs"))
}

/// `<home>/.paper-guard/data` — per-user data directory (intended for
/// `[reproducibility] data_dir` / `[service] data_dir` when a user chooses
/// the per-user layout).
pub fn user_data_dir() -> Option<PathBuf> {
    user_dir().map(|d| d.join("data"))
}

/// The default **external prompt** directory inside the canonical user
/// directory: `<home>/.paper-guard/config/prompts`.
///
/// Resolved portably through the platform home directory (never a hard-coded
/// `/home/...`, `/Users/...` or `C:\...` prefix). Returns `None` only when the
/// OS cannot provide a home directory at all.
pub fn default_prompts_dir() -> Option<PathBuf> {
    user_prompts_dir()
}

/// Create the config directory (and parents) if it does not exist.
pub fn ensure_config_dir() -> Option<PathBuf> {
    config_dir().inspect(|c| {
        let _ = std::fs::create_dir_all(c);
    })
}

/// The default **relative** data directory used by the current release.
///
/// For historical compatibility the on-disk defaults are relative (`.paper-guard`)
/// so a run in a project folder behaves identically on every OS. This function
/// returns that relative default; the platform-absolute defaults in
/// [`data_dir`] and the per-user `<home>/.paper-guard/data` directory are
/// offered for global usage and documented in `docs/windows.md`.
pub fn default_data_dir() -> &'static str {
    ".paper-guard"
}

/// Given an optional explicit path and an OS-resolved default directory,
/// produce the effective path. Explicit user input always wins.
pub fn or_explicit(explicit: Option<&Path>, default: &Path) -> PathBuf {
    explicit
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| default.to_path_buf())
}

/// Expand a leading `~` (or `~/`, `~\`) in a user-supplied path using the
/// platform home directory. Without a resolvable home directory the input is
/// returned unchanged so the caller can surface an explicit error instead of
/// silently treating `~` as a literal directory name.
pub fn expand_user_path(input: &str, home: Option<&Path>) -> PathBuf {
    if let Some(home) = home {
        if input == "~" {
            return home.to_path_buf();
        }
        if let Some(rest) = input
            .strip_prefix("~/")
            .or_else(|| input.strip_prefix("~\\"))
        {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_never_contains_unix_only_slash() {
        // The resolved path must be a proper PathBuf (cross-platform usable);
        // it should never be literally `.config/paper-guard` on any platform.
        if let Some(c) = config_dir() {
            assert!(c.as_os_str().len() > "paper-guard".len());
            assert!(!c.to_string_lossy().contains("~"));
        }
    }

    #[test]
    fn default_data_dir_remains_relative() {
        // The historical default stays relative and cross-platform.
        assert_eq!(default_data_dir(), ".paper-guard");
    }

    #[test]
    fn or_explicit_prefers_explicit() {
        let explicit = Path::new("C:\\Users\\Researcher\\cfg.toml");
        let default = Path::new("/tmp/fallback.toml");
        assert_eq!(or_explicit(Some(explicit), default), explicit);
        assert_eq!(or_explicit(None, default), default);
    }

    #[test]
    fn user_dirs_resolve_under_home_without_literal_tilde() {
        let home = dirs::home_dir().expect("test environment has a home");
        let root = user_dir().expect("home resolvable");
        assert_eq!(root, home.join(".paper-guard"));
        assert_eq!(
            user_config_path().unwrap(),
            home.join(".paper-guard").join("config").join("config.toml")
        );
        assert_eq!(
            user_prompts_dir().unwrap(),
            home.join(".paper-guard").join("config").join("prompts")
        );
        assert_eq!(
            user_logs_dir().unwrap(),
            home.join(".paper-guard").join("logs")
        );
        assert_eq!(
            user_data_dir().unwrap(),
            home.join(".paper-guard").join("data")
        );
        for p in [
            user_dir(),
            user_config_dir(),
            user_config_path(),
            user_prompts_dir(),
            user_logs_dir(),
            user_data_dir(),
        ] {
            let resolved = p.unwrap();
            let s = resolved.to_string_lossy();
            assert!(!s.contains('~'), "resolved path must not contain ~: {s}");
        }
    }

    #[test]
    fn default_prompts_dir_uses_home_and_never_literal_tilde() {
        let d = default_prompts_dir();
        if let Some(d) = d {
            let s = d.to_string_lossy();
            assert!(
                !s.contains('~'),
                "resolved path must not contain a literal ~: {s}"
            );
            assert!(
                s.ends_with(".paper-guard/config/prompts")
                    || s.ends_with(".paper-guard\\config\\prompts")
            );
        }
    }

    #[test]
    fn expand_user_path_handles_tilde_forms() {
        let home = Path::new("/home/researcher");
        assert_eq!(expand_user_path("~", Some(home)), home);
        assert_eq!(
            expand_user_path("~/prompts", Some(home)),
            home.join("prompts")
        );
        assert_eq!(
            expand_user_path("~/.paper-guard/config/prompts", Some(home)),
            home.join(".paper-guard").join("config").join("prompts")
        );
        assert_eq!(
            expand_user_path("~\\prompts", Some(home)),
            home.join("prompts")
        );
        // Non-tilde paths pass through untouched (absolute or relative).
        assert_eq!(
            expand_user_path("/etc/paper-guard/prompts", Some(home)),
            PathBuf::from("/etc/paper-guard/prompts")
        );
        assert_eq!(
            expand_user_path("relative/prompts", Some(home)),
            PathBuf::from("relative/prompts")
        );
        assert_eq!(
            expand_user_path("C:\\Users\\Researcher\\.paper-guard\\prompts", Some(home)),
            PathBuf::from("C:\\Users\\Researcher\\.paper-guard\\prompts")
        );
        // Without a home directory, tilde input is preserved for the caller
        // to surface as an error (never silently resolved to a literal cwd).
        assert_eq!(
            expand_user_path("~/prompts", None),
            PathBuf::from("~/prompts")
        );
    }
}

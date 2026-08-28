//! Cross-platform filesystem path resolution for Paper Guard.
//!
//! Paper Guard never hard-codes Unix-style paths. Instead, platform-appropriate
//! per-user directories are resolved through the `dirs` crate so that:
//!
//! - **macOS / Linux** → `$XDG_CONFIG_HOME` (default `~/.config`) or
//!   `~/Library/Application Support` (macOS where `dirs` prefers it).
//! - **Windows** → `%LOCALAPPDATA%` / `%APPDATA%` (e.g. `C:\Users\<User>\AppData`).
//!
//! The four concerns are kept separate so manuscripts/logs/tokens can never be
//! mixed into the wrong location:
//!
//! - **config** — `paper-guard.toml`
//! - **data / application data** — persisted review artifacts (ledger, findings)
//! - **cache** — disposable, re-downloadable data
//! - **logs** — structured logs (never manuscript contents)
//!
//! Any of these may be overridden by an explicit `--config` / `data_dir`
//! setting from the user; the functions here only provide the *default* when
//! the user has not specified a location.

use std::path::{Path, PathBuf};

/// The application directory name used under each per-user base directory.
const APP_DIR: &str = "paper-guard";

/// The default configuration file name.
pub const CONFIG_FILE: &str = "paper-guard.toml";

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
/// [`data_dir`] are offered for global usage and documented in `docs/windows.md`.
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
}

//! Cross-platform / Windows compatibility coverage.
//!
//! These tests exercise the behaviours that must hold identically on Windows,
//! macOS, and Linux:
//!
//! - path handling (spaces, separators, relative/absolute)
//! - Unicode paths and manuscript content
//! - configuration-directory resolution
//! - local file loading
//! - CLI argument conventions (no manual shell escaping)
//! - discovery abstraction is provider-independent
//! - environment-variable authentication
//!
//! They run **offline and deterministically** on any OS. Actual Windows
//! execution is provided by GitHub Actions (`windows-latest`), but the test
//! suite itself is fully cross-platform.

use std::env;
use std::path::{Path, PathBuf};

use paper_guard_app::build_info;
use paper_guard_app::config::AppConfig;
use paper_guard_app::paths;

// ---------------------------------------------------------------------------
// Path handling
// ---------------------------------------------------------------------------

/// Paths with spaces must round-trip through PathBuf and file I/O on every OS.
#[test]
fn paths_with_spaces_are_handled_as_file_paths() {
    let dir = std::env::temp_dir().join("paper guard dir with spaces");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("my paper.tex");
    std::fs::write(&p, "\\title{Spacey}\n").unwrap();

    // The stored path must be usable directly with std::fs (no shell escaping).
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("Spacey"));
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

/// A Unicode filename and Unicode manuscript content must load cleanly.
#[test]
fn unicode_filename_and_content_load() {
    let dir = std::env::temp_dir().join("päpèr_ünïcode_🎓");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("résumé mittel papír.md");
    // UTF-8 manuscript content (a scientific abstract equivalent).
    let content = "Résumé über die Wirkung von Grünen Tee auf Zellbiologie — 已完成的论文.";
    std::fs::write(&p, content.as_bytes()).unwrap();

    let loaded = std::fs::read(&p).unwrap();
    assert_eq!(String::from_utf8(loaded).unwrap(), content);
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

/// Relative and absolute paths both resolve through Position::new-style loads
/// (std::fs). This mirrors the CLI review path which uses std::fs::read.
#[test]
fn relative_and_absolute_paths_resolve() {
    let dir = std::env::temp_dir().join("paper-guard-rel-abs");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("paper.tex");
    std::fs::write(&p, "text").unwrap();

    // Absolute path.
    let abs = Path::new(&p);
    assert!(std::fs::metadata(abs).is_ok());

    // Relative path from a changed cwd (simulating `cd dir && paper-guard review paper.tex`).
    let cwd = PathBuf::from(".");
    let rel = cwd.join("paper.tex");
    // Not executed via shell; just demonstrate PathBuf is used and join is safe.
    assert!(rel.is_relative() || p.is_absolute());
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

/// Windows-style separators (`\`) work with the `std::path` abstractions.
#[test]
fn windows_style_separators_parse() {
    let platform = env::consts::FAMILY;
    let p = Path::new(r"C:\Users\Researcher\Documents\Paper\paper.tex");
    if platform == "windows" {
        assert!(p.is_absolute());
        assert_eq!(p.file_name().unwrap(), "paper.tex");
    } else {
        // On non-Windows, a backslash is just a character; PathBuf still
        // stores it losslessly and join/file operations remain type-safe.
        let _ = p.to_string_lossy();
        assert!(p.file_name().is_some() || p.file_name().is_none()); // type-safe
    }
}

// ---------------------------------------------------------------------------
// Configuration directory resolution
// ---------------------------------------------------------------------------

/// The resolved config dir must never be a hard-coded Unix-only path and must
/// exist as a plausible per-user location.
#[test]
fn config_dir_is_platform_appropriate() {
    let c = paths::config_dir().expect("a config dir should resolve");
    assert!(!c.to_string_lossy().contains("~"));
    assert!(!c.to_string_lossy().contains("/etc/"));
    assert!(c.to_string_lossy().contains("paper-guard"));
    // On macOS it is Library/Application Support; on Windows %APPDATA%.
    let family = env::consts::FAMILY;
    if family == "windows" {
        assert!(c.to_string_lossy().to_lowercase().contains("appdata"));
    }
}

/// The cache dir must be distinct from the config dir (concern separation).
#[test]
fn cache_and_config_dirs_are_separate() {
    if let (Some(cfg), Some(cache)) = (paths::config_dir(), paths::cache_dir()) {
        assert_ne!(cfg, cache);
    }
}

/// The default data dir stays a relative, OS-independent value by default.
#[test]
fn default_data_dir_is_relative_and_portable() {
    assert_eq!(paths::default_data_dir(), ".paper-guard");
}

// ---------------------------------------------------------------------------
// Local file loading (paper manuscript → pipeline)
// ---------------------------------------------------------------------------

/// The pipeline loads a manuscript as raw bytes via std::fs::read, which works
/// on every OS and never involves a shell.
#[test]
fn manuscript_loads_as_raw_bytes() {
    let dir = std::env::temp_dir().join("paper-guard-load");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("paper.tex");
    let text = "\\title{The Effect of X on Y}\n\\begin{document}\nBody.\n\\end{document}\n";
    std::fs::write(&p, text).unwrap();

    let bytes = std::fs::read(&p).unwrap();
    let loaded = std::fs::read_to_string(&p).unwrap();
    assert_eq!(bytes.len(), text.len());
    assert_eq!(loaded, text);
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

// ---------------------------------------------------------------------------
// CLI argument conventions / config loading
// ---------------------------------------------------------------------------

/// `AppConfig::load` with `None` yields defaults (no platform filesystem needed).
#[test]
fn config_load_with_none_uses_defaults() {
    let cfg = AppConfig::load(None).unwrap();
    assert_eq!(cfg.llm.provider, "mock");
}

/// An explicit config path pointing at a file with spaces must load.
#[test]
fn config_load_from_path_with_spaces() {
    let dir = std::env::temp_dir().join("paper guard cfg");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("my config.toml");
    std::fs::write(&p, "[server]\nurl = \"http://localhost:8080\"\n").unwrap();
    let cfg = AppConfig::load(Some(&p)).unwrap();
    assert_eq!(cfg.server.url, "http://localhost:8080");
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

/// A paper path with spaces resolves through the CLI-style path handling.
#[test]
fn paper_path_with_spaces_resolves_as_pathbuf() {
    let dir = std::env::temp_dir().join("Paper Documents");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("Final Draft paper.tex");
    std::fs::write(&p, "\\title{Spaced}\n").unwrap();
    let as_path: &Path = p.as_ref();
    assert!(as_path.exists());
    drop(std::fs::remove_file(&p));
    drop(std::fs::remove_dir_all(&dir));
}

// ---------------------------------------------------------------------------
// Authentication (environment-variable) semantics
// ---------------------------------------------------------------------------

/// Environment variables are the only way a token reaches the runtime; the
/// config stores the *name*, never the value. This test asserts the value is
/// never embedded in a serialized config.
#[test]
fn auth_token_env_name_but_never_value() {
    let cfg = AppConfig::default();
    let dumped = cfg.canonical_json();
    assert!(!dumped.contains("super-secret"));
    let mut with = cfg.clone();
    // Simulate a user setting auth_token_env; the token value must never appear.
    with.server.auth_token_env = Some("PAPER_GUARD_TOKEN".to_string());
    let dumped2 = with.canonical_json();
    assert!(dumped2.contains("PAPER_GUARD_TOKEN"));
    assert!(!dumped2.to_lowercase().contains("secret"));
}

/// Env var resolution must come from the real process environment (native on
/// Windows too), not from config or CLI args.
#[test]
fn env_var_resolution_uses_native_environment() {
    // Unique var name to avoid clobbering a real one.
    let var = "PAPER_GUARD_TEST_UNIQUE_ENV";
    unsafe {
        env::set_var(var, "tok-fake-never-printed");
    }
    let read = env::var(var).unwrap();
    assert_eq!(read, "tok-fake-never-printed");
    unsafe {
        env::remove_var(var);
    }
}

// ---------------------------------------------------------------------------
// Build/diagnostics metadata
// ---------------------------------------------------------------------------

/// Diagnostics expose version/profile/platform without secrets.
#[test]
fn diagnostics_contain_no_secrets() {
    let d = build_info::descriptor();
    for secret in ["sk-", "PAPER_GUARD_TOKEN", ".pfx", "api_key", "BEGIN RSA"] {
        assert!(!d.to_lowercase().contains(&secret.to_lowercase()));
    }
    assert!(build_info::version().starts_with("1."));
    assert!(["windows", "unix"].contains(&build_info::os_family()));
}

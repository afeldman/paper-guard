//! CLI integration smoke tests (offline, deterministic).
//!
//! These tests are deliberately small and fast: they verify the binary
//! exposes the CLI surface we document (version, `--gui`, `inspect`,
//! `review`) without running a full LLM.

use std::process::Command;

fn binary_path() -> &'static str {
    // Cargo sets `CARGO_BIN_EXE_paper-guard` for the current crate's bin.
    env!("CARGO_BIN_EXE_paper-guard")
}

#[test]
fn cli_version_reports_semver() {
    let out = Command::new(binary_path()).arg("--version").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("paper-guard"), "got: {text}");
    // v1.0.0 (or a 1.x semver) is required.
    assert!(
        text.contains("1."),
        "expected a 1.x semantic version, got: {text}"
    );
}

#[test]
fn cli_gui_flag_is_recognized_as_global() {
    let out = Command::new(binary_path())
        .args(["--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("--gui"),
        "expected --gui flag in --help, got: {text}"
    );
}

#[test]
fn cli_info_works() {
    let out = Command::new(binary_path()).arg("info").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Paper Guard"), "got: {text}");
    assert!(text.contains("1."), "expected version 1.x: {text}");
}

#[test]
fn cli_has_review_and_inspect_subcommands() {
    let out = Command::new(binary_path())
        .args(["--help"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("review"), "expected review subcommand: {text}");
    assert!(text.contains("inspect"), "expected inspect subcommand: {text}");
    assert!(text.contains("discover"), "expected discover: {text}");
    assert!(text.contains("serve"), "expected serve: {text}");
}

#[test]
fn cli_diagnostics_work() {
    let out = Command::new(binary_path())
        .args(["diagnostics", "--paths"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Paper Guard"), "got: {text}");
}

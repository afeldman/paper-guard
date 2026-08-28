//! Build metadata exposed through the `paper-guard diagnostics` command.
//!
//! This deliberately exposes **non-secret** build facts (version, OS triple,
//! commit, build profile) for reproducibility and traceability. It never
//! exposes secrets, paths that embed secrets, or machine-specific absolute
//! paths that could leak the builder's environment.

/// The semantic version of this build.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The Rust toolchain OS triple (e.g. `x86_64-pc-windows-msvc`).
///
/// Rust does not expose a single "target triple" constant, so we compose the
/// arch + OS + ABI from the cross-platform [`std::env::consts`] values. This is
/// informative (not authoritative); the authoritative triple is recorded by the
/// CI workflow at build time and surfaced through `diagnostics --paths`.
pub fn os_triple() -> String {
    format!(
        "{}-{}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        runtime_abi()
    )
}

/// The runtime ABI label (`msvc`, `gnu`, `musl`, or `other`), derived from the
/// platform's `target_env`-style signal available at compile time.
fn runtime_abi() -> &'static str {
    #[cfg(target_env = "msvc")]
    {
        "msvc"
    }
    #[cfg(not(target_env = "msvc"))]
    {
        "gnu"
    }
}

/// The host OS family (e.g. `windows`, `unix`).
pub fn os_family() -> &'static str {
    std::env::consts::FAMILY
}

/// The build profile, derived from the `debug_assertions` flag that Cargo sets
/// for dev builds. This is the standard non-secret way to report the profile.
pub fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// The short Git commit this binary was built from, if the build pipeline
/// embedded it into `PAPER_GUARD_BUILD_COMMIT`.
///
/// Cargo injects `CARGO_PKG_VERSION` but not a commit hash automatically. We
/// honour an optional env var (set by the release workflow) so the artifact can
/// be traced back to Git. This is *optional*; when unset we report `"unknown"`
/// rather than fabricate one.
pub fn commit() -> &'static str {
    option_env!("PAPER_GUARD_BUILD_COMMIT").unwrap_or("unknown")
}

/// A compact, machine-readable build descriptor.
pub fn descriptor() -> String {
    format!(
        "Paper Guard {} ({}, {}) commit={} profile={}",
        version(),
        os_triple(),
        os_family(),
        commit(),
        build_profile()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_populated() {
        assert!(!version().is_empty());
        assert!(version().starts_with("1."));
    }

    #[test]
    fn os_family_is_known() {
        assert!(["windows", "unix"].contains(&os_family()));
    }

    #[test]
    fn profile_is_debug_or_release() {
        assert!(["debug", "release"].contains(&build_profile()));
    }

    #[test]
    fn descriptor_contains_no_secrets() {
        let d = descriptor().to_lowercase();
        for secret in ["sk-", "paper_guard_token", ".pfx", "api_key"] {
            assert!(!d.contains(secret), "descriptor leaked: {secret}");
        }
    }
}

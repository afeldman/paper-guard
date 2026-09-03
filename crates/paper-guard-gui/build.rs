//! Build-time integration of the canonical Paper Guard logo
//! (`<workspace>/docs/logo.png`) into the GUI binary.
//!
//! Why a build script?
//!
//! The canonical logo lives at the workspace root (`docs/logo.png`), outside
//! this crate's package directory. A plain `include_bytes!("../../../docs/logo.png")`
//! would work for a one-off build, but Cargo only tracks files inside the
//! package directory for incremental rebuilds, so the binary could silently
//! keep a stale logo after `docs/logo.png` changes. A build script can declare
//! `cargo:rerun-if-changed` for an absolute path anywhere, which makes the
//! embed refresh correctly.
//!
//! `docs/logo.png` therefore remains the single canonical logo asset; the GUI
//! embeds a byte-identical copy produced here into `OUT_DIR`, keeping the
//! binary fully self-contained (no runtime filesystem access, no network, no
//! dependence on the current working directory).

use std::path::{Path, PathBuf};

/// Resolve the canonical logo at the workspace root:
/// `crates/paper-guard-gui -> ../../docs/logo.png`.
fn canonical_logo_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("logo.png")
}

fn main() {
    let logo = canonical_logo_path();

    // Fail with a clear message (instead of a confusing compile error) if the
    // canonical asset is missing, e.g. in a partial checkout.
    assert!(
        logo.is_file(),
        "canonical logo not found at {} — expected docs/logo.png at the \
         workspace root; build from a full Paper Guard checkout",
        logo.display()
    );

    let bytes = std::fs::read(&logo).expect("failed to read the canonical logo");
    assert!(
        bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n",
        "docs/logo.png is not a PNG file"
    );

    let out =
        Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR is set")).join("paper-guard-logo.png");
    std::fs::write(&out, &bytes).expect("failed to write embedded logo into OUT_DIR");

    // Rebuild whenever the canonical logo changes (the file is outside this
    // package, so Cargo only learns about it through this directive).
    println!("cargo:rerun-if-changed={}", logo.display());
}

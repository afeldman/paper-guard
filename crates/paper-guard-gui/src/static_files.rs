//! Embedded static assets for the local web GUI.
//!
//! The GUI is a single self-contained HTML page with inline CSS and JavaScript,
//! embedded via `include_str!`, plus one binary asset — the Paper Guard logo —
//! embedded via `include_bytes!`. Everything ships inside the binary (no
//! external CDN, no separate static asset directory to ship), so the GUI
//! renders its own chrome without any network request and works from any
//! working directory.
//!
//! The embedded logo bytes are copied from the canonical `docs/logo.png` at
//! the workspace root by this crate's `build.rs`; `docs/logo.png` remains the
//! single source of truth for the project logo.
//!
//! The frontend is **presentation-only**: it talks to the shared HTTP API and
//! never re-implements review/judge/parsing/authorization/ledger logic.

/// The main GUI HTML document. All CSS and JS is inlined.
pub const INDEX_HTML: &str = include_str!("static/index.html");

/// Embedded Paper Guard logo (PNG bytes, copied from the canonical
/// `docs/logo.png` by `build.rs` at compile time).
pub const LOGO_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/paper-guard-logo.png"));

/// The URL path at which the embedded logo is served.
pub const LOGO_PATH: &str = "/logo.png";

/// Extra static assets (returned with the correct content-type).
///
/// Each entry is `(MIME type, bytes, cache-control)`. Currently only the
/// embedded logo is exposed; future binary assets that cannot be inlined into
/// `index.html` can be added here with their own MIME type.
pub fn static_asset(path: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    match path {
        LOGO_PATH => Some(("image/png", LOGO_PNG, "public, max-age=3600")),
        _ => None,
    }
}

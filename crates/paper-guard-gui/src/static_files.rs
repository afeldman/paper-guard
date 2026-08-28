//! Embedded static assets for the local web GUI.
//!
//! The GUI is a single self-contained HTML page with inline CSS and JavaScript,
//! embedded via `include_str!` so the binary stays fully self-contained (no
//! external CDN, no separate static asset directory to ship).
//!
//! The frontend is **presentation-only**: it talks to the shared HTTP API and
//! never re-implements review/judge/parsing/authorization/ledger logic.

/// The main GUI HTML document. All CSS and JS is inlined.
pub const INDEX_HTML: &str = include_str!("static/index.html");

/// Extra static assets (returned with the correct content-type).
pub fn static_asset(path: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    // Currently everything is inlined into index.html; future assets that
    // cannot be inlined can be added here with their own MIME type.
    let _ = path;
    None
}

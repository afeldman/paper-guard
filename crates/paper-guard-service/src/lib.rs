//! # Paper Guard Service
//!
//! The optional HTTP service mode. It exposes a minimal, stable REST API and —
//! critically — runs the *same* application layer as the standalone CLI
//! (`paper_guard_app`), so the two entry points can never diverge in review
//! behaviour:
//!
//! ```text
//!   CLI ───────────────┐
//!                      ▼
//!                 paper-guard-app  (pipeline, review, judge, ledger, memory)
//!                      ▲
//!   HTTP API ──────────┘
//! ```
//!
//! Scope (M3): minimal useful API — `GET /health`, `POST /reviews`,
//! `GET /reviews/{run_id}`, `GET /reviews/{run_id}/findings`. A full web
//! application (auth, multi-tenant, approve/reject workflows) is out of scope
//! for M3; see §7–§9 of the M3 spec.

pub mod api;

pub use api::{app, serve, AppState};

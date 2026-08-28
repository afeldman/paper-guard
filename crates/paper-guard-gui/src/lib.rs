//! # Paper Guard GUI
//!
//! A **small local web GUI** for Paper Guard — a presentation/control layer
//! only. It reuses the shared application layer (`paper_guard_app`) and the
//! existing HTTP service (`paper_guard_service`) so it can never introduce a
//! second review engine or diverge from the CLI in behaviour.
//!
//! ```text
//!                  ┌── CLI
//!                  │
//! Canonical RunRecord
//!                  │
//!                  └── Local Web GUI
//!                          │
//!                          ▼
//!                   Paper Guard API
//!                          │
//!                          ▼
//!                   Existing pipeline
//! ```
//!
//! # Security model
//!
//! * Binds to `127.0.0.1` by default; never `0.0.0.0`, no LAN exposure unless
//!   explicitly configured.
//! * Treats manuscript content as untrusted input.
//! * Presentation styles (`neutral`/`funny`/`insulting`) are applied client-side
//!   from the canonical `RunRecord`; switching a style never triggers an LLM
//!   request and never alters the canonical data.
//! * All mutations funnel through the same domain/API boundaries the CLI uses.
//!
//! # Not implemented in 1.0
//!
//! No configuration wizard, no cloud accounts, no collaboration backend, no
//! remote hosted GUI, no automatic publication decisions. See the docs for the
//! planned v1.1 Configuration Wizard.

pub mod api;
pub mod gui;
pub mod static_files;

pub use api::{gui_router, GuiDashboardResponse, GuiRunListItem, GuiRunSummary};
pub use gui::{start_gui, GuiOptions, GuiStartup};

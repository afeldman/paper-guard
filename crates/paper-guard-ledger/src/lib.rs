//! # Paper Guard Ledger
//!
//! A persistent review ledger. Every run is assigned a stable id and recorded
//! as a versioned JSON artifact. Findings are tracked across iterations through
//! a lifecycle (OPEN -> ... -> RESOLVED / REGRESSED), and each run snapshots
//! the reproducibility metadata (hashes, versions, model configs, timestamps).

pub mod model;
pub mod store;

pub use model::{
    AgentOutcome, FindingRecord, JudgedRecord, ProviderUsage, RunRecord, RunStatus,
    ValidationRecord,
};
pub use store::{Ledger, LedgerError, LedgerStore};

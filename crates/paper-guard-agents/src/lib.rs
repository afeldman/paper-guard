//! # Paper Guard Agents
//!
//! The Revision Agent. It applies approved [`RevisionInstruction`]s strictly
//! within the allowed scope and never performs an integrity-forbidden change
//! (adding results, experiments, references, measurements, etc.).

pub mod revision;

pub use revision::{RevisionEngine, RevisionEngineOptions, RevisionOutcome};

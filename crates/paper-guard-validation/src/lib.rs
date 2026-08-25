//! # Paper Guard Validation
//!
//! Post-revision validation. After a revision the paper is re-rendered and
//! validated to catch lost text, damaged references, broken figures/tables,
//! missing captions, and inconsistent numbering.

pub mod text;

pub use text::{TextValidator, TextValidatorConfig, ValidationIssue, ValidationReport};

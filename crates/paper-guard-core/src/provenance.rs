//! Provenance model.
//!
//! Every scientific statement Paper Guard processes must carry an unambiguous
//! origin. This answers the question *"who produced this content?"* at every
//! stage of the pipeline:
//!
//! ```text
//! AUTHOR_CONTENT (the manuscript as authored)
//!    └─> PARSER_OUTPUT     (canonical model structs extracted by the parser)
//!    └─> REVIEWER_OUTPUT   (findings produced by a reviewer)
//!    └─> JUDGE_OUTPUT      (consolidated findings / revision instructions)
//!    └─> REVISION_INSTRUCTION (the scoped instruction a revision may apply)
//!    └─> REVISION_OUTPUT   (actual textual changes produced by the engine)
//!    └─> VALIDATION_OUTPUT (post-revision validation results)
//! ```
//!
//! Distinguishing provenance is essential to the scientific-integrity model:
//! Paper Guard must never let LLM-generated content be mistaken for
//! author-supplied content, and never let an invented fact be represented as a
//! parsable, author-backed claim.

use serde::{Deserialize, Serialize};

/// The origin of a piece of content within the Paper Guard pipeline.
///
/// The variants are intentionally distinct and mutually exclusive. A statement
/// is either authored by the human, extracted by a parser, or produced by one
/// of the automated stages — never ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Provenance {
    /// Original content authored by the paper's authors.
    AuthorContent,
    /// Content extracted/normalized by a parser into the canonical model.
    ParserOutput,
    /// A finding produced by a reviewer.
    ReviewerOutput,
    /// A consolidated finding / decision produced by the judge.
    JudgeOutput,
    /// A scoped instruction for what a revision may change.
    RevisionInstruction,
    /// An actual textual change produced by the revision engine.
    RevisionOutput,
    /// A validation result emitted after re-rendering.
    ValidationOutput,
}

impl Provenance {
    /// Whether this provenance denotes content authored by the paper's authors
    /// (as opposed to LLM- or machine-produced content).
    pub fn is_author_produced(&self) -> bool {
        matches!(self, Provenance::AuthorContent)
    }

    /// Whether this provenance denotes purely automated system output that must
    /// never be mistaken for author content.
    pub fn is_system_produced(&self) -> bool {
        !self.is_author_produced()
    }

    /// A short, stable tag (e.g. for serialization and reporting).
    pub fn tag(&self) -> &'static str {
        match self {
            Provenance::AuthorContent => "AUTHOR_CONTENT",
            Provenance::ParserOutput => "PARSER_OUTPUT",
            Provenance::ReviewerOutput => "REVIEWER_OUTPUT",
            Provenance::JudgeOutput => "JUDGE_OUTPUT",
            Provenance::RevisionInstruction => "REVISION_INSTRUCTION",
            Provenance::RevisionOutput => "REVISION_OUTPUT",
            Provenance::ValidationOutput => "VALIDATION_OUTPUT",
        }
    }
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.tag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_vs_system_provenance() {
        assert!(Provenance::AuthorContent.is_author_produced());
        assert!(!Provenance::ReviewerOutput.is_author_produced());
        assert!(Provenance::ReviewerOutput.is_system_produced());
        assert!(Provenance::RevisionOutput.is_system_produced());
        // An LLM-generated review finding is never author content.
        assert!(Provenance::ReviewerOutput.is_system_produced());
    }

    #[test]
    fn stable_tags() {
        assert_eq!(Provenance::AuthorContent.tag(), "AUTHOR_CONTENT");
        assert_eq!(Provenance::ParserOutput.tag(), "PARSER_OUTPUT");
        assert_eq!(Provenance::ReviewerOutput.tag(), "REVIEWER_OUTPUT");
        assert_eq!(Provenance::JudgeOutput.tag(), "JUDGE_OUTPUT");
        assert_eq!(Provenance::RevisionInstruction.tag(), "REVISION_INSTRUCTION");
        assert_eq!(Provenance::RevisionOutput.tag(), "REVISION_OUTPUT");
        assert_eq!(Provenance::ValidationOutput.tag(), "VALIDATION_OUTPUT");
    }

    #[test]
    fn serializes_unambiguously() {
        assert_eq!(
            serde_json::to_string(&Provenance::RevisionInstruction).unwrap(),
            "\"REVISION_INSTRUCTION\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::AuthorContent).unwrap(),
            "\"AUTHOR_CONTENT\""
        );
    }
}

//! Review output presentation styles.
//!
//! Paper Guard can render its human-readable review report in three styles:
//!
//! * [`ReviewStyle::Neutral`] — sober, scientific, professional (the default).
//! * [`ReviewStyle::Funny`] — humorous, lightly ironic, but still factually
//!   correct.
//! * [`ReviewStyle::Insulting`] — deliberately sharp and biting toward the
//!   paper/argument/problem, never ad hominem toward real authors.
//!
//! These styles are **purely presentational**. They change only the wording of
//! the human-readable prose. They never alter a finding's scientific content:
//! severity, confidence, evidence, claims, category, recommendation, or any
//! Judge/revision decision are untouched. The canonical finding dataset is
//! byte-for-byte identical across all three styles (see the crate tests, which
//! assert the canonical representation is invariant).

use serde::{Deserialize, Serialize};

/// A review output presentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStyle {
    /// Sober, scientific, professional.
    #[default]
    Neutral,
    /// Humorous, lightly ironic.
    Funny,
    /// Deliberately sharp and biting toward the paper/argument.
    Insulting,
}

impl ReviewStyle {
    /// The canonical lowercase name for this style.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStyle::Neutral => "neutral",
            ReviewStyle::Funny => "funny",
            ReviewStyle::Insulting => "insulting",
        }
    }

    /// Parse a style name, accepting only the three documented names.
    ///
    /// Returns `None` for any other value so the caller can surface a clear
    /// error rather than silently defaulting (which could hide a typo).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "neutral" => Some(ReviewStyle::Neutral),
            "funny" => Some(ReviewStyle::Funny),
            "insulting" => Some(ReviewStyle::Insulting),
            _ => None,
        }
    }

    /// The valid style names, for error messages.
    pub const fn valid_names() -> &'static [&'static str] {
        &["neutral", "funny", "insulting"]
    }
}

impl std::fmt::Display for ReviewStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// An error produced when an invalid style name is encountered.
#[derive(Debug, thiserror::Error)]
#[error(
    "invalid review style `{given}`; expected one of: {valid}",
    valid = ReviewStyle::valid_names().join(", ")
)]
pub struct UnrecognizedStyle {
    given: String,
}

/// Parse a style name or return a helpful error.
pub fn parse_style_or_err(s: &str) -> Result<ReviewStyle, UnrecognizedStyle> {
    ReviewStyle::parse(s).ok_or_else(|| UnrecognizedStyle {
        given: s.trim().to_string(),
    })
}

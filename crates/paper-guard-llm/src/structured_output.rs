//! Structured-output capability for OpenAI-compatible providers.
//!
//! Paper Guard never treats structured output (JSON Schema or `json_object`)
//! as making the model scientifically trustworthy. Structured output only
//! constrains the *transport* format so the endpoint is more likely to return
//! well-formed JSON. Scientific validity is still enforced downstream by
//! Paper Guard's reviewer-side domain validation, evidence checks, provenance,
//! Judge, and integrity guards. JSON Schema enforcement is therefore *not*
//! scientific correctness — the two concerns stay fully separate.

/// How an OpenAI-compatible endpoint should constrain its generated output's
/// transport format.
///
/// This maps exactly onto the `response_format` element of a chat-completions
/// request and is chosen by *configuration* (never inferred or silently
/// downgraded). The mode is a contract between the operator and the endpoint:
///
/// * `Off` — no `response_format` is sent (free-form text). Reviewer-side
///   validation still rejects anything that is not a conforming findings
///   array; this is the historical `structured_output = false` behaviour.
/// * `JsonObject` — sends `{"type":"json_object"}`; the endpoint constrains
///   the reply to a JSON *object* only (conceptual shape, no field schema).
///   This is the historical `structured_output = true` behaviour.
/// * `JsonSchema` — sends a full JSON Schema via
///   `{"type":"json_schema", ...}`. The schema is supplied by the caller (the
///   reviewer) and constrains fields, types, enums, arrays, and requiredness.
///
/// Because the mode is explicit configuration, Paper Guard never silently
/// falls back from a stricter to a looser mode: if an operator requests
/// `JsonSchema` but the request carries no schema, or the endpoint lacks the
/// capability, the provider fails with a clear capability/config error rather
/// than downgrading to unconstrained generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredOutputMode {
    /// No `response_format`; the endpoint returns free-form text.
    Off,
    /// `{"type":"json_object"}` (no field schema). Historical default.
    JsonObject,
    /// `{"type":"json_schema", ...}` with the caller-supplied schema.
    JsonSchema,
}

impl StructuredOutputMode {
    /// Parse a mode from the historical `bool` form.
    ///
    /// `true` means "request structured JSON" which maps to [`JsonObject`];
    /// `false` means free-form ([`Off`]). Booleans never select [`JsonSchema`],
    /// which requires an explicit string so the operator deliberately opts in.
    pub fn from_bool(b: bool) -> Self {
        if b {
            StructuredOutputMode::JsonObject
        } else {
            StructuredOutputMode::Off
        }
    }

    /// Parse a mode from a configuration string. Accepts the documented
    /// lowercase spellings and the historical boolean strings.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "disabled" | "none" => Some(StructuredOutputMode::Off),
            "json_object" | "json-object" | "true" | "object" => {
                Some(StructuredOutputMode::JsonObject)
            }
            "json_schema" | "json-schema" | "schema" => Some(StructuredOutputMode::JsonSchema),
            _ => None,
        }
    }

    /// A short stable label for logging / introspection.
    pub fn as_str(self) -> &'static str {
        match self {
            StructuredOutputMode::Off => "off",
            StructuredOutputMode::JsonObject => "json_object",
            StructuredOutputMode::JsonSchema => "json_schema",
        }
    }
}

impl std::fmt::Display for StructuredOutputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A caller-supplied JSON Schema used to constrain an endpoint's structured
/// output. The *schema owner* is the Paper Guard pipeline (in practice the
/// reviewer layer, which derives it from the strongly-typed domain payload
/// struct), so the model can produce conforming JSON without weakening the
/// downstream domain validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StructuredOutputSpec {
    /// A short, unique name identifying the schema (surfaced as the
    /// `json_schema.name` in the request).
    pub name: String,
    /// The JSON Schema describing the expected payload (draft-07 object).
    pub schema: serde_json::Value,
    /// Whether the endpoint should enforce the schema strictly (`strict: true`).
    #[serde(default = "default_strict")]
    pub strict: bool,
}

fn default_strict() -> bool {
    true
}

impl StructuredOutputSpec {
    /// Build a new spec. `strict` defaults to `true` so endpoints that support
    /// strict JSON Schema enforcement reject everything that does not exactly
    /// match the schema.
    pub fn new(name: impl Into<String>, schema: serde_json::Value) -> Self {
        StructuredOutputSpec {
            name: name.into(),
            schema,
            strict: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_mapping_preserves_historical_semantics() {
        assert_eq!(
            StructuredOutputMode::from_bool(true),
            StructuredOutputMode::JsonObject
        );
        assert_eq!(
            StructuredOutputMode::from_bool(false),
            StructuredOutputMode::Off
        );
        // A bool can never opt into JsonSchema.
        assert_ne!(
            StructuredOutputMode::from_bool(true),
            StructuredOutputMode::JsonSchema
        );
    }

    #[test]
    fn string_parsing_accepts_documented_spellings() {
        assert_eq!(
            StructuredOutputMode::parse_str("json_schema"),
            Some(StructuredOutputMode::JsonSchema)
        );
        assert_eq!(
            StructuredOutputMode::parse_str("json-object"),
            Some(StructuredOutputMode::JsonObject)
        );
        assert_eq!(
            StructuredOutputMode::parse_str("off"),
            Some(StructuredOutputMode::Off)
        );
        assert_eq!(
            StructuredOutputMode::parse_str("true"),
            Some(StructuredOutputMode::JsonObject)
        );
        assert_eq!(
            StructuredOutputMode::parse_str("false"),
            Some(StructuredOutputMode::Off)
        );
        assert_eq!(StructuredOutputMode::parse_str("bogus"), None);
    }

    #[test]
    fn spec_defaults_to_strict() {
        let spec = StructuredOutputSpec::new("finding", serde_json::json!({"type": "object"}));
        assert!(spec.strict);
    }
}

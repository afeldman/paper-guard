//! Configuration (paper-guard.toml).
//!
//! This is the shared application-layer configuration consumed by both the
//! standalone CLI and the HTTP service. It lives here (not in the CLI binary)
//! so that both entry points drive the same pipeline from the same config.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The project configuration root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AppConfig {
    pub project: ProjectConfig,
    pub input: InputConfig,
    pub llm: LlmConfig,
    pub providers: ProvidersConfig,
    pub reviewers: ReviewersConfig,
    pub review: ReviewConfig,
    pub judge: JudgeConfig,
    pub revision: RevisionConfig,
    pub reproducibility: ReproducibilityConfig,
    pub service: ServiceConfig,
    pub memory: MemoryConfig,
    pub server: ServerConfig,
    pub discovery: DiscoverySectionConfig,
}

/// Top-level LLM provider selection.
///
/// `provider = "mock"` (the default) keeps every run offline and deterministic.
/// Set `provider = "openai-compatible"` to use the real provider; the endpoint
/// details live under `[providers.openai-compatible]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// The provider kind used at run time: "mock" or "openai-compatible".
    pub provider: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            provider: "mock".into(),
        }
    }
}

/// Backend-specific provider configuration.
///
/// `rename_all = "kebab-case"` makes the documented TOML key
/// `[providers.openai-compatible]` (hyphenated) map onto the Rust field
/// `openai_compatible`. Serde does not translate `-` to `_` automatically, so
/// without this rename the entire provider section would be silently ignored
/// as an unknown field and `Default` values used instead.
///
/// Note: `deny_unknown_fields` is intentionally NOT applied here. The minimal
/// fix is the kebab-case mapping above; failing loudly on unknown provider
/// sub-keys (e.g. a legacy underscore spelling) would be a separate behavior
/// change that could reject previously-tolerated configs, so it is left for a
/// deliberate follow-up rather than bundled with this bug fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[derive(Default)]
pub struct ProvidersConfig {
    pub openai_compatible: OpenAICompatibleSectionConfig,
}

/// How an OpenAI-compatible endpoint constrains its output at the transport
/// layer (the `response_format` of a chat-completions request).
///
/// This is a *configuration* knob describing what the operator wants the
/// endpoint to do, mapped onto the provider's [`StructuredOutputMode`]. It is
/// deliberately **not** a way to make an LLM scientifically trustworthy: it
/// only constrains the JSON transport shape. Scientific validity is enforced
/// separately by Paper Guard's domain validation, evidence checks, provenance,
/// Judge, and integrity guards. JSON Schema enforcement is *not* scientific
/// correctness.
///
/// It is backward compatible with the historical `bool` form:
///
/// ```toml
/// structured_output = false   # free-form; reviewer-side validation still enforces JSON
/// structured_output = true    # {"type":"json_object"} (historical default)
/// structured_output = "json_object"
/// structured_output = "json_schema"
/// ```
///
/// `false`/`"off"` means no `response_format`; `true`/`"json_object"` means the
/// endpoint constrains replies to a JSON object; `"json_schema"` means a full
/// JSON Schema is sent (provided by the reviewer via the request). The provider
/// never silently downgrades from a stricter requested mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructuredOutputConfig {
    /// No `response_format`; free-form text (with reviewer-side validation).
    Off,
    /// `{"type":"json_object"}` (historical `true`).
    #[default]
    JsonObject,
    /// `{"type":"json_schema", ...}`.
    JsonSchema,
}

impl StructuredOutputConfig {
    /// Whether the endpoint/model is asked to produce structured JSON output.
    pub fn supports_structured(&self) -> bool {
        *self != StructuredOutputConfig::Off
    }

    /// The provider-level mode that corresponds to this configuration.
    pub fn to_mode(&self) -> paper_guard_llm::StructuredOutputMode {
        match self {
            StructuredOutputConfig::Off => paper_guard_llm::StructuredOutputMode::Off,
            StructuredOutputConfig::JsonObject => paper_guard_llm::StructuredOutputMode::JsonObject,
            StructuredOutputConfig::JsonSchema => paper_guard_llm::StructuredOutputMode::JsonSchema,
        }
    }

    /// A short stable label for logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            StructuredOutputConfig::Off => "off",
            StructuredOutputConfig::JsonObject => "json_object",
            StructuredOutputConfig::JsonSchema => "json_schema",
        }
    }
}

impl<'de> Deserialize<'de> for StructuredOutputConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bool(true) => Ok(StructuredOutputConfig::JsonObject),
            Raw::Bool(false) => Ok(StructuredOutputConfig::Off),
            Raw::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
                "off" | "false" | "disabled" | "none" => Ok(StructuredOutputConfig::Off),
                "json_object" | "json-object" | "true" | "object" => {
                    Ok(StructuredOutputConfig::JsonObject)
                }
                "json_schema" | "json-schema" | "schema" => Ok(StructuredOutputConfig::JsonSchema),
                other => Err(serde::de::Error::custom(format!(
                    "invalid structured_output value `{other}`; expected one of \
                     false, true, \"json_object\", or \"json_schema\""
                ))),
            },
        }
    }
}

impl Serialize for StructuredOutputConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// The `[providers.openai-compatible]` section.
///
/// This is the single production backend connecting to any OpenAI-compatible
/// endpoint (OpenAI, Mammoth.ai, a local server such as Ollama's OpenAI-
/// compatible `/v1`, etc.). The difference between backends is *configuration
/// only* — never code. Secrets (the API key) are never stored here; only the
/// name of the environment variable that holds them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAICompatibleSectionConfig {
    /// Base URL of the OpenAI-compatible endpoint, e.g. `https://api.openai.com/v1`.
    pub base_url: String,
    /// Environment variable holding the API key, e.g. `OPENAI_API_KEY`.
    /// When absent, requests are sent without an Authorization header (for
    /// local endpoints such as Ollama that need no key).
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Model name (configuration-driven, never hard-coded).
    pub model: String,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
    /// Maximum number of retries (transient errors only).
    pub max_retries: u32,
    /// How the endpoint constrains its output at the transport layer. See
    /// [`StructuredOutputConfig`] for the accepted values and the distinction
    /// from scientific validity.
    #[serde(default)]
    pub structured_output: StructuredOutputConfig,
    /// Whether the endpoint/model supports multimodal vision input.
    pub vision: bool,
}

impl Default for OpenAICompatibleSectionConfig {
    fn default() -> Self {
        OpenAICompatibleSectionConfig {
            base_url: "https://api.openai.com/v1".into(),
            api_key_env: Some("OPENAI_API_KEY".into()),
            model: "gpt-4o-mini".into(),
            timeout_seconds: 120,
            max_retries: 2,
            structured_output: StructuredOutputConfig::default(),
            vision: false,
        }
    }
}

/// The `[service]` section — the optional HTTP service mode.
///
/// Service mode runs the *same* application pipeline as the CLI over HTTP. The
/// service is local-only by default and exposes no destructive endpoints in
/// M3; authentication/authorization is explicitly out of scope and documented.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceConfig {
    /// The bind address for the HTTP server, e.g. `127.0.0.1:8080`.
    pub bind: String,
    /// Whether the service may bind to a non-loopback (network) address.
    /// Defaults to `false`: the service refuses to start on anything other
    /// than loopback unless this is explicitly set, so it cannot silently
    /// expose an unauthenticated interface to the network.
    pub allow_external_bind: bool,
    /// Directory where service-run artifacts (ledger, manuscripts, memory)
    /// are persisted.
    pub data_dir: String,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        ServiceConfig {
            bind: "127.0.0.1:8080".into(),
            allow_external_bind: false,
            data_dir: ".paper-guard".into(),
        }
    }
}

/// The `[memory]` section — Review Memory (retrieval-based, not training).
///
/// Review Memory stores *human-approved* review units that may later be used
/// as retrieval context for a local reviewer. It is separate from the LLM
/// provider and never becomes current-paper evidence. The default approval
/// state is `PRIVATE`, so nothing is ever used for training unless explicitly
/// approved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Whether Review Memory is enabled at all. Defaults to `false`: existing
    /// behavior is unchanged unless memory is explicitly enabled.
    pub enabled: bool,
    /// Backend: `none` (off, default for standalone), `file` (offline JSON
    /// store), or `qdrant` (vector backend, service mode).
    pub backend: String,
    /// Memory access mode: `off`, `read_only`, `write`, or `read_write`.
    /// `off` disables both storage and retrieval; `read_only` uses approved
    /// memory but stores nothing new; `write` stores approved feedback but
    /// retrieves nothing; `read_write` does both.
    pub mode: String,
    /// Qdrant endpoint (e.g. `http://localhost:6333`). Only used when
    /// `backend = "qdrant"`.
    pub qdrant_url: String,
    /// Qdrant collection name for review memory.
    pub collection: String,
    /// Whether a memory entry requires explicit human approval before it is
    /// eligible to be retrieved as context (MEMORY_APPROVED) or exported to
    /// a training dataset (TRAINING_APPROVED). Defaults to true: nothing is
    /// eligible without explicit consent.
    pub require_approval: bool,
    /// Maximum number of retrieved memory entries per review. Defaults to 5.
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Minimum cosine similarity threshold (0..=1) for vector retrieval.
    /// Defaults to 0.75.
    #[serde(default = "default_min_similarity")]
    pub min_similarity: f32,
    /// The embedding provider kind: `mock` (offline, default) or
    /// `openai-compatible` (incl. Ollama `/embeddings`).
    pub embedding_provider: String,
    /// The embedding model name (used by `openai-compatible`; ignored by
    /// `mock`). Configurable, never hard-coded.
    pub embedding_model: String,
    /// The owner identity attributed to locally-recorded memory (never a
    /// secret; used for scope/authorization).
    #[serde(default)]
    pub owner_id: String,
    /// An optional team id for sharing approved memory across a team.
    #[serde(default)]
    pub team_id: String,
}

fn default_top_k() -> usize {
    5
}

fn default_min_similarity() -> f32 {
    0.75
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            enabled: false,
            backend: "none".into(),
            mode: "off".into(),
            qdrant_url: "http://localhost:6333".into(),
            collection: "review_memory".into(),
            require_approval: true,
            top_k: default_top_k(),
            min_similarity: default_min_similarity(),
            embedding_provider: "mock".into(),
            embedding_model: "mock".into(),
            owner_id: String::new(),
            team_id: String::new(),
        }
    }
}

/// The memory access mode (see `[memory] mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMode {
    /// Store nothing, retrieve nothing.
    Off,
    /// Use approved memory as context, but store no new entries.
    ReadOnly,
    /// Store approved feedback as memory, but do not retrieve as context.
    Write,
    /// Store and retrieve.
    ReadWrite,
}

impl MemoryMode {
    /// Resolve a `[memory] mode` string into a mode.
    pub fn parse(s: &str) -> MemoryMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "read_only" | "readonly" | "read-only" => MemoryMode::ReadOnly,
            "write" => MemoryMode::Write,
            "read_write" | "readwrite" | "read-write" => MemoryMode::ReadWrite,
            _ => MemoryMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryMode::Off => "off",
            MemoryMode::ReadOnly => "read_only",
            MemoryMode::Write => "write",
            MemoryMode::ReadWrite => "read_write",
        }
    }

    /// Whether this mode stores new entries.
    pub fn stores(&self) -> bool {
        matches!(self, MemoryMode::Write | MemoryMode::ReadWrite)
    }

    /// Whether this mode retrieves memory as context.
    pub fn retrieves(&self) -> bool {
        matches!(self, MemoryMode::ReadOnly | MemoryMode::ReadWrite)
    }
}

/// The `[server]` section — optional connection to a remote Paper Guard
/// service (M3.5). When `url` is set, `paper-guard review/run` executes the
/// manuscript on the remote service instead of locally. The service is
/// authoritative for a remote run; the CLI never writes its own ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Base URL of the remote Paper Guard service, e.g.
    /// `http://localhost:8080`. Empty means "local mode" unless an explicit
    /// `--server` flag overrides it on the command line.
    #[serde(default)]
    pub url: String,
    /// Name of an environment variable holding a bearer/API token to send to
    /// the service. Never stores the token itself; the value is read from the
    /// environment at runtime and never logged.
    #[serde(default)]
    pub auth_token_env: Option<String>,
    /// Request timeout in seconds for remote calls.
    #[serde(default = "default_server_timeout")]
    pub timeout_seconds: u64,
}

/// Default remote request timeout (120s) — long enough for a mock or real
/// review pipeline to complete.
fn default_server_timeout() -> u64 {
    120
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            url: String::new(),
            auth_token_env: None,
            timeout_seconds: default_server_timeout(),
        }
    }
}

/// The `[discovery]` section — optional LAN (mDNS/DNS-SD) service discovery.
///
/// Discovery is **disabled by default** so Paper Guard never probes the network
/// implicitly. When enabled, discovery only *lists* and *verifies* Paper Guard
/// services; it never authorises a manuscript upload. A manuscript is only ever
/// sent to a remote service when remote execution has been explicitly selected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoverySectionConfig {
    /// Master switch. `false` disables all discovery regardless of `mode`.
    pub enabled: bool,
    /// Discovery mode: `off`, `manual`, or `auto`. Unknown values fail closed
    /// to `off`.
    pub mode: String,
    /// Optional DNS-SD service type to browse for; usually left at the default
    /// `_paper-guard._tcp.local.`.
    pub service_type: String,
    /// How long (in milliseconds) to wait for mDNS responses.
    pub timeout_ms: u64,
    /// Optional exact hostname (e.g. `paper-guard.lab.local`) that Auto mode
    /// may prefer when multiple services are present. Never "first response
    /// wins".
    pub preferred_service: String,
}

impl Default for DiscoverySectionConfig {
    fn default() -> Self {
        DiscoverySectionConfig {
            enabled: false,
            mode: "off".into(),
            service_type: "_paper-guard._tcp.local.".into(),
            timeout_ms: 3000,
            preferred_service: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            name: "my-paper".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct InputConfig {
    /// Explicit format override (pdf, latex, typst, docx).
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewerSectionConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub seed: Option<u64>,
}

impl Default for ReviewerSectionConfig {
    fn default() -> Self {
        ReviewerSectionConfig {
            enabled: true,
            provider: "mock".into(),
            model: "mock".into(),
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewersConfig {
    pub scientific: ReviewerSectionConfig,
    pub adversarial: ReviewerSectionConfig,
    pub evidence: ReviewerSectionConfig,
    pub references: ReviewerSectionConfig,
    pub figures: ReviewerSectionConfig,
    /// Whether reviews run in parallel (default true).
    pub parallel: bool,
    /// Max concurrent agents.
    pub max_concurrent: usize,
}

impl Default for ReviewersConfig {
    fn default() -> Self {
        ReviewersConfig {
            scientific: ReviewerSectionConfig::default(),
            adversarial: ReviewerSectionConfig::default(),
            evidence: ReviewerSectionConfig::default(),
            references: ReviewerSectionConfig::default(),
            figures: ReviewerSectionConfig::default(),
            parallel: true,
            max_concurrent: 5,
        }
    }
}

/// The `[review]` section — presentation-level review options.
///
/// These are **purely presentational** and never affect the scientific
/// pipeline. `style` selects the human-readable output style (`neutral`,
/// `funny`, or `insulting`), defaulting to `neutral`. The style only changes
/// the wording of the human-readable report; the canonical findings
/// (`findings.json`, `judge.json`, `claims.json`, the ledger) are always
/// style-independent. The CLI `--style` flag overrides this value, and the
/// config value overrides the `neutral` default. There is no implicit
/// switching via environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewConfig {
    /// The presentation style for the human-readable report.
    /// Valid values: `neutral`, `funny`, `insulting`. Defaults to `neutral`.
    pub style: String,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        ReviewConfig {
            style: "neutral".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JudgeConfig {
    pub model: String,
    pub require_human_approval_for_major: bool,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        JudgeConfig {
            model: "mock".into(),
            require_human_approval_for_major: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RevisionConfig {
    pub require_human_approval_for_major: bool,
    /// Path where patch/revision artifacts are emitted.
    pub output_dir: String,
}

impl Default for RevisionConfig {
    fn default() -> Self {
        RevisionConfig {
            require_human_approval_for_major: true,
            output_dir: ".paper-guard".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReproducibilityConfig {
    pub seed: Option<u64>,
    pub prompt_version: String,
    pub data_dir: String,
}

impl Default for ReproducibilityConfig {
    fn default() -> Self {
        ReproducibilityConfig {
            seed: Some(42),
            prompt_version: "v1".into(),
            data_dir: ".paper-guard".into(),
        }
    }
}

impl AppConfig {
    /// Load configuration from a path or use defaults.
    pub fn load(path: Option<&Path>) -> anyhow::Result<AppConfig> {
        match path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p)?;
                Ok(toml::from_str(&text)?)
            }
            _ => Ok(AppConfig::default()),
        }
    }

    /// Write the default config to a path (for `paper-guard init`).
    pub fn write_default_to(path: &Path) -> anyhow::Result<()> {
        let config = AppConfig::default();
        let text = toml::to_string_pretty(&config)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// The prompt version to use across agents.
    pub fn prompt_version(&self) -> &str {
        &self.reproducibility.prompt_version
    }

    /// A canonical JSON dump of the config for hashing.
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The data directory to use for this configuration. Standalone mode uses
    /// `reproducibility.data_dir`; service mode uses `service.data_dir`.
    pub fn effective_data_dir(&self) -> &str {
        &self.reproducibility.data_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_loads() {
        let cfg = AppConfig::default();
        assert!(cfg.reviewers.parallel);
        assert_eq!(cfg.reviewers.max_concurrent, 5);
        assert!(cfg.judge.require_human_approval_for_major);
        // The default provider is mock (offline / deterministic by default).
        assert_eq!(cfg.llm.provider, "mock");
        // The service is local-only and memory is off by default.
        assert_eq!(cfg.service.bind, "127.0.0.1:8080");
        assert!(!cfg.service.allow_external_bind);
        assert_eq!(cfg.memory.backend, "none");
        // Memory is disabled by default (M4): existing behavior is unchanged.
        assert!(!cfg.memory.enabled);
        assert_eq!(cfg.memory.mode, "off");
        assert_eq!(cfg.memory.top_k, 5);
        assert_eq!(cfg.memory.min_similarity, 0.75);
        assert_eq!(cfg.memory.embedding_provider, "mock");
    }

    #[test]
    fn memory_mode_parsing() {
        assert!(MemoryMode::parse("off") == MemoryMode::Off);
        assert!(MemoryMode::parse("read_only") == MemoryMode::ReadOnly);
        assert!(MemoryMode::parse("read-write") == MemoryMode::ReadWrite);
        assert!(MemoryMode::parse("write") == MemoryMode::Write);
        // Unknown modes fail closed to Off (never implicitly enable memory).
        assert!(MemoryMode::parse("garbage") == MemoryMode::Off);
        assert!(!MemoryMode::Off.stores() && !MemoryMode::Off.retrieves());
        assert!(!MemoryMode::ReadOnly.stores() && MemoryMode::ReadOnly.retrieves());
        assert!(MemoryMode::Write.stores() && !MemoryMode::Write.retrieves());
        assert!(MemoryMode::ReadWrite.stores() && MemoryMode::ReadWrite.retrieves());
    }

    #[test]
    fn memory_config_roundtrips_with_new_fields() {
        let src = r#"
[memory]
enabled = true
mode = "read_write"
top_k = 8
min_similarity = 0.7
embedding_provider = "mock"
embedding_model = "mock"
owner_id = "alice"
team_id = "team-a"
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        assert!(cfg.memory.enabled);
        assert_eq!(cfg.memory.mode, "read_write");
        assert_eq!(cfg.memory.top_k, 8);
        assert_eq!(cfg.memory.min_similarity, 0.7);
        assert_eq!(cfg.memory.owner_id, "alice");
        assert_eq!(cfg.memory.team_id, "team-a");
        assert_eq!(MemoryMode::parse(&cfg.memory.mode), MemoryMode::ReadWrite);
    }

    #[test]
    fn server_config_defaults_to_local_mode() {
        let cfg = AppConfig::default();
        // No server URL => local mode by default.
        assert_eq!(cfg.server.url, "");
        assert!(cfg.server.auth_token_env.is_none());
        assert_eq!(cfg.server.timeout_seconds, 120);
    }

    #[test]
    fn server_config_roundtrips_and_never_embeds_token() {
        let src = r#"
[server]
url = "http://localhost:8080"
auth_token_env = "PAPER_GUARD_TOKEN"
timeout_seconds = 60
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        assert_eq!(cfg.server.url, "http://localhost:8080");
        assert_eq!(
            cfg.server.auth_token_env.as_deref(),
            Some("PAPER_GUARD_TOKEN")
        );
        assert_eq!(cfg.server.timeout_seconds, 60);
        // The token itself is never part of the config; only the env name is.
        let dumped = serde_json::to_string(&cfg).unwrap();
        assert!(!dumped.contains("super-secret-token"));
    }

    #[test]
    fn toml_roundtrip_and_back() {
        let cfg = AppConfig::default();
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.project.name, cfg.project.name);
        assert_eq!(parsed.llm.provider, "mock");
        assert_eq!(
            parsed.providers.openai_compatible.base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn openai_provider_config_does_not_embed_secret() {
        let cfg = AppConfig::default();
        let json = cfg.canonical_json();
        // The API key itself must never be in the serialized config; only the
        // environment variable *name* is allowed.
        assert!(!json.contains("sk-"));
        assert!(json.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn providers_openai_compatible_honors_kebab_case_key() {
        // Regression for the config bug where the documented hyphenated TOML
        // key `[providers.openai-compatible]` did not map onto the Rust field
        // `openai_compatible`. Because serde silently ignored the unknown
        // table, the section fell back to `Default` (gpt-4o-mini +
        // OPENAI_API_KEY). This test pins the documented key to the supplied
        // values so the real LM-Studio / Ollama / OpenAI configs actually
        // take effect.
        let src = r#"
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:1234/v1"
model = "qwen/qwen3.5-9b"
api_key_env = ""
timeout_seconds = 120
max_retries = 2
structured_output = true
vision = false
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();

        // Provider selection + endpoint come from the supplied values, NOT the
        // defaults.
        assert_eq!(cfg.llm.provider, "openai-compatible");
        let sec = &cfg.providers.openai_compatible;
        assert_eq!(sec.base_url, "http://localhost:1234/v1");
        assert_eq!(sec.model, "qwen/qwen3.5-9b");
        assert_eq!(sec.timeout_seconds, 120);
        assert_eq!(sec.max_retries, 2);
        // `structured_output = true` must map to JSON-object mode (historical
        // semantics unchanged).
        assert_eq!(
            sec.structured_output,
            crate::config::StructuredOutputConfig::JsonObject
        );
        assert!(sec.structured_output.supports_structured());
        assert!(!sec.vision);

        // Keyless case: `api_key_env = ""` must be treated as "no API key" —
        // i.e. it must never request `OPENAI_API_KEY`. Per the existing
        // configuration semantics, an empty value is keyless (the provider
        // sends no Authorization header). We assert the environment-variable
        // name is blank so no key is required, while deliberately NOT changing
        // that semantics (no `Some("") -> None` normalization is introduced).
        assert_eq!(sec.api_key_env.as_deref().map(str::trim), Some(""));
    }

    #[test]
    fn providers_openai_compatible_defaults_survive_without_section() {
        // When the section is absent entirely, defaults must remain (backward
        // compatibility): an empty `[providers]` table still resolves to the
        // default OpenAI-compatible endpoint.
        let src = r#"
[llm]
provider = "openai-compatible"
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        let sec = &cfg.providers.openai_compatible;
        assert_eq!(sec.model, "gpt-4o-mini");
        assert_eq!(sec.base_url, "https://api.openai.com/v1");
        assert_eq!(
            sec.api_key_env.as_deref(),
            Some("OPENAI_API_KEY"),
            "default must remain key-bearing to preserve existing behavior"
        );
    }

    #[test]
    fn structured_output_accepts_json_schema_string() {
        // The documented opt-in for JSON Schema transport mode. This constrains
        // the JSON *transport* shape only; scientific validity is still enforced
        // by reviewer-side domain validation (not by structured output).
        let src = r#"
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:1234/v1"
model = "qwen/qwen3.5-9b"
api_key_env = ""
structured_output = "json_schema"
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        let sec = &cfg.providers.openai_compatible;
        assert_eq!(
            sec.structured_output,
            crate::config::StructuredOutputConfig::JsonSchema
        );
        assert!(sec.structured_output.supports_structured());
        // The provider-level mode that will be used.
        assert_eq!(
            sec.structured_output.to_mode(),
            paper_guard_llm::StructuredOutputMode::JsonSchema
        );
    }

    #[test]
    fn structured_output_accepts_off_string_and_false() {
        // `structured_output = false` means free-form at the transport layer
        // (reviewer-side validation still enforces JSON). The string `"off"`
        // is equivalent.
        let src = r#"
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
structured_output = false
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        assert_eq!(
            cfg.providers.openai_compatible.structured_output,
            crate::config::StructuredOutputConfig::Off
        );
        assert!(!cfg
            .providers
            .openai_compatible
            .structured_output
            .supports_structured());

        let src2 = r#"
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
structured_output = "off"
"#;
        let cfg2: AppConfig = toml::from_str(src2).unwrap();
        assert_eq!(
            cfg2.providers.openai_compatible.structured_output,
            crate::config::StructuredOutputConfig::Off
        );
    }

    #[test]
    fn structured_output_rejects_unknown_string() {
        let src = r#"
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
structured_output = "bogus"
"#;
        assert!(toml::from_str::<AppConfig>(src).is_err());
    }

    #[test]
    fn discovery_config_defaults_to_disabled() {
        let cfg = AppConfig::default();
        // Discovery is off by default so the client never probes the network
        // implicitly.
        assert!(!cfg.discovery.enabled);
        assert_eq!(cfg.discovery.mode, "off");
        assert_eq!(cfg.discovery.timeout_ms, 3000);
        assert_eq!(cfg.discovery.service_type, "_paper-guard._tcp.local.");
        assert_eq!(cfg.discovery.preferred_service, "");
    }

    #[test]
    fn discovery_config_roundtrips() {
        let src = r#"
[discovery]
enabled = true
mode = "manual"
timeout_ms = 5000
service_type = "_paper-guard._tcp.local."
preferred_service = "paper-guard.lab.local"
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        assert!(cfg.discovery.enabled);
        assert_eq!(cfg.discovery.mode, "manual");
        assert_eq!(cfg.discovery.timeout_ms, 5000);
        assert_eq!(cfg.discovery.preferred_service, "paper-guard.lab.local");
    }

    #[test]
    fn review_style_defaults_to_neutral() {
        // Without any `[review]` section the style must default to `neutral`.
        let cfg = AppConfig::default();
        assert_eq!(cfg.review.style, "neutral");
    }

    #[test]
    fn review_style_parses_from_config() {
        let src = r#"
[review]
style = "funny"
"#;
        let cfg: AppConfig = toml::from_str(src).unwrap();
        assert_eq!(cfg.review.style, "funny");
    }

    #[test]
    fn review_style_config_roundtrips() {
        let cfg = AppConfig::default();
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.review.style, "neutral");
    }
}

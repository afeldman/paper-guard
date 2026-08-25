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
    pub judge: JudgeConfig,
    pub revision: RevisionConfig,
    pub reproducibility: ReproducibilityConfig,
    pub service: ServiceConfig,
    pub memory: MemoryConfig,
    pub server: ServerConfig,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ProvidersConfig {
    pub openai_compatible: OpenAICompatibleSectionConfig,
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
    /// Whether the endpoint supports structured JSON output.
    pub structured_output: bool,
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
            structured_output: true,
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
}

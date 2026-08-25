//! Configuration (paper-guard.toml).

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
/// endpoint (OpenAI, Mammoth.ai, a local server, etc.). The difference between
/// backends is *configuration only* — never code. Secrets (the API key) are
/// never stored here; only the name of the environment variable that holds
/// them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAICompatibleSectionConfig {
    /// Base URL of the OpenAI-compatible endpoint, e.g. `https://api.openai.com/v1`.
    pub base_url: String,
    /// Environment variable holding the API key, e.g. `OPENAI_API_KEY`.
    /// When absent, requests are sent without an Authorization header (for
    /// local endpoints that need no key).
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

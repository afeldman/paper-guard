//! Deterministic mock LLM provider for offline testing and fixtures.
//!
//! The mock does **not** invent facts. It only re-emits findings and revisions
//! that are explicitly configured in a [`MockScenario`]. If no scenario matches,
//! it returns a neutral empty response rather than fabricating content.

use std::collections::HashMap;
use std::sync::RwLock;

use paper_guard_core::ContentHash;
use paper_guard_core::SCHEMA_VERSION;

use crate::{LlmRequest, LlmResponse, LlmUsage, ModelConfig, ProviderKind};

/// A single mocked outcome matched against request content.
#[derive(Debug, Clone)]
pub struct MockOutcome {
    /// A keyword present in the user text that triggers this outcome.
    pub trigger: String,
    /// The JSON text to return.
    pub text: String,
}

/// A scripted scenario: a named set of trigger->outcome pairs plus a fallback.
#[derive(Debug, Clone, Default)]
pub struct MockLlmScenario {
    pub name: String,
    pub outcomes: Vec<MockOutcome>,
    /// Fallback JSON text when no trigger matches.
    pub fallback: String,
}

impl MockLlmScenario {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            outcomes: Vec::new(),
            fallback: String::new(),
        }
    }

    /// Add a trigger->outcome pair.
    pub fn on(mut self, trigger: impl Into<String>, text: impl Into<String>) -> Self {
        self.outcomes.push(MockOutcome {
            trigger: trigger.into(),
            text: text.into(),
        });
        self
    }

    /// Set the fallback JSON.
    pub fn fallback(mut self, fallback: impl Into<String>) -> Self {
        self.fallback = fallback.into();
        self
    }
}

/// A short description of a request (used by tests to introspect the mock).
#[derive(Debug, Clone)]
pub struct MockLlmRequest {
    pub text: String,
}

impl MockLlmRequest {
    pub fn from_request(req: &LlmRequest) -> Self {
        MockLlmRequest {
            text: req.user_text(),
        }
    }
}

/// A factory that produces mock providers keyed by model name.
#[derive(Default)]
pub struct MockLlmFactory {
    scenarios: RwLock<HashMap<String, MockLlmScenario>>,
    default: MockLlmScenario,
}

impl MockLlmFactory {
    pub fn new() -> Self {
        Self {
            scenarios: RwLock::new(HashMap::new()),
            default: MockLlmScenario {
                name: "default".into(),
                outcomes: Vec::new(),
                fallback: "[]".into(),
            },
        }
    }

    /// Register a scenario for a given model name.
    pub fn register(&self, model: &str, scenario: MockLlmScenario) {
        self.scenarios
            .write()
            .unwrap()
            .insert(model.to_string(), scenario);
    }

    /// Build a mock provider for the given model configuration.
    pub fn provider(&self, model_config: &ModelConfig) -> MockProvider {
        let scenario = self
            .scenarios
            .read()
            .unwrap()
            .get(&model_config.model)
            .cloned()
            .unwrap_or_else(|| self.default.clone());
        MockProvider {
            scenario,
            model: model_config.model.clone(),
        }
    }
}

/// A deterministic provider returning scripted responses.
#[derive(Debug, Clone)]
pub struct MockProvider {
    scenario: MockLlmScenario,
    model: String,
}

impl MockProvider {
    pub fn new(model: impl Into<String>, scenario: MockLlmScenario) -> Self {
        MockProvider {
            model: model.into(),
            scenario,
        }
    }

    /// The current scenario (for assertions in tests).
    pub fn scenario(&self) -> &MockLlmScenario {
        &self.scenario
    }

    /// The model name this provider serves.
    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait::async_trait]
impl crate::LlmProvider for MockProvider {
    async fn generate(&self, request: LlmRequest) -> anyhow::Result<LlmResponse> {
        // Deterministic: identical input + scenario => identical output.
        let text = request.user_text();
        let matched = self
            .scenario
            .outcomes
            .iter()
            .find(|o| text.contains(&o.trigger))
            .map(|o| o.text.clone())
            .unwrap_or_else(|| self.scenario.fallback.clone());

        let response = LlmResponse {
            text: matched,
            usage: Some(LlmUsage {
                prompt_tokens: text.len() as u32,
                completion_tokens: 0,
            }),
        };
        // Reproducibility: the request's content hash was already committed to
        // the request object before it reached the provider.
        Ok(response)
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Mock
    }
}

/// Utility to prepend a schema version header to a parsed JSON array payload.
pub fn with_schema(payload: &str) -> String {
    format!(
        r#"{{"schema_version":"{SCH}", "payload": {payload}}}"#,
        SCH = SCHEMA_VERSION
    )
}

/// Computes a content hash of a slice of bytes.
pub fn hash_bytes(bytes: &[u8]) -> ContentHash {
    ContentHash::of_bytes(bytes)
}

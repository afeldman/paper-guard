//! The reviewer abstraction and the five concrete reviewers.

use paper_guard_core::Document;

use crate::output::ReviewerOutput;
use crate::schema::ReviewerKind;

/// Configuration for a reviewer instance.
#[derive(Debug, Clone)]
pub struct ReviewerSettings {
    /// Whether this reviewer is enabled.
    pub enabled: bool,
    /// Provider kind (openai, anthropic, local, mock, ...).
    pub provider: String,
    /// Model name.
    pub model: String,
    /// Seed for reproducibility, if supported.
    pub seed: Option<u64>,
    /// Temperature.
    pub temperature: f32,
}

impl ReviewerSettings {
    /// Default reviewer settings with a given model.
    pub fn default_with_model(_kind: ReviewerKind, model: &str) -> Self {
        let provider = if model == "mock" { "mock" } else { "openai" };
        ReviewerSettings {
            enabled: true,
            provider: provider.to_string(),
            model: model.to_string(),
            seed: None,
            temperature: 0.0,
        }
    }

    /// A disabled reviewer (used in tests to simulate a failed agent).
    pub fn disabled() -> Self {
        ReviewerSettings {
            enabled: false,
            provider: "none".into(),
            model: "none".into(),
            seed: None,
            temperature: 0.0,
        }
    }
}

/// The context a reviewer receives: the document plus its LLM assignment.
#[derive(Debug, Clone)]
pub struct ReviewerContext {
    /// The canonical document.
    pub document: Document,
    /// The prompt version (for reproducibility).
    pub prompt_version: String,
    /// The run id the review belongs to.
    pub run_id: String,
}

/// The reviewer trait. Each reviewer examines the document and returns
/// findings. It must never invent evidence — findings must reference artifacts
/// that exist in the document or be flagged as unverified.
#[async_trait::async_trait]
pub trait Reviewer: Send + Sync {
    /// The reviewer kind.
    fn kind(&self) -> ReviewerKind;

    /// The settings used to construct an LLM request.
    fn settings(&self) -> &ReviewerSettings;

    /// Build the system prompt for this reviewer.
    fn system_prompt(&self) -> String;

    /// Build the user prompt from the context.
    fn user_prompt(&self, ctx: &ReviewerContext) -> String;

    /// Whether this reviewer needs image content (multimodal).
    fn wants_images(&self) -> bool {
        false
    }

    /// Produce the reviewer output for the given context, provider, and raw
    /// response. The default implementation builds a request, calls the
    /// provider, then parses findings — reviewers can override for special
    /// multimodal behavior.
    async fn run(
        &self,
        ctx: &ReviewerContext,
        provider: &dyn paper_guard_llm::LlmProvider,
    ) -> anyhow::Result<ReviewerOutput> {
        let mut request = paper_guard_llm::LlmRequest::new(
            self.settings().model.clone(),
            self.system_prompt(),
            self.user_prompt(ctx),
            ctx.prompt_version.clone(),
        );
        if let Some(seed) = self.settings().seed {
            request = request.with_seed(seed);
        }
        request = request.with_temperature(self.settings().temperature);
        if self.wants_images() {
            for fig in &ctx.document.figures {
                if let Some(asset) = &fig.asset {
                    if let Ok(bytes) = std::fs::read(asset) {
                        let image = paper_guard_llm::LlmImage {
                            media_type: "image/png".into(),
                            base64: base64_encode(&bytes),
                        };
                        request = request.with_image(image);
                    }
                }
            }
        }
        let hash = request.request_hash.clone();
        let response = provider.generate(request).await?;
        let usage = response.usage;
        let text = response.text.clone();
        ReviewerOutput::from_raw(self.kind().name(), text.clone(), Some(hash.0))
            .with_usage(usage)
            .parse_findings(&text)
    }
}

/// Minimal base64 encoding (no external dependency).
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Build a compact text rendering of a document for a prompt.
pub fn render_document_for_prompt(doc: &Document, max_chars: usize) -> String {
    let mut out = String::new();
    if let Some(title) = &doc.meta.title {
        out.push_str(&format!("# {title}\n\n"));
    }
    for sec in &doc.sections {
        out.push_str(&format!("\n## {}\n", sec.title));
        for p in &sec.paragraphs {
            out.push_str(&p.text);
            out.push('\n');
        }
    }
    if !doc.bibliography.is_empty() {
        out.push_str("\n## References\n");
        for r in &doc.bibliography {
            out.push_str(&format!(
                "{} | {} | {} | {}\n",
                r.reference_id,
                r.authors,
                r.year.map(|y| y.to_string()).unwrap_or_else(|| "?".into()),
                r.title
            ));
        }
    }
    for claim in &doc.claims {
        out.push_str(&format!("\n[CLAIM {}] {}\n", claim.claim_id, claim.text));
    }
    if doc.claims.is_empty() {
        out.push_str("\n[NOTE] No claims were auto-extracted; treat as INSUFFICIENT_EVIDENCE.\n");
    }
    if out.len() > max_chars {
        out.truncate(max_chars);
    }
    out
}

// ---------------------------------------------------------------------------
// Concrete reviewers
// ---------------------------------------------------------------------------

/// The Scientific Reviewer checks argumentation, methodology, consistency,
/// interpretation, limitations, and reproducibility.
pub struct ScientificReviewer {
    pub settings: ReviewerSettings,
}

/// The Adversarial / Red-Team Reviewer hunts for the strongest attack a real
/// reviewer could mount.
pub struct AdversarialReviewer {
    pub settings: ReviewerSettings,
}

/// The Evidence / Claim Checker audits Claim -> Evidence -> Result and rejects
/// fabricated support.
pub struct EvidenceReviewer {
    pub settings: ReviewerSettings,
}

/// The Reference Checker audits bibliographic integrity; references it cannot
/// verify are tagged `NOT_VERIFIED` rather than asserted to exist.
pub struct ReferenceReviewer {
    pub settings: ReviewerSettings,
}

/// The Figure / Table Reviewer audits captions, readability, and numeric
/// consistency, and can use multimodal models.
pub struct FigureReviewer {
    pub settings: ReviewerSettings,
}

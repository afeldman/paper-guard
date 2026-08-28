//! The canonical paper model.
//!
//! Reviewers never operate on raw PDF text fragments; they operate on this
//! normalized, structured representation of a document. The model captures the
//! relationships the evidence checker relies on:
//!
//! ```text
//! Claim
//!   ↓
//! Evidence
//!   ↓
//! Result
//!   ↓
//! Figure / Table
//!   ↓
//! Reference
//! ```

use serde::{Deserialize, Serialize};

use crate::ContentHash;

/// Source-level provenance for a block of extracted content.
///
/// This is deliberately distinct from [`crate::Provenance`] (which records
/// whether content is author-authored or system-produced). This type records
/// *where* content physically came from — the source file (for a LaTeX project
/// or a single manuscript) and the offset within it — so that a finding can be
/// traced back to its origin in the manuscript.
///
/// The fields are all optional / defaulted so that legacy JSON artifacts that
/// lack provenance still deserialize cleanly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceLocation {
    /// The source format that produced this content: `latex`, `pdf`, `docx`,
    /// etc.
    pub source_type: String,
    /// The resolved source file path (relative to the project root for a LaTeX
    /// project, otherwise the supplied manuscript path). E.g.
    /// `sections/methods.tex` or `paper.pdf`.
    pub file: String,
    /// For an included LaTeX file, the path of the file that included it.
    pub include_parent: Option<String>,
    /// Include depth in the LaTeX project tree (0 for the root file, 1 for a
    /// direct include, etc.).
    pub include_depth: u32,
    /// The 1-based line number within `file` where this content starts.
    pub start_line: Option<u32>,
    /// The 1-based line number within `file` where this content ends.
    pub end_line: Option<u32>,
    /// For a PDF, the 1-based page number this content occurred on.
    pub page: Option<u32>,
}

impl SourceLocation {
    /// A compact, human-readable rendering of the location, e.g.
    /// `sections/methods.tex, line 42` or `paper.pdf, page 7`.
    pub fn display(&self) -> String {
        if let Some(page) = self.page {
            format!("{}, page {}", self.file, page)
        } else if let Some(start) = self.start_line {
            match self.end_line {
                Some(end) if end > start => {
                    format!("{}, lines {}-{}", self.file, start, end)
                }
                _ => format!("{}, line {}", self.file, start),
            }
        } else {
            self.file.clone()
        }
    }

    /// Whether this location carries any hyper-specific pointer (line or page).
    pub fn has_pointer(&self) -> bool {
        self.start_line.is_some() || self.page.is_some()
    }
}

/// Newtype for structured claim identifiers (e.g. `C17`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClaimId(pub String);

impl std::fmt::Display for ClaimId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype for paragraph identifiers (e.g. `section_4.paragraph_12`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParagraphId(pub String);

impl std::fmt::Display for ParagraphId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype for section identifiers (e.g. `section_4`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SectionId(pub String);

impl std::fmt::Display for SectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype for reference identifiers (e.g. `R12`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReferenceId(pub String);

impl std::fmt::Display for ReferenceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A reference to a specific evidence artifact in the paper.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRef {
    /// A figure artifact.
    Figure(String),
    /// A table artifact.
    Table(String),
    /// A result artifact.
    Result(String),
    /// An external source.
    Reference(ReferenceId),
    /// A method artifact.
    Method(String),
    /// A dataset (external or described in the paper).
    Dataset(String),
}

/// A reference to a table artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TableRef(pub String);

/// The structural kind of a claim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    /// A universal/general claim.
    General,
    /// A comparative claim (X outperforms Y).
    Comparative,
    /// A statistical claim (significance, effect size).
    Statistical,
    /// A causal claim.
    Causal,
    /// A methodological claim.
    Methodological,
    /// A claim about a specific result.
    Result,
    /// A definitional claim.
    Definitional,
    /// A claim that is currently undetermined automatically.
    Unspecified,
}

/// A claim extracted from the paper.
///
/// Claims are the central unit that the evidence checker audits. Every claim is
/// uniquely referenceable via [`ClaimId`] and carries locations and links to
/// evidence, results, and citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub claim_id: ClaimId,
    /// Where in the document the claim occurs (e.g. `section_4.paragraph_12`).
    pub location: ParagraphId,
    /// The claim text.
    pub text: String,
    #[serde(rename = "type")]
    pub claim_type: ClaimType,
    /// Estimated confidence in the claim extraction (0..=1).
    pub confidence: f32,
    /// Evidence artifacts referenced by this claim.
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    /// Result artifacts referenced by this claim.
    #[serde(default)]
    pub result_refs: Vec<String>,
    /// Citation references used to back this claim.
    #[serde(default)]
    pub citation_refs: Vec<ReferenceId>,
}

impl Claim {
    /// A compact one-line summary for logs and UI.
    pub fn summary(&self) -> String {
        let mut out = format!("{} @{}: {}", self.claim_id, self.location, self.text);
        if !self.evidence_refs.is_empty() {
            out.push_str(&format!(" [evidence: {}]", self.evidence_refs.len()));
        }
        out
    }
}

/// An evidence object. Evidences point at the concrete artifacts (data,
/// methods, tables, figures, external sources) underpinning claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub evidence_id: String,
    /// The claim this evidence supports.
    pub claim_id: ClaimId,
    /// Where in the document this evidence lives.
    pub location: ParagraphId,
    /// A short description of the evidence.
    pub description: String,
    /// Concrete artifacts this evidence refers to.
    #[serde(default)]
    pub refs: Vec<EvidenceRef>,
    /// Verification state of the underlying artifacts.
    #[serde(default)]
    pub support_state: crate::EvidenceState,
}

/// A result artifact (an experiment output the paper reports).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result_ {
    pub result_id: String,
    /// Where the result is described.
    pub location: ParagraphId,
    /// A short description of the result.
    pub description: String,
    /// Underlying figures/tables if any.
    #[serde(default)]
    pub artifacts: Vec<String>,
}

/// A method artifact (experiment/procedure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Method {
    pub method_id: String,
    pub location: ParagraphId,
    pub name: String,
    pub description: String,
}

/// A figure artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub figure_id: String,
    /// The caption text.
    pub caption: String,
    /// Where in the text the figure is referenced.
    pub location: ParagraphId,
    /// Path/identifier of the underlying image asset, if any.
    #[serde(default)]
    pub asset: Option<String>,
}

/// A table artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub table_id: String,
    pub caption: String,
    pub location: ParagraphId,
    /// Rows as strings; used for structural validation and number checks.
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
}

/// An equation artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equation {
    pub equation_id: String,
    pub location: ParagraphId,
    pub latex: String,
    pub number: Option<String>,
}

/// A reference in the bibliography.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub reference_id: ReferenceId,
    pub authors: String,
    pub year: Option<u32>,
    pub title: String,
    pub venue: String,
    /// Verification state (only asserted as verified when an authoritative
    /// source confirmed it; otherwise `NotVerified`).
    #[serde(default)]
    pub verification: crate::EvidenceState,
}

/// A citation appearing in the running text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub citation_id: String,
    pub location: ParagraphId,
    /// The reference(s) the citation points to.
    #[serde(default)]
    pub refs: Vec<ReferenceId>,
}

/// A paragraph of running text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub id: ParagraphId,
    pub text: String,
    /// Where this paragraph physically came from (source file, line, page).
    /// Absent for legacy artifacts that predate provenance tracking.
    #[serde(default)]
    pub location: Option<SourceLocation>,
}

/// A section, containing paragraphs and floats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub title: String,
    #[serde(default)]
    pub paragraphs: Vec<Paragraph>,
    /// Where this section's heading physically came from (source file, line,
    /// page). Absent for legacy artifacts that predate provenance tracking.
    #[serde(default)]
    pub location: Option<SourceLocation>,
}

/// Document-level metadata captured by the parser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMeta {
    pub title: Option<String>,
    pub authors: Vec<String>,
    #[serde(default)]
    pub abstract_text: Option<String>,
    pub source_format: String,
    pub source_file: String,
}

/// A parsed, canonical document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Stable document identifier (usually a short hash of the source).
    pub document_id: String,
    pub meta: DocumentMeta,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub bibliography: Vec<Reference>,
    #[serde(default)]
    pub citations: Vec<Citation>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub results: Vec<Result_>,
    #[serde(default)]
    pub methods: Vec<Method>,
    #[serde(default)]
    pub figures: Vec<Figure>,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub equations: Vec<Equation>,
    /// Hash of the original source bytes.
    pub source_hash: ContentHash,
}

impl Document {
    /// Look up a claim by id.
    pub fn claim(&self, id: &ClaimId) -> Option<&Claim> {
        self.claims.iter().find(|c| &c.claim_id == id)
    }

    /// Look up a reference by id.
    pub fn reference(&self, id: &ReferenceId) -> Option<&Reference> {
        self.bibliography.iter().find(|r| &r.reference_id == id)
    }

    /// Number of claims in the document.
    pub fn claim_count(&self) -> usize {
        self.claims.len()
    }
}

/// A builder that incrementally assembles a [`Document`] with consistent
/// default IDs.
#[derive(Debug, Clone, Default)]
pub struct CanonicalDocumentBuilder {
    source_format: Option<String>,
    source_file: Option<String>,
    title: Option<String>,
    authors: Vec<String>,
    abstract_text: Option<String>,
    sections: Vec<Section>,
    bibliography: Vec<Reference>,
    citations: Vec<Citation>,
    claims: Vec<Claim>,
    evidence: Vec<Evidence>,
    results: Vec<Result_>,
    methods: Vec<Method>,
    figures: Vec<Figure>,
    tables: Vec<Table>,
    equations: Vec<Equation>,
}

impl CanonicalDocumentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn source(mut self, format: impl Into<String>, file: impl Into<String>) -> Self {
        self.source_format = Some(format.into());
        self.source_file = Some(file.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.authors.push(author.into());
        self
    }

    pub fn abstract_text(mut self, text: impl Into<String>) -> Self {
        self.abstract_text = Some(text.into());
        self
    }

    pub fn section(mut self, section: Section) -> Self {
        self.sections.push(section);
        self
    }

    pub fn reference(mut self, reference: Reference) -> Self {
        self.bibliography.push(reference);
        self
    }

    pub fn citation(mut self, citation: Citation) -> Self {
        self.citations.push(citation);
        self
    }

    pub fn claim(mut self, claim: Claim) -> Self {
        self.claims.push(claim);
        self
    }

    pub fn evidence(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub fn result(mut self, result: Result_) -> Self {
        self.results.push(result);
        self
    }

    pub fn method(mut self, method: Method) -> Self {
        self.methods.push(method);
        self
    }

    pub fn figure(mut self, figure: Figure) -> Self {
        self.figures.push(figure);
        self
    }

    pub fn table(mut self, table: Table) -> Self {
        self.tables.push(table);
        self
    }

    pub fn equation(mut self, equation: Equation) -> Self {
        self.equations.push(equation);
        self
    }

    /// Build the document, computing a stable `document_id` from the content
    /// hash of the serialized model.
    pub fn build(self) -> Document {
        let format = self.source_format.unwrap_or_default();
        let file = self.source_file.unwrap_or_default();
        let mut doc = Document {
            document_id: String::new(),
            meta: DocumentMeta {
                title: self.title,
                authors: self.authors,
                abstract_text: self.abstract_text,
                source_format: format.clone(),
                source_file: file,
            },
            sections: self.sections,
            bibliography: self.bibliography,
            citations: self.citations,
            claims: self.claims,
            evidence: self.evidence,
            results: self.results,
            methods: self.methods,
            figures: self.figures,
            tables: self.tables,
            equations: self.equations,
            source_hash: ContentHash(
                "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            ),
        };
        let model_hash = ContentHash::compute(&doc);
        doc.document_id = format!("doc-{}", &model_hash.0[..16]);
        doc.source_hash = model_hash;
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_claim() -> Claim {
        Claim {
            claim_id: ClaimId("C17".into()),
            location: ParagraphId("section_4.paragraph_12".into()),
            text: "The proposed method reduces latency by 40%.".into(),
            claim_type: ClaimType::Result,
            confidence: 0.9,
            evidence_refs: vec![EvidenceRef::Figure("F6".into())],
            result_refs: vec!["R12".into()],
            citation_refs: vec![ReferenceId("R3".into())],
        }
    }

    #[test]
    fn claim_builds_and_lookup_works() {
        let doc = CanonicalDocumentBuilder::new()
            .source("latex", "main.tex")
            .title("A Paper")
            .claim(sample_claim())
            .build();
        assert_eq!(
            doc.claim(&ClaimId("C17".into())).unwrap().text,
            "The proposed method reduces latency by 40%."
        );
        assert_eq!(doc.claim_count(), 1);
        assert!(doc.document_id.starts_with("doc-"));
    }

    #[test]
    fn document_id_is_stable() {
        let a = CanonicalDocumentBuilder::new().title("X").build();
        let b = CanonicalDocumentBuilder::new().title("X").build();
        assert_eq!(a.document_id, b.document_id);
        let c = CanonicalDocumentBuilder::new().title("Y").build();
        assert_ne!(a.document_id, c.document_id);
    }

    #[test]
    fn reference_lookup() {
        let r = Reference {
            reference_id: ReferenceId("R12".into()),
            authors: "Smith, J.".into(),
            year: Some(2020),
            title: "A Study".into(),
            venue: "Journal".into(),
            verification: crate::EvidenceState::NotVerified,
        };
        let doc = CanonicalDocumentBuilder::new().reference(r).build();
        assert!(doc.reference(&ReferenceId("R12".into())).is_some());
        assert!(doc.reference(&ReferenceId("R99".into())).is_none());
    }
}

//! # Paper Guard Parser
//!
//! Parses source documents into the canonical [`paper_guard_core::Document`]
//! model. A `Parser` trait decouples the pipeline from any single input format.
//!
//! The first concrete parser handles LaTeX. PDF, Typst, and DOCX are declared
//! as supported source formats but are not yet fully implemented; they return a
//! clear unimplemented error rather than producing a wrong model.

mod latex;
pub mod latex_project;
pub mod pdf;

pub use latex::{parse_latex, LatexParser};
pub use latex_project::{
    parse_latex_project, resolve_latex_project, LatexFragment, ResolvedLatexProject,
};
pub use pdf::{parse_pdf, PdfParser};

use paper_guard_core::{ContentHash, Document};

/// Source formats Paper Guard aims to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Pdf,
    Latex,
    Typst,
    Docx,
    /// A directory of source files (manuscript folder).
    SourceDir,
}

impl std::str::FromStr for SourceFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pdf" => Ok(SourceFormat::Pdf),
            "latex" | "tex" => Ok(SourceFormat::Latex),
            "typst" => Ok(SourceFormat::Typst),
            "docx" => Ok(SourceFormat::Docx),
            "dir" | "source_dir" | "manuscript" => Ok(SourceFormat::SourceDir),
            other => Err(format!("unknown source format: {other}")),
        }
    }
}

impl std::fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SourceFormat::Pdf => "pdf",
                SourceFormat::Latex => "latex",
                SourceFormat::Typst => "typst",
                SourceFormat::Docx => "docx",
                SourceFormat::SourceDir => "source_dir",
            }
        )
    }
}

/// A parsed source, including the canonical model and a hash of the raw bytes.
#[derive(Debug, Clone)]
pub struct ParsedSource {
    pub format: SourceFormat,
    pub source_file: String,
    pub raw_bytes: Vec<u8>,
    pub source_hash: ContentHash,
    pub document: Document,
}

impl ParsedSource {
    /// Create a parsed source. The canonical document is used as-is and its
    /// source hash is recomputed from the raw bytes.
    pub fn new(format: SourceFormat, source_file: String, raw_bytes: Vec<u8>) -> Self {
        let source_hash = ContentHash::of_bytes(&raw_bytes);
        ParsedSource {
            format,
            source_hash: source_hash.clone(),
            source_file: source_file.clone(),
            raw_bytes: raw_bytes.clone(),
            document: Document {
                document_id: String::new(),
                meta: paper_guard_core::DocumentMeta {
                    title: None,
                    authors: Vec::new(),
                    abstract_text: None,
                    source_format: format.to_string(),
                    source_file,
                },
                sections: Vec::new(),
                bibliography: Vec::new(),
                citations: Vec::new(),
                claims: Vec::new(),
                evidence: Vec::new(),
                results: Vec::new(),
                methods: Vec::new(),
                figures: Vec::new(),
                tables: Vec::new(),
                equations: Vec::new(),
                source_hash,
            },
        }
    }

    /// A second-phase constructor wrapping an already-built canonical document.
    pub fn with_document(
        format: SourceFormat,
        source_file: String,
        raw_bytes: Vec<u8>,
        mut document: Document,
    ) -> Self {
        let source_hash = ContentHash::of_bytes(&raw_bytes);
        document.source_hash = source_hash.clone();
        document.meta.source_format = format.to_string();
        document.meta.source_file = source_file.clone();
        ParsedSource {
            format,
            source_file,
            raw_bytes,
            source_hash,
            document,
        }
    }
}

/// A parser for a source format.
#[async_trait::async_trait]
pub trait Parser: Send + Sync {
    /// The format this parser handles.
    fn format(&self) -> SourceFormat;

    /// Parse the given source bytes into a [`ParsedSource`].
    async fn parse(&self, source_file: &str, bytes: &[u8]) -> anyhow::Result<ParsedSource>;
}

/// Select a parser for a source format / file path.
pub fn parser_for_format(format: SourceFormat) -> anyhow::Result<Box<dyn Parser>> {
    match format {
        SourceFormat::Latex => Ok(Box::new(LatexParser)),
        SourceFormat::Pdf => Ok(Box::new(PdfParser)),
        SourceFormat::Typst => Err(anyhow::anyhow!(
            "Typst parsing is not yet implemented in this version."
        )),
        SourceFormat::Docx => Err(anyhow::anyhow!(
            "DOCX parsing is not yet implemented in this version."
        )),
        SourceFormat::SourceDir => Err(anyhow::anyhow!(
            "Directory parsing is not yet implemented in this version."
        )),
    }
}

/// Sniff the source format from a file extension.
pub fn format_from_extension(path: &str) -> SourceFormat {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".tex") {
        SourceFormat::Latex
    } else if lower.ends_with(".pdf") {
        SourceFormat::Pdf
    } else if lower.ends_with(".typ") {
        SourceFormat::Typst
    } else if lower.ends_with(".docx") {
        SourceFormat::Docx
    } else {
        SourceFormat::Latex
    }
}

// ---------------------------------------------------------------------------
// DocumentSource — provider-independent, path-aware parsing
// ---------------------------------------------------------------------------

/// A parsed source plus enough provenance to explain *where* content came from.
///
/// This is the single entry point the pipeline uses so that LaTeX projects,
/// single `.tex` files, and PDFs all converge on the same canonical model.
#[derive(Debug, Clone)]
pub struct DocumentSource {
    pub parsed: ParsedSource,
    /// For a LaTeX project, the list of resolved files (relative to the root).
    pub project_files: Vec<String>,
    /// Missing includes (non-fatal structural diagnostics).
    pub missing_includes: Vec<String>,
    /// Include cycles detected (non-fatal structural diagnostics).
    pub include_cycles: Vec<String>,
}

impl DocumentSource {
    /// The resolved source format.
    pub fn format(&self) -> SourceFormat {
        self.parsed.format
    }

    /// Number of distinct source files backing this document.
    pub fn file_count(&self) -> usize {
        if self.project_files.is_empty() {
            1
        } else {
            self.project_files.len()
        }
    }

    /// Whether this is a resolved multi-file LaTeX project.
    pub fn is_project(&self) -> bool {
        !self.project_files.is_empty()
    }
}

/// Parse a file *path* into a [`DocumentSource`], auto-detecting whether a
/// `.tex` file is a single manuscript or the root of a `\input`/`\include`
/// project, and dispatching PDFs to the PDF parser.
///
/// * A `.pdf` always uses the PDF parser.
/// * A `.tex` without any resolvable `\input`/`\include` is parsed as a single
///   file. If it *does* contain includes, it is parsed as a project.
/// * Structural diagnostics (missing includes, cycles) are surfaced on the
///   returned [`DocumentSource`] rather than failing the whole parse, so a
///   researcher still gets a review of the readable content.
pub async fn parse_source_path(source_path: &str) -> anyhow::Result<DocumentSource> {
    let format = format_from_extension(source_path);
    match format {
        SourceFormat::Pdf => {
            let bytes = std::fs::read(source_path)?;
            let parsed = PdfParser.parse(source_path, &bytes).await?;
            Ok(DocumentSource {
                parsed,
                project_files: Vec::new(),
                missing_includes: Vec::new(),
                include_cycles: Vec::new(),
            })
        }
        SourceFormat::Latex => parse_latex_source_path(source_path).await,
        other => Err(anyhow::anyhow!(
            "unsupported source `{source_path}`: {other} is not yet supported in v1.0"
        )),
    }
}

/// Parse a `.tex` path, favouring project resolution when includes are present.
async fn parse_latex_source_path(source_path: &str) -> anyhow::Result<DocumentSource> {
    use std::path::Path;
    let path = Path::new(source_path);
    if !path.is_file() {
        return Err(anyhow::anyhow!(
            "source file `{source_path}` does not exist or is not readable"
        ));
    }
    // Peek: does the root file reference any include?
    let raw = std::fs::read(path)?;
    let root_text = String::from_utf8_lossy(&raw).into_owned();
    let has_include_marker = root_text.contains("\\input") || root_text.contains("\\include");

    if !has_include_marker {
        // Single-file manuscript.
        let parsed = LatexParser.parse(source_path, &raw).await?;
        return Ok(DocumentSource {
            parsed,
            project_files: Vec::new(),
            missing_includes: Vec::new(),
            include_cycles: Vec::new(),
        });
    }

    // Attempt project resolution.
    let project = crate::latex_project::resolve_latex_project(path).map_err(|e| {
        // Fall back to single-file if the root itself can't be a project.
        anyhow::anyhow!("unable to resolve LaTeX project at `{source_path}`: {e}")
    })?;

    let project_files = project
        .fragments
        .iter()
        .map(|f| f.rel_path.clone())
        .collect::<Vec<_>>();
    let missing = project.missing_includes.clone();
    let cycles = project.cycles.clone();

    // If nothing beyond the root resolved, treat it as a single file.
    if project.fragments.len() <= 1 {
        let parsed = LatexParser.parse(source_path, &raw).await?;
        return Ok(DocumentSource {
            parsed,
            project_files: Vec::new(),
            missing_includes: missing,
            include_cycles: cycles,
        });
    }

    let document = crate::latex_project::parse_latex_project(&project)?;
    let raw_bytes = raw;
    let parsed = ParsedSource::with_document(
        SourceFormat::Latex,
        source_path.to_string(),
        raw_bytes.clone(),
        document,
    );
    Ok(DocumentSource {
        parsed,
        project_files,
        missing_includes: missing,
        include_cycles: cycles,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_extension_works() {
        assert_eq!(format_from_extension("main.tex"), SourceFormat::Latex);
        assert_eq!(format_from_extension("paper.pdf"), SourceFormat::Pdf);
        assert_eq!(format_from_extension("doc.typ"), SourceFormat::Typst);
        assert_eq!(format_from_extension("d.docx"), SourceFormat::Docx);
    }

    #[test]
    fn parser_for_pdf_returns_pdf_parser() {
        assert!(parser_for_format(SourceFormat::Pdf).is_ok());
    }

    #[test]
    fn parser_for_unsupported_returns_clear_error() {
        match parser_for_format(SourceFormat::Typst) {
            Err(e) => assert!(e.to_string().contains("not yet implemented")),
            Ok(_) => panic!("expected an error for Typst"),
        }
    }
}

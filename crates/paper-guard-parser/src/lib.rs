//! # Paper Guard Parser
//!
//! Parses source documents into the canonical [`paper_guard_core::Document`]
//! model. A `Parser` trait decouples the pipeline from any single input format.
//!
//! The first concrete parser handles LaTeX. PDF, Typst, and DOCX are declared
//! as supported source formats but are not yet fully implemented; they return a
//! clear unimplemented error rather than producing a wrong model.

mod latex;

pub use latex::{parse_latex, LatexParser};

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
        SourceFormat::Pdf => Err(anyhow::anyhow!(
            "PDF parsing is not yet implemented in this version."
        )),
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
    fn parser_for_unsupported_returns_clear_error() {
        match parser_for_format(SourceFormat::Pdf) {
            Err(e) => assert!(e.to_string().contains("not yet implemented")),
            Ok(_) => panic!("expected an error for PDF"),
        }
    }
}

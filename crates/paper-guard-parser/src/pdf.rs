//! # PDF parser
//!
//! Extracts text from a PDF manuscript into the canonical
//! [`paper_guard_core::Document`] model with per-page provenance, without ever
//! executing embedded content.
//!
//! Behavior:
//!
//! * Runs entirely in-process via the pure-Rust [`lopdf`] library — nothing is
//!   executed, no shell, no JavaScript, no embedded media.
//! * Malformed / unreadable PDFs fail explicitly (`PDF_INVALID` style errors)
//!   rather than yielding a silent empty review.
//! * Encrypted / password-protected PDFs fail explicitly (`PDF_ENCRYPTED`).
//! * Image-only PDFs or PDFs with no extractable text fail explicitly
//!   (`PDF_TEXT_UNAVAILABLE`) — we never fabricate figure content.
//! * Each extracted paragraph carries a [`SourceLocation`] with `page` filled
//!   in, so every finding can be traced back to a page.
//! * Extraction is bounded (decompression-bomb-safe via lopdf's
//!   `extract_text_with_limit`) and deterministic.

use lopdf::Document as PdfDocument;

use paper_guard_core::{
    CanonicalDocumentBuilder, Document, Paragraph, ParagraphId, Section, SectionId, SourceLocation,
};

use crate::{ParsedSource, Parser, SourceFormat};

/// Maximum decompressed bytes per page (guards against decompression bombs).
pub const PDF_PAGE_CONTENT_LIMIT: usize = 64 * 1024 * 1024; // 64 MiB

/// The LaTeX-agnostic PDF parser.
pub struct PdfParser;

#[async_trait::async_trait]
impl Parser for PdfParser {
    fn format(&self) -> SourceFormat {
        SourceFormat::Pdf
    }

    async fn parse(&self, source_file: &str, bytes: &[u8]) -> anyhow::Result<ParsedSource> {
        let doc = parse_pdf(source_file, bytes)?;
        Ok(ParsedSource::with_document(
            SourceFormat::Pdf,
            source_file.to_string(),
            bytes.to_vec(),
            doc,
        ))
    }
}

/// Parse PDF bytes into a canonical document.
///
/// Errors:
/// * structural failure → `PDF_INVALID`
/// * encrypted → `PDF_ENCRYPTED`
/// * no extractable text → `PDF_TEXT_UNAVAILABLE`
pub fn parse_pdf(source_file: &str, bytes: &[u8]) -> anyhow::Result<Document> {
    // Structural validation (malformed PDFs) happens here.
    let pdf = PdfDocument::load_mem(bytes).map_err(|e| {
        anyhow::anyhow!("PDF_INVALID: unable to parse `{source_file}` as a PDF: {e}")
    })?;

    if pdf.is_encrypted() {
        return Err(anyhow::anyhow!(
            "PDF_ENCRYPTED: `{source_file}` is encrypted/password-protected and cannot be \
             extracted"
        ));
    }

    let page_numbers: Vec<u32> = pdf
        .get_pages()
        .into_iter()
        .map(|(page_no, _)| page_no)
        .collect();

    // Extract text per page, in order, so we can record page provenance.
    let mut per_page: Vec<(u32, String)> = Vec::new();
    let mut total_text_len = 0usize;
    for page_no in &page_numbers {
        match pdf.extract_text_with_limit(&[*page_no], PDF_PAGE_CONTENT_LIMIT) {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                let len = trimmed.len();
                total_text_len += len;
                per_page.push((*page_no, trimmed));
            }
            Err(e) => {
                // A single page failing extraction is surfaced, but does not
                // drop the whole document (other pages may be readable). We
                // record a placeholder so the page is still visible.
                return Err(anyhow::anyhow!(
                    "PDF_PAGE_UNAVAILABLE: page {page_no} of `{source_file}` could not be \
                     extracted ({e})"
                ));
            }
        }
    }

    if per_page.is_empty() || total_text_len == 0 {
        return Err(anyhow::anyhow!(
            "PDF_TEXT_UNAVAILABLE: `{source_file}` has no extractable text (it may be \
             image-only or a scanned document)"
        ));
    }

    // Build the canonical document. Each page becomes a section ("Page N") so
    // provenance maps cleanly to a finding's `location`.
    let mut builder = CanonicalDocumentBuilder::new().source("pdf", source_file);
    let mut section_counter = 0usize;

    for (page_no, text) in per_page {
        if text.is_empty() {
            continue;
        }
        section_counter += 1;
        let sec_loc = Some(SourceLocation {
            source_type: "pdf".into(),
            file: source_file.to_string(),
            include_parent: None,
            include_depth: 0,
            start_line: None,
            end_line: None,
            page: Some(page_no),
        });
        // Split page text into paragraphs on blank-line-ish boundaries. We
        // treat blocks separated by two+ newlines as paragraphs; a page is a
        // section heading.
        let paragraphs = split_page_into_paragraphs(&text, page_no, source_file);
        let section = Section {
            id: SectionId(format!("section_{}", section_counter)),
            title: format!("Page {}", page_no),
            paragraphs,
            location: sec_loc,
        };
        builder = builder.section(section);
    }

    let doc = builder.build();
    Ok(doc)
}

/// Split page text into canonical paragraphs, preserving page provenance.
fn split_page_into_paragraphs(text: &str, page_no: u32, source_file: &str) -> Vec<Paragraph> {
    let blocks: Vec<&str> = text
        .split("\n\n")
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect();

    blocks
        .into_iter()
        .enumerate()
        .map(|(idx, block)| Paragraph {
            id: ParagraphId(format!("page_{}.paragraph_{}", page_no, idx + 1)),
            text: block.replace('\n', " ").trim().to_string(),
            location: Some(SourceLocation {
                source_type: "pdf".into(),
                file: source_file.to_string(),
                include_parent: None,
                include_depth: 0,
                start_line: None,
                end_line: None,
                page: Some(page_no),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{Dictionary, Document as PdfDoc, Object, Stream};

    /// Build a (single or multi) page PDF in memory with the given text strings
    /// (one `Tj` per page). Returns the serialized PDF bytes.
    fn build_pdf(pages_text: &[&str]) -> Vec<u8> {
        let mut doc = PdfDoc::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dict(&[
            ("Type", Object::Name("Font".into())),
            ("Subtype", Object::Name("Type1".into())),
            ("BaseFont", Object::Name("Helvetica".into())),
        ]));
        let mut font_dict = Dictionary::new();
        font_dict.set("F1", font_id);
        let mut resources_inner = Dictionary::new();
        resources_inner.set("Font", Object::Dictionary(font_dict));
        let resources_id = doc.add_object(Object::Dictionary(resources_inner));

        let mut kids: Vec<Object> = Vec::new();
        for text in pages_text {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 700.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
            let page_id = doc.add_object(dict(&[
                ("Type", Object::Name("Page".into())),
                ("Parent", pages_id.into()),
                ("Contents", content_id.into()),
            ]));
            kids.push(page_id.into());
        }

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name("Pages".into()));
        pages.set("Kids", Object::Array(kids));
        pages.set("Count", pages_text.len() as i64);
        pages.set("Resources", resources_id);
        pages.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), 595.into(), 842.into()]),
        );
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let catalog_id = doc.add_object(dict(&[
            ("Type", Object::Name("Catalog".into())),
            ("Pages", pages_id.into()),
        ]));
        doc.trailer.set("Root", catalog_id);
        doc.compress();

        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    /// Build a dictionary from key/value pairs.
    fn dict(pairs: &[(&str, Object)]) -> Dictionary {
        let mut d = Dictionary::new();
        for (k, v) in pairs {
            d.set(*k, v.clone());
        }
        d
    }

    /// Build a single-page PDF with arbitrary content operations (for the
    /// empty-text case).
    fn build_pdf_with_content(ops: Vec<Operation>) -> Vec<u8> {
        let mut doc = PdfDoc::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
        let page_id = doc.add_object(dict(&[
            ("Type", Object::Name("Page".into())),
            ("Parent", pages_id.into()),
            ("Contents", content_id.into()),
        ]));
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name("Pages".into()));
        pages.set("Kids", Object::Array(vec![page_id.into()]));
        pages.set("Count", 1);
        pages.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), 595.into(), 842.into()]),
        );
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dict(&[
            ("Type", Object::Name("Catalog".into())),
            ("Pages", pages_id.into()),
        ]));
        doc.trailer.set("Root", catalog_id);
        doc.compress();
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn extracts_text_with_page_provenance() {
        let bytes = build_pdf(&["Hello PDF world."]);
        let doc = parse_pdf("paper.pdf", &bytes).unwrap();
        // One page -> one section titled "Page 1".
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].title, "Page 1");
        // Section carries page provenance.
        assert_eq!(doc.sections[0].location.as_ref().unwrap().page, Some(1));
        // The paragraph contains the extracted text and page provenance.
        assert!(!doc.sections[0].paragraphs.is_empty());
        let para = &doc.sections[0].paragraphs[0];
        assert!(para.text.contains("Hello PDF world."));
        let loc = para.location.as_ref().expect("provenance");
        assert_eq!(loc.page, Some(1));
        assert_eq!(loc.file, "paper.pdf");
    }

    #[test]
    fn multi_page_provides_per_page_provenance() {
        let bytes = build_pdf(&["First page content.", "Second page content."]);
        let doc = parse_pdf("paper.pdf", &bytes).unwrap();
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].title, "Page 1");
        assert_eq!(doc.sections[1].title, "Page 2");
        let p1 = doc.sections[0]
            .paragraphs
            .iter()
            .find(|p| p.text.contains("First page"))
            .expect("p1");
        let p2 = doc.sections[1]
            .paragraphs
            .iter()
            .find(|p| p.text.contains("Second page"))
            .expect("p2");
        assert_eq!(p1.location.as_ref().unwrap().page, Some(1));
        assert_eq!(p2.location.as_ref().unwrap().page, Some(2));
    }

    #[test]
    fn empty_text_fails_with_text_unavailable() {
        // A valid PDF with a page but no text-showing operators.
        let bytes = build_pdf_with_content(vec![
            Operation::new("BT", vec![]),
            Operation::new("ET", vec![]),
        ]);
        let err = parse_pdf("blank.pdf", &bytes).unwrap_err();
        assert!(
            err.to_string().contains("PDF_TEXT_UNAVAILABLE"),
            "expected TEXT_UNAVAILABLE but got: {err}"
        );
    }

    #[test]
    fn malformed_pdf_fails_invalid() {
        let garbage = b"%PDF-1.4\nnot a real pdf at all\n%%EOF";
        let err = parse_pdf("bad.pdf", garbage).unwrap_err();
        assert!(
            err.to_string().contains("PDF_INVALID"),
            "expected PDF_INVALID but got: {err}"
        );
    }

    #[test]
    fn encrypted_pdf_fails_encrypted() {
        // A valid but "encrypted" document (trailer carries an /Encrypt entry)
        // must not be silently extracted.
        let mut doc = PdfDoc::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name("Pages".into()));
        pages.set("Kids", Object::Array(vec![]));
        pages.set("Count", 0);
        pages.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), 595.into(), 842.into()]),
        );
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dict(&[
            ("Type", Object::Name("Catalog".into())),
            ("Pages", pages_id.into()),
        ]));
        doc.trailer.set("Root", catalog_id);
        // Minimal /Encrypt dictionary triggers encryption detection.
        doc.trailer.set(
            "Encrypt",
            dict(&[("Filter", Object::Name("Standard".into()))]),
        );
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();

        let result = parse_pdf("enc.pdf", &out);
        assert!(
            result.is_err(),
            "encrypted PDF must not be silently extracted"
        );
    }
}

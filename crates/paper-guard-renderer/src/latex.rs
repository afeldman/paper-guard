//! A LaTeX renderer that emits a source representation from the canonical
//! model. Used to re-render after a revision and then re-parse/validate.

use paper_guard_core::Document;

/// A rendered artifact: the produced source text plus metadata.
#[derive(Debug, Clone)]
pub struct RenderedOutput {
    pub format: String,
    pub text: String,
}

/// A renderer for LaTeX sources.
pub struct LatexRenderer;

impl LatexRenderer {
    /// Render a canonical document back to LaTeX-like text.
    ///
    /// This is a lossy but *deterministic* reconstruction used for validation:
    /// it does not invent content, it only emits what is present in the model.
    pub fn render(&self, doc: &Document) -> RenderedOutput {
        let mut out = String::new();
        if let Some(t) = &doc.meta.title {
            out.push_str(&format!("\\title{{{}}}\n", escape_brace(t)));
        }
        out.push_str("\\begin{document}\n");
        if let Some(a) = &doc.meta.abstract_text {
            out.push_str(&format!(
                "\\begin{{abstract}}\n{}\n\\end{{abstract}}\n",
                escape_brace(a)
            ));
        }
        for sec in &doc.sections {
            out.push_str(&format!("\\section{{{}}}\n", escape_brace(&sec.title)));
            for p in &sec.paragraphs {
                out.push_str(&p.text);
                out.push_str("\n\n");
            }
        }
        // Bibliography.
        if !doc.bibliography.is_empty() {
            out.push_str("\\begin{thebibliography}{9}\n");
            for r in &doc.bibliography {
                // Emit the canonical reference id as the bibitem key so that
                // in-text citations (which reference that id) resolve to the
                // entry on re-parse. Previously entries were renumbered
                // ref1, ref2 which broke citation linkage across render/re-parse.
                out.push_str(&format!(
                    "\\bibitem{{{}}}{} ({}). {}.\n",
                    escape_brace(&r.reference_id.0),
                    escape_brace(&r.authors),
                    r.year.map(|y| y.to_string()).unwrap_or_else(|| "n.d.".into()),
                    escape_brace(&r.title)
                ));
            }
            out.push_str("\\end{thebibliography}\n");
        }
        out.push_str("\\end{document}\n");
        RenderedOutput {
            format: "latex".into(),
            text: out,
        }
    }
}

/// Escape braces in LaTeX output to avoid breaking the document structure.
fn escape_brace(s: &str) -> String {
    s.replace('{', "\\{").replace('}', "\\}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{CanonicalDocumentBuilder, Paragraph, ParagraphId, Section, SectionId};

    #[test]
    fn render_roundtrips_section_content() {
        let doc = CanonicalDocumentBuilder::new()
            .title("T")
            .section(Section {
                id: SectionId("s1".into()),
                title: "Intro".into(),
                paragraphs: vec![Paragraph {
                    id: ParagraphId("s1.p1".into()),
                    text: "The cat sat on the mat.".into(),
                }],
            })
            .build();
        let r = LatexRenderer.render(&doc);
        assert!(r.text.contains("\\section{Intro}"));
        assert!(r.text.contains("The cat sat on the mat."));
        assert!(r.text.contains("\\title{T}"));
    }
}

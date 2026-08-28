//! Integration tests: render -> validation for the revision workflow.

use paper_guard_parser::Parser;
use paper_guard_validation::{TextValidator, ValidationReport};

#[test]
fn rerender_then_validate_preserves_content() {
    // Build a canonical document, render it to LaTeX, re-parse, validate.
    let doc = paper_guard_core::CanonicalDocumentBuilder::new()
        .source("latex", "main.tex")
        .title("T")
        .section(paper_guard_core::Section {
            id: paper_guard_core::SectionId("section_1".into()),
            title: "Intro".into(),
            paragraphs: vec![paper_guard_core::Paragraph {
                id: paper_guard_core::ParagraphId("section_1.paragraph_1".into()),
                text: "We show the method reduces latency by 40%.".into(),
                location: None,
            }],
            location: None,
        })
        .build();

    let renderer = paper_guard_renderer::LatexRenderer;
    let rendered = renderer.render(&doc);

    // Re-parse the rendered output.
    let parser = paper_guard_parser::LatexParser;
    let parsed = tokio::runtime::Runtime::new().unwrap().block_on(async {
        parser
            .parse("<rendered>", rendered.text.as_bytes())
            .await
            .unwrap()
    });

    let validator = TextValidator::new();
    let report: ValidationReport = validator.validate(&doc, &parsed.document);
    // The rendered output must preserve the paragraph (n-gram overlap).
    assert!(report.passed, "expected pass but got: {:?}", report.issues);
}

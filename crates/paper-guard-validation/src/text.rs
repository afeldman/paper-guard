//! Text and structural validation after re-rendering.

use paper_guard_core::Document;
use serde::{Deserialize, Serialize};

/// A validation issue found by the validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// The stage that detected it (e.g. `text`, `references`, `floats`).
    pub stage: String,
    /// A severity: error or warning.
    pub level: String,
    /// A short human-readable message.
    pub message: String,
}

/// The result of a validation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub passed: bool,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// An empty, passing report.
    pub fn ok() -> Self {
        ValidationReport {
            passed: true,
            issues: Vec::new(),
        }
    }

    pub fn failing(issues: Vec<ValidationIssue>) -> Self {
        let passed = issues.iter().all(|i| i.level == "warning");
        ValidationReport { passed, issues }
    }

    /// Add an issue, recomputing passed status.
    pub fn with_issue(mut self, issue: ValidationIssue) -> Self {
        let is_error = issue.level == "error";
        self.issues.push(issue);
        if is_error {
            self.passed = false;
        }
        self
    }
}

/// Configuration for the text validator.
#[derive(Debug, Clone)]
pub struct TextValidatorConfig {
    /// Length of the n-gram used for paragraph-overlap detection.
    pub ngram: usize,
}

impl Default for TextValidatorConfig {
    fn default() -> Self {
        TextValidatorConfig { ngram: 4 }
    }
}

/// A validator that checks a re-rendered document against the original.
#[derive(Default)]
pub struct TextValidator {
    pub config: TextValidatorConfig,
}

impl TextValidator {
    pub fn new() -> Self {
        TextValidator {
            config: TextValidatorConfig::default(),
        }
    }

    /// Validate a new document against an original.
    ///
    /// Checks:
    /// - paragraph-level content preservation (no wholesale lost paragraphs)
    /// - citation-to-bibliography integrity
    /// - figures/tables present with captions
    pub fn validate(&self, original: &Document, re_rendered: &Document) -> ValidationReport {
        let mut report = ValidationReport::ok();

        // 1) Paragraphs present in the original should still appear (by
        // normalised overlap) in the re-rendered document.
        let new_paras: Vec<&str> = re_rendered
            .sections
            .iter()
            .flat_map(|s| s.paragraphs.iter().map(|p| p.text.as_str()))
            .collect();
        for sec in &original.sections {
            for p in &sec.paragraphs {
                if !paragraph_overlaps(&p.text, &new_paras, self.config.ngram) {
                    report = report.with_issue(ValidationIssue {
                        stage: "text".into(),
                        level: "error".into(),
                        message: format!(
                            "paragraph {} appears lost or substantially changed after re-render",
                            p.id
                        ),
                    });
                }
            }
        }

        // 2) Every cited reference key must exist in the bibliography.
        for c in &re_rendered.citations {
            for r in &c.refs {
                if re_rendered.reference(r).is_none() {
                    report = report.with_issue(ValidationIssue {
                        stage: "references".into(),
                        level: "error".into(),
                        message: format!(
                            "citation {} references {} which has no bibliography entry",
                            c.citation_id, r
                        ),
                    });
                }
            }
        }

        // 3) Figures/tables should have a non-empty caption and be referenced
        //    in text.
        for f in &re_rendered.figures {
            if f.caption.trim().is_empty() {
                report = report.with_issue(ValidationIssue {
                    stage: "floats".into(),
                    level: "error".into(),
                    message: format!("figure {} has no caption", f.figure_id),
                });
            }
            if !document_mentions(re_rendered, &f.figure_id) {
                report = report.with_issue(ValidationIssue {
                    stage: "floats".into(),
                    level: "warning".into(),
                    message: format!("figure {} is not referenced in the text", f.figure_id),
                });
            }
        }
        for t in &re_rendered.tables {
            if t.caption.trim().is_empty() {
                report = report.with_issue(ValidationIssue {
                    stage: "floats".into(),
                    level: "error".into(),
                    message: format!("table {} has no caption", t.table_id),
                });
            }
        }

        report
    }
}

/// Whether any paragraph shares a substantial n-gram overlap with `text`.
fn paragraph_overlaps(text: &str, paragraphs: &[&str], ngram: usize) -> bool {
    if text.trim().is_empty() {
        return true; // nothing to preserve
    }
    let tokens: Vec<String> = tokenize(text);
    if tokens.len() < ngram {
        // Short paragraph: require exact-ish token set inclusion.
        return paragraphs.iter().any(|p| {
            let pt = tokenize(p);
            tokens.iter().all(|t| pt.contains(t))
        });
    }
    let grams: Vec<Vec<String>> = tokens
        .windows(ngram)
        .map(|w| w.to_vec())
        .collect();
    paragraphs.iter().any(|p| {
        let pt = tokenize(p);
        let overlap = pt.windows(ngram).filter(|w| {
            grams.iter().any(|g| g.iter().zip(w.iter()).all(|(a, b)| a == b))
        }).count();
        overlap >= 1
    })
}

/// Tokenize into lowercase normalized words.
fn tokenize(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether the document text mentions an id string (e.g. "F1", "Table 2").
fn document_mentions(doc: &Document, id: &str) -> bool {
    let needle = id.to_lowercase();
    doc.sections.iter().any(|s| {
        s.paragraphs
            .iter()
            .any(|p| p.text.to_lowercase().contains(&needle))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{
        CanonicalDocumentBuilder, Citation, Paragraph, ParagraphId, ReferenceId, Section, SectionId,
    };

    fn doc_with_paras(texts: &[&str]) -> Document {
        let mut b = CanonicalDocumentBuilder::new();
        let paras: Vec<Paragraph> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| Paragraph {
                id: ParagraphId(format!("s1.p{}", i + 1)),
                text: t.to_string(),
            })
            .collect();
        b = b.section(Section {
            id: SectionId("section_1".into()),
            title: "Intro".into(),
            paragraphs: paras,
        });
        b.build()
    }

    #[test]
    fn identical_documents_pass() {
        let doc = doc_with_paras(&["The cat sat on the mat.", "A second paragraph."]);
        let v = TextValidator::new();
        let report = v.validate(&doc, &doc);
        assert!(report.passed);
    }

    #[test]
    fn lost_paragraph_is_detected() {
        let orig = doc_with_paras(&[
            "The cat sat on the mat quietly.",
            "This very distinctive unique paragraph should survive.",
        ]);
        let re = doc_with_paras(&["The cat sat on the mat quietly."]);
        let v = TextValidator::new();
        let report = v.validate(&orig, &re);
        assert!(report
            .issues
            .iter()
            .any(|i| i.message.contains("lost or substantially changed")));
    }

    #[test]
    fn dangling_citation_flagged() {
        let doc = doc_with_paras(&["A claim."]);
        let mut doc = doc;
        doc.citations.push(Citation {
            citation_id: "CT1".into(),
            location: ParagraphId("s1.p1".into()),
            refs: vec![ReferenceId("R99".into())],
        });
        let v = TextValidator::new();
        let report = v.validate(&doc, &doc);
        assert!(report
            .issues
            .iter()
            .any(|i| i.stage == "references" && i.message.contains("R99")));
    }

    #[test]
    fn figure_without_caption_detected_when_float_present() {
        let orig = doc_with_paras(&["The cat sat on the mat."]);
        let mut re = doc_with_paras(&["The cat sat on the mat."]);
        re.figures.push(paper_guard_core::Figure {
            figure_id: "F1".into(),
            caption: String::new(),
            location: ParagraphId("s1.p1".into()),
            asset: None,
        });
        let v = TextValidator::new();
        let report = v.validate(&orig, &re);
        assert!(report.issues.iter().any(|i| i.message.contains("no caption")));
    }
}

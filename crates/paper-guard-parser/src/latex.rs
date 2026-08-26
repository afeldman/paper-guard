//! A LaTeX parser producing a canonical [`paper_guard_core::Document`].
//!
//! This parser is deliberately conservative: it extracts structure
//! (sections, paragraphs, bibliography, citations, floats) and performs a
//! heuristic first-pass claim extraction. It never invents content — if text
//! cannot be parsed it is dropped or flagged, never guessed.

use paper_guard_core::{
    CanonicalDocumentBuilder, Citation, Claim, ClaimId, ClaimType, Document, Equation, Figure,
    Paragraph, ParagraphId, Reference, ReferenceId, Section, SectionId, Table,
};

use crate::{ParsedSource, Parser, SourceFormat};

/// The LaTeX parser.
pub struct LatexParser;

#[async_trait::async_trait]
impl Parser for LatexParser {
    fn format(&self) -> SourceFormat {
        SourceFormat::Latex
    }

    async fn parse(&self, source_file: &str, bytes: &[u8]) -> anyhow::Result<ParsedSource> {
        let text = String::from_utf8_lossy(bytes).to_string();
        let doc = parse_latex(source_file, &text)?;
        Ok(ParsedSource::with_document(
            SourceFormat::Latex,
            source_file.to_string(),
            bytes.to_vec(),
            doc,
        ))
    }
}

/// Parse LaTeX source text into a canonical document.
pub fn parse_latex(source_file: &str, text: &str) -> anyhow::Result<Document> {
    let mut builder = CanonicalDocumentBuilder::new().source("latex", source_file);

    let title = capture_first(text, r"\\title\s*\{([^}]*)\}");
    if let Some(t) = title {
        builder = builder.title(t);
    }
    if let Some(abstract_txt) =
        capture_first(text, r"(?s)\\begin\{abstract\}(.*?)\\end\{abstract\}")
    {
        builder = builder.abstract_text(abstract_txt.trim().to_string());
    }

    // Split out the bibliography first so body processing never touches it.
    let (body, bibliography) = split_bibliography(text);

    // Remove the abstract environment from the body (already captured above).
    let abstract_re = regex::Regex::new(r"(?s)\\begin\{abstract\}.*?\\end\{abstract\}")
        .expect("abstract regex is valid");
    let body_clean = abstract_re.replace_all(&body, "\n").into_owned();

    // Process the body line-by-line into a section/paragraph layout.
    let (sections, citations, claims) = build_sections(&body_clean);

    for sec in sections {
        builder = builder.section(sec);
    }
    for c in citations {
        builder = builder.citation(c);
    }
    for c in claims {
        builder = builder.claim(c);
    }

    // References are NOT verified (never claim existence without a source).
    for r in parse_references(&bibliography) {
        builder = builder.reference(r);
    }

    // Floats (figures/tables/equations) — minimal structural capture.
    for f in extract_figures(&body) {
        builder = builder.figure(f);
    }
    for t in extract_tables(&body) {
        builder = builder.table(t);
    }
    for e in extract_equations(&body) {
        builder = builder.equation(e);
    }

    let doc = builder.build();
    Ok(doc)
}

/// Build sections from the body text, extracting citations and heuristic
/// claims along the way.
///
/// Returns `(sections, citations, claims)`.
fn build_sections(body: &str) -> (Vec<Section>, Vec<Citation>, Vec<Claim>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut citations: Vec<Citation> = Vec::new();
    let mut claims: Vec<Claim> = Vec::new();
    let mut current: Option<Section> = None;
    let mut section_counter = 0usize;
    let mut pending: Vec<String> = Vec::new();

    // Helper to flush the pending paragraph into the current section; returns
    // any citations found.
    macro_rules! flush {
        () => {{
            if pending.is_empty() {
                Vec::new()
            } else {
                let raw = pending.drain(..).collect::<Vec<_>>().join("\n");
                let cleaned = clean_tex(&raw);
                if cleaned.is_empty() {
                    Vec::new()
                } else {
                    let para_idx = current
                        .as_ref()
                        .map(|s| s.paragraphs.len() + 1)
                        .unwrap_or(1);
                    let base = current.as_ref().map(|s| s.id.0.as_str()).unwrap_or("front");
                    let para_id = ParagraphId(format!("{base}.paragraph_{para_idx}"));
                    // Citations are extracted from RAW text (pre-cleaning).
                    let cit = extract_citations(para_id.clone(), &raw);
                    // Heuristic claims from the cleaned text.
                    let page_claims = extract_claims(para_id.clone(), &cleaned);
                    claims.extend(page_claims);
                    push_paragraph(&mut current, para_id, cleaned);
                    cit
                }
            }
        }};
    }

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            let cit = flush!();
            citations.extend(cit);
            continue;
        }
        if let Some(cap) = regex_capture(line, r"^\\section\s*\{([^}]*)\}") {
            let cit = flush!();
            citations.extend(cit);
            if let Some(prev) = current.take() {
                sections.push(prev);
            }
            section_counter += 1;
            let id = SectionId(format!("section_{}", section_counter));
            current = Some(Section {
                id,
                title: cap.to_string(),
                paragraphs: Vec::new(),
            });
            continue;
        }
        if is_structural_command(line) {
            continue;
        }
        pending.push(raw_line.to_string());
    }
    let cit = flush!();
    citations.extend(cit);
    if let Some(last) = current.take() {
        sections.push(last);
    }
    (sections, citations, claims)
}

/// Push a cleaned paragraph into the current section (creating front matter if
/// needed).
fn push_paragraph(current: &mut Option<Section>, para_id: ParagraphId, text: String) {
    if let Some(sec) = current.as_mut() {
        sec.paragraphs.push(Paragraph { id: para_id, text });
    } else {
        let mut sec = Section {
            id: SectionId("front".into()),
            title: "Front matter".into(),
            paragraphs: Vec::new(),
        };
        sec.paragraphs.push(Paragraph { id: para_id, text });
        *current = Some(sec);
    }
}

/// Whether a text line is a pure structural LaTeX command with no prose worth
/// keeping (does not strip citations, which carry content).
fn is_structural_command(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('\\') && !t.starts_with("\\cite") && !t.starts_with("\\label")
}

/// Remove the bibliography environment from the body and return (body, biblio).
fn split_bibliography(text: &str) -> (String, String) {
    if let Some(start) = text.find("\\begin{thebibliography}") {
        if let Some(end_rel) = text[start..].find("\\end{thebibliography}") {
            let end = start + end_rel + "\\end{thebibliography}".len();
            let biblio = text[start..end].to_string();
            let body = format!("{}{}", &text[..start], &text[end..]);
            return (body, biblio);
        }
    }
    (text.to_string(), String::new())
}

/// Heuristic reference-parsing. Removes the `\bibitem` markers and splits each
/// entry. Never verifies existence — every entry starts as `NOT_VERIFIED`.
///
/// The reference's stable identity is the original BibTeX key from
/// `\bibitem{key}` so that in-text `\cite{key}` references resolve to the
/// bibliography entry across parse → render → re-parse. (Previously the key was
/// discarded and entries were renumbered `R1, R2, ...`, which broke the
/// citation → reference link and caused false "dangling citation" errors.)
fn parse_references(biblio: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut count = 0usize;
    // The first split segment is the bibliography environment preamble (e.g.
    // `{9}`) and must not be treated as a reference entry.
    for item in biblio.split("\\bibitem").skip(1) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        count += 1;
        let (key, rest) = split_bib_key(item);
        let clean = clean_tex(&rest);
        let authors = extract_field(item, "author").or_else(|| heuristic_authors(&clean));
        let year = extract_field_u32(item, "year").or_else(|| extract_year(item));
        // Use the original bib key when present; fall back to a positional id
        // only when the entry has no key (defensive).
        let reference_id = if key.is_empty() {
            ReferenceId(format!("R{}", count))
        } else {
            ReferenceId(key)
        };
        out.push(Reference {
            reference_id,
            authors: authors.unwrap_or_default(),
            year,
            title: extract_field(item, "title").unwrap_or_else(|| {
                // Fall back to the first ~12 words of the citation text.
                let words: Vec<&str> = clean.split_whitespace().take(12).collect();
                words.join(" ")
            }),
            venue: extract_field(item, "journal")
                .or_else(|| extract_field(item, "booktitle"))
                .unwrap_or_default(),
            verification: paper_guard_core::EvidenceState::NotVerified,
        });
    }
    out
}

/// A conservative heuristic for author names in plain-text references.
///
/// If a reference has no `\author{...}`, we take the text before the first
/// year-parenthesized group, trimmed, as the author string. This is only used
/// to *describe* the entry — existence is never asserted.
fn heuristic_authors(clean: &str) -> Option<String> {
    let upper_bound = clean
        .find(" (")
        .or_else(|| clean.find("("))
        .unwrap_or_else(|| clean.len().min(120));
    let candidate = clean[..upper_bound].trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

/// Extract an unsigned integer field (e.g. `\year{2020}`).
fn extract_field_u32(text: &str, field: &str) -> Option<u32> {
    extract_field(text, field)?.trim().parse::<u32>().ok()
}

/// Return the `\bibitem{key}` and the remainder after the key's closing brace.
fn split_bib_key(item: &str) -> (String, String) {
    if let Some((_, rest)) = item.split_once('{') {
        if let Some((k, r)) = rest.split_once('}') {
            return (k.trim().to_string(), r.to_string());
        }
    }
    // No `{key}` pattern: treat the whole item as remainder.
    (String::new(), item.to_string())
}

fn extract_field(text: &str, field: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"\\{}\s*{{([^}}]*)}}", regex::escape(field))).ok()?;
    re.captures(text)
        .map(|c| clean_tex(c.get(1).map(|m| m.as_str()).unwrap_or_default()))
}

fn extract_year(text: &str) -> Option<u32> {
    regex::Regex::new(r"\((19|20)\d{2}\)")
        .ok()?
        .captures(text)
        .and_then(|c| c.get(0))
        .and_then(|m| {
            m.as_str()
                .trim_matches(|c| c == '(' || c == ')')
                .parse()
                .ok()
        })
}

/// Extract `\cite{...}` citations from a paragraph.
fn extract_citations(location: ParagraphId, paragraph: &str) -> Vec<Citation> {
    let re = regex::Regex::new(r"\\cite\{([^}]*)\}").expect("valid regex");
    re.captures_iter(paragraph)
        .enumerate()
        .map(|(i, cap)| {
            let keys: Vec<ReferenceId> = cap[1]
                .split(',')
                .filter_map(|k| {
                    let t = k.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(ReferenceId(t.to_string()))
                    }
                })
                .collect();
            Citation {
                citation_id: format!("CT{}_{}", location.0, i + 1),
                location: location.clone(),
                refs: keys,
            }
        })
        .collect()
}

/// Heuristic first-pass claim extraction.
///
/// Claims are detected via indicator phrases commonly used for strong
/// assertions, and are NOT treated as verified — extraction only tags the text
/// and its location so a reviewer can examine it later.
fn extract_claims(location: ParagraphId, paragraph: &str) -> Vec<Claim> {
    let indicators = [
        " we show",
        " we find",
        " demonstrates",
        "shows that",
        " proves that",
        " we demonstrate",
        " achieves",
        " outperforms",
        " significantly reduces",
        " is more accurate",
        " demonstrates a",
        " establishes that",
    ];
    let para_lower = paragraph.to_lowercase();
    let mut claims = Vec::new();
    for (idx, indicator) in indicators.iter().enumerate() {
        if let Some(rel) = para_lower.find(indicator) {
            let start = rel.saturating_sub(80);
            let end = (rel + indicator.len() + 160).min(paragraph.len());
            let text = paragraph[start..end].trim().to_string();
            let id = ClaimId(format!("C{}_{}", location.0, idx + 1));
            claims.push(Claim {
                claim_id: id,
                location: location.clone(),
                text,
                claim_type: ClaimType::Result,
                confidence: 0.6,
                evidence_refs: Vec::new(),
                result_refs: Vec::new(),
                citation_refs: Vec::new(),
            });
            // Only take the first claim per paragraph in this heuristic pass.
            break;
        }
    }
    claims
}

/// Extract figures minimally.
fn extract_figures(body: &str) -> Vec<Figure> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for block in body.split("\\begin{figure}") {
        if block.find("\\end{figure}").is_none() {
            continue;
        }
        idx += 1;
        let caption = capture_first(block, r"(?s)\b(caption|Caption)\s*\{([^}]*)\}")
            .or_else(|| capture_first(block, r"\\caption\{([^}]*)\}"));
        out.push(Figure {
            figure_id: format!("F{}", idx),
            caption: caption.unwrap_or_default(),
            location: ParagraphId(format!("figure_{}_location", idx)),
            asset: None,
        });
    }
    out
}

/// Extract tables minimally.
fn extract_tables(body: &str) -> Vec<Table> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for block in body.split("\\begin{table}") {
        if block.find("\\end{table}").is_none() {
            continue;
        }
        idx += 1;
        let caption = capture_first(block, r"\\caption\{([^}]*)\}");
        out.push(Table {
            table_id: format!("T{}", idx),
            caption: caption.unwrap_or_default(),
            location: ParagraphId(format!("table_{}_location", idx)),
            rows: Vec::new(),
        });
    }
    out
}

/// Extract equations minimally.
fn extract_equations(body: &str) -> Vec<Equation> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for block in body.split("\\begin{equation}") {
        if let Some(end) = block.find("\\end{equation}") {
            idx += 1;
            out.push(Equation {
                equation_id: format!("Eq{}", idx),
                location: ParagraphId(format!("equation_{}_location", idx)),
                latex: block[..end].trim().to_string(),
                number: Some(idx.to_string()),
            });
        }
    }
    out
}

/// Simple LaTeX cleanup: strip braces, backslash commands, and math.
fn clean_tex(input: &str) -> String {
    // Remove comments.
    let no_comments = input
        .lines()
        .map(|l| {
            if l.trim_start().starts_with('%') {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Remove inline math.
    let no_math = regex::Regex::new(r"\$[^$]*\$")
        .map(|re| re.replace_all(&no_comments, " [math] ").into_owned())
        .unwrap_or_else(|_| no_comments);
    // Strip known structural commands.
    let re_cmd = regex::Regex::new(r"\\(section|subsection|label|usepackage|documentclass|begin|end|maketitle|unknown)\s*(\[[^\]]*\])?\{[^}]*\}")
        .map(|re| re.replace_all(&no_math, "").into_owned())
        .unwrap_or(no_math);
    // Replace remaining backslash-commands.
    let re_any = regex::Regex::new(r"\\([a-zA-Z@]+\s*)?[a-zA-Z@]+")
        .map(|re| re.replace_all(&re_cmd, "").into_owned())
        .unwrap_or(re_cmd);
    re_any
        .replace(['{', '}'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn regex_capture<'a>(text: &'a str, pattern: &str) -> Option<&'a str> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)
        .map(|c| c.get(1).map(|m| m.as_str()).unwrap_or_default())
}

fn capture_first(text: &str, pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"\documentclass{article}
\title{A Sample Paper}
\begin{document}
\maketitle
\begin{abstract}
We study a method.
\end{abstract}
\section{Introduction}
This paper introduces a new approach.

Our method significantly reduces latency by 40\%.
\section{Evaluation}
We show that the approach outperforms baselines \cite{smith2020}.

\begin{thebibliography}{9}
\bibitem{smith2020} Smith, J. (2020). A Study. Journal of X.
\end{thebibliography}
\end{document}"#;

    #[tokio::test]
    async fn parses_sections_and_paragraphs() {
        let parser = LatexParser;
        let parsed = parser.parse("main.tex", SAMPLE.as_bytes()).await.unwrap();
        let doc = &parsed.document;
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].title, "Introduction");
        // No invented claims/evidence.
        assert!(doc.claims.iter().all(|c| c.text.trim() != "None"));
    }

    #[tokio::test]
    async fn parses_bibliography() {
        let parser = LatexParser;
        let parsed = parser.parse("main.tex", SAMPLE.as_bytes()).await.unwrap();
        let doc = &parsed.document;
        assert_eq!(doc.bibliography.len(), 1);
        assert_eq!(doc.bibliography[0].authors, "Smith, J.");
        // References are NOT verified by default.
        assert_eq!(
            doc.bibliography[0].verification,
            paper_guard_core::EvidenceState::NotVerified
        );
    }

    #[tokio::test]
    async fn extracts_citations() {
        let parser = LatexParser;
        let parsed = parser.parse("main.tex", SAMPLE.as_bytes()).await.unwrap();
        let doc = &parsed.document;
        assert!(!doc.citations.is_empty());
        assert!(doc.citations.iter().any(|c| !c.refs.is_empty()));
    }

    #[test]
    fn clean_tex_removes_commands() {
        let s = clean_tex("We show that the method works \\cite{smith2020} with $x+y$.");
        assert!(!s.contains('\\'));
        assert!(!s.contains('{'));
        assert!(!s.contains('}'));
        assert!(s.contains("works"));
    }

    #[test]
    fn split_bibliography_removes_env() {
        let (body, biblio) = split_bibliography(SAMPLE);
        assert!(biblio.contains("smith2020"));
        assert!(!body.contains("\\begin{thebibliography}"));
        assert!(body.contains("\\section{Introduction}"));
    }

    #[test]
    fn extract_claims_is_conservative() {
        // No strong indicator -> no claims invented.
        let claims = extract_claims(
            ParagraphId("s.p".into()),
            "The weather this week has been mostly sunny with occasional clouds.",
        );
        assert!(claims.is_empty());
    }

    /// Regression: an in-text citation key must resolve to a bibliography
    /// entry with the same stable id. Previously citations kept `\cite{key}`
    /// (e.g. `smith2020`) while the bibliography discarded the key and used
    /// positional `R1, R2`, breaking the link and causing false "dangling
    /// citation" errors.
    #[test]
    fn citation_key_resolves_to_bibliography_entry() {
        let doc = parse_latex(
            "x.tex",
            r#"\documentclass{article}
\title{T}
\begin{document}
\section{Intro}
We rely on a prior result \cite{smith2020}.
\begin{thebibliography}{9}
\bibitem{smith2020} Smith, J. (2020). A Study. Journal of X.
\end{thebibliography}
\end{document}"#,
        )
        .unwrap();
        // The cited reference must resolve to a bibliography entry with the
        // same stable id (the bib key), not a positional renumber.
        assert!(!doc.citations.is_empty(), "expected a citation");
        let citing_key = &doc.citations[0].refs[0];
        assert_eq!(citing_key.0, "smith2020");
        let resolved = doc.reference(citing_key);
        assert!(
            resolved.is_some(),
            "citation {citing_key} must resolve to a bibliography entry"
        );
        assert_eq!(resolved.unwrap().authors, "Smith, J.");
    }
}

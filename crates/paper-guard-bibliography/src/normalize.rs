//! Deterministic normalization helpers for bibliography matching.
//!
//! All comparisons in the verification layer go through these functions so a
//! given (paper metadata, source metadata) pair always produces the same
//! match decision on every platform.

/// Normalize a title for comparison: lowercase, strip punctuation, collapse
/// whitespace, drop non-alphanumeric characters (Unicode letters kept).
pub fn normalize_title(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if ch.is_whitespace() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Normalize an arbitrary string for author/token comparisons.
pub fn normalize_token(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if ch.is_whitespace() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// Normalize a DOI: lowercase, trim whitespace/punctuation.
pub fn normalize_doi(input: &str) -> String {
    input
        .trim()
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_lowercase()
}

/// Normalize an arXiv id: lowercase, trim, remove version suffix (`v1`).
/// Accepts the bare id (`2101.12345`), prefixed forms (`arXiv:2101.12345`),
/// and absolute arXiv URLs (`http://arxiv.org/abs/2101.12345v2`).
pub fn normalize_arxiv_id(input: &str) -> String {
    let raw = input.trim().to_lowercase();
    let mut bare = raw.as_str();
    if let Some(idx) = raw.find("/abs/") {
        bare = &raw[idx + "/abs/".len()..];
    } else if let Some(stripped) = bare
        .strip_prefix("arxiv:")
        .or_else(|| bare.strip_prefix("arxiv/"))
    {
        bare = stripped;
    }
    let bare = bare.trim().trim_end_matches('/').trim_end_matches('.');
    // Strip a trailing version suffix (e.g. `2101.12345v2` -> `2101.12345`).
    if let Some(idx) = bare.rfind('v') {
        let (head, tail) = bare.split_at(idx);
        if tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            return head.to_string();
        }
    }
    bare.to_string()
}

/// Extract a bare arXiv id from free text like `arXiv:2101.12345v2`,
/// `arXiv:2101.12345`, or the legacy `arXiv:hep-th/9901001` form.
pub fn extract_arxiv_id(text: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"(?i)\barxiv\s*[:/]\s*(\d{4}\.\d{4,5}(?:v\d+)?|[a-z\-]+(\.[a-z]{2})?/\d{4,7}(?:v\d+)?)",
    )
    .ok()?;
    let cap = re.captures(text)?;
    let id = cap.get(1)?.as_str();
    let normalized = normalize_arxiv_id(id);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Extract a DOI (`10.xxxx/...`) from free text when present.
pub fn extract_doi(text: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)\b(10\.\d{4,9}/[^\s,;()\[\]]+)").ok()?;
    let cap = re.captures(text)?;
    let doi = cap.get(1)?.as_str().trim_end_matches('.').to_string();
    if doi.len() < 8 {
        return None;
    }
    Some(normalize_doi(&doi))
}

/// Extract the author surnames from an author string like
/// `Bugiolacchi, R. and Wöhler, C.`, `Smith, J.` or `Doe, A., Jones, B.`.
///
/// Returns lowercased tokenized surnames (e.g. `["bugiolacchi", "wöhler"]`).
pub fn author_surnames(authors: &str) -> Vec<String> {
    let mut out = Vec::new();
    // Split into author units on the separators used by bibliography styles.
    for raw_unit in authors.split(&[';', '&', '+'][..]) {
        for unit in raw_unit.split(" and ") {
            let unit = unit.trim();
            if unit.is_empty() {
                continue;
            }
            let surname = if let Some((last, _first)) = unit.split_once(',') {
                // "Last, First"
                let last = normalize_token(last.trim());
                if last.is_empty() {
                    None
                } else {
                    Some(last)
                }
            } else {
                // "First Last" or a single name: family name is the last token.
                let tokens: Vec<String> = unit
                    .split_whitespace()
                    .map(normalize_token)
                    .filter(|t| !t.is_empty())
                    .collect();
                tokens.last().cloned()
            };
            if let Some(s) = surname {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Whether any of the probe's surnames appears in the candidate's full author
/// text (normalized).
pub fn author_overlap(probe_authors: &str, candidate_authors: &str) -> bool {
    let surnames = author_surnames(probe_authors);
    if surnames.is_empty() {
        return true; // nothing to compare — do not treat absence as a mismatch
    }
    let haystack = normalize_token(candidate_authors);
    if haystack.is_empty() {
        return false;
    }
    surnames.iter().any(|s| haystack.contains(s.as_str()))
}

/// Word-level title similarity in [0, 1]. Exact normalized equality => 1.0;
/// otherwise the blend of Jaccard overlap and the smaller-set containment
/// (both symmetric in effect), so close title variants score highly.
pub fn title_similarity(a: &str, b: &str) -> f32 {
    let na = normalize_title(a);
    let nb = normalize_title(b);
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }
    let words_a: std::collections::HashSet<&str> = na.split(' ').collect();
    let words_b: std::collections::HashSet<&str> = nb.split(' ').collect();
    let union_len = words_a.union(&words_b).count();
    if union_len == 0 {
        return 0.0;
    }
    let common = words_a.intersection(&words_b).count() as f32;
    let jaccard = common / union_len as f32;
    let max_len = words_a.len().max(words_b.len()) as f32;
    let containment = if max_len == 0.0 {
        0.0
    } else {
        common / max_len
    };
    0.5 * jaccard + 0.5 * containment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_normalization_is_deterministic() {
        assert_eq!(
            normalize_title("Small craters population"),
            "small craters population"
        );
        assert_eq!(
            normalize_title("  SMALL   Craters, Population! "),
            "small craters population"
        );
        assert_eq!(normalize_title("Über die Hügel"), "über die hügel");
    }

    #[test]
    fn arxiv_ids_normalize_and_extract() {
        assert_eq!(normalize_arxiv_id("2101.12345"), "2101.12345");
        assert_eq!(normalize_arxiv_id("arXiv:2101.12345v2"), "2101.12345");
        assert_eq!(normalize_arxiv_id("ARXIV:2101.12345v2"), "2101.12345");
        assert_eq!(
            normalize_arxiv_id("http://arxiv.org/abs/2101.12345v2"),
            "2101.12345"
        );
        assert_eq!(
            normalize_arxiv_id("https://arxiv.org/abs/hep-th/9901001"),
            "hep-th/9901001"
        );
        assert_eq!(normalize_arxiv_id("hep-th/9901001"), "hep-th/9901001");
        assert_eq!(
            extract_arxiv_id("Preprint arXiv:2101.12345v2, 2021").as_deref(),
            Some("2101.12345")
        );
        assert_eq!(
            extract_arxiv_id("see arXiv:hep-th/9901001 for details").as_deref(),
            Some("hep-th/9901001")
        );
        assert_eq!(extract_arxiv_id("no id here"), None);
    }

    #[test]
    fn doi_extracts_and_normalizes() {
        assert_eq!(
            extract_doi("https://doi.org/10.1000/xyz123").as_deref(),
            Some("10.1000/xyz123")
        );
        assert_eq!(
            extract_doi("doi:10.1234/journal.5678.").as_deref(),
            Some("10.1234/journal.5678")
        );
        assert_eq!(extract_doi("nothing"), None);
        assert_eq!(normalize_doi("  10.1000/ABC  "), "10.1000/abc");
    }

    #[test]
    fn author_surnames_split_common_styles() {
        let names = author_surnames("Bugiolacchi, R. and Wöhler, C.");
        assert!(names.contains(&"bugiolacchi".to_string()));
        assert!(names.contains(&"wöhler".to_string()));

        let single = author_surnames("Smith, J.");
        assert_eq!(single, vec!["smith".to_string()]);

        let empty = author_surnames("");
        assert!(empty.is_empty());
    }

    #[test]
    fn author_overlap_detects_presence_and_absence() {
        assert!(author_overlap(
            "Bugiolacchi, R.",
            "Roberto Bugiolacchi and Christian Wöhler"
        ));
        assert!(author_overlap("", "Somebody Else"));
        assert!(!author_overlap("Smith, J.", "Doe, J. and Jones, A."));
    }

    #[test]
    fn title_similarity_ranges() {
        assert!((title_similarity("A B C", "A B C") - 1.0).abs() < 1e-6);
        let close = title_similarity(
            "Small craters population as a tool",
            "Small craters population as a geological tool",
        );
        assert!(close > 0.6 && close < 1.0, "close={close}");
        let far = title_similarity(
            "Quantum gravity in the early universe",
            "Craters on Mars: a review",
        );
        assert!(far < 0.35, "far={far}");
    }
}

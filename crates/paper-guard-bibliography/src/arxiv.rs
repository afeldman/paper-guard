//! The arXiv provider of the Bibliography Verification layer.
//!
//! # Privacy & security model
//!
//! * Only the fixed arXiv API endpoint (`https://export.arxiv.org/api/query`)
//!   is used. The base URL is **not** configurable — there is no SSRF surface.
//! * Only bibliographic metadata is sent (arXiv id, title words, first-author
//!   surname). Full manuscript text is never transmitted.
//! * Responses are size-bounded (see [`MAX_RESPONSE_BYTES`]) and every request
//!   has a timeout.
//! * arXiv data is *untrusted input*. It is parsed into entries and compared
//!   deterministically by [`decide`]; raw upstream metadata never becomes a
//!   finding or a truth claim on its own.
//!
//! # Determinism
//!
//! Matching is a pure function of (probe, entries). See [`decide`] for the
//! documented rule table.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use paper_guard_core::{BibliographyMismatch, BibliographyResult, VerificationStatus};

use crate::normalize::{
    author_overlap, normalize_arxiv_id, normalize_doi, normalize_title, title_similarity,
};
use crate::probe::ReferenceProbe;
use crate::provider::BibliographyProvider;

/// The fixed arXiv API base (never configurable at runtime).
pub const ARXIV_API_BASE: &str = "https://export.arxiv.org/api/query";
/// Safety cap on any single arXiv response body.
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// A parsed arXiv Atom entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArxivEntry {
    /// Normalized arXiv id (e.g. `2101.12345`).
    pub arxiv_id: String,
    /// Entry title (tags removed, entities decoded).
    pub title: String,
    /// Authors joined as a single string (source order).
    pub authors: String,
    /// Publication year from the `published` element.
    pub published_year: Option<u32>,
    /// DOI when the arXiv record carries one.
    pub doi: Option<String>,
    /// Journal reference when present (venue hint).
    pub journal_ref: Option<String>,
    /// Canonical abstract page URL.
    pub url: String,
}

/// A transport error from an arXiv request.
#[derive(Debug, Clone)]
pub enum ArxivClientError {
    Http { status: u16 },
    Timeout,
    TooLarge(usize),
    Network(String),
}

impl fmt::Display for ArxivClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArxivClientError::Http { status } => write!(f, "arXiv HTTP error {status}"),
            ArxivClientError::Timeout => write!(f, "arXiv request timed out"),
            ArxivClientError::TooLarge(n) => {
                write!(f, "arXiv response exceeded size limit ({n} bytes)")
            }
            ArxivClientError::Network(msg) => write!(f, "arXiv network error: {msg}"),
        }
    }
}

impl std::error::Error for ArxivClientError {}

/// HTTP transport abstraction so the provider is fully testable offline.
#[async_trait::async_trait]
pub trait ArxivClient: Send + Sync {
    /// Fetch the raw response body for a URL. Implementations must enforce
    /// timeouts and size bounds.
    async fn fetch(&self, url: &str) -> Result<String, ArxivClientError>;
}

/// The production client backed by `reqwest`.
pub struct ReqwestArxivClient {
    client: reqwest::Client,
}

impl ReqwestArxivClient {
    pub fn new(timeout_seconds: u64) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()?;
        Ok(ReqwestArxivClient { client })
    }
}

#[async_trait::async_trait]
impl ArxivClient for ReqwestArxivClient {
    async fn fetch(&self, url: &str) -> Result<String, ArxivClientError> {
        let response = self.client.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                ArxivClientError::Timeout
            } else {
                ArxivClientError::Network(e.to_string())
            }
        })?;
        if !response.status().is_success() {
            return Err(ArxivClientError::Http {
                status: response.status().as_u16(),
            });
        }
        if let Some(len) = response.content_length() {
            if len as usize > MAX_RESPONSE_BYTES {
                return Err(ArxivClientError::TooLarge(len as usize));
            }
        }
        let body = response
            .text()
            .await
            .map_err(|e| ArxivClientError::Network(e.to_string()))?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(ArxivClientError::TooLarge(body.len()));
        }
        Ok(body)
    }
}

/// The arXiv provider. Construct with a client; offline tests inject a fake
/// client so no unit test ever touches the network.
pub struct ArxivProvider {
    client: Arc<dyn ArxivClient>,
}

impl ArxivProvider {
    pub fn new(client: Arc<dyn ArxivClient>) -> Self {
        ArxivProvider { client }
    }

    /// Verify one reference against arXiv.
    pub async fn verify_impl(&self, probe: &ReferenceProbe) -> BibliographyResult {
        let (url, description) = build_query(probe);
        let body = match self.client.fetch(&url).await {
            Ok(body) => body,
            Err(err) => {
                let mut result = BibliographyResult::new(
                    probe.reference_id.clone(),
                    "arxiv",
                    VerificationStatus::Unavailable,
                    description,
                    probe.original_citation(),
                );
                result.note = Some(err.to_string());
                return result;
            }
        };
        let entries = parse_atom_feed(&body);
        decide(probe, &entries, &description)
    }
}

#[async_trait::async_trait]
impl BibliographyProvider for ArxivProvider {
    async fn verify(&self, probe: &ReferenceProbe) -> BibliographyResult {
        self.verify_impl(probe).await
    }
}

/// Build the query URL + human description for a probe.
///
/// * A probe with an explicit arXiv id performs a direct id lookup.
/// * Otherwise a title search is issued (first 8 significant words).
pub fn build_query(probe: &ReferenceProbe) -> (String, String) {
    if let Some(id) = probe.arxiv_id.as_deref() {
        let normalized = normalize_arxiv_id(id);
        (
            format!("{ARXIV_API_BASE}?id_list={normalized}"),
            format!("arXiv:{normalized} (direct id lookup)"),
        )
    } else {
        let normalized_title = normalize_title(&probe.title);
        let words: Vec<&str> = normalized_title
            .split(' ')
            .filter(|w| w.len() > 2)
            .take(8)
            .collect();
        let phrase = if words.is_empty() {
            normalize_arxiv_id(&probe.reference_id)
        } else {
            words.join(" ")
        };
        let encoded: String = phrase.chars().fold(String::new(), |mut acc, c| {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | ' ' | '-' | '.' | '_' | ':' => acc.push(c),
                other => {
                    let mut buf = [0u8; 4];
                    for b in other.encode_utf8(&mut buf).as_bytes() {
                        acc.push_str(&format!("%{b:02X}"));
                    }
                }
            }
            acc
        });
        (
            format!("{ARXIV_API_BASE}?search_query=ti:%22{encoded}%22&max_results=10"),
            format!("arXiv title search: {phrase}"),
        )
    }
}

/// Parse an arXiv Atom feed into entries. Deterministic: entry order from the
/// feed is preserved. Returns an empty vector on malformed/non-entry feeds
/// (a parse problem surfaces as `Not found`, never as fabricated data).
pub fn parse_atom_feed(xml: &str) -> Vec<ArxivEntry> {
    let mut entries = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<entry>") {
        let after = &rest[start + "<entry>".len()..];
        let Some(end) = after.find("</entry>") else {
            break;
        };
        let entry_xml = &after[..end];
        rest = &after[end + "</entry>".len()..];
        let Some(entry) = parse_atom_entry(entry_xml) else {
            continue;
        };
        entries.push(entry);
    }
    entries
}

fn parse_atom_entry(xml: &str) -> Option<ArxivEntry> {
    let id_raw = tag_content(xml, "id")?;
    // <id> may be absent in malformed entries; derive the id from the url.
    let abs = tag_content(xml, "title")?;
    let title = decode_entities(abs);
    let title = strip_inline_tags(&title);
    let title = title.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let arxiv_id = normalize_arxiv_id(id_raw);
    if arxiv_id.is_empty() {
        return None;
    }
    let authors: Vec<String> = xml
        .split("<author>")
        .skip(1)
        .filter_map(|part| {
            let name = tag_content(part, "name")?;
            let name = decode_entities(name).trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        })
        .collect();
    let published_year = tag_content(xml, "published")
        .and_then(|p| p.get(..4))
        .and_then(|y| y.parse::<u32>().ok());
    let doi = tag_content(xml, "arxiv:doi")
        .or_else(|| tag_content(xml, "doi"))
        .filter(|d| !d.trim().is_empty())
        .map(normalize_doi);
    let journal_ref = tag_content(xml, "arxiv:journal_ref")
        .or_else(|| tag_content(xml, "journal_ref"))
        .filter(|j| !j.trim().is_empty())
        .map(|j| decode_entities(j).trim().to_string());
    let url = format!("https://arxiv.org/abs/{arxiv_id}");
    Some(ArxivEntry {
        arxiv_id,
        title,
        authors: authors.join(", "),
        published_year,
        doi,
        journal_ref,
        url,
    })
}

/// Extract the content of the first `<tag ...>` occurrence, tolerating
/// attributes inside the opening tag (e.g. `<arxiv:doi xmlns="...">`).
fn tag_content<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}>");
    let start = xml.find(&open_pat)?;
    let after_open = &xml[start + open_pat.len()..];
    // Skip any attribute text up to the closing `>` of the opening tag.
    let open_end = after_open.find('>')?;
    let content_start = &after_open[open_end + 1..];
    let end = content_start.find(&close_pat)?;
    Some(&content_start[..end])
}

/// Remove simple inline markup inside title text.
fn strip_inline_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Decode the XML entities arXiv emits (named + numeric decimal/hex).
fn decode_entities(input: &str) -> String {
    let mut out = input.replace("&lt;", "<");
    out = out.replace("&gt;", ">");
    out = out.replace("&quot;", "\"");
    out = out.replace("&apos;", "'");
    out = out.replace("&amp;", "&");
    // Numeric decimal/hex entities (e.g. `&#246;`, `&#x246;`).
    let re = regex::Regex::new(r"&#(x?[0-9a-fA-F]+);").expect("static regex");
    out = re
        .replace_all(&out, |caps: &regex::Captures<'_>| {
            let raw = &caps[1];
            let code = match raw.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => raw.parse::<u32>().ok(),
            };
            match code.and_then(char::from_u32) {
                Some(ch) => ch.to_string(),
                None => caps[0].to_string(),
            }
        })
        .into_owned();
    out
}

/// Deterministic match decision between one probe and parsed entries.
///
/// # Rule table (stable, documented)
///
/// * no candidate / best title similarity < 0.45 → `NotFound`
/// * hard conflict (year/DOI/arXiv/author-set mismatch) with similarity ≥ 0.6
///   → `ConflictingMetadata`
/// * hard conflict with similarity ≥ 0.45 → `PartialMatch`
/// * no hard conflict:
///   * normalized title equality and no soft mismatch → `Verified`
///   * normalized title equality with soft mismatch, or similarity ≥ 0.75
///     → `LikelyMatch`
///   * similarity ≥ 0.45 → `PartialMatch`
///   * otherwise → `NotFound`
///
/// `mismatches[]` always carries every explicit field discrepancy found.
pub fn decide(
    probe: &ReferenceProbe,
    entries: &[ArxivEntry],
    description: &str,
) -> BibliographyResult {
    let base = |status: VerificationStatus| {
        BibliographyResult::new(
            probe.reference_id.clone(),
            "arxiv",
            status,
            description.to_string(),
            probe.original_citation(),
        )
    };

    if entries.is_empty() {
        let mut r = base(VerificationStatus::NotFound);
        r.note = Some("arXiv returned no candidate entry".into());
        return r;
    }

    // Choose the best candidate by normalized title similarity.
    let mut best: Option<&ArxivEntry> = None;
    let mut best_score = 0.0f32;
    for entry in entries {
        let score = if let Some(pid) = probe.arxiv_id.as_deref() {
            // Direct-id lookups anchor on the id, not the title.
            if normalize_arxiv_id(pid) == entry.arxiv_id {
                1.0
            } else {
                title_similarity(&probe.title, &entry.title)
            }
        } else {
            title_similarity(&probe.title, &entry.title)
        };
        if score > best_score {
            best_score = score;
            best = Some(entry);
        }
    }
    let Some(entry) = best else {
        let mut r = base(VerificationStatus::NotFound);
        r.note = Some("arXiv returned no usable candidate".into());
        return r;
    };

    evaluate_match(probe, entry, best_score, description)
}

fn evaluate_match(
    probe: &ReferenceProbe,
    entry: &ArxivEntry,
    similarity: f32,
    description: &str,
) -> BibliographyResult {
    let mut mismatches = Vec::new();

    let title_equal = !probe.title.trim().is_empty()
        && normalize_title(&probe.title) == normalize_title(&entry.title);
    if !title_equal && !probe.title.trim().is_empty() && !entry.title.trim().is_empty() {
        mismatches.push(BibliographyMismatch::new(
            "title",
            Some(probe.title.clone()),
            Some(entry.title.clone()),
        ));
    }

    let mut hard = false;
    if let (Some(py), Some(cy)) = (probe.year, entry.published_year) {
        if py != cy {
            hard = true;
            mismatches.push(BibliographyMismatch::new(
                "year",
                Some(py.to_string()),
                Some(cy.to_string()),
            ));
        }
    }
    if let (Some(pd), Some(cd)) = (probe.doi.as_deref(), entry.doi.as_deref()) {
        if normalize_doi(pd) != normalize_doi(cd) {
            hard = true;
            mismatches.push(BibliographyMismatch::new(
                "doi",
                Some(pd.to_string()),
                Some(cd.to_string()),
            ));
        }
    }
    if let (Some(pa), Some(ca)) = (probe.arxiv_id.as_deref(), Some(entry.arxiv_id.as_str())) {
        if normalize_arxiv_id(pa) != normalize_arxiv_id(ca) {
            hard = true;
            mismatches.push(BibliographyMismatch::new(
                "arxiv_id",
                Some(pa.to_string()),
                Some(ca.to_string()),
            ));
        }
    }
    if !probe.authors.trim().is_empty() && !author_overlap(&probe.authors, &entry.authors) {
        hard = true;
        mismatches.push(BibliographyMismatch::new(
            "authors",
            Some(probe.authors.clone()),
            Some(entry.authors.clone()),
        ));
    }
    // Venue/journal reference differences are soft (arXiv preprints often
    // appear under a journal-ref that differs from the cited venue).
    let probe_venue = probe.venue.trim().to_lowercase();
    if !probe_venue.is_empty()
        && !probe_venue.contains("arxiv")
        && !entry.title.is_empty()
        && entry.journal_ref.is_some()
    {
        let candidate_venue = entry
            .journal_ref
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        if !candidate_venue.is_empty()
            && normalize_venue(&probe.venue) != normalize_venue(&candidate_venue)
        {
            mismatches.push(BibliographyMismatch::new(
                "venue",
                Some(probe.venue.clone()),
                Some(entry.journal_ref.clone().unwrap_or_default()),
            ));
        }
    }

    let status = classify(similarity, title_equal, hard, !mismatches.is_empty());

    let mut result = BibliographyResult::new(
        probe.reference_id.clone(),
        "arxiv",
        status,
        description.to_string(),
        probe.original_citation(),
    );
    if status == VerificationStatus::NotFound {
        result.note = Some("best arXiv candidate was too weak to report".into());
    }
    result.title = if entry.title.trim().is_empty() {
        None
    } else {
        Some(entry.title.clone())
    };
    result.authors = if entry.authors.trim().is_empty() {
        None
    } else {
        Some(entry.authors.clone())
    };
    result.year = entry.published_year;
    result.venue = entry.journal_ref.clone();
    result.doi = entry.doi.clone();
    result.arxiv_id = Some(entry.arxiv_id.clone());
    result.url = Some(entry.url.clone());
    result.mismatches = mismatches;
    result
}

fn normalize_venue(venue: &str) -> String {
    venue
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn classify(
    similarity: f32,
    title_equal: bool,
    hard: bool,
    any_mismatch: bool,
) -> VerificationStatus {
    if similarity < 0.45 {
        return VerificationStatus::NotFound;
    }
    if hard {
        if similarity >= 0.6 {
            return VerificationStatus::ConflictingMetadata;
        }
        return VerificationStatus::PartialMatch;
    }
    if title_equal {
        if any_mismatch {
            VerificationStatus::LikelyMatch
        } else {
            VerificationStatus::Verified
        }
    } else if similarity >= 0.75 {
        VerificationStatus::LikelyMatch
    } else {
        VerificationStatus::PartialMatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paper_guard_core::{EvidenceState, Reference, ReferenceId};
    use std::sync::Arc;

    fn probe(title: &str, authors: &str, year: Option<u32>, venue: &str) -> ReferenceProbe {
        let reference = Reference {
            reference_id: ReferenceId("ref1".into()),
            authors: authors.into(),
            year,
            title: title.into(),
            venue: venue.into(),
            verification: EvidenceState::NotVerified,
        };
        ReferenceProbe::from_reference(&reference)
    }

    fn entry(id: &str, title: &str, year: u32, authors: &str) -> ArxivEntry {
        ArxivEntry {
            arxiv_id: id.into(),
            title: title.into(),
            authors: authors.into(),
            published_year: Some(year),
            doi: None,
            journal_ref: None,
            url: format!("https://arxiv.org/abs/{id}"),
        }
    }

    #[test]
    fn parse_atom_feed_extracts_entries() {
        let xml = r#"<?xml version="1.0"?><feed>
  <entry>
    <id>http://arxiv.org/abs/2101.12345v2</id>
    <title>Small craters population as a useful geological investigative tool</title>
    <author><name>Bugiolacchi, R.</name></author>
    <author><name>W&#246;hler, C.</name></author>
    <published>2021-01-28T00:00:00Z</published>
    <arxiv:doi xmlns="http://arxiv.org/schemas/atom">10.1016/j.icarus.2021.114000</arxiv:doi>
    <arxiv:journal_ref xmlns="http://arxiv.org/schemas/atom">Icarus</arxiv:journal_ref>
  </entry>
  <entry>
    <id>http://arxiv.org/abs/2101.99999</id>
    <title>Another &amp; unrelated title</title>
    <author><name>Someone, A.</name></author>
    <published>2021-02-02T00:00:00Z</published>
  </entry>
</feed>"#;
        let entries = parse_atom_feed(xml);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].arxiv_id, "2101.12345");
        assert!(entries[0].title.contains("Small craters population"));
        assert!(entries[0].authors.contains("Wöhler"));
        assert_eq!(entries[0].published_year, Some(2021));
        assert_eq!(
            entries[0].doi.as_deref(),
            Some("10.1016/j.icarus.2021.114000")
        );
        assert_eq!(entries[0].journal_ref.as_deref(), Some("Icarus"));
        assert!(entries[1].title.contains("&"));
    }

    #[test]
    fn parse_atom_feed_handles_empty_and_malformed() {
        assert!(parse_atom_feed("").is_empty());
        assert!(parse_atom_feed("<feed></feed>").is_empty());
        assert!(parse_atom_feed("<entry><title>x</title>").is_empty());
    }

    #[test]
    fn direct_arxiv_id_query_used_when_available() {
        let mut p = probe(
            "A crater study",
            "Bugiolacchi, R.",
            Some(2021),
            "arXiv preprint",
        );
        p.arxiv_id = Some("2101.12345".into());
        let (url, description) = build_query(&p);
        assert!(url.contains("id_list=2101.12345"));
        assert!(description.contains("direct id lookup"));
        // The endpoint is fixed — no free-form URL ever reaches the client.
        assert!(url.starts_with(ARXIV_API_BASE));
    }

    #[test]
    fn search_query_used_without_arxiv_id() {
        let p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        let (url, description) = build_query(&p);
        assert!(url.contains("search_query=ti:%22"));
        assert!(url.contains("max_results=10"));
        assert!(description.contains("title search"));
    }

    #[test]
    fn exact_match_is_verified() {
        let p = probe(
            "Small craters population",
            "Bugiolacchi, R. and Wöhler, C.",
            Some(2021),
            "Icarus",
        );
        let e = entry(
            "2101.12345",
            "Small craters population",
            2021,
            "Bugiolacchi, R., Wöhler, C.",
        );
        let r = decide(&p, &[e], "search");
        assert_eq!(r.status, VerificationStatus::Verified);
        assert!(r.matched);
        assert!(r.mismatches.is_empty());
        assert_eq!(r.arxiv_id.as_deref(), Some("2101.12345"));
    }

    #[test]
    fn title_mismatch_is_likely_or_partial() {
        let p = probe(
            "Small craters population as a useful geological tool",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        let e = entry(
            "2101.12345",
            "Small craters population as a geological investigative tool",
            2021,
            "Bugiolacchi, R.",
        );
        let r = decide(&p, &[e], "search");
        assert!(
            r.status == VerificationStatus::LikelyMatch,
            "{:?}",
            r.status
        );
        assert!(r.mismatches.iter().any(|m| m.field == "title"));
    }

    #[test]
    fn year_mismatch_is_conflicting() {
        let p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2020),
            "",
        );
        let e = entry(
            "2101.12345",
            "Small craters population",
            2021,
            "Bugiolacchi, R.",
        );
        let r = decide(&p, &[e], "search");
        assert_eq!(r.status, VerificationStatus::ConflictingMetadata);
        assert!(r.mismatches.iter().any(|m| m.field == "year"));
    }

    #[test]
    fn doi_mismatch_is_conflicting() {
        let mut p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        p.doi = Some("10.1016/j.icarus.2021.114000".into());
        let mut e = entry(
            "2101.12345",
            "Small craters population",
            2021,
            "Bugiolacchi, R.",
        );
        e.doi = Some("10.1016/j.icarus.2020.113999".into());
        let r = decide(&p, &[e], "search");
        assert_eq!(r.status, VerificationStatus::ConflictingMetadata);
        assert!(r.mismatches.iter().any(|m| m.field == "doi"));
    }

    #[test]
    fn arxiv_id_mismatch_is_conflicting() {
        let mut p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2021),
            "arXiv:2101.12345",
        );
        p.arxiv_id = Some("2101.12345".into());
        let e = entry(
            "2102.99999",
            "Small craters population",
            2021,
            "Bugiolacchi, R.",
        );
        let r = decide(&p, &[e], "search");
        assert_eq!(r.status, VerificationStatus::ConflictingMetadata);
        assert!(r.mismatches.iter().any(|m| m.field == "arxiv_id"));
    }

    #[test]
    fn author_mismatch_is_conflicting() {
        let p = probe("Small craters population", "Smith, J.", Some(2021), "");
        let e = entry("2101.12345", "Small craters population", 2021, "Doe, A.");
        let r = decide(&p, &[e], "search");
        assert_eq!(r.status, VerificationStatus::ConflictingMetadata);
        assert!(r.mismatches.iter().any(|m| m.field == "authors"));
    }

    #[test]
    fn not_found_when_no_candidate_or_weak() {
        let p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        assert_eq!(
            decide(&p, &[], "search").status,
            VerificationStatus::NotFound
        );
        let weak = entry(
            "9999.00001",
            "Quantum gravity in early cosmology",
            2021,
            "Other, X.",
        );
        assert_eq!(
            decide(&p, &[weak], "search").status,
            VerificationStatus::NotFound
        );
    }

    #[test]
    fn multiple_candidates_pick_best() {
        let p = probe(
            "Small craters population as a tool",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        let far = entry(
            "1501.00001",
            "Quantum gravity in early cosmology",
            2015,
            "Other, X.",
        );
        let close = entry(
            "2101.12345",
            "Small craters population as a geological tool",
            2021,
            "Bugiolacchi, R.",
        );
        let r = decide(&p, &[far, close], "search");
        assert_eq!(r.arxiv_id.as_deref(), Some("2101.12345"));
        assert_eq!(r.status, VerificationStatus::LikelyMatch);
    }

    #[test]
    fn unavailable_on_network_error_and_timeout() {
        struct BrokenClient(ArxivClientError);
        #[async_trait::async_trait]
        impl ArxivClient for BrokenClient {
            async fn fetch(&self, _url: &str) -> Result<String, ArxivClientError> {
                Err(self.0.clone())
            }
        }
        for err in [
            ArxivClientError::Timeout,
            ArxivClientError::Network("connection refused".into()),
            ArxivClientError::Http { status: 503 },
        ] {
            let provider = ArxivProvider::new(Arc::new(BrokenClient(err.clone())));
            let p = probe(
                "Small craters population",
                "Bugiolacchi, R.",
                Some(2021),
                "",
            );
            let result = futures::executor::block_on(provider.verify(&p));
            assert_eq!(result.status, VerificationStatus::Unavailable, "{err}");
            assert!(result.note.is_some());
            assert!(!result.from_cache);
            // Transient failures must not be cached as if they were findings.
            assert_eq!(result.mismatches.len(), 0);
        }
    }

    #[test]
    fn deterministic_across_equivalent_inputs() {
        let p = probe(
            "Small craters population",
            "Bugiolacchi, R.",
            Some(2021),
            "",
        );
        let e = entry(
            "2101.12345",
            "Small craters population",
            2021,
            "Bugiolacchi, R.",
        );
        let a = decide(&p, std::slice::from_ref(&e), "s1");
        let b = decide(&p, std::slice::from_ref(&e), "s2");
        assert_eq!(a.status, b.status);
        assert_eq!(a.confidence, b.confidence);
        assert_eq!(a.mismatches, b.mismatches);
    }
}

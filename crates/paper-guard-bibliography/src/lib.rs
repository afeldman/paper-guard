//! # Paper Guard Bibliography Verification Layer (M10)
//!
//! Optional verification of a paper's bibliographic metadata against
//! scholarly sources — without ever touching the scientific review results.
//!
//! ```text
//! Paper
//!   │
//!   └── Bibliography ──► BibliographyVerifier
//!                          ├── ArxivProvider        (real, opt-in network)
//!                          ├── GoogleScholarProvider (Unavailable slot — see below)
//!                          └── MockProvider          (offline test/demo double)
//!                          ▼
//!                      canonical paper_guard_core::BibliographyResult
//!                          ▼
//!                      RunRecord.bibliography / human report / JSON
//! ```
//!
//! # Non-negotiable boundaries
//!
//! * The layer is **additive**: results are appended to the canonical
//!   RunRecord. No existing finding, confidence, severity, evidence state, or
//!   ledger entry is ever modified or deleted, and the manuscript is never
//!   altered.
//! * Providers treat external data as **untrusted input**; the final status is
//!   always produced by Paper Guard's deterministic matcher
//!   ([`arxiv::decide`]), never copied verbatim from upstream.
//! * Bibliography verification checks **metadata identity**. It does not
//!   prove that a cited scientific statement is substantively correct.
//! * Only the bibliographic metadata needed for lookup is sent to a source —
//!   never full manuscript text. The arXiv endpoint is fixed and is not
//!   configurable (no SSRF surface). Requests are bounded in size and time.
//!
//! # Google Scholar status
//!
//! Paper Guard does **not** automate Google Scholar: there is no stable
//! official API for this purpose, HTML scraping is fragile and violates the
//! service's terms of use, and CAPTCHA/rate-limit evasion is out of bounds.
//! The provider abstraction is in place ([`provider::BibliographyProvider`]),
//! and the Scholar slot returns a clean `Unavailable` result with an
//! explanatory note whenever enabled. It never fabricates matches.
//! Crossref / OpenAlex / DataCite are documented as technically viable future
//! providers behind the same trait.
//!
//! # Offline by default
//!
//! Verification is disabled unless `[bibliography] enabled = true` in the
//! config. All tests in this crate run offline against deterministic fakes.

pub mod arxiv;
pub mod cache;
pub mod mock;
pub mod normalize;
pub mod probe;
pub mod provider;
pub mod scholar;
pub mod verifier;

pub use arxiv::{
    build_query, decide, parse_atom_feed, ArxivClient, ArxivClientError, ArxivEntry, ArxivProvider,
    ReqwestArxivClient, ARXIV_API_BASE, MAX_RESPONSE_BYTES,
};
pub use cache::DiskCache;
pub use mock::MockProvider;
pub use probe::ReferenceProbe;
pub use provider::BibliographyProvider;
pub use scholar::GoogleScholarProvider;
pub use verifier::{BibliographyVerifier, NamedProvider, VerifierConfig};

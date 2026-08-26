//! Verification and selection of discovered services.
//!
//! Discovery produces *candidates*, but a candidate is only conditionally
//! trusted after it has been verified. This module implements:
//!
//! - **Health verification** — `GET /health` on the candidate to confirm it is
//!   really Paper Guard and to read its version. This performs **no** manuscript
//!   transmission and **no** local filesystem access.
//! - **Version compatibility** — the client and the remote service may differ;
//!   we prefer API compatibility over binary equality and only reject when the
//!   remote reports a clearly incompatible major.
//! - **Selection** — when multiple services are present, we never pick "first
//!   response wins"; selection requires an explicit `preferred_service` and a
//!   user confirmation step that is strictly outside this module.

use paper_guard_client::PaperGuardClient;

use super::error::{DiscoveryError, DiscoveryResult};
use super::model::ServiceEndpoint;

/// Maximum version-scheme distance considered "API compatible".
///
/// We treat a remote *major* that differs from our own major as incompatible.
/// Patch/minor differences are always accepted: API compatibility is what
/// matters, not binary version equality.
const INCOMPATIBLE_MAJOR_TOLERANCE: u32 = 0;

/// The result of classifying a single verified candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// The candidate is healthy and API-compatible.
    Verified,
    /// The candidate is healthy but advertises an incompatible version.
    IncompatibleVersion,
    /// The candidate could not be reached or failed health checks.
    Rejected,
}

/// A verified endpoint along with the version returned by `GET /health`.
#[derive(Debug, Clone)]
pub struct VerifiedEndpoint {
    /// The endpoint, with its `version` field updated from health where known.
    pub endpoint: ServiceEndpoint,
    /// The outcome of verification.
    pub outcome: VerificationOutcome,
}

/// Verify a single candidate by calling `GET /health`.
///
/// This is the only place the client contacts a candidate; it transmits no
/// manuscript bytes. The HTTP timeout is short (5s) so a hostile or dead
/// endpoint cannot stall discovery.
pub async fn verify_and_classify(endpoint: ServiceEndpoint, our_version: &str) -> VerifiedEndpoint {
    let mut ep = endpoint.clone();
    let base = ep.base_url();
    let client = match PaperGuardClient::new(&ephemeral_client_config(&base)) {
        Ok(c) => c,
        Err(_) => {
            return VerifiedEndpoint {
                outcome: VerificationOutcome::Rejected,
                endpoint: ep,
            }
        }
    };
    match client.health().await {
        Ok(health) => {
            // Establish identity: only accept responses that self-identify as
            // Paper Guard.
            if health.service != "paper-guard" {
                return VerifiedEndpoint {
                    outcome: VerificationOutcome::Rejected,
                    endpoint: ep,
                };
            }
            ep.version = health.version.clone();
            let outcome = if version_incompatible(our_version, &health.version) {
                VerificationOutcome::IncompatibleVersion
            } else {
                VerificationOutcome::Verified
            };
            VerifiedEndpoint {
                endpoint: ep,
                outcome,
            }
        }
        Err(_) => VerifiedEndpoint {
            outcome: VerificationOutcome::Rejected,
            endpoint: ep,
        },
    }
}

/// Select a single service from verified candidates.
///
/// Returns `Err(NotFound)` when there is nothing usable, and `Err(Disabled)` /
/// explicit errors when selection is ambiguous. Automatic selection is only
/// permitted when `preferred_service` is non-empty and exactly matches one
/// candidate's hostname. Manual callers should instead present the full list to
/// the user for explicit choice.
pub fn select_service(
    verified: &[VerifiedEndpoint],
    preferred_service: &str,
) -> DiscoveryResult<ServiceEndpoint> {
    // Only Verified endpoints are eligible for automatic selection. A prefer
    // an incompatible-but-healthy service is never automatic.
    let eligible: Vec<ServiceEndpoint> = verified
        .iter()
        .filter(|v| v.outcome == VerificationOutcome::Verified)
        .map(|v| v.endpoint.clone())
        .collect();

    if eligible.is_empty() {
        let any_incompatible = verified
            .iter()
            .any(|v| v.outcome == VerificationOutcome::IncompatibleVersion);
        if any_incompatible {
            return Err(DiscoveryError::IncompatibleVersion(
                "discovered Paper Guard service(s) are API-incompatible".into(),
            ));
        }
        return Err(DiscoveryError::NotFound);
    }

    // No automatic selection without an explicit preferred service.
    if preferred_service.trim().is_empty() {
        if eligible.len() == 1 {
            return Ok(eligible.into_iter().next().unwrap());
        }
        return Err(DiscoveryError::MalformedRecord(
            "multiple Paper Guard services found; select one explicitly".into(),
        ));
    }

    let pref = preferred_service.trim().trim_end_matches('.');
    let mut found: Vec<ServiceEndpoint> = eligible
        .into_iter()
        .filter(|e| e.hostname.trim_end_matches('.') == pref || e.name == pref)
        .collect();

    if let Some(exact) = found.pop() {
        if found.is_empty() {
            return Ok(exact);
        }
    }
    Err(DiscoveryError::MalformedRecord(
        "preferred service does not uniquely match; refusing automatic selection".into(),
    ))
}

/// Whether a remote version is API-incompatible with ours.
///
/// Conservative: differing major versions are rejected; anything else accepted.
/// Whether a remote version is API-incompatible with ours. Exposed for tests.
pub fn version_incompatible(ours: &str, theirs: &str) -> bool {
    let (Some(our_major), Some(their_major)) = (major_version(ours), major_version(theirs)) else {
        // Unknown/absent versions are treated as *compatible* so that we never
        // hard-fail on a service that simply omits its version in discovery; the
        // health response carries a real version and its parse failure here is
        // the exception.
        return false;
    };
    let diff = (our_major as i64 - their_major as i64).unsigned_abs();
    diff > u64::from(INCOMPATIBLE_MAJOR_TOLERANCE)
}

/// Parse the leading numeric component of a semantic version (e.g. `0.5.0` →
/// `0`). Returns `None` for values that are not valid semver prefixes.
fn major_version(v: &str) -> Option<u32> {
    let first = v.trim().split('.').next()?;
    let digits = first
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// Build a short-timeout, token-free client config for health verification.
fn ephemeral_client_config(base_url: &str) -> paper_guard_client::ClientConfig {
    // Build a token-free, short-timeout client. `ClientConfig::new` strips
    // trailing slashes and never embeds any auth token.
    paper_guard_client::ClientConfig::new(base_url, 5)
}

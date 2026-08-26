//! A deterministic, network-free discovery provider for tests and demos.
//!
//! [`MockServiceDiscovery`] lets callers exercise the entire discovery →
//! verification → selection flow without a LAN, without multicast, and without
//! a real service. It is the primary way the CLI's `discover` behaviour is
//! tested deterministically in CI.

use async_trait::async_trait;

use super::error::DiscoveryResult;
use super::model::ServiceEndpoint;
use super::ServiceDiscovery;

/// A deterministic discovery provider driven by a fixed candidate list.
///
/// The [`ServiceDiscovery`] impl simply returns the configured endpoints
/// verbatim; it performs no verification and no selection, exactly like a real
/// discovery backend. This makes it trivial to test zero-, one-, and
/// multi-service scenarios as well as malformed records (which are surfaced
/// through the candidate list exactly as the caller injected them).
#[derive(Debug, Clone, Default)]
pub struct MockServiceDiscovery {
    candidates: Vec<ServiceEndpoint>,
}

impl MockServiceDiscovery {
    /// A provider that always returns the supplied candidates.
    pub fn new(candidates: Vec<ServiceEndpoint>) -> Self {
        Self { candidates }
    }

    /// A provider that returns no candidates.
    pub fn empty() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }
}

#[async_trait]
impl ServiceDiscovery for MockServiceDiscovery {
    async fn discover(&self) -> DiscoveryResult<Vec<ServiceEndpoint>> {
        Ok(self.candidates.clone())
    }
}

/// Build a [`ServiceEndpoint`] fixture for tests.
pub fn endpoint(
    name: &str,
    hostname: &str,
    address: &str,
    port: u16,
    version: &str,
) -> ServiceEndpoint {
    ServiceEndpoint {
        name: name.to_string(),
        hostname: hostname.to_string(),
        address: address.to_string(),
        port,
        scheme: "http".to_string(),
        service_type: "_paper-guard._tcp".to_string(),
        version: version.to_string(),
        capabilities: vec!["review".to_string()],
    }
}

//! # paper-guard-discovery
//!
//! Provider-independent LAN service discovery for Paper Guard.
//!
//! The discovery subsystem is deliberately decoupled from any concrete mDNS
//! implementation. Consumers (the CLI, higher-level services) depend only on
//! the [`ServiceDiscovery`] trait and the [`ServiceEndpoint`] model. This keeps
//! Paper Guard free of any Avahi/mDNS-specific logic while still enabling
//! mDNS/DNS-SD discovery.
//!
//! ## Security contract
//!
//! - **Discovery ≠ authorization.** Finding a service never authorises an
//!   upload. A manuscript is only ever sent to a remote service when remote
//!   execution has been explicitly selected.
//! - **Untrusted input.** Every discovery record is treated as untrusted and
//!   validated before it becomes a [`ServiceEndpoint`]; a malicious record
//!   cannot cause command execution, filesystem access, secret disclosure, or
//!   arbitrary URL injection.
//! - **Identity.** A discovered address is *not* trusted on the strength of a
//!   hostname; candidates are cross-checked through `GET /health`.
//! - **Multiple services.** No "first response wins". The client lists all
//!   candidates; automatic selection requires an explicit configuration.

pub mod error;
pub mod mdns;
pub mod mock;
pub mod model;
pub mod verify;

pub use error::{DiscoveryError, DiscoveryResult};
pub use mdns::MdnsServiceDiscovery;
pub use mock::MockServiceDiscovery;
pub use model::{
    DiscoveryConfig, DiscoveryMode, ServiceEndpoint, PAPER_GUARD_SERVICE_DOMAIN,
    PAPER_GUARD_SERVICE_TYPE, TXT_KEY_SCHEME, TXT_KEY_VERSION,
};
pub use verify::{select_service, verify_and_classify, VerificationOutcome, VerifiedEndpoint};

use async_trait::async_trait;

/// A provider-independent discovery interface.
///
/// Implementations return zero or more *candidate* endpoints. Callers are
/// expected to verify candidates (e.g. through `GET /health`) before trusting
/// them; discovery itself never grants permission to transmit a manuscript.
#[async_trait]
pub trait ServiceDiscovery: Send + Sync {
    /// Discover candidate Paper Guard endpoints on the network.
    ///
    /// This performs *no* verification and *no* manuscript transmission. It
    /// may return multiple candidates, never performs automatic selection, and
    /// never touches any manuscript data.
    async fn discover(&self) -> DiscoveryResult<Vec<ServiceEndpoint>>;
}

/// A no-op discovery provider used when discovery is disabled. It returns an
/// empty candidate list (never errors on the "not found" condition).
pub struct DisabledDiscovery;

#[async_trait]
impl ServiceDiscovery for DisabledDiscovery {
    async fn discover(&self) -> DiscoveryResult<Vec<ServiceEndpoint>> {
        Ok(Vec::new())
    }
}

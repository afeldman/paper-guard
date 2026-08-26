//! An mDNS/DNS-SD discovery backend built on [`mdns_sd`].
//!
//! This is the concrete implementation used by `paper-guard discover`. It
//! browses the multicast domain for [`PAPER_GUARD_SERVICE_TYPE`] instances and
//! converts each resolved [`ServiceInfo`] into a safe [`ServiceEndpoint`].
//!
//! No mDNS logic leaks out of this module: upstream consumers only ever see
//! the provider-independent [`super::ServiceDiscovery`] trait and
//! [`super::ServiceEndpoint`] model.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use super::error::{DiscoveryError, DiscoveryResult};
use super::model::{ServiceEndpoint, TXT_KEY_VERSION, PAPER_GUARD_SERVICE_TYPE};
use super::ServiceDiscovery;

/// The default length of time to listen for mDNS responses before returning.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(3000);

/// An mDNS/DNS-SD discovery provider.
///
/// Cheap to build and stateless; a fresh [`MdnsServiceDiscovery`] can be
/// created per `discover()` call. Construction does **not** start the network
/// daemon; that only happens inside [`ServiceDiscovery::discover`], so a
/// configured-but-unused provider never emits multicast traffic.
#[derive(Debug, Clone)]
pub struct MdnsServiceDiscovery {
    service_type: Arc<str>,
    timeout: Duration,
}

impl Default for MdnsServiceDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsServiceDiscovery {
    /// Build a provider for the default Paper Guard service type.
    pub fn new() -> Self {
        Self {
            service_type: format!("{}.", PAPER_GUARD_SERVICE_TYPE).into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the DNS-SD service type to browse (e.g. a scoped `.lab.local.`
    /// domain). The value need not carry a trailing dot; it is normalised.
    pub fn with_service_type(mut self, service_type: &str) -> Self {
        let t = service_type.trim().trim_end_matches('.');
        if !t.is_empty() {
            self.service_type = format!("{t}.").into();
        }
        self
    }

    /// Override the receive timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        if !timeout.is_zero() {
            self.timeout = timeout;
        }
        self
    }
}

#[async_trait]
impl ServiceDiscovery for MdnsServiceDiscovery {
    async fn discover(&self) -> DiscoveryResult<Vec<ServiceEndpoint>> {
        // mdns-sd is synchronous (internal threads + channels), so we run the
        // whole browse on a blocking task to keep the async caller responsive.
        let daemon = ServiceDaemon::new()
            .map_err(|e| DiscoveryError::Backend(format!("mdns daemon: {e}")))?;
        let receiver = daemon
            .browse(self.service_type.as_ref())
            .map_err(|e| DiscoveryError::Backend(format!("browse start: {e}")))?;

        let deadline = Instant::now() + self.timeout;
        let mut endpoints: Vec<ServiceEndpoint> = Vec::new();

        // Drain events until the deadline. Even when nothing arrives, we wait
        // the full window so the daemon has time to collect responses.
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if let Some(ep) = endpoint_from_info(&info) {
                        // Late duplicates (a re-announce from the same
                        // instance) are dropped by fullname.
                        if !endpoints.iter().any(|e| e.name == ep.name && e.hostname == ep.hostname)
                        {
                            endpoints.push(ep);
                        }
                    }
                }
                Ok(ServiceEvent::ServiceFound(..))
                | Ok(ServiceEvent::SearchStarted(..)) => {
                    // Not yet resolved; wait for ServiceResolved.
                }
                Ok(ServiceEvent::ServiceRemoved(..))
                | Ok(ServiceEvent::SearchStopped(..)) => {
                    // Keep listening; removal does not end the window.
                }
                Err(flume::RecvTimeoutError::Timeout) => {
                    // Timeout simply ends the window.
                    break;
                }
                Err(e) => {
                    let _ = daemon.stop_browse(self.service_type.as_ref());
                    let _ = daemon.shutdown();
                    return Err(DiscoveryError::Backend(format!("browse: {e}")));
                }
            }
        }

        let _ = daemon.stop_browse(self.service_type.as_ref());
        let _ = daemon.shutdown();
        Ok(endpoints)
    }
}

/// Convert a resolved [`ServiceInfo`] into a safe [`ServiceEndpoint`], dropping
/// malformed records (they are untrusted input) rather than propagating them.
fn endpoint_from_info(info: &ServiceInfo) -> Option<ServiceEndpoint> {
    let service_type = info.get_type().to_string();
    let hostname = info.get_hostname().trim_end_matches('.').to_string();
    let name = info.get_fullname().split('.').next().unwrap_or("").to_string();
    let port = info.get_port();
    if port == 0 {
        return None;
    }

    // Take the first IPv4 address; skip link-local 0.0.0.0 placeholders.
    let address = info
        .get_addresses()
        .iter()
        .find_map(|ip| match ip {
            std::net::IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    // Advertised version (optional; may be empty and later confirmed by health).
    let version = info
        .get_property_val_str(TXT_KEY_VERSION)
        .map(str::to_string)
        .unwrap_or_default();

    let scheme = "http".to_string();
    let capabilities = Vec::new();

    if name.is_empty() && version.is_empty() && address.is_empty() {
        // Nothing usable; treat as malformed.
        return None;
    }

    Some(ServiceEndpoint {
        name,
        hostname,
        address,
        port,
        scheme,
        service_type,
        version,
        capabilities,
    })
}

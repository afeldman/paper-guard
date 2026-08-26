//! Stable discovery representation and configuration.
//!
//! This module defines the *provider-independent* discovery model. Neither the
//! client nor the CLI is allowed to talk to Avahi, mDNS impl details, or any
//! specific backend — they only ever see a [`ServiceEndpoint`]. Future
//! mechanisms (mDNS, DNS-SD, static config, Kubernetes discovery) all converge
//! on this representation.

use serde::{Deserialize, Serialize};

/// The DNS-SD service type that uniquely identifies Paper Guard.
///
/// This follows the RFC 6763 convention `<service>.<proto>.<domain>` where the
/// `<proto>` label is `_tcp` for TCP services. The service name `_paper-guard`
/// does not collide with any registered IANA service; it is advertised locally
/// and only significant within the multicast domain. The trailing `.local.` is
/// the mDNS domain.
pub const PAPER_GUARD_SERVICE_TYPE: &str = "_paper-guard._tcp";
/// The mDNS browse domain used by [`PAPER_GUARD_SERVICE_TYPE`].
pub const PAPER_GUARD_SERVICE_DOMAIN: &str = "_paper-guard._tcp.local.";

/// A single, safe, verified discovery candidate.
///
/// This is the only object a discoverer (or a consumer such as the CLI) is
/// allowed to hold. It never embeds secrets, never exposes infrastructure-only
/// metadata, and is built purely from *untrusted* wire data that has been
/// validated and — where required — cross-checked against `GET /health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceEndpoint {
    /// Human-friendly instance name, e.g. `paper-guard` or `paper-guard-lab`.
    pub name: String,
    /// The mDNS/DNS-SD hostname, e.g. `paper-guard.local`. This is advisory
    /// only; it is never the sole basis for trusting the service.
    pub hostname: String,
    /// IPv4 address as advertised, e.g. `192.168.1.50`. Empty when unknown.
    pub address: String,
    /// Port the service listens on.
    pub port: u16,
    /// Scheme (`http` or `https`) to reach the service.
    pub scheme: String,
    /// The DNS-SD service type that matched, e.g. `_paper-guard._tcp`.
    pub service_type: String,
    /// Version of the remote service (advertised or verified). Empty when the
    /// version is not yet known.
    pub version: String,
    /// Capabilities the service advertises (e.g. `review`, `memory`, `qdrant`).
    /// Optional and informational only.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl ServiceEndpoint {
    /// Build the base URL for the endpoint from its scheme, address and port.
    ///
    /// Discovery data is untrusted input, so this method sanitises the host
    /// before embedding it: any path, query, fragment, credentials, port, or
    /// `scheme://` a hostile TXT record might smuggle in is stripped. The result
    /// is always exactly `{scheme}://{host}:{port}` with `{scheme}` coming from
    /// our own trusted field, never from the record. Callers can therefore
    /// safely pass it to an HTTP client without risk of URL injection.
    pub fn base_url(&self) -> String {
        let host = if self.address.is_empty() {
            self.hostname.clone()
        } else {
            self.address.clone()
        };
        // Keep only the host portion: drop `scheme://`, userinfo (`user@`),
        // anything from `/`, `:`, `?`, `#`, and surrounding whitespace.
        // 1. Drop any leading `scheme://`.
        let host = host
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        // 2. Take only the authority up to the first `/`, `?`, or `#`.
        let host = host.split(['/', '?', '#']).next().unwrap_or(host);
        // 3. Drop any userinfo (`user:pass@host` up to the last `@`).
        let host = host.rsplit('@').next().unwrap_or(host);
        // 4. Drop any embedded `:port`.
        let host = host.split(':').next().unwrap_or(host);

        // A bare port would produce `http://:8080`; fall back to localhost.
        if host.is_empty() {
            return format!("{}://localhost:{}", self.scheme, self.port);
        }
        format!("{}://{}:{}", self.scheme, host, self.port)
    }
}

/// The discovery policy. Everything is off by default; a user must explicitly
/// opt into discovery so Paper Guard never probes the network implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryMode {
    /// Discovery is disabled entirely; `paper-guard discover` is a no-op
    /// explaining how to enable it, and no network traffic is emitted.
    #[default]
    Off,
    /// Discovery only runs when explicitly requested (`paper-guard discover`).
    /// It lists and verifies services but never selects one automatically or
    /// uploads anything.
    Manual,
    /// Discovery runs when an explicit `--discover` (or equivalent) flag or a
    /// configured `preferred_service` requires it. It may select a service, but
    /// only with explicit user confirmation before any manuscript leaves the
    /// machine.
    Auto,
}

impl DiscoveryMode {
    /// Parse a mode string from a `paper-guard.toml`, failing closed to `Off`
    /// so an unknown or misspelled value can never silently enable discovery.
    pub fn parse(s: &str) -> DiscoveryMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => DiscoveryMode::Manual,
            "auto" => DiscoveryMode::Auto,
            _ => DiscoveryMode::Off,
        }
    }

    /// Whether this mode permits `paper-guard discover` to run at all.
    pub fn permits_discovery(self) -> bool {
        matches!(self, DiscoveryMode::Manual | DiscoveryMode::Auto)
    }
}

/// The `[discovery]` configuration section.
///
/// Defaults are deliberately restrictive: discovery is disabled unless enabled
/// explicitly, and even when enabled, discovery never authorises a manuscript
/// upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoveryConfig {
    /// Master switch. `false` disables all discovery regardless of `mode`.
    pub enabled: bool,
    /// Discovery mode: `off`, `manual`, or `auto`.
    pub mode: String,
    /// Optional DNS-SD service type to browse for. Usually left at the default.
    pub service_type: String,
    /// How long (in ms) to wait for mDNS responses before returning.
    pub timeout_ms: u64,
    /// Optional exact `hostname` (e.g. `paper-guard.lab.local`) that Auto mode
    /// may prefer when multiple services are present. Never "first response
    /// wins".
    pub preferred_service: String,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            enabled: false,
            mode: "off".into(),
            service_type: PAPER_GUARD_SERVICE_DOMAIN.into(),
            timeout_ms: 3000,
            preferred_service: String::new(),
        }
    }
}

impl DiscoveryConfig {
    /// The effective mode, parsed from the string and gated by `enabled`.
    pub fn effective_mode(&self) -> DiscoveryMode {
        if !self.enabled {
            return DiscoveryMode::Off;
        }
        DiscoveryMode::parse(&self.mode)
    }
}

/// DNS-SD TXT key carrying the advertised Paper Guard version.
pub const TXT_KEY_VERSION: &str = "version";
/// DNS-SD TXT key carrying the advertised scheme (`http`/`https`).
pub const TXT_KEY_SCHEME: &str = "scheme";

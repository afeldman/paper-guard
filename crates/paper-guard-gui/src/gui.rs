//! GUI startup logic: bind to localhost, select an available port, serve the
//! combined service+GUI router, print the local URL, and optionally open the
//! default browser.
//!
//! Security constraints:
//! * Default bind is `127.0.0.1`. Never `0.0.0.0`.
//! * No LAN exposure, no firewall change, no admin/root privileges.
//! * If the user passes an explicit bind address that is not loopback and
//!   `allow_external_bind` is not set in the config, startup fails closed.

use std::net::SocketAddr;
use std::sync::Arc;

use paper_guard_app::config::AppConfig;
use paper_guard_service::AppState;

use crate::api::gui_router;

/// Options for starting the local GUI.
#[derive(Debug, Clone)]
pub struct GuiOptions {
    /// Config path (None = use default discovery).
    pub config_path: Option<String>,
    /// Override the bind address. Defaults to the config's `[service].bind`
    /// if set, otherwise `127.0.0.1:0` (auto-pick an available port).
    pub bind: Option<String>,
    /// Whether to try opening the default browser after startup.
    pub open_browser: bool,
}

impl Default for GuiOptions {
    fn default() -> Self {
        GuiOptions {
            config_path: None,
            bind: None,
            open_browser: true,
        }
    }
}

/// Result of starting the GUI.
#[derive(Debug)]
pub struct GuiStartup {
    /// The local URL to print (e.g. `http://127.0.0.1:8080`).
    pub local_url: String,
    /// The bound socket address.
    pub addr: SocketAddr,
    /// The version string printed in the banner.
    pub version: String,
}

impl GuiStartup {
    /// The banner to print at startup.
    pub fn banner(&self) -> String {
        format!(
            "Paper Guard {}\n\nGUI started:\n{}",
            self.version, self.local_url
        )
    }
}

/// Start the local GUI and block until the server exits.
///
/// This binds the combined service + GUI router to a loopback address,
/// prints the local URL, optionally opens the browser, and serves until the
/// process is interrupted.
pub async fn start_gui(opts: &GuiOptions) -> anyhow::Result<GuiStartup> {
    let cfg_path = opts.config_path.as_deref();
    let cfg = AppConfig::load(cfg_path.map(std::path::Path::new))?;
    let data_dir = cfg.service.data_dir.clone();
    let mem = paper_guard_app::MemoryService::from_config(&cfg)?;

    let state = AppState {
        config: Arc::new(cfg.clone()),
        data_dir,
        enforce_loopback: !cfg.service.allow_external_bind,
        memory: mem,
    };

    // Determine the bind address.
    // - Explicit CLI override wins.
    // - Otherwise use the configured bind.
    // - If the configured bind uses port 0 (or wasn't set), the OS picks an
    //   available port.
    let bind_arg = opts
        .bind
        .clone()
        .unwrap_or_else(|| cfg.service.bind.clone());

    // Never bind to 0.0.0.0 by default.
    if !cfg.service.allow_external_bind {
        let host = bind_arg
            .split(':')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']');
        if host == "0.0.0.0" || host == "::" {
            anyhow::bail!(
                "refusing to bind GUI to `{bind_arg}`: external network exposure is disabled \
                 by default. Set [service] allow_external_bind = true to override, or use a \
                 loopback address."
            );
        }
    }

    // Build the combined router: existing service API + GUI routes.
    let service_router = paper_guard_service::app(state.clone());
    let gui = gui_router(state);
    let combined = service_router.merge(gui);

    // Bind the listener.
    let listener = tokio::net::TcpListener::bind(&bind_arg)
        .await
        .map_err(|e| anyhow::anyhow!("unable to bind GUI to `{bind_arg}`: {e}"))?;
    let addr = listener.local_addr()?;

    let is_loopback = addr.ip().is_loopback();
    if !is_loopback && !cfg.service.allow_external_bind {
        anyhow::bail!(
            "refusing to serve GUI on non-loopback address {addr}: unauthenticated external \
             exposure is disabled by default. Set [service] allow_external_bind = true to override."
        );
    }

    let url_host = if addr.ip().is_loopback() {
        match addr.ip() {
            std::net::IpAddr::V6(_) => "[::1]".to_string(),
            _ => "127.0.0.1".to_string(),
        }
    } else {
        addr.ip().to_string()
    };
    let local_url = format!("http://{url_host}:{}", addr.port());

    let startup = GuiStartup {
        local_url: local_url.clone(),
        addr,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    println!("{}", startup.banner());

    if opts.open_browser {
        open_browser(&local_url);
    }

    axum::serve(listener, combined).await?;
    Ok(startup)
}

/// Attempt to open the default browser for the given URL.
///
/// Platform-safe: on macOS uses `open`, on Linux `xdg-open`, on Windows
/// `cmd /c start`. Failures are silently ignored (the user can open the URL
/// manually).
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        // Unsupported platform: do nothing (the URL is printed to the terminal).
    }
}

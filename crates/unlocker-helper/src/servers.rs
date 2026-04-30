//! Spoofing server lifecycle, owned by the helper.
//!
//! The unprivileged main process can't bind ports 53/80/443 on macOS, so we
//! run DNS, HTTP, and HTTPS in the helper (root) and the main process drives
//! us via RPC. Notify channels for "manifest hit" / "firmware streamed" are
//! exposed to the main as blocking RPC ops.

use anyhow::{anyhow, Result};
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use unlocker_core::cert::mint_leaf_from_pem;
use unlocker_core::dns::{self, DnsConfig, DnsHandle};
use unlocker_core::http::{self, ServerConfig, ServerHandles};
use unlocker_core::types::ArmServerSpec;

pub struct ServerSet {
    dns: Option<DnsHandle>,
    http: Option<ServerHandles>,
    pub on_manifest: Arc<Notify>,
    pub on_firmware: Arc<Notify>,
}

#[derive(Default)]
pub struct ServerHolder {
    inner: Mutex<Option<ServerSet>>,
}

impl ServerHolder {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn arm(&self, spec: ArmServerSpec) -> Result<()> {
        let mut guard = self.inner.lock().await;
        if guard.is_some() {
            return Err(anyhow!("servers already armed"));
        }

        let bridge_ip: Ipv4Addr = spec
            .bridge_ip
            .parse()
            .map_err(|_| anyhow!("invalid bridge_ip: {}", spec.bridge_ip))?;

        let host = spec.locale.api_host();
        let leaf = mint_leaf_from_pem(&spec.root_ca_cert_pem, &spec.root_ca_key_pem, &[host])?;

        let dns_cfg = DnsConfig::for_locale(spec.locale, bridge_ip, spec.dns_internal_port);
        let dns_handle = dns::start(dns_cfg).await?;

        let on_manifest = Arc::new(Notify::new());
        let on_firmware = Arc::new(Notify::new());

        let http_cfg = Arc::new(ServerConfig {
            bridge_ip: bridge_ip.to_string(),
            bind_ip: bridge_ip.into(),
            model: spec.model,
            locale: spec.locale,
            firmware_path: spec.firmware_path.into(),
            firmware_size: spec.firmware_size,
            firmware_sha256: spec.firmware_sha256,
            crosspoint_version: spec.crosspoint_version,
            change_log: spec.change_log,
            root_ca_pem: spec.root_ca_cert_pem.clone(),
            on_manifest_request: on_manifest.clone(),
            on_firmware_streamed: on_firmware.clone(),
        });
        let http_handles = http::start(http_cfg, &spec.root_ca_cert_pem, &leaf).await?;

        *guard = Some(ServerSet {
            dns: Some(dns_handle),
            http: Some(http_handles),
            on_manifest,
            on_firmware,
        });
        tracing::info!("spoofing servers armed");
        Ok(())
    }

    pub async fn disarm(&self) -> Result<()> {
        let mut guard = self.inner.lock().await;
        if let Some(mut set) = guard.take() {
            if let Some(h) = set.http.take() {
                h.shutdown().await;
            }
            if let Some(d) = set.dns.take() {
                d.shutdown().await;
            }
            tracing::info!("spoofing servers disarmed");
        }
        Ok(())
    }

    pub async fn manifest_notify(&self) -> Option<Arc<Notify>> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.on_manifest.clone())
    }

    pub async fn firmware_notify(&self) -> Option<Arc<Notify>> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.on_firmware.clone())
    }
}

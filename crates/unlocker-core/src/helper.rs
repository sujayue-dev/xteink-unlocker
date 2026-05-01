//! Typed JSON-RPC client for the privileged helper.
//!
//! Each call opens a fresh Unix-socket connection so that long-blocking ops
//! (WaitManifest / WaitFirmware) don't tie up other RPCs.

use crate::types::ArmServerSpec;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub fn socket_path() -> PathBuf {
    PathBuf::from("/var/run/com.sofriendly.crosspoint.unlocker.helper.sock")
}

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping,
    IsEnable { ssid: String, psk: String },
    IsDisable,
    PfctlAdd { from_port: u16, to_port: u16 },
    PfctlRemove,
    DhcpdRead,
    BridgeIp,
    FullCleanup,
    ArmServers(ArmServerSpec),
    WaitManifest,
    WaitFirmware,
    DisarmServers,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Response {
    Ok { data: serde_json::Value },
    Err { error: String },
}

#[derive(Debug, Deserialize, Clone)]
pub struct DhcpLease {
    pub ip: String,
    pub mac: String,
    pub name: Option<String>,
}

pub struct Helper;

impl Helper {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    async fn one_shot(&self, req: Request) -> Result<serde_json::Value> {
        let stream = UnixStream::connect(socket_path())
            .await
            .with_context(|| format!("connecting to helper at {}", socket_path().display()))?;
        let (r, mut w) = stream.into_split();
        let mut bytes = serde_json::to_vec(&req)?;
        bytes.push(b'\n');
        w.write_all(&bytes).await?;
        // Half-close the write side so the helper sees EOF after one request.
        // Not strictly needed (helper handles per-line) but cleaner.
        drop(w);
        let mut lines = BufReader::new(r).lines();
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("helper closed connection"))?;
        let resp: Response = serde_json::from_str(&line)?;
        match resp {
            Response::Ok { data } => Ok(data),
            Response::Err { error } => Err(anyhow!("helper: {error}")),
        }
    }

    pub async fn ping(&self) -> Result<()> {
        self.one_shot(Request::Ping).await.map(|_| ())
    }

    pub async fn is_enable(&self, ssid: &str, psk: &str) -> Result<()> {
        self.one_shot(Request::IsEnable {
            ssid: ssid.into(),
            psk: psk.into(),
        })
        .await
        .map(|_| ())
    }

    pub async fn is_disable(&self) -> Result<()> {
        self.one_shot(Request::IsDisable).await.map(|_| ())
    }

    pub async fn pfctl_add(&self, from: u16, to: u16) -> Result<()> {
        self.one_shot(Request::PfctlAdd {
            from_port: from,
            to_port: to,
        })
        .await
        .map(|_| ())
    }

    pub async fn pfctl_remove(&self) -> Result<()> {
        self.one_shot(Request::PfctlRemove).await.map(|_| ())
    }

    pub async fn dhcpd_read(&self) -> Result<Vec<DhcpLease>> {
        let v = self.one_shot(Request::DhcpdRead).await?;
        let leases = v.get("leases").cloned().unwrap_or(serde_json::json!([]));
        Ok(serde_json::from_value(leases)?)
    }

    pub async fn bridge_ip(&self) -> Result<String> {
        let v = self.one_shot(Request::BridgeIp).await?;
        Ok(v.get("ip")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("no ip in response"))?
            .to_string())
    }

    pub async fn full_cleanup(&self) -> Result<()> {
        self.one_shot(Request::FullCleanup).await.map(|_| ())
    }

    pub async fn arm_servers(&self, spec: ArmServerSpec) -> Result<()> {
        self.one_shot(Request::ArmServers(spec)).await.map(|_| ())
    }

    pub async fn disarm_servers(&self) -> Result<()> {
        self.one_shot(Request::DisarmServers).await.map(|_| ())
    }

    /// Blocks until the device's first manifest request hits the helper's
    /// HTTP server. Uses its own connection so other RPCs can run concurrently.
    pub async fn wait_manifest(&self) -> Result<()> {
        self.one_shot(Request::WaitManifest).await.map(|_| ())
    }

    pub async fn wait_firmware(&self) -> Result<()> {
        self.one_shot(Request::WaitFirmware).await.map(|_| ())
    }
}

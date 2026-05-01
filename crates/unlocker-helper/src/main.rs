//! Privileged helper for Xteink Unlocker.
//!
//! Runs as root (LaunchDaemon, registered via SMAppService) and exposes a
//! tiny JSON-RPC protocol over a Unix domain socket. Owns:
//!   * macOS network state (Internet Sharing, pfctl, dhcpd_leases).
//!   * The spoofing servers (DNS / HTTP / HTTPS) bound to privileged ports.
//!
//! The unprivileged main process drives us via RPC; we never run untrusted
//! code from the main.

mod ops;
mod proto;
mod servers;
mod state;

use proto::{Request, Response};
use servers::ServerHolder;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

fn socket_path() -> PathBuf {
    PathBuf::from("/var/run/com.sofriendly.crosspoint.unlocker.helper.sock")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let log_file = std::fs::File::create("/tmp/unlocker-helper.log")
        .expect("cannot create /tmp/unlocker-helper.log");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .init();

    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let listener = UnixListener::bind(&path)?;

    let mut perm = std::fs::metadata(&path)?.permissions();
    perm.set_mode(0o666);
    std::fs::set_permissions(&path, perm)?;

    tracing::info!(?path, "helper listening");

    if let Err(e) = state::recover().await {
        tracing::warn!(?e, "state recovery had issues");
    }

    let servers = ServerHolder::new();

    loop {
        let (stream, _) = listener.accept().await?;
        let s = servers.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, s).await {
                tracing::warn!(?e, "client disconnected");
            }
        });
    }
}

async fn handle(stream: UnixStream, servers: Arc<ServerHolder>) -> anyhow::Result<()> {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => dispatch(req, &servers).await,
            Err(e) => Response::Err {
                error: format!("bad request: {e}"),
            },
        };
        let mut bytes = serde_json::to_vec(&resp)?;
        bytes.push(b'\n');
        w.write_all(&bytes).await?;
    }
    Ok(())
}

async fn dispatch(req: Request, servers: &ServerHolder) -> Response {
    let result: anyhow::Result<serde_json::Value> = match req {
        Request::Ping => Ok(serde_json::json!({"pong": true})),

        Request::IsEnable { ssid, psk } => ops::is_enable(&ssid, &psk)
            .await
            .map(|_| serde_json::json!({"ssid": ssid})),
        Request::IsDisable => ops::is_disable().await.map(|_| serde_json::json!({})),
        Request::PfctlAdd { from_port, to_port } => ops::pfctl_add(from_port, to_port)
            .await
            .map(|_| serde_json::json!({"from": from_port, "to": to_port})),
        Request::PfctlRemove => ops::pfctl_remove().await.map(|_| serde_json::json!({})),
        Request::DhcpdRead => ops::dhcpd_read()
            .await
            .map(|leases| serde_json::json!({ "leases": leases })),
        Request::BridgeIp => ops::bridge_ip()
            .await
            .map(|ip| serde_json::json!({ "ip": ip })),
        Request::FullCleanup => {
            servers.disarm().await.ok();
            ops::full_cleanup().await.map(|_| serde_json::json!({}))
        }

        Request::ArmServers(spec) => servers.arm(spec).await.map(|_| serde_json::json!({})),
        Request::DisarmServers => servers.disarm().await.map(|_| serde_json::json!({})),
        Request::WaitManifest => match servers.manifest_notify().await {
            Some(n) => {
                n.notified().await;
                Ok(serde_json::json!({"event": "manifest_request"}))
            }
            None => Err(anyhow::anyhow!("servers not armed")),
        },
        Request::WaitFirmware => match servers.firmware_notify().await {
            Some(n) => {
                n.notified().await;
                Ok(serde_json::json!({"event": "firmware_streamed"}))
            }
            None => Err(anyhow::anyhow!("servers not armed")),
        },
    };

    match result {
        Ok(data) => Response::Ok { data },
        Err(e) => Response::Err {
            error: format!("{e:#}"),
        },
    }
}

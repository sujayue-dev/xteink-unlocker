//! Privileged operations. Pragmatic shell-outs to system tools.
//!
//! Every state-changing op records its intent in the state file *before*
//! acting, so a crash mid-op is recoverable on next launch.

use crate::proto::DhcpLease;
use crate::state;
use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use tokio::process::Command;

const NAT_PLIST: &str = "/Library/Preferences/SystemConfiguration/com.apple.nat.plist";
const NAT_PLIST_BACKUP: &str = "/var/db/com.sofriendly.crosspoint.unlocker.nat.plist.bak";
const PF_ANCHOR_NAME: &str = "com.sofriendly.crosspoint.unlocker";
const PF_RULES_PATH: &str = "/var/db/com.sofriendly.crosspoint.unlocker.pf.conf";

async fn sh(prog: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(prog).args(args).output().await
        .with_context(|| format!("spawn {prog} {args:?}"))?;
    if !out.status.success() {
        bail!(
            "{prog} {args:?} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── feth (synthetic upstream interface) ──────────────────────────────────────

pub async fn feth_create(name: &str, ip: &str, prefix: u8) -> Result<()> {
    state::mutate(|s| s.feth_interface = Some(name.to_string())).await?;
    // Create. If it already exists (recovery), ifconfig errors but we proceed.
    let _ = sh("ifconfig", &[name, "create"]).await;
    sh("ifconfig", &[name, "inet", &format!("{ip}/{prefix}"), "up"]).await?;
    tracing::info!(%name, %ip, prefix, "feth created");
    Ok(())
}

pub async fn feth_destroy(name: &str) -> Result<()> {
    let _ = sh("ifconfig", &[name, "destroy"]).await;
    state::mutate(|s| s.feth_interface = None).await?;
    tracing::info!(%name, "feth destroyed");
    Ok(())
}

// ── Internet Sharing ─────────────────────────────────────────────────────────

pub async fn is_enable(upstream: &str, ssid: &str, psk: &str) -> Result<()> {
    // Back up the existing plist if we haven't already.
    if Path::new(NAT_PLIST).exists() && !Path::new(NAT_PLIST_BACKUP).exists() {
        tokio::fs::copy(NAT_PLIST, NAT_PLIST_BACKUP).await.ok();
    }

    write_nat_plist(upstream, ssid, psk).await?;
    state::mutate(|s| s.internet_sharing_active = true).await?;

    // Modern macOS: `launchctl kickstart -k system/com.apple.InternetSharing`
    // restarts the daemon, picking up the new plist.
    let _ = sh("launchctl", &["kickstart", "-k", "system/com.apple.InternetSharing"]).await;
    // Older fallback if kickstart didn't work:
    let _ = sh(
        "launchctl",
        &["load", "-w", "/System/Library/LaunchDaemons/com.apple.InternetSharing.plist"],
    )
    .await;

    tracing::info!(%upstream, %ssid, "Internet Sharing enabled");
    Ok(())
}

pub async fn is_disable() -> Result<()> {
    let _ = sh(
        "launchctl",
        &["unload", "/System/Library/LaunchDaemons/com.apple.InternetSharing.plist"],
    )
    .await;
    let _ = sh("launchctl", &["bootout", "system/com.apple.InternetSharing"]).await;

    // Restore the prior plist if we backed one up.
    if Path::new(NAT_PLIST_BACKUP).exists() {
        tokio::fs::rename(NAT_PLIST_BACKUP, NAT_PLIST).await.ok();
    } else {
        tokio::fs::remove_file(NAT_PLIST).await.ok();
    }

    state::mutate(|s| s.internet_sharing_active = false).await?;
    tracing::info!("Internet Sharing disabled");
    Ok(())
}

async fn write_nat_plist(upstream: &str, ssid: &str, psk: &str) -> Result<()> {
    use plist::Value;
    let mut airport = plist::Dictionary::new();
    airport.insert("40BitEncrypt".into(), Value::Integer(0i64.into()));
    airport.insert("Channel".into(), Value::Integer(11i64.into()));
    airport.insert("Enabled".into(), Value::Integer(1i64.into()));
    airport.insert("NetworkName".into(), Value::String(ssid.to_string()));
    airport.insert(
        "NetworkPassword".into(),
        Value::Data(psk.as_bytes().to_vec()),
    );

    let mut nat = plist::Dictionary::new();
    nat.insert("Enabled".into(), Value::Integer(1i64.into()));
    nat.insert(
        "SharingDevices".into(),
        Value::Array(vec![Value::String("en0".into())]),
    );
    nat.insert(
        "PrimaryInterface".into(),
        Value::Dictionary({
            let mut p = plist::Dictionary::new();
            p.insert("Device".into(), Value::String(upstream.to_string()));
            p.insert("Enabled".into(), Value::Integer(1i64.into()));
            p
        }),
    );
    nat.insert("AirPort".into(), Value::Dictionary(airport));

    let mut root = plist::Dictionary::new();
    root.insert("NAT".into(), Value::Dictionary(nat));

    let bytes = {
        let mut buf = Vec::new();
        plist::to_writer_xml(&mut buf, &Value::Dictionary(root))?;
        buf
    };
    if let Some(parent) = Path::new(NAT_PLIST).parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(NAT_PLIST, bytes).await?;
    Ok(())
}

// ── pfctl (DNS port redirect) ────────────────────────────────────────────────

pub async fn pfctl_add(from_port: u16, to_port: u16) -> Result<()> {
    let rules = format!(
        "rdr pass on bridge100 inet proto udp from any to any port {from} -> 127.0.0.1 port {to}\n\
         rdr pass on bridge100 inet proto tcp from any to any port {from} -> 127.0.0.1 port {to}\n",
        from = from_port,
        to = to_port,
    );
    tokio::fs::write(PF_RULES_PATH, rules).await?;

    state::mutate(|s| s.pfctl_anchor_loaded = true).await?;

    sh(
        "pfctl",
        &["-a", PF_ANCHOR_NAME, "-f", PF_RULES_PATH],
    )
    .await?;
    // Enable pf if not already enabled.
    let _ = sh("pfctl", &["-E"]).await;
    tracing::info!(from_port, to_port, "pfctl anchor loaded");
    Ok(())
}

pub async fn pfctl_remove() -> Result<()> {
    let _ = sh("pfctl", &["-a", PF_ANCHOR_NAME, "-F", "all"]).await;
    tokio::fs::remove_file(PF_RULES_PATH).await.ok();
    state::mutate(|s| s.pfctl_anchor_loaded = false).await?;
    tracing::info!("pfctl anchor flushed");
    Ok(())
}

// ── DHCP leases ──────────────────────────────────────────────────────────────

pub async fn dhcpd_read() -> Result<Vec<DhcpLease>> {
    let path = "/var/db/dhcpd_leases";
    let body = match tokio::fs::read_to_string(path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    Ok(parse_dhcpd_leases(&body))
}

fn parse_dhcpd_leases(s: &str) -> Vec<DhcpLease> {
    // Apple's bootpd writes "key=value" lines inside { ... } blocks.
    let mut out = Vec::new();
    let mut cur: Option<(Option<String>, Option<String>, Option<String>)> = None;
    for line in s.lines() {
        let line = line.trim();
        if line == "{" {
            cur = Some((None, None, None));
        } else if line == "}" {
            if let Some((Some(ip), Some(mac), name)) = cur.take() {
                out.push(DhcpLease { ip, mac, name });
            } else {
                cur = None;
            }
        } else if let Some((ref mut ip, ref mut mac, ref mut name)) = cur {
            if let Some(v) = line.strip_prefix("ip_address=") {
                *ip = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("hw_address=") {
                // typical format: "1,aa:bb:cc:dd:ee:ff"
                let mac_only = v.trim().split(',').last().unwrap_or("").to_string();
                *mac = Some(mac_only);
            } else if let Some(v) = line.strip_prefix("name=") {
                *name = Some(v.trim().to_string());
            }
        }
    }
    out
}

// ── bridge IP discovery ──────────────────────────────────────────────────────

pub async fn bridge_ip() -> Result<String> {
    let out = sh("ifconfig", &["bridge100"]).await?;
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("inet ") {
            // "inet 192.168.2.1 netmask 0xffffff00 broadcast ..."
            if let Some(ip) = rest.split_whitespace().next() {
                return Ok(ip.to_string());
            }
        }
    }
    Err(anyhow!("bridge100 has no IPv4 address yet"))
}

// ── full cleanup ─────────────────────────────────────────────────────────────

pub async fn full_cleanup() -> Result<()> {
    let s = state::read().await.unwrap_or_default();
    if s.pfctl_anchor_loaded {
        let _ = pfctl_remove().await;
    }
    if s.internet_sharing_active {
        let _ = is_disable().await;
    }
    if let Some(name) = s.feth_interface {
        let _ = feth_destroy(&name).await;
    }
    Ok(())
}

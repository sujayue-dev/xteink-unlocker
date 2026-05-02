//! Windows privileged ops.
//!
//! Hotspot bring-up/teardown drives the WinRT `NetworkOperatorTetheringManager`
//! via an embedded PowerShell snippet — far less Rust glue than binding the
//! WinRT projections directly, and well-trodden territory for "start Mobile
//! Hotspot from a script". Windows handles NAT + DHCP itself; the host always
//! ends up at 192.168.137.1.
//!
//! Device discovery scans the system ARP table (`arp -a`) under the hotspot's
//! interface heading.

use crate::proto::DhcpLease;
use crate::state;
use anyhow::{anyhow, bail, Context, Result};
use tokio::process::Command;

async fn run_powershell(script: &str) -> Result<String> {
    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .await
        .context("spawning powershell.exe")?;
    if !out.status.success() {
        bail!(
            "powershell failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── Mobile Hotspot ───────────────────────────────────────────────────────────

/// PowerShell helper that resolves the WinRT IAsyncOperation pattern by
/// finding the right AsTask overload and waiting on it synchronously.
const AWAIT_HELPER: &str = r#"
function Await($WinRtTask, $ResultType) {
    $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() |
        Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}
"#;

pub async fn is_enable(ssid: &str, psk: &str) -> Result<()> {
    if ssid.is_empty() {
        bail!("ssid empty");
    }
    if psk.len() < 8 {
        bail!("psk must be ≥ 8 chars (Windows hotspot rule)");
    }

    // Pass SSID/PSK via env so we don't have to escape into the PowerShell
    // string. PowerShell reads them from $env:.
    let script = format!(
        r#"
{await_helper}

[void][Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType=WindowsRuntime]
[void][Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType=WindowsRuntime]
[void][Windows.Networking.NetworkOperators.NetworkOperatorTetheringAccessPointConfiguration, Windows.Networking.NetworkOperators, ContentType=WindowsRuntime]

$profile = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]::GetInternetConnectionProfile()
if ($null -eq $profile) {{ throw 'no active internet connection profile — Windows Mobile Hotspot needs an upstream to share' }}

$mgr = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking.NetworkOperators,ContentType=WindowsRuntime]::CreateFromConnectionProfile($profile)
$cfg = $mgr.GetCurrentAccessPointConfiguration()
$cfg.Ssid = $env:UNLOCKER_SSID
$cfg.Passphrase = $env:UNLOCKER_PSK
$null = Await ($mgr.ConfigureAccessPointAsync($cfg)) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])

if ($mgr.TetheringOperationalState -ne 'On') {{
    $null = Await ($mgr.StartTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
}}
Write-Output 'ok'
"#,
        await_helper = AWAIT_HELPER
    );

    state::mutate(|s| s.internet_sharing_active = true).await?;

    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .env("UNLOCKER_SSID", ssid)
        .env("UNLOCKER_PSK", psk)
        .output()
        .await
        .context("spawning powershell.exe")?;
    if !out.status.success() {
        bail!(
            "tether start failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    tracing::info!(%ssid, "Mobile Hotspot configured and started");
    Ok(())
}

pub async fn is_disable() -> Result<()> {
    let script = format!(
        r#"
{await_helper}

[void][Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType=WindowsRuntime]
[void][Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType=WindowsRuntime]

$profile = [Windows.Networking.Connectivity.NetworkInformation,Windows.Networking.Connectivity,ContentType=WindowsRuntime]::GetInternetConnectionProfile()
if ($null -ne $profile) {{
    $mgr = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager,Windows.Networking.NetworkOperators,ContentType=WindowsRuntime]::CreateFromConnectionProfile($profile)
    if ($mgr.TetheringOperationalState -eq 'On') {{
        $null = Await ($mgr.StopTetheringAsync()) ([Windows.Networking.NetworkOperators.NetworkOperatorTetheringOperationResult])
    }}
}}
Write-Output 'ok'
"#,
        await_helper = AWAIT_HELPER
    );

    let _ = run_powershell(&script).await;
    state::mutate(|s| s.internet_sharing_active = false).await?;
    tracing::info!("Mobile Hotspot stopped");
    Ok(())
}

// ── Port redirection (no-op on Windows) ──────────────────────────────────────

// On macOS, Internet Sharing owns port 53 and we have to rdr around it. On
// Windows, the helper binds the spoofing servers directly to 192.168.137.1
// on the privileged ports — no firewall rewriting needed. We keep the RPC
// methods so the cross-platform protocol stays identical.

pub async fn pfctl_add(_from_port: u16, _to_port: u16) -> Result<()> {
    Ok(())
}

pub async fn pfctl_remove() -> Result<()> {
    Ok(())
}

// ── Bridge IP ────────────────────────────────────────────────────────────────

pub async fn bridge_ip() -> Result<String> {
    // Windows Mobile Hotspot always uses 192.168.137.1. Sanity-check the
    // adapter is actually present so we fail fast if the hotspot didn't come up.
    let script = r#"
$adapter = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
    Where-Object { $_.IPAddress -like '192.168.137.*' } | Select-Object -First 1
if ($adapter) { Write-Output $adapter.IPAddress } else { Write-Output 'none' }
"#;
    let out = run_powershell(script).await?;
    let ip = out.trim();
    if ip == "none" || ip.is_empty() {
        return Err(anyhow!("hotspot adapter not yet present (192.168.137.0/24)"));
    }
    Ok(ip.to_string())
}

// ── DHCP leases (via ARP table) ──────────────────────────────────────────────

pub async fn dhcpd_read() -> Result<Vec<DhcpLease>> {
    let out = Command::new("arp").arg("-a").output().await?;
    if !out.status.success() {
        return Ok(vec![]);
    }
    let body = String::from_utf8_lossy(&out.stdout);
    Ok(parse_arp_output(&body))
}

fn parse_arp_output(s: &str) -> Vec<DhcpLease> {
    // arp -a output:
    //   Interface: 192.168.137.1 --- 0xN
    //     Internet Address      Physical Address      Type
    //     192.168.137.32        aa-bb-cc-dd-ee-ff     dynamic
    //     192.168.137.255       ff-ff-ff-ff-ff-ff     static
    let mut out = Vec::new();
    let mut in_hotspot_section = false;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Interface:") {
            in_hotspot_section = trimmed.contains("192.168.137.");
            continue;
        }
        if !in_hotspot_section {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let (Some(ip), Some(mac), Some(kind)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        // Skip the host entry, broadcast, multicast.
        if ip == "192.168.137.1" || ip.ends_with(".255") || kind == "static" {
            continue;
        }
        if !ip.starts_with("192.168.137.") {
            continue;
        }
        // Normalize MAC from aa-bb-cc-dd-ee-ff to aa:bb:cc:dd:ee:ff for parity
        // with macOS dhcpd output, so consumers can match either.
        let mac_norm = mac.replace('-', ":").to_lowercase();
        out.push(DhcpLease {
            ip: ip.to_string(),
            mac: mac_norm,
            name: None,
        });
    }
    out
}

// ── Full cleanup ─────────────────────────────────────────────────────────────

pub async fn full_cleanup() -> Result<()> {
    let s = state::read().await.unwrap_or_default();
    if s.internet_sharing_active {
        let _ = is_disable().await;
    }
    state::mutate(|s| {
        s.internet_sharing_active = false;
        s.pfctl_anchor_loaded = false;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arp_output() {
        let sample = r#"
Interface: 192.168.137.1 --- 0xa
  Internet Address      Physical Address      Type
  192.168.137.32        aa-bb-cc-dd-ee-ff     dynamic
  192.168.137.255       ff-ff-ff-ff-ff-ff     static
  224.0.0.22            01-00-5e-00-00-16     static

Interface: 10.0.0.5 --- 0xb
  Internet Address      Physical Address      Type
  10.0.0.1              11-22-33-44-55-66     dynamic
"#;
        let leases = parse_arp_output(sample);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].ip, "192.168.137.32");
        assert_eq!(leases[0].mac, "aa:bb:cc:dd:ee:ff");
    }
}

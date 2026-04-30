use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use unlocker_core::helper::Helper;
use unlocker_core::orchestrator::{Orchestrator, State as OrchState};
use unlocker_core::runtime::{await_espressif_lease, ArmConfig, Runtime};
use unlocker_core::types::{Catalog, CrossPointRelease, Locale, Model, Selection};
use unlocker_core::{catalog, session::SessionLog, types::LogEntry};

struct AppState {
    orch: Arc<Orchestrator>,
    log: Arc<SessionLog>,
    http: reqwest::Client,
    helper: Arc<Helper>,
    runtime: Arc<Runtime>,
}

#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<OrchState, String> {
    Ok(state.orch.current_state().await)
}

#[derive(serde::Serialize)]
struct SessionInfo {
    model: Option<Model>,
    locale: Option<Locale>,
    release_id: Option<String>,
    bridge_ip: Option<String>,
    ssid: Option<String>,
    psk: Option<String>,
    device_ip: Option<String>,
}

#[tauri::command]
async fn get_session(state: State<'_, AppState>) -> Result<SessionInfo, String> {
    let d = state.orch.data().await;
    Ok(SessionInfo {
        model: d.model,
        locale: d.locale,
        release_id: d.selection.as_ref().map(|s| s.release_id.clone()),
        bridge_ip: d.bridge_ip,
        ssid: d.ssid,
        psk: d.psk,
        device_ip: d.device_ip,
    })
}

#[tauri::command]
async fn fetch_catalog(state: State<'_, AppState>) -> Result<Catalog, String> {
    state.log.push("info", "fetching catalog", None).await;
    match catalog::fetch_catalog(&state.http).await {
        Ok(c) => Ok(c),
        Err(e) => {
            state
                .log
                .push("warn", format!("catalog fetch failed, using stub: {e}"), None)
                .await;
            Ok(catalog::stub_catalog())
        }
    }
}

#[tauri::command]
async fn check_helper(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.helper.ping().await.is_ok())
}

#[derive(serde::Serialize)]
struct HelperStatus {
    installed: bool,
    status_label: String,
    socket_reachable: bool,
}

#[tauri::command]
async fn helper_status(state: State<'_, AppState>) -> Result<HelperStatus, String> {
    let socket_reachable = state.helper.ping().await.is_ok();
    Ok(HelperStatus {
        installed: socket_reachable,
        status_label: if socket_reachable { "running" } else { "not_running" }.into(),
        socket_reachable,
    })
}

#[tauri::command]
async fn install_helper(app: AppHandle) -> Result<(), String> {
    let helper_path = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("can't resolve bundle path")?
        .join("MacOS/unlocker-helper");

    let path_str = helper_path
        .to_str()
        .ok_or("non-utf8 helper path")?
        .replace('\'', "'\\''");

    // Kill any stale helper from a previous run, then start fresh.
    // Use pkill with the exact binary name (not -f) to avoid matching the shell itself.
    let script = format!(
        "do shell script \"pkill unlocker-helper 2>/dev/null; sleep 1; '{path_str}' &> /dev/null &\" with administrator privileges"
    );

    let status = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .status()
        .await
        .map_err(|e| format!("failed to run osascript: {e}"))?;

    if !status.success() {
        return Err("user cancelled or authorization failed".into());
    }

    // Wait for the helper to start listening on the socket.
    let helper = Helper::new();
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if helper.ping().await.is_ok() {
            return Ok(());
        }
    }
    Err("helper started but socket not reachable after 5s".into())
}

#[tauri::command]
async fn uninstall_helper() -> Result<(), String> {
    let script = "do shell script \"pkill -f unlocker-helper\" with administrator privileges";
    tokio::process::Command::new("osascript")
        .args(["-e", script])
        .status()
        .await
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn accept_consent(
    state: State<'_, AppState>,
    general: bool,
    recovery: bool,
) -> Result<(), String> {
    state.orch.set_consent(general, recovery).await;
    state
        .orch
        .transition(OrchState::SelectingDeviceAndRegion, None)
        .await;
    Ok(())
}

#[tauri::command]
async fn select_device(
    state: State<'_, AppState>,
    model: Model,
    locale: Locale,
) -> Result<(), String> {
    state.orch.set_device(model, locale).await;
    state
        .orch
        .transition(OrchState::SelectingFirmware, None)
        .await;
    Ok(())
}

#[tauri::command]
async fn select_firmware(
    state: State<'_, AppState>,
    selection: Selection,
) -> Result<(), String> {
    state.orch.set_selection(selection.clone()).await;
    state
        .orch
        .transition(OrchState::DownloadingFirmware, None)
        .await;

    let orch = state.orch.clone();
    let log = state.log.clone();
    let http = state.http.clone();
    let runtime = state.runtime.clone();
    let helper = state.helper.clone();

    tokio::spawn(async move {
        if let Err(e) = run_install(orch.clone(), log, http, runtime, helper, selection).await
        {
            orch.fail(format!("{e:#}")).await;
        }
    });

    Ok(())
}

async fn run_install(
    orch: Arc<Orchestrator>,
    log: Arc<SessionLog>,
    http: reqwest::Client,
    runtime: Arc<Runtime>,
    helper: Arc<Helper>,
    selection: Selection,
) -> anyhow::Result<()> {
    // ── Locate + cache + download firmware ──
    let cat = catalog::fetch_catalog(&http)
        .await
        .unwrap_or_else(|_| catalog::stub_catalog());
    let release = cat
        .releases
        .into_iter()
        .find(|r| r.id == selection.release_id)
        .ok_or_else(|| anyhow::anyhow!("selected release not found"))?;

    let (path, sha) = if let Some(sha) = release.firmware_sha256.as_deref() {
        if let Some(p) = catalog::cached_path(sha)? {
            if catalog::verify_file(&p, sha).unwrap_or(false) {
                log.push("info", "firmware cache hit", None).await;
                (p, sha.to_string())
            } else {
                catalog::download_firmware(&http, &release, |_, _| {}).await?
            }
        } else {
            catalog::download_firmware(&http, &release, |_, _| {}).await?
        }
    } else {
        catalog::download_firmware(&http, &release, |_, _| {}).await?
    };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(release.size);
    orch.set_firmware(path.to_string_lossy().into(), sha.clone()).await;

    // ── Hotspot ──
    orch.transition(OrchState::SettingUpHotspot, None).await;
    let ssid = "CrossPoint-Setup".to_string();
    let psk = format!("xtu-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    log.push("info", "configuring Internet Sharing", None).await;
    if let Err(e) = runtime.prepare_hotspot(&helper, &ssid, &psk).await {
        log.push("error", format!("Internet Sharing setup failed: {e:#}"), None).await;
        return Err(e.into());
    }
    log.push("info", "ready — enable Internet Sharing in System Settings", None).await;

    // Wait for the user to enable Internet Sharing in System Settings.
    orch.transition(OrchState::WaitingForInternetSharing, None).await;
    let info = match runtime.await_hotspot(&helper, &ssid, &psk, Duration::from_secs(300)).await {
        Ok(info) => info,
        Err(e) => {
            log.push("error", format!("bridge100 timeout: {e:#}"), None).await;
            return Err(e);
        }
    };
    log.push("info", format!("hotspot up — bridge at {}", info.bridge_ip), None).await;
    orch.set_hotspot(info.ssid, info.psk, info.bridge_ip.to_string()).await;

    // ── Arm DNS + HTTP + HTTPS immediately so they're ready before any
    //    device connects. The device may check for updates the moment it
    //    joins the network. ──
    let bridge_ip: std::net::Ipv4Addr = info.bridge_ip;
    let arm_cfg = ArmConfig {
        bridge_ip,
        model: orch.data().await.model.unwrap(),
        locale: orch.data().await.locale.unwrap(),
        firmware_path: path,
        firmware_size: size,
        firmware_sha256: sha,
        crosspoint_version: release.version.clone(),
        change_log: render_changelog(&release),
    };
    runtime.arm(&helper, arm_cfg).await?;
    log.push("info", "DNS + HTTP + HTTPS servers armed", None).await;

    orch.transition(OrchState::AwaitingClient, None).await;

    // ── Wait for device to join ──
    let (mac, ip) = await_espressif_lease(&helper, Duration::from_secs(300)).await?;
    log.push("info", format!("device joined: {mac} -> {ip}"), None)
        .await;
    orch.set_device_ip(ip).await;

    // Servers are already armed. We block here until the helper reports
    // the manifest request.
    orch.transition(OrchState::AwaitingDeviceRequest, None).await;
    log.push("info", "armed; waiting for device check-update", None).await;
    helper.wait_manifest().await?;
    log.push("info", "device fetched manifest", None).await;
    orch.transition(OrchState::Serving, Some("Manifest served".into()))
        .await;

    helper.wait_firmware().await?;
    log.push("info", "firmware streamed", None).await;
    orch.transition(OrchState::Flashing, Some("Streaming firmware".into()))
        .await;

    // The device will reboot on its own. Give it a moment, then advance.
    tokio::time::sleep(Duration::from_secs(10)).await;
    orch.transition(OrchState::Verifying, None).await;

    Ok(())
}

fn render_changelog(release: &CrossPointRelease) -> String {
    format!(
        "Installing CrossPoint Reader {ver}\n\n\
         This update replaces the stock Xteink firmware with CrossPoint, an open-source firmware with more features and full local control.\n\n\
         Highlights:\n{notes}\n\n\
         Learn more: https://crosspointreader.com\n\n\
         If you change your mind, you can restore stock firmware via the WebSerial flasher.",
        ver = release.version,
        notes = release.notes,
    )
}

#[tauri::command]
async fn confirm_running(state: State<'_, AppState>) -> Result<(), String> {
    state.orch.transition(OrchState::Done, None).await;
    state
        .runtime
        .teardown(&state.helper)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn cancel(state: State<'_, AppState>) -> Result<(), String> {
    let _ = state.runtime.teardown(&state.helper).await;
    state.orch.cleanup().await;
    Ok(())
}

#[tauri::command]
async fn get_logs(state: State<'_, AppState>) -> Result<Vec<LogEntry>, String> {
    Ok(state.log.snapshot().await)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let orch = Orchestrator::new();
    let log = SessionLog::new(500);
    let http = reqwest::Client::builder()
        .user_agent("XteinkUnlocker/0.1")
        .build()
        .expect("reqwest");
    let helper = Helper::new();
    let runtime = Runtime::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            orch: orch.clone(),
            log: log.clone(),
            http,
            helper: helper.clone(),
            runtime: runtime.clone(),
        })
        .setup(move |app| {
            let handle: AppHandle = app.handle().clone();

            let mut rx = orch.subscribe();
            let h2 = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(ev) = rx.recv().await {
                    let _ = h2.emit("state-changed", &ev);
                }
            });

            let mut lr = log.subscribe();
            let h3 = handle.clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(entry) = lr.recv().await {
                    let _ = h3.emit("log", &entry);
                }
            });

            let o = orch.clone();
            tauri::async_runtime::spawn(async move {
                o.transition(OrchState::Consenting, None).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_session,
            fetch_catalog,
            check_helper,
            helper_status,
            install_helper,
            uninstall_helper,
            accept_consent,
            select_device,
            select_firmware,
            confirm_running,
            cancel,
            get_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

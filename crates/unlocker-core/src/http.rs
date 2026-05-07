//! Manifest server.
//!
//! Two listeners on the bridge IP:
//!   * port 80   — plain HTTP. Serves the stock updater path.
//!   * port 443  — HTTPS with a self-signed cert for the spoofed API hostname.
//!                 Handles the CrossPoint GitHub API spoof and firmware asset.
//!
//! The CrossPoint OTA path should look like a plain fixed-length HTTPS asset
//! download, not a chunked application stream.

use crate::cert::SelfSignedCert;
use crate::types::{Locale, Model};
use axum::{
    body::Body,
    extract::{Path as AxPath, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request as AxRequest, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bridge_ip: String,
    pub bind_ip: IpAddr,
    pub model: Model,
    pub locale: Locale,
    pub firmware_path: PathBuf,
    pub firmware_size: u64,
    pub firmware_sha256: String,
    pub crosspoint_version: String,
    pub change_log: String,
    /// Notified on every manifest request. Orchestrator awaits the first
    /// notification to advance from AwaitingDeviceRequest.
    pub on_manifest_request: Arc<tokio::sync::Notify>,
    /// Notified when the firmware binary download completes.
    pub on_firmware_streamed: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    #[serde(default)]
    pub current_version: String,
    #[serde(default)]
    pub device_type: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub lng: String,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub code: i32,
    pub data: ManifestData,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ManifestData {
    pub version: String,
    pub change_log: String,
    pub download_url: String,
    pub size: u64,
    pub upload_time: String,
    pub checksum: Option<String>,
}

pub fn router(cfg: Arc<ServerConfig>) -> Router {
    Router::new()
        .route("/api/v1/check-update", get(check_update))
        .route("/api/v1/device/activate", post(device_activate))
        .route("/firmware/{filename}", get(serve_firmware))
        // GitHub-shaped OTA: CrossPoint, CrossInk, and CrossPoint KO all hit
        // `api.github.com/repos/{owner}/{repo}/releases/latest`. We DNS-spoof
        // api.github.com to ourselves, so any repo path lands here — answer
        // with our manifest regardless of which firmware variant is asking.
        .route(
            "/repos/{owner}/{repo}/releases/latest",
            get(github_releases_latest),
        )
        .fallback(catch_all)
        .layer(middleware::from_fn(log_request))
        .with_state(cfg)
}

async fn log_request(req: AxRequest<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let ua = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    tracing::info!(%method, %uri, %host, %ua, "http request");
    next.run(req).await
}

async fn check_update(
    State(cfg): State<Arc<ServerConfig>>,
    headers: HeaderMap,
    Query(q): Query<UpdateQuery>,
) -> Json<Manifest> {
    tracing::info!(
        host = ?headers.get(header::HOST),
        device_type = %q.device_type,
        current_version = %q.current_version,
        "stock device requested update"
    );

    // notify_one buffers a permit if no waiter is registered yet — protects
    // against the device hitting check-update before the orchestrator has
    // begun awaiting the manifest event (e.g. when device discovery is slow).
    cfg.on_manifest_request.notify_one();

    let filename = format!(
        "V99.9.9-{model}-{locale}-PROD-{date}.bin",
        model = cfg.model.short(),
        locale = cfg.locale.short(),
        date = chrono::Utc::now().format("%m%d"),
    );

    Json(Manifest {
        code: 0,
        data: ManifestData {
            version: "V99.9.9".into(),
            change_log: cfg.change_log.clone(),
            download_url: format!("http://{}/firmware/{}", cfg.bridge_ip, filename),
            size: cfg.firmware_size,
            upload_time: chrono::Utc::now().to_rfc3339(),
            checksum: Some(format!("sha256:{}", cfg.firmware_sha256)),
        },
        message: "Update available".into(),
    })
}

async fn serve_firmware(
    State(cfg): State<Arc<ServerConfig>>,
    headers: HeaderMap,
    AxPath(filename): AxPath<String>,
) -> Result<Response, StatusCode> {
    tracing::info!(
        %filename,
        path = %cfg.firmware_path.display(),
        size = cfg.firmware_size,
        range = ?headers.get(header::RANGE),
        ?headers,
        "firmware download requested"
    );
    let size = cfg.firmware_size;
    let range = parse_range(headers.get(header::RANGE), size)?;
    tracing::info!(size, ?range, "serving firmware");
    // Advance the app UI as soon as the device begins the firmware GET.
    // Waiting for the whole transfer to finish hides the install screen while
    // the device is already on its OTA progress view.
    cfg.on_firmware_streamed.notify_one();

    let bytes = tokio::fs::read(&cfg.firmware_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let served_sha256 = hex::encode(Sha256::digest(&bytes));
    let head24 = hex::encode(&bytes[..bytes.len().min(24)]);
    if !served_sha256.eq_ignore_ascii_case(&cfg.firmware_sha256) {
        tracing::error!(
            path = %cfg.firmware_path.display(),
            expected_sha256 = %cfg.firmware_sha256,
            served_sha256 = %served_sha256,
            head24 = %head24,
            "refusing to serve firmware because bytes on disk do not match selected firmware hash"
        );
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    tracing::info!(
        path = %cfg.firmware_path.display(),
        served_sha256 = %served_sha256,
        head24 = %head24,
        "firmware bytes loaded for response"
    );

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Firmware-Sha256", served_sha256)
        .header("X-Firmware-Head24", head24)
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=firmware.bin"),
        );

    let body = match range {
        Some((start, end)) => {
            let content_len = end - start + 1;
            let start = start as usize;
            let end_inclusive = end as usize;
            let chunk = bytes
                .get(start..=end_inclusive)
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
                .to_vec();
            builder = builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"))
                .header(header::CONTENT_LENGTH, content_len);
            Body::from(chunk)
        }
        None => {
            builder = builder.header(header::CONTENT_LENGTH, size);
            Body::from(bytes)
        }
    };

    Ok(builder.body(body).unwrap())
}

fn parse_range(range: Option<&HeaderValue>, size: u64) -> Result<Option<(u64, u64)>, StatusCode> {
    let Some(range) = range else {
        return Ok(None);
    };

    let raw = range
        .to_str()
        .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let raw = raw
        .strip_prefix("bytes=")
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;

    if raw.contains(',') {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    let (start_raw, end_raw) = raw
        .split_once('-')
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;

    if size == 0 {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    let last = size - 1;

    let (start, end) = if start_raw.is_empty() {
        let suffix_len: u64 = end_raw
            .parse()
            .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
        if suffix_len == 0 {
            return Err(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        let start = size.saturating_sub(suffix_len);
        (start, last)
    } else {
        let start: u64 = start_raw
            .parse()
            .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
        let end = if end_raw.is_empty() {
            last
        } else {
            end_raw
                .parse()
                .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?
        };
        (start, end)
    };

    if start > end || end >= size {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }

    Ok(Some((start, end)))
}

/// Spoofs `GET /repos/{owner}/{repo}/releases/latest` for any repo. Used by
/// CrossPoint, CrossInk, and CrossPoint KO firmwares — they all check GitHub
/// for updates, just under different repo paths.
async fn github_releases_latest(
    State(cfg): State<Arc<ServerConfig>>,
    AxPath((owner, repo)): AxPath<(String, String)>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    tracing::info!(
        host = ?headers.get(header::HOST),
        user_agent = ?headers.get(header::USER_AGENT),
        %owner, %repo,
        "device requested update via GitHub API"
    );

    cfg.on_manifest_request.notify_one();

    // Use the hostname that matches our Let's Encrypt cert so TLS
    // validation passes (SAN check). DNS spoofs this to the bridge IP.
    let download_url = "https://unlocker.crosspointreader.com/firmware/firmware.bin".to_string();

    // `tag_name` stays unprefixed — CrossPoint's `sscanf("%d.%d.%d")` would
    // fail on a leading `v`. CrossInk's parser strips the optional `v`, so
    // unprefixed is accepted by both for the tag.
    let tag = "99.9.9";
    let asset = |name: String| {
        serde_json::json!({
            "name": name,
            "browser_download_url": download_url,
            "size": cfg.firmware_size,
            "content_type": "application/octet-stream",
        })
    };

    // Identify CrossInk by the repo path. Their build advertises variants and
    // expects the `v`-prefixed canonical filename (`firmware-<variant>-v<ver>.bin`)
    // per the maintainer. Other GitHub-shaped firmwares (CrossPoint, KO) look for
    // a plain `firmware.bin`; mixing the variant entries into their manifest is
    // unnecessary and could trip stricter parsers in future revisions.
    let is_crossink = repo.eq_ignore_ascii_case("crossink");
    let assets = if is_crossink {
        // Variants come from CrossInk's platformio.ini (`tiny`, `xlarge`,
        // `no_emoji`). All point at the same firmware bytes — the device's
        // variant matcher picks the one for its build.
        let asset_version = "v99.9.9.1";
        ["no_emoji", "tiny", "xlarge"]
            .iter()
            .map(|v| asset(format!("firmware-{v}-{asset_version}.bin")))
            .collect::<Vec<_>>()
    } else {
        vec![asset("firmware.bin".to_string())]
    };

    // Use a very high version so the device always considers it newer.
    // CrossPoint's version check uses sscanf("%d.%d.%d") so this parses
    // as 99.9.9 which is greater than any real version.
    Json(serde_json::json!({
        "tag_name": tag,
        "name": format!("CrossPoint {}", cfg.crosspoint_version),
        "assets": assets,
    }))
}

/// Stub for `POST /api/v1/device/activate` — V5.5.3+ stock firmware POSTs here
/// on boot. Returning 404 was harmless for the OTA itself (the device still
/// downloaded the manifest and firmware) but surfaced as a user-visible error
/// on the device UI. Reply with the same `{code:0,message:"ok",data:{}}`
/// envelope the real Xteink API uses so the device treats activation as
/// successful.
async fn device_activate(headers: HeaderMap, body: String) -> Json<serde_json::Value> {
    tracing::info!(
        host = ?headers.get(header::HOST),
        device_id = ?headers.get("device_id"),
        device_type = ?headers.get("device_type"),
        device_version = ?headers.get("device_version"),
        body_len = body.len(),
        "device activate (stubbed ok)"
    );
    Json(serde_json::json!({
        "code": 0,
        "message": "ok",
        "data": {},
    }))
}

/// Fallback for any request on a spoofed host that didn't match a route.
///
/// Returns a benign `{code:0,message:"ok",data:{}}` envelope instead of 404.
/// The unlocker only sees traffic for hosts it DNS-spoofs, so this fires only
/// on Xteink API paths we don't yet know about. Logging at `warn` keeps the
/// URI visible so we can add a real handler the next time the firmware adds
/// an endpoint.
async fn catch_all(method: Method, headers: HeaderMap, uri: axum::http::Uri) -> Response {
    tracing::warn!(%method, ?uri, ?headers, "unknown request — returning ok stub");
    Json(serde_json::json!({
        "code": 0,
        "message": "ok",
        "data": {},
    }))
    .into_response()
}

pub struct ServerHandles {
    pub http: tokio::task::JoinHandle<std::io::Result<()>>,
    pub https: tokio::task::JoinHandle<std::io::Result<()>>,
    pub http_handle: axum_server::Handle<SocketAddr>,
    pub https_handle: axum_server::Handle<SocketAddr>,
}

impl ServerHandles {
    pub async fn shutdown(self) {
        self.http_handle.shutdown();
        self.https_handle.shutdown();
        let _ = self.http.await;
        let _ = self.https.await;
    }
}

pub async fn start(cfg: Arc<ServerConfig>, cert: &SelfSignedCert) -> anyhow::Result<ServerHandles> {
    let app = router(cfg.clone());

    let http_addr = SocketAddr::new(cfg.bind_ip, 80);
    let https_addr = SocketAddr::new(cfg.bind_ip, 443);

    let http_handle = axum_server::Handle::new();
    let https_handle = axum_server::Handle::new();

    // Plain HTTP listener.
    let app_http = app.clone();
    let h1 = http_handle.clone();
    let http = tokio::spawn(async move {
        axum_server::bind(http_addr)
            .handle(h1)
            .serve(app_http.into_make_service())
            .await
    });

    // HTTPS listener. Force HTTP/1.1 only — ESP32's esp_http_client
    // doesn't support HTTP/2, and ALPN negotiation can cause issues.
    let certs =
        rustls_pemfile::certs(&mut cert.cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut cert.key_pem.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("no private key found in PEM"))?;
    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    server_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tls = RustlsConfig::from_config(std::sync::Arc::new(server_config));
    let app_https = app.clone();
    let h2 = https_handle.clone();
    let https = tokio::spawn(async move {
        axum_server::bind_rustls(https_addr, tls)
            .handle(h2)
            .serve(app_https.into_make_service())
            .await
    });

    tracing::info!(%http_addr, %https_addr, "manifest servers up");

    Ok(ServerHandles {
        http,
        https,
        http_handle,
        https_handle,
    })
}

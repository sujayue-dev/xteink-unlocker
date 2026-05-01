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
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
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
        .route("/firmware/{filename}", get(serve_firmware))
        // CrossPoint OTA: spoofs the GitHub releases API endpoint.
        .route(
            "/repos/crosspoint-reader/crosspoint-reader/releases/latest",
            get(github_releases_latest),
        )
        .fallback(catch_all)
        .with_state(cfg)
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

    cfg.on_manifest_request.notify_waiters();

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
    cfg.on_firmware_streamed.notify_waiters();

    let bytes = tokio::fs::read(&cfg.firmware_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ACCEPT_RANGES, "bytes")
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
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{size}"),
                )
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

    let raw = range.to_str().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
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

/// Spoofs `GET /repos/crosspoint-reader/crosspoint-reader/releases/latest`
/// so CrossPoint's OTA updater sees the firmware we're serving as a new release.
async fn github_releases_latest(
    State(cfg): State<Arc<ServerConfig>>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    tracing::info!(
        host = ?headers.get(header::HOST),
        user_agent = ?headers.get(header::USER_AGENT),
        "CrossPoint device requested update via GitHub API"
    );

    cfg.on_manifest_request.notify_waiters();

    // Use the hostname that matches our Let's Encrypt cert so TLS
    // validation passes (SAN check). DNS spoofs this to the bridge IP.
    let download_url = "https://unlocker.crosspointreader.com/firmware/firmware.bin".to_string();

    // Use a very high version so the device always considers it newer.
    // CrossPoint's version check uses sscanf("%d.%d.%d") so this parses
    // as 99.9.9 which is greater than any real version.
    Json(serde_json::json!({
        "tag_name": "99.9.9",
        "name": format!("CrossPoint {}", cfg.crosspoint_version),
        "assets": [{
            "name": "firmware.bin",
            "browser_download_url": download_url,
            "size": cfg.firmware_size,
            "content_type": "application/octet-stream"
        }]
    }))
}

async fn catch_all(headers: HeaderMap, uri: axum::http::Uri) -> impl IntoResponse {
    tracing::warn!(?uri, ?headers, "unknown request");
    (StatusCode::NOT_FOUND, "not found")
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

pub async fn start(
    cfg: Arc<ServerConfig>,
    cert: &SelfSignedCert,
) -> anyhow::Result<ServerHandles> {
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
    let certs = rustls_pemfile::certs(&mut cert.cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
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

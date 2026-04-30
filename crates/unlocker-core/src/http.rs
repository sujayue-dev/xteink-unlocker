//! Manifest server.
//!
//! Two listeners on the bridge IP:
//!   * port 80   — plain HTTP. Serves `/firmware/*.bin` (the download URL we
//!                 hand back in the manifest is HTTP).
//!   * port 443  — HTTPS with a self-signed cert for the spoofed API hostname.
//!                 Handles `/api/v1/check-update`.
//!
//! Both listeners share the same axum router: stock can hit the manifest
//! over either scheme, and we serve the binary over plain HTTP regardless.

use crate::cert::SelfSignedCert;
use crate::types::{Locale, Model};
use axum::{
    body::Body,
    extract::{Path as AxPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::io::ReaderStream;

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
    AxPath(_filename): AxPath<String>,
) -> Result<Response, StatusCode> {
    let file = tokio::fs::File::open(&cfg.firmware_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let notify = cfg.on_firmware_streamed.clone();
    // Best-effort: notify after a short delay matching expected stream time.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        notify.notify_waiters();
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_LENGTH, &cfg.firmware_size.to_string()),
        ],
        body,
    )
        .into_response())
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

    let download_url = format!(
        "https://{}/firmware/firmware.bin",
        cfg.bridge_ip,
    );

    Json(serde_json::json!({
        "tag_name": cfg.crosspoint_version,
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

    // HTTPS listener with self-signed cert.
    let tls = RustlsConfig::from_pem(
        cert.cert_pem.clone().into_bytes(),
        cert.key_pem.clone().into_bytes(),
    )
    .await?;
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

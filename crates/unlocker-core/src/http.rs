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
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Body stream that yields the firmware in fixed-size chunks while logging
/// progress and final state. The Drop impl is the key piece: it tells us
/// whether the transfer finished cleanly or the connection was torn down
/// mid-stream (TLS error, RST, device aborted, etc.) — info we couldn't see
/// before when the body was a single `Body::from(Vec)`.
///
/// We deliberately pace the stream with a small sleep between chunks. Without
/// pacing, rustls writes burst into macOS's TCP send buffer faster than
/// bridge100 + Wi-Fi can drain to the ESP32 — the bridge interface output
/// queue overflows after ~28-29 chunks and packets get dropped silently,
/// causing the device to RST the connection partway through. Pacing keeps the
/// effective rate near what the device can actually receive + flash-write.
const FIRMWARE_CHUNK_SIZE: usize = 16 * 1024;
const FIRMWARE_CHUNK_PACING: std::time::Duration = std::time::Duration::from_millis(8);

struct LoggedFirmwareStream {
    bytes: Bytes,
    offset: usize,
    total: usize,
    next_log_at: usize,
    path: String,
    finished: bool,
    pacing: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl LoggedFirmwareStream {
    fn new(bytes: Bytes, path: String) -> Self {
        let total = bytes.len();
        Self {
            bytes,
            offset: 0,
            total,
            next_log_at: 0,
            path,
            finished: false,
            pacing: None,
        }
    }
}

impl Stream for LoggedFirmwareStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.offset >= self.total {
            self.finished = true;
            return Poll::Ready(None);
        }
        // Honor pacing between chunks so we don't burst into bridge100's
        // output queue faster than the Wi-Fi link can drain to the device.
        if let Some(sleep) = self.pacing.as_mut() {
            match sleep.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => {
                    self.pacing = None;
                }
            }
        }
        let end = (self.offset + FIRMWARE_CHUNK_SIZE).min(self.total);
        let chunk = self.bytes.slice(self.offset..end);
        self.offset = end;
        if self.offset >= self.next_log_at {
            tracing::info!(
                path = %self.path,
                written = self.offset,
                total = self.total,
                "firmware stream progress"
            );
            // Log roughly every 10% of the transfer.
            self.next_log_at = self.offset + (self.total / 10).max(FIRMWARE_CHUNK_SIZE);
        }
        // Arm the pacing sleep for the next chunk (skipped after the last).
        if self.offset < self.total {
            self.pacing = Some(Box::pin(tokio::time::sleep(FIRMWARE_CHUNK_PACING)));
        }
        Poll::Ready(Some(Ok(chunk)))
    }
}

impl Drop for LoggedFirmwareStream {
    fn drop(&mut self) {
        if self.finished {
            tracing::info!(
                path = %self.path,
                written = self.offset,
                total = self.total,
                "firmware stream complete"
            );
        } else {
            tracing::warn!(
                path = %self.path,
                written = self.offset,
                total = self.total,
                "firmware stream closed early (peer disconnected or TLS error)"
            );
        }
    }
}

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

    let bytes = match tokio::fs::read(&cfg.firmware_path).await {
        Ok(b) => b,
        Err(e) => {
            // Previously this mapped silently to 404 and the device gave up
            // with no diagnostic in the log. The common failure is the helper
            // running as root via osascript admin, where macOS TCC blocks
            // reads from ~/Downloads/~/Desktop/etc. even for root. Surface
            // the underlying io error so we can tell EPERM from ENOENT.
            tracing::error!(
                path = %cfg.firmware_path.display(),
                error = %e,
                kind = ?e.kind(),
                "failed to read firmware file from disk"
            );
            return Err(StatusCode::NOT_FOUND);
        }
    };
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

    let path_for_log = cfg.firmware_path.display().to_string();
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
            Body::from_stream(LoggedFirmwareStream::new(Bytes::from(chunk), path_for_log))
        }
        None => {
            builder = builder.header(header::CONTENT_LENGTH, size);
            Body::from_stream(LoggedFirmwareStream::new(Bytes::from(bytes), path_for_log))
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

    // Log the leaf cert's subject + SANs so we can confirm the cert we present
    // covers the hostnames the device will request. esp_https_ota verifies the
    // server hostname against the cert SAN list after the handshake — if our
    // cert only covers some of the spoofed hostnames, the device will tear
    // down the connection right after handshake with no visible logs on our
    // side. Logging once at boot makes that misconfiguration obvious.
    if let Some(leaf) = certs.first() {
        match x509_parser::parse_x509_certificate(leaf.as_ref()) {
            Ok((_, parsed)) => {
                let subject = parsed.subject().to_string();
                let sans: Vec<String> = parsed
                    .extensions()
                    .iter()
                    .filter_map(|ext| match ext.parsed_extension() {
                        x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) => {
                            Some(
                                san.general_names
                                    .iter()
                                    .map(|n| format!("{n:?}"))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            )
                        }
                        _ => None,
                    })
                    .collect();
                let not_before = parsed.validity().not_before.to_string();
                let not_after = parsed.validity().not_after.to_string();
                tracing::info!(%subject, sans = ?sans, %not_before, %not_after, "tls cert loaded");
            }
            Err(e) => tracing::warn!(error = %e, "failed to parse leaf cert for SAN logging"),
        }
    }

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

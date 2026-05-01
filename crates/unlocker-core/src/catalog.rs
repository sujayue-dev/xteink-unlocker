use crate::types::{Catalog, Channel, CrossPointRelease, Model};
use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub const CATALOG_URL: &str = "https://crosspointreader.com/api/catalog";

pub fn cache_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| anyhow!("no data dir"))?;
    let dir = base.join("XteinkUnlocker").join("firmware");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn catalog_cache_path() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| anyhow!("no data dir"))?;
    let dir = base.join("XteinkUnlocker");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("catalog.json"))
}

fn read_cached_catalog() -> Result<Catalog> {
    let path = catalog_cache_path()?;
    let bytes = std::fs::read(&path)
        .with_context(|| format!("reading cached catalog from {}", path.display()))?;
    serde_json::from_slice(&bytes).context("decoding cached catalog")
}

fn write_cached_catalog(catalog: &Catalog) -> Result<()> {
    let path = catalog_cache_path()?;
    let bytes = serde_json::to_vec_pretty(catalog)?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("writing cached catalog to {}", path.display()))?;
    Ok(())
}

pub async fn fetch_catalog(client: &reqwest::Client) -> Result<Catalog> {
    // The companion catalog spec is owned by the user; we treat it as live.
    // If unreachable, fall back to the most recently cached live catalog.
    match client.get(CATALOG_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            let cat: Catalog = resp.json().await.context("decoding catalog")?;
            let _ = write_cached_catalog(&cat);
            Ok(cat)
        }
        Ok(resp) => read_cached_catalog()
            .with_context(|| format!("catalog HTTP {}", resp.status())),
        Err(e) => read_cached_catalog()
            .with_context(|| format!("catalog fetch failed: {e}")),
    }
}

/// Fallback / dev catalog. Useful before the real endpoint exists.
pub fn stub_catalog() -> Catalog {
    Catalog {
        schema_version: 1,
        releases: vec![
            CrossPointRelease {
                id: "stable-1.2.0".into(),
                channel: Channel::Stable,
                name: "1.2.0".into(),
                version: "1.2.0".into(),
                released_at: "2026-04-15T00:00:00Z".into(),
                notes: "Improved EPUB rendering speed\nAdded support for custom sleep screen images\nBug fixes".into(),
                firmware_url: "https://crosspointreader.com/api/release/firmware".into(),
                firmware_sha256: None,
                size: 6_291_456,
                supported_devices: vec![Model::X3, Model::X4],
            },
            CrossPointRelease {
                id: "beta-sd-storage".into(),
                channel: Channel::Beta,
                name: "Remote font downloads + SD storage".into(),
                version: "1.3.0-beta.1".into(),
                released_at: "2026-04-20T00:00:00Z".into(),
                notes: "Test build for upcoming 1.3 features. Known issue: ...".into(),
                firmware_url: "https://crosspointreader.com/api/beta/sd-storage/firmware".into(),
                firmware_sha256: None,
                size: 6_320_000,
                supported_devices: vec![Model::X3, Model::X4],
            },
            CrossPointRelease {
                id: "beta-calibre-sync".into(),
                channel: Channel::Beta,
                name: "Calibre sync experiment".into(),
                version: "1.3.0-beta.2".into(),
                released_at: "2026-04-22T00:00:00Z".into(),
                notes: "Alternate beta exploring calibre sync. Not compatible with the SD storage beta.".into(),
                firmware_url: "https://crosspointreader.com/api/beta/calibre-sync/firmware".into(),
                firmware_sha256: None,
                size: 6_315_000,
                supported_devices: vec![Model::X3, Model::X4],
            },
            CrossPointRelease {
                id: "insider-latest".into(),
                channel: Channel::Insider,
                name: "master-abc1234".into(),
                version: "master-abc1234".into(),
                released_at: "2026-04-29T00:00:00Z".into(),
                notes: "Latest nightly build".into(),
                firmware_url: "https://crosspointreader.com/api/build/firmware".into(),
                firmware_sha256: None,
                size: 6_350_000,
                supported_devices: vec![Model::X3, Model::X4],
            },
        ],
    }
}

pub async fn download_firmware(
    client: &reqwest::Client,
    release: &CrossPointRelease,
    on_progress: impl Fn(u64, u64),
) -> Result<(PathBuf, String)> {
    use futures::StreamExt;

    let dir = cache_dir()?;
    let resp = client
        .get(&release.firmware_url)
        .send()
        .await
        .context("starting firmware download")?
        .error_for_status()?;

    let total = resp.content_length().unwrap_or(release.size);
    let tmp = dir.join(format!(".dl-{}.bin", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha256::new();
    let mut got: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        on_progress(got, total);
    }
    file.flush().await?;
    drop(file);

    let sha = hex::encode(hasher.finalize());
    if let Some(expected) = &release.firmware_sha256 {
        if !expected.eq_ignore_ascii_case(&sha) {
            tokio::fs::remove_file(&tmp).await.ok();
            return Err(anyhow!("sha256 mismatch (expected {expected}, got {sha})"));
        }
    }
    let final_path = dir.join(format!("{sha}.bin"));
    if !final_path.exists() {
        tokio::fs::rename(&tmp, &final_path).await?;
    } else {
        tokio::fs::remove_file(&tmp).await.ok();
    }
    Ok((final_path, sha))
}

pub fn cached_path(sha256: &str) -> Result<Option<PathBuf>> {
    let p = cache_dir()?.join(format!("{sha256}.bin"));
    Ok(if p.exists() { Some(p) } else { None })
}

pub fn verify_file(path: &Path, expected_sha: &str) -> Result<bool> {
    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    std::io::copy(&mut f, &mut hasher)?;
    let got = hex::encode(hasher.finalize());
    Ok(got.eq_ignore_ascii_case(expected_sha))
}


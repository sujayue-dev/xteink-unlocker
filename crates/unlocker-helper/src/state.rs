//! Crash-recovery state.
//!
//! Whenever the helper takes an action that needs reversing, it records the
//! action in this file *before* doing the work. On startup, we read the file
//! and reverse anything that's still flagged as in-place.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tokio::sync::Mutex;

const STATE_PATH: &str = "/var/db/com.sofriendly.crosspoint.unlocker.helper.state.json";

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct HelperState {
    pub internet_sharing_active: bool,
    pub pfctl_anchor_loaded: bool,
}

static LOCK: Mutex<()> = Mutex::const_new(());

pub fn path() -> PathBuf {
    PathBuf::from(STATE_PATH)
}

pub async fn read() -> anyhow::Result<HelperState> {
    let _g = LOCK.lock().await;
    match fs::read(path()).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HelperState::default()),
        Err(e) => Err(e.into()),
    }
}

pub async fn write(s: &HelperState) -> anyhow::Result<()> {
    let _g = LOCK.lock().await;
    if let Some(parent) = path().parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(path(), bytes).await?;
    Ok(())
}

pub async fn mutate<F: FnOnce(&mut HelperState)>(f: F) -> anyhow::Result<()> {
    let mut s = read().await?;
    f(&mut s);
    write(&s).await
}

/// On helper start: reverse anything left in place by a prior crash.
pub async fn recover() -> anyhow::Result<()> {
    let s = read().await?;
    if s.pfctl_anchor_loaded {
        tracing::warn!("recovering: removing leftover pfctl anchor");
        let _ = crate::ops::pfctl_remove().await;
    }
    if s.internet_sharing_active {
        tracing::warn!("recovering: stopping leftover Internet Sharing");
        let _ = crate::ops::is_disable().await;
    }
    Ok(())
}

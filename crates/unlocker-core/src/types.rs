use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    X3,
    X4,
}

impl Model {
    pub fn short(&self) -> &'static str {
        match self {
            Model::X3 => "X3",
            Model::X4 => "X4",
        }
    }

    pub fn device_type(&self) -> &'static str {
        match self {
            Model::X3 => "ESP32C3_X3",
            Model::X4 => "ESP32C3_X4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    English,
    Chinese,
}

impl Locale {
    pub fn short(&self) -> &'static str {
        match self {
            Locale::English => "EN",
            Locale::Chinese => "CH",
        }
    }

    pub fn api_host(&self) -> &'static str {
        match self {
            Locale::English => "api-prod.xteink.cc",
            Locale::Chinese => "api-prod.xteink.cn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    Insider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossPointRelease {
    pub id: String,
    pub channel: Channel,
    /// Human-friendly label. For stable/insider this is the version string;
    /// for beta it's the author-supplied name (e.g. "SD storage experiment").
    pub name: String,
    pub version: String,
    pub released_at: String,
    pub notes: String,
    pub firmware_url: String,
    pub firmware_sha256: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema_version: u32,
    pub releases: Vec<CrossPointRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    pub model: Model,
    pub locale: Locale,
    pub release_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: String,
    pub level: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// What the unprivileged main process tells the helper when arming.
/// Crosses the JSON-RPC boundary, so everything is serializable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmServerSpec {
    pub bridge_ip: String,
    pub model: Model,
    pub locale: Locale,
    pub firmware_path: String,
    pub firmware_size: u64,
    pub firmware_sha256: String,
    pub crosspoint_version: String,
    pub change_log: String,
    pub dns_internal_port: u16,
}

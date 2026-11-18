//! 客户端配置加载 (云端地址、设备标识、内网空闲超时默认 180 秒、本地 IPC 端口 127.0.0.1:53318、TLS 指纹固定).

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use mi_protocol::ipc::DEFAULT_LOCAL_IPC_ADDR;

pub const DEFAULT_EVENT_QUEUE_CAPACITY: usize = 50;
pub const DEFAULT_LAN_IDLE_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TlsPinningError {
    #[error("TLS certificate fingerprint mismatch: expected {expected}, actual {actual}")]
    TlsFingerprintMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub device_id: String,
    pub device_name: String,
    pub os_name: String,
    pub cloud_ws_url: String,
    pub cloud_http_url: String,
    pub auth_token: String,
    pub lan_bind_addr: String,
    pub local_ipc_addr: String,
    pub event_queue_capacity: usize,
    pub lan_idle_timeout_secs: u64,
    #[serde(default)]
    pub server_cert_sha256: Option<String>,
    #[serde(default)]
    pub allowed_dirs: Vec<PathBuf>,
    #[serde(default = "DaemonConfig::platform_data_dir")]
    pub data_dir: PathBuf,
}

impl DaemonConfig {
    /// 按操作系统规范返回客户端配置与日志目录：
    /// - Windows: `%APPDATA%\multinteractor`
    /// - macOS: `~/Library/Application Support/multinteractor`
    /// - Linux/其他: `~/.config/multinteractor`
    pub fn platform_data_dir() -> PathBuf {
        if let Ok(custom) = std::env::var("MI_DATA_DIR") {
            if !custom.trim().is_empty() {
                return PathBuf::from(custom);
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                return PathBuf::from(appdata).join("multinteractor");
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("multinteractor");
            }
        }
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            return PathBuf::from(home).join(".config").join("multinteractor");
        }
        std::env::temp_dir().join("multinteractor")
    }

    /// 从可选 JSON 配置文件及环境变量加载客户端配置.
    pub fn from_env_and_file(config_file: Option<&Path>) -> Self {
        let mut cfg = if let Some(path) = config_file {
            if let Ok(raw) = fs::read_to_string(path) {
                serde_json::from_str::<DaemonConfig>(&raw).unwrap_or_default()
            } else {
                Self::default()
            }
        } else {
            let default_json = Self::platform_data_dir().join("client.json");
            if let Ok(raw) = fs::read_to_string(&default_json) {
                serde_json::from_str::<DaemonConfig>(&raw).unwrap_or_default()
            } else {
                Self::default()
            }
        };

        if let Ok(v) = std::env::var("MI_DEVICE_ID") {
            if !v.trim().is_empty() {
                cfg.device_id = v;
            }
        }
        if let Ok(v) = std::env::var("MI_DEVICE_NAME") {
            if !v.trim().is_empty() {
                cfg.device_name = v;
            }
        }
        if let Ok(v) = std::env::var("MI_CLOUD_WS_URL") {
            if !v.trim().is_empty() {
                cfg.cloud_ws_url = v;
            }
        }
        if let Ok(v) = std::env::var("MI_CLOUD_HTTP_URL") {
            if !v.trim().is_empty() {
                cfg.cloud_http_url = v;
            }
        }
        if let Ok(v) = std::env::var("MI_CLUSTER_TOKEN") {
            if !v.trim().is_empty() {
                cfg.auth_token = v;
            }
        }
        if let Ok(v) = std::env::var("MI_LAN_BIND_ADDR") {
            if !v.trim().is_empty() {
                cfg.lan_bind_addr = v;
            }
        }
        if let Ok(v) = std::env::var("MI_LOCAL_IPC_ADDR") {
            if !v.trim().is_empty() {
                cfg.local_ipc_addr = v;
            }
        }
        if let Ok(v) = std::env::var("MI_LAN_IDLE_TIMEOUT_SECS") {
            if let Ok(secs) = v.parse::<u64>() {
                cfg.lan_idle_timeout_secs = secs;
            }
        }
        if let Ok(v) = std::env::var("MI_SERVER_CERT_SHA256") {
            if !v.trim().is_empty() {
                cfg.server_cert_sha256 = Some(v.trim().to_lowercase());
            }
        }
        if let Ok(v) = std::env::var("MI_DATA_DIR") {
            if !v.trim().is_empty() {
                cfg.data_dir = PathBuf::from(v);
            }
        }
        cfg
    }

    /// 校验服务端证书指纹是否与本地配置的 `server_cert_sha256` 严格一致 (`OPTIMIZATION_AND_SECURITY_DESIGN.md` 2.1.2).
    pub fn verify_server_cert_fingerprint(&self, presented_fingerprint: &str) -> Result<(), TlsPinningError> {
        if let Some(ref pinned) = self.server_cert_sha256 {
            let clean_expected = pinned.trim().replace(':', "").to_lowercase();
            let clean_actual = presented_fingerprint.trim().replace(':', "").to_lowercase();
            if clean_expected != clean_actual {
                return Err(TlsPinningError::TlsFingerprintMismatch {
                    expected: clean_expected,
                    actual: clean_actual,
                });
            }
        }
        Ok(())
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            device_id: "dev_local_node".to_string(),
            device_name: "Local-Computer".to_string(),
            os_name: std::env::consts::OS.to_string(),
            cloud_ws_url: "ws://127.0.0.1:8000/ws/signaling".to_string(),
            cloud_http_url: "http://127.0.0.1:8000".to_string(),
            auth_token: "mi-personal-secret-token-2026".to_string(),
            lan_bind_addr: "0.0.0.0:8443".to_string(),
            local_ipc_addr: DEFAULT_LOCAL_IPC_ADDR.to_string(),
            event_queue_capacity: DEFAULT_EVENT_QUEUE_CAPACITY,
            lan_idle_timeout_secs: DEFAULT_LAN_IDLE_TIMEOUT_SECS,
            server_cert_sha256: None,
            allowed_dirs: Vec::new(),
            data_dir: Self::platform_data_dir(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_fingerprint_pinning_verification() {
        let mut cfg = DaemonConfig::default();
        assert!(cfg.verify_server_cert_fingerprint("any").is_ok());

        cfg.server_cert_sha256 = Some("AA:BB:CC:11:22".to_string());
        assert!(cfg.verify_server_cert_fingerprint("aabbcc1122").is_ok());
        assert_eq!(
            cfg.verify_server_cert_fingerprint("deadbeef"),
            Err(TlsPinningError::TlsFingerprintMismatch {
                expected: "aabbcc1122".to_string(),
                actual: "deadbeef".to_string(),
            })
        );
    }
}

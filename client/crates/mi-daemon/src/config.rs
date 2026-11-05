//! 客户端配置加载 (云端地址、设备标识、内网空闲超时默认 180 秒、本地 IPC 端口 127.0.0.1:53318).

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use mi_protocol::ipc::DEFAULT_LOCAL_IPC_ADDR;

pub const DEFAULT_EVENT_QUEUE_CAPACITY: usize = 50;
pub const DEFAULT_LAN_IDLE_TIMEOUT_SECS: u64 = 180;

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
}

impl DaemonConfig {
    /// 按操作系统规范返回客户端配置与日志目录：
    /// - Windows: `%APPDATA%\multinteractor`
    /// - macOS: `~/Library/Application Support/multinteractor`
    /// - Linux/其他: `~/.config/multinteractor`
    pub fn platform_data_dir() -> PathBuf {
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
        }
    }
}

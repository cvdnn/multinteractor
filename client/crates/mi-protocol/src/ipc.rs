//! 本地守护进程 (`mi-daemon`) 与 CLI/OS/UI 回环 IPC (`127.0.0.1:53318`) 请求/响应契约.

use serde::{Deserialize, Serialize};
use crate::envelope::CloudEventEnvelope;
use crate::rpc::FsEntry;

pub const DEFAULT_LOCAL_IPC_ADDR: &str = "127.0.0.1:53318";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStatusDto {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub online: bool,
    pub lan_endpoint: Option<String>,
    pub lan_reachable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcCopyRequest {
    pub text: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcCopyResponse {
    pub event_id: String,
    pub content_category: String,
    pub content_hash: String,
    pub file_cache_id: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcPasteRequest {
    pub event_id: Option<String>,
    pub dest_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcPasteResponse {
    pub event_id: String,
    pub content_category: String,
    pub saved_path: Option<String>,
    pub inline_text: Option<String>,
    pub digest_verified: bool,
    pub bytes_transferred: u64,
    pub channel_used: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcRemoteLsResponse {
    pub device_id: String,
    pub path: String,
    pub entries: Vec<FsEntry>,
    pub free_space_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcSyncRequest {
    pub source: String,
    pub target_device: String,
    pub remote_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcSyncResponse {
    pub transfer_id: String,
    pub event_id: String,
    pub status: String,
    pub target_device: String,
    pub remote_saved_path: String,
    pub digest_verified: bool,
    pub channel_used: String,
    pub bytes_transferred: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcTransferStatusResponse {
    pub task_id: String,
    pub file_cache_id: String,
    pub status: String,
    pub verified_offset: u64,
    pub total_size: u64,
    pub digest_verified: bool,
    pub channel_used: String,
    pub resume_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcEventsListResponse {
    pub events: Vec<CloudEventEnvelope>,
}

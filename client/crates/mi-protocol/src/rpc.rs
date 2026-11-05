//! 远程目录浏览 (`FS_LIST_DIR_REQ/RESP`)、远程新建文件夹 (`FS_MKDIR_REQ/RESP`) 与定向同步回执模型.

use serde::{Deserialize, Serialize};

pub const ROOT_PATH_SENTINEL: &str = "ROOT";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsListDirRequest {
    pub request_id: String,
    pub target_device_id: String,
    pub path: String,
    pub include_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mtime: u64,
    pub has_subdirs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsListDirResponse {
    pub request_id: String,
    pub device_id: String,
    pub current_path: String,
    pub entries: Vec<FsEntry>,
    pub free_space_bytes: u64,
    pub scan_duration_ms: u64,
    pub skipped_offline_drives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsMkdirRequest {
    pub request_id: String,
    pub target_device_id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsMkdirResponse {
    pub request_id: String,
    pub created_path: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectSyncAck {
    pub transfer_id: String,
    pub event_id: String,
    pub target_device_id: String,
    pub saved_path: String,
    pub digest_verified: bool,
    pub bytes_transferred: u64,
    pub channel_used: String,
}

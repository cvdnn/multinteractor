//! UI 宿主命令桥接层：调用本地 `mi-daemon` 的远程目录懒加载与定向同步接口.

use mi_protocol::ipc::{IpcRemoteLsResponse, IpcSyncRequest, IpcSyncResponse};
use mi_protocol::rpc::{FsEntry, FsMkdirResponse, ROOT_PATH_SENTINEL};

pub struct RemoteDirectoryPickerSession {
    pub target_device_id: String,
    pub source_local_path: String,
    pub current_remote_path: String,
}

impl RemoteDirectoryPickerSession {
    pub fn new(target_device_id: String, source_local_path: String) -> Self {
        Self {
            target_device_id,
            source_local_path,
            current_remote_path: ROOT_PATH_SENTINEL.to_string(),
        }
    }

    pub fn build_initial_root_response(&self, entries: Vec<FsEntry>, free_space_bytes: u64) -> IpcRemoteLsResponse {
        IpcRemoteLsResponse {
            device_id: self.target_device_id.clone(),
            path: self.current_remote_path.clone(),
            entries,
            free_space_bytes,
        }
    }

    pub fn build_mkdir_response(&self, new_folder_path: String) -> FsMkdirResponse {
        FsMkdirResponse {
            request_id: "req_ui_mkdir".to_string(),
            created_path: new_folder_path,
            status: "CREATED".to_string(),
            error: None,
        }
    }

    pub fn build_sync_request(&self, selected_remote_dir: String) -> IpcSyncRequest {
        IpcSyncRequest {
            source: self.source_local_path.clone(),
            target_device: self.target_device_id.clone(),
            remote_dir: selected_remote_dir,
        }
    }

    pub fn mock_sync_ack(&self, transfer_id: String, remote_saved_path: String, bytes: u64) -> IpcSyncResponse {
        IpcSyncResponse {
            transfer_id,
            event_id: "evt_ui_sync".to_string(),
            status: "COMPLETED".to_string(),
            target_device: self.target_device_id.clone(),
            remote_saved_path,
            digest_verified: true,
            channel_used: "LAN_DIRECT".to_string(),
            bytes_transferred: bytes,
        }
    }
}

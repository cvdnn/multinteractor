//! UI 宿主命令桥接层：调用本地 `mi-daemon` 的远程目录懒加载与定向同步接口.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use mi_protocol::ipc::{
    IpcRemoteLsRequest, IpcRemoteLsResponse, IpcRemoteMkdirRequest, IpcSyncRequest,
    IpcSyncResponse, DEFAULT_LOCAL_IPC_ADDR,
};
use mi_protocol::rpc::{FsEntry, FsMkdirResponse, ROOT_PATH_SENTINEL};

#[derive(Debug, Clone)]
pub struct RemoteDirectoryPickerSession {
    pub target_device_id: String,
    pub source_local_path: String,
    pub current_remote_path: String,
    pub daemon_endpoint: String,
}

impl RemoteDirectoryPickerSession {
    pub fn new(target_device_id: String, source_local_path: String) -> Self {
        let daemon_endpoint = std::env::var("MI_DAEMON_IPC_URL")
            .unwrap_or_else(|_| format!("http://{}", DEFAULT_LOCAL_IPC_ADDR));
        Self {
            target_device_id,
            source_local_path,
            current_remote_path: ROOT_PATH_SENTINEL.to_string(),
            daemon_endpoint,
        }
    }

    pub fn with_daemon_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.daemon_endpoint = endpoint.into();
        self
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

    pub fn fetch_remote_ls(&mut self, path: &str) -> anyhow::Result<IpcRemoteLsResponse> {
        self.current_remote_path = path.to_string();
        let req = IpcRemoteLsRequest {
            device: self.target_device_id.clone(),
            path: path.to_string(),
        };
        let body = serde_json::to_vec(&req)?;
        let resp_bytes = self.send_ipc("POST", "/api/v1/remote-ls", &body)?;
        Ok(serde_json::from_slice(&resp_bytes)?)
    }

    pub fn execute_remote_mkdir(&self, new_folder_path: &str) -> anyhow::Result<FsMkdirResponse> {
        let req = IpcRemoteMkdirRequest {
            device: self.target_device_id.clone(),
            path: new_folder_path.to_string(),
        };
        let body = serde_json::to_vec(&req)?;
        let resp_bytes = self.send_ipc("POST", "/api/v1/remote-mkdir", &body)?;
        Ok(serde_json::from_slice(&resp_bytes)?)
    }

    pub fn execute_sync(&self, selected_remote_dir: String) -> anyhow::Result<IpcSyncResponse> {
        let req = self.build_sync_request(selected_remote_dir);
        let body = serde_json::to_vec(&req)?;
        let resp_bytes = self.send_ipc("POST", "/api/v1/sync", &body)?;
        Ok(serde_json::from_slice(&resp_bytes)?)
    }

    pub fn send_ipc(&self, method: &str, path: &str, body: &[u8]) -> anyhow::Result<Vec<u8>> {
        let addr_str = self
            .daemon_endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        let sock_addr: SocketAddr = addr_str.parse().or_else(|_| {
            addr_str
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid daemon endpoint"))
        })?;
        let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        let head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            method,
            path,
            addr_str,
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        if !body.is_empty() {
            stream.write_all(body)?;
        }
        stream.flush()?;

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let sep = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| anyhow::anyhow!("Malformed HTTP response"))?;
        Ok(raw[sep + 4..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_picker_session_builders() {
        let session = RemoteDirectoryPickerSession::new(
            "node_c".to_string(),
            "/Users/handy/AI教程.docx".to_string(),
        );
        assert_eq!(session.current_remote_path, "ROOT");
        let sync_req = session.build_sync_request("/Users/handy/Desktop".to_string());
        assert_eq!(sync_req.target_device, "node_c");
        assert_eq!(sync_req.remote_dir, "/Users/handy/Desktop");

        let ack = session.mock_sync_ack("tx_1".to_string(), "/Users/handy/Desktop/AI教程.docx".to_string(), 1024);
        assert!(ack.digest_verified);
        assert_eq!(ack.bytes_transferred, 1024);
    }
}


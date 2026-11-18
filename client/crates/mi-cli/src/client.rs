//! 调用本地 `mi-daemon` (`http://127.0.0.1:53318/api/v1/*`) 的轻量 IPC 客户端.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use mi_protocol::ipc::{
    DeviceStatusDto, IpcCertRotateResponse, IpcCertStatusResponse, IpcCopyRequest,
    IpcCopyResponse, IpcEventsListResponse, IpcPairStatusResponse, IpcPasteRequest,
    IpcPasteResponse, IpcPinServerRequest, IpcRemoteLsRequest, IpcRemoteLsResponse,
    IpcRemoteMkdirRequest, IpcSyncRequest, IpcSyncResponse, IpcTransferActionRequest,
    IpcTransferStatusResponse, DEFAULT_LOCAL_IPC_ADDR,
};
use mi_protocol::rpc::FsMkdirResponse;

#[derive(Debug, Clone)]
pub struct DaemonIpcClient {
    pub endpoint: String,
}

impl Default for DaemonIpcClient {
    fn default() -> Self {
        if let Ok(url) = std::env::var("MI_DAEMON_IPC_URL") {
            if !url.trim().is_empty() {
                return Self::new(url);
            }
        }
        if let Ok(addr) = std::env::var("MI_LOCAL_IPC_ADDR") {
            if !addr.trim().is_empty() {
                let ep = if addr.starts_with("http://") {
                    addr
                } else {
                    format!("http://{}", addr)
                };
                return Self::new(ep);
            }
        }
        Self::new(format!("http://{}", DEFAULT_LOCAL_IPC_ADDR))
    }
}

impl DaemonIpcClient {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    pub fn list_devices(&self) -> anyhow::Result<Vec<DeviceStatusDto>> {
        let body = self.request("GET", "/api/v1/devices", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn list_events(&self, limit: usize) -> anyhow::Result<IpcEventsListResponse> {
        let path = format!("/api/v1/events?limit={}", limit);
        let body = self.request("GET", &path, None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn copy(&self, req: &IpcCopyRequest) -> anyhow::Result<IpcCopyResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/copy", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn paste(&self, req: &IpcPasteRequest) -> anyhow::Result<IpcPasteResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/paste", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn remote_ls(&self, req: &IpcRemoteLsRequest) -> anyhow::Result<IpcRemoteLsResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/remote-ls", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn remote_mkdir(&self, req: &IpcRemoteMkdirRequest) -> anyhow::Result<FsMkdirResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/remote-mkdir", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn sync(&self, req: &IpcSyncRequest) -> anyhow::Result<IpcSyncResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/sync", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn transfer_status(&self, task_id: &str) -> anyhow::Result<IpcTransferStatusResponse> {
        let path = format!("/api/v1/transfer/status?task_id={}", task_id);
        let body = self.request("GET", &path, None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn transfer_resume(
        &self,
        req: &IpcTransferActionRequest,
    ) -> anyhow::Result<IpcTransferStatusResponse> {
        let payload = serde_json::to_vec(req)?;
        let body = self.request("POST", "/api/v1/transfer/resume", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn cert_status(&self) -> anyhow::Result<IpcCertStatusResponse> {
        let body = self.request("GET", "/api/v1/cert/status", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn cert_rotate(&self) -> anyhow::Result<IpcCertRotateResponse> {
        let body = self.request("POST", "/api/v1/cert/rotate", Some(b"{}"))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn cert_pin_server(&self, sha256: &str) -> anyhow::Result<IpcCertStatusResponse> {
        let payload = serde_json::to_vec(&IpcPinServerRequest {
            sha256: sha256.to_string(),
        })?;
        let body = self.request("POST", "/api/v1/cert/pin-server", Some(&payload))?;
        Ok(serde_json::from_slice(&body)?)
    }

    pub fn pair_status(&self) -> anyhow::Result<IpcPairStatusResponse> {
        let body = self.request("GET", "/api/v1/pair/status", None)?;
        Ok(serde_json::from_slice(&body)?)
    }

    fn request(&self, method: &str, path: &str, body: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
        let addr_str = self
            .endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');
        let sock_addr: SocketAddr = addr_str.parse().or_else(|_| {
            addr_str
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid ipc endpoint"))
        })?;

        let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        let payload = body.unwrap_or(&[]);
        let req_head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            method,
            path,
            addr_str,
            payload.len()
        );
        stream.write_all(req_head.as_bytes())?;
        if !payload.is_empty() {
            stream.write_all(payload)?;
        }
        stream.flush()?;

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let sep = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| anyhow::anyhow!("Malformed HTTP response from mi-daemon"))?;
        let head = String::from_utf8_lossy(&raw[..sep]);
        let status = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(500);
        let resp_body = raw[sep + 4..].to_vec();
        if status >= 400 {
            anyhow::bail!(
                "Daemon IPC error (HTTP {}): {}",
                status,
                String::from_utf8_lossy(&resp_body)
            );
        }
        Ok(resp_body)
    }
}


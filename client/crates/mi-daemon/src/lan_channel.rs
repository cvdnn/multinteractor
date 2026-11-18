//! 内网即用即连 1-to-N 双向通道池 (`LanChannelPool`) 与空闲满 180 秒自动关闭计时器 (`IdleTimer`) (FR-TRANS-01, TASK-05).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use mi_protocol::digest::DigestManifest;
use mi_protocol::envelope::MAX_TRANSFER_TOTAL_SIZE;
use mi_protocol::rpc::{
    FsListDirRequest, FsListDirResponse, FsMkdirRequest, FsMkdirResponse,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::cert_manager::CertManager;
use crate::fs_rpc::FsRpcHandler;
use crate::transfer::{unpack_archive_to_directory, TransferManager};

pub const DEFAULT_LAN_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPushMetadata {
    pub file_name: String,
    pub remote_dir: String,
    pub is_directory_archive: bool,
    pub digest: DigestManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanSyncPushResponse {
    pub ok: bool,
    pub saved_path: String,
    pub bytes_written: u64,
    pub content_hash: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LanChannelSession {
    pub peer_device_id: String,
    pub peer_endpoint: String,
    pub established_at: Instant,
    pub last_active_at: Instant,
    pub reuse_count: u64,
    pub bytes_transferred: u64,
}

impl LanChannelSession {
    pub fn new(peer_device_id: String, peer_endpoint: String, now: Instant) -> Self {
        Self {
            peer_device_id,
            peer_endpoint,
            established_at: now,
            last_active_at: now,
            reuse_count: 0,
            bytes_transferred: 0,
        }
    }

    pub fn touch(&mut self, now: Instant, bytes_delta: u64) {
        self.last_active_at = now;
        self.reuse_count += 1;
        self.bytes_transferred += bytes_delta;
    }

    pub fn is_idle_expired(&self, now: Instant, idle_timeout: Duration) -> bool {
        now.duration_since(self.last_active_at) >= idle_timeout
    }
}

/// 内网即用即连 1-to-N 并发连接池管理器：
/// 1. 平时零内网常驻连接 (`active_connection_count() == 0`)；
/// 2. 触发文件拉取或目录浏览时即用即连 (`acquire_or_reuse`)，支持一对多 (`1-to-N`) 多机并发；
/// 3. 活跃期复用并重置空闲计时器；当空闲满 `180 秒` (可配置) 无数据交互后自动关闭释放连接 (`reap_idle_channels`).
#[derive(Debug, Clone)]
pub struct LanChannelPool {
    idle_timeout: Duration,
    sessions: Arc<RwLock<HashMap<String, LanChannelSession>>>,
    in_process_peers: Arc<RwLock<HashMap<String, LanChannelServer>>>,
    tls_ctx: Option<(CertManager, Arc<RwLock<HashSet<String>>>)>,
}

impl LanChannelPool {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            idle_timeout,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            in_process_peers: Arc::new(RwLock::new(HashMap::new())),
            tls_ctx: None,
        }
    }

    pub fn with_tls(
        mut self,
        cert_mgr: CertManager,
        allowed_peer_fps: Arc<RwLock<HashSet<String>>>,
    ) -> Self {
        self.tls_ctx = Some((cert_mgr, allowed_peer_fps));
        self
    }

    pub fn sync_allowed_peer_fingerprints(&self, fps: HashSet<String>) {
        if let Some((_, ref allowed)) = self.tls_ctx {
            let mut guard = allowed.write().expect("allowed_peer_fps lock poisoned");
            *guard = fps;
        }
    }

    pub fn lan_client_tls_config(&self) -> Option<Arc<rustls::ClientConfig>> {
        self.tls_ctx
            .as_ref()
            .and_then(|(cm, allowed)| cm.build_lan_mtls_client_config(allowed.clone()).ok())
    }

    pub fn lan_server_tls_config(&self) -> Option<Arc<rustls::ServerConfig>> {
        self.tls_ctx
            .as_ref()
            .and_then(|(cm, allowed)| cm.build_lan_mtls_server_config(allowed.clone()).ok())
    }

    pub fn with_default_timeout() -> Self {
        Self::new(DEFAULT_LAN_IDLE_TIMEOUT)
    }

    /// 注册同进程对端节点服务 (供无网络沙箱集成测试或本地直通加速使用).
    pub fn register_peer_server(&self, peer_device_id: &str, server: LanChannelServer) {
        let mut guard = self
            .in_process_peers
            .write()
            .expect("in_process_peers lock poisoned");
        guard.insert(peer_device_id.to_string(), server);
    }

    fn get_in_process_peer(&self, peer_device_id: &str) -> Option<LanChannelServer> {
        let guard = self
            .in_process_peers
            .read()
            .expect("in_process_peers lock poisoned");
        guard.get(peer_device_id).cloned()
    }

    pub fn active_connection_count(&self) -> usize {
        let guard = self.sessions.read().expect("lan pool lock poisoned");
        guard.len()
    }

    /// 按需建立或复用与目标机器的内网双向通道，并重置空闲 180 秒倒计时.
    pub fn acquire_or_reuse(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        now: Instant,
    ) -> (LanChannelSession, bool) {
        self.reap_idle_channels(now);
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        if let Some(existing) = guard.get_mut(peer_device_id) {
            existing.touch(now, 0);
            return (existing.clone(), true);
        }

        let session = LanChannelSession::new(
            peer_device_id.to_string(),
            peer_endpoint.to_string(),
            now,
        );
        guard.insert(peer_device_id.to_string(), session.clone());
        (session, false)
    }

    pub fn record_io(&self, peer_device_id: &str, bytes: u64, now: Instant) -> bool {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        if let Some(session) = guard.get_mut(peer_device_id) {
            session.touch(now, bytes);
            return true;
        }
        false
    }

    /// 扫描并关闭所有空闲满 `idle_timeout` (默认 180 秒) 无数据交互的内网连接，返回被关闭的设备 ID 列表.
    pub fn reap_idle_channels(&self, now: Instant) -> Vec<String> {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        let mut closed = Vec::new();
        guard.retain(|device_id, session| {
            if session.is_idle_expired(now, self.idle_timeout) {
                closed.push(device_id.clone());
                false
            } else {
                true
            }
        });
        closed
    }

    /// 证书轮换或吊销时立即清理所有存量内网连接，强制下次重建并重新验证 mTLS 指纹.
    pub fn clear_all_sessions(&self) -> usize {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        let count = guard.len();
        guard.clear();
        count
    }

    pub fn force_close(&self, peer_device_id: &str) -> bool {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        guard.remove(peer_device_id).is_some()
    }

    /// 启动后台定时清理协程，定期检测并回收空闲达到 `idle_timeout` (180 秒) 的内网连接.
    pub fn spawn_idle_reaper(
        &self,
        poll_interval: Duration,
        mut shutdown: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let pool = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(poll_interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let _ = pool.reap_idle_channels(Instant::now());
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        })
    }

    /// 通过内网直连通道调用目标设备的 `list-dir` RPC，同时维护 180 秒空闲连接池计数.
    pub fn rpc_list_dir(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        req: &FsListDirRequest,
    ) -> anyhow::Result<FsListDirResponse> {
        let now = Instant::now();
        self.acquire_or_reuse(peer_device_id, peer_endpoint, now);
        if let Some(peer_srv) = self.get_in_process_peer(peer_device_id) {
            peer_srv
                .pool
                .acquire_or_reuse(peer_device_id, peer_endpoint, now);
            let resp = peer_srv.fs_rpc.handle_list_dir(req)?;
            self.record_io(peer_device_id, 256, Instant::now());
            return Ok(resp);
        }
        let body = serde_json::to_vec(req)?;
        let (status, _, resp_body) = send_http_request_with_tls(
            peer_endpoint,
            "POST",
            "/lan/v1/rpc/list-dir",
            &[],
            &body,
            self.lan_client_tls_config(),
        )?;
        self.record_io(
            peer_device_id,
            (body.len() + resp_body.len()) as u64,
            Instant::now(),
        );
        if status >= 400 {
            anyhow::bail!(
                "LAN list-dir failed with status {}: {}",
                status,
                String::from_utf8_lossy(&resp_body)
            );
        }
        Ok(serde_json::from_slice(&resp_body)?)
    }

    /// 通过内网直连通道调用目标设备的 `mkdir` RPC.
    pub fn rpc_mkdir(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        req: &FsMkdirRequest,
    ) -> anyhow::Result<FsMkdirResponse> {
        let now = Instant::now();
        self.acquire_or_reuse(peer_device_id, peer_endpoint, now);
        if let Some(peer_srv) = self.get_in_process_peer(peer_device_id) {
            peer_srv
                .pool
                .acquire_or_reuse(peer_device_id, peer_endpoint, now);
            let resp = peer_srv.fs_rpc.handle_mkdir(req);
            self.record_io(peer_device_id, 128, Instant::now());
            return Ok(resp);
        }
        let body = serde_json::to_vec(req)?;
        let (status, _, resp_body) = send_http_request_with_tls(
            peer_endpoint,
            "POST",
            "/lan/v1/rpc/mkdir",
            &[],
            &body,
            self.lan_client_tls_config(),
        )?;
        self.record_io(
            peer_device_id,
            (body.len() + resp_body.len()) as u64,
            Instant::now(),
        );
        if status >= 400 {
            anyhow::bail!("LAN mkdir failed with status {}", status);
        }
        Ok(serde_json::from_slice(&resp_body)?)
    }

    /// 通过内网直连通道按 `Range: bytes=start_offset-` 拉取文件字节流.
    pub fn fetch_file_range(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        file_cache_id: &str,
        start_offset: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let now = Instant::now();
        self.acquire_or_reuse(peer_device_id, peer_endpoint, now);
        if let Some(peer_srv) = self.get_in_process_peer(peer_device_id) {
            peer_srv
                .pool
                .acquire_or_reuse(peer_device_id, peer_endpoint, now);
            let (bytes, _) = peer_srv
                .transfer_mgr
                .read_staged_range(file_cache_id, start_offset)?;
            self.record_io(peer_device_id, bytes.len() as u64, Instant::now());
            return Ok(bytes);
        }
        let path = format!("/lan/v1/files/{}/stream", file_cache_id);
        let range_val = format!("bytes={}-", start_offset);
        let (status, _, resp_body) = send_http_request_with_tls(
            peer_endpoint,
            "GET",
            &path,
            &[("Range", range_val.as_str())],
            &[],
            self.lan_client_tls_config(),
        )?;
        if status != 200 && status != 206 {
            anyhow::bail!("LAN file stream returned HTTP {}", status);
        }
        self.record_io(peer_device_id, resp_body.len() as u64, Instant::now());
        Ok(resp_body)
    }

    /// 通过内网直连通道将文件或目录归档直接推送至目标机器指定目录 (`remote_dir`).
    pub fn push_sync_payload(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        meta: &LanSyncPushMetadata,
        payload_bytes: &[u8],
    ) -> anyhow::Result<LanSyncPushResponse> {
        let now = Instant::now();
        self.acquire_or_reuse(peer_device_id, peer_endpoint, now);
        if let Some(peer_srv) = self.get_in_process_peer(peer_device_id) {
            peer_srv
                .pool
                .acquire_or_reuse(peer_device_id, peer_endpoint, now);
            let resp = peer_srv.handle_sync_push(meta.clone(), payload_bytes);
            self.record_io(peer_device_id, payload_bytes.len() as u64, Instant::now());
            return Ok(resp);
        }
        let meta_json = serde_json::to_string(meta)?;
        let (status, _, resp_body) = send_http_request_with_tls(
            peer_endpoint,
            "POST",
            "/lan/v1/sync/push",
            &[("X-Mi-Sync-Meta", meta_json.as_str())],
            payload_bytes,
            self.lan_client_tls_config(),
        )?;
        self.record_io(
            peer_device_id,
            (payload_bytes.len() + resp_body.len()) as u64,
            Instant::now(),
        );
        if status >= 400 {
            anyhow::bail!(
                "LAN sync push failed (HTTP {}): {}",
                status,
                String::from_utf8_lossy(&resp_body)
            );
        }
        Ok(serde_json::from_slice(&resp_body)?)
    }
}

/// 内网直连 TCP/HTTP 服务端，处理对端发起的远程目录浏览、新建文件夹、Range 文件拉取与定向同步推送.
#[derive(Debug, Clone)]
pub struct LanChannelServer {
    pub pool: LanChannelPool,
    pub fs_rpc: FsRpcHandler,
    pub transfer_mgr: TransferManager,
}

impl LanChannelServer {
    pub fn new(pool: LanChannelPool, fs_rpc: FsRpcHandler, transfer_mgr: TransferManager) -> Self {
        Self {
            pool,
            fs_rpc,
            transfer_mgr,
        }
    }

    pub async fn bind_and_serve(
        self,
        bind_addr: &str,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let tls_acceptor = self
            .pool
            .lan_server_tls_config()
            .map(tokio_rustls::TlsAcceptor::from);
        let srv = self.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_res = listener.accept() => {
                        if let Ok((mut stream, peer_addr)) = accept_res {
                            let srv_clone = srv.clone();
                            let acceptor_opt = tls_acceptor.clone();
                            tokio::spawn(async move {
                                if let Some(acceptor) = acceptor_opt {
                                    if let Ok(mut tls_stream) = acceptor.accept(stream).await {
                                        let _ = srv_clone.handle_connection(&mut tls_stream, peer_addr).await;
                                    }
                                } else {
                                    let _ = srv_clone.handle_connection(&mut stream, peer_addr).await;
                                }
                            });
                        }
                    }
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        });
        Ok((local_addr, handle))
    }

    async fn handle_connection<S: AsyncReadExt + AsyncWriteExt + Unpin>(
        &self,
        stream: &mut S,
        peer_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let (method, path, headers, body) = read_async_http_request(stream).await?;
        let peer_id = headers
            .get("x-device-id")
            .cloned()
            .unwrap_or_else(|| peer_addr.to_string());
        self.pool
            .acquire_or_reuse(&peer_id, &peer_addr.to_string(), Instant::now());

        if method == "POST" && path == "/lan/v1/rpc/list-dir" {
            let req: FsListDirRequest = serde_json::from_slice(&body)?;
            match self.fs_rpc.handle_list_dir(&req) {
                Ok(resp) => {
                    let resp_bytes = serde_json::to_vec(&resp)?;
                    write_async_http_response(stream, 200, "application/json", &resp_bytes).await?;
                }
                Err(e) => {
                    let err_json = serde_json::json!({ "error": e.to_string() });
                    write_async_http_response(
                        stream,
                        400,
                        "application/json",
                        err_json.to_string().as_bytes(),
                    )
                    .await?;
                }
            }
            return Ok(());
        }

        if method == "POST" && path == "/lan/v1/rpc/mkdir" {
            let req: FsMkdirRequest = serde_json::from_slice(&body)?;
            let resp = self.fs_rpc.handle_mkdir(&req);
            let resp_bytes = serde_json::to_vec(&resp)?;
            write_async_http_response(stream, 200, "application/json", &resp_bytes).await?;
            return Ok(());
        }

        if method == "GET" && path.starts_with("/lan/v1/files/") && path.ends_with("/stream") {
            let trimmed = path
                .trim_start_matches("/lan/v1/files/")
                .trim_end_matches("/stream");
            let start_offset = headers
                .get("range")
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.trim_end_matches('-').parse::<u64>().ok())
                .unwrap_or(0);
            match self.transfer_mgr.read_staged_range(trimmed, start_offset) {
                Ok((bytes, _total)) => {
                    let status = if start_offset > 0 { 206 } else { 200 };
                    write_async_http_response(stream, status, "application/octet-stream", &bytes)
                        .await?;
                }
                Err(e) => {
                    let msg = format!("{{\"error\":\"{}\"}}", e);
                    write_async_http_response(stream, 404, "application/json", msg.as_bytes())
                        .await?;
                }
            }
            return Ok(());
        }

        if method == "POST" && path == "/lan/v1/sync/push" {
            let meta_raw = headers
                .get("x-mi-sync-meta")
                .ok_or_else(|| anyhow::anyhow!("Missing X-Mi-Sync-Meta header"))?;
            let meta: LanSyncPushMetadata = serde_json::from_str(meta_raw)?;
            let resp = self.handle_sync_push(meta, &body);
            let code = if resp.ok { 200 } else { 400 };
            let resp_bytes = serde_json::to_vec(&resp)?;
            write_async_http_response(stream, code, "application/json", &resp_bytes).await?;
            return Ok(());
        }

        write_async_http_response(stream, 404, "application/json", b"{\"error\":\"not found\"}")
            .await?;
        Ok(())
    }

    fn handle_sync_push(&self, meta: LanSyncPushMetadata, payload: &[u8]) -> LanSyncPushResponse {
        // 1. Check directory whitelist & traversal via FsRpcHandler
        let mkdir_check = self.fs_rpc.handle_mkdir(&FsMkdirRequest {
            request_id: "sync_check".to_string(),
            target_device_id: self.fs_rpc.device_id.clone(),
            path: meta.remote_dir.clone(),
        });
        if mkdir_check.status != "CREATED" {
            return LanSyncPushResponse {
                ok: false,
                saved_path: meta.remote_dir,
                bytes_written: 0,
                content_hash: String::new(),
                error: mkdir_check.error,
            };
        }

        // 2. Verify BLAKE3 digest of transferred payload
        if let Err(e) = meta.digest.verify_bytes(payload) {
            return LanSyncPushResponse {
                ok: false,
                saved_path: meta.remote_dir,
                bytes_written: 0,
                content_hash: String::new(),
                error: Some(format!("Digest verification failed: {}", e)),
            };
        }

        let dest_root = Path::new(&meta.remote_dir);
        if meta.is_directory_archive {
            let tmp_archive = dest_root.join(format!(".mi_sync_{}.tar.zst.part", std::process::id()));
            if let Err(e) = std::fs::write(&tmp_archive, payload) {
                return LanSyncPushResponse {
                    ok: false,
                    saved_path: meta.remote_dir,
                    bytes_written: 0,
                    content_hash: String::new(),
                    error: Some(e.to_string()),
                };
            }
            let unpack_res =
                unpack_archive_to_directory(&tmp_archive, dest_root, MAX_TRANSFER_TOTAL_SIZE);
            let _ = std::fs::remove_file(&tmp_archive);
            match unpack_res {
                Ok(extracted) => LanSyncPushResponse {
                    ok: true,
                    saved_path: dest_root.join(&meta.file_name).to_string_lossy().to_string(),
                    bytes_written: extracted,
                    content_hash: meta.digest.full_hash,
                    error: None,
                },
                Err(e) => LanSyncPushResponse {
                    ok: false,
                    saved_path: meta.remote_dir,
                    bytes_written: 0,
                    content_hash: String::new(),
                    error: Some(e.to_string()),
                },
            }
        } else {
            let safe_file_name = Path::new(&meta.file_name)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "synced.bin".to_string());
            let final_path = dest_root.join(&safe_file_name);
            let part_path = dest_root.join(format!("{}.multinteractor.part", safe_file_name));
            if let Err(e) = std::fs::write(&part_path, payload) {
                return LanSyncPushResponse {
                    ok: false,
                    saved_path: final_path.to_string_lossy().to_string(),
                    bytes_written: 0,
                    content_hash: String::new(),
                    error: Some(e.to_string()),
                };
            }
            if let Err(e) = std::fs::rename(&part_path, &final_path) {
                let _ = std::fs::remove_file(&part_path);
                return LanSyncPushResponse {
                    ok: false,
                    saved_path: final_path.to_string_lossy().to_string(),
                    bytes_written: 0,
                    content_hash: String::new(),
                    error: Some(e.to_string()),
                };
            }
            LanSyncPushResponse {
                ok: true,
                saved_path: final_path.to_string_lossy().to_string(),
                bytes_written: payload.len() as u64,
                content_hash: meta.digest.full_hash,
                error: None,
            }
        }
    }
}

pub(crate) fn send_http_request_with_tls(
    endpoint: &str,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    tls_config: Option<Arc<rustls::ClientConfig>>,
) -> anyhow::Result<(u16, HashMap<String, String>, Vec<u8>)> {
    let addr = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let sock_addr: SocketAddr = addr.parse().or_else(|_| {
        std::net::ToSocketAddrs::to_socket_addrs(addr)?
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid endpoint"))
    })?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut req_head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        method,
        path,
        addr,
        body.len()
    );
    for (k, v) in extra_headers {
        req_head.push_str(&format!("{}: {}\r\n", k, v));
    }
    req_head.push_str("\r\n");

    let mut raw_resp = Vec::new();
    if let Some(cfg) = tls_config {
        let host = addr.split(':').next().unwrap_or("localhost").to_string();
        let server_name = rustls::pki_types::ServerName::try_from(host)
            .unwrap_or_else(|_| rustls::pki_types::ServerName::try_from("localhost").unwrap());
        let conn = rustls::ClientConnection::new(cfg, server_name)?;
        let mut tls_stream = rustls::StreamOwned::new(conn, stream);
        tls_stream.write_all(req_head.as_bytes())?;
        if !body.is_empty() {
            tls_stream.write_all(body)?;
        }
        tls_stream.flush()?;
        let _ = tls_stream.read_to_end(&mut raw_resp);
    } else {
        stream.write_all(req_head.as_bytes())?;
        if !body.is_empty() {
            stream.write_all(body)?;
        }
        stream.flush()?;
        stream.read_to_end(&mut raw_resp)?;
    }
    parse_raw_http_response_with_headers(&raw_resp)
}

pub(crate) fn parse_raw_http_response_with_headers(
    raw: &[u8],
) -> anyhow::Result<(u16, HashMap<String, String>, Vec<u8>)> {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("Malformed HTTP response header"))?;
    let head_str = String::from_utf8_lossy(&raw[..sep]);
    let mut lines = head_str.lines();
    let status_line = lines.next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid HTTP status line: {}", status_line))?;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok((status_code, headers, raw[sep + 4..].to_vec()))
}

pub(crate) async fn read_async_http_request<S: AsyncReadExt + Unpin>(
    stream: &mut S,
) -> anyhow::Result<(String, String, HashMap<String, String>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(4096);
    let mut temp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut temp).await?;
        if n == 0 {
            anyhow::bail!("Connection closed before HTTP headers complete");
        }
        buf.extend_from_slice(&temp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            anyhow::bail!("HTTP header too large");
        }
    };

    let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = header_text.lines();
    let req_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty request line"))?;
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_uppercase();
    let path = parts.next().unwrap_or("/").to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    let mut body = Vec::with_capacity(content_len);
    if buf.len() > body_start {
        body.extend_from_slice(&buf[body_start..]);
    }
    while body.len() < content_len {
        let n = stream.read(&mut temp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&temp[..n]);
    }
    body.truncate(content_len);
    Ok((method, path, headers, body))
}

pub(crate) async fn write_async_http_response<S: AsyncWriteExt + Unpin>(
    stream: &mut S,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_initial_connections_1_to_n_concurrency_and_180s_idle_disconnect() {
        let pool = LanChannelPool::with_default_timeout();
        let t0 = Instant::now();

        // 1. 初始空闲状态下内网连接数为 0
        assert_eq!(pool.active_connection_count(), 0);

        // 2. B 和 C 同时按需连入 A (1-to-2 并发)
        let (_, reused_b) = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", t0);
        let (_, reused_c) = pool.acquire_or_reuse("node_c", "192.168.1.12:8443", t0);
        assert!(!reused_b);
        assert!(!reused_c);
        assert_eq!(pool.active_connection_count(), 2);

        // 3. 第 100 秒时 node_b 再次传输 -> 复用连接并重置 node_b 的 180s 计时器
        let t100 = t0 + Duration::from_secs(100);
        let (sess_b, reused_b_again) = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", t100);
        assert!(reused_b_again);
        assert_eq!(sess_b.reuse_count, 1);

        // 4. 第 180 秒时：node_c 已空闲满 180 秒自动关闭，而 node_b 仅空闲 80 秒继续保留
        let t180 = t0 + Duration::from_secs(180);
        let closed_at_180 = pool.reap_idle_channels(t180);
        assert_eq!(closed_at_180, vec!["node_c".to_string()]);
        assert_eq!(pool.active_connection_count(), 1);

        // 5. 第 280 秒时 (node_b 空闲满 180 秒)：node_b 连接也自动关闭，连接数归零
        let t280 = t100 + Duration::from_secs(180);
        let closed_at_280 = pool.reap_idle_channels(t280);
        assert_eq!(closed_at_280, vec!["node_b".to_string()]);
        assert_eq!(pool.active_connection_count(), 0);
    }

    #[tokio::test]
    async fn test_lan_mtls_handshake_and_untrusted_peer_rejection() {
        let base = std::env::temp_dir().join(format!("mi_lan_mtls_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir_a = base.join("a");
        let dir_b = base.join("b");
        let dir_rogue = base.join("rogue");
        std::fs::create_dir_all(&dir_a).unwrap();
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::create_dir_all(&dir_rogue).unwrap();

        let cm_a = CertManager::load_or_create(&dir_a, "node_a", None).unwrap();
        let cm_b = CertManager::load_or_create(&dir_b, "node_b", None).unwrap();
        let cm_rogue = CertManager::load_or_create(&dir_rogue, "node_rogue", None).unwrap();

        let allowed_for_a = Arc::new(RwLock::new(HashSet::from([cm_b.local_identity().cert_sha256])));
        let allowed_for_b = Arc::new(RwLock::new(HashSet::from([cm_a.local_identity().cert_sha256])));
        let allowed_for_rogue = Arc::new(RwLock::new(HashSet::from([cm_a.local_identity().cert_sha256])));

        let pool_a = LanChannelPool::with_default_timeout().with_tls(cm_a, allowed_for_a);
        let pool_b = LanChannelPool::with_default_timeout().with_tls(cm_b, allowed_for_b);
        let pool_rogue = LanChannelPool::with_default_timeout().with_tls(cm_rogue, allowed_for_rogue);

        let fs_a = FsRpcHandler::with_allowed_roots("node_a".to_string(), vec![dir_a.clone()]);
        let srv_a = LanChannelServer::new(pool_a.clone(), fs_a, TransferManager::new());

        let acceptor = tokio_rustls::TlsAcceptor::from(pool_a.lan_server_tls_config().unwrap());
        let connector_b = tokio_rustls::TlsConnector::from(pool_b.lan_client_tls_config().unwrap());
        let connector_rogue =
            tokio_rustls::TlsConnector::from(pool_rogue.lan_client_tls_config().unwrap());
        let sname = rustls::pki_types::ServerName::try_from("localhost").unwrap();

        // 1. Trusted peer B completes mTLS handshake and receives HTTP 200 RPC response
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let acc1 = acceptor.clone();
        let srv1 = srv_a.clone();
        let srv_task = tokio::spawn(async move {
            let mut tls_srv = acc1.accept(server_io).await.unwrap();
            let peer_addr: SocketAddr = "127.0.0.1:19001".parse().unwrap();
            srv1.handle_connection(&mut tls_srv, peer_addr).await.unwrap();
        });

        let mut tls_cli = connector_b.connect(sname.clone(), client_io).await.unwrap();
        let rpc_req = serde_json::to_vec(&FsListDirRequest {
            request_id: "req_1".to_string(),
            target_device_id: "node_a".to_string(),
            path: dir_a.to_string_lossy().to_string(),
            include_files: true,
        })
        .unwrap();
        let http_req = format!(
            "POST /lan/v1/rpc/list-dir HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            rpc_req.len()
        );
        tls_cli.write_all(http_req.as_bytes()).await.unwrap();
        tls_cli.write_all(&rpc_req).await.unwrap();
        tls_cli.flush().await.unwrap();
        let mut resp_buf = Vec::new();
        let _ = tls_cli.read_to_end(&mut resp_buf).await;
        let resp_str = String::from_utf8_lossy(&resp_buf);
        assert!(resp_str.contains("200 OK"), "Expected 200 OK, got: {}", resp_str);
        srv_task.await.unwrap();

        // 2. Untrusted rogue peer must be rejected during mTLS handshake
        let (rogue_io, server_io2) = tokio::io::duplex(64 * 1024);
        let acc2 = acceptor.clone();
        let srv_task2 = tokio::spawn(async move { acc2.accept(server_io2).await });
        let cli_res = connector_rogue.connect(sname, rogue_io).await;
        let srv_res = srv_task2.await.unwrap();
        assert!(
            srv_res.is_err() || cli_res.is_err(),
            "Untrusted rogue peer must fail mTLS handshake"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}


//! 云端常驻 WebSocket 信令客户端 (注册鉴权、心跳保活、Topic 订阅同步、在线设备内网 IP:Port 同步).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use mi_protocol::digest::DigestManifest;
use mi_protocol::envelope::{
    topic_rpc_fs, topic_sync_direct, CloudEventEnvelope, ContentCategory, DeviceRegistrationFrame,
    EventPayload, EventType, SourceDevice, TargetMode, TargetScope, UploadState, SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST, TOPIC_SYSTEM_PRESENCE,
};
use mi_protocol::ipc::DeviceStatusDto;
use mi_protocol::rpc::{FsListDirRequest, FsMkdirRequest};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;

use crate::config::DaemonConfig;
use crate::event_queue::EventCacheList;
use crate::fs_rpc::FsRpcHandler;
use crate::lan_channel::send_http_request_with_tls;
use crate::transfer::UploadSession;

#[derive(Debug, Clone)]
pub struct CloudSignalingClient {
    config: DaemonConfig,
    event_queue: EventCacheList,
    peers: Arc<RwLock<HashMap<String, DeviceStatusDto>>>,
    outbox: Arc<RwLock<Vec<CloudEventEnvelope>>>,
    cert_mgr: Option<crate::cert_manager::CertManager>,
}

impl CloudSignalingClient {
    pub fn new(config: DaemonConfig, event_queue: EventCacheList) -> Self {
        Self {
            config,
            event_queue,
            peers: Arc::new(RwLock::new(HashMap::new())),
            outbox: Arc::new(RwLock::new(Vec::new())),
            cert_mgr: None,
        }
    }

    pub fn with_cert_manager(
        config: DaemonConfig,
        event_queue: EventCacheList,
        cert_mgr: crate::cert_manager::CertManager,
    ) -> Self {
        Self {
            config,
            event_queue,
            peers: Arc::new(RwLock::new(HashMap::new())),
            outbox: Arc::new(RwLock::new(Vec::new())),
            cert_mgr: Some(cert_mgr),
        }
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn build_registration_frame(&self) -> DeviceRegistrationFrame {
        let (cert_sha256, cert_expires_at) = match self.cert_mgr {
            Some(ref cm) => {
                let id = cm.local_identity();
                (Some(id.cert_sha256), Some(id.expires_at))
            }
            None => (None, None),
        };
        DeviceRegistrationFrame {
            device_id: self.config.device_id.clone(),
            device_name: self.config.device_name.clone(),
            os: self.config.os_name.clone(),
            lan_endpoint: self.config.lan_bind_addr.clone(),
            auth_token: self.config.auth_token.clone(),
            subscribed_topics: vec![
                TOPIC_CLIPBOARD_BROADCAST.to_string(),
                TOPIC_SYSTEM_PRESENCE.to_string(),
                topic_sync_direct(&self.config.device_id),
                topic_rpc_fs(&self.config.device_id),
            ],
            cert_sha256,
            cert_expires_at,
        }
    }

    pub fn enqueue_envelope(&self, env: CloudEventEnvelope) {
        let mut guard = self.outbox.write().expect("outbox lock poisoned");
        guard.push(env);
    }

    pub fn drain_outbox(&self) -> Vec<CloudEventEnvelope> {
        let mut guard = self.outbox.write().expect("outbox lock poisoned");
        std::mem::take(&mut *guard)
    }

    pub fn broadcast_text(&self, event_id: String, text: &str, timestamp_ms: u64) -> CloudEventEnvelope {
        let digest = DigestManifest::from_bytes(text.as_bytes());
        let env = CloudEventEnvelope {
            spec_version: SPEC_VERSION_V6.to_string(),
            event_id,
            topic: TOPIC_CLIPBOARD_BROADCAST.to_string(),
            event_type: EventType::ClipboardBroadcast,
            timestamp_ms,
            source_device: SourceDevice {
                device_id: self.config.device_id.clone(),
                device_name: self.config.device_name.clone(),
                os: self.config.os_name.clone(),
                lan_endpoint: Some(self.config.lan_bind_addr.clone()),
            },
            target_scope: TargetScope {
                mode: TargetMode::Broadcast,
                target_device_ids: vec!["*".to_string()],
            },
            payload: EventPayload {
                content_category: ContentCategory::Text,
                digest,
                inline_data: Some(text.to_string()),
                file_cache_id: None,
                upload_state: UploadState::Inline,
                items_meta: vec![],
                destination_override: None,
            },
        };
        self.enqueue_envelope(env.clone());
        env
    }

    pub fn handle_incoming_envelope(&self, envelope: CloudEventEnvelope) -> anyhow::Result<()> {
        let fs_rpc = FsRpcHandler::with_allowed_roots(
            self.config.device_id.clone(),
            self.config.allowed_dirs.clone(),
        );
        let _ = self.handle_incoming_envelope_with_fs_rpc(envelope, &fs_rpc)?;
        Ok(())
    }

    pub fn handle_incoming_envelope_with_fs_rpc(
        &self,
        envelope: CloudEventEnvelope,
        fs_rpc: &FsRpcHandler,
    ) -> anyhow::Result<Option<CloudEventEnvelope>> {
        // Auto-record sender peer endpoint whenever we receive an envelope from a peer
        if envelope.source_device.device_id != self.config.device_id {
            self.upsert_peer(DeviceStatusDto {
                device_id: envelope.source_device.device_id.clone(),
                name: envelope.source_device.device_name.clone(),
                os: envelope.source_device.os.clone(),
                online: true,
                lan_endpoint: envelope.source_device.lan_endpoint.clone(),
                lan_reachable: envelope.source_device.lan_endpoint.is_some(),
                cert_sha256: None,
            });
        }

        match envelope.event_type {
            EventType::ClipboardBroadcast | EventType::DirectSync => {
                self.event_queue.push_verified(envelope)?;
                Ok(None)
            }
            EventType::DevicePresence => {
                if let Some(ref inline_json) = envelope.payload.inline_data {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(inline_json) {
                        let online = val.get("state").and_then(|s| s.as_str()) == Some("ONLINE");
                        let cert_sha256 = val
                            .get("cert_sha256")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let dto = DeviceStatusDto {
                            device_id: envelope.source_device.device_id.clone(),
                            name: envelope.source_device.device_name.clone(),
                            os: envelope.source_device.os.clone(),
                            online,
                            lan_endpoint: envelope.source_device.lan_endpoint.clone(),
                            lan_reachable: online && envelope.source_device.lan_endpoint.is_some(),
                            cert_sha256,
                        };
                        self.upsert_peer(dto);
                    }
                }
                Ok(None)
            }
            EventType::FsRpc => {
                if let Some(ref inline_json) = envelope.payload.inline_data {
                    if let Ok(list_req) = serde_json::from_str::<FsListDirRequest>(inline_json) {
                        if let Ok(resp) = fs_rpc.handle_list_dir(&list_req) {
                            let resp_json = serde_json::to_string(&resp)?;
                            let reply = self.build_rpc_reply_envelope(
                                &envelope.source_device.device_id,
                                &resp_json,
                                envelope.timestamp_ms + 1,
                            );
                            self.enqueue_envelope(reply.clone());
                            return Ok(Some(reply));
                        }
                    }
                    if let Ok(mkdir_req) = serde_json::from_str::<FsMkdirRequest>(inline_json) {
                        let resp = fs_rpc.handle_mkdir(&mkdir_req);
                        let resp_json = serde_json::to_string(&resp)?;
                        let reply = self.build_rpc_reply_envelope(
                            &envelope.source_device.device_id,
                            &resp_json,
                            envelope.timestamp_ms + 1,
                        );
                        self.enqueue_envelope(reply.clone());
                        return Ok(Some(reply));
                    }
                }
                Ok(None)
            }
        }
    }

    fn build_rpc_reply_envelope(
        &self,
        target_device_id: &str,
        reply_json: &str,
        timestamp_ms: u64,
    ) -> CloudEventEnvelope {
        let digest = DigestManifest::from_bytes(reply_json.as_bytes());
        CloudEventEnvelope {
            spec_version: SPEC_VERSION_V6.to_string(),
            event_id: format!("evt_rpc_reply_{}", timestamp_ms),
            topic: topic_rpc_fs(target_device_id),
            event_type: EventType::FsRpc,
            timestamp_ms,
            source_device: SourceDevice {
                device_id: self.config.device_id.clone(),
                device_name: self.config.device_name.clone(),
                os: self.config.os_name.clone(),
                lan_endpoint: Some(self.config.lan_bind_addr.clone()),
            },
            target_scope: TargetScope {
                mode: TargetMode::Direct,
                target_device_ids: vec![target_device_id.to_string()],
            },
            payload: EventPayload {
                content_category: ContentCategory::Text,
                digest,
                inline_data: Some(reply_json.to_string()),
                file_cache_id: None,
                upload_state: UploadState::Inline,
                items_meta: vec![],
                destination_override: None,
            },
        }
    }

    pub fn upsert_peer(&self, dto: DeviceStatusDto) {
        let mut peers = self.peers.write().expect("peers lock poisoned");
        peers.insert(dto.device_id.clone(), dto);
    }

    pub fn resolve_peer(&self, device_id_or_name: &str) -> Option<DeviceStatusDto> {
        let guard = self.peers.read().expect("peers lock poisoned");
        if let Some(found) = guard.get(device_id_or_name) {
            return Some(found.clone());
        }
        guard
            .values()
            .find(|p| p.name.eq_ignore_ascii_case(device_id_or_name))
            .cloned()
    }

    pub fn list_peers(&self) -> Vec<DeviceStatusDto> {
        let guard = self.peers.read().expect("peers lock poisoned");
        let mut list: Vec<DeviceStatusDto> = guard.values().cloned().collect();
        list.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        list
    }

    /// 将 `ws://host:port/ws/signaling` 转换为对应的 HTTP 端点 `host:port`.
    pub fn cloud_http_endpoint(&self) -> Option<String> {
        let raw = self
            .config
            .cloud_ws_url
            .trim_start_matches("ws://")
            .trim_start_matches("wss://")
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let host_port = raw.split('/').next()?;
        if host_port.is_empty() {
            None
        } else {
            Some(host_port.to_string())
        }
    }

    fn cloud_http_tls_config(&self) -> Option<Arc<rustls::ClientConfig>> {
        if self.config.cloud_http_url.starts_with("https://")
            || self.config.cloud_ws_url.starts_with("wss://")
        {
            self.cert_mgr
                .as_ref()
                .and_then(|cm| cm.build_cloud_tls_client_config().ok())
        } else {
            None
        }
    }

    /// 遵循 `/api/v1/cache/*` 协议将本地文件分块上传至云端缓存，支持 `HEAD` 偏移查询与 `PATCH` 4MB 分块续传.
    pub fn upload_file_to_cloud_cache(
        &self,
        session: &UploadSession,
        file_path: &std::path::Path,
        file_name: &str,
        content_category: &str,
    ) -> anyhow::Result<u64> {
        let endpoint = self
            .cloud_http_endpoint()
            .ok_or_else(|| anyhow::anyhow!("Invalid cloud_ws_url"))?;
        let auth_hdr = format!("Bearer {}", self.config.auth_token);
        let tls_cfg = self.cloud_http_tls_config();

        let init_body = serde_json::json!({
            "file_cache_id": session.file_cache_id,
            "source_device_id": self.config.device_id,
            "total_size": session.total_size,
            "digest": session.digest,
            "file_name": file_name,
            "content_category": content_category,
        });
        let init_bytes = serde_json::to_vec(&init_body)?;
        let (status, _, _) = send_http_request_with_tls(
            &endpoint,
            "POST",
            "/api/v1/cache/init",
            &[
                ("Content-Type", "application/json"),
                ("Authorization", auth_hdr.as_str()),
                ("X-Device-Id", self.config.device_id.as_str()),
            ],
            &init_bytes,
            tls_cfg.clone(),
        )?;
        if status >= 400 {
            anyhow::bail!("Cloud cache init failed with HTTP {}", status);
        }

        let patch_path = format!("/api/v1/cache/{}", session.file_cache_id);
        let mut offset = match send_http_request_with_tls(
            &endpoint,
            "HEAD",
            &patch_path,
            &[
                ("Authorization", auth_hdr.as_str()),
                ("X-Device-Id", self.config.device_id.as_str()),
            ],
            &[],
            tls_cfg.clone(),
        ) {
            Ok((h_status, headers, _)) if h_status < 400 => headers
                .get("upload-offset")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                .min(session.total_size),
            _ => 0,
        };

        while offset < session.total_size {
            let (chunk, chunk_hash) = session.read_chunk_at(file_path, offset)?;
            let offset_str = offset.to_string();
            let (p_status, _, _) = send_http_request_with_tls(
                &endpoint,
                "PATCH",
                &patch_path,
                &[
                    ("Authorization", auth_hdr.as_str()),
                    ("X-Device-Id", self.config.device_id.as_str()),
                    ("Upload-Offset", offset_str.as_str()),
                    ("X-Chunk-Blake3", chunk_hash.as_str()),
                ],
                &chunk,
                tls_cfg.clone(),
            )?;
            if p_status >= 400 {
                anyhow::bail!("Cloud cache PATCH failed with HTTP {}", p_status);
            }
            offset += chunk.len() as u64;
        }
        Ok(offset)
    }

    /// 从云端 `/api/v1/cache/{file_cache_id}/stream` 按 `Range: bytes=start_offset-` 拉取字节流.
    pub fn fetch_cloud_cache_range(
        &self,
        file_cache_id: &str,
        start_offset: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let endpoint = self
            .cloud_http_endpoint()
            .ok_or_else(|| anyhow::anyhow!("Invalid cloud_ws_url"))?;
        let auth_hdr = format!("Bearer {}", self.config.auth_token);
        let range_hdr = format!("bytes={}-", start_offset);
        let path = format!("/api/v1/cache/{}/stream", file_cache_id);
        let (status, _, body) = send_http_request_with_tls(
            &endpoint,
            "GET",
            &path,
            &[
                ("Authorization", auth_hdr.as_str()),
                ("X-Device-Id", self.config.device_id.as_str()),
                ("Range", range_hdr.as_str()),
            ],
            &[],
            self.cloud_http_tls_config(),
        )?;
        if status != 200 && status != 206 {
            anyhow::bail!("Cloud cache stream returned HTTP {}", status);
        }
        Ok(body)
    }

    /// 启动后台云端 WebSocket 信令重连与心跳循环 (`PING`/`PONG` 3s 心跳 + Outbox 自动投递).
    pub fn spawn_signaling_loop(
        &self,
        mut shutdown: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let client = self.clone();
        tokio::spawn(async move {
            loop {
                if *shutdown.borrow() {
                    break;
                }
                let _ = client.run_single_ws_session(&mut shutdown).await;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
        })
    }

    async fn run_single_ws_session(&self, shutdown: &mut watch::Receiver<bool>) -> anyhow::Result<()> {
        let endpoint = self
            .cloud_http_endpoint()
            .ok_or_else(|| anyhow::anyhow!("No cloud endpoint"))?;
        let tcp_stream =
            tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&endpoint)).await??;

        if self.config.cloud_ws_url.starts_with("wss://") {
            let cm = self
                .cert_mgr
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("CertManager required for wss://"))?;
            let tls_cfg = cm.build_cloud_tls_client_config()?;
            let connector = tokio_rustls::TlsConnector::from(tls_cfg);
            let host = endpoint.split(':').next().unwrap_or("localhost").to_string();
            let server_name = rustls::pki_types::ServerName::try_from(host)
                .unwrap_or_else(|_| rustls::pki_types::ServerName::try_from("localhost").unwrap());
            let mut tls_stream = connector.connect(server_name, tcp_stream).await?;
            self.run_ws_over_stream(&mut tls_stream, &endpoint, shutdown)
                .await
        } else {
            let mut stream = tcp_stream;
            self.run_ws_over_stream(&mut stream, &endpoint, shutdown)
                .await
        }
    }

    async fn run_ws_over_stream<S: AsyncReadExt + AsyncWriteExt + Unpin>(
        &self,
        stream: &mut S,
        endpoint: &str,
        shutdown: &mut watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let ws_req = format!(
            "GET /ws/signaling HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            endpoint
        );
        stream.write_all(ws_req.as_bytes()).await?;
        let mut hs_buf = [0u8; 1024];
        let n = stream.read(&mut hs_buf).await?;
        let hs_str = String::from_utf8_lossy(&hs_buf[..n]);
        if !hs_str.contains("101") {
            anyhow::bail!("WebSocket handshake rejected: {}", hs_str);
        }

        let reg = self.build_registration_frame();
        let reg_json = serde_json::to_string(&reg)?;
        write_ws_text_frame(stream, &reg_json).await?;

        let mut heartbeat = tokio::time::interval(Duration::from_secs(3));
        let mut outbox_tick = tokio::time::interval(Duration::from_millis(200));

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    write_ws_text_frame(stream, "{\"type\":\"PING\"}").await?;
                }
                _ = outbox_tick.tick() => {
                    for env in self.drain_outbox() {
                        if let Ok(json) = serde_json::to_string(&env) {
                            write_ws_text_frame(stream, &json).await?;
                        }
                    }
                }
                frame_res = read_ws_text_frame(stream) => {
                    let text = frame_res?;
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        let msg_type = val.get("type").and_then(|t| t.as_str());
                        if msg_type == Some("REGISTERED") {
                            if let Some(peers) = val.get("peers").and_then(|p| p.as_array()) {
                                for p in peers {
                                    if let (Some(id), Some(name), Some(os)) = (
                                        p.get("device_id").and_then(|v| v.as_str()),
                                        p.get("device_name").and_then(|v| v.as_str()),
                                        p.get("os").and_then(|v| v.as_str()),
                                    ) {
                                        let lan = p.get("lan_endpoint").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        let online = p.get("online").and_then(|v| v.as_bool()).unwrap_or(true);
                                        let cert_sha256 = p.get("cert_sha256").and_then(|v| v.as_str()).map(|s| s.to_string());
                                        self.upsert_peer(DeviceStatusDto {
                                            device_id: id.to_string(),
                                            name: name.to_string(),
                                            os: os.to_string(),
                                            online,
                                            lan_reachable: online && lan.is_some(),
                                            lan_endpoint: lan,
                                            cert_sha256,
                                        });
                                    }
                                }
                            }
                        } else if msg_type == Some("CERT_REVOKE") {
                            if let Some(fps) = val.get("revoked_cert_fingerprints").and_then(|v| v.as_array()) {
                                let list: Vec<String> = fps.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect();
                                if let Some(ref cm) = self.cert_mgr {
                                    let _ = cm.add_revoked_fingerprints(&list);
                                }
                            }
                        } else if val.get("spec_version").is_some() {
                            if let Ok(env) = serde_json::from_str::<CloudEventEnvelope>(&text) {
                                let _ = self.handle_incoming_envelope(env);
                            }
                        }
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

async fn write_ws_frame<S: AsyncWriteExt + Unpin>(
    stream: &mut S,
    opcode: u8,
    payload: &[u8],
) -> anyhow::Result<()> {
    let mask_key: [u8; 4] = [0x37, 0xFA, 0x21, 0x3D];
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.push(0x80 | (opcode & 0x0F));
    if payload.len() < 126 {
        frame.push(0x80 | (payload.len() as u8));
    } else if payload.len() <= 65535 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask_key);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask_key[i % 4]);
    }
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

async fn write_ws_text_frame<S: AsyncWriteExt + Unpin>(
    stream: &mut S,
    text: &str,
) -> anyhow::Result<()> {
    write_ws_frame(stream, 0x01, text.as_bytes()).await
}

async fn read_ws_text_frame<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    stream: &mut S,
) -> anyhow::Result<String> {
    let mut message_buf = Vec::new();
    loop {
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).await?;
        let fin = (hdr[0] & 0x80) != 0;
        let opcode = hdr[0] & 0x0F;
        let masked = (hdr[1] & 0x80) != 0;
        let mut len = (hdr[1] & 0x7F) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).await?;
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext).await?;
            len = u64::from_be_bytes(ext) as usize;
        }
        if len > 16 * 1024 * 1024 {
            anyhow::bail!("WebSocket frame too large: {} bytes", len);
        }
        let mut mask = [0u8; 4];
        if masked {
            stream.read_exact(&mut mask).await?;
        }
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }

        match opcode {
            0x08 => anyhow::bail!("WebSocket closed by peer"),
            0x09 => {
                // Ping -> reply with Pong
                let _ = write_ws_frame(stream, 0x0A, &payload).await;
                continue;
            }
            0x0A => {
                // Pong -> ignore
                continue;
            }
            0x00 | 0x01 | 0x02 => {
                message_buf.extend_from_slice(&payload);
                if fin {
                    return Ok(String::from_utf8_lossy(&message_buf).to_string());
                }
            }
            _ => continue,
        }
    }
}


//! 本地回环 HTTP/IPC 服务端 (`http://127.0.0.1:53318/api/v1/*`)，供 `mi-cli`、`mi-os`、`mi-ui` 调用.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use mi_protocol::digest::{DigestManifest, DEFAULT_CHUNK_SIZE_BYTES};
use mi_protocol::envelope::{
    topic_sync_direct, CloudEventEnvelope, ContentCategory, EventPayload, EventType, ItemMeta,
    SourceDevice, TargetMode, TargetScope, UploadState, MAX_TRANSFER_TOTAL_SIZE, SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST,
};
use mi_protocol::ipc::{
    DeviceStatusDto, IpcCertRotateResponse, IpcCertStatusResponse, IpcCopyRequest,
    IpcCopyResponse, IpcEventsListResponse, IpcHealthResponse, IpcPairStatusResponse,
    IpcPasteRequest, IpcPasteResponse, IpcPinServerRequest, IpcRemoteLsRequest,
    IpcRemoteLsResponse, IpcRemoteMkdirRequest, IpcSyncRequest, IpcSyncResponse,
    IpcTransferActionRequest, IpcTransferStatusResponse,
};
use mi_protocol::rpc::{FsListDirRequest, FsMkdirRequest, FsMkdirResponse};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::cert_manager::CertManager;
use crate::cloud_client::CloudSignalingClient;
use crate::event_queue::EventCacheList;
use crate::fs_rpc::FsRpcHandler;
use crate::lan_channel::{
    read_async_http_request, write_async_http_response, LanChannelPool, LanSyncPushMetadata,
};
use crate::transfer::{
    unpack_archive_to_directory, ResumableDownloader, ResumableTaskContext, TransferManager,
};

#[derive(Debug, Clone)]
pub struct LocalIpcServer {
    signaling: CloudSignalingClient,
    event_queue: EventCacheList,
    lan_pool: LanChannelPool,
    fs_rpc: FsRpcHandler,
    transfer_mgr: TransferManager,
    cert_mgr: Option<CertManager>,
}

impl LocalIpcServer {
    pub fn new(signaling: CloudSignalingClient, event_queue: EventCacheList) -> Self {
        let cfg = signaling.config().clone();
        let lan_pool = LanChannelPool::new(Duration::from_secs(cfg.lan_idle_timeout_secs));
        let fs_rpc = FsRpcHandler::with_allowed_roots(cfg.device_id.clone(), cfg.allowed_dirs);
        let transfer_mgr = TransferManager::new();
        Self {
            signaling,
            event_queue,
            lan_pool,
            fs_rpc,
            transfer_mgr,
            cert_mgr: None,
        }
    }

    pub fn with_components(
        signaling: CloudSignalingClient,
        event_queue: EventCacheList,
        lan_pool: LanChannelPool,
        fs_rpc: FsRpcHandler,
        transfer_mgr: TransferManager,
    ) -> Self {
        Self {
            signaling,
            event_queue,
            lan_pool,
            fs_rpc,
            transfer_mgr,
            cert_mgr: None,
        }
    }

    pub fn set_cert_manager(&mut self, cert_mgr: CertManager) {
        self.cert_mgr = Some(cert_mgr);
    }

    pub fn transfer_manager(&self) -> &TransferManager {
        &self.transfer_mgr
    }

    pub fn lan_pool(&self) -> &LanChannelPool {
        &self.lan_pool
    }

    pub fn fs_rpc(&self) -> &FsRpcHandler {
        &self.fs_rpc
    }

    pub fn handle_health(&self) -> IpcHealthResponse {
        IpcHealthResponse {
            status: "ok".to_string(),
            device_id: self.signaling.config().device_id.clone(),
            device_name: self.signaling.config().device_name.clone(),
            local_ipc_addr: self.signaling.config().local_ipc_addr.clone(),
            lan_bind_addr: self.signaling.config().lan_bind_addr.clone(),
            active_lan_connections: self.lan_pool.active_connection_count(),
            cached_events: self.event_queue.len(),
        }
    }

    pub fn handle_devices_list(&self) -> Vec<DeviceStatusDto> {
        self.signaling.list_peers()
    }

    pub fn handle_events_list(&self, limit: usize) -> IpcEventsListResponse {
        IpcEventsListResponse {
            events: self.event_queue.list(limit),
        }
    }

    pub fn handle_events_clear(&self) {
        self.event_queue.clear();
    }

    pub fn handle_copy(
        &self,
        req: IpcCopyRequest,
        event_id: String,
        now_ms: u64,
    ) -> anyhow::Result<IpcCopyResponse> {
        if let Some(ref text) = req.text {
            let env = self.signaling.broadcast_text(event_id, text, now_ms);
            let _ = self.event_queue.push_verified(env.clone());
            return Ok(IpcCopyResponse {
                event_id: env.event_id,
                content_category: "TEXT".to_string(),
                content_hash: env.payload.digest.full_hash,
                file_cache_id: None,
                size_bytes: text.len() as u64,
            });
        }

        if let Some(ref path_str) = req.path {
            let source_path = Path::new(path_str);
            if !source_path.exists() {
                anyhow::bail!("Source path does not exist: {}", path_str);
            }
            let file_cache_id = format!("fc_{}_{}", self.signaling.config().device_id, now_ms);
            let staging_dir = self.signaling.config().data_dir.join("staging");
            let staged = self
                .transfer_mgr
                .stage_local_path(file_cache_id.clone(), source_path, &staging_dir)?;

            let is_dir = staged.content_category == ContentCategory::Directory;
            let cat_str = match staged.content_category {
                ContentCategory::Directory => "DIRECTORY",
                ContentCategory::Image => "IMAGE",
                ContentCategory::File => "FILE",
                ContentCategory::Text => "TEXT",
            };

            let session = crate::transfer::UploadSession {
                file_cache_id: file_cache_id.clone(),
                source_mtime: staged.source_mtime,
                total_size: staged.size_bytes,
                digest: staged.digest.clone(),
            };
            let _ = self.signaling.upload_file_to_cloud_cache(
                &session,
                &staged.local_path,
                &staged.original_name,
                cat_str,
            );

            let env = CloudEventEnvelope {
                spec_version: SPEC_VERSION_V6.to_string(),
                event_id: event_id.clone(),
                topic: TOPIC_CLIPBOARD_BROADCAST.to_string(),
                event_type: EventType::ClipboardBroadcast,
                timestamp_ms: now_ms,
                source_device: SourceDevice {
                    device_id: self.signaling.config().device_id.clone(),
                    device_name: self.signaling.config().device_name.clone(),
                    os: self.signaling.config().os_name.clone(),
                    lan_endpoint: Some(self.signaling.config().lan_bind_addr.clone()),
                },
                target_scope: TargetScope {
                    mode: TargetMode::Broadcast,
                    target_device_ids: vec!["*".to_string()],
                },
                payload: EventPayload {
                    content_category: staged.content_category.clone(),
                    digest: staged.digest.clone(),
                    inline_data: None,
                    file_cache_id: Some(file_cache_id.clone()),
                    upload_state: UploadState::Completed,
                    items_meta: vec![ItemMeta {
                        name: staged.original_name.clone(),
                        is_dir,
                        size_bytes: staged.size_bytes,
                        mtime: staged.source_mtime,
                    }],
                    destination_override: None,
                },
            };

            self.signaling.enqueue_envelope(env.clone());
            let _ = self.event_queue.push_verified(env);

            return Ok(IpcCopyResponse {
                event_id,
                content_category: cat_str.to_string(),
                content_hash: staged.digest.full_hash,
                file_cache_id: Some(file_cache_id),
                size_bytes: staged.size_bytes,
            });
        }

        anyhow::bail!("Either text or path must be provided to IpcCopyRequest")
    }

    pub fn handle_paste(&self, req: IpcPasteRequest) -> anyhow::Result<IpcPasteResponse> {
        let env = match req.event_id {
            Some(ref id) => self.event_queue.get_by_id(id),
            None => self.event_queue.latest(),
        }
        .ok_or_else(|| anyhow::anyhow!("No matching event in EventCacheList"))?;

        if let Some(ref text) = env.payload.inline_data {
            env.payload.digest.verify_bytes(text.as_bytes())?;
            let saved_path = if let Some(ref dest_dir) = req.dest_dir {
                let dir_path = Path::new(dest_dir);
                if dir_path.is_dir() {
                    let out_file = dir_path.join(format!("clipboard_{}.txt", env.event_id));
                    std::fs::write(&out_file, text.as_bytes())?;
                    Some(out_file.to_string_lossy().to_string())
                } else {
                    if let Some(parent) = dir_path.parent() {
                        if !parent.as_os_str().is_empty() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                    }
                    std::fs::write(dir_path, text.as_bytes())?;
                    Some(dest_dir.clone())
                }
            } else {
                None
            };
            return Ok(IpcPasteResponse {
                event_id: env.event_id,
                content_category: "TEXT".to_string(),
                saved_path,
                inline_text: Some(text.clone()),
                digest_verified: true,
                bytes_transferred: text.len() as u64,
                channel_used: "CLOUD_SIGNALING".to_string(),
            });
        }

        let file_cache_id = env
            .payload
            .file_cache_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Event missing file_cache_id"))?;
        let dest_dir = PathBuf::from(req.dest_dir.unwrap_or_else(|| ".".to_string()));
        std::fs::create_dir_all(&dest_dir)?;

        let item_meta = env.payload.items_meta.first().cloned().unwrap_or(ItemMeta {
            name: format!("{}.bin", file_cache_id),
            is_dir: env.payload.content_category == ContentCategory::Directory,
            size_bytes: 0,
            mtime: 0,
        });

        let is_dir = env.payload.content_category == ContentCategory::Directory || item_meta.is_dir;
        let download_target_path = if is_dir {
            dest_dir.join(format!(".{}.tar.zst", file_cache_id))
        } else {
            dest_dir.join(&item_meta.name)
        };

        let task_id = format!("tx_{}", env.event_id);
        let peer_lan = env.source_device.lan_endpoint.clone().or_else(|| {
            self.signaling
                .resolve_peer(&env.source_device.device_id)
                .and_then(|p| p.lan_endpoint)
        });

        let ctx = ResumableTaskContext {
            task_id: task_id.clone(),
            event_id: env.event_id.clone(),
            file_cache_id: file_cache_id.clone(),
            target_path: download_target_path,
            dest_dir: dest_dir.clone(),
            content_category: env.payload.content_category.clone(),
            total_size: item_meta.size_bytes,
            source_mtime: item_meta.mtime,
            digest: env.payload.digest.clone(),
            peer_device_id: env.source_device.device_id.clone(),
            peer_lan_endpoint: peer_lan,
        };
        self.transfer_mgr.register_resumable_context(ctx.clone());

        let (final_saved_path, channel_used, bytes_transferred) =
            self.execute_resumable_download(&ctx)?;

        let cat_str = match env.payload.content_category {
            ContentCategory::Directory => "DIRECTORY",
            ContentCategory::Image => "IMAGE",
            ContentCategory::File => "FILE",
            ContentCategory::Text => "TEXT",
        };

        Ok(IpcPasteResponse {
            event_id: env.event_id,
            content_category: cat_str.to_string(),
            saved_path: Some(final_saved_path.to_string_lossy().to_string()),
            inline_text: None,
            digest_verified: true,
            bytes_transferred,
            channel_used,
        })
    }

    fn execute_resumable_download(
        &self,
        ctx: &ResumableTaskContext,
    ) -> anyhow::Result<(PathBuf, String, u64)> {
        let mut downloader = ResumableDownloader::open_or_create(
            ctx.target_path.clone(),
            ctx.task_id.clone(),
            ctx.file_cache_id.clone(),
            ctx.total_size,
            ctx.source_mtime,
            ctx.digest.clone(),
            "LAN_DIRECT",
        )?;

        let start_offset = downloader.state.verified_offset;
        let (remaining_bytes, channel_used) = self.fetch_bytes_with_failover(
            &ctx.peer_device_id,
            ctx.peer_lan_endpoint.as_deref(),
            &ctx.file_cache_id,
            start_offset,
            &mut downloader,
        )?;

        if start_offset == 0 && ctx.total_size > 0 && remaining_bytes.len() as u64 != ctx.total_size
        {
            anyhow::bail!(
                "Incomplete payload received: got {} bytes, expected {}",
                remaining_bytes.len(),
                ctx.total_size
            );
        }

        let mut cursor = 0usize;
        let mut chunk_idx = (start_offset as usize) / DEFAULT_CHUNK_SIZE_BYTES;
        while cursor < remaining_bytes.len() {
            let end = (cursor + DEFAULT_CHUNK_SIZE_BYTES).min(remaining_bytes.len());
            let slice = &remaining_bytes[cursor..end];
            downloader.append_verified_chunk(chunk_idx, slice)?;
            cursor = end;
            chunk_idx += 1;
        }

        let verified_archive_or_file = downloader.finalize_and_verify()?;
        let final_path = if ctx.content_category == ContentCategory::Directory {
            let _ = unpack_archive_to_directory(
                &verified_archive_or_file,
                &ctx.dest_dir,
                MAX_TRANSFER_TOTAL_SIZE,
            )?;
            let _ = std::fs::remove_file(&verified_archive_or_file);
            ctx.dest_dir.clone()
        } else {
            verified_archive_or_file
        };

        let transferred = if ctx.total_size > 0 {
            ctx.total_size
        } else {
            remaining_bytes.len() as u64
        };

        self.transfer_mgr.upsert_task(IpcTransferStatusResponse {
            task_id: ctx.task_id.clone(),
            file_cache_id: ctx.file_cache_id.clone(),
            status: "COMPLETED".to_string(),
            verified_offset: transferred,
            total_size: transferred,
            digest_verified: true,
            channel_used: channel_used.clone(),
            resume_count: downloader.state.resume_count,
        });

        Ok((final_path, channel_used, transferred))
    }

    fn fetch_bytes_with_failover(
        &self,
        peer_device_id: &str,
        peer_lan_endpoint: Option<&str>,
        file_cache_id: &str,
        start_offset: u64,
        downloader: &mut ResumableDownloader,
    ) -> anyhow::Result<(Vec<u8>, String)> {
        // 1. Try LAN direct first if endpoint is present and reachable
        if let Some(endpoint) = peer_lan_endpoint {
            if let Ok(bytes) = self.lan_pool.fetch_file_range(
                peer_device_id,
                endpoint,
                file_cache_id,
                start_offset,
            ) {
                return Ok((bytes, "LAN_DIRECT".to_string()));
            }
        }

        // 2. Try local staged file if same machine / unit test
        if let Ok((bytes, _)) = self
            .transfer_mgr
            .read_staged_range(file_cache_id, start_offset)
        {
            return Ok((bytes, "LAN_DIRECT".to_string()));
        }

        // 3. Failover to Cloud Cache with Range header
        let _ = downloader.failover_to_channel("CLOUD_CACHE");
        let bytes = self
            .signaling
            .fetch_cloud_cache_range(file_cache_id, start_offset)?;
        Ok((bytes, "CLOUD_CACHE".to_string()))
    }

    pub fn handle_remote_ls(&self, req: IpcRemoteLsRequest) -> anyhow::Result<IpcRemoteLsResponse> {
        let rpc_req = FsListDirRequest {
            request_id: format!("ls_{}", now_unix_ms()),
            target_device_id: req.device.clone(),
            path: req.path.clone(),
            include_files: true,
        };

        let fs_resp = if req.device == self.signaling.config().device_id
            || req.device.eq_ignore_ascii_case(&self.signaling.config().device_name)
            || req.device == "local"
            || req.device == "self"
        {
            self.fs_rpc.handle_list_dir(&rpc_req)?
        } else if let Some(peer) = self.signaling.resolve_peer(&req.device) {
            if let Some(ref endpoint) = peer.lan_endpoint {
                self.lan_pool.rpc_list_dir(&peer.device_id, endpoint, &rpc_req)?
            } else {
                anyhow::bail!("Target device '{}' has no reachable LAN endpoint", req.device);
            }
        } else if req.device.contains(':') {
            self.lan_pool.rpc_list_dir(&req.device, &req.device, &rpc_req)?
        } else {
            anyhow::bail!("Target device '{}' is offline or unknown", req.device);
        };

        Ok(IpcRemoteLsResponse {
            device_id: fs_resp.device_id,
            path: fs_resp.current_path,
            entries: fs_resp.entries,
            free_space_bytes: fs_resp.free_space_bytes,
        })
    }

    pub fn handle_remote_mkdir(
        &self,
        req: IpcRemoteMkdirRequest,
    ) -> anyhow::Result<FsMkdirResponse> {
        let rpc_req = FsMkdirRequest {
            request_id: format!("mkdir_{}", now_unix_ms()),
            target_device_id: req.device.clone(),
            path: req.path,
        };

        if req.device == self.signaling.config().device_id
            || req.device.eq_ignore_ascii_case(&self.signaling.config().device_name)
            || req.device == "local"
            || req.device == "self"
        {
            return Ok(self.fs_rpc.handle_mkdir(&rpc_req));
        }

        if let Some(peer) = self.signaling.resolve_peer(&req.device) {
            if let Some(ref endpoint) = peer.lan_endpoint {
                return self.lan_pool.rpc_mkdir(&peer.device_id, endpoint, &rpc_req);
            }
            anyhow::bail!("Target device '{}' has no reachable LAN endpoint", req.device);
        }

        if req.device.contains(':') {
            return self.lan_pool.rpc_mkdir(&req.device, &req.device, &rpc_req);
        }

        anyhow::bail!("Target device '{}' is offline or unknown", req.device)
    }

    pub fn handle_sync(&self, req: IpcSyncRequest, now_ms: u64) -> anyhow::Result<IpcSyncResponse> {
        const LAN_DIRECT_PUSH_MAX_BYTES: u64 = 16 * 1024 * 1024;
        let transfer_id = format!("sync_{}", now_ms);
        let event_id = format!("evt_sync_{}", now_ms);
        let peer_opt = self.signaling.resolve_peer(&req.target_device);
        let target_device_id = peer_opt
            .as_ref()
            .map(|p| p.device_id.clone())
            .unwrap_or_else(|| req.target_device.clone());
        let peer_endpoint = peer_opt
            .and_then(|p| p.lan_endpoint)
            .or_else(|| req.target_device.contains(':').then(|| req.target_device.clone()));
        let is_local_target = target_device_id == self.signaling.config().device_id
            || req.target_device == "local"
            || req.target_device == "self";

        let src_path = Path::new(&req.source);
        let (file_cache_id, local_staged_path, file_name, is_dir_archive, size_bytes, source_mtime, digest, category) =
            if src_path.exists() {
                let fc_id = format!("fc_sync_{}", now_ms);
                let staging_dir = self.signaling.config().data_dir.join("staging");
                let staged = self
                    .transfer_mgr
                    .stage_local_path(fc_id.clone(), src_path, &staging_dir)?;
                (
                    fc_id,
                    Some(staged.local_path),
                    staged.original_name,
                    staged.content_category == ContentCategory::Directory,
                    staged.size_bytes,
                    staged.source_mtime,
                    staged.digest,
                    staged.content_category,
                )
            } else {
                let bytes = req.source.as_bytes();
                let digest = DigestManifest::from_bytes(bytes);
                let staging_dir = self.signaling.config().data_dir.join("staging");
                std::fs::create_dir_all(&staging_dir)?;
                let fc_id = format!("fc_text_{}", now_ms);
                let file_name = format!("sync_note_{}.txt", now_ms);
                let note_path = staging_dir.join(&file_name);
                std::fs::write(&note_path, bytes)?;
                (
                    fc_id,
                    Some(note_path),
                    file_name,
                    false,
                    bytes.len() as u64,
                    now_ms / 1000,
                    digest,
                    ContentCategory::Text,
                )
            };

        let meta = LanSyncPushMetadata {
            file_name: file_name.clone(),
            remote_dir: req.remote_dir.clone(),
            is_directory_archive: is_dir_archive,
            digest: digest.clone(),
        };

        let staged_path = local_staged_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Missing staged path for sync"))?;

        let (saved_path, channel_used, status_str) = if let Some(ref endpoint) = peer_endpoint {
            if size_bytes <= LAN_DIRECT_PUSH_MAX_BYTES {
                let payload_bytes = std::fs::read(staged_path)?;
                let push_resp = self
                    .lan_pool
                    .push_sync_payload(&target_device_id, endpoint, &meta, &payload_bytes)?;
                if !push_resp.ok {
                    anyhow::bail!(
                        "Remote sync rejected by peer: {}",
                        push_resp.error.unwrap_or_default()
                    );
                }
                (push_resp.saved_path, "LAN_DIRECT".to_string(), "VERIFIED_AND_SAVED".to_string())
            } else {
                let session = crate::transfer::UploadSession {
                    file_cache_id: file_cache_id.clone(),
                    source_mtime,
                    total_size: size_bytes,
                    digest: digest.clone(),
                };
                let cat_str = if is_dir_archive { "DIRECTORY" } else { "FILE" };
                let _ = self.signaling.upload_file_to_cloud_cache(
                    &session,
                    staged_path,
                    &file_name,
                    cat_str,
                );
                (
                    format!("{}/{}", req.remote_dir.trim_end_matches('/'), file_name),
                    "LAN_DIRECT".to_string(),
                    "DISPATCHED".to_string(),
                )
            }
        } else if is_local_target {
            let dest_root = Path::new(&req.remote_dir);
            std::fs::create_dir_all(dest_root)?;
            if is_dir_archive {
                unpack_archive_to_directory(staged_path, dest_root, MAX_TRANSFER_TOTAL_SIZE)?;
                (
                    dest_root.join(&file_name).to_string_lossy().to_string(),
                    "LOCAL".to_string(),
                    "VERIFIED_AND_SAVED".to_string(),
                )
            } else {
                let target_file = dest_root.join(&file_name);
                std::fs::copy(staged_path, &target_file)?;
                digest.verify_file(&target_file)?;
                (
                    target_file.to_string_lossy().to_string(),
                    "LOCAL".to_string(),
                    "VERIFIED_AND_SAVED".to_string(),
                )
            }
        } else {
            let session = crate::transfer::UploadSession {
                file_cache_id: file_cache_id.clone(),
                source_mtime,
                total_size: size_bytes,
                digest: digest.clone(),
            };
            let cat_str = if is_dir_archive { "DIRECTORY" } else { "FILE" };
            self.signaling.upload_file_to_cloud_cache(
                &session,
                staged_path,
                &file_name,
                cat_str,
            )?;
            (
                format!("{}/{}", req.remote_dir.trim_end_matches('/'), file_name),
                "CLOUD_CACHE".to_string(),
                "DISPATCHED".to_string(),
            )
        };

        let env = CloudEventEnvelope {
            spec_version: SPEC_VERSION_V6.to_string(),
            event_id: event_id.clone(),
            topic: topic_sync_direct(&target_device_id),
            event_type: EventType::DirectSync,
            timestamp_ms: now_ms,
            source_device: SourceDevice {
                device_id: self.signaling.config().device_id.clone(),
                device_name: self.signaling.config().device_name.clone(),
                os: self.signaling.config().os_name.clone(),
                lan_endpoint: Some(self.signaling.config().lan_bind_addr.clone()),
            },
            target_scope: TargetScope {
                mode: TargetMode::Direct,
                target_device_ids: vec![target_device_id.clone()],
            },
            payload: EventPayload {
                content_category: category,
                digest: digest.clone(),
                inline_data: None,
                file_cache_id: Some(file_cache_id.clone()),
                upload_state: UploadState::Completed,
                items_meta: vec![ItemMeta {
                    name: file_name,
                    is_dir: is_dir_archive,
                    size_bytes,
                    mtime: source_mtime,
                }],
                destination_override: Some(req.remote_dir.clone()),
            },
        };
        self.signaling.enqueue_envelope(env);

        self.transfer_mgr.upsert_task(IpcTransferStatusResponse {
            task_id: transfer_id.clone(),
            file_cache_id,
            status: "COMPLETED".to_string(),
            verified_offset: size_bytes,
            total_size: size_bytes,
            digest_verified: true,
            channel_used: channel_used.clone(),
            resume_count: 0,
        });

        Ok(IpcSyncResponse {
            transfer_id,
            event_id,
            status: status_str,
            target_device: target_device_id,
            remote_saved_path: saved_path,
            digest_verified: true,
            channel_used,
            bytes_transferred: size_bytes,
        })
    }

    pub fn handle_transfer_status(&self, task_id: &str) -> anyhow::Result<IpcTransferStatusResponse> {
        self.transfer_mgr
            .get_task_status(task_id)
            .ok_or_else(|| anyhow::anyhow!("Transfer task not found: {}", task_id))
    }

    pub fn handle_transfer_resume(
        &self,
        req: IpcTransferActionRequest,
    ) -> anyhow::Result<IpcTransferStatusResponse> {
        if let Some(ctx) = self.transfer_mgr.get_resumable_context(&req.task_id) {
            let _ = self.execute_resumable_download(&ctx)?;
        }
        self.handle_transfer_status(&req.task_id)
    }

    pub fn handle_cert_status(&self) -> anyhow::Result<IpcCertStatusResponse> {
        let cm = self
            .cert_mgr
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CertManager not initialized"))?;
        let id = cm.local_identity();
        let ts = cm.trust_state();
        Ok(IpcCertStatusResponse {
            device_id: self.signaling.config().device_id.clone(),
            local_cert_sha256: id.cert_sha256,
            local_cert_expires_at: id.expires_at,
            pinned_server_cert_sha256: ts.pinned_server_cert_sha256,
            backup_server_cert_sha256: ts.backup_server_cert_sha256,
            revoked_fingerprints_count: ts.revoked_fingerprints.len(),
        })
    }

    pub fn handle_cert_rotate(&self) -> anyhow::Result<IpcCertRotateResponse> {
        let cm = self
            .cert_mgr
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CertManager not initialized"))?;
        let (old_fp, new_fp, expires_at) = cm.rotate_local_cert()?;
        let cleared = self.lan_pool.clear_all_sessions();
        let notice = cm.build_rotation_notice(&old_fp, &new_fp, expires_at);
        if let Ok(notice_json) = serde_json::to_string(&notice) {
            let now_ms = now_unix_ms();
            let env = CloudEventEnvelope {
                spec_version: SPEC_VERSION_V6.to_string(),
                event_id: format!("evt_cert_rotate_{}", now_ms),
                topic: mi_protocol::envelope::TOPIC_SYSTEM_PRESENCE.to_string(),
                event_type: EventType::DevicePresence,
                timestamp_ms: now_ms,
                source_device: SourceDevice {
                    device_id: self.signaling.config().device_id.clone(),
                    device_name: self.signaling.config().device_name.clone(),
                    os: self.signaling.config().os_name.clone(),
                    lan_endpoint: Some(self.signaling.config().lan_bind_addr.clone()),
                },
                target_scope: TargetScope {
                    mode: TargetMode::Broadcast,
                    target_device_ids: vec!["*".to_string()],
                },
                payload: EventPayload {
                    content_category: ContentCategory::Text,
                    digest: DigestManifest::from_bytes(notice_json.as_bytes()),
                    inline_data: Some(notice_json),
                    file_cache_id: None,
                    upload_state: UploadState::Inline,
                    items_meta: vec![],
                    destination_override: None,
                },
            };
            self.signaling.enqueue_envelope(env);
        }
        Ok(IpcCertRotateResponse {
            old_cert_sha256: old_fp,
            new_cert_sha256: new_fp,
            expires_at,
            lan_sessions_cleared: cleared,
        })
    }

    pub fn handle_cert_pin_server(
        &self,
        req: IpcPinServerRequest,
    ) -> anyhow::Result<IpcCertStatusResponse> {
        let cm = self
            .cert_mgr
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CertManager not initialized"))?;
        cm.pin_server_cert(&req.sha256)?;
        self.handle_cert_status()
    }

    pub fn handle_pair_status(&self) -> anyhow::Result<IpcPairStatusResponse> {
        let cm = self
            .cert_mgr
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CertManager not initialized"))?;
        let id = cm.local_identity();
        let ts = cm.trust_state();
        let sas_code = ts.pinned_server_cert_sha256.as_deref().map(|s_fp| {
            mi_protocol::envelope::compute_sas_pairing_code(
                s_fp,
                &id.cert_sha256,
                &self.signaling.config().device_id,
            )
        });
        Ok(IpcPairStatusResponse {
            device_id: self.signaling.config().device_id.clone(),
            client_cert_sha256: id.cert_sha256,
            server_cert_sha256: ts.pinned_server_cert_sha256.clone(),
            sas_code,
            paired: ts.pinned_server_cert_sha256.is_some(),
        })
    }

    /// 启动本地回环 HTTP/IPC 监听服务 (`http://127.0.0.1:53318/api/v1/*`).
    pub async fn bind_and_serve(
        self,
        bind_addr: &str,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let srv = self.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_res = listener.accept() => {
                        if let Ok((mut stream, _)) = accept_res {
                            let srv_clone = srv.clone();
                            tokio::spawn(async move {
                                let _ = srv_clone.handle_http_conn(&mut stream).await;
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

    async fn handle_http_conn(&self, stream: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
        let (method, full_path, _headers, body) = read_async_http_request(stream).await?;
        let (path, query) = full_path
            .split_once('?')
            .unwrap_or((full_path.as_str(), ""));

        if method == "OPTIONS" {
            write_async_http_response(stream, 200, "application/json", b"{}").await?;
            return Ok(());
        }

        let res: anyhow::Result<Vec<u8>> = match (method.as_str(), path) {
            ("GET", "/api/v1/health") => Ok(serde_json::to_vec(&self.handle_health())?),
            ("GET", "/api/v1/devices") => Ok(serde_json::to_vec(&self.handle_devices_list())?),
            ("GET", "/api/v1/events") => {
                let limit = parse_query_param(query, "limit")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(10);
                Ok(serde_json::to_vec(&self.handle_events_list(limit))?)
            }
            ("DELETE", "/api/v1/events") => {
                self.handle_events_clear();
                Ok(b"{\"cleared\":true}".to_vec())
            }
            ("POST", "/api/v1/copy") => {
                let req: IpcCopyRequest = serde_json::from_slice(&body)?;
                let now_ms = now_unix_ms();
                let event_id = format!("evt_{}_{}", self.signaling.config().device_id, now_ms);
                let resp = self.handle_copy(req, event_id, now_ms)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/paste") => {
                let req: IpcPasteRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_paste(req)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/remote-ls") => {
                let req: IpcRemoteLsRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_remote_ls(req)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("GET", "/api/v1/remote-ls") => {
                let device = parse_query_param(query, "device").unwrap_or_else(|| "local".to_string());
                let dir_path = parse_query_param(query, "path").unwrap_or_else(|| ".".to_string());
                let resp = self.handle_remote_ls(IpcRemoteLsRequest {
                    device,
                    path: dir_path,
                })?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/remote-mkdir") => {
                let req: IpcRemoteMkdirRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_remote_mkdir(req)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/sync") => {
                let req: IpcSyncRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_sync(req, now_unix_ms())?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("GET", "/api/v1/transfer/status") => {
                let task_id = parse_query_param(query, "task_id").unwrap_or_else(|| "latest".to_string());
                let resp = self.handle_transfer_status(&task_id)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/transfer/resume") => {
                let req: IpcTransferActionRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_transfer_resume(req)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("GET", "/api/v1/cert/status") => {
                let resp = self.handle_cert_status()?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/cert/rotate") => {
                let resp = self.handle_cert_rotate()?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("POST", "/api/v1/cert/pin-server") => {
                let req: IpcPinServerRequest = serde_json::from_slice(&body)?;
                let resp = self.handle_cert_pin_server(req)?;
                Ok(serde_json::to_vec(&resp)?)
            }
            ("GET", "/api/v1/pair/status") => {
                let resp = self.handle_pair_status()?;
                Ok(serde_json::to_vec(&resp)?)
            }
            _ => {
                write_async_http_response(
                    stream,
                    404,
                    "application/json",
                    b"{\"error\":\"route not found\"}",
                )
                .await?;
                return Ok(());
            }
        };

        match res {
            Ok(json_bytes) => {
                write_async_http_response(stream, 200, "application/json", &json_bytes).await?;
            }
            Err(err) => {
                let err_json = serde_json::json!({ "error": err.to_string() });
                write_async_http_response(
                    stream,
                    400,
                    "application/json",
                    err_json.to_string().as_bytes(),
                )
                .await?;
            }
        }
        Ok(())
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn parse_query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(url_decode(v));
            }
        }
    }
    None
}

fn url_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}


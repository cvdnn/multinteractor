//! 本地回环 HTTP/IPC 服务端 (`http://127.0.0.1:53318/api/v1/*`)，供 `mi-cli`、`mi-os`、`mi-ui` 调用.

use mi_protocol::ipc::{
    DeviceStatusDto, IpcCopyRequest, IpcCopyResponse, IpcEventsListResponse, IpcPasteRequest,
    IpcPasteResponse,
};
use crate::cloud_client::CloudSignalingClient;
use crate::event_queue::EventCacheList;

#[derive(Debug, Clone)]
pub struct LocalIpcServer {
    signaling: CloudSignalingClient,
    event_queue: EventCacheList,
}

impl LocalIpcServer {
    pub fn new(signaling: CloudSignalingClient, event_queue: EventCacheList) -> Self {
        Self {
            signaling,
            event_queue,
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

    pub fn handle_copy(&self, req: IpcCopyRequest, event_id: String, now_ms: u64) -> anyhow::Result<IpcCopyResponse> {
        if let Some(ref text) = req.text {
            let env = self.signaling.broadcast_text(event_id, text, now_ms);
            return Ok(IpcCopyResponse {
                event_id: env.event_id,
                content_category: "TEXT".to_string(),
                content_hash: env.payload.digest.full_hash,
                file_cache_id: None,
                size_bytes: text.len() as u64,
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
            return Ok(IpcPasteResponse {
                event_id: env.event_id,
                content_category: "TEXT".to_string(),
                saved_path: req.dest_dir,
                inline_text: Some(text.clone()),
                digest_verified: true,
                bytes_transferred: text.len() as u64,
                channel_used: "CLOUD_SIGNALING".to_string(),
            });
        }

        anyhow::bail!("File/directory paste handled by transfer engine")
    }
}

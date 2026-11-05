//! 云端常驻 WebSocket 信令客户端 (注册鉴权、心跳保活、Topic 订阅同步、在线设备内网 IP:Port 同步).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use mi_protocol::digest::DigestManifest;
use mi_protocol::envelope::{
    topic_rpc_fs, topic_sync_direct, CloudEventEnvelope, ContentCategory, DeviceRegistrationFrame,
    EventPayload, EventType, SourceDevice, TargetMode, TargetScope, UploadState, SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST, TOPIC_SYSTEM_PRESENCE,
};
use mi_protocol::ipc::DeviceStatusDto;
use crate::config::DaemonConfig;
use crate::event_queue::EventCacheList;

#[derive(Debug, Clone)]
pub struct CloudSignalingClient {
    config: DaemonConfig,
    event_queue: EventCacheList,
    peers: Arc<RwLock<HashMap<String, DeviceStatusDto>>>,
    outbox: Arc<RwLock<Vec<CloudEventEnvelope>>>,
}

impl CloudSignalingClient {
    pub fn new(config: DaemonConfig, event_queue: EventCacheList) -> Self {
        Self {
            config,
            event_queue,
            peers: Arc::new(RwLock::new(HashMap::new())),
            outbox: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn build_registration_frame(&self) -> DeviceRegistrationFrame {
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
        }
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
        let mut guard = self.outbox.write().expect("outbox lock poisoned");
        guard.push(env.clone());
        env
    }

    pub fn handle_incoming_envelope(&self, envelope: CloudEventEnvelope) -> anyhow::Result<()> {
        match envelope.event_type {
            EventType::ClipboardBroadcast | EventType::DirectSync => {
                self.event_queue.push_verified(envelope)?;
            }
            EventType::DevicePresence => {
                if let Some(ref inline_json) = envelope.payload.inline_data {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(inline_json) {
                        let online = val.get("state").and_then(|s| s.as_str()) == Some("ONLINE");
                        let dto = DeviceStatusDto {
                            device_id: envelope.source_device.device_id.clone(),
                            name: envelope.source_device.device_name.clone(),
                            os: envelope.source_device.os.clone(),
                            online,
                            lan_endpoint: envelope.source_device.lan_endpoint.clone(),
                            lan_reachable: online && envelope.source_device.lan_endpoint.is_some(),
                        };
                        let mut peers = self.peers.write().expect("peers lock poisoned");
                        peers.insert(dto.device_id.clone(), dto);
                    }
                }
            }
            EventType::FsRpc => {}
        }
        Ok(())
    }

    pub fn list_peers(&self) -> Vec<DeviceStatusDto> {
        let guard = self.peers.read().expect("peers lock poisoned");
        let mut list: Vec<DeviceStatusDto> = guard.values().cloned().collect();
        list.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        list
    }
}

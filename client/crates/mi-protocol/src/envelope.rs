//! 统一云端事件信封 (`CloudEventEnvelope` v6)、Topic 与 EventType 定义.

use serde::{Deserialize, Serialize};
use crate::digest::DigestManifest;

pub const SPEC_VERSION_V6: &str = "v6";

pub const TOPIC_CLIPBOARD_BROADCAST: &str = "topic.clipboard.broadcast";
pub const TOPIC_SYSTEM_PRESENCE: &str = "topic.system.presence";

pub fn topic_sync_direct(device_id: &str) -> String {
    format!("topic.sync.direct.{}", device_id)
}

pub fn topic_rpc_fs(device_id: &str) -> String {
    format!("topic.rpc.fs.{}", device_id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    #[serde(rename = "EVENT_CLIPBOARD_BROADCAST")]
    ClipboardBroadcast,
    #[serde(rename = "EVENT_DIRECT_SYNC")]
    DirectSync,
    #[serde(rename = "EVENT_DEVICE_PRESENCE")]
    DevicePresence,
    #[serde(rename = "EVENT_FS_RPC")]
    FsRpc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetMode {
    #[serde(rename = "BROADCAST")]
    Broadcast,
    #[serde(rename = "DIRECT")]
    Direct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentCategory {
    #[serde(rename = "TEXT")]
    Text,
    #[serde(rename = "IMAGE")]
    Image,
    #[serde(rename = "FILE")]
    File,
    #[serde(rename = "DIRECTORY")]
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UploadState {
    #[serde(rename = "INLINE")]
    Inline,
    #[serde(rename = "STREAMING")]
    Streaming,
    #[serde(rename = "COMPLETED")]
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDevice {
    pub device_id: String,
    pub device_name: String,
    pub os: String,
    pub lan_endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetScope {
    pub mode: TargetMode,
    pub target_device_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemMeta {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mtime: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPayload {
    pub content_category: ContentCategory,
    pub digest: DigestManifest,
    pub inline_data: Option<String>,
    pub file_cache_id: Option<String>,
    pub upload_state: UploadState,
    pub items_meta: Vec<ItemMeta>,
    pub destination_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudEventEnvelope {
    pub spec_version: String,
    pub event_id: String,
    pub topic: String,
    pub event_type: EventType,
    pub timestamp_ms: u64,
    pub source_device: SourceDevice,
    pub target_scope: TargetScope,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistrationFrame {
    pub device_id: String,
    pub device_name: String,
    pub os: String,
    pub lan_endpoint: String,
    pub auth_token: String,
    pub subscribed_topics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionFrame {
    pub device_id: String,
    pub subscribe_topics: Vec<String>,
    pub unsubscribe_topics: Vec<String>,
    pub allowed_categories: Option<Vec<ContentCategory>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_envelope_roundtrip() {
        let raw = include_str!("../../../../tests/fixtures/envelope_v6_golden.json");
        let env: CloudEventEnvelope = serde_json::from_str(raw).expect("valid golden envelope");
        assert_eq!(env.spec_version, "v6");
        assert_eq!(env.topic, TOPIC_CLIPBOARD_BROADCAST);
        assert_eq!(env.source_device.device_id, "dev_b_laptop");
        assert_eq!(env.payload.content_category, ContentCategory::File);
        assert_eq!(env.payload.digest.chunk_size_bytes, 4_194_304);
    }
}

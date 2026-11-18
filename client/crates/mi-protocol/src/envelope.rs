//! 统一云端事件信封 (`CloudEventEnvelope` v6)、Topic 与 EventType 定义.

use serde::{Deserialize, Serialize};
use crate::digest::DigestManifest;

pub const SPEC_VERSION_V6: &str = "v6";

pub const MAX_HELLO_FRAME_SIZE: usize = 1024;
pub const MAX_INLINE_DATA_SIZE: usize = 1024 * 1024;
pub const MAX_CHUNK_SIZE: usize = 4 * 1024 * 1024;
pub const MAX_TRANSFER_TOTAL_SIZE: u64 = 100 * 1024 * 1024 * 1024;
pub const MAX_PATH_LENGTH: usize = 1024;

pub const TOPIC_CLIPBOARD_BROADCAST: &str = "topic.clipboard.broadcast";
pub const TOPIC_SYSTEM_PRESENCE: &str = "topic.system.presence";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolValidationError {
    #[error("Unsupported spec_version: {0} (expected v6)")]
    UnsupportedSpecVersion(String),
    #[error("Invalid file_cache_id '{0}': must be 1-64 characters [A-Za-z0-9_-]")]
    InvalidFileCacheId(String),
    #[error("inline_data size {0} exceeds MAX_INLINE_DATA_SIZE ({MAX_INLINE_DATA_SIZE} bytes)")]
    InlineDataTooLarge(usize),
    #[error("chunk_size_bytes {0} must be between 1 and {MAX_CHUNK_SIZE}")]
    InvalidChunkSize(usize),
    #[error("Total transfer size {0} exceeds MAX_TRANSFER_TOTAL_SIZE ({MAX_TRANSFER_TOTAL_SIZE} bytes)")]
    TransferTotalSizeExceeded(u64),
    #[error("Item name or path length {0} exceeds MAX_PATH_LENGTH ({MAX_PATH_LENGTH} bytes)")]
    PathTooLong(usize),
    #[error("Hello registration frame size {0} exceeds MAX_HELLO_FRAME_SIZE ({MAX_HELLO_FRAME_SIZE} bytes)")]
    HelloFrameTooLarge(usize),
}

pub fn validate_file_cache_id(file_cache_id: &str) -> Result<(), ProtocolValidationError> {
    if file_cache_id.is_empty()
        || file_cache_id.len() > 64
        || !file_cache_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(ProtocolValidationError::InvalidFileCacheId(
            file_cache_id.to_string(),
        ));
    }
    Ok(())
}

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

impl CloudEventEnvelope {
    /// 执行协议边界前置防御校验 (`OPTIMIZATION_AND_SECURITY_DESIGN.md` 2.2.1).
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        if self.spec_version != SPEC_VERSION_V6 {
            return Err(ProtocolValidationError::UnsupportedSpecVersion(
                self.spec_version.clone(),
            ));
        }
        if let Some(ref cid) = self.payload.file_cache_id {
            validate_file_cache_id(cid)?;
        }
        if let Some(ref inline) = self.payload.inline_data {
            if inline.len() > MAX_INLINE_DATA_SIZE {
                return Err(ProtocolValidationError::InlineDataTooLarge(inline.len()));
            }
        }
        let chunk_sz = self.payload.digest.chunk_size_bytes;
        if chunk_sz == 0 || chunk_sz > MAX_CHUNK_SIZE {
            return Err(ProtocolValidationError::InvalidChunkSize(chunk_sz));
        }
        let mut total_size: u64 = 0;
        for item in &self.payload.items_meta {
            if item.name.len() > MAX_PATH_LENGTH {
                return Err(ProtocolValidationError::PathTooLong(item.name.len()));
            }
            total_size = total_size.saturating_add(item.size_bytes);
        }
        if total_size > MAX_TRANSFER_TOTAL_SIZE {
            return Err(ProtocolValidationError::TransferTotalSizeExceeded(total_size));
        }
        if let Some(ref dest) = self.payload.destination_override {
            if dest.len() > MAX_PATH_LENGTH {
                return Err(ProtocolValidationError::PathTooLong(dest.len()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistrationFrame {
    pub device_id: String,
    pub device_name: String,
    pub os: String,
    pub lan_endpoint: String,
    pub auth_token: String,
    pub subscribed_topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_expires_at: Option<u64>,
}

impl DeviceRegistrationFrame {
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        let encoded_len = serde_json::to_string(self)
            .map(|s| s.len())
            .unwrap_or(usize::MAX);
        if encoded_len > MAX_HELLO_FRAME_SIZE {
            return Err(ProtocolValidationError::HelloFrameTooLarge(encoded_len));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairChallenge {
    pub device_id: String,
    pub server_cert_sha256: String,
    pub client_cert_sha256: String,
    pub nonce: String,
    pub sas_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairConfirm {
    pub device_id: String,
    pub sas_code: String,
    pub approved: bool,
    pub device_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertRotateNotice {
    pub device_id: String,
    pub old_cert_sha256: String,
    pub new_cert_sha256: String,
    pub new_cert_expires_at: u64,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertRevokeList {
    pub revoked_cert_fingerprints: Vec<String>,
    pub updated_at_ms: u64,
}

/// 计算 6 位指纹绑定配对校验码 (SAS): `Trunc6(BLAKE3("MI-PAIR-V1|" || S_fp || "|" || C_fp || "|" || nonce))`.
pub fn compute_sas_pairing_code(
    server_cert_sha256: &str,
    client_cert_sha256: &str,
    nonce: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"MI-PAIR-V1|");
    hasher.update(server_cert_sha256.trim().to_ascii_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(client_cert_sha256.trim().to_ascii_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(nonce.trim().as_bytes());
    let digest = hasher.finalize();
    let bytes = digest.as_bytes();
    let num = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000;
    format!("{:06}", num)
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
        assert!(env.validate().is_ok());

        let reserialized = serde_json::to_value(&env).expect("reserialize");
        let expected: serde_json::Value = serde_json::from_str(raw).expect("expected json");
        assert_eq!(reserialized, expected);
    }

    #[test]
    fn test_defensive_protocol_limits_validation() {
        assert!(validate_file_cache_id("fc_20261105_99887766").is_ok());
        assert!(validate_file_cache_id("../etc/passwd").is_err());
        assert!(validate_file_cache_id("").is_err());

        let raw = include_str!("../../../../tests/fixtures/envelope_v6_golden.json");
        let mut env: CloudEventEnvelope = serde_json::from_str(raw).unwrap();
        env.payload.file_cache_id = Some("../bad".to_string());
        assert_eq!(
            env.validate(),
            Err(ProtocolValidationError::InvalidFileCacheId("../bad".to_string()))
        );
    }
}

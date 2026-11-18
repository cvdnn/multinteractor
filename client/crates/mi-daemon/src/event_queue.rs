//! 本地接收事件缓存环形队列 (`EventCacheList`，默认容量 50 条) (FR-COPY-02).

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use mi_protocol::digest::DigestError;
use mi_protocol::envelope::{CloudEventEnvelope, ContentCategory};

#[derive(Debug, Clone)]
pub struct EventCacheList {
    capacity: usize,
    inner: Arc<RwLock<VecDeque<CloudEventEnvelope>>>,
    snapshot_path: Option<PathBuf>,
}

impl EventCacheList {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(capacity.max(1)))),
            snapshot_path: None,
        }
    }

    pub fn with_persistence(capacity: usize, snapshot_path: PathBuf) -> Self {
        let cap = capacity.max(1);
        let mut initial = VecDeque::with_capacity(cap);
        if let Ok(raw) = fs::read_to_string(&snapshot_path) {
            if let Ok(items) = serde_json::from_str::<Vec<CloudEventEnvelope>>(&raw) {
                for item in items.into_iter().take(cap) {
                    initial.push_back(item);
                }
            }
        }
        Self {
            capacity: cap,
            inner: Arc::new(RwLock::new(initial)),
            snapshot_path: Some(snapshot_path),
        }
    }

    fn persist_snapshot(&self, queue: &VecDeque<CloudEventEnvelope>) {
        if let Some(ref path) = self.snapshot_path {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&queue.iter().collect::<Vec<_>>()) {
                let tmp = path.with_extension("json.tmp");
                if fs::write(&tmp, json).is_ok() {
                    let _ = fs::rename(&tmp, path);
                }
            }
        }
    }

    /// 入队前对内联文本/图片负载执行 BLAKE3 摘要校验；若存在相同 `event_id` 则去重更新.
    pub fn push_verified(&self, envelope: CloudEventEnvelope) -> Result<(), DigestError> {
        if matches!(
            envelope.payload.content_category,
            ContentCategory::Text | ContentCategory::Image
        ) {
            if let Some(ref inline_text) = envelope.payload.inline_data {
                envelope.payload.digest.verify_bytes(inline_text.as_bytes())?;
            }
        }

        let mut guard = self.inner.write().expect("event queue lock poisoned");
        guard.retain(|e| e.event_id != envelope.event_id);
        if guard.len() >= self.capacity {
            guard.pop_back();
        }
        guard.push_front(envelope);
        self.persist_snapshot(&guard);
        Ok(())
    }

    pub fn clear(&self) {
        let mut guard = self.inner.write().expect("event queue lock poisoned");
        guard.clear();
        self.persist_snapshot(&guard);
    }

    pub fn latest(&self) -> Option<CloudEventEnvelope> {
        let guard = self.inner.read().expect("event queue lock poisoned");
        guard.front().cloned()
    }

    pub fn get_by_id(&self, event_id: &str) -> Option<CloudEventEnvelope> {
        let guard = self.inner.read().expect("event queue lock poisoned");
        guard.iter().find(|e| e.event_id == event_id).cloned()
    }

    pub fn list(&self, limit: usize) -> Vec<CloudEventEnvelope> {
        let guard = self.inner.read().expect("event queue lock poisoned");
        guard.iter().take(limit).cloned().collect()
    }

    pub fn len(&self) -> usize {
        let guard = self.inner.read().expect("event queue lock poisoned");
        guard.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_protocol::digest::DigestManifest;
    use mi_protocol::envelope::{
        EventPayload, EventType, SourceDevice, TargetMode, TargetScope, UploadState,
        SPEC_VERSION_V6, TOPIC_CLIPBOARD_BROADCAST,
    };

    fn make_text_event(event_id: &str, text: &str) -> CloudEventEnvelope {
        CloudEventEnvelope {
            spec_version: SPEC_VERSION_V6.to_string(),
            event_id: event_id.to_string(),
            topic: TOPIC_CLIPBOARD_BROADCAST.to_string(),
            event_type: EventType::ClipboardBroadcast,
            timestamp_ms: 1762310000000,
            source_device: SourceDevice {
                device_id: "node_a".to_string(),
                device_name: "Node-A".to_string(),
                os: "windows".to_string(),
                lan_endpoint: Some("192.168.1.10:8443".to_string()),
            },
            target_scope: TargetScope {
                mode: TargetMode::Broadcast,
                target_device_ids: vec!["*".to_string()],
            },
            payload: EventPayload {
                content_category: ContentCategory::Text,
                digest: DigestManifest::from_bytes(text.as_bytes()),
                inline_data: Some(text.to_string()),
                file_cache_id: None,
                upload_state: UploadState::Inline,
                items_meta: vec![],
                destination_override: None,
            },
        }
    }

    #[test]
    fn test_ring_buffer_capacity_and_digest_verification() {
        let queue = EventCacheList::new(3);
        queue
            .push_verified(make_text_event("evt_1", "人工智能-1"))
            .unwrap();
        queue
            .push_verified(make_text_event("evt_2", "人工智能-2"))
            .unwrap();
        queue
            .push_verified(make_text_event("evt_3", "人工智能-3"))
            .unwrap();
        queue
            .push_verified(make_text_event("evt_4", "人工智能-4"))
            .unwrap();

        assert_eq!(queue.len(), 3);
        assert_eq!(queue.latest().unwrap().event_id, "evt_4");
        assert!(queue.get_by_id("evt_1").is_none());
    }
}

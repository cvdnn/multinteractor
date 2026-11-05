//! 断点续传状态元数据 (`.multinteractor.resume.json`) 数据结构与偏移量管理.

use serde::{Deserialize, Serialize};
use crate::digest::DigestManifest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeState {
    pub transfer_id: String,
    pub file_cache_id: String,
    pub file_name: String,
    pub total_size: u64,
    pub verified_offset: u64,
    pub source_mtime: u64,
    pub digest: DigestManifest,
    pub active_channel: String,
    pub resume_count: u32,
}

impl ResumeState {
    pub fn new(
        transfer_id: String,
        file_cache_id: String,
        file_name: String,
        total_size: u64,
        source_mtime: u64,
        digest: DigestManifest,
        active_channel: String,
    ) -> Self {
        Self {
            transfer_id,
            file_cache_id,
            file_name,
            total_size,
            verified_offset: 0,
            source_mtime,
            digest,
            active_channel,
            resume_count: 0,
        }
    }

    pub fn is_source_unchanged(&self, current_mtime: u64, current_size: u64) -> bool {
        self.source_mtime == current_mtime && self.total_size == current_size
    }

    pub fn advance_verified_offset(&mut self, chunk_bytes: u64) {
        self.verified_offset = (self.verified_offset + chunk_bytes).min(self.total_size);
    }

    pub fn switch_channel(&mut self, new_channel: &str) {
        if self.active_channel != new_channel {
            self.active_channel = new_channel.to_string();
        }
        self.resume_count += 1;
    }

    pub fn is_completed(&self) -> bool {
        self.verified_offset >= self.total_size
    }
}

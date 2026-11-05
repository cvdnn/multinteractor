//! 分块流式上传器 (`HEAD` 查询偏移 + `PATCH` 4MB 分块续传 + 源文件变更检测防 RISK-01).

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;
use mi_protocol::digest::{DigestManifest, DEFAULT_CHUNK_SIZE_BYTES};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("RISK-01 Guard: Source file was modified during transfer (expected mtime={expected_mtime}, size={expected_size}; actual mtime={actual_mtime}, size={actual_size})")]
    SourceFileModified {
        expected_mtime: u64,
        expected_size: u64,
        actual_mtime: u64,
        actual_size: u64,
    },
    #[error("Digest error: {0}")]
    Digest(#[from] mi_protocol::digest::DigestError),
}

#[derive(Debug, Clone)]
pub struct UploadSession {
    pub file_cache_id: String,
    pub source_mtime: u64,
    pub total_size: u64,
    pub digest: DigestManifest,
}

pub fn file_mtime_and_size(path: &Path) -> Result<(u64, u64), std::io::Error> {
    let meta = fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok((mtime, meta.len()))
}

impl UploadSession {
    pub fn from_file(file_cache_id: String, path: &Path) -> Result<Self, UploadError> {
        let (source_mtime, total_size) = file_mtime_and_size(path)?;
        let digest = DigestManifest::from_file(path)?;
        Ok(Self {
            file_cache_id,
            source_mtime,
            total_size,
            digest,
        })
    }

    /// 读取从 `resume_offset` 起的下一个分块，读取前强制校验源文件 `mtime` 与 `size` 是否被篡改 (RISK-01).
    pub fn read_chunk_at(
        &self,
        path: &Path,
        resume_offset: u64,
    ) -> Result<(Vec<u8>, String), UploadError> {
        let (cur_mtime, cur_size) = file_mtime_and_size(path)?;
        if cur_mtime != self.source_mtime || cur_size != self.total_size {
            return Err(UploadError::SourceFileModified {
                expected_mtime: self.source_mtime,
                expected_size: self.total_size,
                actual_mtime: cur_mtime,
                actual_size: cur_size,
            });
        }

        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(resume_offset))?;
        let remaining = (self.total_size.saturating_sub(resume_offset)) as usize;
        let to_read = remaining.min(DEFAULT_CHUNK_SIZE_BYTES);
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf)?;
        let chunk_hash = blake3::hash(&buf).to_hex().to_string();
        Ok((buf, chunk_hash))
    }
}

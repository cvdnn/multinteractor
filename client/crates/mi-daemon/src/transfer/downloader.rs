//! 断点续传下载器 (`.multinteractor.part` 写入 + `Range` 跨通道无缝续传 + 全文 BLAKE3 摘要校验后原子重命名).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use mi_protocol::digest::{DigestError, DigestManifest};
use mi_protocol::resume::ResumeState;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Digest verification failed: {0}")]
    Digest(#[from] DigestError),
}

pub struct ResumableDownloader {
    pub target_path: PathBuf,
    pub part_path: PathBuf,
    pub resume_path: PathBuf,
    pub state: ResumeState,
}

impl ResumableDownloader {
    pub fn part_and_resume_paths(target_path: &Path) -> (PathBuf, PathBuf) {
        let base = target_path.as_os_str().to_string_lossy();
        let part_path = PathBuf::from(format!("{}.multinteractor.part", base));
        let resume_path = PathBuf::from(format!("{}.multinteractor.resume.json", base));
        (part_path, resume_path)
    }

    pub fn open_or_create(
        target_path: PathBuf,
        transfer_id: String,
        file_cache_id: String,
        total_size: u64,
        source_mtime: u64,
        digest: DigestManifest,
        initial_channel: &str,
    ) -> Result<Self, DownloadError> {
        let (part_path, resume_path) = Self::part_and_resume_paths(&target_path);
        let state = if resume_path.exists() && part_path.exists() {
            let raw = fs::read_to_string(&resume_path)?;
            let mut loaded: ResumeState = serde_json::from_str(&raw)?;
            let actual_part_len = fs::metadata(&part_path)?.len();
            if actual_part_len < loaded.verified_offset {
                loaded.verified_offset = actual_part_len;
            }
            loaded
        } else {
            let file_name = target_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            fs::write(&part_path, b"")?;
            let st = ResumeState::new(
                transfer_id,
                file_cache_id,
                file_name,
                total_size,
                source_mtime,
                digest,
                initial_channel.to_string(),
            );
            fs::write(&resume_path, serde_json::to_string_pretty(&st)?)?;
            st
        };

        Ok(Self {
            target_path,
            part_path,
            resume_path,
            state,
        })
    }

    pub fn range_header_for_remaining(&self) -> String {
        format!("bytes={}-", self.state.verified_offset)
    }

    /// 校验分块 BLAKE3 摘要通过后追加写入 `.multinteractor.part` 并更新 `.resume.json`；
    /// 若校验失败则立即拒绝写入 (丢弃损坏分块以触发单独重传，满足 TC-RES-05).
    pub fn append_verified_chunk(
        &mut self,
        chunk_index: usize,
        chunk_data: &[u8],
    ) -> Result<u64, DownloadError> {
        self.state.digest.verify_chunk(chunk_index, chunk_data)?;
        let mut file = OpenOptions::new().append(true).open(&self.part_path)?;
        file.write_all(chunk_data)?;
        file.flush()?;
        self.state.advance_verified_offset(chunk_data.len() as u64);
        self.persist_resume_state()?;
        Ok(self.state.verified_offset)
    }

    /// 内网通道中断时无缝切换至云端缓存通道 (`CLOUD_CACHE`) 继续从 `verified_offset` 断点续传 (满足 TC-RES-04).
    pub fn failover_to_channel(&mut self, new_channel: &str) -> Result<String, DownloadError> {
        self.state.switch_channel(new_channel);
        self.persist_resume_state()?;
        Ok(self.range_header_for_remaining())
    }

    /// 下载达到 `total_size` 后对 `.part` 文件执行全文 BLAKE3 摘要校验，100% 一致才原子重命名为正式目标文件.
    pub fn finalize_and_verify(&self) -> Result<PathBuf, DownloadError> {
        let part_bytes = fs::read(&self.part_path)?;
        self.state.digest.verify_bytes(&part_bytes)?;
        fs::rename(&self.part_path, &self.target_path)?;
        if self.resume_path.exists() {
            fs::remove_file(&self.resume_path)?;
        }
        Ok(self.target_path.clone())
    }

    /// 取消传输时立即清理 `.multinteractor.part` 与 `.resume.json` 残留文件 (满足 TC-NFR-05).
    pub fn cancel_and_cleanup(self) -> Result<(), DownloadError> {
        if self.part_path.exists() {
            fs::remove_file(&self.part_path)?;
        }
        if self.resume_path.exists() {
            fs::remove_file(&self.resume_path)?;
        }
        Ok(())
    }

    fn persist_resume_state(&self) -> Result<(), DownloadError> {
        let tmp = self.resume_path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.state)?)?;
        fs::rename(&tmp, &self.resume_path)?;
        Ok(())
    }
}

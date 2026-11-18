//! 可靠文件传输与全链路断点续传调度器 (内网直连优先，中断自动携带 Range 切换云端边传边下缓存续传) (TASK-06).

pub mod archive;
pub mod downloader;
pub mod uploader;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use mi_protocol::digest::DigestManifest;
use mi_protocol::envelope::ContentCategory;
use mi_protocol::ipc::IpcTransferStatusResponse;

pub use archive::{
    pack_directory_to_archive, unpack_archive_to_directory, validate_safe_archive_entry,
    ArchiveError, ArchiveSecurityError,
};
pub use downloader::{DownloadError, ResumableDownloader};
pub use uploader::{UploadError, UploadSession};

#[derive(Debug, Clone)]
pub struct StagedFileRecord {
    pub file_cache_id: String,
    pub local_path: PathBuf,
    pub original_name: String,
    pub content_category: ContentCategory,
    pub size_bytes: u64,
    pub source_mtime: u64,
    pub digest: DigestManifest,
}

#[derive(Debug, Clone)]
pub struct ResumableTaskContext {
    pub task_id: String,
    pub event_id: String,
    pub file_cache_id: String,
    pub target_path: PathBuf,
    pub dest_dir: PathBuf,
    pub content_category: ContentCategory,
    pub total_size: u64,
    pub source_mtime: u64,
    pub digest: DigestManifest,
    pub peer_device_id: String,
    pub peer_lan_endpoint: Option<String>,
}

/// 管理本地待发送文件/目录归档缓存 (`file_cache_id -> StagedFileRecord`) 与传输任务状态表 (`task_id -> IpcTransferStatusResponse`).
#[derive(Debug, Clone, Default)]
pub struct TransferManager {
    staged_files: Arc<RwLock<HashMap<String, StagedFileRecord>>>,
    tasks: Arc<RwLock<HashMap<String, IpcTransferStatusResponse>>>,
    resumable_contexts: Arc<RwLock<HashMap<String, ResumableTaskContext>>>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stage_file(&self, record: StagedFileRecord) {
        let mut guard = self.staged_files.write().expect("staged_files lock poisoned");
        guard.insert(record.file_cache_id.clone(), record);
    }

    pub fn get_staged_file(&self, file_cache_id: &str) -> Option<StagedFileRecord> {
        let guard = self.staged_files.read().expect("staged_files lock poisoned");
        guard.get(file_cache_id).cloned()
    }

    pub fn upsert_task(&self, status: IpcTransferStatusResponse) {
        let mut guard = self.tasks.write().expect("tasks lock poisoned");
        guard.insert(status.task_id.clone(), status);
    }

    pub fn register_resumable_context(&self, ctx: ResumableTaskContext) {
        let mut guard = self
            .resumable_contexts
            .write()
            .expect("resumable_contexts lock poisoned");
        guard.insert(ctx.task_id.clone(), ctx);
    }

    pub fn get_resumable_context(&self, task_id: &str) -> Option<ResumableTaskContext> {
        let guard = self
            .resumable_contexts
            .read()
            .expect("resumable_contexts lock poisoned");
        guard.get(task_id).cloned()
    }

    pub fn get_task_status(&self, task_id: &str) -> Option<IpcTransferStatusResponse> {
        let guard = self.tasks.read().expect("tasks lock poisoned");
        if task_id.is_empty() || task_id == "latest" {
            return guard.values().last().cloned();
        }
        guard.get(task_id).cloned()
    }

    pub fn list_tasks(&self) -> Vec<IpcTransferStatusResponse> {
        let guard = self.tasks.read().expect("tasks lock poisoned");
        let mut v: Vec<_> = guard.values().cloned().collect();
        v.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        v
    }

    /// 读取已暂存文件的指定偏移区间 `[start, end)`，并在读取前通过 `UploadSession` 校验源文件未被修改 (RISK-01).
    pub fn read_staged_range(&self, file_cache_id: &str, start_offset: u64) -> Result<(Vec<u8>, u64), UploadError> {
        let staged = self.get_staged_file(file_cache_id).ok_or_else(|| {
            UploadError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Staged file not found for file_cache_id={}", file_cache_id),
            ))
        })?;
        let (cur_mtime, cur_size) = uploader::file_mtime_and_size(&staged.local_path)?;
        if cur_mtime != staged.source_mtime || cur_size != staged.size_bytes {
            return Err(UploadError::SourceFileModified {
                expected_mtime: staged.source_mtime,
                expected_size: staged.size_bytes,
                actual_mtime: cur_mtime,
                actual_size: cur_size,
            });
        }
        let all_bytes = std::fs::read(&staged.local_path)?;
        let total = all_bytes.len() as u64;
        if start_offset >= total {
            return Ok((Vec::new(), total));
        }
        Ok((all_bytes[start_offset as usize..].to_vec(), total))
    }

    pub fn stage_local_path(
        &self,
        file_cache_id: String,
        source_path: &Path,
        staging_dir: &Path,
    ) -> anyhow::Result<StagedFileRecord> {
        let sym_meta = std::fs::symlink_metadata(source_path)?;
        if sym_meta.file_type().is_symlink() {
            anyhow::bail!("Symlinks are rejected for security: {}", source_path.display());
        }
        let original_name = source_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());

        if sym_meta.is_dir() {
            std::fs::create_dir_all(staging_dir)?;
            let archive_path = staging_dir.join(format!("{}.tar.zst", file_cache_id));
            pack_directory_to_archive(source_path, &archive_path)?;
            let (source_mtime, size_bytes) = uploader::file_mtime_and_size(&archive_path)?;
            let digest = DigestManifest::from_file(&archive_path)?;
            let rec = StagedFileRecord {
                file_cache_id,
                local_path: archive_path,
                original_name,
                content_category: ContentCategory::Directory,
                size_bytes,
                source_mtime,
                digest,
            };
            self.stage_file(rec.clone());
            Ok(rec)
        } else {
            let (source_mtime, size_bytes) = uploader::file_mtime_and_size(source_path)?;
            let digest = DigestManifest::from_file(source_path)?;
            let ext = source_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let content_category = if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") {
                ContentCategory::Image
            } else {
                ContentCategory::File
            };
            let rec = StagedFileRecord {
                file_cache_id,
                local_path: source_path.to_path_buf(),
                original_name,
                content_category,
                size_bytes,
                source_mtime,
                digest,
            };
            self.stage_file(rec.clone());
            Ok(rec)
        }
    }
}


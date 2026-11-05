//! 可靠文件传输与全链路断点续传调度器 (内网直连优先，中断自动携带 Range 切换云端边传边下缓存续传) (TASK-06).

pub mod archive;
pub mod downloader;
pub mod uploader;

pub use archive::{validate_safe_archive_entry, ArchiveSecurityError};
pub use downloader::{DownloadError, ResumableDownloader};
pub use uploader::{UploadError, UploadSession};

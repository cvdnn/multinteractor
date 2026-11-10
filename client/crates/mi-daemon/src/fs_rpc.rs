//! 远程文件系统 RPC 服务端 (`ROOT` 盘符探测、500ms 单层目录扫描超时保护防 RISK-03、远程新建文件夹 `MKDIR`).

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};
use mi_protocol::rpc::{
    FsEntry, FsListDirRequest, FsListDirResponse, FsMkdirRequest, FsMkdirResponse,
    ROOT_PATH_SENTINEL,
};

pub const DEFAULT_DIR_SCAN_TIMEOUT: Duration = Duration::from_millis(500);
pub const MAX_PATH_LENGTH: usize = 1024;

pub fn is_forbidden_system_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let lower = path_str.to_lowercase();

    // Windows 敏感系统路径
    if lower.starts_with("c:\\windows") || lower.starts_with("c:/windows") {
        return true;
    }

    // Unix 敏感系统目录
    for forbidden in ["/etc", "/proc", "/sys", "/dev", "/boot", "/root"] {
        if path_str == forbidden || path_str.starts_with(&format!("{}/", forbidden)) {
            return true;
        }
    }
    false
}

pub fn validate_rpc_path(path_str: &str) -> anyhow::Result<PathBuf> {
    if path_str.is_empty() {
        anyhow::bail!("Path cannot be empty");
    }
    if path_str.len() > MAX_PATH_LENGTH {
        anyhow::bail!("Path exceeds maximum length of {} bytes", MAX_PATH_LENGTH);
    }
    if path_str.contains('\0') {
        anyhow::bail!("Path contains null bytes");
    }
    let p = Path::new(path_str);
    for comp in p.components() {
        if let Component::ParentDir = comp {
            anyhow::bail!("Parent directory '..' traversal rejected");
        }
    }
    if is_forbidden_system_path(p) {
        anyhow::bail!("Access to sensitive system path is forbidden: {}", path_str);
    }
    Ok(p.to_path_buf())
}

pub struct FsRpcHandler {
    pub device_id: String,
    pub scan_timeout: Duration,
}

impl FsRpcHandler {
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            scan_timeout: DEFAULT_DIR_SCAN_TIMEOUT,
        }
    }

    pub fn handle_list_dir(&self, req: &FsListDirRequest) -> anyhow::Result<FsListDirResponse> {
        let t0 = Instant::now();
        let mut entries = Vec::new();
        let mut skipped_offline = Vec::new();

        if req.path == ROOT_PATH_SENTINEL {
            for candidate in Self::root_and_shortcut_paths() {
                if t0.elapsed() > self.scan_timeout {
                    skipped_offline.push(candidate.to_string_lossy().to_string());
                    continue;
                }
                if candidate.exists() {
                    let name = candidate
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| candidate.to_string_lossy().to_string());
                    entries.push(FsEntry {
                        name,
                        path: candidate.to_string_lossy().to_string(),
                        is_dir: true,
                        size_bytes: 0,
                        mtime: 0,
                        has_subdirs: true,
                    });
                }
            }
        } else {
            let dir_path = validate_rpc_path(&req.path)?;
            for item in fs::read_dir(&dir_path)? {
                if t0.elapsed() > self.scan_timeout {
                    skipped_offline.push(req.path.clone());
                    break;
                }
                let entry = item?;
                let meta = entry.metadata()?;
                if !req.include_files && !meta.is_dir() {
                    continue;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                entries.push(FsEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: entry.path().to_string_lossy().to_string(),
                    is_dir: meta.is_dir(),
                    size_bytes: meta.len(),
                    mtime,
                    has_subdirs: meta.is_dir(),
                });
            }
            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        }

        Ok(FsListDirResponse {
            request_id: req.request_id.clone(),
            device_id: self.device_id.clone(),
            current_path: req.path.clone(),
            entries,
            free_space_bytes: 64 * 1024 * 1024 * 1024,
            scan_duration_ms: t0.elapsed().as_millis() as u64,
            skipped_offline_drives: skipped_offline,
        })
    }

    pub fn handle_mkdir(&self, req: &FsMkdirRequest) -> FsMkdirResponse {
        let valid_path = match validate_rpc_path(&req.path) {
            Ok(p) => p,
            Err(err) => {
                return FsMkdirResponse {
                    request_id: req.request_id.clone(),
                    created_path: req.path.clone(),
                    status: "ERROR".to_string(),
                    error: Some(format!("SECURITY_REJECTED: {}", err)),
                };
            }
        };

        match fs::create_dir_all(&valid_path) {
            Ok(()) => FsMkdirResponse {
                request_id: req.request_id.clone(),
                created_path: req.path.clone(),
                status: "CREATED".to_string(),
                error: None,
            },
            Err(err) => FsMkdirResponse {
                request_id: req.request_id.clone(),
                created_path: req.path.clone(),
                status: "ERROR".to_string(),
                error: Some(err.to_string()),
            },
        }
    }

    fn root_and_shortcut_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let home_path = PathBuf::from(home);
            for sub in ["Desktop", "Downloads", "Documents"] {
                let p = home_path.join(sub);
                if p.exists() {
                    paths.push(p);
                }
            }
            paths.push(home_path);
        }
        #[cfg(target_os = "windows")]
        {
            for letter in b'C'..=b'Z' {
                let drive = PathBuf::from(format!("{}:\\", letter as char));
                if drive.exists() {
                    paths.push(drive);
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let volumes = PathBuf::from("/Volumes");
            if let Ok(read_dir) = fs::read_dir(&volumes) {
                for entry in read_dir.flatten() {
                    paths.push(entry.path());
                }
            }
            paths.push(PathBuf::from("/"));
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            paths.push(PathBuf::from("/"));
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_path_security_validation() {
        assert!(validate_rpc_path("/tmp").is_ok());
        assert!(validate_rpc_path("/etc").is_err());
        assert!(validate_rpc_path("/etc/passwd").is_err());
        assert!(validate_rpc_path("/proc/cpuinfo").is_err());
        assert!(validate_rpc_path("C:\\Windows\\System32").is_err());
        assert!(validate_rpc_path("foo/../bar").is_err());
        assert!(validate_rpc_path("foo\0bar").is_err());
        let long_path = "a/".repeat(600);
        assert!(validate_rpc_path(&long_path).is_err());
    }

    #[test]
    fn test_mkdir_security_rejection() {
        let handler = FsRpcHandler::new("test_dev".to_string());
        let res = handler.handle_mkdir(&FsMkdirRequest {
            request_id: "req1".to_string(),
            target_device_id: "test_dev".to_string(),
            path: "/etc/malicious_create".to_string(),
        });
        assert_eq!(res.status, "ERROR");
        assert!(res.error.unwrap().contains("SECURITY_REJECTED"));
    }
}

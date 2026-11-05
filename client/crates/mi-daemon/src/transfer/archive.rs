//! 多层级文件夹流式 `.tar.zst` 边读边打包与边下边解包 (内置防 `..` 路径穿越安全拦截).

use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArchiveSecurityError {
    #[error("SECURITY ALERT (TC-NFR-03): Path traversal or absolute path rejected in archive entry: {0}")]
    PathTraversalDetected(String),
}

/// 严格校验归档条目相对路径，拦截任何包含 `..`、根路径 `/` `\` 或盘符前缀 (`C:`) 的恶意条目 (TC-NFR-03).
pub fn validate_safe_archive_entry(dest_root: &Path, entry_rel_path: &str) -> Result<PathBuf, ArchiveSecurityError> {
    if entry_rel_path.is_empty()
        || entry_rel_path.starts_with('/')
        || entry_rel_path.starts_with('\\')
        || entry_rel_path.contains(':')
    {
        return Err(ArchiveSecurityError::PathTraversalDetected(
            entry_rel_path.to_string(),
        ));
    }

    let normalized = entry_rel_path.replace('\\', "/");
    for segment in normalized.split('/') {
        if segment == ".." {
            return Err(ArchiveSecurityError::PathTraversalDetected(
                entry_rel_path.to_string(),
            ));
        }
    }

    let rel_path = Path::new(&normalized);
    for comp in rel_path.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveSecurityError::PathTraversalDetected(
                    entry_rel_path.to_string(),
                ));
            }
        }
    }

    Ok(dest_root.join(rel_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_path_traversal_entries() {
        let root = Path::new("/tmp/safe_extract");
        assert!(validate_safe_archive_entry(root, "课程资料/chapter1/notes.md").is_ok());
        assert!(validate_safe_archive_entry(root, "../../etc/passwd").is_err());
        assert!(validate_safe_archive_entry(root, "..\\Windows\\System32\\evil.dll").is_err());
        assert!(validate_safe_archive_entry(root, "/etc/shadow").is_err());
        assert!(validate_safe_archive_entry(root, "C:\\Windows\\evil.exe").is_err());
    }
}

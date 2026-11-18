//! 多层级文件夹流式 `.tar.zst` 边读边打包与边下边解包 (内置防 `..` 路径穿越安全拦截).

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use mi_protocol::envelope::MAX_TRANSFER_TOTAL_SIZE;
use thiserror::Error;

pub const MAX_PATH_LENGTH: usize = 1024;
/// Zstandard skippable frame magic (`0x184D2A50`) + `"MITZST01"` header for zero-C-dependency streaming `.tar.zst` framing.
pub const MI_ZSTD_ARCHIVE_MAGIC: &[u8; 16] = b"\x50\x2A\x4D\x18\x08\x00\x00\x00MITZST01";
const ENTRY_KIND_DIR: u8 = 1;
const ENTRY_KIND_FILE: u8 = 2;
const ENTRY_KIND_EOF: u8 = 0;
const STREAM_IO_BUF_SIZE: usize = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArchiveSecurityError {
    #[error("SECURITY ALERT (TC-NFR-03): Path traversal or absolute path rejected in archive entry: {0}")]
    PathTraversalDetected(String),
    #[error("SECURITY ALERT: Path exceeds maximum length of {MAX_PATH_LENGTH} bytes: {0}")]
    PathTooLong(String),
    #[error("SECURITY ALERT: Null byte detected in entry path")]
    NullByteDetected,
}

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Security error: {0}")]
    Security(#[from] ArchiveSecurityError),
    #[error("SECURITY ALERT: Symlink entry rejected during directory archive operation: {0}")]
    SymlinkRejected(String),
    #[error("SECURITY ALERT: Archive extraction exceeded quota limit ({actual} > {limit} bytes)")]
    QuotaExceeded { actual: u64, limit: u64 },
    #[error("Corrupted or unsupported archive header")]
    CorruptedArchive,
}

/// 严格校验归档条目相对路径，拦截任何包含 `..`、根路径 `/` `\`、盘符前缀 (`C:`)、超长路径或空字符的恶意条目 (TC-NFR-03).
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

    if entry_rel_path.len() > MAX_PATH_LENGTH {
        return Err(ArchiveSecurityError::PathTooLong(entry_rel_path.to_string()));
    }

    if entry_rel_path.contains('\0') {
        return Err(ArchiveSecurityError::NullByteDetected);
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

    let joined = dest_root.join(rel_path);
    if !joined.starts_with(dest_root) {
        return Err(ArchiveSecurityError::PathTraversalDetected(
            entry_rel_path.to_string(),
        ));
    }

    Ok(joined)
}

/// 流式递归打包目录至 `.tar.zst` 归档文件：
/// - 保留顶层目录名、空文件夹、多级子目录与文件；
/// - 拒绝符号链接 (`symlink_metadata().file_type().is_symlink()`) 防止越权软链泄露；
/// - 流式 64KB 缓冲读写，无需将整个目录载入内存.
pub fn pack_directory_to_archive(src_dir: &Path, archive_path: &Path) -> Result<u64, ArchiveError> {
    let meta = fs::symlink_metadata(src_dir)?;
    if meta.file_type().is_symlink() {
        return Err(ArchiveError::SymlinkRejected(
            src_dir.to_string_lossy().to_string(),
        ));
    }
    if !meta.is_dir() {
        return Err(ArchiveError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Source path is not a directory: {}", src_dir.display()),
        )));
    }

    let top_name = src_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "folder".to_string());

    let out_file = File::create(archive_path)?;
    let mut writer = BufWriter::new(out_file);
    writer.write_all(MI_ZSTD_ARCHIVE_MAGIC)?;

    let mut total_payload_bytes = 0u64;
    write_dir_recursive(src_dir, &top_name, &mut writer, &mut total_payload_bytes)?;
    writer.write_all(&[ENTRY_KIND_EOF])?;
    writer.flush()?;
    Ok(fs::metadata(archive_path)?.len())
}

fn write_dir_recursive<W: Write>(
    current_dir: &Path,
    rel_prefix: &str,
    writer: &mut W,
    total_payload_bytes: &mut u64,
) -> Result<(), ArchiveError> {
    let dir_meta = fs::symlink_metadata(current_dir)?;
    if dir_meta.file_type().is_symlink() {
        return Err(ArchiveError::SymlinkRejected(
            current_dir.to_string_lossy().to_string(),
        ));
    }

    let mtime = dir_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_entry_header(writer, ENTRY_KIND_DIR, rel_prefix, mtime, 0)?;

    let mut entries: Vec<_> = fs::read_dir(current_dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let sym_meta = fs::symlink_metadata(&path)?;
        if sym_meta.file_type().is_symlink() {
            return Err(ArchiveError::SymlinkRejected(
                path.to_string_lossy().to_string(),
            ));
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let child_rel = format!("{}/{}", rel_prefix.trim_end_matches('/'), name);
        validate_safe_archive_entry(Path::new("/safe_root"), &child_rel)?;

        if sym_meta.is_dir() {
            write_dir_recursive(&path, &child_rel, writer, total_payload_bytes)?;
        } else if sym_meta.is_file() {
            let file_len = sym_meta.len();
            *total_payload_bytes = total_payload_bytes.saturating_add(file_len);
            if *total_payload_bytes > MAX_TRANSFER_TOTAL_SIZE {
                return Err(ArchiveError::QuotaExceeded {
                    actual: *total_payload_bytes,
                    limit: MAX_TRANSFER_TOTAL_SIZE,
                });
            }
            let file_mtime = sym_meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            write_entry_header(writer, ENTRY_KIND_FILE, &child_rel, file_mtime, file_len)?;

            let mut f = File::open(&path)?;
            let mut buf = vec![0u8; STREAM_IO_BUF_SIZE];
            let mut remaining = file_len;
            while remaining > 0 {
                let to_read = (remaining as usize).min(buf.len());
                f.read_exact(&mut buf[..to_read])?;
                writer.write_all(&buf[..to_read])?;
                remaining -= to_read as u64;
            }
        }
    }

    Ok(())
}

fn write_entry_header<W: Write>(
    writer: &mut W,
    kind: u8,
    rel_path: &str,
    mtime_secs: u64,
    size_bytes: u64,
) -> Result<(), ArchiveError> {
    let path_bytes = rel_path.as_bytes();
    if path_bytes.len() > MAX_PATH_LENGTH {
        return Err(ArchiveError::Security(ArchiveSecurityError::PathTooLong(
            rel_path.to_string(),
        )));
    }
    writer.write_all(&[kind])?;
    writer.write_all(&(path_bytes.len() as u32).to_le_bytes())?;
    writer.write_all(path_bytes)?;
    writer.write_all(&mtime_secs.to_le_bytes())?;
    writer.write_all(&size_bytes.to_le_bytes())?;
    Ok(())
}

/// 流式解包 `.tar.zst` 归档文件至 `dest_root`：
/// - 对每一个归档条目强制执行 `validate_safe_archive_entry(dest_root, rel_path)` 防路径穿越；
/// - 累计写出字节数超过 `max_total_bytes` 立即中断并返回 `ArchiveError::QuotaExceeded` 防归档炸弹.
pub fn unpack_archive_to_directory(
    archive_path: &Path,
    dest_root: &Path,
    max_total_bytes: u64,
) -> Result<u64, ArchiveError> {
    fs::create_dir_all(dest_root)?;
    let in_file = File::open(archive_path)?;
    let mut reader = BufReader::new(in_file);

    let mut magic = [0u8; 16];
    reader
        .read_exact(&mut magic)
        .map_err(|_| ArchiveError::CorruptedArchive)?;
    if &magic != MI_ZSTD_ARCHIVE_MAGIC {
        return Err(ArchiveError::CorruptedArchive);
    }

    let mut extracted_bytes = 0u64;
    let mut buf = vec![0u8; STREAM_IO_BUF_SIZE];

    loop {
        let mut kind_buf = [0u8; 1];
        reader
            .read_exact(&mut kind_buf)
            .map_err(|_| ArchiveError::CorruptedArchive)?;
        let kind = kind_buf[0];
        if kind == ENTRY_KIND_EOF {
            break;
        }

        let mut path_len_buf = [0u8; 4];
        reader
            .read_exact(&mut path_len_buf)
            .map_err(|_| ArchiveError::CorruptedArchive)?;
        let path_len = u32::from_le_bytes(path_len_buf) as usize;
        if path_len == 0 || path_len > MAX_PATH_LENGTH {
            return Err(ArchiveError::Security(ArchiveSecurityError::PathTooLong(
                format!("len={}", path_len),
            )));
        }

        let mut path_bytes = vec![0u8; path_len];
        reader
            .read_exact(&mut path_bytes)
            .map_err(|_| ArchiveError::CorruptedArchive)?;
        let rel_path =
            String::from_utf8(path_bytes).map_err(|_| ArchiveError::CorruptedArchive)?;
        let safe_target = validate_safe_archive_entry(dest_root, &rel_path)?;

        let mut mtime_buf = [0u8; 8];
        let mut size_buf = [0u8; 8];
        reader
            .read_exact(&mut mtime_buf)
            .map_err(|_| ArchiveError::CorruptedArchive)?;
        reader
            .read_exact(&mut size_buf)
            .map_err(|_| ArchiveError::CorruptedArchive)?;
        let size_bytes = u64::from_le_bytes(size_buf);

        match kind {
            ENTRY_KIND_DIR => {
                fs::create_dir_all(&safe_target)?;
            }
            ENTRY_KIND_FILE => {
                extracted_bytes = extracted_bytes.saturating_add(size_bytes);
                if extracted_bytes > max_total_bytes {
                    return Err(ArchiveError::QuotaExceeded {
                        actual: extracted_bytes,
                        limit: max_total_bytes,
                    });
                }
                if let Some(parent) = safe_target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out = File::create(&safe_target)?;
                let mut remaining = size_bytes;
                while remaining > 0 {
                    let to_read = (remaining as usize).min(buf.len());
                    reader
                        .read_exact(&mut buf[..to_read])
                        .map_err(|_| ArchiveError::CorruptedArchive)?;
                    out.write_all(&buf[..to_read])?;
                    remaining -= to_read as u64;
                }
                out.flush()?;
            }
            _ => return Err(ArchiveError::CorruptedArchive),
        }
    }

    Ok(extracted_bytes)
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
        assert!(validate_safe_archive_entry(root, "foo\0bar").is_err());
        let long_path = "a/".repeat(600);
        assert!(validate_safe_archive_entry(root, &long_path).is_err());
    }

    #[test]
    fn test_pack_and_unpack_nested_directory_with_empty_folders_and_quota() {
        let base = std::env::temp_dir().join(format!("mi_archive_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src_dir = base.join("课程资料");
        fs::create_dir_all(src_dir.join("chapter1/empty_sub")).unwrap();
        fs::create_dir_all(src_dir.join("chapter2")).unwrap();
        fs::write(src_dir.join("README.md"), b"# Course Intro\n").unwrap();
        fs::write(src_dir.join("chapter1/notes.txt"), b"Lecture 1 content").unwrap();

        let archive_path = base.join("course.tar.zst");
        let archive_size = pack_directory_to_archive(&src_dir, &archive_path).unwrap();
        assert!(archive_size > 16);

        let extract_dir = base.join("extracted");
        let unpacked_bytes =
            unpack_archive_to_directory(&archive_path, &extract_dir, 1024 * 1024).unwrap();
        assert_eq!(unpacked_bytes, (b"# Course Intro\n".len() + b"Lecture 1 content".len()) as u64);
        assert!(extract_dir.join("课程资料/chapter1/empty_sub").is_dir());
        assert_eq!(
            fs::read_to_string(extract_dir.join("课程资料/README.md")).unwrap(),
            "# Course Intro\n"
        );
        assert_eq!(
            fs::read_to_string(extract_dir.join("课程资料/chapter1/notes.txt")).unwrap(),
            "Lecture 1 content"
        );

        // Quota exceeded test
        let small_quota_dir = base.join("small_quota");
        let err = unpack_archive_to_directory(&archive_path, &small_quota_dir, 5).unwrap_err();
        assert!(matches!(err, ArchiveError::QuotaExceeded { .. }));

        let _ = fs::remove_dir_all(&base);
    }
}


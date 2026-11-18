//! `mi copy --text/--path --json`：计算 BLAKE3 摘要并广播文本或文件/文件夹 (等同 `Alt+C`).

use std::path::Path;
use mi_protocol::digest::DigestManifest;
use mi_protocol::ipc::IpcCopyResponse;

pub fn execute_local_copy(
    event_id: String,
    text: Option<&str>,
    path: Option<&str>,
) -> anyhow::Result<IpcCopyResponse> {
    if let Some(t) = text {
        let digest = DigestManifest::from_bytes(t.as_bytes());
        return Ok(IpcCopyResponse {
            event_id,
            content_category: "TEXT".to_string(),
            content_hash: digest.full_hash,
            file_cache_id: None,
            size_bytes: t.len() as u64,
        });
    }
    if let Some(p) = path {
        let target = Path::new(p);
        if target.is_dir() {
            let mut acc_bytes = Vec::new();
            let mut total_size = 0u64;
            collect_dir_digest_material(target, &mut acc_bytes, &mut total_size)?;
            let digest = DigestManifest::from_bytes(&acc_bytes);
            return Ok(IpcCopyResponse {
                event_id: event_id.clone(),
                content_category: "DIRECTORY".to_string(),
                content_hash: digest.full_hash,
                file_cache_id: Some(format!("fc_{}", event_id)),
                size_bytes: total_size,
            });
        }
        let digest = DigestManifest::from_file(target)?;
        let size = std::fs::metadata(target)?.len();
        return Ok(IpcCopyResponse {
            event_id: event_id.clone(),
            content_category: "FILE".to_string(),
            content_hash: digest.full_hash,
            file_cache_id: Some(format!("fc_{}", event_id)),
            size_bytes: size,
        });
    }
    anyhow::bail!("Either --text or --path is required")
}

fn collect_dir_digest_material(
    dir: &Path,
    acc: &mut Vec<u8>,
    total_size: &mut u64,
) -> anyhow::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let p = entry.path();
        let meta = std::fs::symlink_metadata(&p)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        acc.extend_from_slice(entry.file_name().to_string_lossy().as_bytes());
        if meta.is_dir() {
            collect_dir_digest_material(&p, acc, total_size)?;
        } else if meta.is_file() {
            let bytes = std::fs::read(&p)?;
            *total_size += bytes.len() as u64;
            acc.extend_from_slice(&bytes);
        }
    }
    Ok(())
}



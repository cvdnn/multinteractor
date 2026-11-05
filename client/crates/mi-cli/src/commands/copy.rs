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
        let digest = DigestManifest::from_file(target)?;
        let size = std::fs::metadata(target)?.len();
        return Ok(IpcCopyResponse {
            event_id: event_id.clone(),
            content_category: if target.is_dir() { "DIRECTORY" } else { "FILE" }.to_string(),
            content_hash: digest.full_hash,
            file_cache_id: Some(format!("fc_{}", event_id)),
            size_bytes: size,
        });
    }
    anyhow::bail!("Either --text or --path is required")
}

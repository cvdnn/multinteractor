//! `mi sync --source --device --remote-dir --json`：定向同步文件/文件夹或文本至目标机器指定目录 (等同右键【同步到...】).

use mi_protocol::ipc::IpcSyncResponse;

pub fn format_sync_json(resp: &IpcSyncResponse) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(resp)?)
}

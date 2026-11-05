//! `mi transfer status/resume --task-id --json`：查询大文件传输进度与触发断点续传恢复.

use mi_protocol::ipc::IpcTransferStatusResponse;

pub fn format_transfer_status_json(resp: &IpcTransferStatusResponse) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(resp)?)
}

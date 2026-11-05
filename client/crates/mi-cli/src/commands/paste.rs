//! `mi paste [--event-id] [--dest] --json`：粘贴最新或指定事件至焦点窗口或指定目录 (等同 `Alt+V`).

use mi_protocol::ipc::IpcPasteResponse;

pub fn format_paste_response(resp: &IpcPasteResponse) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(resp)?)
}

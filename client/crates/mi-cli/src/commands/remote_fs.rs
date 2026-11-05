//! `mi remote-ls` & `mi remote-mkdir --json`：按需动态拉取目标机器目录结构与远程新建文件夹.

use mi_protocol::ipc::IpcRemoteLsResponse;
use mi_protocol::rpc::FsMkdirResponse;

pub fn format_remote_ls_json(resp: &IpcRemoteLsResponse) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(resp)?)
}

pub fn format_remote_mkdir_json(resp: &FsMkdirResponse) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(resp)?)
}

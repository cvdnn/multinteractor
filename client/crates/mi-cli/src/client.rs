//! 调用本地 `mi-daemon` (`http://127.0.0.1:53318/api/v1/*`) 的轻量 IPC 客户端.

use mi_protocol::ipc::DEFAULT_LOCAL_IPC_ADDR;

#[derive(Debug, Clone)]
pub struct DaemonIpcClient {
    pub endpoint: String,
}

impl Default for DaemonIpcClient {
    fn default() -> Self {
        Self {
            endpoint: format!("http://{}", DEFAULT_LOCAL_IPC_ADDR),
        }
    }
}

impl DaemonIpcClient {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }
}

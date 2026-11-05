//! 本地常驻后台守护服务启动入口 (`mi-daemon`).

use mi_daemon::config::DaemonConfig;
use mi_daemon::DaemonNode;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = DaemonConfig::default();
    let node = DaemonNode::new(config);
    println!(
        "multinteractor daemon started: device_id={}, ipc={}",
        node.config.device_id, node.config.local_ipc_addr
    );
    Ok(())
}

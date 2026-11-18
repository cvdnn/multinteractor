//! 本地常驻后台守护服务启动入口 (`mi-daemon`).

use std::path::PathBuf;
use clap::Parser;
use mi_daemon::config::DaemonConfig;
use mi_daemon::DaemonNode;

#[derive(Debug, Parser)]
#[command(name = "mi-daemon", version, about = "multinteractor local daemon service")]
struct DaemonCliArgs {
    /// Optional JSON config file path
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override local IPC bind address (default 127.0.0.1:53318)
    #[arg(long)]
    ipc_addr: Option<String>,

    /// Override LAN bind address (default 0.0.0.0:8443)
    #[arg(long)]
    lan_addr: Option<String>,

    /// Override device ID
    #[arg(long)]
    device_id: Option<String>,

    /// Run self-check and exit immediately without blocking
    #[arg(long, default_value_t = false)]
    once: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = DaemonCliArgs::parse();
    let mut config = DaemonConfig::from_env_and_file(args.config.as_deref());
    if let Some(ipc) = args.ipc_addr {
        config.local_ipc_addr = ipc;
    }
    if let Some(lan) = args.lan_addr {
        config.lan_bind_addr = lan;
    }
    if let Some(id) = args.device_id {
        config.device_id = id;
    }

    let node = DaemonNode::new(config);
    if args.once {
        println!(
            "multinteractor daemon self-check ok: device_id={}, ipc={}",
            node.config.device_id, node.config.local_ipc_addr
        );
        return Ok(());
    }

    let handles = node.start_background().await?;
    println!(
        "multinteractor daemon started: device_id={}, ipc={}, lan={}",
        node.config.device_id, handles.ipc_addr, handles.lan_addr
    );
    tokio::signal::ctrl_c().await?;
    handles.shutdown();
    Ok(())
}


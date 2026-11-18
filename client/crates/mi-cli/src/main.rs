//! `mi` 命令行工具入口，供 AI 智能体 (`multinteractor` Skill) 与脚本调用 (FR-SKILL-01, TASK-07).

mod client;
mod commands;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "mi", about = "multinteractor cross-device collaboration CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// 查询在线设备列表 (`mi devices list --json`)
    Devices {
        #[command(subcommand)]
        action: DevicesAction,
    },
    /// 查看本机接收事件缓存列表 (`mi events list --limit 10 --json`)
    Events {
        #[command(subcommand)]
        action: EventsAction,
    },
    /// 复制并广播文本或文件/目录 (`mi copy --text/--path --json`)
    Copy {
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// 触发粘贴或指定目录落盘 (`mi paste [--event-id] [--dest] --json`)
    Paste {
        #[arg(long)]
        event_id: Option<String>,
        #[arg(long)]
        dest: Option<String>,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// 动态拉取目标机器指定路径下的文件目录结构 (`mi remote-ls --device --path --json`)
    RemoteLs {
        #[arg(long)]
        device: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// 在目标机器上远程创建新文件夹 (`mi remote-mkdir --device --path --json`)
    RemoteMkdir {
        #[arg(long)]
        device: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// 将本地文件/文件夹或文本直接同步至目标机器指定目录 (`mi sync --source --device --remote-dir --json`)
    Sync {
        #[arg(long)]
        source: String,
        #[arg(long)]
        device: String,
        #[arg(long)]
        remote_dir: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    /// 查询大文件传输状态或触发断点续传 (`mi transfer status/resume --task-id --json`)
    Transfer {
        #[command(subcommand)]
        action: TransferAction,
    },
    /// 本地自签证书与服务端指纹固定管理 (`mi cert status/rotate/pin-server`)
    Cert {
        #[command(subcommand)]
        action: CertAction,
    },
    /// 查看本机 6 位指纹配对码与配对状态 (`mi pair`)
    Pair {
        #[arg(long, default_value_t = true)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum DevicesAction {
    List {
        #[arg(long, default_value_t = true)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum EventsAction {
    List {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TransferAction {
    Status {
        #[arg(long)]
        task_id: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    Resume {
        #[arg(long)]
        task_id: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum CertAction {
    Status {
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    Rotate {
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    PinServer {
        #[arg(long)]
        sha256: String,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let ipc = client::DaemonIpcClient::default();

    match cli.command {
        Commands::Devices { action } => match action {
            DevicesAction::List { json: _ } => {
                let devices = ipc.list_devices().unwrap_or_default();
                println!("{}", commands::devices::format_devices_json(&devices)?);
            }
        },
        Commands::Events { action } => match action {
            EventsAction::List { limit, json: _ } => {
                let resp = ipc.list_events(limit).unwrap_or(mi_protocol::ipc::IpcEventsListResponse {
                    events: Vec::new(),
                });
                println!(
                    "{}",
                    commands::events::format_events_json(&resp.events, limit)?
                );
            }
        },
        Commands::Copy { text, path, json: _ } => {
            let req = mi_protocol::ipc::IpcCopyRequest {
                text: text.clone(),
                path: path.clone(),
            };
            let resp = match ipc.copy(&req) {
                Ok(r) => r,
                Err(_) => commands::copy::execute_local_copy(
                    "evt_cli_01".to_string(),
                    text.as_deref(),
                    path.as_deref(),
                )?,
            };
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Paste {
            event_id,
            dest,
            json: _,
        } => {
            let req = mi_protocol::ipc::IpcPasteRequest {
                event_id,
                dest_dir: dest,
            };
            let resp = ipc.paste(&req)?;
            println!("{}", commands::paste::format_paste_response(&resp)?);
        }
        Commands::RemoteLs {
            device,
            path,
            json: _,
        } => {
            let req = mi_protocol::ipc::IpcRemoteLsRequest { device, path };
            let resp = ipc.remote_ls(&req)?;
            println!("{}", commands::remote_fs::format_remote_ls_json(&resp)?);
        }
        Commands::RemoteMkdir {
            device,
            path,
            json: _,
        } => {
            let req = mi_protocol::ipc::IpcRemoteMkdirRequest { device, path };
            let resp = ipc.remote_mkdir(&req)?;
            println!("{}", commands::remote_fs::format_remote_mkdir_json(&resp)?);
        }
        Commands::Sync {
            source,
            device,
            remote_dir,
            json: _,
        } => {
            let req = mi_protocol::ipc::IpcSyncRequest {
                source,
                target_device: device,
                remote_dir,
            };
            let resp = ipc.sync(&req)?;
            println!("{}", commands::sync::format_sync_json(&resp)?);
        }
        Commands::Transfer { action } => match action {
            TransferAction::Status { task_id, json: _ } => {
                let resp = ipc.transfer_status(&task_id)?;
                println!(
                    "{}",
                    commands::transfer::format_transfer_status_json(&resp)?
                );
            }
            TransferAction::Resume { task_id, json: _ } => {
                let req = mi_protocol::ipc::IpcTransferActionRequest { task_id };
                let resp = ipc.transfer_resume(&req)?;
                println!(
                    "{}",
                    commands::transfer::format_transfer_status_json(&resp)?
                );
            }
        },
        Commands::Cert { action } => match action {
            CertAction::Status { json: _ } => {
                let resp = ipc.cert_status()?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            CertAction::Rotate { json: _ } => {
                let resp = ipc.cert_rotate()?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            CertAction::PinServer { sha256, json: _ } => {
                let resp = ipc.cert_pin_server(&sha256)?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
        Commands::Pair { json: _ } => {
            let resp = ipc.pair_status()?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }
    Ok(())
}


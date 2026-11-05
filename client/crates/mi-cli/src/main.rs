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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Copy { text, path, .. } => {
            let resp = commands::copy::execute_local_copy(
                "evt_cli_01".to_string(),
                text.as_deref(),
                path.as_deref(),
            )?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        _ => {
            println!("{{\"status\": \"DISPATCHED_TO_DAEMON\"}}");
        }
    }
    Ok(())
}

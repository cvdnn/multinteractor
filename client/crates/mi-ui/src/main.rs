//! 轻量级远程目录树选择弹窗与传输进度框宿主入口 (`mi-ui`) (FR-CTX-01~03, TASK-09).

pub mod commands;
pub mod gui;

use std::path::PathBuf;
use clap::Parser;
use commands::RemoteDirectoryPickerSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const EMBEDDED_INDEX_HTML: &str = include_str!("../ui/index.html");
const EMBEDDED_PICKER_JS: &str = include_str!("../ui/directory_picker.js");
const EMBEDDED_TOAST_JS: &str = include_str!("../ui/transfer_toast.js");

#[derive(Debug, Parser)]
#[command(name = "mi-ui", version, about = "multinteractor desktop GUI, tray & remote directory picker")]
struct UiArgs {
    /// Optional JSON config file path for the embedded daemon
    #[arg(long)]
    config: Option<PathBuf>,

    /// Open remote directory picker modal (`--pick-remote-dir`)
    #[arg(long, default_value_t = false)]
    pick_remote_dir: bool,

    /// Target peer device ID or name
    #[arg(long, default_value = "node_c")]
    device: String,

    /// Local source file or directory path to sync
    #[arg(long, default_value = "./AI教程.docx")]
    source: String,

    /// Optional remote directory to immediately sync to in headless mode
    #[arg(long)]
    remote_dir: Option<String>,

    /// Serve interactive Web UI on the given local address (e.g. 127.0.0.1:53319)
    #[arg(long)]
    serve: Option<String>,
}

fn open_url_in_os(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

async fn serve_picker_ui(
    bind_addr: &str,
    session: RemoteDirectoryPickerSession,
    open_browser: bool,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;
    let url = format!(
        "http://{}/?device={}&source={}",
        local_addr, session.target_device_id, session.source_local_path
    );
    println!(
        "multinteractor remote directory picker UI listening on {} (initial_path={})",
        url, session.current_remote_path
    );
    if open_browser {
        open_url_in_os(&url);
    }
    loop {
        let (mut stream, _) = listener.accept().await?;
        let sess = session.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 16384];
            if let Ok(n) = stream.read(&mut buf).await {
                if n == 0 {
                    return;
                }
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("GET / HTTP/1.1");
                let mut parts = first_line.split_whitespace();
                let method = parts.next().unwrap_or("GET");
                let full_path = parts.next().unwrap_or("/");
                let clean_path = full_path.split('?').next().unwrap_or("/");
                if full_path.starts_with("/api/v1/") {
                    let sep = buf[..n]
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| p + 4)
                        .unwrap_or(n);
                    let body = &buf[sep..n];
                    let resp_body = sess
                        .send_ipc(method, full_path, body)
                        .unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e).into_bytes());
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        resp_body.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&resp_body).await;
                } else if clean_path == "/directory_picker.js" {
                    let js_bytes = EMBEDDED_PICKER_JS.as_bytes();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/javascript; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        js_bytes.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(js_bytes).await;
                } else if clean_path == "/transfer_toast.js" {
                    let js_bytes = EMBEDDED_TOAST_JS.as_bytes();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/javascript; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        js_bytes.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(js_bytes).await;
                } else {
                    let html_bytes = EMBEDDED_INDEX_HTML.as_bytes();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        html_bytes.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(html_bytes).await;
                }
            }
        });
    }
}

fn main() -> anyhow::Result<()> {
    let args = UiArgs::parse();

    if let Some(ref target_dir) = args.remote_dir {
        let session = RemoteDirectoryPickerSession::new(args.device.clone(), args.source.clone());
        let resp = session.execute_sync(target_dir.clone())?;
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }

    if let Some(bind_addr) = args.serve {
        let session = RemoteDirectoryPickerSession::new(args.device.clone(), args.source.clone());
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(serve_picker_ui(&bind_addr, session, false));
    }

    if args.pick_remote_dir {
        let session = RemoteDirectoryPickerSession::new(args.device.clone(), args.source.clone());
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        return rt.block_on(serve_picker_ui("127.0.0.1:0", session, true));
    }

    gui::run_native_gui(args.config, args.device, args.source)
}


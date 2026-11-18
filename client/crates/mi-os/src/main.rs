//! 操作系统深度交互代理 (`mi-os-agent`)：连接 `Alt+C`/`Alt+V` 全局热键监听、上下文抓取、智能粘贴与右键菜单导出至 `mi-daemon` 本地 IPC.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use mi_os::capture::{CapturedSelection, ContextCaptureEngine};
use mi_os::context_menu::ContextMenuRegistry;
use mi_os::hotkey::{
    run_agent_event_loop, start_global_hotkey_listener, GlobalHotkeyBinding, HotkeyAction,
};
use mi_os::platform::OsPlatformAdapter;
use mi_os::power::PowerSleepInhibitor;
use mi_os::smart_paste::{SmartPasteDecision, SmartPasteRouter};
use mi_protocol::ipc::{
    DeviceStatusDto, IpcCopyRequest, IpcEventsListResponse, IpcPasteRequest,
    DEFAULT_LOCAL_IPC_ADDR,
};

#[cfg(target_os = "windows")]
use mi_os::platform::windows::WindowsPlatformAdapter as ActivePlatformAdapter;
#[cfg(target_os = "macos")]
use mi_os::platform::macos::MacOsPlatformAdapter as ActivePlatformAdapter;
#[cfg(all(unix, not(target_os = "macos")))]
use mi_os::platform::linux::LinuxPlatformAdapter as ActivePlatformAdapter;

#[derive(Debug, Parser)]
#[command(
    name = "mi-os-agent",
    version = option_env!("MI_APP_VERSION").unwrap_or("v1"),
    about = "multinteractor OS hotkey & context integration agent"
)]
struct OsAgentArgs {
    /// Trigger an action directly: "copy" (Alt+C) or "paste" (Alt+V)
    #[arg(long)]
    trigger: Option<String>,

    /// Export & refresh OS context menu files (.reg / Info.plist) into the given directory
    #[arg(long)]
    install_context_menu: Option<PathBuf>,

    /// Override local daemon IPC address (default 127.0.0.1:53318)
    #[arg(long)]
    ipc_addr: Option<String>,

    /// Register global hotkeys, print status, and exit immediately (for health check)
    #[arg(long, default_value_t = false)]
    once: bool,
}

fn ipc_request(addr: &str, method: &str, path: &str, body: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
    let clean = addr.trim_start_matches("http://");
    let sock_addr: SocketAddr = clean.parse().or_else(|_| {
        clean
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid ipc addr"))
    })?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(3))?;
    let payload = body.unwrap_or(&[]);
    let head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        method, path, clean, payload.len()
    );
    stream.write_all(head.as_bytes())?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    stream.flush()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("Malformed IPC response"))?;
    Ok(raw[sep + 4..].to_vec())
}

fn execute_copy(ipc_addr: &str, platform: &ActivePlatformAdapter) -> anyhow::Result<String> {
    let capture = ContextCaptureEngine::new(platform.clone());
    let sel = capture.capture_on_alt_c()?;
    let req = match sel {
        CapturedSelection::Text(t) => IpcCopyRequest {
            text: Some(t),
            path: None,
        },
        CapturedSelection::FileSystemPaths(paths) => {
            let first = paths
                .first()
                .ok_or_else(|| anyhow::anyhow!("Empty file selection"))?;
            IpcCopyRequest {
                text: None,
                path: Some(first.to_string_lossy().to_string()),
            }
        }
        CapturedSelection::ImageBytes(bytes) => {
            let tmp = std::env::temp_dir().join("mi_clipboard_image.png");
            std::fs::write(&tmp, &bytes)?;
            IpcCopyRequest {
                text: None,
                path: Some(tmp.to_string_lossy().to_string()),
            }
        }
    };
    let payload = serde_json::to_vec(&req)?;
    let resp = ipc_request(ipc_addr, "POST", "/api/v1/copy", Some(&payload))?;
    Ok(String::from_utf8_lossy(&resp).to_string())
}

fn execute_paste(ipc_addr: &str, platform: &ActivePlatformAdapter) -> anyhow::Result<String> {
    let raw = ipc_request(ipc_addr, "GET", "/api/v1/events?limit=1", None)?;
    let list: IpcEventsListResponse = serde_json::from_slice(&raw)?;
    let latest = list
        .events
        .first()
        .ok_or_else(|| anyhow::anyhow!("No events in EventCacheList"))?;
    let router = SmartPasteRouter::new(platform.clone());
    let decision = router.route_alt_v(latest)?;
    match decision {
        SmartPasteDecision::PasteTextIntoEditor { text, .. } => {
            platform.write_text_to_clipboard_and_paste(&text)?;
            Ok(serde_json::to_string_pretty(&serde_json::json!({
                "action": "PASTE_TO_EDITOR",
                "event_id": latest.event_id,
            }))?)
        }
        SmartPasteDecision::DirectSaveToFolder { target_dir, .. } => {
            let _inhibitor = PowerSleepInhibitor::acquire(format!("paste_{}", latest.event_id));
            let req = IpcPasteRequest {
                event_id: Some(latest.event_id.clone()),
                dest_dir: Some(target_dir.to_string_lossy().to_string()),
            };
            let payload = serde_json::to_vec(&req)?;
            let resp = ipc_request(ipc_addr, "POST", "/api/v1/paste", Some(&payload))?;
            Ok(String::from_utf8_lossy(&resp).to_string())
        }
        SmartPasteDecision::ShowNativeSaveAsDialog {
            chosen_dir: Some(target_dir),
            ..
        } => {
            let _inhibitor = PowerSleepInhibitor::acquire(format!("paste_{}", latest.event_id));
            let req = IpcPasteRequest {
                event_id: Some(latest.event_id.clone()),
                dest_dir: Some(target_dir.to_string_lossy().to_string()),
            };
            let payload = serde_json::to_vec(&req)?;
            let resp = ipc_request(ipc_addr, "POST", "/api/v1/paste", Some(&payload))?;
            Ok(String::from_utf8_lossy(&resp).to_string())
        }
        SmartPasteDecision::ShowNativeSaveAsDialog { chosen_dir: None, .. } => {
            Ok("{\"action\":\"CANCELLED\"}".to_string())
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = OsAgentArgs::parse();
    let ipc_addr = args
        .ipc_addr
        .or_else(|| std::env::var("MI_LOCAL_IPC_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_LOCAL_IPC_ADDR.to_string());

    if let Some(ref out_dir) = args.install_context_menu {
        let raw = ipc_request(&ipc_addr, "GET", "/api/v1/devices", None).unwrap_or_else(|_| b"[]".to_vec());
        let devices: Vec<DeviceStatusDto> = serde_json::from_slice(&raw).unwrap_or_default();
        let mut reg = ContextMenuRegistry::new();
        reg.refresh_online_devices(&devices, "");
        let (reg_path, plist_path) = reg.write_install_files(out_dir)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "installed": true,
                "online_targets": reg.menu_items().len(),
                "windows_reg": reg_path,
                "macos_plist": plist_path,
            }))?
        );
        return Ok(());
    }

    let binding = GlobalHotkeyBinding::default();
    let platform = ActivePlatformAdapter::default();

    match args.trigger.as_deref() {
        Some("copy") => {
            assert_eq!(binding.resolve_chord("Alt+C"), Some(HotkeyAction::BroadcastCopy));
            println!("{}", execute_copy(&ipc_addr, &platform)?);
        }
        Some("paste") => {
            assert_eq!(binding.resolve_chord("Alt+V"), Some(HotkeyAction::SmartPaste));
            println!("{}", execute_paste(&ipc_addr, &platform)?);
        }
        Some(other) => {
            anyhow::bail!("Unknown trigger '{}', expected 'copy' or 'paste'", other);
        }
        None => {
            let (hotkey_handle, hotkey_rx) = start_global_hotkey_listener(&binding)?;
            println!(
                "mi-os-agent ready (copy={}, paste={}, ipc={}, active={}, backend=\"{}\", note=\"{}\")",
                binding.copy_chord,
                binding.paste_chord,
                ipc_addr,
                hotkey_handle.status.active,
                hotkey_handle.status.backend,
                hotkey_handle.status.note,
            );
            if args.once {
                return Ok(());
            }

            let stop_flag = Arc::new(AtomicBool::new(false));
            let worker_stop = stop_flag.clone();
            let ipc_clone = ipc_addr.clone();
            std::thread::spawn(move || {
                while let Ok(action) = hotkey_rx.recv() {
                    if worker_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let res = match action {
                        HotkeyAction::BroadcastCopy => execute_copy(&ipc_clone, &platform),
                        HotkeyAction::SmartPaste => execute_paste(&ipc_clone, &platform),
                    };
                    match res {
                        Ok(out) => println!("[hotkey:{:?}] {}", action, out),
                        Err(err) => eprintln!("[hotkey:{:?}] error: {}", action, err),
                    }
                }
            });

            run_agent_event_loop(&stop_flag);
            stop_flag.store(true, Ordering::SeqCst);
        }
    }

    Ok(())
}

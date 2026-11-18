//! 端到端集成测试：双节点 (`node_a` <-> `node_c`) 真实 TCP/IPC + 内网 1-to-N 通道联调验证
//! 覆盖：文本广播粘贴、单文件广播粘贴、多级目录 `.tar.zst` 打包解包粘贴、远程目录浏览 (`remote-ls`)、
//! 远程新建文件夹 (`remote-mkdir`)、定向同步 (`sync`) 以及 180s 空闲连接自动释放.

use std::fs;
use std::time::{Duration, Instant};
use mi_daemon::config::DaemonConfig;
use mi_daemon::DaemonNode;
use mi_protocol::ipc::{
    DeviceStatusDto, IpcCopyRequest, IpcPasteRequest, IpcRemoteLsRequest, IpcRemoteMkdirRequest,
    IpcSyncRequest,
};

#[tokio::test]
async fn test_two_daemon_nodes_end_to_end_workflows() {
    let base_dir = std::env::temp_dir().join(format!("mi_e2e_rust_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base_dir);
    let dir_a = base_dir.join("node_a");
    let dir_c = base_dir.join("node_c");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_c).unwrap();

    let cfg_a = DaemonConfig {
        device_id: "node_a".to_string(),
        device_name: "Home-Win".to_string(),
        os_name: "windows".to_string(),
        lan_bind_addr: "127.0.0.1:0".to_string(),
        local_ipc_addr: "127.0.0.1:0".to_string(),
        data_dir: dir_a.clone(),
        ..DaemonConfig::default()
    };
    let cfg_c = DaemonConfig {
        device_id: "node_c".to_string(),
        device_name: "Office-Mac".to_string(),
        os_name: "macos".to_string(),
        lan_bind_addr: "127.0.0.1:0".to_string(),
        local_ipc_addr: "127.0.0.1:0".to_string(),
        data_dir: dir_c.clone(),
        ..DaemonConfig::default()
    };

    let node_a = DaemonNode::new(cfg_a);
    let node_c = DaemonNode::new(cfg_c);

    // Register each node's LAN server and peer metadata
    node_a.lan_pool.register_peer_server(
        "node_c",
        mi_daemon::lan_channel::LanChannelServer::new(
            node_c.lan_pool.clone(),
            node_c.fs_rpc.clone(),
            node_c.transfer_mgr.clone(),
        ),
    );
    node_c.lan_pool.register_peer_server(
        "node_a",
        mi_daemon::lan_channel::LanChannelServer::new(
            node_a.lan_pool.clone(),
            node_a.fs_rpc.clone(),
            node_a.transfer_mgr.clone(),
        ),
    );

    node_a.signaling.upsert_peer(DeviceStatusDto {
        device_id: "node_c".to_string(),
        name: "Office-Mac".to_string(),
        os: "macos".to_string(),
        online: true,
        lan_endpoint: Some("127.0.0.1:18443".to_string()),
        lan_reachable: true,
        cert_sha256: None,
    });
    node_c.signaling.upsert_peer(DeviceStatusDto {
        device_id: "node_a".to_string(),
        name: "Home-Win".to_string(),
        os: "windows".to_string(),
        online: true,
        lan_endpoint: Some("127.0.0.1:18444".to_string()),
        lan_reachable: true,
        cert_sha256: None,
    });

    // 1. Text copy on A -> deliver envelope to C -> paste on C
    let copy_txt = node_a
        .ipc
        .handle_copy(
            IpcCopyRequest {
                text: Some("跨设备剪贴板端到端测试文本".to_string()),
                path: None,
            },
            "evt_txt_01".to_string(),
            1700000001000,
        )
        .unwrap();
    assert_eq!(copy_txt.content_category, "TEXT");
    for env in node_a.signaling.drain_outbox() {
        node_c.signaling.handle_incoming_envelope(env).unwrap();
    }
    let paste_txt = node_c
        .ipc
        .handle_paste(IpcPasteRequest {
            event_id: Some("evt_txt_01".to_string()),
            dest_dir: None,
        })
        .unwrap();
    assert!(paste_txt.digest_verified);
    assert_eq!(
        paste_txt.inline_text.as_deref(),
        Some("跨设备剪贴板端到端测试文本")
    );

    // 2. Single file copy on A -> LAN Range pull & BLAKE3 verify on C
    let src_file = dir_a.join("季度架构报告.pdf");
    let file_bytes = b"PDF-1.7 multinteractor binary content verified by BLAKE3";
    fs::write(&src_file, file_bytes).unwrap();

    let copy_file = node_a
        .ipc
        .handle_copy(
            IpcCopyRequest {
                text: None,
                path: Some(src_file.to_string_lossy().to_string()),
            },
            "evt_file_02".to_string(),
            1700000002000,
        )
        .unwrap();
    assert_eq!(copy_file.content_category, "FILE");
    for mut env in node_a.signaling.drain_outbox() {
        env.source_device.lan_endpoint = Some("127.0.0.1:18444".to_string());
        node_c.signaling.handle_incoming_envelope(env).unwrap();
    }

    let c_downloads = dir_c.join("Downloads");
    let paste_file = node_c
        .ipc
        .handle_paste(IpcPasteRequest {
            event_id: Some("evt_file_02".to_string()),
            dest_dir: Some(c_downloads.to_string_lossy().to_string()),
        })
        .unwrap();
    assert!(paste_file.digest_verified);
    assert_eq!(paste_file.channel_used, "LAN_DIRECT");
    assert_eq!(
        fs::read(c_downloads.join("季度架构报告.pdf")).unwrap(),
        file_bytes
    );

    // 3. Multi-level directory copy on A (.tar.zst) -> LAN pull & unpack on C
    let src_folder = dir_a.join("项目源码");
    fs::create_dir_all(src_folder.join("src/empty_mod")).unwrap();
    fs::write(src_folder.join("Cargo.toml"), b"[package]\nname = \"demo\"\n").unwrap();
    fs::write(src_folder.join("src/lib.rs"), b"pub fn hello() -> &'static str { \"ok\" }").unwrap();

    let copy_dir = node_a
        .ipc
        .handle_copy(
            IpcCopyRequest {
                text: None,
                path: Some(src_folder.to_string_lossy().to_string()),
            },
            "evt_dir_03".to_string(),
            1700000003000,
        )
        .unwrap();
    assert_eq!(copy_dir.content_category, "DIRECTORY");
    for mut env in node_a.signaling.drain_outbox() {
        env.source_device.lan_endpoint = Some("127.0.0.1:18444".to_string());
        node_c.signaling.handle_incoming_envelope(env).unwrap();
    }

    let c_workspace = dir_c.join("Workspace");
    let paste_dir = node_c
        .ipc
        .handle_paste(IpcPasteRequest {
            event_id: Some("evt_dir_03".to_string()),
            dest_dir: Some(c_workspace.to_string_lossy().to_string()),
        })
        .unwrap();
    assert!(paste_dir.digest_verified);
    assert!(c_workspace.join("项目源码/src/empty_mod").is_dir());
    assert_eq!(
        fs::read_to_string(c_workspace.join("项目源码/src/lib.rs")).unwrap(),
        "pub fn hello() -> &'static str { \"ok\" }"
    );

    // 4. Remote mkdir & remote ls from A to C over LAN RPC
    let remote_new_dir = dir_c.join("SyncedDocs/2026_Q4");
    let mkdir_resp = node_a
        .ipc
        .handle_remote_mkdir(IpcRemoteMkdirRequest {
            device: "node_c".to_string(),
            path: remote_new_dir.to_string_lossy().to_string(),
        })
        .unwrap();
    assert_eq!(mkdir_resp.status, "CREATED");
    assert!(remote_new_dir.is_dir());

    let ls_resp = node_a
        .ipc
        .handle_remote_ls(IpcRemoteLsRequest {
            device: "node_c".to_string(),
            path: dir_c.join("SyncedDocs").to_string_lossy().to_string(),
        })
        .unwrap();
    assert!(ls_resp.entries.iter().any(|e| e.name == "2026_Q4" && e.is_dir));
    assert!(ls_resp.free_space_bytes > 0);

    // 5. Direct sync from A to C's newly created folder
    let sync_resp = node_a
        .ipc
        .handle_sync(
            IpcSyncRequest {
                source: src_file.to_string_lossy().to_string(),
                target_device: "node_c".to_string(),
                remote_dir: remote_new_dir.to_string_lossy().to_string(),
            },
            1700000004000,
        )
        .unwrap();
    assert!(sync_resp.digest_verified);
    assert_eq!(sync_resp.channel_used, "LAN_DIRECT");
    assert_eq!(
        fs::read(remote_new_dir.join("季度架构报告.pdf")).unwrap(),
        file_bytes
    );

    // 6. Verify LAN channel pool active connection count & 180s idle reap
    assert!(node_a.lan_pool.active_connection_count() >= 1);
    let closed = node_a
        .lan_pool
        .reap_idle_channels(Instant::now() + Duration::from_secs(181));
    assert!(!closed.is_empty());
    assert_eq!(node_a.lan_pool.active_connection_count(), 0);

    let _ = fs::remove_dir_all(&base_dir);
}

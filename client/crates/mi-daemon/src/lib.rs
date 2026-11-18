//! 本地常驻守护服务核心模块：云端长连接、本地事件队列、1-to-N 内网即用即连 (180s 空闲断开) 与全链路断点续传引擎.

pub mod cert_manager;
pub mod cloud_client;
pub mod config;
pub mod event_queue;
pub mod fs_rpc;
pub mod ipc_server;
pub mod lan_channel;
pub mod transfer;

use std::net::SocketAddr;
use std::time::Duration;
use cert_manager::CertManager;
use cloud_client::CloudSignalingClient;
use config::DaemonConfig;
use event_queue::EventCacheList;
use fs_rpc::FsRpcHandler;
use ipc_server::LocalIpcServer;
use lan_channel::{LanChannelPool, LanChannelServer};
use tokio::sync::watch;
use transfer::TransferManager;

pub struct RunningDaemonHandles {
    pub ipc_addr: SocketAddr,
    pub lan_addr: SocketAddr,
    pub shutdown_tx: watch::Sender<bool>,
}

impl RunningDaemonHandles {
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

#[derive(Debug, Clone)]
pub struct DaemonNode {
    pub config: DaemonConfig,
    pub cert_mgr: CertManager,
    pub event_queue: EventCacheList,
    pub signaling: CloudSignalingClient,
    pub lan_pool: LanChannelPool,
    pub fs_rpc: FsRpcHandler,
    pub transfer_mgr: TransferManager,
    pub ipc: LocalIpcServer,
}

impl DaemonNode {
    pub fn new(config: DaemonConfig) -> Self {
        let cert_mgr = CertManager::load_or_create(
            &config.data_dir,
            &config.device_id,
            config.server_cert_sha256.as_deref(),
        )
        .expect("Failed to initialize local CertManager");
        let events_file = config.data_dir.join(format!("events_{}.json", config.device_id));
        let event_queue = EventCacheList::with_persistence(config.event_queue_capacity, events_file);
        let signaling = CloudSignalingClient::with_cert_manager(
            config.clone(),
            event_queue.clone(),
            cert_mgr.clone(),
        );
        let allowed_peer_fps = std::sync::Arc::new(std::sync::RwLock::new(
            std::collections::HashSet::new(),
        ));
        let lan_pool = LanChannelPool::new(Duration::from_secs(config.lan_idle_timeout_secs))
            .with_tls(cert_mgr.clone(), allowed_peer_fps);
        let fs_rpc = FsRpcHandler::with_allowed_roots(
            config.device_id.clone(),
            config.allowed_dirs.clone(),
        );
        let transfer_mgr = TransferManager::new();
        let mut ipc = LocalIpcServer::with_components(
            signaling.clone(),
            event_queue.clone(),
            lan_pool.clone(),
            fs_rpc.clone(),
            transfer_mgr.clone(),
        );
        ipc.set_cert_manager(cert_mgr.clone());
        Self {
            config,
            cert_mgr,
            event_queue,
            signaling,
            lan_pool,
            fs_rpc,
            transfer_mgr,
            ipc,
        }
    }

    /// 启动本地 IPC 服务、内网 1-to-N 监听服务以及 180 秒空闲连接自动回收协程.
    pub async fn start_background(&self) -> anyhow::Result<RunningDaemonHandles> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let lan_srv = LanChannelServer::new(
            self.lan_pool.clone(),
            self.fs_rpc.clone(),
            self.transfer_mgr.clone(),
        );
        let (lan_addr, _lan_handle) = lan_srv
            .bind_and_serve(&self.config.lan_bind_addr, shutdown_rx.clone())
            .await?;

        let (ipc_addr, _ipc_handle) = self
            .ipc
            .clone()
            .bind_and_serve(&self.config.local_ipc_addr, shutdown_rx.clone())
            .await?;

        let _reaper = self
            .lan_pool
            .spawn_idle_reaper(Duration::from_secs(5), shutdown_rx.clone());

        let _signaling = self.signaling.spawn_signaling_loop(shutdown_rx);

        Ok(RunningDaemonHandles {
            ipc_addr,
            lan_addr,
            shutdown_tx,
        })
    }
}


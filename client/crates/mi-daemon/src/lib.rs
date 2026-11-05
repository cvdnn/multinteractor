//! 本地常驻守护服务核心模块：云端长连接、本地事件队列、1-to-N 内网即用即连 (180s 空闲断开) 与全链路断点续传引擎.

pub mod cloud_client;
pub mod config;
pub mod event_queue;
pub mod fs_rpc;
pub mod ipc_server;
pub mod lan_channel;
pub mod transfer;

use cloud_client::CloudSignalingClient;
use config::DaemonConfig;
use event_queue::EventCacheList;
use ipc_server::LocalIpcServer;

#[derive(Debug, Clone)]
pub struct DaemonNode {
    pub config: DaemonConfig,
    pub event_queue: EventCacheList,
    pub signaling: CloudSignalingClient,
    pub ipc: LocalIpcServer,
}

impl DaemonNode {
    pub fn new(config: DaemonConfig) -> Self {
        let event_queue = EventCacheList::new(config.event_queue_capacity);
        let signaling = CloudSignalingClient::new(config.clone(), event_queue.clone());
        let ipc = LocalIpcServer::new(signaling.clone(), event_queue.clone());
        Self {
            config,
            event_queue,
            signaling,
            ipc,
        }
    }
}

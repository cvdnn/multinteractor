//! 内网即用即连 1-to-N 双向通道池 (`LanChannelPool`) 与空闲满 180 秒自动关闭计时器 (`IdleTimer`) (FR-TRANS-01, TASK-05).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub const DEFAULT_LAN_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
pub struct LanChannelSession {
    pub peer_device_id: String,
    pub peer_endpoint: String,
    pub established_at: Instant,
    pub last_active_at: Instant,
    pub reuse_count: u64,
    pub bytes_transferred: u64,
}

impl LanChannelSession {
    pub fn new(peer_device_id: String, peer_endpoint: String, now: Instant) -> Self {
        Self {
            peer_device_id,
            peer_endpoint,
            established_at: now,
            last_active_at: now,
            reuse_count: 0,
            bytes_transferred: 0,
        }
    }

    pub fn touch(&mut self, now: Instant, bytes_delta: u64) {
        self.last_active_at = now;
        self.reuse_count += 1;
        self.bytes_transferred += bytes_delta;
    }

    pub fn is_idle_expired(&self, now: Instant, idle_timeout: Duration) -> bool {
        now.duration_since(self.last_active_at) >= idle_timeout
    }
}

/// 内网即用即连 1-to-N 并发连接池管理器：
/// 1. 平时零内网常驻连接 (`active_connection_count() == 0`)；
/// 2. 触发文件拉取或目录浏览时即用即连 (`acquire_or_reuse`)，支持一对多 (`1-to-N`) 多机并发；
/// 3. 活跃期复用并重置空闲计时器；当空闲满 `180 秒` (可配置) 无数据交互后自动关闭释放连接 (`reap_idle_channels`).
#[derive(Debug, Clone)]
pub struct LanChannelPool {
    idle_timeout: Duration,
    sessions: Arc<RwLock<HashMap<String, LanChannelSession>>>,
}

impl LanChannelPool {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            idle_timeout,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_default_timeout() -> Self {
        Self::new(DEFAULT_LAN_IDLE_TIMEOUT)
    }

    pub fn active_connection_count(&self) -> usize {
        let guard = self.sessions.read().expect("lan pool lock poisoned");
        guard.len()
    }

    /// 按需建立或复用与目标机器的内网双向通道，并重置空闲 180 秒倒计时.
    pub fn acquire_or_reuse(
        &self,
        peer_device_id: &str,
        peer_endpoint: &str,
        now: Instant,
    ) -> (LanChannelSession, bool) {
        self.reap_idle_channels(now);
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        if let Some(existing) = guard.get_mut(peer_device_id) {
            existing.touch(now, 0);
            return (existing.clone(), true);
        }

        let session = LanChannelSession::new(
            peer_device_id.to_string(),
            peer_endpoint.to_string(),
            now,
        );
        guard.insert(peer_device_id.to_string(), session.clone());
        (session, false)
    }

    pub fn record_io(&self, peer_device_id: &str, bytes: u64, now: Instant) -> bool {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        if let Some(session) = guard.get_mut(peer_device_id) {
            session.touch(now, bytes);
            return true;
        }
        false
    }

    /// 扫描并关闭所有空闲满 `idle_timeout` (默认 180 秒) 无数据交互的内网连接，返回被关闭的设备 ID 列表.
    pub fn reap_idle_channels(&self, now: Instant) -> Vec<String> {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        let mut closed = Vec::new();
        guard.retain(|device_id, session| {
            if session.is_idle_expired(now, self.idle_timeout) {
                closed.push(device_id.clone());
                false
            } else {
                true
            }
        });
        closed
    }

    pub fn force_close(&self, peer_device_id: &str) -> bool {
        let mut guard = self.sessions.write().expect("lan pool lock poisoned");
        guard.remove(peer_device_id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_initial_connections_1_to_n_concurrency_and_180s_idle_disconnect() {
        let pool = LanChannelPool::with_default_timeout();
        let t0 = Instant::now();

        // 1. 初始空闲状态下内网连接数为 0
        assert_eq!(pool.active_connection_count(), 0);

        // 2. B 和 C 同时按需连入 A (1-to-2 并发)
        let (_, reused_b) = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", t0);
        let (_, reused_c) = pool.acquire_or_reuse("node_c", "192.168.1.12:8443", t0);
        assert!(!reused_b);
        assert!(!reused_c);
        assert_eq!(pool.active_connection_count(), 2);

        // 3. 第 100 秒时 node_b 再次传输 -> 复用连接并重置 node_b 的 180s 计时器
        let t100 = t0 + Duration::from_secs(100);
        let (sess_b, reused_b_again) = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", t100);
        assert!(reused_b_again);
        assert_eq!(sess_b.reuse_count, 1);

        // 4. 第 180 秒时：node_c 已空闲满 180 秒自动关闭，而 node_b 仅空闲 80 秒继续保留
        let t180 = t0 + Duration::from_secs(180);
        let closed_at_180 = pool.reap_idle_channels(t180);
        assert_eq!(closed_at_180, vec!["node_c".to_string()]);
        assert_eq!(pool.active_connection_count(), 1);

        // 5. 第 280 秒时 (node_b 空闲满 180 秒)：node_b 连接也自动关闭，连接数归零
        let t280 = t100 + Duration::from_secs(180);
        let closed_at_280 = pool.reap_idle_channels(t280);
        assert_eq!(closed_at_280, vec!["node_b".to_string()]);
        assert_eq!(pool.active_connection_count(), 0);
    }
}

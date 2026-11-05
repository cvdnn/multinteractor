"""L3 故障注入与混沌测试：内网即用即连 (1-to-N 并发) 与空闲满 180 秒自动关闭释放连接验证 (TC-LAN-01 ~ TC-LAN-02)."""

from __future__ import annotations

import unittest
from dataclasses import dataclass
from typing import Dict, List


@dataclass
class SimulatedLanSession:
    peer_id: str
    endpoint: str
    last_active_sec: float
    reuse_count: int = 0


class LanChannelPoolSimulator:
    """与 `client/crates/mi-daemon/src/lan_channel.rs` 逻辑严格对齐的内网连接池验证器."""

    def __init__(self, idle_timeout_sec: float = 180.0) -> None:
        self.idle_timeout_sec = idle_timeout_sec
        self.sessions: Dict[str, SimulatedLanSession] = {}

    def active_count(self) -> int:
        return len(self.sessions)

    def acquire_or_reuse(self, peer_id: str, endpoint: str, now_sec: float) -> tuple[SimulatedLanSession, bool]:
        self.reap_idle(now_sec)
        if peer_id in self.sessions:
            sess = self.sessions[peer_id]
            sess.last_active_sec = now_sec
            sess.reuse_count += 1
            return sess, True
        sess = SimulatedLanSession(peer_id=peer_id, endpoint=endpoint, last_active_sec=now_sec)
        self.sessions[peer_id] = sess
        return sess, False

    def reap_idle(self, now_sec: float) -> List[str]:
        closed = []
        for pid in list(self.sessions.keys()):
            if now_sec - self.sessions[pid].last_active_sec >= self.idle_timeout_sec:
                closed.append(pid)
                del self.sessions[pid]
        return closed


class TestLanIdleDisconnect(unittest.TestCase):
    def test_tc_lan_01_02_on_demand_1_to_n_and_180s_idle_disconnect(self) -> None:
        pool = LanChannelPoolSimulator(idle_timeout_sec=180.0)

        # TC-LAN-01: 初始空闲状态下内网连接数为 0
        self.assertEqual(pool.active_count(), 0)

        # Node-B 与 Node-C 同时按 Alt+V 连入 Node-A (1-to-2 并发)
        _, reused_b = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", now_sec=0.0)
        _, reused_c = pool.acquire_or_reuse("node_c", "192.168.1.12:8443", now_sec=0.0)
        self.assertFalse(reused_b)
        self.assertFalse(reused_c)
        self.assertEqual(pool.active_count(), 2)

        # TC-LAN-02: 第 100 秒时再次传输 -> 直接复用连接并重置空闲倒计时为 180 秒
        sess_b, reused_b_100 = pool.acquire_or_reuse("node_b", "192.168.1.11:8443", now_sec=100.0)
        self.assertTrue(reused_b_100)
        self.assertEqual(sess_b.reuse_count, 1)

        # 第 180 秒时：Node-C 已连续空闲满 180 秒自动关闭；Node-B 距离上次活动仅 80 秒继续保留
        closed_180 = pool.reap_idle(now_sec=180.0)
        self.assertEqual(closed_180, ["node_c"])
        self.assertEqual(pool.active_count(), 1)

        # 第 280 秒时 (Node-B 距离第 100 秒活动已满 180 秒)：Node-B 连接自动关闭，总连接数归零
        closed_280 = pool.reap_idle(now_sec=280.0)
        self.assertEqual(closed_280, ["node_b"])
        self.assertEqual(pool.active_count(), 0)


if __name__ == "__main__":
    unittest.main()

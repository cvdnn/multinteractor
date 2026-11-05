"""客户端注册准入与 Topic 订阅路由隔离单元测试 (对应 TC-SRV-01, TC-SRV-02, TC-NFR-04)."""

from __future__ import annotations

import asyncio
import unittest

from server.app.client_sub_manager import PermissionDeniedError
from server.app.models import (
    SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST,
    CloudEventEnvelope,
    DigestManifest,
    EventPayload,
    SourceDevice,
    TargetScope,
    topic_sync_direct,
)
from server.tests.conftest import cleanup_test_hub, make_test_hub


class TestClientSubscriptionManager(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)

    def test_three_nodes_registration_presence_and_topic_isolation(self) -> None:
        async def _run() -> None:
            dev_a = await self.hub.register_ws_client("node_a", "Node-A", "windows", "192.168.1.10:8443")
            dev_b = await self.hub.register_ws_client("node_b", "Node-B", "windows", "192.168.1.11:8443")
            dev_c = await self.hub.register_ws_client("node_c", "Node-C", "macos", "192.168.1.12:8443")

            # 清空初始上线 Presence 通知
            while not dev_a.inbox.empty():
                await dev_a.inbox.get()
            while not dev_b.inbox.empty():
                await dev_b.inbox.get()
            while not dev_c.inbox.empty():
                await dev_c.inbox.get()

            # 1. 广播事件：Node-A 发送 topic.clipboard.broadcast -> Node-B 和 Node-C 均收到，Node-A 不回传
            text_bytes = "人工智能".encode("utf-8")
            bcast_env = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_bcast_01",
                topic=TOPIC_CLIPBOARD_BROADCAST,
                event_type="EVENT_CLIPBOARD_BROADCAST",
                timestamp_ms=1762310000000,
                source_device=SourceDevice("node_a", "Node-A", "windows", "192.168.1.10:8443"),
                target_scope=TargetScope(mode="BROADCAST", target_device_ids=["*"]),
                payload=EventPayload(
                    content_category="TEXT",
                    digest=DigestManifest.from_bytes(text_bytes),
                    inline_data="人工智能",
                    file_cache_id=None,
                    upload_state="INLINE",
                ),
            )
            delivered = await self.hub.publish_event(bcast_env)
            self.assertEqual(sorted(delivered), ["node_b", "node_c"])
            self.assertTrue(dev_a.inbox.empty())
            msg_b = await asyncio.wait_for(dev_b.inbox.get(), timeout=1.0)
            msg_c = await asyncio.wait_for(dev_c.inbox.get(), timeout=1.0)
            self.assertEqual(msg_b["event_id"], "evt_bcast_01")
            self.assertEqual(msg_c["event_id"], "evt_bcast_01")

            # 2. 单播事件隔离 (TC-SRV-02)：Node-A 向 Node-C 发布定向同步 -> 仅 Node-C 收到，Node-B 不收到
            direct_env = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_direct_02",
                topic=topic_sync_direct("node_c"),
                event_type="EVENT_DIRECT_SYNC",
                timestamp_ms=1762310001000,
                source_device=SourceDevice("node_a", "Node-A", "windows", "192.168.1.10:8443"),
                target_scope=TargetScope(mode="DIRECT", target_device_ids=["node_c"]),
                payload=EventPayload(
                    content_category="FILE",
                    digest=DigestManifest.from_bytes(b"docx-content"),
                    inline_data=None,
                    file_cache_id="fc_01",
                    upload_state="COMPLETED",
                    destination_override="D:\\工作目录",
                ),
            )
            delivered_direct = await self.hub.publish_event(direct_env)
            self.assertEqual(delivered_direct, ["node_c"])
            self.assertTrue(dev_b.inbox.empty())
            direct_msg_c = await asyncio.wait_for(dev_c.inbox.get(), timeout=1.0)
            self.assertEqual(direct_msg_c["event_id"], "evt_direct_02")

            # 3. Node-B 下线后，Node-A 与 Node-C 立即收到 EVENT_DEVICE_PRESENCE (TC-SRV-01)
            await self.hub.disconnect_ws_client("node_b")
            pres_a = await asyncio.wait_for(dev_a.inbox.get(), timeout=1.0)
            pres_c = await asyncio.wait_for(dev_c.inbox.get(), timeout=1.0)
            self.assertEqual(pres_a["event_type"], "EVENT_DEVICE_PRESENCE")
            self.assertIn("OFFLINE", pres_a["payload"]["inline_data"])
            self.assertIn("OFFLINE", pres_c["payload"]["inline_data"])

            # 4. 非法 Token 注册被拒绝 (TC-NFR-04)
            with self.assertRaises(PermissionDeniedError):
                await self.hub.register_ws_client(
                    "node_evil", "Evil", "linux", "10.0.0.9:8443", auth_token="wrong-token"
                )

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

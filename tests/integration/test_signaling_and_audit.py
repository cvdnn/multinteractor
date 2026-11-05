"""L2 集成测试：客户端与云端信令注册、Topic 订阅隔离及 SQLite 全链路审计日志验证 (TC-SRV-01 ~ TC-SRV-03)."""

from __future__ import annotations

import asyncio
import unittest

from server.app.models import (
    SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST,
    CloudEventEnvelope,
    DigestManifest,
    EventPayload,
    ItemMeta,
    SourceDevice,
    TargetScope,
    topic_sync_direct,
)
from server.tests.conftest import cleanup_test_hub, make_test_hub


class TestSignalingAndAuditIntegration(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)

    def test_tc_srv_01_02_03_full_signaling_and_audit_pipeline(self) -> None:
        async def _run() -> None:
            # TC-SRV-01: Node-A, Node-B, Node-C 依次注册上线并同步内网 IP:Port
            dev_a = await self.hub.register_ws_client("node_a", "Node-A", "windows", "192.168.1.10:8443")
            dev_b = await self.hub.register_ws_client("node_b", "Node-B", "windows", "192.168.1.11:8443")
            dev_c = await self.hub.register_ws_client("node_c", "Node-C", "macos", "192.168.1.12:8443")

            devices = self.hub.devices_api.list_devices(online_only=True)
            self.assertEqual(len(devices), 3)

            # 清空上线 Presence 通知
            for d in (dev_a, dev_b, dev_c):
                while not d.inbox.empty():
                    await d.inbox.get()

            # TC-SRV-02: 广播事件与定向单播事件隔离
            text_bytes = "人工智能".encode("utf-8")
            bcast = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_tc_srv_bcast",
                topic=TOPIC_CLIPBOARD_BROADCAST,
                event_type="EVENT_CLIPBOARD_BROADCAST",
                timestamp_ms=1762310000100,
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
            targets = await self.hub.publish_event(bcast)
            self.assertEqual(sorted(targets), ["node_b", "node_c"])

            direct = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_tc_srv_direct",
                topic=topic_sync_direct("node_c"),
                event_type="EVENT_DIRECT_SYNC",
                timestamp_ms=1762310000200,
                source_device=SourceDevice("node_a", "Node-A", "windows", "192.168.1.10:8443"),
                target_scope=TargetScope(mode="DIRECT", target_device_ids=["node_c"]),
                payload=EventPayload(
                    content_category="FILE",
                    digest=DigestManifest.from_bytes(b"AI tutorial docx bytes"),
                    inline_data=None,
                    file_cache_id="fc_srv_01",
                    upload_state="COMPLETED",
                    items_meta=[ItemMeta(name="AI教程.docx", is_dir=False, size_bytes=22, mtime=1762310000)],
                    destination_override="D:\\工作目录",
                ),
            )
            direct_targets = await self.hub.publish_event(direct)
            self.assertEqual(direct_targets, ["node_c"])
            self.assertEqual(dev_b.inbox.qsize(), 1)  # 仅收到广播，未收到 node_c 的单播
            self.assertEqual(dev_c.inbox.qsize(), 2)  # 收到广播 + 单播

            # 记录文件传输审计并验证 TC-SRV-03
            self.hub.audit.log_transfer(
                transfer_id="tr_srv_01",
                file_cache_id="fc_srv_01",
                source_device_id="node_a",
                target_device_id="node_c",
                channel_used="LAN_DIRECT",
                bytes_transferred=22,
                verification_status="VERIFIED_OK",
                content_hash=direct.payload.digest.full_hash,
            )

            await self.hub.disconnect_ws_client("node_b")
            audit_res = self.hub.get_audit_logs()
            self.assertGreaterEqual(len(audit_res["connection_logs"]), 4)
            self.assertEqual(len(audit_res["event_logs"]), 2)
            self.assertEqual(audit_res["transfer_logs"][0]["verification_status"], "VERIFIED_OK")

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

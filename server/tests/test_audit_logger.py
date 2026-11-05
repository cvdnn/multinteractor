"""SQLite WAL 审计日志并发写入与多维检索单元测试 (对应 FR-SRV-02, TC-SRV-03)."""

from __future__ import annotations

import asyncio
import unittest

from server.tests.conftest import cleanup_test_hub, make_test_hub


class TestAuditLogger(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)

    def test_connection_event_and_transfer_audit_logs(self) -> None:
        async def _run() -> None:
            await self.hub.register_ws_client("node_a", "Node-A", "windows", "192.168.1.10:8443")
            await self.hub.register_ws_client("node_b", "Node-B", "macos", "192.168.1.11:8443")

            self.hub.audit.log_transfer(
                transfer_id="tr_1001",
                file_cache_id="fc_20261105_01",
                source_device_id="node_a",
                target_device_id="node_b",
                channel_used="LAN_DIRECT",
                bytes_transferred=1548290,
                verification_status="VERIFIED_OK",
                content_hash="4a8b9c",
                resumed=True,
                resume_offset=524288,
                resume_count=1,
            )

            logs = self.hub.get_audit_logs(device_id="node_a")
            self.assertEqual(len(logs["connection_logs"]), 1)
            self.assertEqual(logs["connection_logs"][0]["action"], "ONLINE")
            self.assertEqual(len(logs["transfer_logs"]), 1)
            tlog = logs["transfer_logs"][0]
            self.assertEqual(tlog["verification_status"], "VERIFIED_OK")
            self.assertTrue(tlog["resumed"])
            self.assertEqual(tlog["resume_offset"], 524288)

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

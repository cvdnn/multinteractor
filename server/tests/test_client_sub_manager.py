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

    def test_token_rotation_and_device_management(self) -> None:
        """安全测试：集群 Token 轮转吊销旧密钥、设备禁用/下线踢出与注销删除."""

        async def _run() -> None:
            old_token = self.hub.config.shared_auth_token

            # 1. 使用旧 Token 注册 node_1 -> 成功
            dev1 = await self.hub.register_ws_client(
                "node_1", "Node-1", "macos", "192.168.1.50:8443", auth_token=old_token
            )
            self.assertTrue(dev1.online)
            self.assertTrue(dev1.enabled)

            # 2. 轮转集群密钥
            rotation_res = self.hub.rotate_cluster_token()
            new_token = rotation_res["token"]
            self.assertNotEqual(old_token, new_token)

            # 3. 再次使用旧 Token 注册 node_2 -> 必须被拒绝 (AUTH_FAILED)
            with self.assertRaises(PermissionDeniedError):
                await self.hub.register_ws_client(
                    "node_2", "Node-2", "windows", "192.168.1.51:8443", auth_token=old_token
                )

            # 4. 使用新 Token 注册 node_2 -> 成功
            dev2 = await self.hub.register_ws_client(
                "node_2", "Node-2", "windows", "192.168.1.51:8443", auth_token=new_token
            )
            self.assertTrue(dev2.online)

            # 5. 管理员禁用 node_1 -> 立即强制断开会话
            await self.hub.disable_device("node_1")
            self.assertFalse(dev1.online)
            self.assertFalse(dev1.enabled)

            # 6. 被禁用的 node_1 尝试重连 (即便携带新 Token) -> 必须被拒绝 (DEVICE_DISABLED)
            with self.assertRaises(PermissionDeniedError) as cm:
                await self.hub.register_ws_client(
                    "node_1", "Node-1", "macos", "192.168.1.50:8443", auth_token=new_token
                )
            self.assertIn("Device is disabled", str(cm.exception))

            # 7. 重新启用 node_1 -> 恢复准入
            self.hub.enable_device("node_1")
            dev1_re = await self.hub.register_ws_client(
                "node_1", "Node-1", "macos", "192.168.1.50:8443", auth_token=new_token
            )
            self.assertTrue(dev1_re.online)
            self.assertTrue(dev1_re.enabled)

            # 8. 踢出在线连接 (KICK) -> 迫使离线
            kicked = await self.hub.kick_device("node_1")
            self.assertTrue(kicked)
            self.assertFalse(dev1_re.online)

            # 9. 彻底注销并删除设备 (REMOVE) -> 从台账完全清除
            removed = await self.hub.remove_device("node_1")
            self.assertTrue(removed)
            device_ids = [d["device_id"] for d in self.hub.devices_api.list_devices()]
            self.assertNotIn("node_1", device_ids)

        asyncio.run(_run())

    def test_topic_acl_cross_device_blocked(self) -> None:
        """安全测试：Topic 级 ACL 访问控制，严禁越权订阅他人私有 RPC 与定向同步频道."""

        async def _run() -> None:
            token = self.hub.config.shared_auth_token

            # 1. 注册时企图订阅他人私有 RPC 频道 `topic.rpc.fs.node_victim` -> 拒绝
            with self.assertRaises(PermissionDeniedError) as cm:
                await self.hub.register_ws_client(
                    "node_attacker",
                    "Attacker",
                    "linux",
                    "10.0.0.1:8443",
                    auth_token=token,
                    extra_topics=["topic.rpc.fs.node_victim"],
                )
            self.assertIn("not authorized to subscribe", str(cm.exception))

            # 2. 正常注册后，尝试动态订阅他人定向同步频道 `topic.sync.direct.node_victim` -> 拒绝
            attacker = await self.hub.register_ws_client(
                "node_attacker",
                "Attacker",
                "linux",
                "10.0.0.1:8443",
                auth_token=token,
            )
            with self.assertRaises(PermissionDeniedError) as cm:
                self.hub.sub_manager.update_subscription(
                    "node_attacker",
                    subscribe_topics=["topic.sync.direct.node_victim"],
                )
            self.assertIn("not authorized to subscribe", str(cm.exception))

            # 3. 订阅自己本机的私有频道 -> 允许
            self.hub.sub_manager.update_subscription(
                "node_attacker",
                subscribe_topics=["topic.sync.direct.node_attacker"],
            )
            self.assertIn("topic.sync.direct.node_attacker", attacker.subscribed_topics)

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

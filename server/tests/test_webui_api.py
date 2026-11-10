"""Cloud WebUI 控制台、双 Token 平面隔离、24H 缓存清理与 SQLite 状态持久化测试 (TASK-11 ~ TASK-15)."""

from __future__ import annotations

import asyncio
import importlib.util
import shutil
import tempfile
import time
import unittest
from pathlib import Path
from typing import List

from server.app.api.auth import ADMIN_SESSION_COOKIE_NAME
from server.app.client_sub_manager import PermissionDeniedError
from server.app.config import DEFAULT_CLUSTER_TOKEN, ServerConfig
from server.app.main import _HAS_FASTAPI, CloudHubService, create_fastapi_app
from server.app.models import (
    SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST,
    CloudEventEnvelope,
    DigestManifest,
    EventPayload,
    SourceDevice,
    TargetScope,
    compute_blake3_hex,
)

_HAS_HTTPX = importlib.util.find_spec("httpx") is not None


def _make_isolated_hub(
    host: str = "0.0.0.0",
    cluster_token: str = "cluster-secret-2026",
    admin_token: str | None = "admin-secret-2026",
    cache_ttl_seconds: int = 24 * 3600,
) -> tuple[CloudHubService, Path]:
    tmp_dir = Path(tempfile.mkdtemp(prefix="mi_webui_test_"))
    cfg = ServerConfig(
        host=host,
        port=8000,
        data_dir=tmp_dir,
        shared_auth_token=cluster_token,
        admin_token=admin_token,
        cache_ttl_seconds=cache_ttl_seconds,
    )
    return CloudHubService(config=cfg), tmp_dir


class TestCloudWebUIGovernance(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp_dirs: List[Path] = []
        self.hubs: List[CloudHubService] = []

    def tearDown(self) -> None:
        for hub in self.hubs:
            try:
                hub.close()
            except Exception:
                pass
        for d in self.tmp_dirs:
            shutil.rmtree(d, ignore_errors=True)

    def _track(self, hub: CloudHubService, tmp_dir: Path) -> CloudHubService:
        self.hubs.append(hub)
        self.tmp_dirs.append(tmp_dir)
        return hub

    def test_mandatory_auth_dual_token_plane_isolation_and_fallback_warnings(self) -> None:
        """验证非 127.0.0.1 强制鉴权、MI_ADMIN_TOKEN 与 MI_CLUSTER_TOKEN 双向隔离、及兜底安全告警."""
        hub = self._track(*_make_isolated_hub(host="0.0.0.0"))

        # 1. 未提供任何凭证必须被拒绝 (401)
        ok, code, _ = hub.auth_manager.authorize_request()
        self.assertFalse(ok)
        self.assertEqual(code, 401)

        # 2. 使用错误凭据登录返回 401 并记入审计日志
        status_bad, body_bad, tok_bad = hub.auth_manager.login("wrong-token", client_host="192.168.10.22")
        self.assertEqual(status_bad, 401)
        self.assertFalse(body_bad["authenticated"])
        self.assertIsNone(tok_bad)

        # 3. 尝试使用数据面 MI_CLUSTER_TOKEN 登录管理控制台或 Bearer 请求 -> 严格返回 403 Forbidden
        status_cross, body_cross, tok_cross = hub.auth_manager.login(
            "cluster-secret-2026", client_host="192.168.10.22"
        )
        self.assertEqual(status_cross, 403)
        self.assertFalse(body_cross["authenticated"])
        self.assertIn("MI_CLUSTER_TOKEN", body_cross["detail"])
        self.assertIsNone(tok_cross)

        ok_hdr, code_hdr, _ = hub.auth_manager.authorize_request(authorization_header="Bearer cluster-secret-2026")
        self.assertFalse(ok_hdr)
        self.assertEqual(code_hdr, 403)

        # 4. 控制面 MI_ADMIN_TOKEN 不可用于客户端数据面设备接入
        self.assertFalse(hub.sub_manager.verify_token("admin-secret-2026"))
        self.assertTrue(hub.sub_manager.verify_token("cluster-secret-2026"))

        # 5. 使用正确 MI_ADMIN_TOKEN 登录成功并签发可校验 Session Cookie
        status_ok, body_ok, session_tok = hub.auth_manager.login("admin-secret-2026", client_host="192.168.10.22")
        self.assertEqual(status_ok, 200)
        self.assertTrue(body_ok["authenticated"])
        self.assertTrue(body_ok["admin_token_is_separate"])
        self.assertEqual(body_ok["security_warnings"], [])
        self.assertIsNotNone(session_tok)
        self.assertEqual(hub.auth_manager.authorize_request(session_cookie=session_tok), (True, 200, "OK"))

        # 6. 登出立即吊销 JTI
        hub.auth_manager.logout(session_tok)
        ok_after, code_after, _ = hub.auth_manager.authorize_request(session_cookie=session_tok)
        self.assertFalse(ok_after)
        self.assertEqual(code_after, 401)

        # 7. 未配置 MI_ADMIN_TOKEN + 使用默认 MI_CLUSTER_TOKEN 且监听 0.0.0.0 -> 兜底允许并触发 HIGH 告警
        hub_fb = self._track(
            *_make_isolated_hub(
                host="0.0.0.0",
                cluster_token=DEFAULT_CLUSTER_TOKEN,
                admin_token=None,
            )
        )
        warns = {w["code"]: w for w in hub_fb.auth_manager.get_security_warnings()}
        self.assertIn("DEFAULT_CLUSTER_TOKEN", warns)
        self.assertIn("ADMIN_TOKEN_NOT_ISOLATED", warns)
        self.assertEqual(warns["DEFAULT_CLUSTER_TOKEN"]["severity"], "HIGH")
        self.assertEqual(warns["ADMIN_TOKEN_NOT_ISOLATED"]["severity"], "HIGH")
        s_fb, b_fb, c_fb = hub_fb.auth_manager.login(DEFAULT_CLUSTER_TOKEN, client_host="127.0.0.1")
        self.assertEqual(s_fb, 200)
        self.assertFalse(b_fb["admin_token_is_separate"])
        self.assertIsNotNone(c_fb)

    def test_cluster_token_rotation_preserves_admin_session_and_revokes_device_tokens(self) -> None:
        """验证轮转 MI_CLUSTER_TOKEN 立即吊销旧集群与设备 Token，但不注销管理员 WebUI 会话."""
        hub = self._track(*_make_isolated_hub())

        _, _, admin_cookie = hub.auth_manager.login("admin-secret-2026", client_host="10.0.0.5")
        self.assertIsNotNone(admin_cookie)

        issued = hub.devices_api.issue_token(label="Workstation-Mac", expected_os="macos")
        dev_tok = issued["token"]
        self.assertTrue(hub.sub_manager.verify_token(dev_tok))
        self.assertTrue(hub.sub_manager.verify_token("cluster-secret-2026"))

        # 单独吊销某个设备 Token
        issued_extra = hub.devices_api.issue_token(label="Temp-Laptop", expected_os="windows")
        self.assertTrue(hub.sub_manager.verify_token(issued_extra["token"]))
        hub.devices_api.revoke_token(issued_extra["token_id"])
        self.assertFalse(hub.sub_manager.verify_token(issued_extra["token"]))

        # 执行主集群 Token 轮转
        rotated = hub.rotate_cluster_token("rotated-cluster-999")
        self.assertEqual(rotated["token"], "rotated-cluster-999")
        self.assertFalse(hub.sub_manager.verify_token("cluster-secret-2026"))
        self.assertFalse(hub.sub_manager.verify_token(dev_tok))
        self.assertTrue(hub.sub_manager.verify_token("rotated-cluster-999"))

        # 管理员现有 Cookie 会话保持有效，未因轮转掉线
        self.assertEqual(hub.auth_manager.authorize_request(session_cookie=admin_cookie), (True, 200, "OK"))

        # 脱敏安全视图校验
        masked_view = hub.devices_api.list_security_tokens(reveal_cluster_token=False)
        self.assertNotIn("plaintext", masked_view["cluster_token"])
        self.assertIn("••••", masked_view["cluster_token"]["masked"])
        plain_view = hub.devices_api.list_security_tokens(reveal_cluster_token=True)
        self.assertEqual(plain_view["cluster_token"]["plaintext"], "rotated-cluster-999")

    def test_state_store_persistence_acl_and_live_kick_across_restart(self) -> None:
        """验证设备台账、禁用状态、内容分类 ACL、轮转密钥及专属 Token 在重启后完整恢复与生效."""
        tmp_dir = Path(tempfile.mkdtemp(prefix="mi_persist_test_"))
        self.tmp_dirs.append(tmp_dir)

        cfg1 = ServerConfig(
            host="0.0.0.0",
            data_dir=tmp_dir,
            shared_auth_token="init-cluster",
            admin_token="init-admin",
        )
        hub1 = CloudHubService(config=cfg1)

        async def _setup_hub1() -> None:
            kicked_reasons: List[str] = []
            await hub1.register_ws_client("dev_a", "Dev-A", "windows", "192.168.1.10:8443", auth_token="init-cluster")
            await hub1.register_ws_client("dev_b", "Dev-B", "macos", "192.168.1.11:8443", auth_token="init-cluster")
            hub1.sub_manager.bind_session_closer("dev_a", kicked_reasons.append)

            # 踢出触发 closer 回调
            await hub1.kick_device("dev_a")
            self.assertEqual(kicked_reasons, ["Device kicked by administrator"])

            # 禁用 dev_a，限制 dev_b 仅接收 TEXT 类型事件，轮转密钥并签发专属 Token
            await hub1.disable_device("dev_a")
            hub1.devices_api.update_device_acl("dev_b", allowed_categories=["TEXT"])
            hub1.rotate_cluster_token("persisted-cluster-888")
            hub1.devices_api.issue_token(token="persisted-dev-tok-777", label="BuildNode", expected_os="linux")

        asyncio.run(_setup_hub1())
        hub1.close()

        # 模拟服务进程重启（使用相同的初始 env shared_auth_token="init-cluster"）
        cfg2 = ServerConfig(
            host="0.0.0.0",
            data_dir=tmp_dir,
            shared_auth_token="init-cluster",
            admin_token="init-admin",
        )
        hub2 = self._track(CloudHubService(config=cfg2), tmp_dir)

        # 1. 轮转密钥与签发设备 Token 持久化保留
        self.assertEqual(hub2.sub_manager.shared_auth_token, "persisted-cluster-888")
        self.assertFalse(hub2.sub_manager.verify_token("init-cluster"))
        self.assertTrue(hub2.sub_manager.verify_token("persisted-cluster-888"))
        self.assertTrue(hub2.sub_manager.verify_token("persisted-dev-tok-777"))

        # 2. dev_a 重启后保持 offline + disabled，严禁重连
        dev_a_restored = hub2.sub_manager.get_device("dev_a")
        self.assertIsNotNone(dev_a_restored)
        assert dev_a_restored is not None
        self.assertFalse(dev_a_restored.online)
        self.assertFalse(dev_a_restored.enabled)

        async def _verify_hub2_routing() -> None:
            with self.assertRaises(PermissionDeniedError):
                await hub2.register_ws_client(
                    "dev_a", "Dev-A", "windows", "192.168.1.10:8443", auth_token="persisted-cluster-888"
                )

            dev_b = await hub2.register_ws_client(
                "dev_b", "Dev-B", "macos", "192.168.1.11:8443", auth_token="persisted-cluster-888"
            )
            await hub2.register_ws_client(
                "dev_sender", "Sender", "linux", "192.168.1.12:8443", auth_token="persisted-dev-tok-777"
            )
            while not dev_b.inbox.empty():
                await dev_b.inbox.get()

            # FILE 事件被 dev_b 的持久化 ACL ["TEXT"] 拦截
            file_env = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_acl_file",
                topic=TOPIC_CLIPBOARD_BROADCAST,
                event_type="EVENT_CLIPBOARD_BROADCAST",
                timestamp_ms=int(time.time() * 1000),
                source_device=SourceDevice("dev_sender", "Sender", "linux", "192.168.1.12:8443"),
                target_scope=TargetScope(mode="BROADCAST", target_device_ids=["*"]),
                payload=EventPayload(
                    content_category="FILE",
                    digest=DigestManifest.from_bytes(b"binary-file"),
                    inline_data=None,
                    file_cache_id="fc_acl_file_01",
                    upload_state="COMPLETED",
                ),
            )
            delivered_file = await hub2.publish_event(file_env)
            self.assertEqual(delivered_file, [])
            self.assertTrue(dev_b.inbox.empty())

            # TEXT 事件放行送达 dev_b
            text_env = CloudEventEnvelope(
                spec_version=SPEC_VERSION_V6,
                event_id="evt_acl_text",
                topic=TOPIC_CLIPBOARD_BROADCAST,
                event_type="EVENT_CLIPBOARD_BROADCAST",
                timestamp_ms=int(time.time() * 1000),
                source_device=SourceDevice("dev_sender", "Sender", "linux", "192.168.1.12:8443"),
                target_scope=TargetScope(mode="BROADCAST", target_device_ids=["*"]),
                payload=EventPayload(
                    content_category="TEXT",
                    digest=DigestManifest.from_bytes(b"hello-acl"),
                    inline_data="hello-acl",
                    file_cache_id=None,
                    upload_state="INLINE",
                ),
            )
            delivered_text = await hub2.publish_event(text_env)
            self.assertEqual(delivered_text, ["dev_b"])
            msg = await asyncio.wait_for(dev_b.inbox.get(), timeout=1.0)
            self.assertEqual(msg["event_id"], "evt_acl_text")

        asyncio.run(_verify_hub2_routing())

    def test_strict_24h_ttl_non_touching_reads_readers_and_orphan_prune(self) -> None:
        """验证只读探测不续期、活跃消费者追踪、严格 24H TTL 超时清理与孤立 .part 碎片清扫."""
        hub = self._track(*_make_isolated_hub(cache_ttl_seconds=86400))

        async def _run() -> None:
            chunk_size = 1024
            payload = b"A" * chunk_size + b"B" * chunk_size
            digest = DigestManifest.from_bytes(payload, chunk_size_bytes=chunk_size)

            await hub.cache_api.init_cache(
                file_cache_id="fc_24h_01",
                source_device_id="node_src",
                total_size=len(payload),
                digest_dict=digest.to_dict(),
                file_name="project.tar.zst",
                content_category="DIRECTORY",
            )
            await hub.cache_api.patch_cache("fc_24h_01", 0, payload[:chunk_size], compute_blake3_hex(payload[:chunk_size]))

            # 1. 只读操作 head/list/detail 不得刷新 last_accessed_at（避免轮询续期导致 24H 无法失效）
            entry = hub.stream_cache.get_entry("fc_24h_01")
            past_ts = entry.created_at - 100.0
            entry.last_accessed_at = past_ts
            _ = hub.cache_api.head_cache("fc_24h_01")
            _ = hub.cache_api.list_sessions()
            detail = hub.cache_api.get_session_detail("fc_24h_01")
            self.assertEqual(entry.last_accessed_at, past_ts)
            self.assertEqual(detail["file_name"], "project.tar.zst")
            self.assertEqual(detail["content_category"], "DIRECTORY")

            # 2. 活跃边传边下消费者追踪 (ReaderProgress)
            observed_readers: List[dict] = []
            read_started = asyncio.Event()
            chunks_read: List[bytes] = []

            async def _consumer() -> None:
                async for part in hub.stream_cache.stream_range(
                    "fc_24h_01", start_offset=0, read_chunk_size=512, reader_device_id="node_consumer_1"
                ):
                    chunks_read.append(part)
                    if not read_started.is_set():
                        observed_readers.extend(hub.cache_api.get_session_detail("fc_24h_01")["active_readers"])
                        read_started.set()

            reader_task = asyncio.create_task(_consumer())
            await asyncio.wait_for(read_started.wait(), timeout=2.0)
            self.assertEqual(len(observed_readers), 1)
            self.assertEqual(observed_readers[0]["reader_device_id"], "node_consumer_1")

            await hub.cache_api.patch_cache(
                "fc_24h_01", chunk_size, payload[chunk_size:], compute_blake3_hex(payload[chunk_size:])
            )
            await asyncio.wait_for(reader_task, timeout=2.0)
            self.assertEqual(b"".join(chunks_read), payload)
            self.assertEqual(entry.download_count, 1)
            self.assertEqual(entry.active_readers, {})

            # 3. 写入孤立 .part 碎片与第二个归档会话，验证 prune(orphan_parts_only=True) 不误删有效归档
            orphan_part = hub.stream_cache.cache_dir / "ghost_abandoned.part"
            orphan_part.write_bytes(b"1234567890")
            prune_res = hub.cache_api.prune_cache(orphan_parts_only=True)
            self.assertEqual(prune_res["orphan_files_removed"], 1)
            self.assertGreaterEqual(prune_res["freed_bytes"], 10)
            self.assertFalse(orphan_part.exists())
            self.assertEqual(hub.cache_api.head_cache("fc_24h_01")["Cache-State"], "COMPLETED")

            # 4. 严格 24H (86400s) 超时后台清理触发 -> 彻底清空实体文件与会话
            tick_before = hub.run_maintenance_tick(now_time=entry.created_at + 3600)
            self.assertEqual(tick_before["cache_removed"], 0)
            tick_expired = hub.run_maintenance_tick(now_time=entry.created_at + 86401)
            self.assertGreaterEqual(tick_expired["cache_removed"], 1)
            self.assertEqual(hub.cache_api.list_sessions()["sessions"], [])
            self.assertFalse((hub.stream_cache.cache_dir / "fc_24h_01.cache").exists())
            self.assertFalse((hub.stream_cache.cache_dir / "fc_24h_01.meta.json").exists())

        asyncio.run(_run())

    def test_audit_search_export_wal_checkpoint_and_overview_topology(self) -> None:
        """验证多维审计检索/导出、WAL 检查点及总览拓扑子网聚合 (不虚构客户端 180s 计时)."""
        hub = self._track(*_make_isolated_hub())

        async def _run() -> None:
            await hub.register_ws_client("win_1", "Win-1", "windows", "192.168.50.10:8443", auth_token="cluster-secret-2026")
            await hub.register_ws_client("mac_1", "Mac-1", "macos", "192.168.50.22:8443", auth_token="cluster-secret-2026")
            await hub.register_ws_client("v6_node", "V6-Node", "linux", "[2001:db8::10]:8443", auth_token="cluster-secret-2026")

            hub.audit.log_transfer(
                transfer_id="tr_lan_1",
                file_cache_id="fc_lan_1",
                source_device_id="win_1",
                target_device_id="mac_1",
                channel_used="LAN_DIRECT",
                bytes_transferred=2048,
                verification_status="VERIFIED_OK",
                content_hash="abcd1234",
                resumed=False,
            )
            hub.audit.log_transfer(
                transfer_id="tr_cloud_err",
                file_cache_id="fc_cloud_err",
                source_device_id="win_1",
                target_device_id="v6_node",
                channel_used="CLOUD_CACHE",
                bytes_transferred=4096,
                verification_status="HASH_MISMATCH",
                content_hash="deadbeef",
                resumed=True,
                resume_offset=1024,
                resume_count=2,
            )

            # 1. 多维筛选与分页
            failed_res = hub.audit_api.query_logs_paged(log_type="transfer", status="FAILED")
            self.assertEqual(failed_res["total"], 1)
            self.assertEqual(failed_res["items"][0]["transfer_id"], "tr_cloud_err")

            resumed_res = hub.audit_api.query_logs_paged(status="RESUMED", keyword="deadbeef")
            self.assertEqual(resumed_res["total"], 1)
            self.assertTrue(resumed_res["items"][0]["resumed"])

            conn_paged = hub.audit_api.query_logs_paged(log_type="connection", limit=1, offset=1)
            self.assertEqual(len(conn_paged["items"]), 1)
            self.assertEqual(conn_paged["total_counts"]["connection"], 3)

            # 2. JSON 导出文档
            exported = hub.audit_api.export_logs(log_type="transfer")
            self.assertEqual(exported["total"], 2)
            self.assertEqual(exported["summary"]["lan_transfer_count"], 1)
            self.assertEqual(exported["summary"]["cloud_transfer_count"], 1)

            # 3. WAL Checkpoint 与 Overview 子网/通道聚合适配
            ckpt = hub.system_api.wal_checkpoint()
            self.assertEqual(ckpt["checkpoint_mode"], "TRUNCATE")
            self.assertEqual(ckpt["journal_mode"], "WAL")

            ov = hub.system_api.get_overview()
            self.assertEqual(ov["device_stats"]["online"], 3)
            self.assertEqual(sorted(ov["lan_subnets"]["192.168.50.0/24"]), ["mac_1", "win_1"])
            self.assertEqual(ov["lan_subnets"]["2001:db8::/64"], ["v6_node"])
            self.assertEqual(len(ov["channel_activity"]), 2)
            self.assertEqual(ov["system"]["lan_idle_timeout_seconds"], 180)

        asyncio.run(_run())

    def test_webui_static_bundle_present_and_offline_zero_cdn(self) -> None:
        """验证预编译 WebUI 静态产物存在且 100% 离线零外部 CDN 引用."""
        static_dir = Path(__file__).resolve().parents[1] / "app" / "static"
        index_html = static_dir / "index.html"
        assets_dir = static_dir / "assets"

        self.assertTrue(index_html.is_file(), f"Missing built WebUI index.html at {index_html}")
        html_text = index_html.read_text(encoding="utf-8")
        self.assertIn("multinteractor", html_text)
        self.assertIn("/static/assets/", html_text)

        js_files = list(assets_dir.glob("*.js"))
        css_files = list(assets_dir.glob("*.css"))
        self.assertGreater(len(js_files), 0, "Expected compiled JS bundle in server/app/static/assets")
        self.assertGreater(len(css_files), 0, "Expected compiled CSS bundle in server/app/static/assets")

        for asset in [index_html, *js_files, *css_files]:
            text = asset.read_text(encoding="utf-8")
            self.assertNotIn("https://", text, f"Unexpected external HTTPS URL in {asset.name}")
            self.assertNotIn("http://", text, f"Unexpected external HTTP URL in {asset.name}")


@unittest.skipUnless(_HAS_FASTAPI and _HAS_HTTPX, "FastAPI and httpx required for ASGI HTTP/WS integration tests")
class TestFastAPIWebUIEndToEnd(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = _make_isolated_hub(host="0.0.0.0")
        self.app = create_fastapi_app(config=self.hub.config, hub_service=self.hub)

    def tearDown(self) -> None:
        self.hub.close()
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def test_http_auth_dual_token_rotation_and_websockets(self) -> None:
        from fastapi.testclient import TestClient

        with TestClient(self.app) as client:
            # Health & Static UI
            self.assertEqual(client.get("/healthz").json(), {"status": "ok"})
            ui_resp = client.get("/ui")
            self.assertEqual(ui_resp.status_code, 200)
            self.assertIn("multinteractor", ui_resp.text)

            # Unauthenticated REST blocked with 401
            self.assertEqual(client.get("/api/v1/overview").status_code, 401)

            # Cluster token denied on control plane with 403
            r_cross = client.post("/api/v1/auth/login", json={"token": "cluster-secret-2026"})
            self.assertEqual(r_cross.status_code, 403)

            # Admin token succeeds and sets HttpOnly cookie
            r_login = client.post("/api/v1/auth/login", json={"token": "admin-secret-2026"})
            self.assertEqual(r_login.status_code, 200)
            self.assertIn(ADMIN_SESSION_COOKIE_NAME, r_login.cookies)

            # Overview & rotate keep cookie session alive
            self.assertEqual(client.get("/api/v1/overview").status_code, 200)
            r_rot = client.post("/api/v1/devices/token/rotate", json={"new_token": "cluster-next-777"})
            self.assertEqual(r_rot.status_code, 200)
            self.assertEqual(client.get("/api/v1/overview").status_code, 200)

            # /ws/signaling rejects admin token, accepts new cluster token
            with client.websocket_connect("/ws/signaling") as ws_bad:
                ws_bad.send_json(
                    {
                        "device_id": "bad_dev",
                        "device_name": "Bad",
                        "os": "linux",
                        "lan_endpoint": "10.0.0.2:8443",
                        "auth_token": "admin-secret-2026",
                    }
                )
                err_frame = ws_bad.receive_json()
                self.assertEqual(err_frame["type"], "ERROR")

            with client.websocket_connect("/ws/signaling") as ws_ok:
                ws_ok.send_json(
                    {
                        "device_id": "good_dev",
                        "device_name": "Good",
                        "os": "macos",
                        "lan_endpoint": "192.168.1.50:8443",
                        "auth_token": "cluster-next-777",
                    }
                )
                reg = ws_ok.receive_json()
                self.assertEqual(reg["type"], "REGISTERED")

            # Authenticated /ws/admin pushes initial telemetry snapshot
            with client.websocket_connect("/ws/admin") as ws_admin:
                snap = ws_admin.receive_json()
                self.assertEqual(snap["type"], "telemetry")
                self.assertEqual(snap["event"], "snapshot")

            # Logout invalidates session cookie
            self.assertEqual(client.post("/api/v1/auth/logout").status_code, 200)
            self.assertEqual(client.get("/api/v1/overview").status_code, 401)


if __name__ == "__main__":
    unittest.main()

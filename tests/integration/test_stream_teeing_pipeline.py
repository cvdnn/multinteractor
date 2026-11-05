"""L2 集成测试：云端“边传边下 (Stream Teeing)”实时分流与源设备离线异步拉取验证 (TC-RES-01 ~ TC-RES-02)."""

from __future__ import annotations

import asyncio
import time
import unittest

from server.app.models import DigestManifest, compute_blake3_hex
from server.tests.conftest import cleanup_test_hub, make_test_hub


class TestStreamTeeingPipeline(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)

    def test_tc_res_01_stream_teeing_at_4_percent_upload(self) -> None:
        """TC-RES-01: Node-A 刚上传至 4% 时 Node-C 按下 Alt+V，100ms 内收到首字节并跟随水位线完成下载."""

        async def _run() -> None:
            chunk_size = 2048
            total_chunks = 25  # 1 块即为 4%
            payload = b"".join(bytes([(i * 17) % 256]) * chunk_size for i in range(total_chunks))
            digest = DigestManifest.from_bytes(payload, chunk_size_bytes=chunk_size)
            cid = "fc_tc_res_01"

            await self.hub.cache_api.init_cache(cid, "node_a", len(payload), digest.to_dict())

            # Node-A 先上传第 0 块 (4%)
            first_chunk = payload[:chunk_size]
            await self.hub.cache_api.patch_cache(cid, 0, first_chunk, compute_blake3_hex(first_chunk))

            first_byte_ms = None
            downloaded = bytearray()

            async def _node_c_alt_v() -> None:
                nonlocal first_byte_ms
                t0 = time.perf_counter()
                async for data in self.hub.stream_cache.stream_range(cid, start_offset=0):
                    if first_byte_ms is None:
                        first_byte_ms = (time.perf_counter() - t0) * 1000.0
                    downloaded.extend(data)

            async def _node_a_continue_upload() -> None:
                for idx in range(1, total_chunks):
                    await asyncio.sleep(0.005)
                    off = idx * chunk_size
                    blk = payload[off : off + chunk_size]
                    await self.hub.cache_api.patch_cache(cid, off, blk, compute_blake3_hex(blk))

            await asyncio.gather(_node_c_alt_v(), _node_a_continue_upload())

            self.assertIsNotNone(first_byte_ms)
            self.assertLessEqual(first_byte_ms, 100.0)
            self.assertEqual(bytes(downloaded), payload)
            digest.verify_bytes(bytes(downloaded))

        asyncio.run(_run())

    def test_tc_res_02_async_pull_after_source_device_offline(self) -> None:
        """TC-RES-02: 源设备 Node-B 上传完毕后直接关机下线，Node-C 仍能从云端缓存池 100% 拉取并校验摘要."""

        async def _run() -> None:
            await self.hub.register_ws_client("node_b", "Node-B", "windows", "192.168.1.11:8443")
            await self.hub.register_ws_client("node_c", "Node-C", "windows", "192.168.1.12:8443")

            docx_bytes = b"PK\x03\x04-AI-Tutorial-Docx-Payload-" * 200
            digest = DigestManifest.from_bytes(docx_bytes)
            cid = "fc_tc_res_02_offline"

            await self.hub.cache_api.init_cache(cid, "node_b", len(docx_bytes), digest.to_dict())
            await self.hub.cache_api.patch_cache(cid, 0, docx_bytes, digest.full_hash)

            # 源设备 Node-B 彻底下线/关机
            await self.hub.disconnect_ws_client("node_b")
            self.assertFalse(self.hub.sub_manager.get_device("node_b").online)

            # Node-C 异步拉取
            status, headers, fetched = await self.hub.cache_api.get_stream_bytes(cid)
            self.assertEqual(status, 200)
            self.assertEqual(fetched, docx_bytes)
            digest.verify_bytes(fetched)
            self.assertEqual(headers["X-Full-Blake3"], digest.full_hash)

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

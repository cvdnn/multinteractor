"""边传边下 (Stream Teeing)、分块 BLAKE3 校验与断点续传缓存单元测试 (对应 TASK-04, TC-RES-01 ~ TC-RES-05)."""

from __future__ import annotations

import asyncio
import time
import unittest

from server.app.models import DigestManifest, DigestMismatchError, compute_blake3_hex
from server.tests.conftest import cleanup_test_hub, make_test_hub


class TestStreamTeeCache(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)

    def test_stream_teeing_concurrent_upload_and_download(self) -> None:
        """准出测试：源端上传至 10% 时目标端发起拉取，目标端 <100ms 收到首字节并实时边传边下至 100%."""

        async def _run() -> None:
            chunk_size = 4096
            total_chunks = 10
            full_payload = b"".join(
                f"chunk-{i:02d}-".encode("utf-8") + bytes([i % 251]) * (chunk_size - 9)
                for i in range(total_chunks)
            )
            digest = DigestManifest.from_bytes(full_payload, chunk_size_bytes=chunk_size)
            cache_id = "fc_tee_test_01"

            await self.hub.stream_cache.create_session(
                file_cache_id=cache_id,
                source_device_id="node_a",
                total_size=len(full_payload),
                digest=digest,
            )

            # 源端先上传第 0 块 (10%)
            c0 = full_payload[:chunk_size]
            await self.hub.cache_api.patch_cache(
                file_cache_id=cache_id,
                upload_offset=0,
                chunk_data=c0,
                chunk_blake3=compute_blake3_hex(c0),
            )
            head = self.hub.cache_api.head_cache(cache_id)
            self.assertEqual(head["Upload-Offset"], chunk_size)
            self.assertEqual(head["Cache-State"], "STREAMING")

            first_byte_latency_ms = None
            received_chunks = []

            async def _reader() -> None:
                nonlocal first_byte_latency_ms
                t0 = time.perf_counter()
                async for part in self.hub.stream_cache.stream_range(cache_id, start_offset=0):
                    if first_byte_latency_ms is None:
                        first_byte_latency_ms = (time.perf_counter() - t0) * 1000.0
                    received_chunks.append(part)

            async def _writer() -> None:
                for idx in range(1, total_chunks):
                    await asyncio.sleep(0.01)
                    offset = idx * chunk_size
                    part = full_payload[offset : offset + chunk_size]
                    await self.hub.cache_api.patch_cache(
                        file_cache_id=cache_id,
                        upload_offset=offset,
                        chunk_data=part,
                        chunk_blake3=compute_blake3_hex(part),
                    )

            await asyncio.gather(_reader(), _writer())

            self.assertIsNotNone(first_byte_latency_ms)
            self.assertLessEqual(first_byte_latency_ms, 100.0)
            assembled = b"".join(received_chunks)
            self.assertEqual(assembled, full_payload)
            digest.verify_bytes(assembled)
            self.assertEqual(self.hub.cache_api.head_cache(cache_id)["Cache-State"], "COMPLETED")

        asyncio.run(_run())

    def test_resumable_upload_range_download_and_corrupted_chunk_rejection(self) -> None:
        """验证 Upload-Offset 断点查询续传、HTTP Range 断点下载与篡改分块拦截."""

        async def _run() -> None:
            chunk_size = 2048
            data = b"A" * chunk_size + b"B" * chunk_size + b"C" * chunk_size
            digest = DigestManifest.from_bytes(data, chunk_size_bytes=chunk_size)
            cid = "fc_resume_02"

            await self.hub.cache_api.init_cache(cid, "node_b", len(data), digest.to_dict())

            # 上传前两块
            for i in range(2):
                off = i * chunk_size
                blk = data[off : off + chunk_size]
                await self.hub.cache_api.patch_cache(cid, off, blk, compute_blake3_hex(blk))

            # 模拟断网重连：先通过 HEAD 查询服务端已落盘偏移量
            head = self.hub.cache_api.head_cache(cid)
            self.assertEqual(head["Upload-Offset"], 2 * chunk_size)

            # 故意发送比特翻转损坏的第 3 块 -> 服务端强制拒绝写入且不推进偏移量
            corrupted_blk3 = b"X" + data[2 * chunk_size + 1 :]
            with self.assertRaises(DigestMismatchError):
                await self.hub.cache_api.patch_cache(cid, 2 * chunk_size, corrupted_blk3)
            self.assertEqual(self.hub.cache_api.head_cache(cid)["Upload-Offset"], 2 * chunk_size)

            # 重传正确的第 3 块 -> 完成上传并全文 BLAKE3 校验转正
            valid_blk3 = data[2 * chunk_size :]
            await self.hub.cache_api.patch_cache(cid, 2 * chunk_size, valid_blk3, compute_blake3_hex(valid_blk3))
            self.assertEqual(self.hub.cache_api.head_cache(cid)["Cache-State"], "COMPLETED")

            # 模拟目标端携带 Range: bytes=2048- 从断点拉取剩余 4096 字节
            status, headers, body = await self.hub.cache_api.get_stream_bytes(
                cid, range_header=f"bytes={chunk_size}-"
            )
            self.assertEqual(status, 206)
            self.assertEqual(body, data[chunk_size:])
            self.assertEqual(headers["X-Full-Blake3"], digest.full_hash)

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

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

    def test_cross_device_instant_upload_disabled_and_same_device_dedup(self) -> None:
        """安全测试：验证跨设备秒传已被严格关闭，严禁其他设备凭借已知哈希空手套白狼窃取缓存."""

        async def _run() -> None:
            data = b"CONFIDENTIAL_USER_DATA_4096_BYTES" * 128
            digest = DigestManifest.from_bytes(data)
            cid_a = "fc_device_a_file"
            cid_b = "fc_device_b_steal_attempt"
            cid_a2 = "fc_device_a_retry_file"

            # 1. 设备 A 正常上传完整文件至 COMPLETED
            await self.hub.cache_api.init_cache(cid_a, "device_a", len(data), digest.to_dict())
            await self.hub.cache_api.patch_cache(cid_a, 0, data, digest.full_hash)
            head_a = self.hub.cache_api.head_cache(cid_a)
            self.assertEqual(head_a["Cache-State"], "COMPLETED")

            # 2. 设备 B 尝试凭借相同哈希与大小发起会话，意图“跨设备秒传”偷读
            #    断言：跨设备秒传已关闭，设备 B 拿到的会话状态必须是 STREAMING，必须上传实际数据
            entry_b = await self.hub.stream_cache.create_session(
                file_cache_id=cid_b,
                source_device_id="device_b",
                total_size=len(data),
                digest=digest,
            )
            self.assertEqual(entry_b.state, "STREAMING")
            self.assertEqual(entry_b.written_offset, 0)
            self.assertEqual(self.hub.cache_api.head_cache(cid_b)["Cache-State"], "STREAMING")

            # 3. 设备 A 本身重复上传相同哈希文件 (cid_a2) -> 允许同设备秒传
            entry_a2 = await self.hub.stream_cache.create_session(
                file_cache_id=cid_a2,
                source_device_id="device_a",
                total_size=len(data),
                digest=digest,
            )
            self.assertEqual(entry_a2.state, "COMPLETED")
            self.assertEqual(entry_a2.written_offset, len(data))

            # 4. 设备 B 试图使用相同的 session ID (cid_a) 抢占/修改设备 A 的会话 -> 必须拒绝
            with self.assertRaises(ValueError) as cm:
                await self.hub.stream_cache.create_session(
                    file_cache_id=cid_a,
                    source_device_id="device_b",
                    total_size=len(data),
                    digest=digest,
                )
            self.assertIn("already exists with different owner", str(cm.exception))

        asyncio.run(_run())

    def test_path_traversal_blocked(self) -> None:
        """安全测试：拦截包含路径穿越符、斜杠及非法字符的 file_cache_id."""

        async def _run() -> None:
            data = b"hello"
            digest = DigestManifest.from_bytes(data)

            invalid_ids = [
                "../escape",
                "../../etc/passwd",
                "foo/bar",
                "foo\\bar",
                "has space",
                "special*char",
                "",
                "a" * 65,  # 超过 64 字符
            ]
            for bad_id in invalid_ids:
                with self.assertRaises(ValueError):
                    await self.hub.stream_cache.create_session(
                        file_cache_id=bad_id,
                        source_device_id="node_a",
                        total_size=len(data),
                        digest=digest,
                    )

        asyncio.run(_run())

    def test_write_beyond_total_size_and_oversized_chunk_rejected(self) -> None:
        """安全测试：拦截超出 MAX_CHUNK_SIZE 或越界超过 total_size 的恶意写入."""

        async def _run() -> None:
            data = b"small_payload"
            digest = DigestManifest.from_bytes(data)
            cid = "fc_bounds_test_01"

            await self.hub.stream_cache.create_session(
                file_cache_id=cid,
                source_device_id="node_a",
                total_size=len(data),
                digest=digest,
            )

            # 1. 尝试写入超过 total_size 的超大数据
            overflow_data = b"X" * (len(data) + 10)
            with self.assertRaises(ValueError) as cm:
                await self.hub.cache_api.patch_cache(cid, 0, overflow_data)
            self.assertIn("exceeds total_size", str(cm.exception))

            # 2. 尝试单次写入超过 4MB 的巨型分块
            huge_data = b"H" * (5 * 1024 * 1024)
            with self.assertRaises(ValueError) as cm:
                await self.hub.stream_cache.append_chunk(cid, 0, huge_data)
            self.assertIn("MAX_CHUNK_SIZE", str(cm.exception))

        asyncio.run(_run())

    def test_session_recovery_after_restart(self) -> None:
        """可靠性测试：服务端进程重启后，未完成和已完成的缓存会话自动从元数据恢复."""

        async def _run() -> None:
            from server.app.stream_tee_cache import StreamTeeCache

            chunk_size = 1024
            data = b"R" * (chunk_size * 2)
            digest = DigestManifest.from_bytes(data, chunk_size_bytes=chunk_size)
            cid = "fc_restart_test_01"

            # 1. 建立会话并写入第 1 块 (STREAMING 状态)
            await self.hub.cache_api.init_cache(cid, "node_a", len(data), digest.to_dict())
            c0 = data[:chunk_size]
            await self.hub.cache_api.patch_cache(cid, 0, c0, compute_blake3_hex(c0))

            # 2. 模拟重启：在同一缓存目录下重新实例化 StreamTeeCache
            recovered_cache = StreamTeeCache(
                cache_dir=self.hub.config.stream_cache_dir,
                ttl_seconds=self.hub.config.cache_ttl_seconds,
                quota_bytes=self.hub.config.cache_quota_bytes,
            )

            # 断言会话与写入进度已成功恢复
            recovered_entry = recovered_cache.get_entry(cid)
            self.assertEqual(recovered_entry.file_cache_id, cid)
            self.assertEqual(recovered_entry.state, "STREAMING")
            self.assertEqual(recovered_entry.written_offset, chunk_size)
            self.assertEqual(recovered_entry.total_size, len(data))

            # 3. 在恢复的缓存引擎上继续写入第 2 块完成会话
            c1 = data[chunk_size:]
            await recovered_cache.append_chunk(
                file_cache_id=cid,
                upload_offset=chunk_size,
                chunk_data=c1,
                expected_chunk_blake3=compute_blake3_hex(c1),
            )
            self.assertEqual(recovered_cache.get_entry(cid).state, "COMPLETED")

        asyncio.run(_run())


if __name__ == "__main__":
    unittest.main()

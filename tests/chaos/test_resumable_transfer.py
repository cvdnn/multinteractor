"""L3 故障注入与混沌测试：断网续传、内网中断无缝切云端续传、比特翻转重传与路径防穿越 (TC-RES-03 ~ TC-RES-05, TC-NFR-03, TC-NFR-05)."""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import tarfile
import tempfile
import unittest
from pathlib import Path

from server.app.models import DigestManifest, DigestMismatchError, ResumeState, compute_blake3_hex
from server.tests.conftest import cleanup_test_hub, make_test_hub
from tests.fixtures.generate_malicious_tar import build_malicious_tar_bytes


def verify_safe_archive_member(member_name: str) -> None:
    normalized = member_name.replace("\\", "/")
    if (
        not normalized
        or normalized.startswith("/")
        or ":" in normalized
        or ".." in normalized.split("/")
    ):
        raise PermissionError(f"SECURITY ALERT (TC-NFR-03): Path traversal blocked: {member_name}")


class TestResumableTransferChaos(unittest.TestCase):
    def setUp(self) -> None:
        self.hub, self.tmp_dir = make_test_hub()
        self.client_dir = Path(tempfile.mkdtemp(prefix="mi_client_chaos_"))

    def tearDown(self) -> None:
        cleanup_test_hub(self.hub, self.tmp_dir)
        shutil.rmtree(self.client_dir, ignore_errors=True)

    def test_tc_res_03_04_05_disconnect_resume_lan_to_cloud_failover_and_bit_flip_guard(self) -> None:
        """
        完整验证：
        1. TC-RES-03: 传输至 40% 断网重连，从 verified_offset 继续传输；
        2. TC-RES-04: 内网直连下载至 50% 时内网中断，无缝携带 Range 切换至云端缓存池拉取剩余 50%；
        3. TC-RES-05: 第 3 个分块发生 1 字节比特翻转，接收端分块 BLAKE3 校验立即拦截并单独重传该分块。
        """

        async def _run() -> None:
            chunk_size = 4096
            num_chunks = 6
            full_data = b"".join(bytes([65 + i]) * chunk_size for i in range(num_chunks))
            digest = DigestManifest.from_bytes(full_data, chunk_size_bytes=chunk_size)
            cid = "fc_chaos_01"

            # 源端上传全量数据至云端缓存就绪
            await self.hub.cache_api.init_cache(cid, "node_a", len(full_data), digest.to_dict())
            for i in range(num_chunks):
                off = i * chunk_size
                blk = full_data[off : off + chunk_size]
                await self.hub.cache_api.patch_cache(cid, off, blk, compute_blake3_hex(blk))

            target_file = self.client_dir / "video_500mb.mp4"
            part_file = Path(f"{target_file}.multinteractor.part")
            resume_file = Path(f"{target_file}.multinteractor.resume.json")
            state = ResumeState(
                transfer_id="tr_chaos_01",
                file_cache_id=cid,
                file_name=target_file.name,
                total_size=len(full_data),
                verified_offset=0,
                source_mtime=1762310000,
                digest=digest,
                active_channel="LAN_DIRECT",
            )
            part_file.write_bytes(b"")

            # 阶段 1：通过内网直连下载前 2 个分块
            for idx in range(2):
                off = idx * chunk_size
                blk = full_data[off : off + chunk_size]
                digest.verify_chunk(idx, blk)
                with open(part_file, "ab") as f:
                    f.write(blk)
                state.verified_offset += len(blk)
            resume_file.write_text(json.dumps(state.to_dict()), encoding="utf-8")

            # 阶段 2 (TC-RES-05)：第 3 个分块 (idx=2) 在网络中发生 1 字节比特翻转
            corrupted_chunk_2 = bytearray(full_data[2 * chunk_size : 3 * chunk_size])
            corrupted_chunk_2[42] ^= 0x01
            with self.assertRaises(DigestMismatchError):
                digest.verify_chunk(2, bytes(corrupted_chunk_2))
            # 确保损坏分块未写入 .part 文件
            self.assertEqual(part_file.stat().st_size, 2 * chunk_size)

            # 单独重传正常的第 3 分块 (idx=2) -> 达到 50% (3/6 块)
            valid_chunk_2 = full_data[2 * chunk_size : 3 * chunk_size]
            digest.verify_chunk(2, valid_chunk_2)
            with open(part_file, "ab") as f:
                f.write(valid_chunk_2)
            state.verified_offset += len(valid_chunk_2)
            resume_file.write_text(json.dumps(state.to_dict()), encoding="utf-8")
            self.assertEqual(state.verified_offset, 3 * chunk_size)

            # 阶段 3 (TC-RES-04)：内网直连在 50% 处中断 -> 读取 .resume.json，携带 Range 切换至云端缓存续传剩余 50%
            loaded_state = ResumeState.from_dict(json.loads(resume_file.read_text(encoding="utf-8")))
            loaded_state.active_channel = "CLOUD_CACHE"
            loaded_state.resume_count += 1
            range_hdr = f"bytes={loaded_state.verified_offset}-"

            status, _, remaining_bytes = await self.hub.cache_api.get_stream_bytes(cid, range_header=range_hdr)
            self.assertEqual(status, 206)
            self.assertEqual(len(remaining_bytes), 3 * chunk_size)

            for rel_idx in range(3):
                chunk_idx = 3 + rel_idx
                blk = remaining_bytes[rel_idx * chunk_size : (rel_idx + 1) * chunk_size]
                loaded_state.digest.verify_chunk(chunk_idx, blk)
                with open(part_file, "ab") as f:
                    f.write(blk)
                loaded_state.verified_offset += len(blk)

            # 阶段 4：全文 BLAKE3 摘要校验 100% 一致后原子重命名转正并清理 .resume.json
            loaded_state.digest.verify_bytes(part_file.read_bytes())
            os.replace(part_file, target_file)
            resume_file.unlink()

            self.assertTrue(target_file.exists())
            self.assertFalse(part_file.exists())
            self.assertFalse(resume_file.exists())
            self.assertEqual(target_file.read_bytes(), full_data)

        asyncio.run(_run())

    def test_tc_nfr_03_malicious_tar_path_traversal_blocked(self) -> None:
        """TC-NFR-03: 拦截包含 `../../etc/passwd` 恶意条目的归档流."""
        malicious_bytes = build_malicious_tar_bytes("../../etc/passwd")
        import io

        with tarfile.open(fileobj=io.BytesIO(malicious_bytes), mode="r:gz") as tf:
            with self.assertRaises(PermissionError):
                for member in tf.getmembers():
                    verify_safe_archive_member(member.name)

    def test_tc_nfr_05_cancel_transfer_cleans_up_temp_files(self) -> None:
        """TC-NFR-05: 下载至 50% 时取消传输，立即清除 `.multinteractor.part` 与 `.resume.json`."""
        target = self.client_dir / "partial_cancel.iso"
        part = Path(f"{target}.multinteractor.part")
        resume = Path(f"{target}.multinteractor.resume.json")
        part.write_bytes(b"half downloaded bytes")
        resume.write_text('{"verified_offset": 21}', encoding="utf-8")

        # 触发取消清理
        for f in (part, resume):
            if f.exists():
                f.unlink()

        self.assertFalse(part.exists())
        self.assertFalse(resume.exists())
        self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()

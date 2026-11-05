"""内嵌式“边传边下 (Stream Teeing)”与 Upload-Offset / Range 断点续传缓存引擎 (FR-SRV-03, TASK-04)."""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, AsyncIterator, Dict, Optional

from server.app.models import DigestManifest, DigestMismatchError, compute_blake3_hex


class OffsetConflictError(ValueError):
    """当上传请求的 Upload-Offset 与服务端当前已确认落盘偏移量不一致时抛出 (HTTP 409)."""


@dataclass
class StreamCacheEntry:
    file_cache_id: str
    source_device_id: str
    total_size: int
    digest: DigestManifest
    part_path: Path
    final_path: Path
    meta_path: Path
    written_offset: int = 0
    state: str = "STREAMING"  # "STREAMING" | "COMPLETED" | "ABORTED"
    created_at: float = field(default_factory=time.time)
    last_accessed_at: float = field(default_factory=time.time)
    condition: asyncio.Condition = field(default_factory=asyncio.Condition, repr=False)

    @property
    def active_read_path(self) -> Path:
        if self.state == "COMPLETED" and self.final_path.exists():
            return self.final_path
        return self.part_path

    def to_meta_dict(self) -> Dict[str, Any]:
        return {
            "file_cache_id": self.file_cache_id,
            "source_device_id": self.source_device_id,
            "total_size": self.total_size,
            "written_offset": self.written_offset,
            "state": self.state,
            "created_at": self.created_at,
            "last_accessed_at": self.last_accessed_at,
            "digest": self.digest.to_dict(),
        }


class StreamTeeCache:
    """
    支持一写多读实时分流 (Stream Teeing)、4MB 分块 BLAKE3 校验、
    Upload-Offset 上传断点续传、HTTP Range 下载断点续传与 CAS 秒传去重的缓存引擎.
    """

    def __init__(
        self,
        cache_dir: Path,
        ttl_seconds: int = 24 * 3600,
        quota_bytes: int = 10 * 1024 * 1024 * 1024,
    ) -> None:
        self.cache_dir = Path(cache_dir)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.ttl_seconds = ttl_seconds
        self.quota_bytes = quota_bytes
        self._entries: Dict[str, StreamCacheEntry] = {}
        self._hash_index: Dict[str, str] = {}  # full_hash -> completed file_cache_id

    def _paths_for(self, file_cache_id: str) -> tuple[Path, Path, Path]:
        part = self.cache_dir / f"{file_cache_id}.part"
        final = self.cache_dir / f"{file_cache_id}.cache"
        meta = self.cache_dir / f"{file_cache_id}.meta.json"
        return part, final, meta

    async def create_session(
        self,
        file_cache_id: str,
        source_device_id: str,
        total_size: int,
        digest: DigestManifest,
    ) -> StreamCacheEntry:
        if file_cache_id in self._entries:
            return self._entries[file_cache_id]

        part_path, final_path, meta_path = self._paths_for(file_cache_id)

        # CAS 秒传检测：若已有相同 full_hash 且大小一致的已完成缓存文件，直接复用
        existing_id = self._hash_index.get(digest.full_hash)
        if existing_id and existing_id in self._entries:
            existing_entry = self._entries[existing_id]
            if existing_entry.state == "COMPLETED" and existing_entry.total_size == total_size:
                if existing_entry.final_path.exists() and not final_path.exists():
                    shutil.copyfile(existing_entry.final_path, final_path)
                entry = StreamCacheEntry(
                    file_cache_id=file_cache_id,
                    source_device_id=source_device_id,
                    total_size=total_size,
                    digest=digest,
                    part_path=part_path,
                    final_path=final_path,
                    meta_path=meta_path,
                    written_offset=total_size,
                    state="COMPLETED",
                )
                self._entries[file_cache_id] = entry
                self._persist_meta(entry)
                return entry

        part_path.write_bytes(b"")
        entry = StreamCacheEntry(
            file_cache_id=file_cache_id,
            source_device_id=source_device_id,
            total_size=total_size,
            digest=digest,
            part_path=part_path,
            final_path=final_path,
            meta_path=meta_path,
            written_offset=0,
            state="STREAMING",
        )
        self._entries[file_cache_id] = entry
        if total_size == 0:
            await self._finalize_entry(entry)
        else:
            self._persist_meta(entry)
        return entry

    def get_entry(self, file_cache_id: str) -> StreamCacheEntry:
        if file_cache_id not in self._entries:
            raise KeyError(f"Cache entry not found: {file_cache_id}")
        entry = self._entries[file_cache_id]
        entry.last_accessed_at = time.time()
        return entry

    def head_status(self, file_cache_id: str) -> Dict[str, Any]:
        entry = self.get_entry(file_cache_id)
        return {
            "Upload-Offset": entry.written_offset,
            "Upload-Length": entry.total_size,
            "Cache-State": entry.state,
            "X-Full-Blake3": entry.digest.full_hash,
        }

    async def append_chunk(
        self,
        file_cache_id: str,
        upload_offset: int,
        chunk_data: bytes,
        expected_chunk_blake3: Optional[str] = None,
        chunk_index: Optional[int] = None,
    ) -> Dict[str, Any]:
        entry = self.get_entry(file_cache_id)
        async with entry.condition:
            if entry.state == "COMPLETED":
                return self.head_status(file_cache_id)

            if upload_offset != entry.written_offset:
                raise OffsetConflictError(
                    f"Upload-Offset mismatch: expected {entry.written_offset}, got {upload_offset}"
                )

            actual_hash = compute_blake3_hex(chunk_data)
            if expected_chunk_blake3 and actual_hash != expected_chunk_blake3:
                raise DigestMismatchError(
                    f"X-Chunk-Blake3 HASH_MISMATCH at offset {upload_offset}: "
                    f"expected={expected_chunk_blake3}, actual={actual_hash}"
                )

            # 若对齐在固定分块边界，同时对照 DigestManifest.chunk_hashes 校验
            step = entry.digest.chunk_size_bytes
            if chunk_index is None and step > 0 and (upload_offset % step == 0):
                inferred_idx = upload_offset // step
                if len(chunk_data) == step or (upload_offset + len(chunk_data) == entry.total_size):
                    entry.digest.verify_chunk(inferred_idx, chunk_data)
            elif chunk_index is not None:
                entry.digest.verify_chunk(chunk_index, chunk_data)

            with open(entry.part_path, "ab") as f:
                f.write(chunk_data)
                f.flush()

            entry.written_offset += len(chunk_data)
            entry.last_accessed_at = time.time()

            if entry.written_offset >= entry.total_size:
                await self._finalize_entry(entry)
            else:
                self._persist_meta(entry)

            # 实时唤醒所有正在等待边传边下 (Stream Teeing) 的下载协程
            entry.condition.notify_all()
            return self.head_status(file_cache_id)

    async def _finalize_entry(self, entry: StreamCacheEntry) -> None:
        raw_bytes = entry.part_path.read_bytes() if entry.part_path.exists() else b""
        entry.digest.verify_bytes(raw_bytes)
        os.replace(entry.part_path, entry.final_path)
        entry.state = "COMPLETED"
        self._hash_index[entry.digest.full_hash] = entry.file_cache_id
        self._persist_meta(entry)

    def _persist_meta(self, entry: StreamCacheEntry) -> None:
        tmp = entry.meta_path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(entry.to_meta_dict(), ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, entry.meta_path)

    async def stream_range(
        self,
        file_cache_id: str,
        start_offset: int = 0,
        end_offset: Optional[int] = None,
        read_chunk_size: int = 65536,
        wait_timeout: float = 30.0,
    ) -> AsyncIterator[bytes]:
        """
        边传边下 (Stream Teeing) 与 Range 断点续传异步字节流生成器.
        当源端正在上传 (state == STREAMING) 时，实时跟随 written_offset 水位线吐出已落盘字节；
        当源端已离线但缓存已完成 (state == COMPLETED) 时，直接从缓存文件按 Range 切片返回.
        """
        entry = self.get_entry(file_cache_id)
        target_end = entry.total_size if end_offset is None else min(end_offset + 1, entry.total_size)
        cursor = max(0, start_offset)

        while cursor < target_end:
            async with entry.condition:
                while cursor >= entry.written_offset and entry.state == "STREAMING":
                    await asyncio.wait_for(entry.condition.wait(), timeout=wait_timeout)
                if entry.state == "ABORTED":
                    raise RuntimeError(f"Stream cache {file_cache_id} was aborted")
                available_up_to = min(entry.written_offset, target_end)

            while cursor < available_up_to:
                to_read = min(read_chunk_size, available_up_to - cursor)
                read_path = entry.active_read_path
                with open(read_path, "rb") as f:
                    f.seek(cursor)
                    data = f.read(to_read)
                if not data:
                    break
                cursor += len(data)
                entry.last_accessed_at = time.time()
                yield data

    async def read_range_bytes(
        self,
        file_cache_id: str,
        start_offset: int = 0,
        end_offset: Optional[int] = None,
        wait_timeout: float = 30.0,
    ) -> bytes:
        chunks = []
        async for chunk in self.stream_range(
            file_cache_id=file_cache_id,
            start_offset=start_offset,
            end_offset=end_offset,
            wait_timeout=wait_timeout,
        ):
            chunks.append(chunk)
        return b"".join(chunks)

    def cleanup_expired_and_quota(self, now_time: Optional[float] = None) -> int:
        now = now_time if now_time is not None else time.time()
        removed = 0
        for cid, entry in list(self._entries.items()):
            if now - entry.last_accessed_at > self.ttl_seconds:
                self._remove_entry(cid)
                removed += 1

        # 检查总配额，按 LRU 淘汰已完成缓存
        completed = [e for e in self._entries.values() if e.state == "COMPLETED"]
        total_used = sum(e.written_offset for e in self._entries.values())
        completed.sort(key=lambda e: e.last_accessed_at)
        while total_used > self.quota_bytes and completed:
            victim = completed.pop(0)
            total_used -= victim.written_offset
            self._remove_entry(victim.file_cache_id)
            removed += 1
        return removed

    def _remove_entry(self, file_cache_id: str) -> None:
        entry = self._entries.pop(file_cache_id, None)
        if not entry:
            return
        for p in (entry.part_path, entry.final_path, entry.meta_path):
            if p.exists():
                p.unlink()

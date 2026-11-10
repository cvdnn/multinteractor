"""内嵌式“边传边下 (Stream Teeing)”与 Upload-Offset / Range 断点续传缓存引擎 (FR-SRV-03, TASK-04 / TASK-11)."""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, AsyncIterator, Callable, Dict, List, Optional, Tuple

from .models import (
    MAX_CHUNK_SIZE,
    DigestManifest,
    DigestMismatchError,
    compute_blake3_hex,
    validate_file_cache_id,
)


class OffsetConflictError(ValueError):
    """当上传请求的 Upload-Offset 与服务端当前已确认落盘偏移量不一致时抛出 (HTTP 409)."""


@dataclass
class ReaderProgress:
    reader_id: str
    reader_device_id: str
    start_offset: int
    read_offset: int
    started_at: float = field(default_factory=time.time)
    last_read_at: float = field(default_factory=time.time)

    def to_dict(self, total_size: int) -> Dict[str, Any]:
        pct = 100.0 if total_size == 0 else round((self.read_offset / total_size) * 100.0, 2)
        return {
            "reader_id": self.reader_id,
            "reader_device_id": self.reader_device_id,
            "start_offset": self.start_offset,
            "read_offset": self.read_offset,
            "progress_pct": min(100.0, pct),
            "started_at": self.started_at,
            "last_read_at": self.last_read_at,
        }


@dataclass
class StreamCacheEntry:
    file_cache_id: str
    source_device_id: str
    total_size: int
    digest: DigestManifest
    part_path: Path
    final_path: Path
    meta_path: Path
    file_name: Optional[str] = None
    content_category: str = "FILE"
    written_offset: int = 0
    download_count: int = 0
    state: str = "STREAMING"  # "STREAMING" | "COMPLETED" | "ABORTED"
    created_at: float = field(default_factory=time.time)
    last_accessed_at: float = field(default_factory=time.time)
    _condition: Optional[asyncio.Condition] = field(default=None, repr=False, compare=False)
    _condition_loop: Optional[asyncio.AbstractEventLoop] = field(default=None, repr=False, compare=False)
    active_readers: Dict[str, ReaderProgress] = field(default_factory=dict, repr=False)

    @property
    def condition(self) -> asyncio.Condition:
        running_loop: Optional[asyncio.AbstractEventLoop]
        try:
            running_loop = asyncio.get_running_loop()
        except RuntimeError:
            running_loop = None

        if self._condition is None or (
            running_loop is not None
            and self._condition_loop is not None
            and self._condition_loop is not running_loop
        ):
            self._condition = asyncio.Condition()
            self._condition_loop = running_loop
        elif running_loop is not None and self._condition_loop is None:
            self._condition_loop = running_loop
        return self._condition

    @property
    def active_read_path(self) -> Path:
        if self.state == "COMPLETED" and self.final_path.exists():
            return self.final_path
        return self.part_path

    def to_meta_dict(self) -> Dict[str, Any]:
        return {
            "file_cache_id": self.file_cache_id,
            "source_device_id": self.source_device_id,
            "file_name": self.file_name,
            "content_category": self.content_category,
            "total_size": self.total_size,
            "written_offset": self.written_offset,
            "download_count": self.download_count,
            "state": self.state,
            "created_at": self.created_at,
            "last_accessed_at": self.last_accessed_at,
            "digest": self.digest.to_dict(),
        }

    def to_session_dict(self, ttl_seconds: int, now_time: Optional[float] = None) -> Dict[str, Any]:
        now = now_time if now_time is not None else time.time()
        expires_at = self.created_at + ttl_seconds
        ttl_remaining = max(0, int(expires_at - now))
        chunk_size = max(1, self.digest.chunk_size_bytes)
        total_chunks = max(len(self.digest.chunk_hashes), 1 if self.total_size > 0 else 0)
        if self.state == "COMPLETED":
            verified_chunks = total_chunks
        else:
            verified_chunks = min(total_chunks, self.written_offset // chunk_size)

        pct = 100.0 if self.total_size == 0 else round((self.written_offset / self.total_size) * 100.0, 2)
        readers = [r.to_dict(self.total_size) for r in self.active_readers.values()]
        return {
            "file_cache_id": self.file_cache_id,
            "source_device_id": self.source_device_id,
            "file_name": self.file_name or self.file_cache_id,
            "content_category": self.content_category,
            "total_size": self.total_size,
            "written_offset": self.written_offset,
            "progress_pct": min(100.0, pct),
            "state": self.state,
            "download_count": self.download_count,
            "created_at": self.created_at,
            "last_accessed_at": self.last_accessed_at,
            "expires_at": expires_at,
            "ttl_remaining_seconds": ttl_remaining,
            "chunk_size_bytes": self.digest.chunk_size_bytes,
            "verified_chunks": verified_chunks,
            "total_chunks": total_chunks,
            "full_hash": self.digest.full_hash,
            "active_readers": readers,
        }


class StreamTeeCache:
    """
    支持一写多读实时分流 (Stream Teeing)、4MB 分块 BLAKE3 校验、
    Upload-Offset 上传断点续传、HTTP Range 下载断点续传、CAS 秒传去重与 24H TTL 强制清理的缓存引擎.
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
        # (source_device_id, full_hash) -> completed file_cache_id
        # 仅限定同设备内部秒传复用，彻底关闭跨设备秒传，防止凭借已知哈希越权窃取他人缓存
        self._hash_index: Dict[Tuple[str, str], str] = {}
        self._listeners: List[Callable[[str, Dict[str, Any]], None]] = []
        self._restore_persisted_sessions()

    def add_listener(self, callback: Callable[[str, Dict[str, Any]], None]) -> None:
        self._listeners.append(callback)

    def _emit(self, event_type: str, payload: Dict[str, Any]) -> None:
        for cb in list(self._listeners):
            try:
                cb(event_type, payload)
            except Exception:
                pass

    def _restore_persisted_sessions(self) -> None:
        """从磁盘元数据文件恢复因服务重启而中断的会话，清理已过期或损坏条目."""
        now = time.time()
        for meta_file in self.cache_dir.glob("*.meta.json"):
            try:
                data = json.loads(meta_file.read_text(encoding="utf-8"))
                file_cache_id = data.get("file_cache_id")
                if not file_cache_id:
                    continue
                validate_file_cache_id(file_cache_id)
                part_path, final_path, meta_path = self._paths_for(file_cache_id)
                created_at = float(data.get("created_at", now))
                if now - created_at >= self.ttl_seconds:
                    for p in (part_path, final_path, meta_path):
                        p.unlink(missing_ok=True)
                    continue

                digest = DigestManifest.from_dict(data["digest"])
                state = data.get("state", "STREAMING")
                total_size = int(data["total_size"])
                source_device_id = str(data["source_device_id"])
                written_offset = int(data.get("written_offset", 0))

                if state == "COMPLETED":
                    if not final_path.exists():
                        if part_path.exists():
                            state = "STREAMING"
                            written_offset = part_path.stat().st_size
                        else:
                            meta_file.unlink(missing_ok=True)
                            continue
                    else:
                        written_offset = final_path.stat().st_size
                elif state == "STREAMING":
                    if part_path.exists():
                        written_offset = part_path.stat().st_size
                    else:
                        written_offset = 0

                entry = StreamCacheEntry(
                    file_cache_id=file_cache_id,
                    source_device_id=source_device_id,
                    file_name=data.get("file_name"),
                    content_category=str(data.get("content_category", "FILE")),
                    total_size=total_size,
                    digest=digest,
                    part_path=part_path,
                    final_path=final_path,
                    meta_path=meta_path,
                    written_offset=written_offset,
                    download_count=int(data.get("download_count", 0)),
                    state=state,
                    created_at=created_at,
                    last_accessed_at=float(data.get("last_accessed_at", created_at)),
                )
                self._entries[file_cache_id] = entry
                if state == "COMPLETED":
                    self._hash_index[(source_device_id, digest.full_hash)] = file_cache_id
            except Exception:
                continue

    def _paths_for(self, file_cache_id: str) -> Tuple[Path, Path, Path]:
        validate_file_cache_id(file_cache_id)
        base = self.cache_dir.resolve()
        part = (self.cache_dir / f"{file_cache_id}.part").resolve()
        final = (self.cache_dir / f"{file_cache_id}.cache").resolve()
        meta = (self.cache_dir / f"{file_cache_id}.meta.json").resolve()
        try:
            if os.path.commonpath([str(base), str(part)]) != str(base):
                raise ValueError(f"Path traversal detected in file_cache_id: {file_cache_id}")
        except ValueError:
            raise ValueError(f"Path traversal detected in file_cache_id: {file_cache_id}")
        return part, final, meta

    async def create_session(
        self,
        file_cache_id: str,
        source_device_id: str,
        total_size: int,
        digest: DigestManifest,
        file_name: Optional[str] = None,
        content_category: str = "FILE",
    ) -> StreamCacheEntry:
        now = time.time()
        if file_cache_id in self._entries:
            existing = self._entries[file_cache_id]
            if (
                existing.source_device_id != source_device_id
                or existing.total_size != total_size
                or existing.digest.full_hash != digest.full_hash
            ):
                raise ValueError(
                    f"Cache session {file_cache_id} already exists with different owner or payload"
                )
            if file_name and not existing.file_name:
                existing.file_name = file_name
                self._persist_meta(existing)
            return existing

        part_path, final_path, meta_path = self._paths_for(file_cache_id)

        # CAS 秒传检测：仅限同一台源设备 (source_device_id) 复用未过期的已完成缓存，跨设备严禁秒传
        existing_id = self._hash_index.get((source_device_id, digest.full_hash))
        if existing_id and existing_id in self._entries:
            existing_entry = self._entries[existing_id]
            not_expired = (now - existing_entry.created_at) < self.ttl_seconds
            if not_expired and existing_entry.state == "COMPLETED" and existing_entry.total_size == total_size:
                if existing_entry.final_path.exists() and not final_path.exists():
                    shutil.copyfile(existing_entry.final_path, final_path)
                entry = StreamCacheEntry(
                    file_cache_id=file_cache_id,
                    source_device_id=source_device_id,
                    file_name=file_name or existing_entry.file_name,
                    content_category=content_category,
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
                self._emit("cache_session_created", entry.to_session_dict(self.ttl_seconds))
                return entry

        part_path.write_bytes(b"")
        entry = StreamCacheEntry(
            file_cache_id=file_cache_id,
            source_device_id=source_device_id,
            file_name=file_name,
            content_category=content_category,
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
        self._emit("cache_session_created", entry.to_session_dict(self.ttl_seconds))
        return entry

    def _peek_entry(self, file_cache_id: str) -> StreamCacheEntry:
        """只读检索条目，不刷新过期时间戳，防止 WebUI 轮询/HEAD 探测意外延长 24H TTL."""
        if file_cache_id not in self._entries:
            raise KeyError(f"Cache entry not found: {file_cache_id}")
        return self._entries[file_cache_id]

    def get_entry(self, file_cache_id: str) -> StreamCacheEntry:
        return self._peek_entry(file_cache_id)

    def head_status(self, file_cache_id: str) -> Dict[str, Any]:
        entry = self._peek_entry(file_cache_id)
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
        entry = self._peek_entry(file_cache_id)
        async with entry.condition:
            if entry.state == "COMPLETED":
                return self.head_status(file_cache_id)
            if entry.state == "ABORTED":
                raise RuntimeError(f"Stream cache {file_cache_id} was aborted")

            if len(chunk_data) > MAX_CHUNK_SIZE:
                raise ValueError(
                    f"Chunk size {len(chunk_data)} exceeds MAX_CHUNK_SIZE ({MAX_CHUNK_SIZE})"
                )

            if upload_offset + len(chunk_data) > entry.total_size:
                raise ValueError(
                    f"Chunk write exceeds total_size: offset={upload_offset}, chunk_len={len(chunk_data)}, total={entry.total_size}"
                )

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
            self._emit("cache_progress", entry.to_session_dict(self.ttl_seconds))
            return self.head_status(file_cache_id)

    async def _finalize_entry(self, entry: StreamCacheEntry) -> None:
        raw_bytes = entry.part_path.read_bytes() if entry.part_path.exists() else b""
        entry.digest.verify_bytes(raw_bytes)
        os.replace(entry.part_path, entry.final_path)
        entry.state = "COMPLETED"
        self._hash_index[(entry.source_device_id, entry.digest.full_hash)] = entry.file_cache_id
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
        reader_device_id: Optional[str] = None,
    ) -> AsyncIterator[bytes]:
        """
        边传边下 (Stream Teeing) 与 Range 断点续传异步字节流生成器.
        当源端正在上传 (state == STREAMING) 时，实时跟随 written_offset 水位线吐出已落盘字节；
        当源端已离线但缓存已完成 (state == COMPLETED) 时，直接从缓存文件按 Range 切片返回.
        """
        entry = self._peek_entry(file_cache_id)
        target_end = entry.total_size if end_offset is None else min(end_offset + 1, entry.total_size)
        cursor = max(0, start_offset)
        reader_id = uuid.uuid4().hex[:10]
        reader = ReaderProgress(
            reader_id=reader_id,
            reader_device_id=reader_device_id or f"reader-{reader_id[:6]}",
            start_offset=cursor,
            read_offset=cursor,
        )
        entry.active_readers[reader_id] = reader
        self._emit("cache_progress", entry.to_session_dict(self.ttl_seconds))

        try:
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
                    now = time.time()
                    entry.last_accessed_at = now
                    reader.read_offset = cursor
                    reader.last_read_at = now
                    self._emit("cache_progress", entry.to_session_dict(self.ttl_seconds))
                    yield data
        finally:
            entry.active_readers.pop(reader_id, None)
            if cursor >= target_end and file_cache_id in self._entries:
                entry.download_count += 1
                self._persist_meta(entry)
            self._emit("cache_progress", entry.to_session_dict(self.ttl_seconds))

    async def read_range_bytes(
        self,
        file_cache_id: str,
        start_offset: int = 0,
        end_offset: Optional[int] = None,
        wait_timeout: float = 30.0,
        reader_device_id: Optional[str] = None,
    ) -> bytes:
        chunks = []
        async for chunk in self.stream_range(
            file_cache_id=file_cache_id,
            start_offset=start_offset,
            end_offset=end_offset,
            wait_timeout=wait_timeout,
            reader_device_id=reader_device_id,
        ):
            chunks.append(chunk)
        return b"".join(chunks)

    def list_sessions(self, now_time: Optional[float] = None) -> List[Dict[str, Any]]:
        now = now_time if now_time is not None else time.time()
        items = [e.to_session_dict(self.ttl_seconds, now_time=now) for e in self._entries.values()]
        items.sort(key=lambda x: x["created_at"], reverse=True)
        return items

    def get_session_detail(self, file_cache_id: str, now_time: Optional[float] = None) -> Dict[str, Any]:
        entry = self._peek_entry(file_cache_id)
        detail = entry.to_session_dict(self.ttl_seconds, now_time=now_time)
        detail["digest"] = entry.digest.to_dict()
        return detail

    def get_quota_stats(self, now_time: Optional[float] = None) -> Dict[str, Any]:
        used = sum(e.written_offset for e in self._entries.values())
        streaming_cnt = sum(1 for e in self._entries.values() if e.state == "STREAMING")
        completed_cnt = sum(1 for e in self._entries.values() if e.state == "COMPLETED")
        aborted_cnt = sum(1 for e in self._entries.values() if e.state == "ABORTED")
        reader_cnt = sum(len(e.active_readers) for e in self._entries.values())
        usage_pct = round((used / self.quota_bytes) * 100.0, 2) if self.quota_bytes > 0 else 0.0
        return {
            "cache_dir": str(self.cache_dir),
            "quota_bytes": self.quota_bytes,
            "used_bytes": used,
            "remaining_bytes": max(0, self.quota_bytes - used),
            "usage_pct": min(100.0, usage_pct),
            "ttl_seconds": self.ttl_seconds,
            "total_sessions": len(self._entries),
            "streaming_count": streaming_cnt,
            "completed_count": completed_cnt,
            "aborted_count": aborted_cnt,
            "active_reader_count": reader_cnt,
        }

    async def abort_session(self, file_cache_id: str) -> Dict[str, Any]:
        entry = self._peek_entry(file_cache_id)
        async with entry.condition:
            if entry.state == "COMPLETED":
                return entry.to_session_dict(self.ttl_seconds)
            entry.state = "ABORTED"
            self._persist_meta(entry)
            entry.condition.notify_all()
        detail = entry.to_session_dict(self.ttl_seconds)
        self._emit("cache_session_aborted", detail)
        return detail

    async def delete_session(self, file_cache_id: str) -> Dict[str, Any]:
        entry = self._peek_entry(file_cache_id)
        freed_bytes = entry.written_offset
        async with entry.condition:
            if entry.state == "STREAMING":
                entry.state = "ABORTED"
                entry.condition.notify_all()
        self._remove_entry(file_cache_id)
        self._emit("cache_session_deleted", {"file_cache_id": file_cache_id, "freed_bytes": freed_bytes})
        return {"status": "DELETED", "file_cache_id": file_cache_id, "freed_bytes": freed_bytes}

    def _sweep_orphan_files(self, parts_only: bool = False) -> Tuple[int, int]:
        """清理磁盘上未归属任何活动会话的孤立 .part / .cache / .meta.json 碎片."""
        removed_files = 0
        freed_bytes = 0
        if not self.cache_dir.exists():
            return removed_files, freed_bytes
        for p in list(self.cache_dir.iterdir()):
            if not p.is_file():
                continue
            name = p.name
            cid: Optional[str] = None
            if name.endswith(".meta.json"):
                if parts_only:
                    continue
                cid = name[: -len(".meta.json")]
            elif name.endswith(".json.tmp"):
                cid = name[: -len(".json.tmp")]
            elif name.endswith(".part"):
                cid = name[: -len(".part")]
            elif name.endswith(".cache"):
                if parts_only:
                    continue
                cid = name[: -len(".cache")]
            else:
                continue

            if cid not in self._entries:
                try:
                    sz = p.stat().st_size
                    p.unlink(missing_ok=True)
                    removed_files += 1
                    freed_bytes += sz
                except OSError:
                    pass
        return removed_files, freed_bytes

    def cleanup_expired_and_quota(self, now_time: Optional[float] = None) -> int:
        """严格按 24 小时 TTL (created_at) 与磁盘配额清理超期缓存及孤立磁盘碎片."""
        now = now_time if now_time is not None else time.time()
        removed = 0
        for cid, entry in list(self._entries.items()):
            # 严格 24h TTL：从创建起超时即清理（同时兼容未更新的最后访问时间判断）
            if (now - entry.created_at >= self.ttl_seconds) or (now - entry.last_accessed_at >= self.ttl_seconds):
                if entry.state == "STREAMING":
                    entry.state = "ABORTED"
                self._remove_entry(cid)
                removed += 1

        # 清扫孤立磁盘文件
        orphan_cnt, _ = self._sweep_orphan_files(parts_only=False)
        removed += orphan_cnt

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

    def prune_cache(
        self,
        orphan_parts_only: bool = False,
        stale_part_seconds: int = 3600,
        now_time: Optional[float] = None,
    ) -> Dict[str, Any]:
        """运维清理入口：清理超期(24H)缓存、ABORTED 会话及孤立 .part 碎片."""
        now = now_time if now_time is not None else time.time()
        removed_sessions = 0
        freed_bytes = 0

        if not orphan_parts_only:
            for cid, entry in list(self._entries.items()):
                if (
                    (now - entry.created_at >= self.ttl_seconds)
                    or (now - entry.last_accessed_at >= self.ttl_seconds)
                    or entry.state == "ABORTED"
                ):
                    freed_bytes += entry.written_offset
                    self._remove_entry(cid)
                    removed_sessions += 1

            # 配额检查
            completed = [e for e in self._entries.values() if e.state == "COMPLETED"]
            total_used = sum(e.written_offset for e in self._entries.values())
            completed.sort(key=lambda e: e.last_accessed_at)
            while total_used > self.quota_bytes and completed:
                victim = completed.pop(0)
                total_used -= victim.written_offset
                freed_bytes += victim.written_offset
                self._remove_entry(victim.file_cache_id)
                removed_sessions += 1
        else:
            for cid, entry in list(self._entries.items()):
                is_stale_streaming = (
                    entry.state == "STREAMING"
                    and not entry.active_readers
                    and (now - entry.last_accessed_at >= stale_part_seconds)
                )
                if entry.state == "ABORTED" or is_stale_streaming:
                    freed_bytes += entry.written_offset
                    self._remove_entry(cid)
                    removed_sessions += 1

        orphan_files, orphan_bytes = self._sweep_orphan_files(parts_only=orphan_parts_only)
        freed_bytes += orphan_bytes
        summary = {
            "removed_sessions": removed_sessions,
            "orphan_files_removed": orphan_files,
            "freed_bytes": freed_bytes,
            "orphan_parts_only": orphan_parts_only,
        }
        if removed_sessions or orphan_files:
            self._emit("cache_pruned", summary)
        return summary

    def _remove_entry(self, file_cache_id: str) -> None:
        entry = self._entries.pop(file_cache_id, None)
        if not entry:
            return
        self._hash_index.pop((entry.source_device_id, entry.digest.full_hash), None)
        for p in (entry.part_path, entry.final_path, entry.meta_path):
            if p.exists():
                p.unlink()

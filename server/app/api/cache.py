"""分块断点上传 (HEAD/PATCH Upload-Offset) 与边传边下流式下载 (GET Range) 接口 (/api/v1/cache)."""

from __future__ import annotations

import re
from typing import Any, Dict, Optional, Tuple

from server.app.models import DigestManifest
from server.app.stream_tee_cache import StreamTeeCache


def parse_range_header(range_header: Optional[str], total_size: int) -> Tuple[int, Optional[int]]:
    """解析 HTTP `Range: bytes=start-end` 请求头."""
    if not range_header:
        return 0, None
    m = re.match(r"^bytes=(\d+)-(\d*)$", range_header.strip())
    if not m:
        raise ValueError(f"Invalid Range header: {range_header}")
    start = int(m.group(1))
    end = int(m.group(2)) if m.group(2) else None
    if start > total_size:
        raise ValueError(f"Range start {start} exceeds total_size {total_size}")
    return start, end


class CacheApiHandler:
    def __init__(self, cache: StreamTeeCache) -> None:
        self.cache = cache

    async def init_cache(
        self,
        file_cache_id: str,
        source_device_id: str,
        total_size: int,
        digest_dict: Dict[str, Any],
    ) -> Dict[str, Any]:
        digest = DigestManifest.from_dict(digest_dict)
        await self.cache.create_session(
            file_cache_id=file_cache_id,
            source_device_id=source_device_id,
            total_size=total_size,
            digest=digest,
        )
        return self.cache.head_status(file_cache_id)

    def head_cache(self, file_cache_id: str) -> Dict[str, Any]:
        return self.cache.head_status(file_cache_id)

    async def patch_cache(
        self,
        file_cache_id: str,
        upload_offset: int,
        chunk_data: bytes,
        chunk_blake3: Optional[str] = None,
    ) -> Dict[str, Any]:
        return await self.cache.append_chunk(
            file_cache_id=file_cache_id,
            upload_offset=upload_offset,
            chunk_data=chunk_data,
            expected_chunk_blake3=chunk_blake3,
        )

    async def get_stream_bytes(
        self,
        file_cache_id: str,
        range_header: Optional[str] = None,
        wait_timeout: float = 30.0,
    ) -> Tuple[int, Dict[str, str], bytes]:
        entry = self.cache.get_entry(file_cache_id)
        start, end = parse_range_header(range_header, entry.total_size)
        data = await self.cache.read_range_bytes(
            file_cache_id=file_cache_id,
            start_offset=start,
            end_offset=end,
            wait_timeout=wait_timeout,
        )
        actual_end = start + len(data) - 1 if data else start
        status_code = 206 if range_header else 200
        headers = {
            "Content-Range": f"bytes {start}-{actual_end}/{entry.total_size}",
            "Content-Length": str(len(data)),
            "X-Full-Blake3": entry.digest.full_hash,
            "Cache-State": entry.state,
        }
        return status_code, headers, data

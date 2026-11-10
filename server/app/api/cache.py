"""分块断点上传 (HEAD/PATCH Upload-Offset)、边传边下流式下载 (GET Range) 与 WebUI 缓存管理接口 (/api/v1/cache)."""

from __future__ import annotations

import re
from typing import Any, Callable, Dict, Optional, Tuple

from ..models import DigestManifest, DigestMismatchError
from ..stream_tee_cache import OffsetConflictError, StreamTeeCache

try:
    from fastapi import APIRouter, Body, Depends, Header, HTTPException, Query, Request, Response
    from fastapi.responses import JSONResponse, StreamingResponse

    _HAS_FASTAPI = True
except ImportError:
    _HAS_FASTAPI = False


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
        file_name: Optional[str] = None,
        content_category: str = "FILE",
    ) -> Dict[str, Any]:
        digest = DigestManifest.from_dict(digest_dict)
        await self.cache.create_session(
            file_cache_id=file_cache_id,
            source_device_id=source_device_id,
            total_size=total_size,
            digest=digest,
            file_name=file_name,
            content_category=content_category,
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
        reader_device_id: Optional[str] = None,
    ) -> Tuple[int, Dict[str, str], bytes]:
        entry = self.cache.get_entry(file_cache_id)
        start, end = parse_range_header(range_header, entry.total_size)
        data = await self.cache.read_range_bytes(
            file_cache_id=file_cache_id,
            start_offset=start,
            end_offset=end,
            wait_timeout=wait_timeout,
            reader_device_id=reader_device_id,
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

    def list_sessions(self) -> Dict[str, Any]:
        return {
            "quota": self.cache.get_quota_stats(),
            "sessions": self.cache.list_sessions(),
        }

    def get_session_detail(self, file_cache_id: str) -> Dict[str, Any]:
        return self.cache.get_session_detail(file_cache_id)

    async def delete_session(self, file_cache_id: str) -> Dict[str, Any]:
        return await self.cache.delete_session(file_cache_id)

    async def abort_session(self, file_cache_id: str) -> Dict[str, Any]:
        return await self.cache.abort_session(file_cache_id)

    def prune_cache(self, orphan_parts_only: bool = False) -> Dict[str, Any]:
        return self.cache.prune_cache(orphan_parts_only=orphan_parts_only)


def build_cache_router(
    handler: CacheApiHandler,
    require_admin: Callable[..., Any],
    require_client: Callable[..., Any],
) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is not installed")

    router = APIRouter(prefix="/api/v1/cache", tags=["cache"])

    # ---- Admin Management Routes (Control Plane) ----
    @router.get("/sessions")
    async def api_list_cache_sessions(_admin: Any = Depends(require_admin)) -> Dict[str, Any]:
        return handler.list_sessions()

    @router.get("/sessions/{file_cache_id}")
    async def api_get_cache_session(
        file_cache_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return handler.get_session_detail(file_cache_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.post("/prune")
    async def api_prune_cache(
        payload: Optional[Dict[str, Any]] = Body(default=None),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        body = payload or {}
        return handler.prune_cache(orphan_parts_only=bool(body.get("orphan_parts_only", False)))

    @router.post("/{file_cache_id}/abort")
    async def api_abort_cache_session(
        file_cache_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return await handler.abort_session(file_cache_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.delete("/{file_cache_id}")
    async def api_delete_cache_session(
        file_cache_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return await handler.delete_session(file_cache_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    # ---- Client Data-Plane Streaming & Resumable Routes ----
    @router.post("/init")
    async def api_init_cache(
        payload: Dict[str, Any] = Body(...),
        _client: Any = Depends(require_client),
    ) -> Dict[str, Any]:
        try:
            return await handler.init_cache(
                file_cache_id=str(payload["file_cache_id"]),
                source_device_id=str(payload["source_device_id"]),
                total_size=int(payload["total_size"]),
                digest_dict=dict(payload["digest"]),
                file_name=payload.get("file_name"),
                content_category=str(payload.get("content_category", "FILE")),
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise HTTPException(status_code=400, detail=str(exc))

    @router.head("/{file_cache_id}")
    async def api_head_cache(
        file_cache_id: str,
        _client: Any = Depends(require_client),
    ) -> Response:
        try:
            status_map = handler.head_cache(file_cache_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))
        headers = {k: str(v) for k, v in status_map.items()}
        return Response(status_code=200, headers=headers)

    @router.patch("/{file_cache_id}")
    async def api_patch_cache(
        file_cache_id: str,
        request: Request,
        upload_offset: int = Header(..., alias="Upload-Offset"),
        x_chunk_blake3: Optional[str] = Header(default=None, alias="X-Chunk-Blake3"),
        _client: Any = Depends(require_client),
    ) -> JSONResponse:
        chunk_data = await request.body()
        try:
            res = await handler.patch_cache(
                file_cache_id=file_cache_id,
                upload_offset=int(upload_offset),
                chunk_data=chunk_data,
                chunk_blake3=x_chunk_blake3,
            )
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))
        except OffsetConflictError as exc:
            raise HTTPException(status_code=409, detail=str(exc))
        except DigestMismatchError as exc:
            raise HTTPException(status_code=422, detail=str(exc))
        except (ValueError, RuntimeError) as exc:
            raise HTTPException(status_code=400, detail=str(exc))

        headers = {k: str(v) for k, v in res.items()}
        return JSONResponse(status_code=200, content=res, headers=headers)

    @router.get("/{file_cache_id}/stream")
    async def api_stream_cache(
        file_cache_id: str,
        range_header: Optional[str] = Header(default=None, alias="Range"),
        x_device_id: Optional[str] = Header(default=None, alias="X-Device-Id"),
        reader_device_id: Optional[str] = Query(default=None),
        _client: Any = Depends(require_client),
    ) -> StreamingResponse:
        try:
            entry = handler.cache.get_entry(file_cache_id)
            start, end = parse_range_header(range_header, entry.total_size)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))
        except ValueError as exc:
            raise HTTPException(status_code=416, detail=str(exc))

        target_end = entry.total_size - 1 if end is None else min(end, max(0, entry.total_size - 1))
        expected_len = max(0, (entry.total_size if end is None else min(end + 1, entry.total_size)) - start)
        consumer_id = reader_device_id or x_device_id
        headers = {
            "Accept-Ranges": "bytes",
            "Content-Range": f"bytes {start}-{target_end}/{entry.total_size}",
            "Content-Length": str(expected_len),
            "X-Full-Blake3": entry.digest.full_hash,
            "Cache-State": entry.state,
        }
        status_code = 206 if range_header else 200
        return StreamingResponse(
            handler.cache.stream_range(
                file_cache_id=file_cache_id,
                start_offset=start,
                end_offset=end,
                reader_device_id=consumer_id,
            ),
            status_code=status_code,
            headers=headers,
            media_type="application/octet-stream",
        )

    return router

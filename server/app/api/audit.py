"""审计日志多维检索与取证导出接口 (/api/v1/audit)."""

from __future__ import annotations

import json
import time
from typing import Any, Callable, Dict, List, Optional

from ..audit_logger import AuditLogger

try:
    from fastapi import APIRouter, Depends, Query
    from fastapi.responses import Response

    _HAS_FASTAPI = True
except ImportError:
    _HAS_FASTAPI = False


class AuditApiHandler:
    def __init__(self, audit_logger: AuditLogger) -> None:
        self.audit_logger = audit_logger

    def get_logs(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        limit: int = 100,
    ) -> Dict[str, List[Dict[str, Any]]]:
        return self.audit_logger.query_logs(log_type=log_type, device_id=device_id, limit=limit)

    def query_logs_paged(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        status: Optional[str] = None,
        resumed_only: bool = False,
        keyword: Optional[str] = None,
        start_ms: Optional[int] = None,
        end_ms: Optional[int] = None,
        limit: int = 100,
        offset: int = 0,
    ) -> Dict[str, Any]:
        return self.audit_logger.query_logs_paged(
            log_type=log_type,
            device_id=device_id,
            status=status,
            resumed_only=resumed_only,
            keyword=keyword,
            start_ms=start_ms,
            end_ms=end_ms,
            limit=limit,
            offset=offset,
        )

    def get_summary(self) -> Dict[str, Any]:
        return {
            "summary": self.audit_logger.get_audit_summary(),
            "wal": self.audit_logger.get_db_stats(),
        }

    def export_logs(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        status: Optional[str] = None,
        resumed_only: bool = False,
        keyword: Optional[str] = None,
        limit: int = 1000,
    ) -> Dict[str, Any]:
        res = self.audit_logger.query_logs_paged(
            log_type=log_type,
            device_id=device_id,
            status=status,
            resumed_only=resumed_only,
            keyword=keyword,
            limit=limit,
            offset=0,
        )
        return {
            "exported_at_ms": int(time.time() * 1000),
            "filters": {
                "log_type": log_type or "all",
                "device_id": device_id,
                "status": status,
                "resumed_only": resumed_only,
                "keyword": keyword,
            },
            "summary": self.audit_logger.get_audit_summary(),
            "total": res["total"],
            "items": res["items"],
        }

    def cleanup_expired_logs(self) -> Dict[str, int]:
        deleted = self.audit_logger.cleanup_expired_logs()
        return {"deleted_rows": deleted}


def build_audit_router(
    handler: AuditApiHandler,
    require_admin: Callable[..., Any],
) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is not installed")

    router = APIRouter(prefix="/api/v1/audit", tags=["audit"])

    @router.get("/summary")
    async def api_audit_summary(_admin: Any = Depends(require_admin)) -> Dict[str, Any]:
        return handler.get_summary()

    @router.get("/export")
    async def api_audit_export(
        log_type: Optional[str] = Query(default=None),
        device_id: Optional[str] = Query(default=None),
        status: Optional[str] = Query(default=None),
        resumed_only: bool = Query(default=False),
        keyword: Optional[str] = Query(default=None),
        limit: int = Query(default=1000, ge=1, le=1000),
        _admin: Any = Depends(require_admin),
    ) -> Response:
        export_doc = handler.export_logs(
            log_type=log_type,
            device_id=device_id,
            status=status,
            resumed_only=resumed_only,
            keyword=keyword,
            limit=limit,
        )
        body = json.dumps(export_doc, ensure_ascii=False, indent=2)
        return Response(
            content=body,
            media_type="application/json",
            headers={
                "Content-Disposition": 'attachment; filename="multinteractor-audit-export.json"',
            },
        )

    @router.get("/logs")
    async def api_audit_logs(
        log_type: Optional[str] = Query(default=None),
        device_id: Optional[str] = Query(default=None),
        status: Optional[str] = Query(default=None),
        resumed_only: bool = Query(default=False),
        keyword: Optional[str] = Query(default=None),
        start_ms: Optional[int] = Query(default=None),
        end_ms: Optional[int] = Query(default=None),
        limit: int = Query(default=100, ge=1, le=1000),
        offset: int = Query(default=0, ge=0),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        return handler.query_logs_paged(
            log_type=log_type,
            device_id=device_id,
            status=status,
            resumed_only=resumed_only,
            keyword=keyword,
            start_ms=start_ms,
            end_ms=end_ms,
            limit=limit,
            offset=offset,
        )

    @router.delete("/logs/expired")
    async def api_cleanup_expired_logs(_admin: Any = Depends(require_admin)) -> Dict[str, int]:
        return handler.cleanup_expired_logs()

    return router

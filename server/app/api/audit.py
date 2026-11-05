"""审计日志多维检索接口 (/api/v1/audit/logs)."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from server.app.audit_logger import AuditLogger


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

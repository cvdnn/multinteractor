"""系统健康状态、WAL 检查点运维与仪表盘聚合总览接口 (/api/v1/system/*, /api/v1/overview)."""

from __future__ import annotations

import ipaddress
import platform
import time
from typing import Any, Callable, Dict, List, Optional

from ..audit_logger import AuditLogger
from ..client_sub_manager import ClientSubscriptionManager
from ..config import DEFAULT_CLUSTER_TOKEN, ServerConfig
from ..models import DEFAULT_CHUNK_SIZE_BYTES
from ..stream_tee_cache import StreamTeeCache
from .auth import AdminAuthManager

try:
    from fastapi import APIRouter, Depends

    import fastapi as _fastapi_pkg

    _HAS_FASTAPI = True
    _FASTAPI_VERSION = getattr(_fastapi_pkg, "__version__", "unknown")
except ImportError:
    _HAS_FASTAPI = False
    _FASTAPI_VERSION = "not-installed"


def _extract_lan_subnet(lan_endpoint: Optional[str]) -> Optional[str]:
    if not lan_endpoint:
        return None
    host_part = lan_endpoint.strip()
    if host_part.startswith("["):
        end = host_part.find("]")
        if end > 0:
            host_part = host_part[1:end]
    elif ":" in host_part:
        host_part = host_part.rsplit(":", 1)[0]
    try:
        ip = ipaddress.ip_address(host_part)
        if ip.version == 4:
            net = ipaddress.ip_network(f"{host_part}/24", strict=False)
            return str(net)
        net6 = ipaddress.ip_network(f"{host_part}/64", strict=False)
        return str(net6)
    except ValueError:
        return None


class SystemApiHandler:
    def __init__(
        self,
        config: ServerConfig,
        sub_manager: ClientSubscriptionManager,
        stream_cache: StreamTeeCache,
        audit_logger: AuditLogger,
        auth_manager: AdminAuthManager,
        started_at: Optional[float] = None,
    ) -> None:
        self.config = config
        self.sub_manager = sub_manager
        self.stream_cache = stream_cache
        self.audit = audit_logger
        self.auth_manager = auth_manager
        self.started_at = started_at if started_at is not None else time.time()

    def get_status(self) -> Dict[str, Any]:
        wal_stats = self.audit.get_db_stats()
        warnings = self.auth_manager.get_security_warnings()
        return {
            "status": "ok",
            "uptime_seconds": max(0, int(time.time() - self.started_at)),
            "runtime": {
                "python_version": platform.python_version(),
                "platform": platform.platform(),
                "fastapi_version": _FASTAPI_VERSION,
            },
            "server": {
                "host": self.config.host,
                "port": self.config.port,
                "ui_enabled": self.config.ui_enabled,
                "data_dir": str(self.config.data_dir),
                "cache_dir": str(self.stream_cache.cache_dir),
                "state_db_path": str(self.config.state_db_path),
            },
            "policies": {
                "cache_ttl_seconds": self.config.cache_ttl_seconds,
                "cache_quota_bytes": self.config.cache_quota_bytes,
                "cache_cleanup_interval_seconds": self.config.cache_cleanup_interval_seconds,
                "lan_idle_timeout_seconds": self.config.lan_idle_timeout_seconds,
                "audit_retention_days": self.config.audit_retention_days,
                "chunk_size_bytes": DEFAULT_CHUNK_SIZE_BYTES,
            },
            "wal": wal_stats,
            "security": {
                "admin_token_is_separate": self.config.admin_token_is_separate,
                "is_default_cluster_token": self.sub_manager.shared_auth_token == DEFAULT_CLUSTER_TOKEN,
                "constant_time_compare": True,
                "warnings": warnings,
            },
        }

    def wal_checkpoint(self) -> Dict[str, Any]:
        return self.audit.wal_checkpoint(mode="TRUNCATE")

    def get_overview(self) -> Dict[str, Any]:
        devices = self.sub_manager.list_devices()
        online_devices = [d for d in devices if d.get("online")]
        os_counts: Dict[str, int] = {"windows": 0, "macos": 0, "linux": 0, "other": 0}
        lan_subnets: Dict[str, List[str]] = {}
        for d in online_devices:
            os_val = str(d.get("os", "")).lower()
            if "win" in os_val:
                os_counts["windows"] += 1
            elif "mac" in os_val or "darwin" in os_val:
                os_counts["macos"] += 1
            elif "linux" in os_val or "ubuntu" in os_val:
                os_counts["linux"] += 1
            else:
                os_counts["other"] += 1

            subnet = _extract_lan_subnet(d.get("lan_endpoint"))
            if subnet:
                lan_subnets.setdefault(subnet, []).append(d["device_id"])

        sessions = self.stream_cache.list_sessions()
        active_streams = [s for s in sessions if s["state"] == "STREAMING"]
        cache_quota = self.stream_cache.get_quota_stats()
        audit_summary = self.audit.get_audit_summary()
        recent_paged = self.audit.query_logs_paged(limit=12)
        channel_activity = self.audit.get_recent_channel_activity(window_ms=300_000, limit=20)
        wal_stats = self.audit.get_db_stats()

        return {
            "timestamp_ms": int(time.time() * 1000),
            "devices": devices,
            "device_stats": {
                "total": len(devices),
                "online": len(online_devices),
                "disabled": sum(1 for d in devices if not d.get("enabled", True)),
                "online_by_os": os_counts,
            },
            "lan_subnets": lan_subnets,
            "cache_quota": cache_quota,
            "active_streams": active_streams,
            "recent_sessions": sessions[:6],
            "audit_summary": audit_summary,
            "recent_activity": recent_paged["items"],
            "channel_activity": channel_activity,
            "security_warnings": self.auth_manager.get_security_warnings(),
            "system": {
                "host": self.config.host,
                "port": self.config.port,
                "uptime_seconds": max(0, int(time.time() - self.started_at)),
                "cache_ttl_seconds": self.config.cache_ttl_seconds,
                "lan_idle_timeout_seconds": self.config.lan_idle_timeout_seconds,
                "journal_mode": wal_stats["journal_mode"],
                "synchronous": wal_stats["synchronous"],
            },
        }


def build_system_router(
    handler: SystemApiHandler,
    require_admin: Callable[..., Any],
) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is not installed")

    router = APIRouter(tags=["system"])

    @router.get("/api/v1/overview")
    async def api_get_overview(_admin: Any = Depends(require_admin)) -> Dict[str, Any]:
        return handler.get_overview()

    @router.get("/api/v1/system/status")
    async def api_system_status(_admin: Any = Depends(require_admin)) -> Dict[str, Any]:
        return handler.get_status()

    @router.post("/api/v1/system/wal-checkpoint")
    async def api_wal_checkpoint(_admin: Any = Depends(require_admin)) -> Dict[str, Any]:
        return handler.wal_checkpoint()

    return router

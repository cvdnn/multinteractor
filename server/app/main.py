"""FastAPI 入口、生命周期管理与 WebSocket 信令网关 (FR-SRV-01 ~ FR-SRV-03)."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from server.app.api.audit import AuditApiHandler
from server.app.api.cache import CacheApiHandler
from server.app.api.devices import DevicesApiHandler
from server.app.audit_logger import AuditLogger
from server.app.client_sub_manager import ClientSubscriptionManager, RegisteredDevice
from server.app.config import ServerConfig
from server.app.models import CloudEventEnvelope
from server.app.stream_tee_cache import StreamTeeCache


class CloudHubService:
    """个人云端单节点中心服务（整合客户端管理、订阅总线、SQLite 审计与边传边下流式缓存池）."""

    def __init__(self, config: Optional[ServerConfig] = None) -> None:
        self.config = config or ServerConfig()
        self.audit = AuditLogger(
            db_path=self.config.audit_db_path,
            retention_days=self.config.audit_retention_days,
        )
        self.sub_manager = ClientSubscriptionManager(
            shared_auth_token=self.config.shared_auth_token,
            audit_logger=self.audit,
        )
        self.stream_cache = StreamTeeCache(
            cache_dir=self.config.stream_cache_dir,
            ttl_seconds=self.config.cache_ttl_seconds,
            quota_bytes=self.config.cache_quota_bytes,
        )
        self.devices_api = DevicesApiHandler(self.sub_manager)
        self.audit_api = AuditApiHandler(self.audit)
        self.cache_api = CacheApiHandler(self.stream_cache)

    async def register_ws_client(
        self,
        device_id: str,
        device_name: str,
        os_name: str,
        lan_endpoint: Optional[str],
        auth_token: Optional[str] = None,
        extra_topics: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        tok = auth_token if auth_token is not None else self.config.shared_auth_token
        return await self.sub_manager.register_device(
            device_id=device_id,
            device_name=device_name,
            os_name=os_name,
            lan_endpoint=lan_endpoint,
            auth_token=tok,
            extra_topics=extra_topics,
        )

    async def disconnect_ws_client(self, device_id: str) -> None:
        await self.sub_manager.disconnect_device(device_id)

    async def publish_event(self, raw_or_envelope: Any) -> List[str]:
        if isinstance(raw_or_envelope, CloudEventEnvelope):
            env = raw_or_envelope
        else:
            env = CloudEventEnvelope.from_dict(raw_or_envelope)
        return await self.sub_manager.publish_event(env)

    def get_audit_logs(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        limit: int = 100,
    ) -> Dict[str, List[Dict[str, Any]]]:
        return self.audit_api.get_logs(log_type=log_type, device_id=device_id, limit=limit)

    def close(self) -> None:
        self.audit.close()


def create_app(config: Optional[ServerConfig] = None) -> CloudHubService:
    return CloudHubService(config=config)

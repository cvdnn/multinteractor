"""客户端注册准入、在线状态/内网 IP:Port 映射维护与 Pub/Sub Topic 订阅路由管理 (FR-SRV-01, TASK-02)."""

from __future__ import annotations

import asyncio
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Set

from server.app.audit_logger import AuditLogger
from server.app.models import (
    SPEC_VERSION_V6,
    TOPIC_CLIPBOARD_BROADCAST,
    TOPIC_SYSTEM_PRESENCE,
    CloudEventEnvelope,
    DigestManifest,
    EventPayload,
    SourceDevice,
    TargetScope,
    topic_rpc_fs,
    topic_sync_direct,
)


def _now_ms() -> int:
    return int(time.time() * 1000)


class PermissionDeniedError(PermissionError):
    """客户端鉴权失败、已被禁用或无权访问目标设备时抛出."""


@dataclass
class RegisteredDevice:
    device_id: str
    device_name: str
    os: str
    lan_endpoint: Optional[str]
    online: bool = True
    enabled: bool = True
    first_seen_ms: int = field(default_factory=_now_ms)
    last_seen_ms: int = field(default_factory=_now_ms)
    subscribed_topics: Set[str] = field(default_factory=set)
    allowed_categories: Optional[Set[str]] = None
    inbox: asyncio.Queue = field(default_factory=asyncio.Queue, repr=False)

    def to_public_dict(self) -> Dict[str, Any]:
        return {
            "device_id": self.device_id,
            "name": self.device_name,
            "device_name": self.device_name,
            "os": self.os,
            "lan_endpoint": self.lan_endpoint,
            "online": self.online,
            "enabled": self.enabled,
            "first_seen_ms": self.first_seen_ms,
            "last_seen_ms": self.last_seen_ms,
            "subscribed_topics": sorted(self.subscribed_topics),
        }


class ClientSubscriptionManager:
    """统一管理客户端注册鉴权、内网 IP:Port 同步与 Pub/Sub 事件订阅分发."""

    def __init__(self, shared_auth_token: str, audit_logger: AuditLogger) -> None:
        self._shared_auth_token = shared_auth_token
        self._revoked_tokens: Set[str] = set()
        self._issued_tokens: Set[str] = {shared_auth_token}
        self._devices: Dict[str, RegisteredDevice] = {}
        self._audit = audit_logger

    def issue_token(self, token: Optional[str] = None) -> str:
        new_tok = token or f"mi-tok-{uuid.uuid4().hex}"
        self._issued_tokens.add(new_tok)
        self._revoked_tokens.discard(new_tok)
        return new_tok

    def revoke_token(self, token: str) -> None:
        self._revoked_tokens.add(token)

    def verify_token(self, token: str) -> bool:
        return bool(token) and (token in self._issued_tokens) and (token not in self._revoked_tokens)

    @staticmethod
    def default_topics_for_device(device_id: str) -> Set[str]:
        return {
            TOPIC_CLIPBOARD_BROADCAST,
            TOPIC_SYSTEM_PRESENCE,
            topic_sync_direct(device_id),
            topic_rpc_fs(device_id),
        }

    async def register_device(
        self,
        device_id: str,
        device_name: str,
        os_name: str,
        lan_endpoint: Optional[str],
        auth_token: str,
        extra_topics: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        if not self.verify_token(auth_token):
            self._audit.log_connection(
                device_id=device_id,
                device_name=device_name,
                action="REGISTER",
                lan_endpoint=lan_endpoint,
                status="AUTH_FAILED",
                detail="Invalid or revoked auth token",
            )
            raise PermissionDeniedError("403 Forbidden: Invalid or revoked auth token")

        existing = self._devices.get(device_id)
        if existing and not existing.enabled:
            self._audit.log_connection(
                device_id=device_id,
                device_name=device_name,
                action="REGISTER",
                lan_endpoint=lan_endpoint,
                status="DEVICE_DISABLED",
                detail="Device is disabled by administrator",
            )
            raise PermissionDeniedError("403 Forbidden: Device is disabled")

        topics = self.default_topics_for_device(device_id)
        if extra_topics:
            topics.update(extra_topics)

        now = _now_ms()
        if existing is None:
            dev = RegisteredDevice(
                device_id=device_id,
                device_name=device_name,
                os=os_name,
                lan_endpoint=lan_endpoint,
                online=True,
                enabled=True,
                first_seen_ms=now,
                last_seen_ms=now,
                subscribed_topics=topics,
            )
            self._devices[device_id] = dev
        else:
            existing.device_name = device_name
            existing.os = os_name
            existing.lan_endpoint = lan_endpoint
            existing.online = True
            existing.last_seen_ms = now
            existing.subscribed_topics.update(topics)
            dev = existing

        self._audit.log_connection(
            device_id=device_id,
            device_name=device_name,
            action="ONLINE",
            lan_endpoint=lan_endpoint,
            status="OK",
            detail=f"Subscribed topics: {sorted(dev.subscribed_topics)}",
        )
        await self._broadcast_presence(dev, state="ONLINE")
        return dev

    async def disconnect_device(self, device_id: str) -> None:
        dev = self._devices.get(device_id)
        if dev is None or not dev.online:
            return
        dev.online = False
        dev.last_seen_ms = _now_ms()
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="OFFLINE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Client disconnected",
        )
        await self._broadcast_presence(dev, state="OFFLINE")

    def update_subscription(
        self,
        device_id: str,
        subscribe_topics: Optional[List[str]] = None,
        unsubscribe_topics: Optional[List[str]] = None,
        allowed_categories: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        dev = self.get_online_device(device_id)
        if subscribe_topics:
            dev.subscribed_topics.update(subscribe_topics)
        if unsubscribe_topics:
            for t in unsubscribe_topics:
                dev.subscribed_topics.discard(t)
        if allowed_categories is not None:
            dev.allowed_categories = set(allowed_categories)
        return dev

    def get_device(self, device_id_or_name: str) -> Optional[RegisteredDevice]:
        if device_id_or_name in self._devices:
            return self._devices[device_id_or_name]
        for dev in self._devices.values():
            if dev.device_name == device_id_or_name:
                return dev
        return None

    def get_online_device(self, device_id: str) -> RegisteredDevice:
        dev = self._devices.get(device_id)
        if dev is None or not dev.online or not dev.enabled:
            raise PermissionDeniedError(f"403 Forbidden: Device {device_id} is not online or not authorized")
        return dev

    def list_devices(self, online_only: bool = False) -> List[Dict[str, Any]]:
        items = [
            dev.to_public_dict()
            for dev in self._devices.values()
            if (not online_only or dev.online)
        ]
        return sorted(items, key=lambda x: x["device_id"])

    async def publish_event(self, envelope: CloudEventEnvelope) -> List[str]:
        """校验事件发送方权限并按 Topic 与订阅过滤规则精准路由."""
        sender = self.get_online_device(envelope.source_device.device_id)
        sender.last_seen_ms = _now_ms()

        delivered_to: List[str] = []
        topic = envelope.topic
        cat = envelope.payload.content_category

        for dev_id, dev in self._devices.items():
            if not dev.online or not dev.enabled:
                continue
            # 广播事件不回传给发送方自身
            if envelope.target_scope.mode == "BROADCAST" and dev_id == sender.device_id:
                continue
            # 单播事件仅投递给目标列表且已订阅该 Topic 的设备
            if envelope.target_scope.mode == "DIRECT":
                if "*" not in envelope.target_scope.target_device_ids and dev_id not in envelope.target_scope.target_device_ids:
                    continue
            if topic not in dev.subscribed_topics:
                continue
            if dev.allowed_categories is not None and cat not in dev.allowed_categories:
                continue

            await dev.inbox.put(envelope.to_dict())
            delivered_to.append(dev_id)

        total_bytes = sum(m.size_bytes for m in envelope.payload.items_meta)
        if total_bytes == 0 and envelope.payload.inline_data:
            total_bytes = len(envelope.payload.inline_data.encode("utf-8"))

        self._audit.log_event(
            event_id=envelope.event_id,
            topic=envelope.topic,
            event_type=envelope.event_type,
            source_device_id=sender.device_id,
            target_device_ids=delivered_to,
            content_category=cat,
            content_hash=envelope.payload.digest.full_hash,
            size_bytes=total_bytes,
            timestamp_ms=envelope.timestamp_ms,
        )
        return delivered_to

    async def _broadcast_presence(self, changed_dev: RegisteredDevice, state: str) -> None:
        presence_payload = json_bytes = f'{{"device_id":"{changed_dev.device_id}","state":"{state}","lan_endpoint":"{changed_dev.lan_endpoint or ""}"}}'
        digest = DigestManifest.from_bytes(json_bytes.encode("utf-8"))
        env = CloudEventEnvelope(
            spec_version=SPEC_VERSION_V6,
            event_id=f"evt_presence_{uuid.uuid4().hex[:12]}",
            topic=TOPIC_SYSTEM_PRESENCE,
            event_type="EVENT_DEVICE_PRESENCE",
            timestamp_ms=_now_ms(),
            source_device=SourceDevice(
                device_id=changed_dev.device_id,
                device_name=changed_dev.device_name,
                os=changed_dev.os,
                lan_endpoint=changed_dev.lan_endpoint,
            ),
            target_scope=TargetScope(mode="BROADCAST", target_device_ids=["*"]),
            payload=EventPayload(
                content_category="TEXT",
                digest=digest,
                inline_data=presence_payload,
                file_cache_id=None,
                upload_state="INLINE",
                items_meta=[],
            ),
        )
        for dev_id, dev in self._devices.items():
            if dev_id != changed_dev.device_id and dev.online and TOPIC_SYSTEM_PRESENCE in dev.subscribed_topics:
                await dev.inbox.put(env.to_dict())

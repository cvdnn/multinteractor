"""客户端注册准入、在线状态/内网 IP:Port 映射维护与 Pub/Sub Topic 订阅路由管理 (FR-SRV-01, TASK-02 / TASK-11)."""

from __future__ import annotations

import asyncio
import json
import secrets
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Set

from .audit_logger import AuditLogger
from .models import (
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
from .state_store import HubStateStore, hash_token_sha256

VALID_CONTENT_CATEGORIES = ("TEXT", "IMAGE", "FILE", "DIRECTORY")


def _now_ms() -> int:
    return int(time.time() * 1000)


def _mask_secret(secret: str) -> str:
    if len(secret) <= 8:
        return secret[:2] + "•" * 16
    return f"{secret[:12]}••••••••••••••••••••{secret[-4:]}"


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
    _inbox: Optional[asyncio.Queue] = field(default=None, repr=False, compare=False)
    _inbox_loop: Optional[asyncio.AbstractEventLoop] = field(default=None, repr=False, compare=False)

    @property
    def inbox(self) -> asyncio.Queue:
        running_loop: Optional[asyncio.AbstractEventLoop]
        try:
            running_loop = asyncio.get_running_loop()
        except RuntimeError:
            running_loop = None

        if self._inbox is None or (
            running_loop is not None and self._inbox_loop is not None and self._inbox_loop is not running_loop
        ):
            self._inbox = asyncio.Queue(maxsize=500)
            self._inbox_loop = running_loop
        elif running_loop is not None and self._inbox_loop is None:
            self._inbox_loop = running_loop
        return self._inbox

    def to_public_dict(self) -> Dict[str, Any]:
        cats = (
            sorted(self.allowed_categories)
            if self.allowed_categories is not None
            else list(VALID_CONTENT_CATEGORIES)
        )
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
            "allowed_categories": cats,
            "acl_restricted": self.allowed_categories is not None,
        }


class ClientSubscriptionManager:
    """统一管理客户端注册鉴权、内网 IP:Port 同步与 Pub/Sub 事件订阅分发."""

    def __init__(
        self,
        shared_auth_token: str,
        audit_logger: AuditLogger,
        state_db_path: Optional[Path] = None,
    ) -> None:
        self._state = HubStateStore(state_db_path)
        self._shared_auth_token = self._state.resolve_cluster_token(shared_auth_token)
        self._devices: Dict[str, RegisteredDevice] = {}
        self._audit = audit_logger
        self._listeners: List[Callable[[str, Dict[str, Any]], None]] = []
        self._session_closers: Dict[str, Callable[[str], None]] = {}
        self._restore_persisted_devices()

    @property
    def shared_auth_token(self) -> str:
        return self._shared_auth_token

    def add_listener(self, callback: Callable[[str, Dict[str, Any]], None]) -> None:
        self._listeners.append(callback)

    def _emit(self, event_type: str, payload: Dict[str, Any]) -> None:
        for cb in list(self._listeners):
            try:
                cb(event_type, payload)
            except Exception:
                pass

    def bind_session_closer(self, device_id: str, closer: Callable[[str], None]) -> None:
        self._session_closers[device_id] = closer

    def unbind_session_closer(self, device_id: str, closer: Optional[Callable[[str], None]] = None) -> None:
        current = self._session_closers.get(device_id)
        if closer is None or current is closer:
            self._session_closers.pop(device_id, None)

    def _trigger_session_close(self, device_id: str, reason: str) -> None:
        closer = self._session_closers.pop(device_id, None)
        if closer is not None:
            try:
                closer(reason)
            except Exception:
                pass

    def _restore_persisted_devices(self) -> None:
        for row in self._state.load_devices():
            allowed = set(row["allowed_categories"]) if row["allowed_categories"] is not None else None
            self._devices[row["device_id"]] = RegisteredDevice(
                device_id=row["device_id"],
                device_name=row["device_name"],
                os=row["os"],
                lan_endpoint=row["lan_endpoint"],
                online=False,
                enabled=row["enabled"],
                first_seen_ms=row["first_seen_ms"],
                last_seen_ms=row["last_seen_ms"],
                subscribed_topics=set(row["subscribed_topics"]),
                allowed_categories=allowed,
            )

    def _persist_device(self, dev: RegisteredDevice) -> None:
        self._state.upsert_device(
            device_id=dev.device_id,
            device_name=dev.device_name,
            os_name=dev.os,
            lan_endpoint=dev.lan_endpoint,
            enabled=dev.enabled,
            first_seen_ms=dev.first_seen_ms,
            last_seen_ms=dev.last_seen_ms,
            subscribed_topics=sorted(dev.subscribed_topics),
            allowed_categories=sorted(dev.allowed_categories) if dev.allowed_categories is not None else None,
        )

    def issue_token(
        self,
        token: Optional[str] = None,
        label: str = "",
        expected_os: str = "",
    ) -> str:
        new_tok = token or f"mi-tok-{uuid.uuid4().hex}"
        rec = self._state.upsert_token(new_tok, label=label, expected_os=expected_os)
        self._audit.log_connection(
            device_id="__admin__",
            device_name=label or "AdminConsole",
            action="TOKEN_ISSUE",
            status="OK",
            detail=f"Issued token_id={rec.token_id} os={expected_os or 'any'}",
        )
        self._emit("token_issued", rec.to_public_dict())
        return new_tok

    def issue_token_with_meta(
        self,
        token: Optional[str] = None,
        label: str = "",
        expected_os: str = "",
    ) -> Dict[str, Any]:
        new_tok = token or f"mi-tok-{uuid.uuid4().hex}"
        rec = self._state.upsert_token(new_tok, label=label, expected_os=expected_os)
        self._audit.log_connection(
            device_id="__admin__",
            device_name=label or "AdminConsole",
            action="TOKEN_ISSUE",
            status="OK",
            detail=f"Issued token_id={rec.token_id} os={expected_os or 'any'}",
        )
        self._emit("token_issued", rec.to_public_dict())
        return {"token": new_tok, **rec.to_public_dict()}

    def revoke_token(self, token: str) -> Optional[Dict[str, Any]]:
        rec = self._state.revoke_token_by_id_or_raw(token)
        self._audit.log_connection(
            device_id="__admin__",
            device_name="AdminConsole",
            action="TOKEN_REVOKE",
            status="OK",
            detail=f"Revoked token_id={rec.token_id if rec else 'unknown'}",
        )
        if rec is not None:
            self._emit("token_revoked", rec.to_public_dict())
            return rec.to_public_dict()
        return None

    def rotate_cluster_token(self, new_token: Optional[str] = None) -> str:
        """主集群密钥轮转：吊销所有旧 Token 并生效新 Token（不影响 MI_ADMIN_TOKEN 控制面会话）."""
        new_tok = new_token or f"mi-tok-{uuid.uuid4().hex}"
        old_tok = self._shared_auth_token
        revoked_count = self._state.revoke_all_tokens()
        # 记录旧集群密钥哈希进入黑名单以保留审计回溯（若与新密钥不同）
        if old_tok and not secrets.compare_digest(old_tok, new_tok):
            self._state.revoke_token_by_id_or_raw(old_tok)
        self._state.save_rotated_cluster_token(new_tok)
        self._shared_auth_token = new_tok
        self._audit.log_connection(
            device_id="__admin__",
            device_name="AdminConsole",
            action="TOKEN_ROTATE",
            status="OK",
            detail=f"Cluster token rotated; revoked {revoked_count} device token(s)",
        )
        self._emit("token_rotated", {"revoked_count": revoked_count})
        return new_tok

    def get_security_tokens_view(self, reveal_cluster_token: bool = False) -> Dict[str, Any]:
        records = [r.to_public_dict() for r in self._state.list_tokens()]
        cluster_info: Dict[str, Any] = {
            "masked": _mask_secret(self._shared_auth_token),
            "length": len(self._shared_auth_token),
        }
        if reveal_cluster_token:
            cluster_info["plaintext"] = self._shared_auth_token
        return {
            "cluster_token": cluster_info,
            "device_tokens": records,
            "constant_time_guard": "secrets.compare_digest (Enabled)",
        }

    def verify_token(self, token: str) -> bool:
        """防时序攻击的恒定时间 Token 比对校验."""
        if not token:
            return False
        cluster_match = secrets.compare_digest(token, self._shared_auth_token)
        cand_hash = hash_token_sha256(token)
        active_match = False
        revoked_match = False
        for rec in self._state.list_tokens():
            is_eq = secrets.compare_digest(cand_hash, rec.token_hash)
            if is_eq:
                if rec.revoked:
                    revoked_match = True
                else:
                    active_match = True
        return (cluster_match or active_match) and (not revoked_match)

    @staticmethod
    def default_topics_for_device(device_id: str) -> Set[str]:
        return {
            TOPIC_CLIPBOARD_BROADCAST,
            TOPIC_SYSTEM_PRESENCE,
            topic_sync_direct(device_id),
            topic_rpc_fs(device_id),
        }

    @staticmethod
    def validate_topic_subscription(device_id: str, topic: str) -> bool:
        """Topic 粒度 ACL 访问控制：严禁跨设备越权订阅其他设备的定向私有频道."""
        if topic in (TOPIC_CLIPBOARD_BROADCAST, TOPIC_SYSTEM_PRESENCE):
            return True
        if topic == topic_sync_direct(device_id):
            return True
        if topic == topic_rpc_fs(device_id):
            return True
        return False

    @staticmethod
    def _enqueue_envelope(dev: RegisteredDevice, data: Dict[str, Any]) -> None:
        """向客户端发送队列投递帧，当队列满 (500 帧) 时丢弃最旧帧，防止内存泄漏."""
        try:
            dev.inbox.put_nowait(data)
        except asyncio.QueueFull:
            try:
                _ = dev.inbox.get_nowait()
            except asyncio.QueueEmpty:
                pass
            try:
                dev.inbox.put_nowait(data)
            except asyncio.QueueFull:
                pass

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
            for t in extra_topics:
                if not self.validate_topic_subscription(device_id, t):
                    self._audit.log_connection(
                        device_id=device_id,
                        device_name=device_name,
                        action="REGISTER",
                        lan_endpoint=lan_endpoint,
                        status="TOPIC_ACL_DENIED",
                        detail=f"Unauthorized topic subscription: {t}",
                    )
                    raise PermissionDeniedError(
                        f"403 Forbidden: Device {device_id} is not authorized to subscribe to topic '{t}'"
                    )
                topics.add(t)

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

        self._persist_device(dev)
        self._audit.log_connection(
            device_id=device_id,
            device_name=device_name,
            action="ONLINE",
            lan_endpoint=lan_endpoint,
            status="OK",
            detail=f"Subscribed topics: {sorted(dev.subscribed_topics)}",
        )
        await self._broadcast_presence(dev, state="ONLINE")
        self._emit("device_online", dev.to_public_dict())
        return dev

    async def disconnect_device(self, device_id: str) -> None:
        dev = self._devices.get(device_id)
        if dev is None or not dev.online:
            return
        dev.online = False
        dev.last_seen_ms = _now_ms()
        self._persist_device(dev)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="OFFLINE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Client disconnected",
        )
        await self._broadcast_presence(dev, state="OFFLINE")
        self._emit("device_offline", dev.to_public_dict())

    async def disable_device(self, device_id: str) -> bool:
        """禁用设备台账并立即切断其在线会话."""
        dev = self._devices.get(device_id)
        if dev is None:
            return False
        dev.enabled = False
        self._persist_device(dev)
        self._trigger_session_close(device_id, "Device disabled by administrator")
        if dev.online:
            await self.disconnect_device(device_id)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="DISABLE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Device disabled by administrator",
        )
        self._emit("device_disabled", dev.to_public_dict())
        return True

    def enable_device(self, device_id: str) -> bool:
        """重新启用处于禁用状态的设备."""
        dev = self._devices.get(device_id)
        if dev is None:
            return False
        dev.enabled = True
        self._persist_device(dev)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="ENABLE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Device re-enabled by administrator",
        )
        self._emit("device_enabled", dev.to_public_dict())
        return True

    async def kick_device(self, device_id: str) -> bool:
        """剔除当前在线连接，迫使其重连鉴权."""
        dev = self._devices.get(device_id)
        if dev is None or not dev.online:
            return False
        self._trigger_session_close(device_id, "Device kicked by administrator")
        await self.disconnect_device(device_id)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="KICK",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Device session kicked by administrator",
        )
        self._emit("device_kicked", dev.to_public_dict())
        return True

    async def remove_device(self, device_id: str) -> bool:
        """彻底注销并从台账中删除设备."""
        dev = self._devices.get(device_id)
        if dev is None:
            return False
        self._trigger_session_close(device_id, "Device removed by administrator")
        if dev.online:
            await self.disconnect_device(device_id)
        del self._devices[device_id]
        self._state.delete_device(device_id)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="REMOVE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail="Device registration removed by administrator",
        )
        self._emit("device_removed", {"device_id": device_id})
        return True

    def update_subscription(
        self,
        device_id: str,
        subscribe_topics: Optional[List[str]] = None,
        unsubscribe_topics: Optional[List[str]] = None,
        allowed_categories: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        dev = self.get_online_device(device_id)
        if subscribe_topics:
            for t in subscribe_topics:
                if not self.validate_topic_subscription(device_id, t):
                    raise PermissionDeniedError(
                        f"403 Forbidden: Device {device_id} is not authorized to subscribe to topic '{t}'"
                    )
                dev.subscribed_topics.add(t)
        if unsubscribe_topics:
            for t in unsubscribe_topics:
                dev.subscribed_topics.discard(t)
        if allowed_categories is not None:
            dev.allowed_categories = set(allowed_categories)
        self._persist_device(dev)
        return dev

    def update_device_acl(
        self,
        device_id: str,
        allowed_categories: Optional[List[str]] = None,
        subscribed_topics: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        """管理员从 WebUI 更新设备 ACL（允许离线或在线设备更新并持久化）."""
        dev = self._devices.get(device_id)
        if dev is None:
            raise KeyError(f"Device not found: {device_id}")

        if subscribed_topics is not None:
            validated: Set[str] = set()
            for t in subscribed_topics:
                if not self.validate_topic_subscription(device_id, t):
                    raise PermissionDeniedError(
                        f"403 Forbidden: Device {device_id} is not authorized to subscribe to topic '{t}'"
                    )
                validated.add(t)
            dev.subscribed_topics = validated

        if allowed_categories is not None:
            cleaned: Set[str] = set()
            for cat in allowed_categories:
                c = str(cat).upper().strip()
                if c not in VALID_CONTENT_CATEGORIES:
                    raise ValueError(f"Invalid content_category in ACL: {cat}")
                cleaned.add(c)
            dev.allowed_categories = None if len(cleaned) == len(VALID_CONTENT_CATEGORIES) else cleaned

        self._persist_device(dev)
        self._audit.log_connection(
            device_id=dev.device_id,
            device_name=dev.device_name,
            action="ACL_UPDATE",
            lan_endpoint=dev.lan_endpoint,
            status="OK",
            detail=f"allowed_categories={sorted(dev.allowed_categories) if dev.allowed_categories is not None else 'ALL'}",
        )
        self._emit("device_acl_updated", dev.to_public_dict())
        return dev

    def touch_device_heartbeat(self, device_id: str) -> None:
        dev = self._devices.get(device_id)
        if dev is not None and dev.online:
            dev.last_seen_ms = _now_ms()

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

            self._enqueue_envelope(dev, envelope.to_dict())
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
        payload_data = {
            "device_id": changed_dev.device_id,
            "state": state,
            "lan_endpoint": changed_dev.lan_endpoint or "",
        }
        json_bytes = json.dumps(payload_data, separators=(",", ":")).encode("utf-8")
        digest = DigestManifest.from_bytes(json_bytes)
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
                inline_data=json_bytes.decode("utf-8"),
                file_cache_id=None,
                upload_state="INLINE",
                items_meta=[],
            ),
        )
        for dev_id, dev in self._devices.items():
            if dev_id != changed_dev.device_id and dev.online and TOPIC_SYSTEM_PRESENCE in dev.subscribed_topics:
                self._enqueue_envelope(dev, env.to_dict())

    def close(self) -> None:
        self._state.close()

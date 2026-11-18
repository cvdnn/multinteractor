"""FastAPI 入口、生命周期后台清理守护、Admin Telemetry 与 Client WebSocket 信令网关 (FR-SRV-01 ~ FR-SRV-03)."""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Set

from .api.audit import AuditApiHandler, build_audit_router
from .api.auth import ADMIN_SESSION_COOKIE_NAME, AdminAuthManager, build_auth_router
from .api.cache import CacheApiHandler, build_cache_router
from .api.devices import DevicesApiHandler, build_devices_router
from .api.system import SystemApiHandler, build_system_router
from .audit_logger import AuditLogger
from .client_sub_manager import ClientSubscriptionManager, PermissionDeniedError, RegisteredDevice
from .config import ServerConfig
from .models import MAX_HELLO_FRAME_SIZE, CloudEventEnvelope
from .stream_tee_cache import StreamTeeCache

try:
    from fastapi import FastAPI, Header, HTTPException, WebSocket, WebSocketDisconnect
    from fastapi.responses import HTMLResponse, RedirectResponse
    from fastapi.staticfiles import StaticFiles

    _HAS_FASTAPI = True
except ImportError:
    _HAS_FASTAPI = False

STATIC_DIR = Path(__file__).resolve().parent / "static"
LOGGER = logging.getLogger("multinteractor.hub")


class CloudHubService:
    """个人云端单节点中心服务（整合客户端管理、订阅总线、SQLite 审计、边传边下流式缓存池与控制面鉴权）."""

    def __init__(self, config: Optional[ServerConfig] = None) -> None:
        self.config = config or ServerConfig()
        self.started_at = time.time()
        self.audit = AuditLogger(
            db_path=self.config.audit_db_path,
            retention_days=self.config.audit_retention_days,
        )
        self.sub_manager = ClientSubscriptionManager(
            shared_auth_token=self.config.shared_auth_token,
            audit_logger=self.audit,
            state_db_path=self.config.state_db_path,
        )
        # 同步持久化层还原的集群密钥（如重启前执行过轮转）
        self.config.shared_auth_token = self.sub_manager.shared_auth_token

        self.auth_manager = AdminAuthManager(
            config=self.config,
            audit_logger=self.audit,
            cluster_token_provider=lambda: self.sub_manager.shared_auth_token,
        )
        self.stream_cache = StreamTeeCache(
            cache_dir=self.config.stream_cache_dir,
            ttl_seconds=self.config.cache_ttl_seconds,
            quota_bytes=self.config.cache_quota_bytes,
        )
        self.devices_api = DevicesApiHandler(self.sub_manager)
        self.audit_api = AuditApiHandler(self.audit)
        self.cache_api = CacheApiHandler(self.stream_cache)
        self.system_api = SystemApiHandler(
            config=self.config,
            sub_manager=self.sub_manager,
            stream_cache=self.stream_cache,
            audit_logger=self.audit,
            auth_manager=self.auth_manager,
            started_at=self.started_at,
        )

        self._telemetry_subscribers: Set[asyncio.Queue] = set()
        self.sub_manager.add_listener(self._on_hub_event)
        self.stream_cache.add_listener(self._on_hub_event)
        self.audit.add_listener(self._on_hub_event)

    def _on_hub_event(self, event_type: str, payload: Dict[str, Any]) -> None:
        if not self._telemetry_subscribers:
            return
        msg = {"event": event_type, "payload": payload, "ts_ms": int(time.time() * 1000)}
        for q in list(self._telemetry_subscribers):
            try:
                q.put_nowait(msg)
            except asyncio.QueueFull:
                try:
                    _ = q.get_nowait()
                    q.put_nowait(msg)
                except Exception:
                    pass
            except Exception:
                pass

    def subscribe_telemetry(self) -> asyncio.Queue:
        q: asyncio.Queue = asyncio.Queue(maxsize=128)
        self._telemetry_subscribers.add(q)
        return q

    def unsubscribe_telemetry(self, q: asyncio.Queue) -> None:
        self._telemetry_subscribers.discard(q)

    async def register_ws_client(
        self,
        device_id: str,
        device_name: str,
        os_name: str,
        lan_endpoint: Optional[str],
        auth_token: Optional[str] = None,
        extra_topics: Optional[List[str]] = None,
    ) -> RegisteredDevice:
        tok = auth_token if auth_token is not None else self.sub_manager.shared_auth_token
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
        # 若事件关联了流式缓存 ID 与文件元数据，则同步补全缓存会话展示名称
        cid = env.payload.file_cache_id
        if cid and cid in self.stream_cache._entries:
            entry = self.stream_cache._entries[cid]
            if not entry.file_name and env.payload.items_meta:
                entry.file_name = env.payload.items_meta[0].name
                entry.content_category = env.payload.content_category
                self.stream_cache._persist_meta(entry)
        return await self.sub_manager.publish_event(env)

    def rotate_cluster_token(self, new_token: Optional[str] = None) -> Dict[str, str]:
        res = self.devices_api.rotate_cluster_token(new_token=new_token)
        self.config.shared_auth_token = res["token"]
        return res

    async def disable_device(self, device_id: str) -> Dict[str, Any]:
        return await self.devices_api.disable_device(device_id)

    def enable_device(self, device_id: str) -> Dict[str, Any]:
        return self.devices_api.enable_device(device_id)

    async def remove_device(self, device_id: str) -> Dict[str, Any]:
        return await self.devices_api.remove_device(device_id)

    async def kick_device(self, device_id: str) -> Dict[str, Any]:
        return await self.devices_api.kick_device(device_id)

    def get_audit_logs(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        limit: int = 100,
    ) -> Dict[str, List[Dict[str, Any]]]:
        return self.audit_api.get_logs(log_type=log_type, device_id=device_id, limit=limit)

    def run_maintenance_tick(self, now_time: Optional[float] = None) -> Dict[str, int]:
        """执行严格 24H 云端缓存超时清理、磁盘配额淘汰及 90d 审计日志清理."""
        cache_removed = self.stream_cache.cleanup_expired_and_quota(now_time=now_time)
        now_ms = int((now_time if now_time is not None else time.time()) * 1000)
        audit_deleted = self.audit.cleanup_expired_logs(now_ms=now_ms)
        return {"cache_removed": cache_removed, "audit_deleted": audit_deleted}

    def close(self) -> None:
        self.sub_manager.close()
        self.audit.close()


def create_app(config: Optional[ServerConfig] = None) -> CloudHubService:
    return CloudHubService(config=config)


def create_fastapi_app(
    config: Optional[ServerConfig] = None,
    hub_service: Optional[CloudHubService] = None,
) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is required to build the ASGI application. Install server[dev] or Python >= 3.11 deps.")

    hub = hub_service or CloudHubService(config=config)

    @contextlib.asynccontextmanager
    async def lifespan(_app: FastAPI):
        stop_event = asyncio.Event()
        hub.run_maintenance_tick()

        async def _cleanup_worker() -> None:
            interval = max(5, hub.config.cache_cleanup_interval_seconds)
            while not stop_event.is_set():
                try:
                    await asyncio.wait_for(stop_event.wait(), timeout=interval)
                    break
                except asyncio.TimeoutError:
                    res = hub.run_maintenance_tick()
                    if res["cache_removed"] or res["audit_deleted"]:
                        LOGGER.info(
                            "Maintenance sweep cleaned %s expired/orphan cache item(s) and %s audit row(s)",
                            res["cache_removed"],
                            res["audit_deleted"],
                        )

        worker_task = asyncio.create_task(_cleanup_worker())
        try:
            yield
        finally:
            stop_event.set()
            worker_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await worker_task
            if hub_service is None:
                hub.close()

    fastapi_app = FastAPI(
        title="multinteractor Cloud Hub",
        version="0.1.0",
        description="Cross-device signaling gateway, stream-teeing cache, and Cloud Governance WebUI",
        lifespan=lifespan,
    )
    fastapi_app.state.hub = hub

    # ---- Auth Dependencies ----
    from fastapi import Cookie

    async def require_admin(
        authorization: Optional[str] = Header(default=None),
        mi_admin_session: Optional[str] = Cookie(default=None, alias=ADMIN_SESSION_COOKIE_NAME),
    ) -> str:
        ok, code, reason = hub.auth_manager.authorize_request(
            authorization_header=authorization,
            session_cookie=mi_admin_session,
        )
        if not ok:
            raise HTTPException(status_code=code, detail=reason)
        return "admin"

    async def require_client(
        authorization: Optional[str] = Header(default=None),
        x_device_id: Optional[str] = Header(default=None, alias="X-Device-Id"),
    ) -> str:
        if not authorization:
            raise HTTPException(status_code=401, detail="Missing client Bearer token")
        parts = authorization.strip().split(" ", 1)
        if len(parts) != 2 or parts[0].lower() != "bearer":
            raise HTTPException(status_code=401, detail="Invalid Authorization header format")
        tok = parts[1].strip()
        if not hub.sub_manager.verify_token(tok):
            raise HTTPException(status_code=403, detail="Invalid or revoked cluster/device token")
        if x_device_id:
            dev = hub.sub_manager.get_device(x_device_id)
            if dev is not None and not dev.enabled:
                raise HTTPException(status_code=403, detail="Device is disabled by administrator")
        return x_device_id or "client"

    def _on_cluster_rotate(new_tok: str) -> None:
        hub.config.shared_auth_token = new_tok

    # ---- Mount API Routers ----
    fastapi_app.include_router(build_auth_router(hub.auth_manager))
    fastapi_app.include_router(build_devices_router(hub.devices_api, require_admin, on_cluster_rotate=_on_cluster_rotate))
    fastapi_app.include_router(build_cache_router(hub.cache_api, require_admin, require_client))
    fastapi_app.include_router(build_audit_router(hub.audit_api, require_admin))
    fastapi_app.include_router(build_system_router(hub.system_api, require_admin))

    # ---- Healthcheck ----
    @fastapi_app.get("/healthz", tags=["health"])
    async def healthz() -> Dict[str, str]:
        return {"status": "ok"}

    # ---- Client Signaling WebSocket Gateway (/ws/signaling) ----
    @fastapi_app.websocket("/ws/signaling")
    async def ws_signaling(websocket: WebSocket) -> None:
        await websocket.accept()
        device_id: Optional[str] = None
        close_event = asyncio.Event()

        def _force_close(reason: str) -> None:
            close_event.set()
            try:
                loop = asyncio.get_running_loop()
                loop.create_task(websocket.close(code=4001, reason=reason[:120]))
            except Exception:
                pass

        try:
            raw_hello = await asyncio.wait_for(websocket.receive_text(), timeout=15.0)
            if len(raw_hello.encode("utf-8")) > max(MAX_HELLO_FRAME_SIZE, 4096):
                await websocket.send_json({"type": "ERROR", "detail": "Hello frame too large"})
                await websocket.close(code=1009)
                return

            hello = json.loads(raw_hello)
            device_id = str(hello["device_id"])
            device_name = str(hello.get("device_name", device_id))
            os_name = str(hello.get("os", "unknown"))
            lan_endpoint = hello.get("lan_endpoint")
            auth_token = str(hello.get("auth_token", ""))
            if not auth_token:
                auth_hdr = websocket.headers.get("authorization", "")
                if auth_hdr.lower().startswith("bearer "):
                    auth_token = auth_hdr.split(" ", 1)[1].strip()

            requested_topics = hello.get("subscribed_topics") or hello.get("extra_topics") or []
            defaults = ClientSubscriptionManager.default_topics_for_device(device_id)
            extra_topics = [t for t in requested_topics if t not in defaults]

            dev = await hub.sub_manager.register_device(
                device_id=device_id,
                device_name=device_name,
                os_name=os_name,
                lan_endpoint=lan_endpoint,
                auth_token=auth_token,
                extra_topics=extra_topics or None,
                cert_sha256=hello.get("cert_sha256"),
                cert_expires_at=hello.get("cert_expires_at"),
            )
            hub.sub_manager.bind_session_closer(device_id, _force_close)
            await websocket.send_json(
                {
                    "type": "REGISTERED",
                    "device_id": dev.device_id,
                    "peers": hub.sub_manager.list_devices(),
                    "revoked_cert_fingerprints": hub.sub_manager.list_revoked_certs(),
                }
            )

            async def _sender_loop() -> None:
                while not close_event.is_set():
                    try:
                        frame = await asyncio.wait_for(dev.inbox.get(), timeout=1.0)
                    except asyncio.TimeoutError:
                        continue
                    await websocket.send_json(frame)

            async def _receiver_loop() -> None:
                while not close_event.is_set():
                    raw_msg = await websocket.receive_text()
                    data = json.loads(raw_msg)
                    msg_type = str(data.get("type", "")).upper()
                    if msg_type == "PING":
                        hub.sub_manager.touch_device_heartbeat(device_id)
                        await websocket.send_json({"type": "PONG", "ts_ms": int(time.time() * 1000)})
                    elif msg_type == "PONG":
                        hub.sub_manager.touch_device_heartbeat(device_id)
                    elif msg_type == "TRANSFER_AUDIT":
                        hub.audit.log_transfer(
                            transfer_id=str(data["transfer_id"]),
                            file_cache_id=str(data.get("file_cache_id", "")),
                            source_device_id=str(data["source_device_id"]),
                            target_device_id=str(data["target_device_id"]),
                            channel_used=str(data.get("channel_used", "LAN_DIRECT")),
                            bytes_transferred=int(data.get("bytes_transferred", 0)),
                            verification_status=str(data.get("verification_status", "VERIFIED_OK")),
                            content_hash=str(data.get("content_hash", "")),
                            resumed=bool(data.get("resumed", False)),
                            resume_offset=int(data.get("resume_offset", 0)),
                            resume_count=int(data.get("resume_count", 0)),
                        )
                    elif "spec_version" in data:
                        delivered = await hub.publish_event(data)
                        await websocket.send_json(
                            {"type": "EVENT_ACK", "event_id": data.get("event_id"), "delivered_to": delivered}
                        )

            sender_task = asyncio.create_task(_sender_loop())
            receiver_task = asyncio.create_task(_receiver_loop())
            done, pending = await asyncio.wait(
                {sender_task, receiver_task},
                return_when=asyncio.FIRST_COMPLETED,
            )
            for t in pending:
                t.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await t
            for t in done:
                exc = t.exception()
                if exc and not isinstance(exc, WebSocketDisconnect):
                    raise exc
        except PermissionDeniedError as exc:
            with contextlib.suppress(Exception):
                await websocket.send_json({"type": "ERROR", "status": 403, "detail": str(exc)})
                await websocket.close(code=4003, reason=str(exc)[:120])
        except (WebSocketDisconnect, asyncio.TimeoutError):
            pass
        except Exception as exc:
            LOGGER.warning("WebSocket signaling session closed with error: %s", exc)
            with contextlib.suppress(Exception):
                await websocket.close(code=1011)
        finally:
            if device_id:
                hub.sub_manager.unbind_session_closer(device_id, _force_close)
                await hub.disconnect_ws_client(device_id)

    # ---- Admin Telemetry WebSocket (/ws/admin) ----
    @fastapi_app.websocket("/ws/admin")
    async def ws_admin(websocket: WebSocket) -> None:
        session_cookie = websocket.cookies.get(ADMIN_SESSION_COOKIE_NAME)
        auth_header = websocket.headers.get("authorization")
        query_tok = websocket.query_params.get("token")
        effective_auth = auth_header or (f"Bearer {query_tok}" if query_tok else None)

        ok, code, _reason = hub.auth_manager.authorize_request(
            authorization_header=effective_auth,
            session_cookie=session_cookie,
        )
        if not ok:
            await websocket.close(code=4003 if code == 403 else 4001)
            return

        await websocket.accept()
        q = hub.subscribe_telemetry()
        try:
            await websocket.send_json(
                {
                    "type": "telemetry",
                    "event": "snapshot",
                    "overview": hub.system_api.get_overview(),
                }
            )
            last_push = time.monotonic()
            while True:
                try:
                    item = await asyncio.wait_for(q.get(), timeout=20.0)
                except asyncio.TimeoutError:
                    await websocket.send_json({"type": "ping", "ts_ms": int(time.time() * 1000)})
                    continue

                ev_name = item["event"]
                # 对高频进度推送进行约 250ms 节流合并，避免压垮浏览器渲染线程
                if ev_name == "cache_progress":
                    now_mono = time.monotonic()
                    if (now_mono - last_push) < 0.25:
                        continue
                    while not q.empty():
                        nxt = q.get_nowait()
                        item = nxt
                        if nxt["event"] != "cache_progress":
                            break
                last_push = time.monotonic()
                await websocket.send_json(
                    {
                        "type": "telemetry",
                        "event": item["event"],
                        "payload": item.get("payload"),
                        "overview": hub.system_api.get_overview(),
                    }
                )
        except WebSocketDisconnect:
            pass
        except Exception:
            with contextlib.suppress(Exception):
                await websocket.close()
        finally:
            hub.unsubscribe_telemetry(q)

    # ---- WebUI Static Bundle & Routes ----
    if STATIC_DIR.is_dir():
        fastapi_app.mount("/static", StaticFiles(directory=str(STATIC_DIR)), name="static")
        assets_dir = STATIC_DIR / "assets"
        if assets_dir.is_dir():
            fastapi_app.mount("/assets", StaticFiles(directory=str(assets_dir)), name="assets")

    def _read_webui_index() -> Optional[str]:
        idx = STATIC_DIR / "index.html"
        if not idx.is_file():
            return None
        return idx.read_text(encoding="utf-8")

    @fastapi_app.get("/", include_in_schema=False)
    async def root_redirect() -> Any:
        if hub.config.ui_enabled and (STATIC_DIR / "index.html").is_file():
            return RedirectResponse(url="/ui", status_code=302)
        return {"service": "multinteractor-cloud-hub", "status": "ok"}

    @fastapi_app.get("/ui", response_class=HTMLResponse, include_in_schema=False)
    @fastapi_app.get("/ui/{subpath:path}", response_class=HTMLResponse, include_in_schema=False)
    async def serve_webui(subpath: str = "") -> HTMLResponse:
        if not hub.config.ui_enabled:
            raise HTTPException(status_code=404, detail="WebUI is disabled by MI_UI_ENABLED=false")
        html = _read_webui_index()
        if html is None:
            raise HTTPException(
                status_code=503,
                detail="WebUI static bundle not built yet. Run `make webui-build` in server/webui.",
            )
        return HTMLResponse(content=html)

    return fastapi_app


if _HAS_FASTAPI:
    app = create_fastapi_app()
else:
    app = None

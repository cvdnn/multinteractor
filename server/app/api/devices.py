"""设备台账查询、ACL 策略、在线状态与 Token 签发/吊销接口 (/api/v1/devices, /api/v1/security)."""

from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional

from ..client_sub_manager import ClientSubscriptionManager, PermissionDeniedError

try:
    from fastapi import APIRouter, Body, Depends, HTTPException, Query

    _HAS_FASTAPI = True
except ImportError:
    _HAS_FASTAPI = False


class DevicesApiHandler:
    def __init__(self, sub_manager: ClientSubscriptionManager) -> None:
        self.sub_manager = sub_manager

    def list_devices(self, online_only: bool = False) -> List[Dict[str, Any]]:
        return self.sub_manager.list_devices(online_only=online_only)

    def issue_token(
        self,
        token: Optional[str] = None,
        label: str = "",
        expected_os: str = "",
    ) -> Dict[str, Any]:
        return self.sub_manager.issue_token_with_meta(
            token=token,
            label=label,
            expected_os=expected_os,
        )

    def revoke_token(self, token: str) -> Dict[str, Any]:
        meta = self.sub_manager.revoke_token(token)
        out: Dict[str, Any] = {"status": "REVOKED", "token": token}
        if meta:
            out.update(meta)
        return out

    def rotate_cluster_token(self, new_token: Optional[str] = None) -> Dict[str, str]:
        token = self.sub_manager.rotate_cluster_token(new_token=new_token)
        return {"status": "ROTATED", "token": token}

    def list_security_tokens(self, reveal_cluster_token: bool = False) -> Dict[str, Any]:
        return self.sub_manager.get_security_tokens_view(reveal_cluster_token=reveal_cluster_token)

    async def disable_device(self, device_id: str) -> Dict[str, Any]:
        success = await self.sub_manager.disable_device(device_id)
        if not success:
            raise KeyError(f"Device not found: {device_id}")
        return {"status": "DISABLED", "device_id": device_id}

    def enable_device(self, device_id: str) -> Dict[str, Any]:
        success = self.sub_manager.enable_device(device_id)
        if not success:
            raise KeyError(f"Device not found: {device_id}")
        return {"status": "ACTIVE", "device_id": device_id}

    async def remove_device(self, device_id: str) -> Dict[str, Any]:
        success = await self.sub_manager.remove_device(device_id)
        if not success:
            raise KeyError(f"Device not found: {device_id}")
        return {"status": "REMOVED", "device_id": device_id}

    async def kick_device(self, device_id: str) -> Dict[str, Any]:
        success = await self.sub_manager.kick_device(device_id)
        if not success:
            raise KeyError(f"Device not found or not online: {device_id}")
        return {"status": "KICKED", "device_id": device_id}

    def update_device_acl(
        self,
        device_id: str,
        allowed_categories: Optional[List[str]] = None,
        subscribed_topics: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        dev = self.sub_manager.update_device_acl(
            device_id=device_id,
            allowed_categories=allowed_categories,
            subscribed_topics=subscribed_topics,
        )
        return {"status": "UPDATED", "device": dev.to_public_dict()}


def build_devices_router(
    handler: DevicesApiHandler,
    require_admin: Callable[..., Any],
    on_cluster_rotate: Optional[Callable[[str], None]] = None,
) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is not installed")

    router = APIRouter(tags=["devices", "security"])

    @router.get("/api/v1/devices")
    async def api_list_devices(
        online_only: bool = Query(default=False),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        devices = handler.list_devices(online_only=online_only)
        return {"devices": devices, "total": len(devices)}

    @router.post("/api/v1/devices/{device_id}/kick")
    async def api_kick_device(
        device_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return await handler.kick_device(device_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.post("/api/v1/devices/{device_id}/disable")
    async def api_disable_device(
        device_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return await handler.disable_device(device_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.post("/api/v1/devices/{device_id}/enable")
    async def api_enable_device(
        device_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return handler.enable_device(device_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.delete("/api/v1/devices/{device_id}")
    async def api_remove_device(
        device_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return await handler.remove_device(device_id)
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))

    @router.patch("/api/v1/devices/{device_id}/acl")
    async def api_update_device_acl(
        device_id: str,
        payload: Dict[str, Any] = Body(...),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        try:
            return handler.update_device_acl(
                device_id=device_id,
                allowed_categories=payload.get("allowed_categories"),
                subscribed_topics=payload.get("subscribed_topics"),
            )
        except KeyError as exc:
            raise HTTPException(status_code=404, detail=str(exc))
        except PermissionDeniedError as exc:
            raise HTTPException(status_code=403, detail=str(exc))
        except ValueError as exc:
            raise HTTPException(status_code=400, detail=str(exc))

    @router.get("/api/v1/security/tokens")
    async def api_list_security_tokens(
        reveal: bool = Query(default=False),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        return handler.list_security_tokens(reveal_cluster_token=reveal)

    @router.post("/api/v1/devices/token/issue")
    async def api_issue_token(
        payload: Optional[Dict[str, Any]] = Body(default=None),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        body = payload or {}
        return handler.issue_token(
            token=body.get("token"),
            label=str(body.get("label", "")).strip(),
            expected_os=str(body.get("expected_os", "")).strip(),
        )

    @router.delete("/api/v1/devices/token/{token_id}")
    async def api_revoke_token(
        token_id: str,
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, Any]:
        return handler.revoke_token(token_id)

    @router.post("/api/v1/devices/token/rotate")
    async def api_rotate_cluster_token(
        payload: Optional[Dict[str, Any]] = Body(default=None),
        _admin: Any = Depends(require_admin),
    ) -> Dict[str, str]:
        body = payload or {}
        raw_new = body.get("new_token")
        new_token = str(raw_new).strip() if raw_new else None
        if new_token == "":
            new_token = None
        res = handler.rotate_cluster_token(new_token=new_token)
        if on_cluster_rotate is not None:
            on_cluster_rotate(res["token"])
        return res

    return router

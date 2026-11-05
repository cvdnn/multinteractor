"""设备台账查询、在线状态与 Token 签发/吊销接口 (/api/v1/devices)."""

from __future__ import annotations

from typing import Any, Dict, List

from server.app.client_sub_manager import ClientSubscriptionManager


class DevicesApiHandler:
    def __init__(self, sub_manager: ClientSubscriptionManager) -> None:
        self.sub_manager = sub_manager

    def list_devices(self, online_only: bool = False) -> List[Dict[str, Any]]:
        return self.sub_manager.list_devices(online_only=online_only)

    def issue_token(self) -> Dict[str, str]:
        return {"token": self.sub_manager.issue_token()}

    def revoke_token(self, token: str) -> Dict[str, str]:
        self.sub_manager.revoke_token(token)
        return {"status": "REVOKED", "token": token}

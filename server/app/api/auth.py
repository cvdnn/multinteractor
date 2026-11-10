"""管理控制面鉴权与双 Token (MI_ADMIN_TOKEN vs MI_CLUSTER_TOKEN) 隔离守卫."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import secrets
import time
import uuid
from typing import Any, Callable, Dict, List, Optional, Set, Tuple

from ..audit_logger import AuditLogger
from ..config import DEFAULT_CLUSTER_TOKEN, ServerConfig

try:
    from fastapi import APIRouter, Body, Cookie, Header, HTTPException, Request, Response, status

    _HAS_FASTAPI = True
except ImportError:
    _HAS_FASTAPI = False


ADMIN_SESSION_COOKIE_NAME = "mi_admin_session"


def _b64u_encode(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def _b64u_decode(val: str) -> bytes:
    pad = "=" * (-len(val) % 4)
    return base64.urlsafe_b64decode(val + pad)


class AdminAuthManager:
    """
    WebUI 与管理端控制面认证管理器：
    - 强制基于 MI_ADMIN_TOKEN 校验（禁止按 127.0.0.1 源地址免鉴权）
    - 当 MI_ADMIN_TOKEN 已配置时，严格拒绝使用数据面 MI_CLUSTER_TOKEN 访问管理接口
    - Session Cookie 采用独立 HMAC-SHA256 签名，轮转 MI_CLUSTER_TOKEN 不中断管理员会话
    """

    def __init__(
        self,
        config: ServerConfig,
        audit_logger: AuditLogger,
        cluster_token_provider: Optional[Callable[[], str]] = None,
        session_ttl_seconds: int = 12 * 3600,
        hmac_secret: Optional[bytes] = None,
    ) -> None:
        self.config = config
        self.audit = audit_logger
        self._cluster_token_provider = cluster_token_provider or (lambda: self.config.shared_auth_token)
        self.session_ttl_seconds = session_ttl_seconds
        self._hmac_secret = hmac_secret or secrets.token_bytes(32)
        self._revoked_jtis: Set[str] = set()

    def _current_cluster_token(self) -> str:
        return self._cluster_token_provider()

    def _effective_admin_token(self) -> str:
        if self.config.admin_token_is_separate and self.config.admin_token:
            return self.config.admin_token
        return self._current_cluster_token()

    def get_security_warnings(self) -> List[Dict[str, str]]:
        warnings: List[Dict[str, str]] = []
        non_loopback = self.config.host not in ("127.0.0.1", "localhost", "::1")
        is_default_cluster = self._current_cluster_token() == DEFAULT_CLUSTER_TOKEN

        if is_default_cluster:
            warnings.append(
                {
                    "code": "DEFAULT_CLUSTER_TOKEN",
                    "severity": "HIGH" if non_loopback else "MEDIUM",
                    "title": "正在使用默认集群密钥 (DEFAULT_CLUSTER_TOKEN)",
                    "message": (
                        f"服务端当前监听于 {self.config.host}:{self.config.port} 且仍在使用内置默认密钥，"
                        "请立即前往【安全凭证与 Token】执行一键密钥轮转，或在部署配置中设置高强 MI_CLUSTER_TOKEN。"
                    ),
                }
            )
        if not self.config.admin_token_is_separate:
            warnings.append(
                {
                    "code": "ADMIN_TOKEN_NOT_ISOLATED",
                    "severity": "HIGH" if non_loopback else "MEDIUM",
                    "title": "未配置独立管理员密钥 (MI_ADMIN_TOKEN)",
                    "message": (
                        "当前未设置 MI_ADMIN_TOKEN，管理后台暂以 MI_CLUSTER_TOKEN 作为兜底凭据。"
                        "建议在环境变量中为控制面单独配置高强度的 MI_ADMIN_TOKEN 以实现平面隔离。"
                    ),
                }
            )
        return warnings

    def verify_admin_credentials(self, token: str) -> Tuple[bool, int, str]:
        if not token:
            return False, 401, "Missing authentication token"

        cluster_tok = self._current_cluster_token()
        if self.config.admin_token_is_separate and self.config.admin_token:
            if secrets.compare_digest(token, self.config.admin_token):
                return True, 200, "OK"
            if cluster_tok and secrets.compare_digest(token, cluster_tok):
                return (
                    False,
                    403,
                    "Forbidden: MI_CLUSTER_TOKEN is restricted to client data-plane access; use MI_ADMIN_TOKEN for WebUI/Admin API.",
                )
            return False, 401, "Invalid MI_ADMIN_TOKEN"

        effective = self._effective_admin_token()
        if effective and secrets.compare_digest(token, effective):
            return True, 200, "OK"
        return False, 401, "Invalid admin credentials"

    def create_session_token(self) -> Tuple[str, int]:
        now = int(time.time())
        exp = now + self.session_ttl_seconds
        payload = {
            "jti": uuid.uuid4().hex[:16],
            "iat": now,
            "exp": exp,
            "role": "admin",
        }
        payload_b64 = _b64u_encode(json.dumps(payload, separators=(",", ":")).encode("utf-8"))
        sig = hmac.new(self._hmac_secret, payload_b64.encode("ascii"), hashlib.sha256).digest()
        return f"{payload_b64}.{_b64u_encode(sig)}", exp

    def _parse_session_payload(self, cookie_value: Optional[str]) -> Optional[Dict[str, Any]]:
        if not cookie_value or "." not in cookie_value:
            return None
        try:
            payload_b64, sig_b64 = cookie_value.split(".", 1)
            expected_sig = _b64u_encode(
                hmac.new(self._hmac_secret, payload_b64.encode("ascii"), hashlib.sha256).digest()
            )
            if not secrets.compare_digest(sig_b64, expected_sig):
                return None
            payload = json.loads(_b64u_decode(payload_b64).decode("utf-8"))
            if int(payload.get("exp", 0)) < int(time.time()):
                return None
            jti = str(payload.get("jti", ""))
            if not jti or jti in self._revoked_jtis:
                return None
            return payload
        except Exception:
            return None

    def verify_session_token(self, cookie_value: Optional[str]) -> bool:
        return self._parse_session_payload(cookie_value) is not None

    def revoke_session_token(self, cookie_value: Optional[str]) -> None:
        payload = self._parse_session_payload(cookie_value)
        if payload and payload.get("jti"):
            self._revoked_jtis.add(str(payload["jti"]))

    def authorize_request(
        self,
        authorization_header: Optional[str] = None,
        session_cookie: Optional[str] = None,
    ) -> Tuple[bool, int, str]:
        if authorization_header:
            parts = authorization_header.strip().split(" ", 1)
            if len(parts) == 2 and parts[0].lower() == "bearer":
                return self.verify_admin_credentials(parts[1].strip())
            return False, 401, "Invalid Authorization header format (expected Bearer token)"

        if session_cookie and self.verify_session_token(session_cookie):
            return True, 200, "OK"

        return False, 401, "Admin authentication required"

    def login(
        self,
        token: str,
        client_host: Optional[str] = None,
    ) -> Tuple[int, Dict[str, Any], Optional[str]]:
        ok, code, reason = self.verify_admin_credentials(token)
        if not ok:
            self.audit.log_connection(
                device_id="__webui_admin__",
                device_name="WebUIAdmin",
                action="ADMIN_LOGIN",
                lan_endpoint=client_host,
                status="AUTH_FAILED",
                detail=reason,
            )
            return code, {"authenticated": False, "detail": reason}, None

        session_tok, exp = self.create_session_token()
        self.audit.log_connection(
            device_id="__webui_admin__",
            device_name="WebUIAdmin",
            action="ADMIN_LOGIN",
            lan_endpoint=client_host,
            status="OK",
            detail=f"mode={'isolated' if self.config.admin_token_is_separate else 'fallback'}",
        )
        return (
            200,
            {
                "authenticated": True,
                "expires_at": exp,
                "admin_token_is_separate": self.config.admin_token_is_separate,
                "security_warnings": self.get_security_warnings(),
            },
            session_tok,
        )

    def logout(self, session_cookie: Optional[str]) -> Dict[str, Any]:
        self.revoke_session_token(session_cookie)
        self.audit.log_connection(
            device_id="__webui_admin__",
            device_name="WebUIAdmin",
            action="ADMIN_LOGOUT",
            status="OK",
            detail="Admin session closed",
        )
        return {"authenticated": False}

    def get_session_status(
        self,
        authorization_header: Optional[str] = None,
        session_cookie: Optional[str] = None,
    ) -> Dict[str, Any]:
        ok, _, _ = self.authorize_request(authorization_header=authorization_header, session_cookie=session_cookie)
        if not ok:
            return {
                "authenticated": False,
                "admin_token_is_separate": self.config.admin_token_is_separate,
            }
        return {
            "authenticated": True,
            "admin_token_is_separate": self.config.admin_token_is_separate,
            "is_default_cluster_token": self._current_cluster_token() == DEFAULT_CLUSTER_TOKEN,
            "security_warnings": self.get_security_warnings(),
        }


def build_auth_router(auth_manager: AdminAuthManager) -> Any:
    if not _HAS_FASTAPI:
        raise RuntimeError("FastAPI is not installed")

    router = APIRouter(prefix="/api/v1/auth", tags=["auth"])

    @router.post("/login")
    async def auth_login(
        request: Request,
        response: Response,
        payload: Dict[str, Any] = Body(...),
    ) -> Dict[str, Any]:
        token = str(payload.get("token", ""))
        client_host = request.client.host if request.client else None
        code, body, session_tok = auth_manager.login(token=token, client_host=client_host)
        if code != 200 or not session_tok:
            raise HTTPException(status_code=code, detail=body.get("detail", "Unauthorized"))

        is_https = request.url.scheme == "https" or request.headers.get("x-forwarded-proto") == "https"
        response.set_cookie(
            key=ADMIN_SESSION_COOKIE_NAME,
            value=session_tok,
            max_age=auth_manager.session_ttl_seconds,
            httponly=True,
            samesite="strict",
            secure=is_https,
            path="/",
        )
        return body

    @router.post("/logout")
    async def auth_logout(
        response: Response,
        mi_admin_session: Optional[str] = Cookie(default=None),
    ) -> Dict[str, Any]:
        res = auth_manager.logout(mi_admin_session)
        response.delete_cookie(key=ADMIN_SESSION_COOKIE_NAME, path="/")
        return res

    @router.get("/session")
    async def auth_session(
        authorization: Optional[str] = Header(default=None),
        mi_admin_session: Optional[str] = Cookie(default=None),
    ) -> Dict[str, Any]:
        return auth_manager.get_session_status(
            authorization_header=authorization,
            session_cookie=mi_admin_session,
        )

    return router

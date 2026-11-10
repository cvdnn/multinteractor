"""服务端环境变量与运行配置加载 (端口、双 Token 密钥、24h TTL、10GB 缓存配额、90d 审计保留期)."""

from __future__ import annotations

import logging
import os
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

DEFAULT_CLUSTER_TOKEN = "mi-personal-secret-token-2026"


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name)
    if raw is None or not raw.strip():
        return default
    try:
        return int(raw.strip())
    except ValueError:
        return default


def _env_bool(name: str, default: bool) -> bool:
    raw = os.environ.get(name)
    if raw is None or not raw.strip():
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


@dataclass
class ServerConfig:
    host: str = field(default_factory=lambda: os.environ.get("MI_SERVER_HOST", "0.0.0.0"))
    port: int = field(default_factory=lambda: _env_int("MI_SERVER_PORT", 8000))
    data_dir: Path = field(
        default_factory=lambda: Path(
            os.environ.get("MI_SERVER_DATA_DIR", os.path.join(tempfile.gettempdir(), "multinteractor-server"))
        )
    )
    shared_auth_token: str = field(
        default_factory=lambda: os.environ.get(
            "MI_CLUSTER_TOKEN",
            os.environ.get("MI_AUTH_TOKEN", DEFAULT_CLUSTER_TOKEN),
        )
    )
    admin_token: Optional[str] = field(
        default_factory=lambda: (os.environ.get("MI_ADMIN_TOKEN") or "").strip() or None
    )
    cache_dir_override: Optional[Path] = field(
        default_factory=lambda: Path(os.environ["MI_CACHE_DIR"]) if os.environ.get("MI_CACHE_DIR") else None
    )
    cache_ttl_seconds: int = field(default_factory=lambda: _env_int("MI_CACHE_TTL_SECONDS", 24 * 3600))
    cache_quota_bytes: int = field(
        default_factory=lambda: _env_int("MI_CACHE_MAX_BYTES", 10 * 1024 * 1024 * 1024)
    )
    audit_retention_days: int = field(default_factory=lambda: _env_int("MI_AUDIT_RETENTION_DAYS", 90))
    lan_idle_timeout_seconds: int = field(default_factory=lambda: _env_int("MI_LAN_IDLE_TIMEOUT_SECONDS", 180))
    cache_cleanup_interval_seconds: int = field(
        default_factory=lambda: _env_int("MI_CACHE_CLEANUP_INTERVAL_SECONDS", 60)
    )
    ui_enabled: bool = field(default_factory=lambda: _env_bool("MI_UI_ENABLED", True))

    def __post_init__(self) -> None:
        self.data_dir = Path(self.data_dir)
        if self.cache_dir_override is not None:
            self.cache_dir_override = Path(self.cache_dir_override)

        log = logging.getLogger("multinteractor.config")
        if self.host not in ("127.0.0.1", "localhost", "::1"):
            if self.shared_auth_token == DEFAULT_CLUSTER_TOKEN:
                log.warning(
                    "SECURITY WARNING: Server is bound to '%s' with the default MI_CLUSTER_TOKEN. "
                    "Please set MI_CLUSTER_TOKEN to a strong secret!",
                    self.host,
                )
            if not self.admin_token_is_separate:
                log.warning(
                    "SECURITY WARNING: MI_ADMIN_TOKEN is not set while bound to '%s'. "
                    "WebUI admin authentication falls back to MI_CLUSTER_TOKEN; set MI_ADMIN_TOKEN for plane isolation.",
                    self.host,
                )

    @property
    def effective_admin_token(self) -> str:
        return self.admin_token if self.admin_token_is_separate and self.admin_token else self.shared_auth_token

    @property
    def admin_token_is_separate(self) -> bool:
        return bool(self.admin_token and self.admin_token.strip())

    @property
    def is_default_cluster_token(self) -> bool:
        return self.shared_auth_token == DEFAULT_CLUSTER_TOKEN

    @property
    def db_dir(self) -> Path:
        path = self.data_dir / "db"
        path.mkdir(parents=True, exist_ok=True)
        return path

    @property
    def audit_db_path(self) -> Path:
        return self.db_dir / "audit.db"

    @property
    def state_db_path(self) -> Path:
        return self.db_dir / "hub_state.db"

    @property
    def stream_cache_dir(self) -> Path:
        path = self.cache_dir_override if self.cache_dir_override is not None else (self.data_dir / "stream_cache")
        path.mkdir(parents=True, exist_ok=True)
        return path

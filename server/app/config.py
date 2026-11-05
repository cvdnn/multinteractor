"""服务端环境变量与运行配置加载 (端口、Token 密钥、24h TTL、10GB 缓存配额、90d 审计保留期)."""

from __future__ import annotations

import os
import tempfile
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class ServerConfig:
    host: str = "127.0.0.1"
    port: int = 8000
    data_dir: Path = field(
        default_factory=lambda: Path(
            os.environ.get("MI_SERVER_DATA_DIR", os.path.join(tempfile.gettempdir(), "multinteractor-server"))
        )
    )
    shared_auth_token: str = field(
        default_factory=lambda: os.environ.get("MI_AUTH_TOKEN", "mi-personal-secret-token-2026")
    )
    cache_ttl_seconds: int = 24 * 3600  # 默认保留 24 小时
    cache_quota_bytes: int = 10 * 1024 * 1024 * 1024  # 默认 10 GB 配额
    audit_retention_days: int = 90  # 默认保留 90 天
    lan_idle_timeout_seconds: int = 180  # 内网即用即连空闲满 180 秒自动断开

    @property
    def db_dir(self) -> Path:
        path = self.data_dir / "db"
        path.mkdir(parents=True, exist_ok=True)
        return path

    @property
    def audit_db_path(self) -> Path:
        return self.db_dir / "audit.db"

    @property
    def stream_cache_dir(self) -> Path:
        path = self.data_dir / "stream_cache"
        path.mkdir(parents=True, exist_ok=True)
        return path

"""服务端测试共享夹具."""

from __future__ import annotations

import shutil
import tempfile
from pathlib import Path
from typing import Tuple

from server.app.config import ServerConfig
from server.app.main import CloudHubService


def make_test_hub() -> Tuple[CloudHubService, Path]:
    tmp_dir = Path(tempfile.mkdtemp(prefix="mi_hub_test_"))
    cfg = ServerConfig(data_dir=tmp_dir, shared_auth_token="test-token-2026")
    hub = CloudHubService(config=cfg)
    return hub, tmp_dir


def cleanup_test_hub(hub: CloudHubService, tmp_dir: Path) -> None:
    hub.close()
    shutil.rmtree(tmp_dir, ignore_errors=True)

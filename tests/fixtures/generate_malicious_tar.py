"""构造包含 `../../` 或绝对路径穿越条目的恶意归档测试样本生成器 (用于安全边界测试 TC-NFR-03)."""

from __future__ import annotations

import io
import tarfile
from pathlib import Path
from typing import Optional


def build_malicious_tar_bytes(evil_entry_name: str = "../../etc/passwd") -> bytes:
    """在内存中构造包含路径穿越条目 (`../` 或绝对路径) 的归档字节流."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tf:
        safe_data = b"normal file content\n"
        safe_info = tarfile.TarInfo(name="safe_dir/readme.txt")
        safe_info.size = len(safe_data)
        tf.addfile(safe_info, io.BytesIO(safe_data))

        evil_data = b"malicious payload attempting path traversal\n"
        evil_info = tarfile.TarInfo(name=evil_entry_name)
        evil_info.size = len(evil_data)
        tf.addfile(evil_info, io.BytesIO(evil_data))
    return buf.getvalue()


def write_malicious_tar(dest_path: Path, evil_entry_name: Optional[str] = None) -> Path:
    data = build_malicious_tar_bytes(evil_entry_name or "../../etc/passwd")
    dest_path.write_bytes(data)
    return dest_path

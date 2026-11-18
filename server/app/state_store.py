"""SQLite WAL 运行态与凭据持久化存储 (设备台账、ACL、Cluster Token 与设备专属 Token)."""

from __future__ import annotations

import hashlib
import json
import sqlite3
import threading
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


def _now_ms() -> int:
    return int(time.time() * 1000)


def hash_token_sha256(token: str) -> str:
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def make_token_prefix(token: str) -> str:
    if len(token) <= 10:
        return token[:4] + "••••"
    return token[:13] + "••••••••"


@dataclass
class AccessTokenRecord:
    token_id: str
    token_hash: str
    token_prefix: str
    label: str
    expected_os: str
    issued_at_ms: int
    revoked: bool = False
    revoked_at_ms: Optional[int] = None

    def to_public_dict(self) -> Dict[str, Any]:
        return {
            "token_id": self.token_id,
            "token_prefix": self.token_prefix,
            "label": self.label,
            "expected_os": self.expected_os,
            "issued_at_ms": self.issued_at_ms,
            "revoked": self.revoked,
            "revoked_at_ms": self.revoked_at_ms,
        }


class HubStateStore:
    """持久化存储已注册设备、Topic/分类 ACL、轮转后的集群密钥及设备专属 Token."""

    def __init__(self, db_path: Optional[Path] = None) -> None:
        self._is_memory = db_path is None or str(db_path) == ":memory:"
        self.db_path = Path(db_path) if not self._is_memory else Path(":memory:")
        if not self._is_memory:
            self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(
            ":memory:" if self._is_memory else str(self.db_path),
            check_same_thread=False,
        )
        self._conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self) -> None:
        with self._lock:
            cur = self._conn.cursor()
            if not self._is_memory:
                cur.execute("PRAGMA journal_mode=WAL;")
                cur.execute("PRAGMA synchronous=NORMAL;")
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS cluster_config (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS registered_devices (
                    device_id TEXT PRIMARY KEY,
                    device_name TEXT NOT NULL,
                    os TEXT NOT NULL,
                    lan_endpoint TEXT,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    first_seen_ms INTEGER NOT NULL,
                    last_seen_ms INTEGER NOT NULL,
                    subscribed_topics_json TEXT NOT NULL,
                    allowed_categories_json TEXT
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS access_tokens (
                    token_id TEXT PRIMARY KEY,
                    token_hash TEXT UNIQUE NOT NULL,
                    token_prefix TEXT NOT NULL,
                    label TEXT NOT NULL DEFAULT '',
                    expected_os TEXT NOT NULL DEFAULT '',
                    issued_at_ms INTEGER NOT NULL,
                    revoked INTEGER NOT NULL DEFAULT 0,
                    revoked_at_ms INTEGER
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS device_certificates (
                    device_id TEXT PRIMARY KEY,
                    cert_sha256 TEXT NOT NULL,
                    cert_expires_at INTEGER,
                    pairing_status TEXT NOT NULL DEFAULT 'PENDING_PAIR',
                    sas_code TEXT,
                    updated_at_ms INTEGER NOT NULL
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS revoked_cert_fingerprints (
                    cert_sha256 TEXT PRIMARY KEY,
                    reason TEXT NOT NULL DEFAULT 'ADMIN_REVOKED',
                    revoked_at_ms INTEGER NOT NULL
                );
                """
            )
            self._conn.commit()

    def _get_config(self, key: str) -> Optional[str]:
        row = self._conn.execute("SELECT value FROM cluster_config WHERE key = ?", (key,)).fetchone()
        return str(row["value"]) if row is not None else None

    def _set_config(self, key: str, value: str) -> None:
        self._conn.execute(
            """
            INSERT INTO cluster_config (key, value, updated_at_ms)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at_ms = excluded.updated_at_ms
            """,
            (key, value, _now_ms()),
        )

    def resolve_cluster_token(self, env_token: str) -> str:
        """
        加载或初始化持久化的主集群密钥。
        若进程重启前管理员在 UI 执行过密钥轮转且环境变量 MI_CLUSTER_TOKEN 未变更，则保留轮转后的密钥；
        若用户显式修改了环境变量 MI_CLUSTER_TOKEN，则以新环境变量为准。
        """
        env_hash = hash_token_sha256(env_token)
        with self._lock:
            stored_token = self._get_config("shared_auth_token")
            stored_env_hash = self._get_config("env_token_sha256")
            if stored_token is None or stored_env_hash != env_hash:
                self._set_config("shared_auth_token", env_token)
                self._set_config("env_token_sha256", env_hash)
                self._conn.commit()
                return env_token
            return stored_token

    def save_rotated_cluster_token(self, new_token: str) -> None:
        with self._lock:
            self._set_config("shared_auth_token", new_token)
            self._conn.commit()

    def load_devices(self) -> List[Dict[str, Any]]:
        with self._lock:
            rows = self._conn.execute("SELECT * FROM registered_devices ORDER BY device_id ASC").fetchall()
            out: List[Dict[str, Any]] = []
            for r in rows:
                allowed_raw = r["allowed_categories_json"]
                out.append(
                    {
                        "device_id": str(r["device_id"]),
                        "device_name": str(r["device_name"]),
                        "os": str(r["os"]),
                        "lan_endpoint": r["lan_endpoint"],
                        "enabled": bool(r["enabled"]),
                        "first_seen_ms": int(r["first_seen_ms"]),
                        "last_seen_ms": int(r["last_seen_ms"]),
                        "subscribed_topics": json.loads(r["subscribed_topics_json"] or "[]"),
                        "allowed_categories": json.loads(allowed_raw) if allowed_raw is not None else None,
                    }
                )
            return out

    def upsert_device(
        self,
        device_id: str,
        device_name: str,
        os_name: str,
        lan_endpoint: Optional[str],
        enabled: bool,
        first_seen_ms: int,
        last_seen_ms: int,
        subscribed_topics: List[str],
        allowed_categories: Optional[List[str]],
    ) -> None:
        topics_json = json.dumps(sorted(subscribed_topics), ensure_ascii=False)
        cats_json = (
            json.dumps(sorted(allowed_categories), ensure_ascii=False)
            if allowed_categories is not None
            else None
        )
        with self._lock:
            self._conn.execute(
                """
                INSERT INTO registered_devices
                (device_id, device_name, os, lan_endpoint, enabled, first_seen_ms, last_seen_ms,
                 subscribed_topics_json, allowed_categories_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(device_id) DO UPDATE SET
                    device_name = excluded.device_name,
                    os = excluded.os,
                    lan_endpoint = excluded.lan_endpoint,
                    enabled = excluded.enabled,
                    last_seen_ms = excluded.last_seen_ms,
                    subscribed_topics_json = excluded.subscribed_topics_json,
                    allowed_categories_json = excluded.allowed_categories_json
                """,
                (
                    device_id,
                    device_name,
                    os_name,
                    lan_endpoint,
                    1 if enabled else 0,
                    int(first_seen_ms),
                    int(last_seen_ms),
                    topics_json,
                    cats_json,
                ),
            )
            self._conn.commit()

    def delete_device(self, device_id: str) -> None:
        with self._lock:
            self._conn.execute("DELETE FROM registered_devices WHERE device_id = ?", (device_id,))
            self._conn.commit()

    def upsert_token(
        self,
        raw_token: str,
        label: str = "",
        expected_os: str = "",
    ) -> AccessTokenRecord:
        tok_hash = hash_token_sha256(raw_token)
        prefix = make_token_prefix(raw_token)
        now = _now_ms()
        with self._lock:
            existing = self._conn.execute(
                "SELECT * FROM access_tokens WHERE token_hash = ?", (tok_hash,)
            ).fetchone()
            if existing is not None:
                token_id = str(existing["token_id"])
                self._conn.execute(
                    """
                    UPDATE access_tokens
                    SET revoked = 0, revoked_at_ms = NULL,
                        label = CASE WHEN ? != '' THEN ? ELSE label END,
                        expected_os = CASE WHEN ? != '' THEN ? ELSE expected_os END
                    WHERE token_id = ?
                    """,
                    (label, label, expected_os, expected_os, token_id),
                )
                self._conn.commit()
                row = self._conn.execute("SELECT * FROM access_tokens WHERE token_id = ?", (token_id,)).fetchone()
                assert row is not None
                return self._row_to_token(row)

            token_id = f"tok_{uuid.uuid4().hex[:12]}"
            self._conn.execute(
                """
                INSERT INTO access_tokens
                (token_id, token_hash, token_prefix, label, expected_os, issued_at_ms, revoked, revoked_at_ms)
                VALUES (?, ?, ?, ?, ?, ?, 0, NULL)
                """,
                (token_id, tok_hash, prefix, label, expected_os, now),
            )
            self._conn.commit()
            return AccessTokenRecord(
                token_id=token_id,
                token_hash=tok_hash,
                token_prefix=prefix,
                label=label,
                expected_os=expected_os,
                issued_at_ms=now,
                revoked=False,
                revoked_at_ms=None,
            )

    def revoke_token_by_id_or_raw(self, token_or_id: str) -> Optional[AccessTokenRecord]:
        now = _now_ms()
        tok_hash = hash_token_sha256(token_or_id)
        with self._lock:
            row = self._conn.execute(
                "SELECT * FROM access_tokens WHERE token_id = ? OR token_hash = ?",
                (token_or_id, tok_hash),
            ).fetchone()
            if row is not None:
                self._conn.execute(
                    "UPDATE access_tokens SET revoked = 1, revoked_at_ms = ? WHERE token_id = ?",
                    (now, row["token_id"]),
                )
                self._conn.commit()
                updated = self._conn.execute(
                    "SELECT * FROM access_tokens WHERE token_id = ?", (row["token_id"],)
                ).fetchone()
                return self._row_to_token(updated) if updated is not None else None

            # 若吊销的是尚未入库的原始字符串 Token，则作为已吊销黑名单条目写入
            token_id = f"tok_{uuid.uuid4().hex[:12]}"
            prefix = make_token_prefix(token_or_id)
            self._conn.execute(
                """
                INSERT INTO access_tokens
                (token_id, token_hash, token_prefix, label, expected_os, issued_at_ms, revoked, revoked_at_ms)
                VALUES (?, ?, ?, 'revoked-direct', '', ?, 1, ?)
                """,
                (token_id, tok_hash, prefix, now, now),
            )
            self._conn.commit()
            return AccessTokenRecord(
                token_id=token_id,
                token_hash=tok_hash,
                token_prefix=prefix,
                label="revoked-direct",
                expected_os="",
                issued_at_ms=now,
                revoked=True,
                revoked_at_ms=now,
            )

    def revoke_all_tokens(self) -> int:
        now = _now_ms()
        with self._lock:
            cur = self._conn.execute(
                "UPDATE access_tokens SET revoked = 1, revoked_at_ms = ? WHERE revoked = 0",
                (now,),
            )
            self._conn.commit()
            return int(cur.rowcount or 0)

    def list_tokens(self) -> List[AccessTokenRecord]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT * FROM access_tokens ORDER BY issued_at_ms DESC"
            ).fetchall()
            return [self._row_to_token(r) for r in rows]

    @staticmethod
    def _row_to_token(row: sqlite3.Row) -> AccessTokenRecord:
        return AccessTokenRecord(
            token_id=str(row["token_id"]),
            token_hash=str(row["token_hash"]),
            token_prefix=str(row["token_prefix"]),
            label=str(row["label"] or ""),
            expected_os=str(row["expected_os"] or ""),
            issued_at_ms=int(row["issued_at_ms"]),
            revoked=bool(row["revoked"]),
            revoked_at_ms=int(row["revoked_at_ms"]) if row["revoked_at_ms"] is not None else None,
        )

    def upsert_device_cert(
        self,
        device_id: str,
        cert_sha256: str,
        cert_expires_at: Optional[int] = None,
        pairing_status: str = "PENDING_PAIR",
        sas_code: Optional[str] = None,
    ) -> None:
        norm = cert_sha256.strip().replace(":", "").lower()
        now = _now_ms()
        with self._lock:
            self._conn.execute(
                """
                INSERT INTO device_certificates (
                    device_id, cert_sha256, cert_expires_at, pairing_status, sas_code, updated_at_ms
                ) VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(device_id) DO UPDATE SET
                    cert_sha256 = excluded.cert_sha256,
                    cert_expires_at = excluded.cert_expires_at,
                    pairing_status = excluded.pairing_status,
                    sas_code = COALESCE(excluded.sas_code, device_certificates.sas_code),
                    updated_at_ms = excluded.updated_at_ms
                """,
                (device_id, norm, cert_expires_at, pairing_status, sas_code, now),
            )
            self._conn.commit()

    def get_device_cert(self, device_id: str) -> Optional[Dict[str, Any]]:
        with self._lock:
            row = self._conn.execute(
                "SELECT * FROM device_certificates WHERE device_id = ?",
                (device_id,),
            ).fetchone()
            return dict(row) if row is not None else None

    def approve_device_pairing(self, device_id: str, sas_code: Optional[str] = None) -> bool:
        with self._lock:
            row = self._conn.execute(
                "SELECT sas_code FROM device_certificates WHERE device_id = ?",
                (device_id,),
            ).fetchone()
            if row is None:
                return False
            if sas_code and row["sas_code"] and str(row["sas_code"]) != sas_code.strip():
                return False
            self._conn.execute(
                "UPDATE device_certificates SET pairing_status = 'ACTIVE', updated_at_ms = ? WHERE device_id = ?",
                (_now_ms(), device_id),
            )
            self._conn.commit()
            return True

    def revoke_cert_fingerprint(self, cert_sha256: str, reason: str = "ADMIN_REVOKED") -> None:
        norm = cert_sha256.strip().replace(":", "").lower()
        now = _now_ms()
        with self._lock:
            self._conn.execute(
                """
                INSERT INTO revoked_cert_fingerprints (cert_sha256, reason, revoked_at_ms)
                VALUES (?, ?, ?)
                ON CONFLICT(cert_sha256) DO UPDATE SET
                    reason = excluded.reason,
                    revoked_at_ms = excluded.revoked_at_ms
                """,
                (norm, reason, now),
            )
            self._conn.execute(
                "UPDATE device_certificates SET pairing_status = 'REVOKED', updated_at_ms = ? WHERE cert_sha256 = ?",
                (now, norm),
            )
            self._conn.commit()

    def is_cert_revoked(self, cert_sha256: str) -> bool:
        norm = cert_sha256.strip().replace(":", "").lower()
        with self._lock:
            row = self._conn.execute(
                "SELECT 1 FROM revoked_cert_fingerprints WHERE cert_sha256 = ?",
                (norm,),
            ).fetchone()
            return row is not None

    def list_revoked_cert_fingerprints(self) -> List[str]:
        with self._lock:
            rows = self._conn.execute(
                "SELECT cert_sha256 FROM revoked_cert_fingerprints ORDER BY revoked_at_ms DESC"
            ).fetchall()
            return [str(r["cert_sha256"]) for r in rows]

    def close(self) -> None:
        with self._lock:
            self._conn.close()

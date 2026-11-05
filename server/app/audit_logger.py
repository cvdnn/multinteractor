"""SQLite WAL 全链路日志审计引擎 (连接审计 / 事件投递审计 / 文件传输与校验审计) (FR-SRV-02, TASK-02)."""

from __future__ import annotations

import json
import sqlite3
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional


def _now_ms() -> int:
    return int(time.time() * 1000)


class AuditLogger:
    """基于 SQLite WAL 模式的全链路日志审计中心."""

    def __init__(self, db_path: Path, retention_days: int = 90) -> None:
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self.retention_days = retention_days
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(str(self.db_path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self) -> None:
        with self._lock:
            cur = self._conn.cursor()
            cur.execute("PRAGMA journal_mode=WAL;")
            cur.execute("PRAGMA synchronous=NORMAL;")
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS audit_connection_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp_ms INTEGER NOT NULL,
                    device_id TEXT NOT NULL,
                    device_name TEXT NOT NULL,
                    action TEXT NOT NULL,
                    lan_endpoint TEXT,
                    status TEXT NOT NULL,
                    detail TEXT
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS audit_event_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp_ms INTEGER NOT NULL,
                    event_id TEXT NOT NULL,
                    topic TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    source_device_id TEXT NOT NULL,
                    target_device_ids_json TEXT NOT NULL,
                    content_category TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL
                );
                """
            )
            cur.execute(
                """
                CREATE TABLE IF NOT EXISTS audit_transfer_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp_ms INTEGER NOT NULL,
                    transfer_id TEXT NOT NULL,
                    file_cache_id TEXT NOT NULL,
                    source_device_id TEXT NOT NULL,
                    target_device_id TEXT NOT NULL,
                    channel_used TEXT NOT NULL,
                    resumed INTEGER NOT NULL DEFAULT 0,
                    resume_offset INTEGER NOT NULL DEFAULT 0,
                    resume_count INTEGER NOT NULL DEFAULT 0,
                    bytes_transferred INTEGER NOT NULL,
                    verification_status TEXT NOT NULL,
                    content_hash TEXT NOT NULL
                );
                """
            )
            self._conn.commit()

    def log_connection(
        self,
        device_id: str,
        device_name: str,
        action: str,
        status: str,
        lan_endpoint: Optional[str] = None,
        detail: Optional[str] = None,
        timestamp_ms: Optional[int] = None,
    ) -> int:
        ts = timestamp_ms if timestamp_ms is not None else _now_ms()
        with self._lock:
            cur = self._conn.execute(
                """
                INSERT INTO audit_connection_log
                (timestamp_ms, device_id, device_name, action, lan_endpoint, status, detail)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (ts, device_id, device_name, action, lan_endpoint, status, detail),
            )
            self._conn.commit()
            return int(cur.lastrowid or 0)

    def log_event(
        self,
        event_id: str,
        topic: str,
        event_type: str,
        source_device_id: str,
        target_device_ids: List[str],
        content_category: str,
        content_hash: str,
        size_bytes: int,
        timestamp_ms: Optional[int] = None,
    ) -> int:
        ts = timestamp_ms if timestamp_ms is not None else _now_ms()
        with self._lock:
            cur = self._conn.execute(
                """
                INSERT INTO audit_event_log
                (timestamp_ms, event_id, topic, event_type, source_device_id,
                 target_device_ids_json, content_category, content_hash, size_bytes)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    ts,
                    event_id,
                    topic,
                    event_type,
                    source_device_id,
                    json.dumps(target_device_ids, ensure_ascii=False),
                    content_category,
                    content_hash,
                    int(size_bytes),
                ),
            )
            self._conn.commit()
            return int(cur.lastrowid or 0)

    def log_transfer(
        self,
        transfer_id: str,
        file_cache_id: str,
        source_device_id: str,
        target_device_id: str,
        channel_used: str,
        bytes_transferred: int,
        verification_status: str,
        content_hash: str,
        resumed: bool = False,
        resume_offset: int = 0,
        resume_count: int = 0,
        timestamp_ms: Optional[int] = None,
    ) -> int:
        ts = timestamp_ms if timestamp_ms is not None else _now_ms()
        with self._lock:
            cur = self._conn.execute(
                """
                INSERT INTO audit_transfer_log
                (timestamp_ms, transfer_id, file_cache_id, source_device_id, target_device_id,
                 channel_used, resumed, resume_offset, resume_count, bytes_transferred,
                 verification_status, content_hash)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    ts,
                    transfer_id,
                    file_cache_id,
                    source_device_id,
                    target_device_id,
                    channel_used,
                    1 if resumed else 0,
                    int(resume_offset),
                    int(resume_count),
                    int(bytes_transferred),
                    verification_status,
                    content_hash,
                ),
            )
            self._conn.commit()
            return int(cur.lastrowid or 0)

    def query_logs(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        limit: int = 100,
    ) -> Dict[str, List[Dict[str, Any]]]:
        result: Dict[str, List[Dict[str, Any]]] = {}
        with self._lock:
            if log_type in (None, "connection"):
                sql = "SELECT * FROM audit_connection_log"
                params: List[Any] = []
                if device_id:
                    sql += " WHERE device_id = ?"
                    params.append(device_id)
                sql += " ORDER BY id DESC LIMIT ?"
                params.append(limit)
                rows = self._conn.execute(sql, params).fetchall()
                result["connection_logs"] = [dict(r) for r in rows]

            if log_type in (None, "event"):
                sql = "SELECT * FROM audit_event_log"
                params = []
                if device_id:
                    sql += " WHERE source_device_id = ? OR target_device_ids_json LIKE ?"
                    params.extend([device_id, f'%"{device_id}"%'])
                sql += " ORDER BY id DESC LIMIT ?"
                params.append(limit)
                rows = self._conn.execute(sql, params).fetchall()
                parsed_events = []
                for r in rows:
                    item = dict(r)
                    item["target_device_ids"] = json.loads(item.pop("target_device_ids_json", "[]"))
                    parsed_events.append(item)
                result["event_logs"] = parsed_events

            if log_type in (None, "transfer"):
                sql = "SELECT * FROM audit_transfer_log"
                params = []
                if device_id:
                    sql += " WHERE source_device_id = ? OR target_device_id = ?"
                    params.extend([device_id, device_id])
                sql += " ORDER BY id DESC LIMIT ?"
                params.append(limit)
                rows = self._conn.execute(sql, params).fetchall()
                parsed_transfers = []
                for r in rows:
                    item = dict(r)
                    item["resumed"] = bool(item["resumed"])
                    parsed_transfers.append(item)
                result["transfer_logs"] = parsed_transfers

        return result

    def cleanup_expired_logs(self, now_ms: Optional[int] = None) -> int:
        cutoff = (now_ms if now_ms is not None else _now_ms()) - (self.retention_days * 86400 * 1000)
        deleted = 0
        with self._lock:
            for tbl in ("audit_connection_log", "audit_event_log", "audit_transfer_log"):
                cur = self._conn.execute(f"DELETE FROM {tbl} WHERE timestamp_ms < ?", (cutoff,))
                deleted += cur.rowcount
            self._conn.commit()
        return deleted

    def close(self) -> None:
        with self._lock:
            self._conn.close()

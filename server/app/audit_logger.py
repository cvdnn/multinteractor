"""SQLite WAL 全链路日志审计引擎 (连接审计 / 事件投递审计 / 文件传输与校验审计) (FR-SRV-02, TASK-02 / TASK-11)."""

from __future__ import annotations

import json
import sqlite3
import threading
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional


def _now_ms() -> int:
    return int(time.time() * 1000)


class AuditLogger:
    """基于 SQLite WAL 模式的全链路日志审计中心."""

    def __init__(self, db_path: Path, retention_days: int = 90) -> None:
        self._is_memory = str(db_path) == ":memory:"
        self.db_path = Path(db_path) if not self._is_memory else Path(":memory:")
        if not self._is_memory:
            self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self.retention_days = retention_days
        self._lock = threading.Lock()
        self._listeners: List[Callable[[str, Dict[str, Any]], None]] = []
        self._conn = sqlite3.connect(
            ":memory:" if self._is_memory else str(self.db_path),
            check_same_thread=False,
        )
        self._conn.row_factory = sqlite3.Row
        self._init_schema()

    def add_listener(self, callback: Callable[[str, Dict[str, Any]], None]) -> None:
        self._listeners.append(callback)

    def _emit(self, record_type: str, payload: Dict[str, Any]) -> None:
        for cb in list(self._listeners):
            try:
                cb(record_type, payload)
            except Exception:
                pass

    def _init_schema(self) -> None:
        with self._lock:
            cur = self._conn.cursor()
            if not self._is_memory:
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
            cur.execute(
                "CREATE INDEX IF NOT EXISTS idx_conn_ts ON audit_connection_log(timestamp_ms DESC);"
            )
            cur.execute(
                "CREATE INDEX IF NOT EXISTS idx_event_ts ON audit_event_log(timestamp_ms DESC);"
            )
            cur.execute(
                "CREATE INDEX IF NOT EXISTS idx_transfer_ts ON audit_transfer_log(timestamp_ms DESC);"
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
            row_id = int(cur.lastrowid or 0)

        self._emit(
            "CONNECTION",
            {
                "id": row_id,
                "record_type": "CONNECTION",
                "timestamp_ms": ts,
                "device_id": device_id,
                "device_name": device_name,
                "action": action,
                "lan_endpoint": lan_endpoint,
                "status": status,
                "detail": detail,
            },
        )
        return row_id

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
            row_id = int(cur.lastrowid or 0)

        self._emit(
            "EVENT",
            {
                "id": row_id,
                "record_type": "EVENT",
                "timestamp_ms": ts,
                "event_id": event_id,
                "topic": topic,
                "event_type": event_type,
                "source_device_id": source_device_id,
                "target_device_ids": list(target_device_ids),
                "content_category": content_category,
                "content_hash": content_hash,
                "size_bytes": int(size_bytes),
            },
        )
        return row_id

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
            row_id = int(cur.lastrowid or 0)

        self._emit(
            "TRANSFER",
            {
                "id": row_id,
                "record_type": "TRANSFER",
                "timestamp_ms": ts,
                "transfer_id": transfer_id,
                "file_cache_id": file_cache_id,
                "source_device_id": source_device_id,
                "target_device_id": target_device_id,
                "channel_used": channel_used,
                "resumed": bool(resumed),
                "resume_offset": int(resume_offset),
                "resume_count": int(resume_count),
                "bytes_transferred": int(bytes_transferred),
                "verification_status": verification_status,
                "content_hash": content_hash,
            },
        )
        return row_id

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

    def query_logs_paged(
        self,
        log_type: Optional[str] = None,
        device_id: Optional[str] = None,
        status: Optional[str] = None,
        resumed_only: bool = False,
        keyword: Optional[str] = None,
        start_ms: Optional[int] = None,
        end_ms: Optional[int] = None,
        limit: int = 100,
        offset: int = 0,
    ) -> Dict[str, Any]:
        """多维过滤、关键词检索引擎与分页聚合查询."""
        t0 = time.perf_counter()
        normalized_type = (log_type or "").lower().strip() or None
        if normalized_type == "all":
            normalized_type = None
        kw = (keyword or "").strip()
        kw_like = f"%{kw}%" if kw else None
        status_norm = (status or "").strip() or None
        if status_norm == "RESUMED":
            resumed_only = True
            status_norm = None

        limit = max(1, min(int(limit), 1000))
        offset = max(0, int(offset))
        fetch_cap = limit + offset

        conn_logs: List[Dict[str, Any]] = []
        event_logs: List[Dict[str, Any]] = []
        transfer_logs: List[Dict[str, Any]] = []
        counts = {"connection": 0, "event": 0, "transfer": 0}

        with self._lock:
            # 1. audit_connection_log
            if normalized_type in (None, "connection") and not resumed_only:
                where: List[str] = []
                params: List[Any] = []
                if device_id:
                    where.append("(device_id = ? OR device_name = ?)")
                    params.extend([device_id, device_id])
                if status_norm:
                    if status_norm == "FAILED":
                        where.append("status != 'OK'")
                    elif status_norm == "VERIFIED_OK":
                        where.append("status = 'OK'")
                    else:
                        where.append("status = ?")
                        params.append(status_norm)
                if start_ms is not None:
                    where.append("timestamp_ms >= ?")
                    params.append(int(start_ms))
                if end_ms is not None:
                    where.append("timestamp_ms <= ?")
                    params.append(int(end_ms))
                if kw_like:
                    where.append(
                        "(device_id LIKE ? OR device_name LIKE ? OR action LIKE ? OR "
                        "COALESCE(lan_endpoint, '') LIKE ? OR status LIKE ? OR COALESCE(detail, '') LIKE ?)"
                    )
                    params.extend([kw_like] * 6)

                clause = (" WHERE " + " AND ".join(where)) if where else ""
                cnt_row = self._conn.execute(
                    f"SELECT COUNT(*) AS c FROM audit_connection_log{clause}", params
                ).fetchone()
                counts["connection"] = int(cnt_row["c"]) if cnt_row else 0
                if normalized_type == "connection":
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_connection_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ? OFFSET ?",
                        params + [limit, offset],
                    ).fetchall()
                else:
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_connection_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ?",
                        params + [fetch_cap],
                    ).fetchall()
                for r in rows:
                    item = dict(r)
                    item["record_type"] = "CONNECTION"
                    conn_logs.append(item)

            # 2. audit_event_log
            if normalized_type in (None, "event") and not resumed_only:
                where = []
                params = []
                if device_id:
                    where.append("(source_device_id = ? OR target_device_ids_json LIKE ?)")
                    params.extend([device_id, f'%"{device_id}"%'])
                if status_norm:
                    if status_norm == "FAILED":
                        # 事件表无失败状态，保留 0 命中
                        where.append("1 = 0")
                    elif status_norm != "VERIFIED_OK":
                        where.append("content_category = ?")
                        params.append(status_norm)
                if start_ms is not None:
                    where.append("timestamp_ms >= ?")
                    params.append(int(start_ms))
                if end_ms is not None:
                    where.append("timestamp_ms <= ?")
                    params.append(int(end_ms))
                if kw_like:
                    where.append(
                        "(event_id LIKE ? OR topic LIKE ? OR event_type LIKE ? OR "
                        "source_device_id LIKE ? OR target_device_ids_json LIKE ? OR content_hash LIKE ?)"
                    )
                    params.extend([kw_like] * 6)

                clause = (" WHERE " + " AND ".join(where)) if where else ""
                cnt_row = self._conn.execute(
                    f"SELECT COUNT(*) AS c FROM audit_event_log{clause}", params
                ).fetchone()
                counts["event"] = int(cnt_row["c"]) if cnt_row else 0
                if normalized_type == "event":
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_event_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ? OFFSET ?",
                        params + [limit, offset],
                    ).fetchall()
                else:
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_event_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ?",
                        params + [fetch_cap],
                    ).fetchall()
                for r in rows:
                    item = dict(r)
                    item["target_device_ids"] = json.loads(item.pop("target_device_ids_json", "[]"))
                    item["record_type"] = "EVENT"
                    event_logs.append(item)

            # 3. audit_transfer_log
            if normalized_type in (None, "transfer"):
                where = []
                params = []
                if device_id:
                    where.append("(source_device_id = ? OR target_device_id = ?)")
                    params.extend([device_id, device_id])
                if resumed_only:
                    where.append("resumed = 1")
                if status_norm:
                    if status_norm == "FAILED":
                        where.append("verification_status != 'VERIFIED_OK'")
                    else:
                        where.append("verification_status = ?")
                        params.append(status_norm)
                if start_ms is not None:
                    where.append("timestamp_ms >= ?")
                    params.append(int(start_ms))
                if end_ms is not None:
                    where.append("timestamp_ms <= ?")
                    params.append(int(end_ms))
                if kw_like:
                    where.append(
                        "(transfer_id LIKE ? OR file_cache_id LIKE ? OR source_device_id LIKE ? OR "
                        "target_device_id LIKE ? OR channel_used LIKE ? OR content_hash LIKE ? OR verification_status LIKE ?)"
                    )
                    params.extend([kw_like] * 7)

                clause = (" WHERE " + " AND ".join(where)) if where else ""
                cnt_row = self._conn.execute(
                    f"SELECT COUNT(*) AS c FROM audit_transfer_log{clause}", params
                ).fetchone()
                counts["transfer"] = int(cnt_row["c"]) if cnt_row else 0
                if normalized_type == "transfer":
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_transfer_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ? OFFSET ?",
                        params + [limit, offset],
                    ).fetchall()
                else:
                    rows = self._conn.execute(
                        f"SELECT * FROM audit_transfer_log{clause} ORDER BY timestamp_ms DESC, id DESC LIMIT ?",
                        params + [fetch_cap],
                    ).fetchall()
                for r in rows:
                    item = dict(r)
                    item["resumed"] = bool(item["resumed"])
                    item["record_type"] = "TRANSFER"
                    transfer_logs.append(item)

        if normalized_type is None:
            merged = conn_logs + event_logs + transfer_logs
            merged.sort(key=lambda x: (int(x.get("timestamp_ms", 0)), int(x.get("id", 0))), reverse=True)
            items = merged[offset : offset + limit]
        elif normalized_type == "connection":
            items = conn_logs
        elif normalized_type == "event":
            items = event_logs
        else:
            items = transfer_logs

        query_ms = round((time.perf_counter() - t0) * 1000.0, 2)
        payload: Dict[str, Any] = {
            "items": items,
            "total": counts["connection"] + counts["event"] + counts["transfer"],
            "total_counts": counts,
            "limit": limit,
            "offset": offset,
            "query_ms": query_ms,
        }
        if normalized_type in (None, "connection"):
            payload["connection_logs"] = conn_logs
        if normalized_type in (None, "event"):
            payload["event_logs"] = event_logs
        if normalized_type in (None, "transfer"):
            payload["transfer_logs"] = transfer_logs
        return payload

    def get_audit_summary(self) -> Dict[str, Any]:
        with self._lock:
            c_cnt = int(self._conn.execute("SELECT COUNT(*) FROM audit_connection_log").fetchone()[0])
            e_cnt = int(self._conn.execute("SELECT COUNT(*) FROM audit_event_log").fetchone()[0])
            t_row = self._conn.execute(
                """
                SELECT
                    COUNT(*) AS total,
                    SUM(CASE WHEN verification_status = 'VERIFIED_OK' THEN 1 ELSE 0 END) AS ok_cnt,
                    SUM(CASE WHEN verification_status != 'VERIFIED_OK' THEN 1 ELSE 0 END) AS fail_cnt,
                    SUM(CASE WHEN resumed = 1 THEN 1 ELSE 0 END) AS resumed_cnt,
                    SUM(CASE WHEN resumed = 1 THEN resume_offset ELSE 0 END) AS resumed_saved_bytes,
                    SUM(CASE WHEN channel_used = 'LAN_DIRECT' THEN 1 ELSE 0 END) AS lan_cnt,
                    SUM(CASE WHEN channel_used = 'CLOUD_CACHE' THEN 1 ELSE 0 END) AS cloud_cnt,
                    COALESCE(SUM(bytes_transferred), 0) AS total_bytes
                FROM audit_transfer_log
                """
            ).fetchone()
            t_total = int(t_row["total"] or 0)
            ok_cnt = int(t_row["ok_cnt"] or 0)
            fail_cnt = int(t_row["fail_cnt"] or 0)
            pass_rate = 100.0 if t_total == 0 else round((ok_cnt / t_total) * 100.0, 2)
            return {
                "total_connections": c_cnt,
                "total_events": e_cnt,
                "total_transfers": t_total,
                "verified_ok_count": ok_cnt,
                "hash_mismatch_count": fail_cnt,
                "blake3_pass_rate": pass_rate,
                "resumed_transfer_count": int(t_row["resumed_cnt"] or 0),
                "resumed_bytes_saved": int(t_row["resumed_saved_bytes"] or 0),
                "lan_transfer_count": int(t_row["lan_cnt"] or 0),
                "cloud_transfer_count": int(t_row["cloud_cnt"] or 0),
                "total_transfer_bytes": int(t_row["total_bytes"] or 0),
            }

    def get_recent_channel_activity(self, window_ms: int = 300_000, limit: int = 20) -> List[Dict[str, Any]]:
        """基于审计记录提取近期设备间数据传输通道信息 (LAN_DIRECT / CLOUD_CACHE / 断网兜底)."""
        cutoff = _now_ms() - window_ms
        with self._lock:
            rows = self._conn.execute(
                """
                SELECT timestamp_ms, transfer_id, file_cache_id, source_device_id, target_device_id,
                       channel_used, resumed, resume_offset, resume_count, bytes_transferred, verification_status
                FROM audit_transfer_log
                WHERE timestamp_ms >= ?
                ORDER BY timestamp_ms DESC, id DESC
                LIMIT ?
                """,
                (cutoff, limit),
            ).fetchall()
            out: List[Dict[str, Any]] = []
            for r in rows:
                item = dict(r)
                item["resumed"] = bool(item["resumed"])
                out.append(item)
            return out

    def get_db_stats(self) -> Dict[str, Any]:
        with self._lock:
            jm = self._conn.execute("PRAGMA journal_mode;").fetchone()
            sync = self._conn.execute("PRAGMA synchronous;").fetchone()
            c_cnt = int(self._conn.execute("SELECT COUNT(*) FROM audit_connection_log").fetchone()[0])
            e_cnt = int(self._conn.execute("SELECT COUNT(*) FROM audit_event_log").fetchone()[0])
            t_cnt = int(self._conn.execute("SELECT COUNT(*) FROM audit_transfer_log").fetchone()[0])

        db_size = self.db_path.stat().st_size if (not self._is_memory and self.db_path.exists()) else 0
        wal_path = Path(str(self.db_path) + "-wal")
        wal_size = wal_path.stat().st_size if (not self._is_memory and wal_path.exists()) else 0
        sync_map = {0: "OFF", 1: "NORMAL", 2: "FULL", 3: "EXTRA"}
        sync_val = sync[0] if sync else 1
        return {
            "db_path": ":memory:" if self._is_memory else str(self.db_path),
            "journal_mode": str(jm[0]).upper() if jm else "WAL",
            "synchronous": sync_map.get(int(sync_val), str(sync_val)) if str(sync_val).isdigit() else str(sync_val),
            "db_size_bytes": db_size,
            "wal_size_bytes": wal_size,
            "retention_days": self.retention_days,
            "row_counts": {
                "audit_connection_log": c_cnt,
                "audit_event_log": e_cnt,
                "audit_transfer_log": t_cnt,
            },
        }

    def wal_checkpoint(self, mode: str = "TRUNCATE") -> Dict[str, Any]:
        safe_mode = mode.upper()
        if safe_mode not in ("PASSIVE", "FULL", "RESTART", "TRUNCATE"):
            safe_mode = "TRUNCATE"
        with self._lock:
            row = self._conn.execute(f"PRAGMA wal_checkpoint({safe_mode});").fetchone()
            self._conn.commit()
        stats = self.get_db_stats()
        stats["checkpoint_mode"] = safe_mode
        stats["checkpoint_result"] = list(row) if row is not None else [0, 0, 0]
        return stats

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

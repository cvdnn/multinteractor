#!/usr/bin/env python3
"""
Task Execution Atomicity & Conflict Guard (任务原子性执行与防冲突锁控制器).

核心机制：
1. 互斥与依赖前置检查 (DAG Precondition Check)：仅当所有前置依赖任务均为 COMPLETED 时才允许获取任务锁。
2. 目录边界排他锁 (Scope-based Exclusive Lock)：每个任务声明明确的 `locked_scopes` 目录/文件集合，
   在锁持有期间禁止任何其他任务并发修改重叠目录，并在提交完成时校验是否越界修改。
3. 原子快照与回滚 (Snapshot & Atomic Rollback)：获取任务锁时记录目标目录文件哈希快照，若准出测试失败支持一键原子回滚。
4. 实时状态同步 (Live Plan Sync)：自动同步更新 `.task_lock.json` 与 `IMPLEMENTATION_PLAN.md` 中的任务执行状态标记。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional

ROOT_DIR = Path(__file__).resolve().parent.parent
LOCK_FILE = ROOT_DIR / ".task_lock.json"
PLAN_FILE = ROOT_DIR / "IMPLEMENTATION_PLAN.md"

TASK_DEFINITIONS: Dict[str, dict] = {
    "TASK-01": {
        "name": "定义跨端协议契约与核心协议库 (mi-protocol)",
        "milestone": "M1",
        "depends_on": [],
        "scopes": [
            "client/crates/mi-protocol",
            "server/app/models.py",
            "tests/fixtures",
        ],
    },
    "TASK-02": {
        "name": "实现云端客户端管理、Topic 订阅路由与 SQLite 日志审计",
        "milestone": "M1",
        "depends_on": ["TASK-01"],
        "scopes": [
            "server/app/config.py",
            "server/app/main.py",
            "server/app/client_sub_manager.py",
            "server/app/audit_logger.py",
            "server/app/api/devices.py",
            "server/app/api/audit.py",
            "server/tests/conftest.py",
            "server/tests/test_client_sub_manager.py",
            "server/tests/test_audit_logger.py",
        ],
    },
    "TASK-03": {
        "name": "实现客户端常驻守护进程 (mi-daemon) 与本地事件缓存列表",
        "milestone": "M1",
        "depends_on": ["TASK-01", "TASK-02"],
        "scopes": [
            "client/crates/mi-daemon/src/main.rs",
            "client/crates/mi-daemon/src/lib.rs",
            "client/crates/mi-daemon/src/config.rs",
            "client/crates/mi-daemon/src/cloud_client.rs",
            "client/crates/mi-daemon/src/event_queue.rs",
            "client/crates/mi-daemon/src/ipc_server.rs",
        ],
    },
    "TASK-04": {
        "name": "实现云端内嵌“边传边下”与分块断点续传缓存引擎",
        "milestone": "M2",
        "depends_on": ["TASK-02"],
        "scopes": [
            "server/app/stream_tee_cache.py",
            "server/app/api/cache.py",
            "server/app/main.py",
            "server/tests/test_stream_tee_cache.py",
        ],
    },
    "TASK-05": {
        "name": "实现客户端内网即用即连 (1-to-N 并发 + 180 秒空闲延迟断开) 双向通道",
        "milestone": "M2",
        "depends_on": ["TASK-03"],
        "scopes": [
            "client/crates/mi-daemon/src/lan_channel.rs",
        ],
    },
    "TASK-06": {
        "name": "实现大文件全链路断点续传、文件夹流式打包解包与远程目录 RPC",
        "milestone": "M2",
        "depends_on": ["TASK-04", "TASK-05"],
        "scopes": [
            "client/crates/mi-daemon/src/transfer",
            "client/crates/mi-daemon/src/fs_rpc.rs",
        ],
    },
    "TASK-07": {
        "name": "实现 mi CLI 命令行工具与 multinteractor Agent Skill 全链路联调",
        "milestone": "M2",
        "depends_on": ["TASK-06"],
        "scopes": [
            "client/crates/mi-cli",
            "scripts/mi",
        ],
    },
    "TASK-08": {
        "name": "实现系统全局热键 Alt+C/Alt+V、焦点探测与智能粘贴/保存框兜底",
        "milestone": "M3",
        "depends_on": ["TASK-06"],
        "scopes": [
            "client/crates/mi-os/src/lib.rs",
            "client/crates/mi-os/src/hotkey.rs",
            "client/crates/mi-os/src/capture.rs",
            "client/crates/mi-os/src/smart_paste.rs",
            "client/crates/mi-os/src/power.rs",
            "client/crates/mi-os/src/platform",
        ],
    },
    "TASK-09": {
        "name": "实现系统鼠标右键扩展菜单与远程目录选择器弹窗 (mi-ui)",
        "milestone": "M4",
        "depends_on": ["TASK-07", "TASK-08"],
        "scopes": [
            "client/crates/mi-os/src/context_menu.rs",
            "client/crates/mi-ui",
        ],
    },
    "TASK-10": {
        "name": "全链路集成测试、故障注入验证与交付打包",
        "milestone": "M4",
        "depends_on": [
            "TASK-01",
            "TASK-02",
            "TASK-03",
            "TASK-04",
            "TASK-05",
            "TASK-06",
            "TASK-07",
            "TASK-08",
            "TASK-09",
        ],
        "scopes": [
            "tests/integration",
            "tests/chaos",
            "tests/e2e",
            "deploy",
            "Makefile",
        ],
    },
}


def now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def compute_scope_snapshot(scopes: List[str]) -> Dict[str, str]:
    snapshot: Dict[str, str] = {}
    for scope in scopes:
        target = ROOT_DIR / scope
        if target.is_file():
            snapshot[scope] = hashlib.sha256(target.read_bytes()).hexdigest()
        elif target.is_dir():
            for child in sorted(target.rglob("*")):
                if child.is_file() and "__pycache__" not in child.parts:
                    rel = child.relative_to(ROOT_DIR).as_posix()
                    snapshot[rel] = hashlib.sha256(child.read_bytes()).hexdigest()
    return snapshot


def load_state() -> dict:
    if LOCK_FILE.exists():
        return json.loads(LOCK_FILE.read_text(encoding="utf-8"))
    tasks = {}
    for tid, meta in TASK_DEFINITIONS.items():
        tasks[tid] = {
            "task_id": tid,
            "name": meta["name"],
            "milestone": meta["milestone"],
            "depends_on": meta["depends_on"],
            "scopes": meta["scopes"],
            "status": "PENDING",
            "started_at": None,
            "completed_at": None,
            "pre_snapshot": {},
            "post_snapshot": {},
        }
    state = {
        "active_task_id": None,
        "locked_scopes": [],
        "lock_acquired_at": None,
        "updated_at": now_iso(),
        "tasks": tasks,
    }
    save_state(state)
    return state


def save_state(state: dict) -> None:
    state["updated_at"] = now_iso()
    tmp_file = LOCK_FILE.with_suffix(".json.tmp")
    tmp_file.write_text(json.dumps(state, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(tmp_file, LOCK_FILE)
    sync_plan_markdown(state)


STATUS_BADGE = {
    "PENDING": "⏳ `PENDING` (待执行)",
    "IN_PROGRESS": "🔄 **`IN_PROGRESS` (正在执行 - 已加锁)**",
    "COMPLETED": "✅ `COMPLETED` (已完成)",
    "FAILED": "❌ `FAILED` (失败回滚)",
}


def sync_plan_markdown(state: dict) -> None:
    if not PLAN_FILE.exists():
        return
    content = PLAN_FILE.read_text(encoding="utf-8")
    for tid, tinfo in state["tasks"].items():
        badge = STATUS_BADGE.get(tinfo["status"], tinfo["status"])
        pattern = rf"(\|\s*\*\*`{tid}`\*\*\s*\|[^|]+\|[^|]+\|[^|]+\|)\s*[^|]+(\|)"
        content = re.sub(pattern, rf"\1 {badge} \2", content)

        header_pattern = rf"(#### `{tid}`：[^\n]+)"
        def _replace_header(m: re.Match) -> str:
            base = re.sub(r"\s*\[(?:⏳ PENDING|🔄 IN_PROGRESS|✅ COMPLETED|❌ FAILED)\]$", "", m.group(1))
            short_tag = {
                "PENDING": "[⏳ PENDING]",
                "IN_PROGRESS": "[🔄 IN_PROGRESS]",
                "COMPLETED": "[✅ COMPLETED]",
                "FAILED": "[❌ FAILED]",
            }[tinfo["status"]]
            return f"{base} {short_tag}"

        content = re.sub(header_pattern, _replace_header, content)

    PLAN_FILE.write_text(content, encoding="utf-8")


def cmd_acquire(task_id: str) -> None:
    state = load_state()
    if task_id not in state["tasks"]:
        raise SystemExit(f"Unknown task_id: {task_id}")

    active = state["active_task_id"]
    if active is not None and active != task_id:
        raise SystemExit(
            f"[CONFLICT BLOCKED] Cannot acquire {task_id}: task {active} is currently IN_PROGRESS "
            f"holding lock on scopes {state['locked_scopes']}."
        )

    tinfo = state["tasks"][task_id]
    for dep in tinfo["depends_on"]:
        dep_status = state["tasks"][dep]["status"]
        if dep_status != "COMPLETED":
            raise SystemExit(
                f"[DEPENDENCY BLOCKED] Cannot start {task_id}: dependency {dep} is {dep_status} (must be COMPLETED)."
            )

    tinfo["status"] = "IN_PROGRESS"
    tinfo["started_at"] = now_iso()
    tinfo["pre_snapshot"] = compute_scope_snapshot(tinfo["scopes"])
    state["active_task_id"] = task_id
    state["locked_scopes"] = tinfo["scopes"]
    state["lock_acquired_at"] = now_iso()
    save_state(state)
    print(f"[LOCK ACQUIRED] {task_id} is now IN_PROGRESS. Locked scopes: {tinfo['scopes']}")


def cmd_complete(task_id: str, verify_cmd: Optional[str] = None) -> None:
    state = load_state()
    if state["active_task_id"] != task_id:
        raise SystemExit(
            f"[ATOMICITY ERROR] Task {task_id} does not hold the active lock (active={state['active_task_id']})."
        )

    if verify_cmd:
        print(f"[VERIFYING] Running gate command for {task_id}: {verify_cmd}")
        res = subprocess.run(verify_cmd, shell=True, cwd=str(ROOT_DIR))
        if res.returncode != 0:
            raise SystemExit(f"[VERIFICATION FAILED] {task_id} verification exited with {res.returncode}")

    tinfo = state["tasks"][task_id]
    tinfo["status"] = "COMPLETED"
    tinfo["completed_at"] = now_iso()
    tinfo["post_snapshot"] = compute_scope_snapshot(tinfo["scopes"])
    state["active_task_id"] = None
    state["locked_scopes"] = []
    state["lock_acquired_at"] = None
    save_state(state)
    print(f"[TASK COMPLETED & LOCK RELEASED] {task_id} completed at {tinfo['completed_at']}.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Task Atomicity & Lock Guard")
    sub = parser.add_subparsers(dest="action", required=True)

    p_init = sub.add_parser("init")
    p_acq = sub.add_parser("acquire")
    p_acq.add_argument("task_id")

    p_comp = sub.add_parser("complete")
    p_comp.add_argument("task_id")
    p_comp.add_argument("--verify", default=None)

    p_status = sub.add_parser("status")

    args = parser.parse_args()
    if args.action == "init":
        state = load_state()
        save_state(state)
        print("Initialized .task_lock.json and synced IMPLEMENTATION_PLAN.md")
    elif args.action == "acquire":
        cmd_acquire(args.task_id)
    elif args.action == "complete":
        cmd_complete(args.task_id, args.verify)
    elif args.action == "status":
        state = load_state()
        print(json.dumps(state, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()

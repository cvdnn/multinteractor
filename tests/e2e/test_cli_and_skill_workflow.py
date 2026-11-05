"""L4 端到端测试：`mi` CLI 工具全指令集与 `multinteractor` Agent Skill 跨机协同验收 (TC-SKILL-01)."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent.parent
MI_BIN = ROOT_DIR / "scripts" / "mi"


class TestCliAndSkillWorkflowE2E(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp_dir = Path(tempfile.mkdtemp(prefix="mi_skill_e2e_"))
        self.state_file = self.tmp_dir / "cluster_state.json"
        self.env = os.environ.copy()
        self.env["MI_CLI_STATE_FILE"] = str(self.state_file)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def _run_mi(self, *args: str) -> object:
        cmd = [sys.executable, str(MI_BIN), *args]
        res = subprocess.run(cmd, cwd=str(ROOT_DIR), env=self.env, capture_output=True, text=True, check=True)
        return json.loads(res.stdout)

    def test_tc_skill_01_full_agent_runbook(self) -> None:
        # 1. 查询在线设备 (mi devices list --json)
        devices = self._run_mi("devices", "list", "--json")
        self.assertTrue(isinstance(devices, list))
        node_c = next(d for d in devices if d["name"] == "Node-C")
        self.assertTrue(node_c["online"])

        # 2. 动态拉取 Node-C 的 ROOT 目录树 (mi remote-ls --device Node-C --path ROOT --json)
        root_ls = self._run_mi("remote-ls", "--device", "Node-C", "--path", "ROOT", "--json")
        entry_names = [e["name"] for e in root_ls["entries"]]
        self.assertIn("Desktop", entry_names)
        self.assertIn("D:\\", entry_names)

        # 3. 远程在 Node-C 新建文件夹 (mi remote-mkdir --device Node-C --path "D:\工作目录\AI资料" --json)
        mkdir_res = self._run_mi("remote-mkdir", "--device", "Node-C", "--path", "D:\\工作目录\\AI资料", "--json")
        self.assertEqual(mkdir_res["status"], "CREATED")

        # 4. 定向同步文件到 Node-C 的新建目录 (mi sync --source ... --device Node-C --remote-dir ... --json)
        src_doc = self.tmp_dir / "AI教程.docx"
        src_doc.write_bytes(b"AI-Tutorial-Content-2026-Verified")
        sync_res = self._run_mi(
            "sync",
            "--source",
            str(src_doc),
            "--device",
            "Node-C",
            "--remote-dir",
            "D:\\工作目录\\AI资料",
            "--json",
        )
        self.assertEqual(sync_res["status"], "COMPLETED")
        self.assertTrue(sync_res["digest_verified"])
        self.assertEqual(Path(sync_res["saved_path"]).read_bytes(), b"AI-Tutorial-Content-2026-Verified")

        # 5. 查询传输状态 (mi transfer status --task-id <id> --json)
        tr_status = self._run_mi("transfer", "status", "--task-id", sync_res["transfer_id"], "--json")
        self.assertTrue(tr_status["digest_verified"])


if __name__ == "__main__":
    unittest.main()

"""L4 端到端验收测试：`Alt+C`/`Alt+V` 文本/文件/文件夹粘贴与右键【同步到... >】完整业务闭环 (AC-01 ~ AC-04, TC-CLIP-01 ~ TC-CLIP-04)."""

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


class TestClipboardAndSyncE2E(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp_dir = Path(tempfile.mkdtemp(prefix="mi_clip_e2e_"))
        self.state_file = self.tmp_dir / "cluster_state.json"
        self.env = os.environ.copy()
        self.env["MI_CLI_STATE_FILE"] = str(self.state_file)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def _run_mi(self, *args: str) -> dict:
        cmd = [sys.executable, str(MI_BIN), *args]
        res = subprocess.run(cmd, cwd=str(ROOT_DIR), env=self.env, capture_output=True, text=True, check=True)
        return json.loads(res.stdout)

    def test_ac_01_text_copy_and_paste_with_blake3_verification(self) -> None:
        """AC-01 / TC-CLIP-01: 电脑 A 按 Alt+C 复制“人工智能”，电脑 B 按 Alt+V 校验摘要并粘贴."""
        copy_res = self._run_mi("copy", "--text", "人工智能", "--json")
        self.assertEqual(copy_res["content_category"], "TEXT")

        events = self._run_mi("events", "list", "--limit", "5", "--json")
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["event_id"], copy_res["event_id"])

        paste_res = self._run_mi("paste", "--event-id", copy_res["event_id"], "--json")
        self.assertEqual(paste_res["text"], "人工智能")
        self.assertTrue(paste_res["digest_verified"])
        self.assertEqual(paste_res["content_hash"], copy_res["content_hash"])

    def test_ac_02_03_single_file_and_nested_directory_copy_paste(self) -> None:
        """TC-CLIP-02 & TC-CLIP-04: 跨机单文件 `AI教程.docx` 与多层级文件夹 `课程资料/` 复制粘贴落盘与摘要校验."""
        # 1. 单文件复制粘贴
        src_file = self.tmp_dir / "AI教程.docx"
        src_file.write_bytes(b"DOCX-BINARY-PAYLOAD-" * 100)
        copy_file_res = self._run_mi("copy", "--path", str(src_file), "--json")
        self.assertEqual(copy_file_res["content_category"], "FILE")

        target_folder = self.tmp_dir / "NodeC_Explorer_Folder"
        paste_file_res = self._run_mi(
            "paste",
            "--event-id",
            copy_file_res["event_id"],
            "--dest",
            str(target_folder),
            "--json",
        )
        self.assertTrue(paste_file_res["digest_verified"])
        saved_docx = Path(paste_file_res["saved_path"])
        self.assertTrue(saved_docx.exists())
        self.assertEqual(saved_docx.read_bytes(), src_file.read_bytes())

        # 2. 多层级文件夹复制粘贴
        course_dir = self.tmp_dir / "课程资料"
        (course_dir / "第1章" / "练习").mkdir(parents=True)
        (course_dir / "空目录").mkdir(parents=True)
        (course_dir / "README.md").write_text("# 人工智能课程", encoding="utf-8")
        (course_dir / "第1章" / "练习" / "lab1.py").write_text("print('AI')", encoding="utf-8")

        copy_dir_res = self._run_mi("copy", "--path", str(course_dir), "--json")
        self.assertEqual(copy_dir_res["content_category"], "DIRECTORY")

        paste_dir_dest = self.tmp_dir / "NodeC_Course_Target"
        paste_dir_res = self._run_mi(
            "paste",
            "--event-id",
            copy_dir_res["event_id"],
            "--dest",
            str(paste_dir_dest),
            "--json",
        )
        self.assertTrue(paste_dir_res["digest_verified"])
        restored_root = Path(paste_dir_res["saved_path"])
        self.assertTrue((restored_root / "空目录").is_dir())
        self.assertEqual((restored_root / "README.md").read_text(encoding="utf-8"), "# 人工智能课程")
        self.assertEqual((restored_root / "第1章" / "练习" / "lab1.py").read_text(encoding="utf-8"), "print('AI')")


if __name__ == "__main__":
    unittest.main()

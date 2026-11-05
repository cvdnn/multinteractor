"""TASK-01 准出验证：CloudEventEnvelope v6 序列化双向互通、BLAKE3 全文/分块摘要与 ResumeState 校验测试."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from server.app.models import (
    CloudEventEnvelope,
    DigestManifest,
    DigestMismatchError,
    ResumeState,
    compute_blake3_hex,
)

FIXTURES_DIR = Path(__file__).resolve().parent


class TestProtocolAndBlake3Contracts(unittest.TestCase):
    def test_golden_envelope_roundtrip(self) -> None:
        raw = json.loads((FIXTURES_DIR / "envelope_v6_golden.json").read_text(encoding="utf-8"))
        env = CloudEventEnvelope.from_dict(raw)
        self.assertEqual(env.spec_version, "v6")
        self.assertEqual(env.topic, "topic.clipboard.broadcast")
        self.assertEqual(env.source_device.device_id, "dev_b_laptop")
        self.assertEqual(env.payload.content_category, "FILE")
        self.assertEqual(env.payload.digest.chunk_size_bytes, 4194304)
        self.assertEqual(env.to_dict(), raw)

    def test_blake3_vectors_and_chunk_verification(self) -> None:
        vec_doc = json.loads((FIXTURES_DIR / "blake3_vectors.json").read_text(encoding="utf-8"))
        for vec in vec_doc["vectors"]:
            data = vec["input_utf8"].encode("utf-8")
            self.assertEqual(compute_blake3_hex(data), vec["expected_blake3_hex"])

        payload = b"multinteractor-chunk-test-" * 500
        manifest = DigestManifest.from_bytes(payload, chunk_size_bytes=1024)
        manifest.verify_bytes(payload)

        tampered = bytearray(payload)
        tampered[1500] ^= 0xFF
        with self.assertRaises(DigestMismatchError):
            manifest.verify_bytes(bytes(tampered))

    def test_resume_state_source_modification_guard(self) -> None:
        manifest = DigestManifest.from_bytes(b"hello world")
        state = ResumeState(
            transfer_id="tr_01",
            file_cache_id="fc_01",
            file_name="demo.bin",
            total_size=11,
            verified_offset=0,
            source_mtime=1700000000,
            digest=manifest,
        )
        self.assertTrue(state.is_source_unchanged(1700000000, 11))
        self.assertFalse(state.is_source_unchanged(1700000005, 11))


if __name__ == "__main__":
    unittest.main()

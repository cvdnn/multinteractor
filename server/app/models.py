"""
协议数据契约模型与 BLAKE3 摘要计算/校验器 (TASK-01).

严格对齐 `CloudEventEnvelope` (v6)、全文与 4MB 分块 `BLAKE3` 摘要结构 (`DigestManifest`)、
断点续传状态模型 (`ResumeState`) 以及远程目录浏览 RPC (`FS_LIST_DIR` / `FS_MKDIR`).
内置遵循官方 BLAKE3 规范 (1024B Block / 7-Round G-Function / Binary Merkle Tree) 的纯 Python 实现，
当环境安装有 C 扩展 `blake3` 时自动加速，确保任何环境下与 Rust `blake3` 哈希值 100% 比特级一致。
"""

from __future__ import annotations

import struct
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

try:
    import blake3 as _c_blake3  # type: ignore
except ImportError:
    _c_blake3 = None

SPEC_VERSION_V6 = "v6"
DEFAULT_CHUNK_SIZE_BYTES = 4 * 1024 * 1024  # 4 MB

TOPIC_CLIPBOARD_BROADCAST = "topic.clipboard.broadcast"
TOPIC_SYSTEM_PRESENCE = "topic.system.presence"


def topic_sync_direct(device_id: str) -> str:
    return f"topic.sync.direct.{device_id}"


def topic_rpc_fs(device_id: str) -> str:
    return f"topic.rpc.fs.{device_id}"


# ============================================================================
# Official BLAKE3 Reference Implementation (Pure Python + C Extension Support)
# ============================================================================

_IV = [
    0x6A09E667,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
]

_MSG_PERMUTATION = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8]

_CHUNK_START = 1 << 0
_CHUNK_END = 1 << 1
_PARENT = 1 << 2
_ROOT = 1 << 3
_BLOCK_LEN = 64
_CHUNK_LEN = 1024
_MASK32 = 0xFFFFFFFF


def _rotr32(x: int, n: int) -> int:
    return ((x >> n) | ((x << (32 - n)) & _MASK32)) & _MASK32


def _g(state: List[int], a: int, b: int, c: int, d: int, mx: int, my: int) -> None:
    state[a] = (state[a] + state[b] + mx) & _MASK32
    state[d] = _rotr32(state[d] ^ state[a], 16)
    state[c] = (state[c] + state[d]) & _MASK32
    state[b] = _rotr32(state[b] ^ state[c], 12)
    state[a] = (state[a] + state[b] + my) & _MASK32
    state[d] = _rotr32(state[d] ^ state[a], 8)
    state[c] = (state[c] + state[d]) & _MASK32
    state[b] = _rotr32(state[b] ^ state[c], 7)


def _round(state: List[int], m: List[int]) -> None:
    _g(state, 0, 4, 8, 12, m[0], m[1])
    _g(state, 1, 5, 9, 13, m[2], m[3])
    _g(state, 2, 6, 10, 14, m[4], m[5])
    _g(state, 3, 7, 11, 15, m[6], m[7])
    _g(state, 0, 5, 10, 15, m[8], m[9])
    _g(state, 1, 6, 11, 12, m[10], m[11])
    _g(state, 2, 7, 8, 13, m[12], m[13])
    _g(state, 3, 4, 9, 14, m[14], m[15])


def _compress(
    chaining_value: List[int],
    block_words: List[int],
    counter: int,
    block_len: int,
    flags: int,
) -> List[int]:
    state = [
        chaining_value[0],
        chaining_value[1],
        chaining_value[2],
        chaining_value[3],
        chaining_value[4],
        chaining_value[5],
        chaining_value[6],
        chaining_value[7],
        _IV[0],
        _IV[1],
        _IV[2],
        _IV[3],
        counter & _MASK32,
        (counter >> 32) & _MASK32,
        block_len & _MASK32,
        flags & _MASK32,
    ]
    m = list(block_words)
    for r in range(7):
        _round(state, m)
        if r < 6:
            m = [m[i] for i in _MSG_PERMUTATION]
    for i in range(8):
        state[i] ^= state[i + 8]
        state[i + 8] ^= chaining_value[i]
    return state


def _words_from_little_endian_bytes(b: bytes) -> List[int]:
    padded = b + b"\x00" * (_BLOCK_LEN - len(b)) if len(b) < _BLOCK_LEN else b[:_BLOCK_LEN]
    return list(struct.unpack("<16I", padded))


def _chunk_output(chunk_bytes: bytes, chunk_counter: int, flags: int) -> Tuple[List[int], List[int], int, int, int]:
    cv = list(_IV)
    if not chunk_bytes:
        return cv, _words_from_little_endian_bytes(b""), chunk_counter, 0, flags | _CHUNK_START | _CHUNK_END

    num_blocks = (len(chunk_bytes) + _BLOCK_LEN - 1) // _BLOCK_LEN
    for idx in range(num_blocks):
        start = idx * _BLOCK_LEN
        block = chunk_bytes[start : start + _BLOCK_LEN]
        block_words = _words_from_little_endian_bytes(block)
        block_flags = flags
        if idx == 0:
            block_flags |= _CHUNK_START
        if idx == num_blocks - 1:
            block_flags |= _CHUNK_END
            return cv, block_words, chunk_counter, len(block), block_flags
        cv = _compress(cv, block_words, chunk_counter, len(block), block_flags)[:8]
    raise RuntimeError("unreachable")


def _output_chaining_value(out_tuple: Tuple[List[int], List[int], int, int, int]) -> List[int]:
    cv, bw, counter, blen, bflags = out_tuple
    return _compress(cv, bw, counter, blen, bflags)[:8]


def _output_root_hex(out_tuple: Tuple[List[int], List[int], int, int, int]) -> str:
    cv, bw, _, blen, bflags = out_tuple
    words = _compress(cv, bw, 0, blen, bflags | _ROOT)[:8]
    return struct.pack("<8I", *words).hex()


def compute_blake3_hex(data: bytes) -> str:
    """计算字节流的 256-bit BLAKE3 十六进制摘要（与 Rust blake3::hash 100% 一致）."""
    if _c_blake3 is not None:
        return _c_blake3.blake3(data).hexdigest()

    if len(data) <= _CHUNK_LEN:
        out = _chunk_output(data, 0, 0)
        return _output_root_hex(out)

    cv_stack: List[List[int]] = []
    total_chunks = (len(data) + _CHUNK_LEN - 1) // _CHUNK_LEN
    for chunk_counter in range(total_chunks):
        start = chunk_counter * _CHUNK_LEN
        chunk = data[start : start + _CHUNK_LEN]
        out = _chunk_output(chunk, chunk_counter, 0)
        if chunk_counter < total_chunks - 1:
            new_cv = _output_chaining_value(out)
            total_c = chunk_counter + 1
            while (total_c & 1) == 0:
                left_cv = cv_stack.pop()
                parent_block = left_cv + new_cv
                out = (_IV, parent_block, 0, _BLOCK_LEN, _PARENT)
                new_cv = _output_chaining_value(out)
                total_c >>= 1
            cv_stack.append(new_cv)
        else:
            while cv_stack:
                new_cv = _output_chaining_value(out)
                left_cv = cv_stack.pop()
                parent_block = left_cv + new_cv
                out = (_IV, parent_block, 0, _BLOCK_LEN, _PARENT)
            return _output_root_hex(out)
    raise RuntimeError("unreachable")


# ============================================================================
# Protocol Data Models (CloudEventEnvelope v6, DigestManifest, RPC, Resume)
# ============================================================================


class DigestMismatchError(ValueError):
    """当全文或分块 BLAKE3 摘要校验不一致时抛出."""


@dataclass
class DigestManifest:
    algorithm: str
    full_hash: str
    chunk_size_bytes: int
    chunk_hashes: List[str]

    @classmethod
    def from_bytes(cls, data: bytes, chunk_size_bytes: int = DEFAULT_CHUNK_SIZE_BYTES) -> "DigestManifest":
        step = max(1, chunk_size_bytes)
        full_hash = compute_blake3_hex(data)
        chunk_hashes: List[str] = []
        for offset in range(0, len(data), step):
            chunk_hashes.append(compute_blake3_hex(data[offset : offset + step]))
        return cls(
            algorithm="blake3",
            full_hash=full_hash,
            chunk_size_bytes=step,
            chunk_hashes=chunk_hashes,
        )

    @classmethod
    def from_file(cls, path: Path, chunk_size_bytes: int = DEFAULT_CHUNK_SIZE_BYTES) -> "DigestManifest":
        return cls.from_bytes(path.read_bytes(), chunk_size_bytes=chunk_size_bytes)

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "DigestManifest":
        if raw.get("algorithm") != "blake3":
            raise ValueError(f"Unsupported digest algorithm: {raw.get('algorithm')}")
        return cls(
            algorithm=str(raw["algorithm"]),
            full_hash=str(raw["full_hash"]),
            chunk_size_bytes=int(raw.get("chunk_size_bytes", DEFAULT_CHUNK_SIZE_BYTES)),
            chunk_hashes=[str(h) for h in raw.get("chunk_hashes", [])],
        )

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    def verify_chunk(self, chunk_index: int, chunk_data: bytes) -> None:
        if 0 <= chunk_index < len(self.chunk_hashes):
            expected = self.chunk_hashes[chunk_index]
            actual = compute_blake3_hex(chunk_data)
            if actual != expected:
                raise DigestMismatchError(
                    f"Chunk {chunk_index} HASH_MISMATCH: expected={expected}, actual={actual}"
                )

    def verify_bytes(self, data: bytes) -> None:
        step = max(1, self.chunk_size_bytes)
        for idx, offset in enumerate(range(0, len(data), step)):
            self.verify_chunk(idx, data[offset : offset + step])
        actual_full = compute_blake3_hex(data)
        if actual_full != self.full_hash:
            raise DigestMismatchError(
                f"Full payload HASH_MISMATCH: expected={self.full_hash}, actual={actual_full}"
            )


@dataclass
class SourceDevice:
    device_id: str
    device_name: str
    os: str
    lan_endpoint: Optional[str] = None

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "SourceDevice":
        return cls(
            device_id=str(raw["device_id"]),
            device_name=str(raw["device_name"]),
            os=str(raw["os"]),
            lan_endpoint=raw.get("lan_endpoint"),
        )


@dataclass
class TargetScope:
    mode: str  # "BROADCAST" | "DIRECT"
    target_device_ids: List[str] = field(default_factory=lambda: ["*"])

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "TargetScope":
        mode = str(raw["mode"])
        if mode not in ("BROADCAST", "DIRECT"):
            raise ValueError(f"Invalid target_scope.mode: {mode}")
        return cls(mode=mode, target_device_ids=[str(x) for x in raw.get("target_device_ids", ["*"])])


@dataclass
class ItemMeta:
    name: str
    is_dir: bool
    size_bytes: int
    mtime: int

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "ItemMeta":
        return cls(
            name=str(raw["name"]),
            is_dir=bool(raw["is_dir"]),
            size_bytes=int(raw["size_bytes"]),
            mtime=int(raw["mtime"]),
        )


@dataclass
class EventPayload:
    content_category: str  # "TEXT" | "IMAGE" | "FILE" | "DIRECTORY"
    digest: DigestManifest
    inline_data: Optional[str]
    file_cache_id: Optional[str]
    upload_state: str  # "INLINE" | "STREAMING" | "COMPLETED"
    items_meta: List[ItemMeta] = field(default_factory=list)
    destination_override: Optional[str] = None

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "EventPayload":
        cat = str(raw["content_category"])
        if cat not in ("TEXT", "IMAGE", "FILE", "DIRECTORY"):
            raise ValueError(f"Invalid content_category: {cat}")
        return cls(
            content_category=cat,
            digest=DigestManifest.from_dict(raw["digest"]),
            inline_data=raw.get("inline_data"),
            file_cache_id=raw.get("file_cache_id"),
            upload_state=str(raw.get("upload_state", "INLINE")),
            items_meta=[ItemMeta.from_dict(m) for m in raw.get("items_meta", [])],
            destination_override=raw.get("destination_override"),
        )


@dataclass
class CloudEventEnvelope:
    spec_version: str
    event_id: str
    topic: str
    event_type: str
    timestamp_ms: int
    source_device: SourceDevice
    target_scope: TargetScope
    payload: EventPayload

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "CloudEventEnvelope":
        spec = str(raw.get("spec_version", ""))
        if spec != SPEC_VERSION_V6:
            raise ValueError(f"Unsupported spec_version: {spec} (expected {SPEC_VERSION_V6})")
        return cls(
            spec_version=spec,
            event_id=str(raw["event_id"]),
            topic=str(raw["topic"]),
            event_type=str(raw["event_type"]),
            timestamp_ms=int(raw["timestamp_ms"]),
            source_device=SourceDevice.from_dict(raw["source_device"]),
            target_scope=TargetScope.from_dict(raw["target_scope"]),
            payload=EventPayload.from_dict(raw["payload"]),
        )

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class FsEntry:
    name: str
    path: str
    is_dir: bool
    size_bytes: int
    mtime: int
    has_subdirs: bool = False

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class ResumeState:
    transfer_id: str
    file_cache_id: str
    file_name: str
    total_size: int
    verified_offset: int
    source_mtime: int
    digest: DigestManifest
    active_channel: str = "LAN"
    resume_count: int = 0

    def is_source_unchanged(self, current_mtime: int, current_size: int) -> bool:
        return self.source_mtime == current_mtime and self.total_size == current_size

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    @classmethod
    def from_dict(cls, raw: Dict[str, Any]) -> "ResumeState":
        return cls(
            transfer_id=str(raw["transfer_id"]),
            file_cache_id=str(raw["file_cache_id"]),
            file_name=str(raw["file_name"]),
            total_size=int(raw["total_size"]),
            verified_offset=int(raw["verified_offset"]),
            source_mtime=int(raw["source_mtime"]),
            digest=DigestManifest.from_dict(raw["digest"]),
            active_channel=str(raw.get("active_channel", "LAN")),
            resume_count=int(raw.get("resume_count", 0)),
        )

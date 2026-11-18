//! 全文与 4MB 分块 BLAKE3 摘要计算器 (`DigestManifest`) 与流式校验器.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_CHUNK_SIZE_BYTES: usize = 4 * 1024 * 1024; // 4 MB
pub const BLAKE3_ALGORITHM: &str = "blake3";

#[derive(Debug, Error)]
pub enum DigestError {
    #[error("IO error while computing digest: {0}")]
    Io(#[from] io::Error),
    #[error("Digest mismatch: expected {expected}, actual {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("Chunk {chunk_index} digest mismatch: expected {expected}, actual {actual}")]
    ChunkHashMismatch {
        chunk_index: usize,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestManifest {
    pub algorithm: String,
    pub full_hash: String,
    pub chunk_size_bytes: usize,
    pub chunk_hashes: Vec<String>,
}

impl DigestManifest {
    pub fn from_bytes(data: &[u8]) -> Self {
        Self::from_bytes_with_chunk_size(data, DEFAULT_CHUNK_SIZE_BYTES)
    }

    pub fn from_bytes_with_chunk_size(data: &[u8], chunk_size: usize) -> Self {
        let full_hash = blake3::hash(data).to_hex().to_string();
        let mut chunk_hashes = Vec::new();
        let step = chunk_size.max(1);
        for chunk in data.chunks(step) {
            chunk_hashes.push(blake3::hash(chunk).to_hex().to_string());
        }
        Self {
            algorithm: BLAKE3_ALGORITHM.to_string(),
            full_hash,
            chunk_size_bytes: step,
            chunk_hashes,
        }
    }

    pub fn from_file(path: &Path) -> Result<Self, DigestError> {
        let mut file = File::open(path)?;
        let mut full_hasher = blake3::Hasher::new();
        let mut chunk_hashes = Vec::new();
        let mut buf = vec![0u8; DEFAULT_CHUNK_SIZE_BYTES];

        loop {
            let mut filled = 0;
            while filled < buf.len() {
                let n = file.read(&mut buf[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            let slice = &buf[..filled];
            full_hasher.update(slice);
            chunk_hashes.push(blake3::hash(slice).to_hex().to_string());
        }

        Ok(Self {
            algorithm: BLAKE3_ALGORITHM.to_string(),
            full_hash: full_hasher.finalize().to_hex().to_string(),
            chunk_size_bytes: DEFAULT_CHUNK_SIZE_BYTES,
            chunk_hashes,
        })
    }

    pub fn verify_bytes(&self, data: &[u8]) -> Result<(), DigestError> {
        let step = self.chunk_size_bytes.max(1);
        for (idx, (chunk, expected_chunk_hash)) in
            data.chunks(step).zip(self.chunk_hashes.iter()).enumerate()
        {
            let actual_chunk_hash = blake3::hash(chunk).to_hex().to_string();
            if &actual_chunk_hash != expected_chunk_hash {
                return Err(DigestError::ChunkHashMismatch {
                    chunk_index: idx,
                    expected: expected_chunk_hash.clone(),
                    actual: actual_chunk_hash,
                });
            }
        }
        let actual_full = blake3::hash(data).to_hex().to_string();
        if actual_full != self.full_hash {
            return Err(DigestError::HashMismatch {
                expected: self.full_hash.clone(),
                actual: actual_full,
            });
        }
        Ok(())
    }

    pub fn verify_file(&self, path: &Path) -> Result<(), DigestError> {
        let mut file = File::open(path)?;
        let step = self.chunk_size_bytes.max(1);
        let mut full_hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; step];
        let mut chunk_idx = 0usize;

        loop {
            let mut filled = 0;
            while filled < buf.len() {
                let n = file.read(&mut buf[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            let slice = &buf[..filled];
            self.verify_chunk(chunk_idx, slice)?;
            full_hasher.update(slice);
            chunk_idx += 1;
        }

        let actual_full = full_hasher.finalize().to_hex().to_string();
        if actual_full != self.full_hash {
            return Err(DigestError::HashMismatch {
                expected: self.full_hash.clone(),
                actual: actual_full,
            });
        }
        Ok(())
    }

    pub fn verify_chunk(&self, chunk_index: usize, chunk_data: &[u8]) -> Result<(), DigestError> {
        if let Some(expected) = self.chunk_hashes.get(chunk_index) {
            let actual = blake3::hash(chunk_data).to_hex().to_string();
            if &actual != expected {
                return Err(DigestError::ChunkHashMismatch {
                    chunk_index,
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_golden_vectors_and_chunk_tamper() {
        let raw = include_str!("../../../../tests/fixtures/blake3_vectors.json");
        let doc: serde_json::Value = serde_json::from_str(raw).expect("valid vectors json");
        for vec_item in doc["vectors"].as_array().unwrap() {
            let input = vec_item["input_utf8"].as_str().unwrap();
            let expected = vec_item["expected_blake3_hex"].as_str().unwrap();
            let manifest = DigestManifest::from_bytes(input.as_bytes());
            assert_eq!(manifest.full_hash, expected);
            assert!(manifest.verify_bytes(input.as_bytes()).is_ok());
        }

        let data = b"multinteractor-streaming-test-".repeat(200);
        let manifest = DigestManifest::from_bytes_with_chunk_size(&data, 512);
        assert!(manifest.verify_bytes(&data).is_ok());

        let mut tampered = data.clone();
        tampered[600] ^= 0x55;
        match manifest.verify_bytes(&tampered) {
            Err(DigestError::ChunkHashMismatch { chunk_index: 1, .. }) => {}
            other => panic!("expected chunk 1 mismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_streaming_file_digest_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("mi_digest_test_{}.bin", std::process::id()));
        let payload = b"0123456789abcdef".repeat(1024);
        std::fs::write(&tmp, &payload).unwrap();

        let manifest = DigestManifest::from_file(&tmp).unwrap();
        assert!(manifest.verify_file(&tmp).is_ok());

        std::fs::write(&tmp, b"corrupted").unwrap();
        assert!(manifest.verify_file(&tmp).is_err());
        let _ = std::fs::remove_file(&tmp);
    }
}

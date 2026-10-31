// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # LZ4 Compression Module
//!
//! LZ4 압축/해제 기능을 제공합니다.
//!
//! ## Features
//! - 고속 압축/해제
//! - 스트리밍 압축 지원
//! - 다양한 압축 레벨

#![allow(dead_code)]

use bytes::{Bytes, BytesMut};
use lz4_flex::{compress_prepend_size, decompress_size_prepended};

use super::FrameError;

/// 압축 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// 빠른 압축 (낮은 압축률)
    Fast,
    /// 기본 압축
    #[default]
    Default,
    /// 높은 압축률 (느림)
    High,
}

impl CompressionLevel {
    /// LZ4 acceleration 값으로 변환
    /// (lz4_flex는 acceleration을 직접 지원하지 않으므로 참고용)
    pub fn acceleration(&self) -> i32 {
        match self {
            Self::Fast => 100,
            Self::Default => 1,
            Self::High => 1,
        }
    }
}

/// 압축기
#[derive(Debug, Clone)]
pub struct Compressor {
    /// 압축 레벨
    level: CompressionLevel,
    /// 압축 임계값 (이 크기 미만은 압축하지 않음)
    threshold: usize,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            level: CompressionLevel::Default,
            threshold: 256, // 256바이트 미만은 압축 안함
        }
    }
}

impl Compressor {
    /// 새 압축기 생성
    pub fn new(level: CompressionLevel) -> Self {
        Self {
            level,
            threshold: 256,
        }
    }

    /// 압축 레벨 설정
    pub fn with_level(mut self, level: CompressionLevel) -> Self {
        self.level = level;
        self
    }

    /// 압축 임계값 설정
    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.threshold = threshold;
        self
    }

    /// 압축 수행
    ///
    /// 압축된 데이터가 원본보다 작을 때만 압축 결과 반환
    /// 그렇지 않으면 None 반환
    pub fn compress(&self, data: &[u8]) -> Option<Bytes> {
        // 임계값 미만은 압축하지 않음
        if data.len() < self.threshold {
            return None;
        }

        let compressed = compress_prepend_size(data);

        // 압축 후 더 커지면 압축하지 않음
        if compressed.len() >= data.len() {
            return None;
        }

        Some(Bytes::from(compressed))
    }

    /// 강제 압축 (크기 비교 없이)
    pub fn compress_force(&self, data: &[u8]) -> Bytes {
        Bytes::from(compress_prepend_size(data))
    }

    /// 압축 레벨 조회
    pub fn level(&self) -> CompressionLevel {
        self.level
    }

    /// 압축 임계값 조회
    pub fn threshold(&self) -> usize {
        self.threshold
    }
}

/// 해제기
#[derive(Debug, Clone, Default)]
pub struct Decompressor {
    /// 최대 해제 크기 제한
    max_size: Option<usize>,
}

impl Decompressor {
    /// 새 해제기 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 최대 해제 크기 설정
    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_size = Some(max_size);
        self
    }

    /// 압축 해제
    pub fn decompress(&self, data: &[u8]) -> Result<Bytes, FrameError> {
        let declared_size = declared_decompressed_size(data)?;

        if let Some(max) = self.max_size
            && declared_size > max
        {
            return Err(FrameError::TooLarge {
                size: declared_size,
                max,
            });
        }

        let decompressed = decompress_size_prepended(data)
            .map_err(|e| FrameError::DecompressionFailed(e.to_string()))?;
        Ok(Bytes::from(decompressed))
    }

    /// 최대 크기 조회
    pub fn max_size(&self) -> Option<usize> {
        self.max_size
    }
}

/// 스트리밍 압축기
pub struct StreamCompressor {
    buffer: BytesMut,
    level: CompressionLevel,
}

impl StreamCompressor {
    /// 새 스트리밍 압축기 생성
    pub fn new(level: CompressionLevel) -> Self {
        Self {
            buffer: BytesMut::with_capacity(64 * 1024), // 64KB 버퍼
            level,
        }
    }

    /// 데이터 추가
    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// 버퍼 플러시 및 압축
    pub fn flush(&mut self) -> Option<Bytes> {
        if self.buffer.is_empty() {
            return None;
        }

        let data = self.buffer.split().freeze();
        let compressor = Compressor::new(self.level);
        compressor.compress(&data).or(Some(data))
    }

    /// 최종 압축 (남은 데이터 모두)
    pub fn finish(mut self) -> Option<Bytes> {
        self.flush()
    }

    /// 버퍼 크기 조회
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// 버퍼 비우기
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// 스트리밍 해제기
pub struct StreamDecompressor {
    max_size: Option<usize>,
    total_decompressed: usize,
}

impl StreamDecompressor {
    /// 새 스트리밍 해제기 생성
    pub fn new() -> Self {
        Self {
            max_size: None,
            total_decompressed: 0,
        }
    }

    /// 최대 크기 설정
    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_size = Some(max_size);
        self
    }

    /// 청크 해제
    pub fn decompress_chunk(&mut self, data: &[u8]) -> Result<Bytes, FrameError> {
        let declared_size = declared_decompressed_size(data)?;
        let next_total =
            self.total_decompressed
                .checked_add(declared_size)
                .ok_or(FrameError::TooLarge {
                    size: usize::MAX,
                    max: self.max_size.unwrap_or(usize::MAX),
                })?;
        if let Some(max) = self.max_size
            && next_total > max
        {
            return Err(FrameError::TooLarge {
                size: next_total,
                max,
            });
        }

        let decompressor = Decompressor::new().with_max_size(declared_size);
        let decompressed = decompressor.decompress(data)?;
        self.total_decompressed = next_total;
        Ok(decompressed)
    }

    /// 총 해제된 크기 조회
    pub fn total_decompressed(&self) -> usize {
        self.total_decompressed
    }

    /// 리셋
    pub fn reset(&mut self) {
        self.total_decompressed = 0;
    }
}

impl Default for StreamDecompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// 압축 통계
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// 압축된 바이트 수
    pub compressed_bytes: u64,
    /// 원본 바이트 수
    pub original_bytes: u64,
    /// 압축 횟수
    pub compression_count: u64,
    /// 압축 스킵 횟수 (임계값 미만 또는 압축 효과 없음)
    pub skip_count: u64,
}

impl CompressionStats {
    /// 압축률 계산 (0.0 ~ 1.0, 낮을수록 좋음)
    pub fn ratio(&self) -> f64 {
        if self.original_bytes == 0 {
            1.0
        } else {
            self.compressed_bytes as f64 / self.original_bytes as f64
        }
    }

    /// 절약된 바이트 수
    pub fn saved_bytes(&self) -> u64 {
        self.original_bytes.saturating_sub(self.compressed_bytes)
    }

    /// 압축 기록
    pub fn record_compression(&mut self, original: usize, compressed: usize) {
        self.original_bytes += original as u64;
        self.compressed_bytes += compressed as u64;
        self.compression_count += 1;
    }

    /// 스킵 기록
    pub fn record_skip(&mut self, size: usize) {
        self.original_bytes += size as u64;
        self.compressed_bytes += size as u64;
        self.skip_count += 1;
    }

    /// 병합
    pub fn merge(&mut self, other: &Self) {
        self.compressed_bytes += other.compressed_bytes;
        self.original_bytes += other.original_bytes;
        self.compression_count += other.compression_count;
        self.skip_count += other.skip_count;
    }
}

/// 압축 가능 여부 체크
pub fn should_compress(data: &[u8], threshold: usize) -> bool {
    data.len() >= threshold
}

/// 빠른 압축 (기본 설정)
pub fn compress_fast(data: &[u8]) -> Bytes {
    Bytes::from(compress_prepend_size(data))
}

fn declared_decompressed_size(data: &[u8]) -> Result<usize, FrameError> {
    let prefix = data
        .get(..std::mem::size_of::<u32>())
        .ok_or_else(|| FrameError::DecompressionFailed("missing LZ4 size prefix".to_owned()))?;
    let mut bytes = [0_u8; std::mem::size_of::<u32>()];
    bytes.copy_from_slice(prefix);
    Ok(u32::from_le_bytes(bytes) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_expanded_size_is_rejected_before_decompression() {
        let forged = (1024_u32 * 1024).to_le_bytes();
        let error = Decompressor::new()
            .with_max_size(64)
            .decompress(&forged)
            .err();

        assert!(matches!(
            error,
            Some(FrameError::TooLarge {
                size: 1_048_576,
                max: 64
            })
        ));
    }

    #[test]
    fn streaming_limit_is_checked_before_expanding_the_next_chunk() {
        let compressor = Compressor::default();
        let chunk = compressor.compress_force(&[0_u8; 40]);
        let mut decompressor = StreamDecompressor::new().with_max_size(64);

        assert!(matches!(
            decompressor
                .decompress_chunk(&chunk)
                .map(|value| value.len()),
            Ok(40)
        ));
        let error = decompressor.decompress_chunk(&chunk).err();
        assert!(matches!(
            error,
            Some(FrameError::TooLarge { size: 80, max: 64 })
        ));
        assert_eq!(decompressor.total_decompressed(), 40);
    }
}

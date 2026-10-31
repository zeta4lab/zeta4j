// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 페이지 압축 모듈
//!
//! LZ4 압축을 사용하여 디스크 페이지를 압축/해제합니다.
//! 압축률 목표: 70% 이상의 저장공간 절감

use crate::error::{GraphError, Result};
use std::io::{Read, Write};

/// 압축 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionType {
    /// 압축 없음
    #[default]
    None,
    /// LZ4 압축 (빠른 압축/해제, 적당한 압축률)
    Lz4,
    /// LZ4 고압축 모드 (더 느리지만 더 높은 압축률)
    Lz4Hc,
}

impl CompressionType {
    /// 문자열에서 압축 타입 파싱
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "none" => Some(Self::None),
            "lz4" => Some(Self::Lz4),
            "lz4hc" | "lz4_hc" => Some(Self::Lz4Hc),
            _ => None,
        }
    }

    /// 압축 타입을 문자열로 변환
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lz4 => "lz4",
            Self::Lz4Hc => "lz4hc",
        }
    }
}

/// 압축된 데이터 블록 헤더
///
/// 구조:
/// - magic: 2 bytes (0x5A43 = "ZC")
/// - version: 1 byte
/// - compression_type: 1 byte
/// - original_size: 4 bytes (little endian)
/// - compressed_size: 4 bytes (little endian)
/// - checksum: 4 bytes (CRC32)
///
/// 총 16 bytes
#[derive(Debug, Clone, Copy)]
pub struct CompressedBlockHeader {
    pub magic: u16,
    pub version: u8,
    pub compression_type: u8,
    pub original_size: u32,
    pub compressed_size: u32,
    pub checksum: u32,
}

impl CompressedBlockHeader {
    /// 매직 넘버: "ZC" (Zeta Compressed)
    pub const MAGIC: u16 = 0x435A; // 'Z' 'C' in little endian
    pub const VERSION: u8 = 1;
    pub const SIZE: usize = 16;

    /// 새 헤더 생성
    pub fn new(
        compression_type: CompressionType,
        original_size: u32,
        compressed_size: u32,
        checksum: u32,
    ) -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            compression_type: match compression_type {
                CompressionType::None => 0,
                CompressionType::Lz4 => 1,
                CompressionType::Lz4Hc => 2,
            },
            original_size,
            compressed_size,
            checksum,
        }
    }

    /// 헤더를 바이트로 직렬화
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..2].copy_from_slice(&self.magic.to_le_bytes());
        buf[2] = self.version;
        buf[3] = self.compression_type;
        buf[4..8].copy_from_slice(&self.original_size.to_le_bytes());
        buf[8..12].copy_from_slice(&self.compressed_size.to_le_bytes());
        buf[12..16].copy_from_slice(&self.checksum.to_le_bytes());
        buf
    }

    /// 바이트에서 헤더 역직렬화
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::SIZE {
            return Err(GraphError::Storage {
                message: format!("Header too short: {} < {}", buf.len(), Self::SIZE),
            });
        }

        let magic = u16::from_le_bytes([buf[0], buf[1]]);
        if magic != Self::MAGIC {
            return Err(GraphError::Storage {
                message: format!("Invalid compression magic: 0x{:04X}", magic),
            });
        }

        let version = buf[2];
        if version != Self::VERSION {
            return Err(GraphError::Storage {
                message: format!("Unsupported compression version: {}", version),
            });
        }

        Ok(Self {
            magic,
            version,
            compression_type: buf[3],
            original_size: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            compressed_size: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            checksum: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        })
    }

    /// 압축 타입 가져오기
    pub fn get_compression_type(&self) -> CompressionType {
        match self.compression_type {
            0 => CompressionType::None,
            1 => CompressionType::Lz4,
            2 => CompressionType::Lz4Hc,
            _ => CompressionType::None,
        }
    }
}

/// 간단한 CRC32 체크섬 계산 (IEEE 다항식)
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// 데이터 압축
///
/// 압축된 데이터가 원본보다 크면 원본을 그대로 반환 (compression_type = None)
pub fn compress(data: &[u8], compression_type: CompressionType) -> Result<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let checksum = crc32(data);
    let original_size = data.len() as u32;

    let (compressed_data, actual_compression) = match compression_type {
        CompressionType::None => (data.to_vec(), CompressionType::None),
        CompressionType::Lz4 | CompressionType::Lz4Hc => {
            let mut encoder = lz4::EncoderBuilder::new()
                .level(if compression_type == CompressionType::Lz4Hc {
                    9
                } else {
                    4
                })
                .build(Vec::new())
                .map_err(|e| GraphError::Storage {
                    message: format!("LZ4 encoder creation failed: {}", e),
                })?;

            encoder.write_all(data).map_err(|e| GraphError::Storage {
                message: format!("LZ4 compression failed: {}", e),
            })?;

            let (compressed, result) = encoder.finish();
            result.map_err(|e| GraphError::Storage {
                message: format!("LZ4 finish failed: {}", e),
            })?;

            // 압축 결과가 원본보다 크면 비압축 사용
            if compressed.len() >= data.len() {
                (data.to_vec(), CompressionType::None)
            } else {
                (compressed, compression_type)
            }
        }
    };

    let header = CompressedBlockHeader::new(
        actual_compression,
        original_size,
        compressed_data.len() as u32,
        checksum,
    );

    let mut result = Vec::with_capacity(CompressedBlockHeader::SIZE + compressed_data.len());
    result.extend_from_slice(&header.to_bytes());
    result.extend_from_slice(&compressed_data);

    Ok(result)
}

/// 압축 해제
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    if data.len() < CompressedBlockHeader::SIZE {
        return Err(GraphError::Storage {
            message: "Data too short for decompression".to_string(),
        });
    }

    let header = CompressedBlockHeader::from_bytes(data)?;
    let compressed_data = &data[CompressedBlockHeader::SIZE..];

    if compressed_data.len() != header.compressed_size as usize {
        return Err(GraphError::Storage {
            message: format!(
                "Compressed size mismatch: expected {}, got {}",
                header.compressed_size,
                compressed_data.len()
            ),
        });
    }

    let decompressed = match header.get_compression_type() {
        CompressionType::None => compressed_data.to_vec(),
        CompressionType::Lz4 | CompressionType::Lz4Hc => {
            let mut decoder =
                lz4::Decoder::new(compressed_data).map_err(|e| GraphError::Storage {
                    message: format!("LZ4 decoder creation failed: {}", e),
                })?;

            let mut decompressed = Vec::with_capacity(header.original_size as usize);
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| GraphError::Storage {
                    message: format!("LZ4 decompression failed: {}", e),
                })?;

            decompressed
        }
    };

    // 원본 크기 검증
    if decompressed.len() != header.original_size as usize {
        return Err(GraphError::Storage {
            message: format!(
                "Decompressed size mismatch: expected {}, got {}",
                header.original_size,
                decompressed.len()
            ),
        });
    }

    // 체크섬 검증
    let checksum = crc32(&decompressed);
    if checksum != header.checksum {
        return Err(GraphError::Storage {
            message: format!(
                "Checksum mismatch: expected 0x{:08X}, got 0x{:08X}",
                header.checksum, checksum
            ),
        });
    }

    Ok(decompressed)
}

/// 압축률 계산 (원본 대비 압축 후 크기 비율)
pub fn compression_ratio(original_size: usize, compressed_size: usize) -> f64 {
    if original_size == 0 {
        return 1.0;
    }
    compressed_size as f64 / original_size as f64
}

/// 공간 절감률 계산 (1.0 - 압축률)
pub fn space_savings(original_size: usize, compressed_size: usize) -> f64 {
    1.0 - compression_ratio(original_size, compressed_size)
}

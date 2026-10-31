// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 벡터 타입 정의
//!
//! 고차원 벡터(임베딩) 관련 타입을 정의합니다.

use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// VectorError - 벡터 에러 타입
// ============================================================================

/// 벡터 관련 에러
#[derive(Debug, Clone, Error)]
pub enum VectorError {
    #[error("Empty vector is not allowed")]
    EmptyVector,

    #[error("Vector dimensions {got} exceeds maximum {max}")]
    DimensionsTooLarge { got: u32, max: u32 },

    #[error("Dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: u32, got: u32 },

    #[error("Vector contains NaN at index {index}")]
    ContainsNaN { index: usize },

    #[error("Vector contains Infinity at index {index}")]
    ContainsInfinity { index: usize },
}

// ============================================================================
// VectorValue - 고차원 벡터 (임베딩)
// ============================================================================

/// 고차원 벡터 (임베딩)
/// - 최대 차원: 4096
/// - 정밀도: f32 (4바이트/차원)
/// - 메모리: 차원 × 4바이트
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VectorValue {
    /// 벡터 데이터 (f32 배열)
    data: Vec<f32>,
    /// 차원 수 (캐시)
    dimensions: u32,
}

impl VectorValue {
    /// 최대 허용 차원
    pub const MAX_DIMENSIONS: u32 = 4096;

    /// 새 벡터 생성
    pub fn new(data: Vec<f32>) -> Result<Self, VectorError> {
        let dimensions = data.len() as u32;

        if dimensions == 0 {
            return Err(VectorError::EmptyVector);
        }
        if dimensions > Self::MAX_DIMENSIONS {
            return Err(VectorError::DimensionsTooLarge {
                got: dimensions,
                max: Self::MAX_DIMENSIONS,
            });
        }

        Ok(Self { data, dimensions })
    }

    /// 차원 수 반환
    #[inline]
    pub fn dimensions(&self) -> u32 {
        self.dimensions
    }

    /// 데이터 슬라이스 반환
    #[inline]
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// 데이터 소유권 반환
    #[inline]
    pub fn into_vec(self) -> Vec<f32> {
        self.data
    }

    /// L2 정규화 (단위 벡터로 변환)
    pub fn normalize(&mut self) {
        let norm = self.l2_norm();
        if norm > f32::EPSILON {
            for x in &mut self.data {
                *x /= norm;
            }
        }
    }

    /// L2 노름 계산
    pub fn l2_norm(&self) -> f32 {
        self.data.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    /// 정규화된 복사본 반환
    pub fn normalized(&self) -> Self {
        let mut copy = self.clone();
        copy.normalize();
        copy
    }

    /// NaN/Inf 검증
    pub fn validate(&self) -> Result<(), VectorError> {
        for (i, &v) in self.data.iter().enumerate() {
            if v.is_nan() {
                return Err(VectorError::ContainsNaN { index: i });
            }
            if v.is_infinite() {
                return Err(VectorError::ContainsInfinity { index: i });
            }
        }
        Ok(())
    }

    /// 메모리 사용량 추정 (바이트)
    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.data.len().saturating_mul(std::mem::size_of::<f32>()))
    }
}

impl PartialEq for VectorValue {
    fn eq(&self, other: &Self) -> bool {
        if self.dimensions != other.dimensions {
            return false;
        }
        // f32 비교는 epsilon 사용
        const EPSILON: f32 = 1e-6;
        self.data
            .iter()
            .zip(other.data.iter())
            .all(|(a, b)| (a - b).abs() < EPSILON)
    }
}

impl fmt::Display for VectorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dimensions <= 5 {
            // 짧은 벡터는 전체 표시
            write!(f, "vector({:?})", self.data)
        } else {
            // 긴 벡터는 축약 표시
            write!(
                f,
                "vector([{:.4}, {:.4}, {:.4}, ... ({} dims)])",
                self.data[0], self.data[1], self.data[2], self.dimensions
            )
        }
    }
}

impl Hash for VectorValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.dimensions.hash(state);
        // f32는 직접 Hash할 수 없으므로 비트 표현을 사용
        for &v in &self.data {
            v.to_bits().hash(state);
        }
    }
}

//! 벡터 거리 함수 모듈
//!
//! HNSW 인덱스에서 사용하는 거리 계산 함수들을 제공합니다.

use serde::{Deserialize, Serialize};

/// 거리 함수 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DistanceFunction {
    /// 코사인 거리 (1 - cosine_similarity)
    #[default]
    Cosine,
    /// 유클리드 거리 (L2)
    Euclidean,
    /// 내적 거리 (-dot_product, 최대화 → 최소화)
    DotProduct,
}

impl DistanceFunction {
    /// 거리 계산
    #[inline]
    pub fn compute(&self, a: &[f32], b: &[f32]) -> f32 {
        debug_assert_eq!(a.len(), b.len());

        match self {
            Self::Cosine => cosine_distance(a, b),
            Self::Euclidean => euclidean_distance(a, b),
            Self::DotProduct => -dot_product(a, b), // 음수로 변환 (최소화)
        }
    }

    /// 거리 함수 이름
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cosine => "cosine",
            Self::Euclidean => "euclidean",
            Self::DotProduct => "dot_product",
        }
    }

    /// 문자열 표현 반환
    pub fn as_str(&self) -> &'static str {
        self.name()
    }

    /// 바이트 변환 (직렬화용)
    pub fn to_byte(&self) -> u8 {
        match self {
            Self::Euclidean => 0,
            Self::Cosine => 1,
            Self::DotProduct => 2,
        }
    }

    /// 바이트에서 변환 (역직렬화용)
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Euclidean),
            1 => Some(Self::Cosine),
            2 => Some(Self::DotProduct),
            _ => None,
        }
    }
}

impl std::fmt::Display for DistanceFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for DistanceFunction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cosine" => Ok(Self::Cosine),
            "euclidean" | "l2" => Ok(Self::Euclidean),
            "dot_product" | "dot" | "inner_product" | "dotproduct" => Ok(Self::DotProduct),
            _ => Err(format!(
                "Unknown distance function: '{}'. Valid: cosine, euclidean/l2, dot_product/dot/inner_product",
                s
            )),
        }
    }
}

/// 코사인 거리: 1 - (a·b) / (|a||b|)
#[inline]
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot = dot_product(a, b);
    let norm_a = dot_product(a, a).sqrt();
    let norm_b = dot_product(b, b).sqrt();

    if norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 1.0;
    }

    1.0 - (dot / (norm_a * norm_b))
}

/// 코사인 유사도: (a·b) / (|a||b|)
#[inline]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    1.0 - cosine_distance(a, b)
}

/// 유클리드 거리: sqrt(Σ(a_i - b_i)²)
#[inline]
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    euclidean_distance_squared(a, b).sqrt()
}

/// 유클리드 거리 제곱 (sqrt 생략)
#[inline]
pub fn euclidean_distance_squared(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}

/// 내적: Σ(a_i × b_i)
#[inline]
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// L1 거리 (맨해튼 거리)
#[inline]
pub fn manhattan_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
}

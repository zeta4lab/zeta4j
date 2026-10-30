//! Link Features
//!
//! 링크 예측을 위한 노드 쌍 피처 결합

use serde::{Deserialize, Serialize};

/// 링크 피처 결합 방식
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkFeature {
    /// L2 거리: ||u - v||^2
    L2,
    /// Hadamard (element-wise product): u * v
    Hadamard,
    /// 코사인 유사도: (u · v) / (||u|| ||v||)
    Cosine,
    /// 평균: (u + v) / 2
    Average,
    /// 덧셈: u + v
    Sum,
    /// 뺄셈 절대값: |u - v|
    AbsDiff,
}

impl LinkFeature {
    /// 문자열에서 변환
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "L2" => Some(Self::L2),
            "HADAMARD" => Some(Self::Hadamard),
            "COSINE" => Some(Self::Cosine),
            "AVERAGE" | "AVG" => Some(Self::Average),
            "SUM" | "ADD" => Some(Self::Sum),
            "ABSDIFF" | "ABS_DIFF" => Some(Self::AbsDiff),
            _ => None,
        }
    }

    /// 이름
    pub fn name(&self) -> &'static str {
        match self {
            Self::L2 => "L2",
            Self::Hadamard => "HADAMARD",
            Self::Cosine => "COSINE",
            Self::Average => "AVERAGE",
            Self::Sum => "SUM",
            Self::AbsDiff => "ABS_DIFF",
        }
    }
}

/// Link Feature Combiner
///
/// 두 노드의 피처 벡터를 결합하여 링크 피처 생성
#[derive(Debug, Clone)]
pub struct LinkFeatureCombiner {
    /// 사용할 결합 방식
    features: Vec<LinkFeature>,
}

impl LinkFeatureCombiner {
    /// 새 결합기 생성
    pub fn new(features: Vec<LinkFeature>) -> Self {
        Self { features }
    }

    /// Hadamard만 사용
    pub fn hadamard() -> Self {
        Self::new(vec![LinkFeature::Hadamard])
    }

    /// L2만 사용
    pub fn l2() -> Self {
        Self::new(vec![LinkFeature::L2])
    }

    /// 두 노드 피처 벡터 결합
    ///
    /// # Arguments
    /// * `u` - 첫 번째 노드 피처 벡터
    /// * `v` - 두 번째 노드 피처 벡터
    ///
    /// # Returns
    /// 결합된 링크 피처 벡터
    pub fn combine(&self, u: &[f64], v: &[f64]) -> Vec<f64> {
        let mut result = Vec::new();

        for feature in &self.features {
            let combined = self.apply_feature(*feature, u, v);
            result.extend(combined);
        }

        result
    }

    /// 단일 피처 적용
    fn apply_feature(&self, feature: LinkFeature, u: &[f64], v: &[f64]) -> Vec<f64> {
        let dim = u.len().max(v.len());

        match feature {
            LinkFeature::L2 => {
                // ||u - v||^2 -> 단일 스칼라
                let dist: f64 = (0..dim)
                    .map(|i| {
                        let ui = u.get(i).copied().unwrap_or(0.0);
                        let vi = v.get(i).copied().unwrap_or(0.0);
                        (ui - vi).powi(2)
                    })
                    .sum();
                vec![dist]
            }
            LinkFeature::Hadamard => {
                // Element-wise product
                (0..dim)
                    .map(|i| {
                        let ui = u.get(i).copied().unwrap_or(0.0);
                        let vi = v.get(i).copied().unwrap_or(0.0);
                        ui * vi
                    })
                    .collect()
            }
            LinkFeature::Cosine => {
                // 코사인 유사도 -> 단일 스칼라
                let dot: f64 = (0..dim)
                    .map(|i| {
                        let ui = u.get(i).copied().unwrap_or(0.0);
                        let vi = v.get(i).copied().unwrap_or(0.0);
                        ui * vi
                    })
                    .sum();

                let norm_u: f64 = u.iter().map(|&x| x * x).sum::<f64>().sqrt();
                let norm_v: f64 = v.iter().map(|&x| x * x).sum::<f64>().sqrt();

                let similarity = if norm_u > 0.0 && norm_v > 0.0 {
                    dot / (norm_u * norm_v)
                } else {
                    0.0
                };

                vec![similarity]
            }
            LinkFeature::Average => (0..dim)
                .map(|i| {
                    let ui = u.get(i).copied().unwrap_or(0.0);
                    let vi = v.get(i).copied().unwrap_or(0.0);
                    (ui + vi) / 2.0
                })
                .collect(),
            LinkFeature::Sum => (0..dim)
                .map(|i| {
                    let ui = u.get(i).copied().unwrap_or(0.0);
                    let vi = v.get(i).copied().unwrap_or(0.0);
                    ui + vi
                })
                .collect(),
            LinkFeature::AbsDiff => (0..dim)
                .map(|i| {
                    let ui = u.get(i).copied().unwrap_or(0.0);
                    let vi = v.get(i).copied().unwrap_or(0.0);
                    (ui - vi).abs()
                })
                .collect(),
        }
    }

    /// 결합된 피처 차원
    pub fn output_dim(&self, input_dim: usize) -> usize {
        self.features
            .iter()
            .map(|f| match f {
                LinkFeature::L2 | LinkFeature::Cosine => 1,
                LinkFeature::Hadamard
                | LinkFeature::Average
                | LinkFeature::Sum
                | LinkFeature::AbsDiff => input_dim,
            })
            .sum()
    }

    /// 사용 중인 피처 목록
    pub fn features(&self) -> &[LinkFeature] {
        &self.features
    }
}

impl Default for LinkFeatureCombiner {
    fn default() -> Self {
        Self::hadamard()
    }
}

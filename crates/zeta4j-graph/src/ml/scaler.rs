//! Feature Scaling
//!
//! Min-Max, Standard (Z-score) 스케일링 구현

use serde::{Deserialize, Serialize};

/// 스케일러 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScalerType {
    /// Min-Max 스케일링: (x - min) / (max - min)
    MinMax,
    /// Standard 스케일링 (Z-score): (x - mean) / std
    #[default]
    Standard,
    /// Max Abs 스케일링: x / max(|x|)
    MaxAbs,
    /// 스케일링 없음
    None,
}

/// 스케일러 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalerConfig {
    pub scaler_type: ScalerType,
    /// Min-Max 스케일링 범위 (기본: 0.0 ~ 1.0)
    pub feature_range: (f64, f64),
}

impl Default for ScalerConfig {
    fn default() -> Self {
        Self {
            scaler_type: ScalerType::Standard,
            feature_range: (0.0, 1.0),
        }
    }
}

/// Feature Scaler
///
/// 피처 스케일링을 위한 통계량 저장 및 변환 수행
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureScaler {
    config: ScalerConfig,
    /// 각 피처별 통계량
    stats: Vec<FeatureStats>,
    /// 피처 개수
    n_features: usize,
    /// fit 여부
    is_fitted: bool,
}

/// 피처별 통계량
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std: f64,
    pub max_abs: f64,
}

impl FeatureScaler {
    /// 새 스케일러 생성
    pub fn new(config: ScalerConfig) -> Self {
        Self {
            config,
            stats: Vec::new(),
            n_features: 0,
            is_fitted: false,
        }
    }

    /// 기본 설정으로 Standard 스케일러 생성
    pub fn standard() -> Self {
        Self::new(ScalerConfig {
            scaler_type: ScalerType::Standard,
            ..Default::default()
        })
    }

    /// Min-Max 스케일러 생성
    pub fn min_max() -> Self {
        Self::new(ScalerConfig {
            scaler_type: ScalerType::MinMax,
            ..Default::default()
        })
    }

    /// Min-Max 스케일러 (범위 지정)
    pub fn min_max_with_range(min: f64, max: f64) -> Self {
        Self::new(ScalerConfig {
            scaler_type: ScalerType::MinMax,
            feature_range: (min, max),
        })
    }

    /// 데이터로부터 통계량 학습
    ///
    /// # Arguments
    /// * `data` - 2D 배열 (samples x features)
    pub fn fit(&mut self, data: &[Vec<f64>]) {
        if data.is_empty() {
            self.is_fitted = true;
            return;
        }

        self.n_features = data[0].len();
        self.stats = vec![FeatureStats::default(); self.n_features];

        let n_samples = data.len();

        // 각 피처별 통계량 계산
        for feature_idx in 0..self.n_features {
            let values: Vec<f64> = data.iter().map(|row| row[feature_idx]).collect();

            // Min, Max
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            // Mean
            let sum: f64 = values.iter().sum();
            let mean = sum / n_samples as f64;

            // Std (population)
            let variance: f64 =
                values.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / n_samples as f64;
            let std = variance.sqrt();

            // Max Abs
            let max_abs = values.iter().map(|x| x.abs()).fold(0.0f64, f64::max);

            self.stats[feature_idx] = FeatureStats {
                min,
                max,
                mean,
                std,
                max_abs,
            };
        }

        self.is_fitted = true;
    }

    /// 데이터 변환
    ///
    /// # Arguments
    /// * `data` - 변환할 2D 배열
    ///
    /// # Returns
    /// 스케일링된 데이터
    pub fn transform(&self, data: &[Vec<f64>]) -> Vec<Vec<f64>> {
        if !self.is_fitted || data.is_empty() {
            return data.to_vec();
        }

        data.iter().map(|row| self.transform_row(row)).collect()
    }

    /// 단일 행 변환
    fn transform_row(&self, row: &[f64]) -> Vec<f64> {
        row.iter()
            .enumerate()
            .map(|(i, &value)| {
                if i >= self.stats.len() {
                    return value;
                }
                let stats = &self.stats[i];
                self.scale_value(value, stats)
            })
            .collect()
    }

    /// 값 스케일링
    fn scale_value(&self, value: f64, stats: &FeatureStats) -> f64 {
        match self.config.scaler_type {
            ScalerType::MinMax => {
                let range = stats.max - stats.min;
                if range == 0.0 {
                    return self.config.feature_range.0;
                }
                let scaled = (value - stats.min) / range;
                let (min_range, max_range) = self.config.feature_range;
                scaled * (max_range - min_range) + min_range
            }
            ScalerType::Standard => {
                if stats.std == 0.0 {
                    return 0.0;
                }
                (value - stats.mean) / stats.std
            }
            ScalerType::MaxAbs => {
                if stats.max_abs == 0.0 {
                    return 0.0;
                }
                value / stats.max_abs
            }
            ScalerType::None => value,
        }
    }

    /// Fit and Transform
    pub fn fit_transform(&mut self, data: &[Vec<f64>]) -> Vec<Vec<f64>> {
        self.fit(data);
        self.transform(data)
    }

    /// 역변환
    pub fn inverse_transform(&self, data: &[Vec<f64>]) -> Vec<Vec<f64>> {
        if !self.is_fitted || data.is_empty() {
            return data.to_vec();
        }

        data.iter()
            .map(|row| self.inverse_transform_row(row))
            .collect()
    }

    /// 단일 행 역변환
    fn inverse_transform_row(&self, row: &[f64]) -> Vec<f64> {
        row.iter()
            .enumerate()
            .map(|(i, &value)| {
                if i >= self.stats.len() {
                    return value;
                }
                let stats = &self.stats[i];
                self.inverse_scale_value(value, stats)
            })
            .collect()
    }

    /// 값 역변환
    fn inverse_scale_value(&self, value: f64, stats: &FeatureStats) -> f64 {
        match self.config.scaler_type {
            ScalerType::MinMax => {
                let (min_range, max_range) = self.config.feature_range;
                let range = max_range - min_range;
                if range == 0.0 {
                    return stats.min;
                }
                let scaled = (value - min_range) / range;
                scaled * (stats.max - stats.min) + stats.min
            }
            ScalerType::Standard => value * stats.std + stats.mean,
            ScalerType::MaxAbs => value * stats.max_abs,
            ScalerType::None => value,
        }
    }

    /// 피처 개수
    pub fn n_features(&self) -> usize {
        self.n_features
    }

    /// Fit 여부
    pub fn is_fitted(&self) -> bool {
        self.is_fitted
    }

    /// 통계량 조회
    pub fn stats(&self) -> &[FeatureStats] {
        &self.stats
    }
}

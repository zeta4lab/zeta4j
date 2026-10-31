// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Holt-Winters 모델
//!
//! 계절성을 지원하는 Holt-Winters 삼중 지수 평활법을 구현합니다.

use serde::{Deserialize, Serialize};

use super::{ModelError, PredictionModel, z_score_for_confidence};
use crate::tuning::forecasting::timeseries::{mean, std_dev};

/// Holt-Winters 모델 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HWType {
    /// 가법적 계절성
    Additive,
    /// 승법적 계절성
    Multiplicative,
}

/// Holt-Winters 파라미터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HWParams {
    /// 레벨 스무딩
    pub alpha: f64,
    /// 트렌드 스무딩
    pub beta: f64,
    /// 계절성 스무딩
    pub gamma: f64,
}

impl Default for HWParams {
    fn default() -> Self {
        Self {
            alpha: 0.2,
            beta: 0.1,
            gamma: 0.1,
        }
    }
}

/// Holt-Winters 상태
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HWState {
    level: f64,
    trend: f64,
    seasonal: Vec<f64>,
    residual_std: f64,
    data_len: usize,
}

/// Holt-Winters 모델
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoltWinters {
    /// 모델 유형
    model_type: HWType,
    /// 계절 주기
    seasonal_period: usize,
    /// 파라미터
    params: HWParams,
    /// 자동 파라미터 최적화 여부
    optimize_params: bool,
    /// 학습된 상태
    state: Option<HWState>,
}

impl HoltWinters {
    /// 새 Holt-Winters 모델 생성
    pub fn new(model_type: HWType, seasonal_period: usize) -> Self {
        Self {
            model_type,
            seasonal_period,
            params: HWParams::default(),
            optimize_params: true,
            state: None,
        }
    }

    /// 파라미터를 지정하여 생성
    pub fn with_params(model_type: HWType, seasonal_period: usize, params: HWParams) -> Self {
        Self {
            model_type,
            seasonal_period,
            params,
            optimize_params: false,
            state: None,
        }
    }

    /// 파라미터 최적화 (그리드 탐색)
    fn optimize_parameters(&mut self, data: &[f64]) -> Result<(), ModelError> {
        let period = self.seasonal_period;
        if data.len() < period * 3 {
            return Err(ModelError::InsufficientData {
                required: period * 3,
                actual: data.len(),
            });
        }

        // 학습/검증 분할
        let train_size = data.len() - period;
        let train_data = &data[..train_size];
        let valid_data = &data[train_size..];

        let mut best_mse = f64::MAX;
        let mut best_params = self.params.clone();

        // 그리드 탐색
        for alpha in [0.1, 0.2, 0.3, 0.4, 0.5] {
            for beta in [0.05, 0.1, 0.15, 0.2] {
                for gamma in [0.05, 0.1, 0.15, 0.2] {
                    self.params = HWParams { alpha, beta, gamma };
                    self.state = None;

                    if self.train_internal(train_data).is_ok()
                        && let Ok(predictions) = self.predict(valid_data.len())
                    {
                        let mse: f64 = predictions
                            .iter()
                            .zip(valid_data.iter())
                            .map(|(p, a)| (p - a).powi(2))
                            .sum::<f64>()
                            / valid_data.len() as f64;

                        if mse < best_mse {
                            best_mse = mse;
                            best_params = HWParams { alpha, beta, gamma };
                        }
                    }
                }
            }
        }

        self.params = best_params;
        Ok(())
    }

    /// 내부 학습 (최적화 없이)
    fn train_internal(&mut self, data: &[f64]) -> Result<(), ModelError> {
        let period = self.seasonal_period;
        if data.len() < period * 2 {
            return Err(ModelError::InsufficientData {
                required: period * 2,
                actual: data.len(),
            });
        }

        let HWParams { alpha, beta, gamma } = self.params;
        let cycle_count = data.len() / period;

        // 초기 계절 성분
        let mut seasonal = vec![0.0; period];
        let cycle_means: Vec<f64> = (0..cycle_count.min(3))
            .map(|c| mean(&data[c * period..(c + 1) * period]))
            .collect();

        match self.model_type {
            HWType::Additive => {
                for i in 0..period {
                    let sum: f64 = (0..cycle_means.len())
                        .map(|c| data[c * period + i] - cycle_means[c])
                        .sum();
                    seasonal[i] = sum / cycle_means.len() as f64;
                }
                // 정규화
                let seasonal_mean = mean(&seasonal);
                for s in &mut seasonal {
                    *s -= seasonal_mean;
                }
            }
            HWType::Multiplicative => {
                for i in 0..period {
                    let sum: f64 = (0..cycle_means.len())
                        .filter_map(|c| {
                            if cycle_means[c] > 0.0 {
                                Some(data[c * period + i] / cycle_means[c])
                            } else {
                                None
                            }
                        })
                        .sum();
                    seasonal[i] = sum / cycle_means.len() as f64;
                }
                // 정규화 (평균 1)
                let seasonal_mean = mean(&seasonal);
                if seasonal_mean > 0.0 {
                    for s in &mut seasonal {
                        *s /= seasonal_mean;
                    }
                }
            }
        }

        // 초기 레벨과 트렌드
        let mut level = mean(&data[0..period]);
        let mut trend = if data.len() >= period * 2 {
            (mean(&data[period..period * 2]) - level) / period as f64
        } else {
            0.0
        };

        let mut residuals = Vec::with_capacity(data.len());

        // 학습 루프
        for (i, &value) in data.iter().enumerate() {
            let season_idx = i % period;

            let forecast = match self.model_type {
                HWType::Additive => level + trend + seasonal[season_idx],
                HWType::Multiplicative => (level + trend) * seasonal[season_idx].max(0.001),
            };
            residuals.push(value - forecast);

            match self.model_type {
                HWType::Additive => {
                    let new_level =
                        alpha * (value - seasonal[season_idx]) + (1.0 - alpha) * (level + trend);
                    let new_trend = beta * (new_level - level) + (1.0 - beta) * trend;
                    let new_seasonal =
                        gamma * (value - new_level) + (1.0 - gamma) * seasonal[season_idx];

                    level = new_level;
                    trend = new_trend;
                    seasonal[season_idx] = new_seasonal;
                }
                HWType::Multiplicative => {
                    let s = seasonal[season_idx].max(0.001);
                    let new_level = alpha * (value / s) + (1.0 - alpha) * (level + trend);
                    let new_trend = beta * (new_level - level) + (1.0 - beta) * trend;
                    let new_seasonal = if new_level > 0.0 {
                        gamma * (value / new_level) + (1.0 - gamma) * s
                    } else {
                        s
                    };

                    level = new_level;
                    trend = new_trend;
                    seasonal[season_idx] = new_seasonal.max(0.001);
                }
            }
        }

        self.state = Some(HWState {
            level,
            trend,
            seasonal,
            residual_std: std_dev(&residuals),
            data_len: data.len(),
        });

        Ok(())
    }

    /// 모델 유형 반환
    pub fn model_type(&self) -> HWType {
        self.model_type
    }

    /// 계절 주기 반환
    pub fn seasonal_period(&self) -> usize {
        self.seasonal_period
    }

    /// 파라미터 반환
    pub fn params(&self) -> &HWParams {
        &self.params
    }
}

impl PredictionModel for HoltWinters {
    fn train(&mut self, data: &[f64]) -> Result<(), ModelError> {
        if self.optimize_params {
            self.optimize_parameters(data)?;
        }
        self.train_internal(data)
    }

    fn predict(&self, horizon: usize) -> Result<Vec<f64>, ModelError> {
        let state = self.state.as_ref().ok_or(ModelError::NotTrained)?;

        let mut forecasts = Vec::with_capacity(horizon);
        let period = self.seasonal_period;

        for h in 1..=horizon {
            let season_idx = (state.data_len + h - 1) % period;
            let forecast = match self.model_type {
                HWType::Additive => {
                    state.level + state.trend * h as f64 + state.seasonal[season_idx]
                }
                HWType::Multiplicative => {
                    (state.level + state.trend * h as f64) * state.seasonal[season_idx]
                }
            };
            forecasts.push(forecast.max(0.0)); // 음수 방지
        }

        Ok(forecasts)
    }

    fn predict_interval(
        &self,
        horizon: usize,
        confidence: f64,
    ) -> Result<Vec<(f64, f64)>, ModelError> {
        let forecasts = self.predict(horizon)?;
        let state = self.state.as_ref().ok_or(ModelError::NotTrained)?;

        let z = z_score_for_confidence(confidence);

        let mut intervals = Vec::with_capacity(horizon);
        for (h, forecast) in forecasts.iter().enumerate() {
            let error_factor = ((h + 1) as f64).sqrt();
            let margin = z * state.residual_std * error_factor;
            intervals.push(((forecast - margin).max(0.0), forecast + margin));
        }

        Ok(intervals)
    }

    fn name(&self) -> &str {
        match self.model_type {
            HWType::Additive => "HoltWinters-Additive",
            HWType::Multiplicative => "HoltWinters-Multiplicative",
        }
    }

    fn is_trained(&self) -> bool {
        self.state.is_some()
    }
}

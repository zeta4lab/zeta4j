//! 지수 평활법 (Exponential Smoothing)
//!
//! 단순, 이중, 삼중 지수 평활법을 구현합니다.

use serde::{Deserialize, Serialize};

use super::{ModelError, PredictionModel, z_score_for_confidence};
use crate::tuning::forecasting::timeseries::{mean, std_dev};

/// 지수 평활법 유형
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SmoothingType {
    /// 단순 지수 평활 (Simple Exponential Smoothing)
    Simple,
    /// 이중 지수 평활 (Double/Holt's Linear)
    Double,
    /// 삼중 지수 평활 (Triple/Holt-Winters)
    Triple,
}

/// 지수 평활법 상태
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ESState {
    /// 레벨 (평활된 값)
    level: f64,
    /// 트렌드 (기울기)
    trend: Option<f64>,
    /// 계절 성분
    seasonal: Option<Vec<f64>>,
    /// 학습 데이터 분산 (신뢰 구간용)
    residual_std: f64,
}

/// 지수 평활법 모델
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExponentialSmoothing {
    /// 레벨 스무딩 파라미터 (0 < alpha <= 1)
    alpha: f64,
    /// 트렌드 스무딩 파라미터 (0 < beta <= 1)
    beta: Option<f64>,
    /// 계절성 스무딩 파라미터 (0 < gamma <= 1)
    gamma: Option<f64>,
    /// 계절 주기
    seasonal_period: Option<usize>,
    /// 가법적 계절성 여부 (false면 승법적)
    additive: bool,
    /// 학습된 상태
    state: Option<ESState>,
}

impl ExponentialSmoothing {
    /// 단순 지수 평활법 생성
    pub fn simple(alpha: f64) -> Result<Self, ModelError> {
        if alpha <= 0.0 || alpha > 1.0 {
            return Err(ModelError::InvalidParameter(
                "alpha must be between 0 and 1".to_string(),
            ));
        }

        Ok(Self {
            alpha,
            beta: None,
            gamma: None,
            seasonal_period: None,
            additive: true,
            state: None,
        })
    }

    /// 이중 지수 평활법 (Holt's Linear) 생성
    pub fn double(alpha: f64, beta: f64) -> Result<Self, ModelError> {
        if alpha <= 0.0 || alpha > 1.0 {
            return Err(ModelError::InvalidParameter(
                "alpha must be between 0 and 1".to_string(),
            ));
        }
        if beta <= 0.0 || beta > 1.0 {
            return Err(ModelError::InvalidParameter(
                "beta must be between 0 and 1".to_string(),
            ));
        }

        Ok(Self {
            alpha,
            beta: Some(beta),
            gamma: None,
            seasonal_period: None,
            additive: true,
            state: None,
        })
    }

    /// 삼중 지수 평활법 생성
    pub fn triple(
        alpha: f64,
        beta: f64,
        gamma: f64,
        seasonal_period: usize,
        additive: bool,
    ) -> Result<Self, ModelError> {
        if alpha <= 0.0 || alpha > 1.0 {
            return Err(ModelError::InvalidParameter(
                "alpha must be between 0 and 1".to_string(),
            ));
        }
        if beta <= 0.0 || beta > 1.0 {
            return Err(ModelError::InvalidParameter(
                "beta must be between 0 and 1".to_string(),
            ));
        }
        if gamma <= 0.0 || gamma > 1.0 {
            return Err(ModelError::InvalidParameter(
                "gamma must be between 0 and 1".to_string(),
            ));
        }
        if seasonal_period < 2 {
            return Err(ModelError::InvalidParameter(
                "seasonal_period must be at least 2".to_string(),
            ));
        }

        Ok(Self {
            alpha,
            beta: Some(beta),
            gamma: Some(gamma),
            seasonal_period: Some(seasonal_period),
            additive,
            state: None,
        })
    }

    /// 스무딩 타입 반환
    pub fn smoothing_type(&self) -> SmoothingType {
        match (self.beta, self.gamma) {
            (None, None) => SmoothingType::Simple,
            (Some(_), None) => SmoothingType::Double,
            (Some(_), Some(_)) => SmoothingType::Triple,
            _ => SmoothingType::Simple,
        }
    }

    /// 단순 지수 평활법 학습
    fn train_simple(&mut self, data: &[f64]) -> Result<(), ModelError> {
        if data.len() < 2 {
            return Err(ModelError::InsufficientData {
                required: 2,
                actual: data.len(),
            });
        }

        let mut level = data[0];
        let mut residuals = Vec::with_capacity(data.len());

        for &value in data.iter().skip(1) {
            let forecast = level;
            residuals.push(value - forecast);
            level = self.alpha * value + (1.0 - self.alpha) * level;
        }

        self.state = Some(ESState {
            level,
            trend: None,
            seasonal: None,
            residual_std: std_dev(&residuals),
        });

        Ok(())
    }

    /// 이중 지수 평활법 (Holt's Linear) 학습
    fn train_double(&mut self, data: &[f64]) -> Result<(), ModelError> {
        if data.len() < 3 {
            return Err(ModelError::InsufficientData {
                required: 3,
                actual: data.len(),
            });
        }

        let beta = self
            .beta
            .ok_or_else(|| ModelError::InvalidParameter("beta is required".to_string()))?;

        // 초기화
        let mut level = data[0];
        let mut trend = data[1] - data[0];
        let mut residuals = Vec::with_capacity(data.len());

        for &value in data.iter().skip(1) {
            let forecast = level + trend;
            residuals.push(value - forecast);

            let new_level = self.alpha * value + (1.0 - self.alpha) * (level + trend);
            let new_trend = beta * (new_level - level) + (1.0 - beta) * trend;

            level = new_level;
            trend = new_trend;
        }

        self.state = Some(ESState {
            level,
            trend: Some(trend),
            seasonal: None,
            residual_std: std_dev(&residuals),
        });

        Ok(())
    }

    /// 삼중 지수 평활법 학습
    fn train_triple(&mut self, data: &[f64]) -> Result<(), ModelError> {
        let period = self.seasonal_period.ok_or_else(|| {
            ModelError::InvalidParameter("seasonal period is required".to_string())
        })?;
        if data.len() < period * 2 {
            return Err(ModelError::InsufficientData {
                required: period * 2,
                actual: data.len(),
            });
        }

        let beta = self
            .beta
            .ok_or_else(|| ModelError::InvalidParameter("beta is required".to_string()))?;
        let gamma = self
            .gamma
            .ok_or_else(|| ModelError::InvalidParameter("gamma is required".to_string()))?;

        // 초기 계절 성분 계산
        let mut seasonal = vec![0.0; period];
        let cycle_count = data.len() / period;

        if self.additive {
            // 가법적 초기화
            let cycle_means: Vec<f64> = (0..cycle_count)
                .map(|c| mean(&data[c * period..(c + 1) * period]))
                .collect();
            let _overall_mean = mean(&cycle_means);

            for i in 0..period {
                let sum: f64 = (0..cycle_count)
                    .map(|c| data[c * period + i] - cycle_means[c])
                    .sum();
                seasonal[i] = sum / cycle_count as f64;
            }

            // 정규화 (합이 0이 되도록)
            let seasonal_mean = mean(&seasonal);
            for s in &mut seasonal {
                *s -= seasonal_mean;
            }

            // 초기 레벨과 트렌드
            let mut level = mean(&data[0..period]);
            let mut trend = (mean(&data[period..period * 2]) - level) / period as f64;

            let mut residuals = Vec::with_capacity(data.len());

            // 학습
            for (i, &value) in data.iter().enumerate() {
                let season_idx = i % period;
                let forecast = level + trend + seasonal[season_idx];
                residuals.push(value - forecast);

                let new_level = self.alpha * (value - seasonal[season_idx])
                    + (1.0 - self.alpha) * (level + trend);
                let new_trend = beta * (new_level - level) + (1.0 - beta) * trend;
                let new_seasonal =
                    gamma * (value - new_level) + (1.0 - gamma) * seasonal[season_idx];

                level = new_level;
                trend = new_trend;
                seasonal[season_idx] = new_seasonal;
            }

            self.state = Some(ESState {
                level,
                trend: Some(trend),
                seasonal: Some(seasonal),
                residual_std: std_dev(&residuals),
            });
        } else {
            // 승법적 초기화
            let cycle_means: Vec<f64> = (0..cycle_count)
                .map(|c| mean(&data[c * period..(c + 1) * period]))
                .collect();

            for i in 0..period {
                let sum: f64 = (0..cycle_count)
                    .map(|c| {
                        if cycle_means[c] != 0.0 {
                            data[c * period + i] / cycle_means[c]
                        } else {
                            1.0
                        }
                    })
                    .sum();
                seasonal[i] = sum / cycle_count as f64;
            }

            // 정규화 (곱이 1이 되도록)
            let seasonal_mean = mean(&seasonal);
            if seasonal_mean != 0.0 {
                for s in &mut seasonal {
                    *s /= seasonal_mean;
                }
            }

            // 초기 레벨과 트렌드
            let mut level = mean(&data[0..period]);
            let mut trend = (mean(&data[period..period * 2]) - level) / period as f64;

            let mut residuals = Vec::with_capacity(data.len());

            // 학습
            for (i, &value) in data.iter().enumerate() {
                let season_idx = i % period;
                let s = seasonal[season_idx].max(0.001); // 0 방지
                let forecast = (level + trend) * s;
                residuals.push(value - forecast);

                let new_level = self.alpha * (value / s) + (1.0 - self.alpha) * (level + trend);
                let new_trend = beta * (new_level - level) + (1.0 - beta) * trend;
                let new_seasonal = if new_level != 0.0 {
                    gamma * (value / new_level) + (1.0 - gamma) * s
                } else {
                    s
                };

                level = new_level;
                trend = new_trend;
                seasonal[season_idx] = new_seasonal;
            }

            self.state = Some(ESState {
                level,
                trend: Some(trend),
                seasonal: Some(seasonal),
                residual_std: std_dev(&residuals),
            });
        }

        Ok(())
    }
}

impl PredictionModel for ExponentialSmoothing {
    fn train(&mut self, data: &[f64]) -> Result<(), ModelError> {
        match self.smoothing_type() {
            SmoothingType::Simple => self.train_simple(data),
            SmoothingType::Double => self.train_double(data),
            SmoothingType::Triple => self.train_triple(data),
        }
    }

    fn predict(&self, horizon: usize) -> Result<Vec<f64>, ModelError> {
        let state = self.state.as_ref().ok_or(ModelError::NotTrained)?;

        let mut forecasts = Vec::with_capacity(horizon);

        match self.smoothing_type() {
            SmoothingType::Simple => {
                // 단순: 레벨만 반복
                for _ in 0..horizon {
                    forecasts.push(state.level);
                }
            }
            SmoothingType::Double => {
                // 이중: 레벨 + 트렌드 * h
                let trend = state.trend.unwrap_or(0.0);
                for h in 1..=horizon {
                    forecasts.push(state.level + trend * h as f64);
                }
            }
            SmoothingType::Triple => {
                // 삼중: 레벨 + 트렌드 * h +/× 계절성
                let trend = state.trend.unwrap_or(0.0);
                let seasonal = state.seasonal.as_ref().ok_or_else(|| {
                    ModelError::InvalidParameter("seasonal state is missing".to_string())
                })?;
                let period = self.seasonal_period.ok_or_else(|| {
                    ModelError::InvalidParameter("seasonal period is missing".to_string())
                })?;

                for h in 1..=horizon {
                    let season_idx = (h - 1) % period;
                    let forecast = if self.additive {
                        state.level + trend * h as f64 + seasonal[season_idx]
                    } else {
                        (state.level + trend * h as f64) * seasonal[season_idx]
                    };
                    forecasts.push(forecast);
                }
            }
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
            // 오차가 시간에 따라 증가
            let error_factor = ((h + 1) as f64).sqrt();
            let margin = z * state.residual_std * error_factor;
            intervals.push((forecast - margin, forecast + margin));
        }

        Ok(intervals)
    }

    fn name(&self) -> &str {
        match self.smoothing_type() {
            SmoothingType::Simple => "SimpleExponentialSmoothing",
            SmoothingType::Double => "DoubleExponentialSmoothing",
            SmoothingType::Triple => "TripleExponentialSmoothing",
        }
    }

    fn is_trained(&self) -> bool {
        self.state.is_some()
    }
}

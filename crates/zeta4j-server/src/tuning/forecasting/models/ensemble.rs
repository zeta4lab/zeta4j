// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 앙상블 모델
//!
//! 여러 예측 모델을 조합하여 더 안정적인 예측을 수행합니다.

use serde::{Deserialize, Serialize};

use super::{ModelError, PredictionModel};

/// 앙상블 조합 방법
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombinationMethod {
    /// 단순 평균
    Average,
    /// 가중 평균
    WeightedAverage,
    /// 중앙값
    Median,
}

/// 앙상블 모델
#[derive(Debug)]
pub struct EnsembleModel {
    /// 개별 모델들
    models: Vec<Box<dyn PredictionModel>>,
    /// 모델 가중치
    weights: Vec<f64>,
    /// 조합 방법
    combination: CombinationMethod,
    /// 학습 완료 여부
    trained: bool,
}

impl EnsembleModel {
    /// 새 앙상블 모델 생성
    pub fn new(models: Vec<Box<dyn PredictionModel>>, combination: CombinationMethod) -> Self {
        let n = models.len();
        Self {
            models,
            weights: vec![1.0 / n as f64; n],
            combination,
            trained: false,
        }
    }

    /// 가중치를 지정하여 생성
    pub fn with_weights(
        models: Vec<Box<dyn PredictionModel>>,
        weights: Vec<f64>,
    ) -> Result<Self, ModelError> {
        if models.len() != weights.len() {
            return Err(ModelError::InvalidParameter(
                "Number of models and weights must match".to_string(),
            ));
        }

        // 가중치 정규화
        let sum: f64 = weights.iter().sum();
        let normalized: Vec<f64> = weights.iter().map(|w| w / sum).collect();

        Ok(Self {
            models,
            weights: normalized,
            combination: CombinationMethod::WeightedAverage,
            trained: false,
        })
    }

    /// 모델 개수 반환
    pub fn model_count(&self) -> usize {
        self.models.len()
    }

    /// 가중치 반환
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// 검증 데이터로 가중치 최적화
    pub fn optimize_weights(&mut self, validation_data: &[f64]) -> Result<(), ModelError> {
        if !self.trained {
            return Err(ModelError::NotTrained);
        }

        let horizon = validation_data.len();
        let predictions: Vec<Vec<f64>> = self
            .models
            .iter()
            .filter_map(|m| m.predict(horizon).ok())
            .collect();

        if predictions.is_empty() {
            return Err(ModelError::ConvergenceFailed(
                "Prediction failed".to_string(),
            ));
        }

        // 각 모델의 MSE 계산
        let mses: Vec<f64> = predictions
            .iter()
            .map(|preds| {
                preds
                    .iter()
                    .zip(validation_data.iter())
                    .map(|(p, a)| (p - a).powi(2))
                    .sum::<f64>()
                    / horizon as f64
            })
            .collect();

        // MSE 역수로 가중치 계산 (MSE 작을수록 높은 가중치)
        let inv_mses: Vec<f64> = mses.iter().map(|m| 1.0 / (m + 1e-10)).collect();
        let sum: f64 = inv_mses.iter().sum();
        self.weights = inv_mses.iter().map(|w| w / sum).collect();

        Ok(())
    }
}

impl PredictionModel for EnsembleModel {
    fn train(&mut self, data: &[f64]) -> Result<(), ModelError> {
        for model in &mut self.models {
            model.train(data)?;
        }
        self.trained = true;
        Ok(())
    }

    fn predict(&self, horizon: usize) -> Result<Vec<f64>, ModelError> {
        if !self.trained {
            return Err(ModelError::NotTrained);
        }

        // 각 모델의 예측 수집
        let predictions: Vec<Vec<f64>> = self
            .models
            .iter()
            .filter_map(|m| m.predict(horizon).ok())
            .collect();

        if predictions.is_empty() {
            return Err(ModelError::ConvergenceFailed(
                "All models failed to predict".to_string(),
            ));
        }

        let mut combined = vec![0.0; horizon];

        match self.combination {
            CombinationMethod::Average => {
                for preds in &predictions {
                    for (i, &p) in preds.iter().enumerate() {
                        combined[i] += p;
                    }
                }
                for c in &mut combined {
                    *c /= predictions.len() as f64;
                }
            }
            CombinationMethod::WeightedAverage => {
                for (preds, &weight) in predictions.iter().zip(self.weights.iter()) {
                    for (i, &p) in preds.iter().enumerate() {
                        combined[i] += p * weight;
                    }
                }
            }
            CombinationMethod::Median => {
                for i in 0..horizon {
                    let mut values: Vec<f64> = predictions.iter().map(|p| p[i]).collect();
                    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    combined[i] = if values.len().is_multiple_of(2) {
                        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
                    } else {
                        values[values.len() / 2]
                    };
                }
            }
        }

        Ok(combined)
    }

    fn predict_interval(
        &self,
        horizon: usize,
        confidence: f64,
    ) -> Result<Vec<(f64, f64)>, ModelError> {
        if !self.trained {
            return Err(ModelError::NotTrained);
        }

        // 각 모델의 예측 신뢰 구간 수집
        let intervals: Vec<Vec<(f64, f64)>> = self
            .models
            .iter()
            .filter_map(|m| m.predict_interval(horizon, confidence).ok())
            .collect();

        if intervals.is_empty() {
            return Err(ModelError::ConvergenceFailed(
                "All models failed to compute confidence intervals".to_string(),
            ));
        }

        let forecasts = self.predict(horizon)?;

        // 신뢰 구간 결합 (평균 폭 사용)
        let mut combined = Vec::with_capacity(horizon);
        for (i, forecast) in forecasts.iter().enumerate() {
            let avg_lower: f64 =
                intervals.iter().map(|iv| iv[i].0).sum::<f64>() / intervals.len() as f64;
            let avg_upper: f64 =
                intervals.iter().map(|iv| iv[i].1).sum::<f64>() / intervals.len() as f64;

            // 예측값 중심으로 조정
            let width = (avg_upper - avg_lower) / 2.0;
            combined.push((forecast - width, forecast + width));
        }

        Ok(combined)
    }

    fn name(&self) -> &str {
        "Ensemble"
    }

    fn is_trained(&self) -> bool {
        self.trained
    }
}

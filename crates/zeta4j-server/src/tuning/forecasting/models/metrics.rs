// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 모델 평가 지표
//!
//! 예측 모델의 성능을 평가하기 위한 다양한 지표를 제공합니다.

use serde::{Deserialize, Serialize};

use crate::tuning::forecasting::timeseries::mean;

/// 모델 평가 지표
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetrics {
    /// 평균 절대 오차 (Mean Absolute Error)
    pub mae: f64,
    /// 평균 제곱근 오차 (Root Mean Squared Error)
    pub rmse: f64,
    /// 평균 절대 백분율 오차 (Mean Absolute Percentage Error)
    pub mape: f64,
    /// 대칭 MAPE
    pub smape: f64,
    /// 결정 계수 (R²)
    pub r_squared: f64,
}

impl ModelMetrics {
    /// 예측값과 실제값으로 평가 지표 계산
    pub fn calculate(actual: &[f64], predicted: &[f64]) -> Self {
        assert_eq!(
            actual.len(),
            predicted.len(),
            "Actual and predicted values must have the same length"
        );

        let n = actual.len() as f64;

        // MAE
        let mae: f64 = actual
            .iter()
            .zip(predicted.iter())
            .map(|(a, p)| (a - p).abs())
            .sum::<f64>()
            / n;

        // RMSE
        let mse: f64 = actual
            .iter()
            .zip(predicted.iter())
            .map(|(a, p)| (a - p).powi(2))
            .sum::<f64>()
            / n;
        let rmse = mse.sqrt();

        // MAPE (0 값 제외)
        let mut mape_sum = 0.0;
        let mut mape_count = 0.0;
        for (a, p) in actual.iter().zip(predicted.iter()) {
            if a.abs() > 1e-10 {
                mape_sum += ((a - p) / a).abs();
                mape_count += 1.0;
            }
        }
        let mape = if mape_count > 0.0 {
            mape_sum / mape_count * 100.0
        } else {
            0.0
        };

        // sMAPE
        let mut smape_sum = 0.0;
        let mut smape_count = 0.0;
        for (a, p) in actual.iter().zip(predicted.iter()) {
            let denom = a.abs() + p.abs();
            if denom > 1e-10 {
                smape_sum += (a - p).abs() / denom;
                smape_count += 1.0;
            }
        }
        let smape = if smape_count > 0.0 {
            smape_sum / smape_count * 200.0
        } else {
            0.0
        };

        // R²
        let actual_mean = mean(actual);
        let ss_tot: f64 = actual.iter().map(|a| (a - actual_mean).powi(2)).sum();
        let ss_res: f64 = actual
            .iter()
            .zip(predicted.iter())
            .map(|(a, p)| (a - p).powi(2))
            .sum();
        let r_squared = if ss_tot > 1e-10 {
            1.0 - (ss_res / ss_tot)
        } else {
            0.0
        };

        Self {
            mae,
            rmse,
            mape,
            smape,
            r_squared,
        }
    }
}

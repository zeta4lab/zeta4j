//! 예측 모델
//!
//! 시계열 예측을 위한 다양한 통계 모델을 제공합니다.
//!
//! # 모델 종류
//!
//! - `ExponentialSmoothing`: 단순/이중/삼중 지수 평활법
//! - `HoltWinters`: Holt-Winters 삼중 지수 평활법 (계절성 지원)
//! - `Arima`: ARIMA/SARIMA 모델
//! - `EnsembleModel`: 여러 모델의 앙상블

mod arima;
mod ensemble;
mod exponential_smoothing;
mod holt_winters;
mod metrics;

use std::fmt::Debug;

pub use arima::{Arima, ArimaParams, SeasonalParams};
pub use ensemble::{CombinationMethod, EnsembleModel};
pub use exponential_smoothing::{ExponentialSmoothing, SmoothingType};
pub use holt_winters::{HWParams, HWType, HoltWinters};
pub use metrics::ModelMetrics;

/// 예측 모델 트레이트
pub trait PredictionModel: Send + Sync + Debug {
    /// 모델 학습
    fn train(&mut self, data: &[f64]) -> Result<(), ModelError>;

    /// 예측 수행
    fn predict(&self, horizon: usize) -> Result<Vec<f64>, ModelError>;

    /// 예측 신뢰 구간
    fn predict_interval(
        &self,
        horizon: usize,
        confidence: f64,
    ) -> Result<Vec<(f64, f64)>, ModelError>;

    /// 모델 이름
    fn name(&self) -> &str;

    /// 학습 완료 여부
    fn is_trained(&self) -> bool;
}

/// 모델 에러
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    #[error("Insufficient training data: minimum {required} required, {actual} provided")]
    InsufficientData { required: usize, actual: usize },

    #[error("Model not trained")]
    NotTrained,

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Convergence failed: {0}")]
    ConvergenceFailed(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Z-score 계산 헬퍼
pub(crate) fn z_score_for_confidence(confidence: f64) -> f64 {
    if confidence >= 0.99 {
        2.576
    } else if confidence >= 0.95 {
        1.96
    } else if confidence >= 0.90 {
        1.645
    } else {
        1.28
    }
}

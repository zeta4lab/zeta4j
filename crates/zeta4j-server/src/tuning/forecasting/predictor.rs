//! 워크로드 예측 엔진
//!
//! 시계열 데이터와 예측 모델을 사용하여 미래 워크로드를 예측합니다.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::collector::{Aggregation, DataPoint, MetricType};
use super::models::{
    CombinationMethod, EnsembleModel, ExponentialSmoothing, HWType, HoltWinters, ModelError,
    ModelMetrics, PredictionModel,
};
use super::patterns::{PatternRecognizer, RecognizedPattern};
use super::storage::TimeSeriesStorage;
use super::timeseries::TimeSeriesAnalyzer;

/// 예측 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    /// 예측 시점
    pub timestamp: DateTime<Utc>,
    /// 예측 대상 메트릭
    pub metric: MetricType,
    /// 예측값
    pub value: f64,
    /// 신뢰 구간 하한
    pub lower_bound: f64,
    /// 신뢰 구간 상한
    pub upper_bound: f64,
    /// 신뢰도 (0.0 ~ 1.0)
    pub confidence: f64,
    /// 사용된 모델
    pub model: String,
}

impl Prediction {
    /// 예측값이 임계값을 초과하는지 확인
    pub fn exceeds_threshold(&self, threshold: f64) -> bool {
        self.value > threshold
    }

    /// 신뢰 구간 폭
    pub fn interval_width(&self) -> f64 {
        self.upper_bound - self.lower_bound
    }

    /// 상대적 불확실성 (신뢰 구간 폭 / 예측값)
    pub fn relative_uncertainty(&self) -> f64 {
        if self.value > 0.0 {
            self.interval_width() / self.value
        } else {
            f64::INFINITY
        }
    }
}

/// 예측기 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictorConfig {
    /// 기본 예측 horizon (시간 단위)
    pub default_horizon_hours: usize,
    /// 신뢰 수준 (0.0 ~ 1.0)
    pub confidence_level: f64,
    /// 최소 학습 데이터 크기
    pub min_training_size: usize,
    /// 기본 계절 주기 (시간 단위)
    pub default_seasonal_period: usize,
    /// 자동 모델 선택 활성화
    pub auto_model_selection: bool,
    /// 앙상블 사용 여부
    pub use_ensemble: bool,
}

impl Default for PredictorConfig {
    fn default() -> Self {
        Self {
            default_horizon_hours: 24,
            confidence_level: 0.95,
            min_training_size: 168,      // 1주일 시간별
            default_seasonal_period: 24, // 24시간
            auto_model_selection: true,
            use_ensemble: true,
        }
    }
}

/// 모델 선택 전략
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSelectionStrategy {
    /// 가장 낮은 MAPE 기준
    LowestMAPE,
    /// 가장 낮은 RMSE 기준
    LowestRMSE,
    /// 가장 높은 R² 기준
    HighestRSquared,
    /// 앙상블 사용
    Ensemble,
}

/// 모델 선택기
pub struct ModelSelector {
    strategy: ModelSelectionStrategy,
    validation_ratio: f64,
}

impl ModelSelector {
    pub fn new(strategy: ModelSelectionStrategy) -> Self {
        Self {
            strategy,
            validation_ratio: 0.2,
        }
    }

    /// 최적 모델 선택
    pub fn select_best_model(
        &self,
        data: &[f64],
        seasonal_period: usize,
    ) -> Result<Box<dyn PredictionModel>, ModelError> {
        if data.len() < 48 {
            return Err(ModelError::InsufficientData {
                required: 48,
                actual: data.len(),
            });
        }

        let split_idx = (data.len() as f64 * (1.0 - self.validation_ratio)) as usize;
        let (train, valid) = data.split_at(split_idx);

        // 후보 모델들
        let mut candidates: Vec<(Box<dyn PredictionModel>, ModelMetrics)> = Vec::new();

        // 1. 단순 지수 평활
        if let Ok(mut model) = ExponentialSmoothing::simple(0.3)
            && model.train(train).is_ok()
            && let Ok(preds) = model.predict(valid.len())
        {
            let metrics = ModelMetrics::calculate(valid, &preds);
            candidates.push((Box::new(model), metrics));
        }

        // 2. 이중 지수 평활
        if let Ok(mut model) = ExponentialSmoothing::double(0.3, 0.1)
            && model.train(train).is_ok()
            && let Ok(preds) = model.predict(valid.len())
        {
            let metrics = ModelMetrics::calculate(valid, &preds);
            candidates.push((Box::new(model), metrics));
        }

        // 3. Holt-Winters Additive
        if train.len() >= seasonal_period * 2 {
            let mut model = HoltWinters::new(HWType::Additive, seasonal_period);
            if model.train(train).is_ok()
                && let Ok(preds) = model.predict(valid.len())
            {
                let metrics = ModelMetrics::calculate(valid, &preds);
                candidates.push((Box::new(model), metrics));
            }
        }

        // 4. Holt-Winters Multiplicative
        if train.len() >= seasonal_period * 2 && train.iter().all(|&v| v > 0.0) {
            let mut model = HoltWinters::new(HWType::Multiplicative, seasonal_period);
            if model.train(train).is_ok()
                && let Ok(preds) = model.predict(valid.len())
            {
                let metrics = ModelMetrics::calculate(valid, &preds);
                candidates.push((Box::new(model), metrics));
            }
        }

        if candidates.is_empty() {
            return Err(ModelError::ConvergenceFailed(
                "All models failed to train".to_string(),
            ));
        }

        // 전략에 따라 선택
        let best_idx = match self.strategy {
            ModelSelectionStrategy::LowestMAPE => candidates
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.1.mape
                        .partial_cmp(&b.1.mape)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0),

            ModelSelectionStrategy::LowestRMSE => candidates
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.1.rmse
                        .partial_cmp(&b.1.rmse)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0),

            ModelSelectionStrategy::HighestRSquared => candidates
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    a.1.r_squared
                        .partial_cmp(&b.1.r_squared)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0),

            ModelSelectionStrategy::Ensemble => {
                // 앙상블로 결합
                let models: Vec<Box<dyn PredictionModel>> =
                    candidates.into_iter().map(|(m, _)| m).collect();
                let mut ensemble = EnsembleModel::new(models, CombinationMethod::WeightedAverage);
                ensemble.train(data)?;
                return Ok(Box::new(ensemble));
            }
        };

        // 최적 모델을 전체 데이터로 재학습
        let (mut best_model, _) = candidates.swap_remove(best_idx);
        best_model.train(data)?;
        Ok(best_model)
    }
}

impl Default for ModelSelector {
    fn default() -> Self {
        Self::new(ModelSelectionStrategy::LowestMAPE)
    }
}

/// 학습된 모델 정보
struct TrainedModel {
    model: Box<dyn PredictionModel>,
    trained_at: DateTime<Utc>,
    metrics: Option<ModelMetrics>,
    data_points: usize,
}

/// 워크로드 예측기
pub struct WorkloadPredictor {
    /// 메트릭별 학습된 모델
    models: RwLock<HashMap<MetricType, TrainedModel>>,
    /// 시계열 저장소
    storage: Arc<TimeSeriesStorage>,
    /// 모델 선택기
    model_selector: ModelSelector,
    /// 패턴 인식기
    pattern_recognizer: PatternRecognizer,
    /// 시계열 분석기
    ts_analyzer: TimeSeriesAnalyzer,
    /// 설정
    config: PredictorConfig,
}

impl WorkloadPredictor {
    /// 새 예측기 생성
    pub fn new(storage: Arc<TimeSeriesStorage>, config: PredictorConfig) -> Self {
        let strategy = if config.use_ensemble {
            ModelSelectionStrategy::Ensemble
        } else if config.auto_model_selection {
            ModelSelectionStrategy::LowestMAPE
        } else {
            ModelSelectionStrategy::LowestRMSE
        };

        Self {
            models: RwLock::new(HashMap::new()),
            storage,
            model_selector: ModelSelector::new(strategy),
            pattern_recognizer: PatternRecognizer::default(),
            ts_analyzer: TimeSeriesAnalyzer::default(),
            config,
        }
    }

    /// 특정 메트릭 예측
    pub fn predict(
        &self,
        metric: MetricType,
        horizon_hours: usize,
    ) -> Result<Vec<Prediction>, PredictorError> {
        let models = self.models.read();
        let trained = models
            .get(&metric)
            .ok_or(PredictorError::ModelNotTrained(metric))?;

        let forecasts = trained
            .model
            .predict(horizon_hours)
            .map_err(PredictorError::ModelError)?;

        let intervals = trained
            .model
            .predict_interval(horizon_hours, self.config.confidence_level)
            .map_err(PredictorError::ModelError)?;

        let now = Utc::now();
        let mut predictions = Vec::with_capacity(horizon_hours);

        for (h, (value, (lower, upper))) in forecasts.iter().zip(intervals.iter()).enumerate() {
            let timestamp = now + chrono::Duration::hours((h + 1) as i64);
            predictions.push(Prediction {
                timestamp,
                metric,
                value: *value,
                lower_bound: *lower,
                upper_bound: *upper,
                confidence: self.config.confidence_level,
                model: trained.model.name().to_string(),
            });
        }

        Ok(predictions)
    }

    /// 모든 학습된 메트릭 예측
    pub fn predict_all(
        &self,
        horizon_hours: usize,
    ) -> HashMap<MetricType, Result<Vec<Prediction>, PredictorError>> {
        let models = self.models.read();
        models
            .keys()
            .map(|&metric| (metric, self.predict(metric, horizon_hours)))
            .collect()
    }

    /// 피크 시간 예측
    pub fn predict_peak(
        &self,
        metric: MetricType,
        within_hours: usize,
    ) -> Result<Option<Prediction>, PredictorError> {
        let predictions = self.predict(metric, within_hours)?;
        Ok(predictions.into_iter().max_by(|a, b| {
            a.value
                .partial_cmp(&b.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        }))
    }

    /// 임계값 초과 예측
    pub fn predict_threshold_breach(
        &self,
        metric: MetricType,
        threshold: f64,
        within_hours: usize,
    ) -> Result<Vec<Prediction>, PredictorError> {
        let predictions = self.predict(metric, within_hours)?;
        Ok(predictions
            .into_iter()
            .filter(|p| p.exceeds_threshold(threshold))
            .collect())
    }

    /// 모델 학습/재학습
    pub fn train(&self, metric: MetricType) -> Result<ModelMetrics, PredictorError> {
        // 저장소에서 데이터 조회
        let end = Utc::now();
        let start = end - chrono::Duration::days(30); // 최근 30일
        let data = self.storage.query(metric, start, end, Aggregation::None);

        if data.len() < self.config.min_training_size {
            return Err(PredictorError::InsufficientData {
                required: self.config.min_training_size,
                actual: data.len(),
            });
        }

        let values: Vec<f64> = data.iter().map(|p| p.value).collect();

        // 주기 탐지
        let period = self
            .ts_analyzer
            .detect_period(&values, Some(2), Some(values.len() / 4))
            .unwrap_or(self.config.default_seasonal_period);

        // 최적 모델 선택 및 학습
        let mut model = self
            .model_selector
            .select_best_model(&values, period)
            .map_err(PredictorError::ModelError)?;

        // 교차 검증으로 메트릭 계산
        let split_idx = (values.len() as f64 * 0.8) as usize;
        let (train, test) = values.split_at(split_idx);

        let test_model = self
            .model_selector
            .select_best_model(train, period)
            .map_err(PredictorError::ModelError)?;

        let predictions = test_model
            .predict(test.len())
            .map_err(PredictorError::ModelError)?;

        let metrics = ModelMetrics::calculate(test, &predictions);

        // 전체 데이터로 최종 학습
        model.train(&values).map_err(PredictorError::ModelError)?;

        // 모델 저장
        let trained = TrainedModel {
            model,
            trained_at: Utc::now(),
            metrics: Some(metrics.clone()),
            data_points: values.len(),
        };

        self.models.write().insert(metric, trained);

        Ok(metrics)
    }

    /// 모든 기본 메트릭 학습
    pub fn train_all(&self) -> HashMap<MetricType, Result<ModelMetrics, PredictorError>> {
        let metrics = [
            MetricType::QueriesPerSecond,
            MetricType::AvgResponseTime,
            MetricType::P95ResponseTime,
            MetricType::ActiveConnections,
            MetricType::CacheHitRate,
        ];

        metrics
            .into_iter()
            .map(|metric| (metric, self.train(metric)))
            .collect()
    }

    /// 패턴 인식 수행
    pub fn recognize_patterns(&self, metric: MetricType) -> Vec<RecognizedPattern> {
        let end = Utc::now();
        let start = end - chrono::Duration::days(14); // 최근 2주
        let data = self.storage.query(metric, start, end, Aggregation::None);

        self.pattern_recognizer.recognize_all(&data)
    }

    /// 학습된 모델 정보 조회
    pub fn get_model_info(&self, metric: MetricType) -> Option<ModelInfo> {
        let models = self.models.read();
        models.get(&metric).map(|t| ModelInfo {
            model_name: t.model.name().to_string(),
            trained_at: t.trained_at,
            data_points: t.data_points,
            metrics: t.metrics.clone(),
        })
    }

    /// 모델 제거
    pub fn remove_model(&self, metric: MetricType) -> bool {
        self.models.write().remove(&metric).is_some()
    }

    /// 학습된 메트릭 목록
    pub fn trained_metrics(&self) -> Vec<MetricType> {
        self.models.read().keys().copied().collect()
    }

    /// 예측 정확도 평가 (실제 값과 비교)
    pub fn evaluate(
        &self,
        metric: MetricType,
        actual: &[DataPoint],
    ) -> Result<ModelMetrics, PredictorError> {
        let models = self.models.read();
        let trained = models
            .get(&metric)
            .ok_or(PredictorError::ModelNotTrained(metric))?;

        let predictions = trained
            .model
            .predict(actual.len())
            .map_err(PredictorError::ModelError)?;

        let actual_values: Vec<f64> = actual.iter().map(|p| p.value).collect();
        Ok(ModelMetrics::calculate(&actual_values, &predictions))
    }
}

/// 모델 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model_name: String,
    pub trained_at: DateTime<Utc>,
    pub data_points: usize,
    pub metrics: Option<ModelMetrics>,
}

/// 예측기 에러
#[derive(Debug, Clone, thiserror::Error)]
pub enum PredictorError {
    #[error("Model not trained for metric {0:?}")]
    ModelNotTrained(MetricType),

    #[error("Insufficient training data: minimum {required} required, {actual} available")]
    InsufficientData { required: usize, actual: usize },

    #[error("Model error: {0}")]
    ModelError(#[from] ModelError),

    #[error("Storage error: {0}")]
    StorageError(String),
}

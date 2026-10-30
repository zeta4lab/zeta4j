//! 통계 기반 워크로드 예측 시스템
//!
//! 시계열 데이터 수집, 패턴 인식, 워크로드 예측을 통해
//! 사전 최적화를 수행하는 시스템입니다.
//!
//! # 구성요소
//!
//! - `collector`: 메트릭 수집기
//! - `storage`: 시계열 저장소
//! - `timeseries`: 시계열 분석 (STL 분해, ACF, 이상치 탐지)
//! - `patterns`: 패턴 인식 (시간대별, 요일별, 스파이크, 트렌드)
//! - `models`: 예측 모델 (지수 평활법, Holt-Winters, 앙상블)
//! - `predictor`: 워크로드 예측 엔진
//! - `actions`: 자동 최적화 액션
//!
//! # 사용 예시
//!
//! ```ignore
//! use std::sync::Arc;
//! use zeta4j_server::tuning::forecasting::{
//!     TimeSeriesStorage, StorageConfig,
//!     WorkloadPredictor, PredictorConfig,
//!     ActionEngine, ActionEngineConfig,
//!     MetricType,
//! };
//!
//! // 저장소 생성
//! let storage = Arc::new(TimeSeriesStorage::new(StorageConfig::default()));
//!
//! // 예측기 생성 및 학습
//! let predictor = WorkloadPredictor::new(storage.clone(), PredictorConfig::default());
//! predictor.train(MetricType::QueriesPerSecond)?;
//!
//! // 24시간 예측
//! let predictions = predictor.predict(MetricType::QueriesPerSecond, 24)?;
//!
//! // 패턴 인식
//! let patterns = predictor.recognize_patterns(MetricType::QueriesPerSecond);
//!
//! // 액션 엔진
//! let engine = ActionEngine::new(ActionEngineConfig::default());
//! let actions = engine.generate_actions(&predictions, &patterns);
//! ```

pub mod actions;
pub mod collector;
pub mod models;
pub mod patterns;
pub mod predictor;
pub mod storage;
pub mod timeseries;

pub use collector::{
    Aggregation, CollectorConfig, DataPoint, MetricCollector, MetricType, SamplingConfig,
};

pub use storage::{DownsampleRule, RetentionPolicy, StorageConfig, TimeSeriesStorage};

pub use timeseries::{
    AnalyzerConfig,
    AnomalyMethod,
    CriticalValues,
    DecomposedSeries,
    DecompositionMethod,
    StationarityResult,
    TimeSeriesAnalyzer,
    correlation,
    covariance,
    // 통계 유틸리티
    mean,
    median,
    percentile,
    std_dev,
    variance,
};

pub use patterns::{
    PatternConfig, PatternRecognizer, PatternType, RecognizedPattern, SpikeEvent, TrendDirection,
};

pub use models::{
    // ARIMA
    Arima,
    ArimaParams,
    CombinationMethod,
    // 앙상블
    EnsembleModel,
    // 지수 평활법
    ExponentialSmoothing,
    HWParams,
    HWType,
    // Holt-Winters
    HoltWinters,
    ModelError,
    ModelMetrics,
    PredictionModel,
    SeasonalParams,
    SmoothingType,
};

pub use predictor::{
    ModelInfo, ModelSelectionStrategy, ModelSelector, Prediction, PredictorConfig, PredictorError,
    WorkloadPredictor,
};

pub use actions::{
    ActionEngine, ActionEngineConfig, ActionExecution, ActionImpact, ActionResult, ActionTrigger,
    ActionType, AlertSeverity, BatchJobType, MemoryComponent, OptimizationAction, Priority,
};

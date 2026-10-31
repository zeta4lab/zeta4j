// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! ML Pipeline 모듈
//!
//! Neo4j GDS ML Pipeline과 호환되는 머신러닝 파이프라인 구현
//!
//! # 주요 기능
//!
//! - **TrainTestSplit**: 훈련/테스트 데이터 분할 (stratified sampling 지원)
//! - **CrossValidator**: K-fold 교차 검증
//! - **Metrics**: 분류 평가 지표 (accuracy, precision, recall, f1, auc)
//! - **FeatureScaler**: Min-Max, Standard 스케일링
//! - **Models**: LogisticRegression, RandomForest
//! - **Pipelines**: NodeClassification, LinkPrediction 파이프라인
//!
//! # 사용 예시
//!
//! ```ignore
//! use zeta4j_graph::ml::{TrainTestSplit, TrainTestSplitConfig};
//!
//! let config = TrainTestSplitConfig::default();
//! let (train, test) = TrainTestSplit::split(&data, &labels, &config)?;
//! ```

pub mod cross_validation;
pub mod feature;
pub mod metrics;
pub mod models;
pub mod pipeline;
pub mod scaler;
pub mod split;

// Re-exports
pub use cross_validation::{CVResult, CrossValidator, CrossValidatorConfig};
pub use feature::{FeatureExtractor, LinkFeature, LinkFeatureCombiner};
pub use metrics::{BinaryMetrics, ClassificationMetric, ClassificationReport};
pub use models::{
    LogisticConfig, LogisticRegression, Model, ModelType, RandomForest, RandomForestConfig,
};
pub use pipeline::{
    LinkPredictionConfig, LinkPredictionPipeline, ModelCandidate, NodeClassificationConfig,
    NodeClassificationPipeline, NodePropertyStep, Pipeline, PipelineCatalog, PipelineConfig,
    PipelineStatus, TrainedModel,
};
pub use scaler::{FeatureScaler, ScalerConfig, ScalerType};
pub use split::{SplitResult, TrainTestSplit, TrainTestSplitConfig};

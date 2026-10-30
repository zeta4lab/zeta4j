//! Pipeline Base
//!
//! 파이프라인 공통 트레이트 및 구조체

use crate::core::Value;
use crate::ml::cross_validation::CVResult;
use crate::ml::metrics::ClassificationMetric;
use crate::ml::models::{
    LogisticConfig, LogisticRegression, Model, ModelType, RandomForest, RandomForestConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 파이프라인 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStatus {
    /// 생성됨
    Created,
    /// 설정 중
    Configured,
    /// 학습 중
    Training,
    /// 학습 완료
    Trained,
    /// 오류
    Error,
}

/// 파이프라인 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// 테스트 데이터 비율
    pub test_fraction: f64,
    /// 검증 데이터 비율
    pub validation_fraction: f64,
    /// 교차 검증 폴드 수
    pub n_folds: usize,
    /// 평가 지표
    pub metrics: Vec<ClassificationMetric>,
    /// 랜덤 시드
    pub random_seed: Option<u64>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            test_fraction: 0.2,
            validation_fraction: 0.0,
            n_folds: 5,
            metrics: vec![ClassificationMetric::F1Weighted],
            random_seed: None,
        }
    }
}

/// 모델 후보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelCandidate {
    LogisticRegression(LogisticConfig),
    RandomForest(RandomForestConfig),
}

impl ModelCandidate {
    /// 로지스틱 회귀 기본 후보
    pub fn logistic() -> Self {
        Self::LogisticRegression(LogisticConfig::default())
    }

    /// 로지스틱 회귀 (설정 포함)
    pub fn logistic_with_config(penalty: f64, max_iterations: usize) -> Self {
        Self::LogisticRegression(LogisticConfig {
            penalty,
            max_iterations,
            ..Default::default()
        })
    }

    /// 랜덤 포레스트 기본 후보
    pub fn random_forest() -> Self {
        Self::RandomForest(RandomForestConfig::default())
    }

    /// 랜덤 포레스트 (설정 포함)
    pub fn random_forest_with_config(n_estimators: usize, max_depth: Option<usize>) -> Self {
        Self::RandomForest(RandomForestConfig {
            n_estimators,
            max_depth,
            ..Default::default()
        })
    }

    /// 모델 이름
    pub fn name(&self) -> &str {
        match self {
            Self::LogisticRegression(_) => "LogisticRegression",
            Self::RandomForest(_) => "RandomForest",
        }
    }

    /// 모델 종류
    pub fn model_type(&self) -> ModelType {
        match self {
            Self::LogisticRegression(_) => ModelType::LogisticRegression,
            Self::RandomForest(_) => ModelType::RandomForest,
        }
    }

    /// 모델 인스턴스 생성
    pub fn create_model(&self) -> Box<dyn ModelTrait> {
        match self {
            Self::LogisticRegression(config) => Box::new(LogisticRegression::new(config.clone())),
            Self::RandomForest(config) => Box::new(RandomForest::new(config.clone())),
        }
    }

    /// 모델 파라미터를 HashMap으로 추출
    pub fn to_params(&self) -> HashMap<String, Value> {
        let mut params = HashMap::new();

        match self {
            Self::LogisticRegression(config) => {
                params.insert(
                    "learning_rate".to_string(),
                    Value::Float(config.learning_rate),
                );
                params.insert("penalty".to_string(), Value::Float(config.penalty));
                params.insert(
                    "max_iterations".to_string(),
                    Value::Integer(config.max_iterations as i64),
                );
                params.insert("tolerance".to_string(), Value::Float(config.tolerance));
                if let Some(seed) = config.random_seed {
                    params.insert("random_seed".to_string(), Value::Integer(seed as i64));
                }
                if let Some(batch) = config.batch_size {
                    params.insert("batch_size".to_string(), Value::Integer(batch as i64));
                }
            }
            Self::RandomForest(config) => {
                params.insert(
                    "n_estimators".to_string(),
                    Value::Integer(config.n_estimators as i64),
                );
                if let Some(depth) = config.max_depth {
                    params.insert("max_depth".to_string(), Value::Integer(depth as i64));
                }
                params.insert(
                    "min_samples_split".to_string(),
                    Value::Integer(config.min_samples_split as i64),
                );
                params.insert(
                    "min_samples_leaf".to_string(),
                    Value::Integer(config.min_samples_leaf as i64),
                );
                params.insert("bootstrap".to_string(), Value::Boolean(config.bootstrap));
                if let Some(seed) = config.random_seed {
                    params.insert("random_seed".to_string(), Value::Integer(seed as i64));
                }
            }
        }

        params
    }
}

/// 훈련된 모델
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainedModel {
    /// 파이프라인 이름
    pub pipeline_name: String,
    /// 모델 이름
    pub model_name: String,
    /// 모델 종류
    pub model_type: ModelType,
    /// 최적 파라미터
    pub best_params: HashMap<String, Value>,
    /// 교차 검증 점수
    pub cv_score: f64,
    /// 테스트 점수
    pub test_score: f64,
    /// 사용된 평가 지표
    pub metric: ClassificationMetric,
    /// 학습 시간 (밀리초)
    pub training_time_ms: u64,
    /// 직렬화된 모델 데이터
    pub model_data: Vec<u8>,
    /// 피처 속성 목록
    pub feature_properties: Vec<String>,
}

impl TrainedModel {
    /// 모델 역직렬화
    pub fn load_model(&self) -> Option<Box<dyn ModelTrait>> {
        match self.model_type {
            ModelType::LogisticRegression => LogisticRegression::from_bytes(&self.model_data)
                .map(|m| Box::new(m) as Box<dyn ModelTrait>),
            ModelType::RandomForest => RandomForest::from_bytes(&self.model_data)
                .map(|m| Box::new(m) as Box<dyn ModelTrait>),
        }
    }
}

/// Model 트레이트의 object-safe 버전
pub trait ModelTrait: Send + Sync {
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]);
    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64>;
    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>>;
    fn name(&self) -> &str;
    fn model_type(&self) -> ModelType;
    fn to_bytes(&self) -> Vec<u8>;
}

impl ModelTrait for LogisticRegression {
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]) {
        Model::fit(self, features, labels)
    }
    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64> {
        Model::predict(self, features)
    }
    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>> {
        Model::predict_proba(self, features)
    }
    fn name(&self) -> &str {
        Model::name(self)
    }
    fn model_type(&self) -> ModelType {
        Model::model_type(self)
    }
    fn to_bytes(&self) -> Vec<u8> {
        Model::to_bytes(self)
    }
}

impl ModelTrait for RandomForest {
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]) {
        Model::fit(self, features, labels)
    }
    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64> {
        Model::predict(self, features)
    }
    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>> {
        Model::predict_proba(self, features)
    }
    fn name(&self) -> &str {
        Model::name(self)
    }
    fn model_type(&self) -> ModelType {
        Model::model_type(self)
    }
    fn to_bytes(&self) -> Vec<u8> {
        Model::to_bytes(self)
    }
}

/// Pipeline 트레이트
pub trait Pipeline {
    /// 파이프라인 이름
    fn name(&self) -> &str;

    /// 상태 조회
    fn status(&self) -> PipelineStatus;

    /// 피처 속성 목록
    fn feature_properties(&self) -> &[String];

    /// 모델 후보 목록
    fn model_candidates(&self) -> &[ModelCandidate];

    /// 훈련 결과 조회
    fn trained_model(&self) -> Option<&TrainedModel>;
}

/// 모델 선택 통계
#[derive(Debug, Clone)]
pub struct ModelSelectionStats {
    /// 각 모델별 CV 결과
    pub model_results: Vec<(String, CVResult)>,
    /// 선택된 모델 인덱스
    pub selected_index: usize,
    /// 선택 이유
    pub selection_reason: String,
}

impl ModelSelectionStats {
    /// 최적 모델 이름
    pub fn best_model_name(&self) -> &str {
        self.model_results
            .get(self.selected_index)
            .map(|(name, _)| name.as_str())
            .unwrap_or("Unknown")
    }

    /// 최적 모델 점수
    pub fn best_score(&self) -> f64 {
        self.model_results
            .get(self.selected_index)
            .map(|(_, result)| result.mean_score)
            .unwrap_or(0.0)
    }
}

//! ML Models
//!
//! 머신러닝 모델 구현
//!
//! - LogisticRegression: SGD 기반 로지스틱 회귀
//! - RandomForest: 결정 트리 앙상블

mod forest;
mod logistic;

pub use forest::{DecisionTree, RandomForest, RandomForestConfig};
pub use logistic::{LogisticConfig, LogisticRegression};

use serde::{Deserialize, Serialize};

/// 모델 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelType {
    LogisticRegression,
    RandomForest,
}

impl ModelType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LogisticRegression => "LogisticRegression",
            Self::RandomForest => "RandomForest",
        }
    }
}

/// ML Model 트레이트
pub trait Model {
    /// 모델 학습
    fn fit(&mut self, features: &[Vec<f64>], labels: &[i64]);

    /// 예측 (클래스 레이블)
    fn predict(&self, features: &[Vec<f64>]) -> Vec<i64>;

    /// 예측 확률 (각 클래스별 확률)
    fn predict_proba(&self, features: &[Vec<f64>]) -> Vec<Vec<f64>>;

    /// 모델 이름
    fn name(&self) -> &str;

    /// 모델 종류
    fn model_type(&self) -> ModelType;

    /// 직렬화
    fn to_bytes(&self) -> Vec<u8>;

    /// 역직렬화
    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized;
}

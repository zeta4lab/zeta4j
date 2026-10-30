//! K-Fold Cross Validation
//!
//! 교차 검증을 통한 모델 평가

use super::metrics::{ClassificationMetric, ClassificationReport};
use super::models::Model;
use super::split::{TrainTestSplit, TrainTestSplitConfig};
use serde::{Deserialize, Serialize};

/// 교차 검증 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossValidatorConfig {
    /// 폴드 수 (기본: 5)
    pub n_folds: usize,
    /// 셔플 여부 (기본: true)
    pub shuffle: bool,
    /// 층화 샘플링 (기본: true)
    pub stratified: bool,
    /// 랜덤 시드
    pub random_seed: Option<u64>,
    /// 평가 지표 (기본: F1_WEIGHTED)
    pub scoring: ClassificationMetric,
}

impl Default for CrossValidatorConfig {
    fn default() -> Self {
        Self {
            n_folds: 5,
            shuffle: true,
            stratified: true,
            random_seed: None,
            scoring: ClassificationMetric::F1Weighted,
        }
    }
}

/// 교차 검증 결과
#[derive(Debug, Clone)]
pub struct CVResult {
    /// 각 폴드별 점수
    pub fold_scores: Vec<f64>,
    /// 평균 점수
    pub mean_score: f64,
    /// 표준 편차
    pub std_score: f64,
    /// 사용된 지표
    pub metric: ClassificationMetric,
    /// 각 폴드별 상세 보고서
    pub fold_reports: Vec<ClassificationReport>,
}

impl CVResult {
    /// 새 결과 생성
    pub fn new(
        scores: Vec<f64>,
        metric: ClassificationMetric,
        reports: Vec<ClassificationReport>,
    ) -> Self {
        let n = scores.len();
        let mean = if n > 0 {
            scores.iter().sum::<f64>() / n as f64
        } else {
            0.0
        };

        let variance = if n > 1 {
            scores.iter().map(|&s| (s - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };

        Self {
            fold_scores: scores,
            mean_score: mean,
            std_score: variance.sqrt(),
            metric,
            fold_reports: reports,
        }
    }

    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "{}: {:.4} (+/- {:.4})",
            self.metric.name(),
            self.mean_score,
            self.std_score * 2.0
        )
    }
}

/// Cross Validator
pub struct CrossValidator {
    config: CrossValidatorConfig,
}

impl CrossValidator {
    /// 새 교차 검증기 생성
    pub fn new(config: CrossValidatorConfig) -> Self {
        Self { config }
    }

    /// 기본 설정으로 생성
    pub fn default_cv() -> Self {
        Self::new(CrossValidatorConfig::default())
    }

    /// 설정 조회
    pub fn config(&self) -> &CrossValidatorConfig {
        &self.config
    }

    /// 교차 검증 수행
    ///
    /// # Arguments
    /// * `model` - 평가할 모델
    /// * `features` - 피처 행렬 (samples x features)
    /// * `labels` - 레이블 벡터
    ///
    /// # Returns
    /// 교차 검증 결과
    pub fn cross_validate<M: Model + Clone>(
        &self,
        model: &M,
        features: &[Vec<f64>],
        labels: &[i64],
    ) -> CVResult {
        if features.is_empty() || labels.is_empty() || features.len() != labels.len() {
            return CVResult::new(vec![], self.config.scoring, vec![]);
        }

        // 인덱스 생성
        let indices: Vec<usize> = (0..features.len()).collect();

        // 폴드 분할
        let split_config = TrainTestSplitConfig {
            stratify: self.config.stratified,
            random_seed: self.config.random_seed,
            ..Default::default()
        };

        let folds =
            TrainTestSplit::kfold_split(&indices, Some(labels), self.config.n_folds, &split_config);

        let mut scores = Vec::with_capacity(self.config.n_folds);
        let mut reports = Vec::with_capacity(self.config.n_folds);

        // 각 폴드에서 학습 및 평가
        for (train_indices, test_indices) in folds {
            // 훈련/테스트 데이터 준비
            let train_features: Vec<Vec<f64>> =
                train_indices.iter().map(|&i| features[i].clone()).collect();
            let train_labels: Vec<i64> = train_indices.iter().map(|&i| labels[i]).collect();

            let test_features: Vec<Vec<f64>> =
                test_indices.iter().map(|&i| features[i].clone()).collect();
            let test_labels: Vec<i64> = test_indices.iter().map(|&i| labels[i]).collect();

            // 모델 복제 및 학습
            let mut fold_model = model.clone();
            fold_model.fit(&train_features, &train_labels);

            // 예측
            let predictions = fold_model.predict(&test_features);

            // 평가
            let report = ClassificationReport::from_predictions(&test_labels, &predictions);
            let score = report.get_metric(self.config.scoring);

            scores.push(score);
            reports.push(report);
        }

        CVResult::new(scores, self.config.scoring, reports)
    }

    /// 여러 모델 비교 (GridSearchCV-like)
    pub fn compare_models<M: Model + Clone>(
        &self,
        models: &[M],
        features: &[Vec<f64>],
        labels: &[i64],
    ) -> Vec<(usize, CVResult)> {
        models
            .iter()
            .enumerate()
            .map(|(idx, model)| {
                let result = self.cross_validate(model, features, labels);
                (idx, result)
            })
            .collect()
    }

    /// 최적 모델 선택
    pub fn select_best<M: Model + Clone>(
        &self,
        models: &[M],
        features: &[Vec<f64>],
        labels: &[i64],
    ) -> Option<(usize, CVResult)> {
        let results = self.compare_models(models, features, labels);
        results.into_iter().max_by(|a, b| {
            a.1.mean_score
                .partial_cmp(&b.1.mean_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

/// 간편 교차 검증 함수
pub fn cross_val_score<M: Model + Clone>(
    model: &M,
    features: &[Vec<f64>],
    labels: &[i64],
    cv: usize,
    scoring: ClassificationMetric,
) -> CVResult {
    let config = CrossValidatorConfig {
        n_folds: cv,
        scoring,
        ..Default::default()
    };
    let validator = CrossValidator::new(config);
    validator.cross_validate(model, features, labels)
}

//! Node Classification Pipeline
//!
//! 노드 분류를 위한 ML 파이프라인

use super::base::{
    ModelCandidate, ModelSelectionStats, Pipeline, PipelineConfig, PipelineStatus, TrainedModel,
};
use crate::core::Value;
use crate::ml::cross_validation::{CVResult, CrossValidator, CrossValidatorConfig};
use crate::ml::feature::{FeatureExtractor, NodePropertyStep};
use crate::ml::metrics::{ClassificationMetric, ClassificationReport};
use crate::ml::scaler::{FeatureScaler, ScalerType};
use crate::ml::split::{TrainTestSplit, TrainTestSplitConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Node Classification 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeClassificationConfig {
    /// 기본 파이프라인 설정
    pub pipeline: PipelineConfig,
    /// 타겟 속성 (분류할 레이블)
    pub target_property: String,
    /// 피처 스케일링 타입
    pub scaler_type: ScalerType,
}

impl Default for NodeClassificationConfig {
    fn default() -> Self {
        Self {
            pipeline: PipelineConfig::default(),
            target_property: "class".to_string(),
            scaler_type: ScalerType::Standard,
        }
    }
}

/// Node Classification Pipeline
#[derive(Debug, Clone)]
pub struct NodeClassificationPipeline {
    /// 파이프라인 이름
    name: String,
    /// 상태
    status: PipelineStatus,
    /// 설정
    config: NodeClassificationConfig,
    /// 노드 속성 스텝
    node_property_steps: Vec<NodePropertyStep>,
    /// 피처 속성
    feature_properties: Vec<String>,
    /// 모델 후보
    model_candidates: Vec<ModelCandidate>,
    /// 피처 추출기
    feature_extractor: FeatureExtractor,
    /// 피처 스케일러
    scaler: Option<FeatureScaler>,
    /// 훈련된 모델
    trained_model: Option<TrainedModel>,
    /// 모델 선택 통계
    model_selection_stats: Option<ModelSelectionStats>,
}

impl NodeClassificationPipeline {
    /// 새 파이프라인 생성
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: PipelineStatus::Created,
            config: NodeClassificationConfig::default(),
            node_property_steps: Vec::new(),
            feature_properties: Vec::new(),
            model_candidates: Vec::new(),
            feature_extractor: FeatureExtractor::new(),
            scaler: None,
            trained_model: None,
            model_selection_stats: None,
        }
    }

    /// 설정 지정
    pub fn with_config(mut self, config: NodeClassificationConfig) -> Self {
        self.config = config;
        self
    }

    /// 노드 속성 스텝 추가
    pub fn add_node_property(&mut self, step: NodePropertyStep) {
        self.node_property_steps.push(step.clone());
        self.feature_extractor.add_step(step);
        self.status = PipelineStatus::Configured;
    }

    /// 피처 선택
    pub fn select_features(&mut self, properties: Vec<String>) {
        self.feature_properties = properties.clone();
        self.feature_extractor.select_features(properties);
    }

    /// 모델 후보 추가
    pub fn add_model_candidate(&mut self, candidate: ModelCandidate) {
        self.model_candidates.push(candidate);
    }

    /// 로지스틱 회귀 추가
    pub fn add_logistic_regression(&mut self, config: HashMap<String, Value>) {
        let penalty = config
            .get("penalty")
            .and_then(|v| match v {
                Value::Float(f) => Some(*f),
                Value::Integer(i) => Some(*i as f64),
                _ => None,
            })
            .unwrap_or(0.01);

        let max_iterations = config
            .get("maxIterations")
            .and_then(|v| match v {
                Value::Integer(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(100);

        self.add_model_candidate(ModelCandidate::logistic_with_config(
            penalty,
            max_iterations,
        ));
    }

    /// 랜덤 포레스트 추가
    pub fn add_random_forest(&mut self, config: HashMap<String, Value>) {
        let n_estimators = config
            .get("numberOfDecisionTrees")
            .or_else(|| config.get("nEstimators"))
            .and_then(|v| match v {
                Value::Integer(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(100);

        let max_depth = config.get("maxDepth").and_then(|v| match v {
            Value::Integer(i) => Some(*i as usize),
            _ => None,
        });

        self.add_model_candidate(ModelCandidate::random_forest_with_config(
            n_estimators,
            max_depth,
        ));
    }

    /// 타겟 속성 설정
    pub fn set_target_property(&mut self, property: &str) {
        self.config.target_property = property.to_string();
    }

    /// 훈련
    ///
    /// # Arguments
    /// * `node_ids` - 훈련에 사용할 노드 ID 목록
    /// * `node_features` - 각 노드의 피처 맵 (property -> value)
    /// * `node_labels` - 각 노드의 레이블 맵 (node_id -> label)
    ///
    /// # Returns
    /// 훈련 결과 (모델 정보, 모델 선택 통계)
    pub fn train(
        &mut self,
        node_ids: &[u64],
        node_features: &HashMap<u64, HashMap<String, f64>>,
        node_labels: &HashMap<u64, i64>,
    ) -> Result<(TrainedModel, ModelSelectionStats), String> {
        let start_time = Instant::now();
        self.status = PipelineStatus::Training;

        // 피처 벡터 및 레이블 준비
        let mut features = Vec::new();
        let mut labels = Vec::new();
        let mut valid_node_ids = Vec::new();

        for &node_id in node_ids {
            if let (Some(feat_map), Some(&label)) =
                (node_features.get(&node_id), node_labels.get(&node_id))
            {
                let feature_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|prop| feat_map.get(prop).copied().unwrap_or(0.0))
                    .collect();
                features.push(feature_vec);
                labels.push(label);
                valid_node_ids.push(node_id);
            }
        }

        if features.is_empty() {
            self.status = PipelineStatus::Error;
            return Err("No valid training data".to_string());
        }

        // 피처 스케일링
        let mut scaler = FeatureScaler::new(crate::ml::scaler::ScalerConfig {
            scaler_type: self.config.scaler_type,
            ..Default::default()
        });
        let scaled_features = scaler.fit_transform(&features);
        self.scaler = Some(scaler);

        // Train/Test 분할
        let split_config = TrainTestSplitConfig {
            test_fraction: self.config.pipeline.test_fraction,
            validation_fraction: self.config.pipeline.validation_fraction,
            stratify: true,
            random_seed: self.config.pipeline.random_seed,
        };

        let indices: Vec<usize> = (0..scaled_features.len()).collect();
        let split = TrainTestSplit::split(&indices, Some(&labels), &split_config);

        let train_features: Vec<Vec<f64>> = split
            .train
            .iter()
            .map(|&i| scaled_features[i].clone())
            .collect();
        let train_labels: Vec<i64> = split.train.iter().map(|&i| labels[i]).collect();
        let test_features: Vec<Vec<f64>> = split
            .test
            .iter()
            .map(|&i| scaled_features[i].clone())
            .collect();
        let test_labels: Vec<i64> = split.test.iter().map(|&i| labels[i]).collect();

        // 모델 후보가 없으면 기본 모델 추가
        if self.model_candidates.is_empty() {
            self.model_candidates.push(ModelCandidate::logistic());
        }

        // 교차 검증으로 최적 모델 선택
        let cv_config = CrossValidatorConfig {
            n_folds: self.config.pipeline.n_folds,
            shuffle: true,
            stratified: true,
            random_seed: self.config.pipeline.random_seed,
            scoring: self
                .config
                .pipeline
                .metrics
                .first()
                .copied()
                .unwrap_or(ClassificationMetric::F1Weighted),
        };
        let cv = CrossValidator::new(cv_config.clone());

        let mut model_results: Vec<(String, CVResult)> = Vec::new();
        let mut best_idx = 0;
        let mut best_score = f64::NEG_INFINITY;

        for (idx, candidate) in self.model_candidates.iter().enumerate() {
            let result = match candidate {
                ModelCandidate::LogisticRegression(config) => {
                    let model = crate::ml::models::LogisticRegression::new(config.clone());
                    cv.cross_validate(&model, &train_features, &train_labels)
                }
                ModelCandidate::RandomForest(config) => {
                    let model = crate::ml::models::RandomForest::new(config.clone());
                    cv.cross_validate(&model, &train_features, &train_labels)
                }
            };

            if result.mean_score > best_score {
                best_score = result.mean_score;
                best_idx = idx;
            }

            model_results.push((candidate.name().to_string(), result));
        }

        // 최적 모델 학습
        let best_candidate = &self.model_candidates[best_idx];
        let mut best_model = best_candidate.create_model();
        best_model.fit(&train_features, &train_labels);

        // 테스트 세트 평가
        let test_predictions = best_model.predict(&test_features);
        let test_report = ClassificationReport::from_predictions(&test_labels, &test_predictions);
        let test_score = test_report.get_metric(cv_config.scoring);

        // 결과 저장
        let training_time_ms = start_time.elapsed().as_millis() as u64;

        let trained_model = TrainedModel {
            pipeline_name: self.name.clone(),
            model_name: format!("{}_{}", self.name, best_model.name()),
            model_type: best_model.model_type(),
            best_params: best_candidate.to_params(),
            cv_score: best_score,
            test_score,
            metric: cv_config.scoring,
            training_time_ms,
            model_data: best_model.to_bytes(),
            feature_properties: self.feature_properties.clone(),
        };

        let stats = ModelSelectionStats {
            model_results,
            selected_index: best_idx,
            selection_reason: format!(
                "Highest {} score: {:.4}",
                cv_config.scoring.name(),
                best_score
            ),
        };

        self.trained_model = Some(trained_model.clone());
        self.model_selection_stats = Some(stats.clone());
        self.status = PipelineStatus::Trained;

        Ok((trained_model, stats))
    }

    /// 예측 (스트림)
    pub fn predict_stream(
        &self,
        node_ids: &[u64],
        node_features: &HashMap<u64, HashMap<String, f64>>,
    ) -> Result<Vec<(u64, i64, Vec<f64>)>, String> {
        let model = self
            .trained_model
            .as_ref()
            .and_then(|tm| tm.load_model())
            .ok_or("Model not trained")?;

        let scaler = self.scaler.as_ref().ok_or("Scaler not initialized")?;

        let mut results = Vec::new();

        for &node_id in node_ids {
            if let Some(feat_map) = node_features.get(&node_id) {
                let feature_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|prop| feat_map.get(prop).copied().unwrap_or(0.0))
                    .collect();

                let scaled = scaler.transform(&[feature_vec]);
                let predictions = model.predict(&scaled);
                let probabilities = model.predict_proba(&scaled);

                if let (Some(&pred), Some(probs)) = (predictions.first(), probabilities.first()) {
                    results.push((node_id, pred, probs.clone()));
                }
            }
        }

        Ok(results)
    }

    /// 노드 속성 스텝 조회
    pub fn node_property_steps(&self) -> &[NodePropertyStep] {
        &self.node_property_steps
    }

    /// 모델 선택 통계 조회
    pub fn model_selection_stats(&self) -> Option<&ModelSelectionStats> {
        self.model_selection_stats.as_ref()
    }
}

impl Pipeline for NodeClassificationPipeline {
    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> PipelineStatus {
        self.status
    }

    fn feature_properties(&self) -> &[String] {
        &self.feature_properties
    }

    fn model_candidates(&self) -> &[ModelCandidate] {
        &self.model_candidates
    }

    fn trained_model(&self) -> Option<&TrainedModel> {
        self.trained_model.as_ref()
    }
}

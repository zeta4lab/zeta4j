//! Link Prediction Pipeline
//!
//! 링크 예측을 위한 ML 파이프라인

use super::base::{
    ModelCandidate, ModelSelectionStats, Pipeline, PipelineConfig, PipelineStatus, TrainedModel,
};
use crate::core::Value;
use crate::ml::cross_validation::{CVResult, CrossValidator, CrossValidatorConfig};
use crate::ml::feature::{FeatureExtractor, LinkFeature, LinkFeatureCombiner, NodePropertyStep};
use crate::ml::metrics::{ClassificationMetric, ClassificationReport};
use crate::ml::scaler::{FeatureScaler, ScalerType};
use crate::ml::split::{TrainTestSplit, TrainTestSplitConfig};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Link Split 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSplitConfig {
    /// 테스트 비율
    pub test_fraction: f64,
    /// 훈련 비율
    pub train_fraction: f64,
    /// 네거티브 샘플링 비율 (양성 엣지 대비)
    pub negative_sampling_ratio: f64,
    /// 랜덤 시드
    pub random_seed: Option<u64>,
}

impl Default for LinkSplitConfig {
    fn default() -> Self {
        Self {
            test_fraction: 0.2,
            train_fraction: 0.8,
            negative_sampling_ratio: 1.0,
            random_seed: None,
        }
    }
}

/// Link Prediction 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkPredictionConfig {
    /// 기본 파이프라인 설정
    pub pipeline: PipelineConfig,
    /// 링크 분할 설정
    pub link_split: LinkSplitConfig,
    /// 타겟 관계 유형
    pub target_relationship_type: Option<String>,
    /// 피처 스케일링 타입
    pub scaler_type: ScalerType,
}

impl Default for LinkPredictionConfig {
    fn default() -> Self {
        Self {
            pipeline: PipelineConfig::default(),
            link_split: LinkSplitConfig::default(),
            target_relationship_type: None,
            scaler_type: ScalerType::Standard,
        }
    }
}

/// 링크 피처 스텝
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkFeatureStep {
    /// 피처 타입
    pub feature_type: LinkFeature,
    /// 사용할 노드 속성
    pub node_properties: Vec<String>,
}

/// Link Prediction Pipeline
#[derive(Debug, Clone)]
pub struct LinkPredictionPipeline {
    /// 파이프라인 이름
    name: String,
    /// 상태
    status: PipelineStatus,
    /// 설정
    config: LinkPredictionConfig,
    /// 노드 속성 스텝
    node_property_steps: Vec<NodePropertyStep>,
    /// 링크 피처 스텝
    link_feature_steps: Vec<LinkFeatureStep>,
    /// 피처 속성 (노드 임베딩용)
    feature_properties: Vec<String>,
    /// 모델 후보
    model_candidates: Vec<ModelCandidate>,
    /// 피처 추출기
    feature_extractor: FeatureExtractor,
    /// 링크 피처 결합기
    link_combiner: LinkFeatureCombiner,
    /// 피처 스케일러
    scaler: Option<FeatureScaler>,
    /// 훈련된 모델
    trained_model: Option<TrainedModel>,
    /// 모델 선택 통계
    model_selection_stats: Option<ModelSelectionStats>,
}

impl LinkPredictionPipeline {
    /// 새 파이프라인 생성
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: PipelineStatus::Created,
            config: LinkPredictionConfig::default(),
            node_property_steps: Vec::new(),
            link_feature_steps: Vec::new(),
            feature_properties: Vec::new(),
            model_candidates: Vec::new(),
            feature_extractor: FeatureExtractor::new(),
            link_combiner: LinkFeatureCombiner::default(),
            scaler: None,
            trained_model: None,
            model_selection_stats: None,
        }
    }

    /// 설정 지정
    pub fn with_config(mut self, config: LinkPredictionConfig) -> Self {
        self.config = config;
        self
    }

    /// 노드 속성 스텝 추가
    pub fn add_node_property(&mut self, step: NodePropertyStep) {
        self.node_property_steps.push(step.clone());
        self.feature_extractor.add_step(step);
        self.status = PipelineStatus::Configured;
    }

    /// 링크 피처 추가
    pub fn add_link_feature(&mut self, feature_type: &str, node_properties: Vec<String>) {
        if let Some(ft) = LinkFeature::parse(feature_type) {
            self.link_feature_steps.push(LinkFeatureStep {
                feature_type: ft,
                node_properties: node_properties.clone(),
            });

            // 링크 결합기 업데이트
            let features: Vec<LinkFeature> = self
                .link_feature_steps
                .iter()
                .map(|s| s.feature_type)
                .collect();
            self.link_combiner = LinkFeatureCombiner::new(features);

            // 피처 속성 업데이트
            for prop in node_properties {
                if !self.feature_properties.contains(&prop) {
                    self.feature_properties.push(prop);
                }
            }
        }
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

    /// 타겟 관계 유형 설정
    pub fn set_target_relationship_type(&mut self, rel_type: &str) {
        self.config.target_relationship_type = Some(rel_type.to_string());
    }

    /// 네거티브 샘플 생성
    fn generate_negative_samples(
        &self,
        positive_edges: &[(u64, u64)],
        all_nodes: &[u64],
        n_samples: usize,
        seed: Option<u64>,
    ) -> Vec<(u64, u64)> {
        let positive_set: HashSet<(u64, u64)> = positive_edges.iter().copied().collect();
        let mut rng = match seed {
            Some(s) => ChaCha8Rng::seed_from_u64(s),
            None => ChaCha8Rng::from_os_rng(),
        };

        let mut negatives = Vec::new();
        let max_attempts = n_samples * 10;
        let mut attempts = 0;

        while negatives.len() < n_samples && attempts < max_attempts {
            let u = all_nodes[rng.random_range(0..all_nodes.len())];
            let v = all_nodes[rng.random_range(0..all_nodes.len())];

            if u != v && !positive_set.contains(&(u, v)) && !positive_set.contains(&(v, u)) {
                negatives.push((u, v));
            }
            attempts += 1;
        }

        negatives
    }

    /// 훈련
    ///
    /// # Arguments
    /// * `positive_edges` - 양성 엣지 목록 (존재하는 링크)
    /// * `node_features` - 각 노드의 피처 맵
    /// * `all_nodes` - 전체 노드 목록 (네거티브 샘플링용)
    pub fn train(
        &mut self,
        positive_edges: &[(u64, u64)],
        node_features: &HashMap<u64, HashMap<String, f64>>,
        all_nodes: &[u64],
    ) -> Result<(TrainedModel, ModelSelectionStats), String> {
        let start_time = Instant::now();
        self.status = PipelineStatus::Training;

        // 네거티브 샘플 생성
        let n_neg =
            (positive_edges.len() as f64 * self.config.link_split.negative_sampling_ratio) as usize;
        let negative_edges = self.generate_negative_samples(
            positive_edges,
            all_nodes,
            n_neg,
            self.config.pipeline.random_seed,
        );

        // 피처 및 레이블 준비
        let mut features = Vec::new();
        let mut labels = Vec::new();

        // 양성 샘플
        for &(u, v) in positive_edges {
            if let (Some(u_feat), Some(v_feat)) = (node_features.get(&u), node_features.get(&v)) {
                let u_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| u_feat.get(p).copied().unwrap_or(0.0))
                    .collect();
                let v_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| v_feat.get(p).copied().unwrap_or(0.0))
                    .collect();

                let combined = self.link_combiner.combine(&u_vec, &v_vec);
                features.push(combined);
                labels.push(1_i64);
            }
        }

        // 음성 샘플
        for &(u, v) in &negative_edges {
            if let (Some(u_feat), Some(v_feat)) = (node_features.get(&u), node_features.get(&v)) {
                let u_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| u_feat.get(p).copied().unwrap_or(0.0))
                    .collect();
                let v_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| v_feat.get(p).copied().unwrap_or(0.0))
                    .collect();

                let combined = self.link_combiner.combine(&u_vec, &v_vec);
                features.push(combined);
                labels.push(0_i64);
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
            test_fraction: self.config.link_split.test_fraction,
            validation_fraction: 0.0,
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
        let scoring = ClassificationMetric::AucRoc;
        let cv_config = CrossValidatorConfig {
            n_folds: self.config.pipeline.n_folds,
            shuffle: true,
            stratified: true,
            random_seed: self.config.pipeline.random_seed,
            scoring,
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
        let test_score = test_report.get_metric(scoring);

        // 결과 저장
        let training_time_ms = start_time.elapsed().as_millis() as u64;

        let trained_model = TrainedModel {
            pipeline_name: self.name.clone(),
            model_name: format!("{}_{}", self.name, best_model.name()),
            model_type: best_model.model_type(),
            best_params: HashMap::new(),
            cv_score: best_score,
            test_score,
            metric: scoring,
            training_time_ms,
            model_data: best_model.to_bytes(),
            feature_properties: self.feature_properties.clone(),
        };

        let stats = ModelSelectionStats {
            model_results,
            selected_index: best_idx,
            selection_reason: format!("Highest {} score: {:.4}", scoring.name(), best_score),
        };

        self.trained_model = Some(trained_model.clone());
        self.model_selection_stats = Some(stats.clone());
        self.status = PipelineStatus::Trained;

        Ok((trained_model, stats))
    }

    /// 예측 (스트림)
    pub fn predict_stream(
        &self,
        node_pairs: &[(u64, u64)],
        node_features: &HashMap<u64, HashMap<String, f64>>,
    ) -> Result<Vec<(u64, u64, f64)>, String> {
        let model = self
            .trained_model
            .as_ref()
            .and_then(|tm| tm.load_model())
            .ok_or("Model not trained")?;

        let scaler = self.scaler.as_ref().ok_or("Scaler not initialized")?;

        let mut results = Vec::new();

        for &(u, v) in node_pairs {
            if let (Some(u_feat), Some(v_feat)) = (node_features.get(&u), node_features.get(&v)) {
                let u_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| u_feat.get(p).copied().unwrap_or(0.0))
                    .collect();
                let v_vec: Vec<f64> = self
                    .feature_properties
                    .iter()
                    .map(|p| v_feat.get(p).copied().unwrap_or(0.0))
                    .collect();

                let combined = self.link_combiner.combine(&u_vec, &v_vec);
                let scaled = scaler.transform(&[combined]);
                let probabilities = model.predict_proba(&scaled);

                if let Some(probs) = probabilities.first() {
                    // 양성 클래스 (링크 존재) 확률
                    let link_prob = probs.get(1).copied().unwrap_or(0.5);
                    results.push((u, v, link_prob));
                }
            }
        }

        Ok(results)
    }

    /// 노드 속성 스텝 조회
    pub fn node_property_steps(&self) -> &[NodePropertyStep] {
        &self.node_property_steps
    }

    /// 링크 피처 스텝 조회
    pub fn link_feature_steps(&self) -> &[LinkFeatureStep] {
        &self.link_feature_steps
    }

    /// 모델 선택 통계 조회
    pub fn model_selection_stats(&self) -> Option<&ModelSelectionStats> {
        self.model_selection_stats.as_ref()
    }
}

impl Pipeline for LinkPredictionPipeline {
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

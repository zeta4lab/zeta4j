//! Feature Extractor
//!
//! 그래프 알고리즘을 통한 노드 피처 추출

use crate::core::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 노드 속성 스텝 (그래프 알고리즘 실행)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePropertyStep {
    /// 알고리즘 이름 (pageRank, degree, etc.)
    pub algorithm: String,
    /// 알고리즘 설정
    pub config: HashMap<String, Value>,
    /// 출력 속성 이름
    pub output_property: String,
    /// 컨텍스트 노드 속성 (optional)
    pub context_node_properties: Vec<String>,
}

impl NodePropertyStep {
    /// 새 스텝 생성
    pub fn new(algorithm: &str, output_property: &str) -> Self {
        Self {
            algorithm: algorithm.to_string(),
            config: HashMap::new(),
            output_property: output_property.to_string(),
            context_node_properties: Vec::new(),
        }
    }

    /// 설정 추가
    pub fn with_config(mut self, key: &str, value: Value) -> Self {
        self.config.insert(key.to_string(), value);
        self
    }

    /// 컨텍스트 속성 추가
    pub fn with_context_property(mut self, property: &str) -> Self {
        self.context_node_properties.push(property.to_string());
        self
    }
}

/// Feature Extractor
///
/// 그래프 알고리즘 실행 결과를 노드 피처로 변환
#[derive(Debug, Clone)]
pub struct FeatureExtractor {
    /// 노드 속성 스텝 목록
    steps: Vec<NodePropertyStep>,
    /// 선택된 피처 속성들
    feature_properties: Vec<String>,
    /// 추출된 피처 (node_id -> property_name -> value)
    extracted_features: HashMap<u64, HashMap<String, f64>>,
}

impl FeatureExtractor {
    /// 새 추출기 생성
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            feature_properties: Vec::new(),
            extracted_features: HashMap::new(),
        }
    }

    /// 노드 속성 스텝 추가
    pub fn add_step(&mut self, step: NodePropertyStep) {
        self.steps.push(step);
    }

    /// 피처 속성 선택
    pub fn select_features(&mut self, properties: Vec<String>) {
        self.feature_properties = properties;
    }

    /// 스텝 목록 조회
    pub fn steps(&self) -> &[NodePropertyStep] {
        &self.steps
    }

    /// 선택된 피처 속성 조회
    pub fn feature_properties(&self) -> &[String] {
        &self.feature_properties
    }

    /// 피처 저장
    pub fn store_feature(&mut self, node_id: u64, property: &str, value: f64) {
        self.extracted_features
            .entry(node_id)
            .or_default()
            .insert(property.to_string(), value);
    }

    /// 노드별 피처 벡터 생성
    ///
    /// # Arguments
    /// * `node_ids` - 피처를 추출할 노드 ID 목록
    ///
    /// # Returns
    /// 각 노드의 피처 벡터
    pub fn get_feature_vectors(&self, node_ids: &[u64]) -> Vec<Vec<f64>> {
        node_ids
            .iter()
            .map(|&node_id| {
                self.feature_properties
                    .iter()
                    .map(|prop| {
                        self.extracted_features
                            .get(&node_id)
                            .and_then(|props| props.get(prop))
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .collect()
            })
            .collect()
    }

    /// 단일 노드 피처 벡터
    pub fn get_feature_vector(&self, node_id: u64) -> Vec<f64> {
        self.feature_properties
            .iter()
            .map(|prop| {
                self.extracted_features
                    .get(&node_id)
                    .and_then(|props| props.get(prop))
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect()
    }

    /// 피처 차원
    pub fn n_features(&self) -> usize {
        self.feature_properties.len()
    }

    /// 피처 통계
    pub fn feature_stats(&self) -> HashMap<String, FeatureStats> {
        let mut stats = HashMap::new();

        for property in &self.feature_properties {
            let values: Vec<f64> = self
                .extracted_features
                .values()
                .filter_map(|props| props.get(property))
                .copied()
                .collect();

            if values.is_empty() {
                continue;
            }

            let n = values.len() as f64;
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let sum: f64 = values.iter().sum();
            let mean = sum / n;
            let variance: f64 = values.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / n;

            stats.insert(
                property.clone(),
                FeatureStats {
                    count: values.len(),
                    min,
                    max,
                    mean,
                    std: variance.sqrt(),
                },
            );
        }

        stats
    }

    /// 모든 추출된 피처 초기화
    pub fn clear(&mut self) {
        self.extracted_features.clear();
    }
}

impl Default for FeatureExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// 피처 통계
#[derive(Debug, Clone)]
pub struct FeatureStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std: f64,
}

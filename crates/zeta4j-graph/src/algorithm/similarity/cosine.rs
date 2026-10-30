//! Cosine Similarity 알고리즘
//!
//! 두 노드 간 코사인 유사도를 계산합니다.
//!
//! Cosine(A, B) = (A · B) / (||A|| × ||B||)
//!
//! 두 가지 방식 지원:
//! 1. 이웃 기반: 이웃 노드를 이진 벡터로 표현
//! 2. 속성 기반: 노드 속성값을 벡터로 사용
//!
//! - 범위: 0.0 (직교) ~ 1.0 (동일 방향)
//! - 추천 시스템, 문서 유사도 등에 활용

use crate::core::{NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;

use super::{SimilarityResult, get_all_node_ids, get_neighbors};

/// Cosine Similarity 설정
#[derive(Debug, Clone)]
pub struct CosineConfig {
    /// 최소 유사도 임계값 (0.0 ~ 1.0)
    pub threshold: f64,
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부 (false면 양방향)
    pub directed: bool,
    /// 속성 기반 유사도에 사용할 속성 목록
    pub properties: Option<Vec<String>>,
}

impl Default for CosineConfig {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            relationship_types: None,
            directed: false,
            properties: None,
        }
    }
}

impl CosineConfig {
    /// 임계값 설정
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }

    /// 속성 목록 설정 (속성 기반 유사도용)
    pub fn with_properties(mut self, properties: Vec<String>) -> Self {
        self.properties = Some(properties);
        self
    }
}

/// Cosine Similarity 계산기
pub struct CosineSimilarity<'a> {
    engine: &'a GraphEngine,
    config: CosineConfig,
}

impl<'a> CosineSimilarity<'a> {
    /// 새로운 Cosine Similarity 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: CosineConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: CosineConfig) -> Self {
        Self { engine, config }
    }

    /// 두 노드 간 이웃 기반 Cosine 유사도 계산
    ///
    /// 이웃 노드를 이진 벡터로 표현하여 코사인 유사도 계산
    /// Cosine = |N(A) ∩ N(B)| / (sqrt(|N(A)|) × sqrt(|N(B)|))
    ///
    /// # Arguments
    /// * `node1` - 첫 번째 노드 ID
    /// * `node2` - 두 번째 노드 ID
    ///
    /// # Returns
    /// Cosine 유사도 (0.0 ~ 1.0)
    pub async fn similarity_neighbors(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        let neighbors1 = get_neighbors(
            self.engine,
            node1,
            &self.config.relationship_types,
            self.config.directed,
        )
        .await?;
        let neighbors2 = get_neighbors(
            self.engine,
            node2,
            &self.config.relationship_types,
            self.config.directed,
        )
        .await?;

        // 둘 중 하나라도 이웃이 없으면 0.0
        if neighbors1.is_empty() || neighbors2.is_empty() {
            return Ok(0.0);
        }

        let intersection = neighbors1.intersection(&neighbors2).count() as f64;
        let norm1 = (neighbors1.len() as f64).sqrt();
        let norm2 = (neighbors2.len() as f64).sqrt();

        let denominator = norm1 * norm2;

        if denominator == 0.0 {
            Ok(0.0)
        } else {
            Ok(intersection / denominator)
        }
    }

    /// 두 노드 간 속성 기반 Cosine 유사도 계산
    ///
    /// 지정된 속성값을 벡터로 사용하여 코사인 유사도 계산
    ///
    /// # Arguments
    /// * `node1` - 첫 번째 노드 ID
    /// * `node2` - 두 번째 노드 ID
    /// * `properties` - 사용할 속성 목록
    ///
    /// # Returns
    /// Cosine 유사도 (0.0 ~ 1.0)
    pub async fn similarity_properties(
        &self,
        node1: NodeId,
        node2: NodeId,
        properties: &[String],
    ) -> Result<f64> {
        let node1_data = self.engine.get_node(node1).await?;
        let node2_data = self.engine.get_node(node2).await?;

        let (props1, props2) = match (node1_data, node2_data) {
            (Some(n1), Some(n2)) => (n1.properties().clone(), n2.properties().clone()),
            _ => return Ok(0.0),
        };

        let mut dot_product = 0.0;
        let mut norm1 = 0.0;
        let mut norm2 = 0.0;

        for prop in properties {
            let v1 = Self::value_to_float(props1.get(prop));
            let v2 = Self::value_to_float(props2.get(prop));

            dot_product += v1 * v2;
            norm1 += v1 * v1;
            norm2 += v2 * v2;
        }

        let denominator = norm1.sqrt() * norm2.sqrt();

        if denominator == 0.0 {
            Ok(0.0)
        } else {
            Ok(dot_product / denominator)
        }
    }

    /// 두 노드 간 Cosine 유사도 계산 (설정에 따라 이웃/속성 기반 선택)
    pub async fn similarity(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        if let Some(ref properties) = self.config.properties {
            self.similarity_properties(node1, node2, properties).await
        } else {
            self.similarity_neighbors(node1, node2).await
        }
    }

    /// 전체 노드 쌍의 Cosine 유사도 계산 (임계값 이상)
    pub async fn calculate_all(&self) -> Result<Vec<SimilarityResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let similarity = self.similarity(nodes[i], nodes[j]).await?;

                if similarity >= self.config.threshold {
                    results.push(SimilarityResult::new(nodes[i], nodes[j], similarity));
                }
            }
        }

        // 유사도 내림차순 정렬
        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        Ok(results)
    }

    /// 특정 노드와 가장 유사한 top-K 노드 찾기
    pub async fn top_k_similar(
        &self,
        target_node: NodeId,
        top_k: usize,
    ) -> Result<Vec<SimilarityResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        for &node in &nodes {
            if node == target_node {
                continue;
            }

            let similarity = self.similarity(target_node, node).await?;

            if similarity >= self.config.threshold {
                results.push(SimilarityResult::new(target_node, node, similarity));
            }
        }

        // 유사도 내림차순 정렬
        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        // top-K만 반환
        results.truncate(top_k);

        Ok(results)
    }

    /// Value를 f64로 변환
    fn value_to_float(value: Option<&Value>) -> f64 {
        match value {
            Some(Value::Float(f)) => *f,
            Some(Value::Integer(i)) => *i as f64,
            _ => 0.0,
        }
    }
}

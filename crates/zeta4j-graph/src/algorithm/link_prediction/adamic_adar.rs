//! Adamic-Adar Index 알고리즘
//!
//! 공통 이웃의 degree 역로그로 가중치를 부여합니다.
//!
//! AA(A, B) = Σ_{z ∈ N(A) ∩ N(B)} 1 / log(|N(z)|)
//!
//! - degree가 낮은 공통 이웃에 더 높은 가중치
//! - 희귀한 연결이 더 의미있다는 직관
//! - 범위: 0 ~ ∞

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use super::{LinkPredictionResult, are_connected, get_all_node_ids, get_neighbors};

/// Adamic-Adar 설정
#[derive(Debug, Clone, Default)]
pub struct AdamicAdarConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부
    pub directed: bool,
}

impl AdamicAdarConfig {
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
}

/// Min-Heap용 래퍼
#[derive(Debug, Clone)]
struct PredictionEntry {
    target: NodeId,
    score: f64,
}

impl PartialEq for PredictionEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.target == other.target
    }
}

impl Eq for PredictionEntry {}

impl PartialOrd for PredictionEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PredictionEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

/// Adamic-Adar 계산기
pub struct AdamicAdar<'a> {
    engine: &'a GraphEngine,
    config: AdamicAdarConfig,
}

impl<'a> AdamicAdar<'a> {
    /// 새로운 Adamic-Adar 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: AdamicAdarConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: AdamicAdarConfig) -> Self {
        Self { engine, config }
    }

    /// 두 노드 간 Adamic-Adar 점수 계산
    ///
    /// # Arguments
    /// * `node1` - 첫 번째 노드 ID
    /// * `node2` - 두 번째 노드 ID
    ///
    /// # Returns
    /// Adamic-Adar 점수 (0.0 ~ ∞)
    pub async fn score(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
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

        // 공통 이웃 찾기
        let common: HashSet<_> = neighbors1.intersection(&neighbors2).cloned().collect();

        let mut score = 0.0;
        for z in common {
            let z_neighbors = get_neighbors(
                self.engine,
                z,
                &self.config.relationship_types,
                self.config.directed,
            )
            .await?;
            let degree = z_neighbors.len();

            // degree가 2 이상일 때만 유효 (log(1) = 0이므로)
            if degree > 1 {
                score += 1.0 / (degree as f64).ln();
            }
        }

        Ok(score)
    }

    /// 특정 노드의 상위 K개 링크 예측 후보
    pub async fn predict(&self, source: NodeId, top_k: usize) -> Result<Vec<LinkPredictionResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut heap: BinaryHeap<PredictionEntry> = BinaryHeap::new();

        for &target in &nodes {
            if target == source {
                continue;
            }

            if are_connected(self.engine, source, target, &self.config.relationship_types).await? {
                continue;
            }

            let score = self.score(source, target).await?;

            if score > 0.0 {
                heap.push(PredictionEntry { target, score });

                if heap.len() > top_k {
                    heap.pop();
                }
            }
        }

        let mut results: Vec<LinkPredictionResult> = heap
            .into_iter()
            .map(|e| LinkPredictionResult::new(source, e.target, e.score))
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        Ok(results)
    }

    /// 전체 노드 쌍의 Adamic-Adar 점수 계산 (임계값 이상)
    pub async fn calculate_all(&self, threshold: f64) -> Result<Vec<LinkPredictionResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let node1 = nodes[i];
                let node2 = nodes[j];

                if are_connected(self.engine, node1, node2, &self.config.relationship_types).await?
                {
                    continue;
                }

                let score = self.score(node1, node2).await?;

                if score >= threshold {
                    results.push(LinkPredictionResult::new(node1, node2, score));
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        Ok(results)
    }
}

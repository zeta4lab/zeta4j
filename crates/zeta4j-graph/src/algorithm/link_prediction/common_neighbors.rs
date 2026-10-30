//! Common Neighbors 알고리즘
//!
//! 두 노드의 공통 이웃 수를 계산합니다.
//!
//! CN(A, B) = |N(A) ∩ N(B)|
//!
//! - 가장 단순하고 직관적인 링크 예측 방법
//! - 공통 친구가 많을수록 연결 가능성 높음
//! - 범위: 0 ~ min(|N(A)|, |N(B)|)

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::{LinkPredictionResult, are_connected, get_all_node_ids, get_neighbors};

/// Common Neighbors 설정
#[derive(Debug, Clone, Default)]
pub struct CommonNeighborsConfig {
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부
    pub directed: bool,
}

impl CommonNeighborsConfig {
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

/// Min-Heap용 래퍼 (top-K 유지용)
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
        // Min-heap: 작은 값이 루트에 오도록
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

/// Common Neighbors 계산기
pub struct CommonNeighbors<'a> {
    engine: &'a GraphEngine,
    config: CommonNeighborsConfig,
}

impl<'a> CommonNeighbors<'a> {
    /// 새로운 Common Neighbors 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: CommonNeighborsConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: CommonNeighborsConfig) -> Self {
        Self { engine, config }
    }

    /// 두 노드 간 Common Neighbors 점수 계산
    ///
    /// # Arguments
    /// * `node1` - 첫 번째 노드 ID
    /// * `node2` - 두 번째 노드 ID
    ///
    /// # Returns
    /// 공통 이웃 수 (정수지만 일관성을 위해 f64로 반환)
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

        let common = neighbors1.intersection(&neighbors2).count();
        Ok(common as f64)
    }

    /// 특정 노드의 상위 K개 링크 예측 후보
    ///
    /// 이미 연결된 노드는 제외됩니다.
    ///
    /// # Arguments
    /// * `source` - 소스 노드 ID
    /// * `top_k` - 반환할 최대 후보 수
    ///
    /// # Returns
    /// 점수 내림차순으로 정렬된 링크 예측 결과
    pub async fn predict(&self, source: NodeId, top_k: usize) -> Result<Vec<LinkPredictionResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut heap: BinaryHeap<PredictionEntry> = BinaryHeap::new();

        for &target in &nodes {
            // 자기 자신 제외
            if target == source {
                continue;
            }

            // 이미 연결된 노드 제외
            if are_connected(self.engine, source, target, &self.config.relationship_types).await? {
                continue;
            }

            let score = self.score(source, target).await?;

            // 점수가 0보다 큰 경우만 추가
            if score > 0.0 {
                heap.push(PredictionEntry { target, score });

                if heap.len() > top_k {
                    heap.pop();
                }
            }
        }

        // 결과 정렬 (점수 내림차순)
        let mut results: Vec<LinkPredictionResult> = heap
            .into_iter()
            .map(|e| LinkPredictionResult::new(source, e.target, e.score))
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        Ok(results)
    }

    /// 전체 노드 쌍의 Common Neighbors 점수 계산 (임계값 이상)
    pub async fn calculate_all(&self, threshold: f64) -> Result<Vec<LinkPredictionResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut results = Vec::new();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let node1 = nodes[i];
                let node2 = nodes[j];

                // 이미 연결된 노드 제외
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

        // 점수 내림차순 정렬
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        Ok(results)
    }
}

// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Node Similarity 알고리즘
//!
//! 전체 노드에 대해 유사도를 계산하고 각 노드별 top-K 유사 노드를 찾습니다.
//!
//! - 다양한 유사도 메트릭 지원 (Jaccard, Overlap, Cosine)
//! - 효율적인 top-K 선택 (Min-Heap 기반)
//! - 대규모 그래프에서도 메모리 효율적

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use super::cosine::CosineSimilarity;
use super::jaccard::JaccardSimilarity;
use super::overlap::OverlapCoefficient;
use super::{SimilarityResult, get_all_node_ids};

/// 유사도 메트릭 종류
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SimilarityMetric {
    /// Jaccard Similarity
    #[default]
    Jaccard,
    /// Overlap Coefficient
    Overlap,
    /// Cosine Similarity (이웃 기반)
    Cosine,
}

/// Node Similarity 설정
#[derive(Debug, Clone)]
pub struct NodeSimilarityConfig {
    /// 각 노드당 반환할 최대 유사 노드 수
    pub top_k: usize,
    /// 사용할 유사도 메트릭
    pub metric: SimilarityMetric,
    /// 최소 유사도 임계값
    pub threshold: f64,
    /// 고려할 관계 타입 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부
    pub directed: bool,
}

impl Default for NodeSimilarityConfig {
    fn default() -> Self {
        Self {
            top_k: 10,
            metric: SimilarityMetric::Jaccard,
            threshold: 0.0,
            relationship_types: None,
            directed: false,
        }
    }
}

impl NodeSimilarityConfig {
    /// top-K 설정
    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k.max(1);
        self
    }

    /// 메트릭 설정
    pub fn with_metric(mut self, metric: SimilarityMetric) -> Self {
        self.metric = metric;
        self
    }

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
}

/// Min-Heap용 래퍼 (top-K 유지용)
#[derive(Debug, Clone)]
struct SimilarityEntry {
    node: NodeId,
    similarity: f64,
}

impl PartialEq for SimilarityEntry {
    fn eq(&self, other: &Self) -> bool {
        self.similarity == other.similarity && self.node == other.node
    }
}

impl Eq for SimilarityEntry {}

impl PartialOrd for SimilarityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SimilarityEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap을 위해 반대로 비교 (작은 값이 루트에 오도록)
        other
            .similarity
            .partial_cmp(&self.similarity)
            .unwrap_or(Ordering::Equal)
    }
}

/// Node Similarity 결과
#[derive(Debug, Clone)]
pub struct NodeSimilarityResult {
    /// 노드별 유사 노드 목록
    pub similarities: HashMap<NodeId, Vec<SimilarityResult>>,
    /// 전체 노드 쌍 수
    pub total_pairs: usize,
    /// 임계값을 넘은 쌍 수
    pub filtered_pairs: usize,
}

impl NodeSimilarityResult {
    /// 특정 노드의 유사 노드 목록 조회
    pub fn similar_to(&self, node_id: NodeId) -> Option<&Vec<SimilarityResult>> {
        self.similarities.get(&node_id)
    }

    /// 가장 유사한 전체 쌍 목록 (모든 노드 통합)
    pub fn all_pairs(&self) -> Vec<SimilarityResult> {
        let mut all: Vec<SimilarityResult> =
            self.similarities.values().flatten().cloned().collect();

        // 중복 제거 (node1, node2 순서가 다른 경우)
        let mut seen = std::collections::HashSet::new();
        all.retain(|r| {
            let key = if r.node1 < r.node2 {
                (r.node1, r.node2)
            } else {
                (r.node2, r.node1)
            };
            seen.insert(key)
        });

        // 유사도 내림차순 정렬
        all.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        all
    }
}

/// Node Similarity 계산기
pub struct NodeSimilarity<'a> {
    engine: &'a GraphEngine,
    config: NodeSimilarityConfig,
}

impl<'a> NodeSimilarity<'a> {
    /// 새로운 Node Similarity 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: NodeSimilarityConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: NodeSimilarityConfig) -> Self {
        Self { engine, config }
    }

    /// 두 노드 간 유사도 계산 (설정된 메트릭 사용)
    async fn calculate_similarity(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        match self.config.metric {
            SimilarityMetric::Jaccard => {
                let jaccard = JaccardSimilarity::new(self.engine);
                jaccard.similarity(node1, node2).await
            }
            SimilarityMetric::Overlap => {
                let overlap = OverlapCoefficient::new(self.engine);
                overlap.coefficient(node1, node2).await
            }
            SimilarityMetric::Cosine => {
                let cosine = CosineSimilarity::new(self.engine);
                cosine.similarity_neighbors(node1, node2).await
            }
        }
    }

    /// 전체 노드에 대해 각각 top-K 유사 노드 계산
    pub async fn calculate(&self) -> Result<NodeSimilarityResult> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut similarities: HashMap<NodeId, Vec<SimilarityResult>> = HashMap::new();
        let mut total_pairs = 0;
        let mut filtered_pairs = 0;

        for &node1 in &nodes {
            let mut heap: BinaryHeap<SimilarityEntry> = BinaryHeap::new();

            for &node2 in &nodes {
                if node1 == node2 {
                    continue;
                }

                total_pairs += 1;

                let similarity = self.calculate_similarity(node1, node2).await?;

                if similarity >= self.config.threshold {
                    filtered_pairs += 1;

                    heap.push(SimilarityEntry {
                        node: node2,
                        similarity,
                    });

                    // top-K 유지 (Min-heap이므로 가장 작은 값 제거)
                    if heap.len() > self.config.top_k {
                        heap.pop();
                    }
                }
            }

            // 결과 정렬 (유사도 내림차순)
            let mut node_results: Vec<SimilarityResult> = heap
                .into_iter()
                .map(|e| SimilarityResult::new(node1, e.node, e.similarity))
                .collect();

            node_results.sort_by(|a, b| {
                b.similarity
                    .partial_cmp(&a.similarity)
                    .unwrap_or(Ordering::Equal)
            });

            similarities.insert(node1, node_results);
        }

        // total_pairs는 양방향으로 세었으므로 절반으로 조정
        Ok(NodeSimilarityResult {
            similarities,
            total_pairs: total_pairs / 2,
            filtered_pairs: filtered_pairs / 2,
        })
    }

    /// 특정 노드에 대해서만 top-K 유사 노드 계산
    pub async fn calculate_for_node(&self, target_node: NodeId) -> Result<Vec<SimilarityResult>> {
        let nodes = get_all_node_ids(self.engine).await?;
        let mut heap: BinaryHeap<SimilarityEntry> = BinaryHeap::new();

        for &node in &nodes {
            if node == target_node {
                continue;
            }

            let similarity = self.calculate_similarity(target_node, node).await?;

            if similarity >= self.config.threshold {
                heap.push(SimilarityEntry { node, similarity });

                if heap.len() > self.config.top_k {
                    heap.pop();
                }
            }
        }

        let mut results: Vec<SimilarityResult> = heap
            .into_iter()
            .map(|e| SimilarityResult::new(target_node, e.node, e.similarity))
            .collect();

        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(Ordering::Equal)
        });

        Ok(results)
    }
}

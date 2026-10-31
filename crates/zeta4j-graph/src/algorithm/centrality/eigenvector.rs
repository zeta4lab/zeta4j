// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Eigenvector Centrality 알고리즘
//!
//! 노드의 중요도가 연결된 이웃 노드들의 중요도에 의해 결정되는 중심성 지표입니다.
//! 중요한 노드들과 연결된 노드가 더 높은 중심성을 가집니다.
//!
//! Power Iteration 방법을 사용하여 주요 고유벡터를 계산합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Eigenvector Centrality 설정
#[derive(Debug, Clone)]
pub struct EigenvectorConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 허용 오차
    pub tolerance: f64,
    /// 정규화 여부 (true면 최대값이 1.0이 되도록 정규화)
    pub normalized: bool,
    /// 방향성 고려 여부
    pub directed: bool,
}

impl Default for EigenvectorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1e-6,
            normalized: true,
            directed: false,
        }
    }
}

impl EigenvectorConfig {
    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 수렴 허용 오차 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// 정규화 설정
    pub fn with_normalized(mut self, normalized: bool) -> Self {
        self.normalized = normalized;
        self
    }

    /// 방향성 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// Eigenvector Centrality 결과
#[derive(Debug, Clone)]
pub struct EigenvectorResult {
    /// 노드별 Eigenvector Centrality 점수
    pub scores: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
}

impl EigenvectorResult {
    /// 특정 노드의 점수 조회
    pub fn score(&self, node_id: NodeId) -> Option<f64> {
        self.scores.get(&node_id).copied()
    }

    /// 상위 N개 노드 반환 (점수 내림차순)
    pub fn top_n(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut items: Vec<_> = self
            .scores
            .iter()
            .map(|(&id, &score)| (id, score))
            .collect();
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(n);
        items
    }
}

/// Eigenvector Centrality 계산기
pub struct EigenvectorCentrality<'a> {
    engine: &'a GraphEngine,
    config: EigenvectorConfig,
}

impl<'a> EigenvectorCentrality<'a> {
    /// 새로운 Eigenvector Centrality 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: EigenvectorConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: EigenvectorConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Eigenvector Centrality 계산
    ///
    /// Power Iteration 방법 사용:
    /// 1. 초기 벡터를 1/sqrt(n)으로 초기화
    /// 2. 반복: x_new = A * x (인접 행렬 곱셈)
    /// 3. 정규화: x_new = x_new / ||x_new||
    /// 4. 수렴 체크: ||x_new - x|| < tolerance
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<EigenvectorResult> {
        // 1. 그래프 구조 수집
        let (nodes, adjacency) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(EigenvectorResult {
                scores: HashMap::new(),
                iterations: 0,
                converged: true,
            });
        }

        let n = nodes.len();

        // 2. 초기 벡터 설정 (균등 분포)
        let initial_value = 1.0 / (n as f64).sqrt();
        let mut scores: HashMap<NodeId, f64> =
            nodes.iter().map(|&id| (id, initial_value)).collect();

        let mut converged = false;
        let mut iterations = 0;

        // 3. Power Iteration
        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // 새로운 점수 계산: x_new = A * x
            let mut new_scores: HashMap<NodeId, f64> = HashMap::new();

            for &node in &nodes {
                let mut sum = 0.0;

                // 이웃 노드들의 점수 합산
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        sum += scores.get(&neighbor).unwrap_or(&0.0);
                    }
                }

                new_scores.insert(node, sum);
            }

            // L2 norm 계산
            let norm: f64 = new_scores.values().map(|&v| v * v).sum::<f64>().sqrt();

            // 정규화 (0으로 나누기 방지)
            if norm > 1e-10 {
                for score in new_scores.values_mut() {
                    *score /= norm;
                }
            }

            // 수렴 체크
            let diff: f64 = nodes
                .iter()
                .map(|&node| {
                    let old = scores.get(&node).unwrap_or(&0.0);
                    let new = new_scores.get(&node).unwrap_or(&0.0);
                    (old - new).abs()
                })
                .sum();

            scores = new_scores;

            if diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 4. 최종 정규화 (최대값을 1.0으로)
        if self.config.normalized {
            let max_score = scores.values().cloned().fold(0.0_f64, f64::max);

            if max_score > 1e-10 {
                for score in scores.values_mut() {
                    *score /= max_score;
                }
            }
        }

        Ok(EigenvectorResult {
            scores,
            iterations,
            converged,
        })
    }

    /// 단일 노드의 Eigenvector Centrality 점수 계산
    pub async fn calculate_single(
        &self,
        node_id: NodeId,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.score(node_id).unwrap_or(0.0))
    }

    /// 그래프 구조 빌드 (노드 목록, 인접 리스트)
    async fn build_graph_structure(
        &self,
        rel_types: Option<&[String]>,
    ) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // Eigenvector는 들어오는 엣지를 기반으로 계산
                // to 노드의 점수에 from 노드가 기여
                adjacency.entry(to).or_default().push(from);

                // 무방향 그래프: 양방향 추가
                if !self.config.directed {
                    adjacency.entry(from).or_default().push(to);
                }
            }
        }

        Ok((nodes, adjacency))
    }
}

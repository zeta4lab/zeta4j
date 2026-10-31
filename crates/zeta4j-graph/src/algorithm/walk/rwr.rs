// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Random Walk with Restart (RWR) 알고리즘
//!
//! Personalized PageRank의 기반이 되는 알고리즘입니다.
//! 각 스텝에서 일정 확률로 시작 노드로 복귀하여
//! 시작 노드와의 관련성을 측정합니다.
//!
//! ## 알고리즘 개요
//! 1. 시작 노드에 초기 확률 1.0 할당
//! 2. 각 반복에서:
//!    - (1-α) 확률로 이웃으로 이동 (확률 분산)
//!    - α 확률로 시작 노드로 복귀
//! 3. 수렴할 때까지 반복
//!
//! ## 활용
//! - 개인화된 추천
//! - 노드 유사도 측정
//! - 관련 노드 탐색

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Random Walk with Restart 설정
#[derive(Debug, Clone)]
pub struct RWRConfig {
    /// 복귀 확률 (0.0 ~ 1.0, 기본값: 0.15)
    pub restart_probability: f64,
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 임계값 (L1 norm 차이)
    pub convergence_threshold: f64,
    /// 관계 타입 필터 (None이면 모든 타입)
    pub relationship_types: Option<Vec<String>>,
    /// 결과에서 상위 N개만 반환 (None이면 모두)
    pub top_k: Option<usize>,
}

impl Default for RWRConfig {
    fn default() -> Self {
        Self {
            restart_probability: 0.15,
            max_iterations: 100,
            convergence_threshold: 1e-6,
            relationship_types: None,
            top_k: None,
        }
    }
}

impl RWRConfig {
    /// 복귀 확률 설정
    pub fn with_restart_probability(mut self, prob: f64) -> Self {
        self.restart_probability = prob.clamp(0.0, 1.0);
        self
    }

    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iter: usize) -> Self {
        self.max_iterations = max_iter;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_convergence_threshold(mut self, threshold: f64) -> Self {
        self.convergence_threshold = threshold;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 상위 K개 결과만 반환
    pub fn with_top_k(mut self, k: usize) -> Self {
        self.top_k = Some(k);
        self
    }
}

/// Random Walk with Restart 결과
#[derive(Debug, Clone)]
pub struct RWRResult {
    /// 노드별 방문 확률/점수
    pub scores: HashMap<NodeId, f64>,
    /// 시작 노드
    pub start_node: NodeId,
    /// 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 최종 L1 norm 차이
    pub final_diff: f64,
}

impl RWRResult {
    /// 특정 노드의 점수
    pub fn score(&self, node_id: NodeId) -> f64 {
        *self.scores.get(&node_id).unwrap_or(&0.0)
    }

    /// 점수 기준 상위 N개 노드
    pub fn top_n(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut sorted: Vec<_> = self
            .scores
            .iter()
            .map(|(&id, &score)| (id, score))
            .collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(n);
        sorted
    }

    /// 임계값 이상 점수를 가진 노드들
    pub fn above_threshold(&self, threshold: f64) -> Vec<(NodeId, f64)> {
        self.scores
            .iter()
            .filter(|&(_, &score)| score >= threshold)
            .map(|(&id, &score)| (id, score))
            .collect()
    }

    /// 시작 노드 제외한 상위 N개 (관련 노드 찾기)
    pub fn related_nodes(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut sorted: Vec<_> = self
            .scores
            .iter()
            .filter(|&(&id, _)| id != self.start_node)
            .map(|(&id, &score)| (id, score))
            .collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(n);
        sorted
    }

    /// 점수 정규화 (합이 1이 되도록)
    pub fn normalized_scores(&self) -> HashMap<NodeId, f64> {
        let sum: f64 = self.scores.values().sum();
        if sum <= 0.0 {
            return self.scores.clone();
        }
        self.scores
            .iter()
            .map(|(&id, &score)| (id, score / sum))
            .collect()
    }
}

/// Random Walk with Restart 알고리즘
pub struct RandomWalkWithRestart<'a> {
    engine: &'a GraphEngine,
    config: RWRConfig,
}

impl<'a> RandomWalkWithRestart<'a> {
    /// 새로운 RWR 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: RWRConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: RWRConfig) -> Self {
        Self { engine, config }
    }

    /// RWR 실행 (Power Iteration)
    pub async fn run(&self, start_node: NodeId) -> Result<RWRResult> {
        // 1. 모든 노드 수집 및 이웃 맵 구축
        let nodes = self.get_all_node_ids().await?;

        if nodes.is_empty() {
            return Ok(RWRResult {
                scores: HashMap::new(),
                start_node,
                iterations: 0,
                converged: true,
                final_diff: 0.0,
            });
        }

        let neighbors_map = self.build_neighbors_map(&nodes).await?;
        let degrees = self.compute_degrees(&neighbors_map);

        // 2. 초기 확률 분포 (시작 노드에 1.0)
        let mut scores: HashMap<NodeId, f64> = nodes
            .iter()
            .map(|&id| (id, if id == start_node { 1.0 } else { 0.0 }))
            .collect();

        // 3. Power Iteration
        let alpha = self.config.restart_probability;
        let mut iterations = 0;
        let mut converged = false;
        let mut final_diff = 0.0;

        while iterations < self.config.max_iterations && !converged {
            iterations += 1;

            let mut new_scores: HashMap<NodeId, f64> = HashMap::new();

            // Dangling nodes (이웃 없는 노드)의 총 확률 계산
            // 이 확률은 시작 노드로 리다이렉트됨
            let dangling_sum: f64 = neighbors_map
                .iter()
                .filter(|(_, neighbors)| neighbors.is_empty())
                .map(|(id, _)| *scores.get(id).unwrap_or(&0.0))
                .sum();

            // 각 노드에 대해 새 점수 계산
            for &node in &nodes {
                let mut contribution = 0.0;

                // 이웃으로부터의 기여 (1 - alpha)
                if let Some(neighbors) = neighbors_map.get(&node) {
                    for &neighbor in neighbors {
                        let neighbor_degree = *degrees.get(&neighbor).unwrap_or(&1);
                        let neighbor_score = *scores.get(&neighbor).unwrap_or(&0.0);
                        contribution += (1.0 - alpha) * neighbor_score / neighbor_degree as f64;
                    }
                }

                // 복귀 확률 (alpha) + dangling nodes 확률 - 시작 노드에만
                if node == start_node {
                    contribution += alpha;
                    // Dangling nodes의 (1-alpha) 확률도 시작 노드로
                    contribution += (1.0 - alpha) * dangling_sum;
                }

                new_scores.insert(node, contribution);
            }

            // 수렴 체크 (L1 norm)
            let diff: f64 = nodes
                .iter()
                .map(|&id| {
                    let old = *scores.get(&id).unwrap_or(&0.0);
                    let new = *new_scores.get(&id).unwrap_or(&0.0);
                    (old - new).abs()
                })
                .sum();

            final_diff = diff;

            if diff < self.config.convergence_threshold {
                converged = true;
            }

            scores = new_scores;
        }

        // 4. top_k 필터링
        let final_scores = if let Some(k) = self.config.top_k {
            let mut sorted: Vec<_> = scores.into_iter().collect();
            sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            sorted.truncate(k);
            sorted.into_iter().collect()
        } else {
            scores
        };

        Ok(RWRResult {
            scores: final_scores,
            start_node,
            iterations,
            converged,
            final_diff,
        })
    }

    /// 여러 시작 노드에서 RWR 실행
    pub async fn run_from_nodes(&self, start_nodes: &[NodeId]) -> Result<Vec<RWRResult>> {
        let mut results = Vec::new();
        for &node in start_nodes {
            results.push(self.run(node).await?);
        }
        Ok(results)
    }

    /// 두 노드 간 유사도 계산 (RWR 점수 기반)
    pub async fn similarity(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        let result1 = self.run(node1).await?;
        let result2 = self.run(node2).await?;

        // 양방향 점수의 기하 평균
        let score1_to_2 = result1.score(node2);
        let score2_to_1 = result2.score(node1);

        Ok((score1_to_2 * score2_to_1).sqrt())
    }

    /// 모든 노드 ID 가져오기
    async fn get_all_node_ids(&self) -> Result<Vec<NodeId>> {
        let mut nodes = Vec::new();
        let node_count = self.engine.node_count().await?;

        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
            }
        }

        Ok(nodes)
    }

    /// 이웃 맵 구축 (무방향)
    async fn build_neighbors_map(&self, nodes: &[NodeId]) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut neighbors_map: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for &node in nodes {
            neighbors_map.insert(node, Vec::new());
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                // 양방향 추가 (무방향 그래프로 처리)
                if neighbors_map.contains_key(&from)
                    && neighbors_map.contains_key(&to)
                    && from != to
                {
                    if let Some(neighbors) = neighbors_map.get_mut(&from) {
                        neighbors.push(to);
                    }
                    if let Some(neighbors) = neighbors_map.get_mut(&to) {
                        neighbors.push(from);
                    }
                }
            }
        }

        Ok(neighbors_map)
    }

    /// 차수 계산
    fn compute_degrees(
        &self,
        neighbors_map: &HashMap<NodeId, Vec<NodeId>>,
    ) -> HashMap<NodeId, usize> {
        neighbors_map
            .iter()
            .map(|(&id, neighbors)| (id, neighbors.len().max(1))) // 최소 1 (0 division 방지)
            .collect()
    }
}

/// RWR 편의 함수
pub async fn rwr(engine: &GraphEngine, start_node: NodeId) -> Result<RWRResult> {
    RandomWalkWithRestart::new(engine).run(start_node).await
}

/// RWR with custom restart probability
pub async fn rwr_with_restart_prob(
    engine: &GraphEngine,
    start_node: NodeId,
    restart_prob: f64,
) -> Result<RWRResult> {
    let config = RWRConfig::default().with_restart_probability(restart_prob);
    RandomWalkWithRestart::with_config(engine, config)
        .run(start_node)
        .await
}

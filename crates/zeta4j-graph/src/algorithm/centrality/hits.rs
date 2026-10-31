// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! HITS (Hyperlink-Induced Topic Search) 알고리즘
//!
//! Kleinberg(1999)가 제안한 알고리즘으로, 각 노드에 두 가지 점수를 부여합니다:
//! - Authority 점수: 좋은 Hub들로부터 링크를 받는 노드
//! - Hub 점수: 좋은 Authority들을 가리키는 노드
//!
//! 반복 공식:
//! - auth(v) = Σ hub(u) for all u → v (들어오는 엣지)
//! - hub(v) = Σ auth(u) for all v → u (나가는 엣지)
//!
//! 시간복잡도: O(k * (V + E)) where k is number of iterations

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// HITS 설정
#[derive(Debug, Clone)]
pub struct HITSConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 수렴 허용 오차
    pub tolerance: f64,
    /// 정규화 여부 (true면 최대값이 1.0이 되도록 정규화)
    pub normalized: bool,
}

impl Default for HITSConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1e-6,
            normalized: true,
        }
    }
}

impl HITSConfig {
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
}

/// HITS 결과
#[derive(Debug, Clone)]
pub struct HITSResult {
    /// 노드별 Hub 점수
    pub hubs: HashMap<NodeId, f64>,
    /// 노드별 Authority 점수
    pub authorities: HashMap<NodeId, f64>,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
}

impl HITSResult {
    /// 특정 노드의 Hub 점수 조회
    pub fn hub_score(&self, node_id: NodeId) -> Option<f64> {
        self.hubs.get(&node_id).copied()
    }

    /// 특정 노드의 Authority 점수 조회
    pub fn authority_score(&self, node_id: NodeId) -> Option<f64> {
        self.authorities.get(&node_id).copied()
    }

    /// 상위 N개 Hub 반환 (점수 내림차순)
    pub fn top_hubs(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut items: Vec<_> = self.hubs.iter().map(|(&id, &score)| (id, score)).collect();
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(n);
        items
    }

    /// 상위 N개 Authority 반환 (점수 내림차순)
    pub fn top_authorities(&self, n: usize) -> Vec<(NodeId, f64)> {
        let mut items: Vec<_> = self
            .authorities
            .iter()
            .map(|(&id, &score)| (id, score))
            .collect();
        items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        items.truncate(n);
        items
    }

    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            hubs: HashMap::new(),
            authorities: HashMap::new(),
            iterations: 0,
            converged: true,
        }
    }
}

/// HITS 알고리즘 계산기
pub struct HITS<'a> {
    engine: &'a GraphEngine,
    config: HITSConfig,
}

impl<'a> HITS<'a> {
    /// 새로운 HITS 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: HITSConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: HITSConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 HITS 계산
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<HITSResult> {
        // 1. 그래프 구조 수집 (in_edges, out_edges)
        let (nodes, in_edges, out_edges) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(HITSResult::empty());
        }

        // 2. 초기 점수 설정 (모두 1.0)
        let mut hubs: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 1.0)).collect();
        let mut authorities: HashMap<NodeId, f64> = nodes.iter().map(|&id| (id, 1.0)).collect();

        let mut converged = false;
        let mut iterations = 0;

        // 3. 반복 계산
        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // 3.1 Authority 점수 업데이트
            // auth(v) = Σ hub(u) for all u → v
            let mut new_authorities: HashMap<NodeId, f64> = HashMap::new();
            for &node in &nodes {
                let mut sum = 0.0;
                if let Some(incoming) = in_edges.get(&node) {
                    for &from in incoming {
                        sum += hubs.get(&from).unwrap_or(&0.0);
                    }
                }
                new_authorities.insert(node, sum);
            }

            // Authority 정규화 (L2 norm)
            let auth_norm: f64 = new_authorities.values().map(|&v| v * v).sum::<f64>().sqrt();
            if auth_norm > 1e-10 {
                for score in new_authorities.values_mut() {
                    *score /= auth_norm;
                }
            }

            // 3.2 Hub 점수 업데이트
            // hub(v) = Σ auth(u) for all v → u
            let mut new_hubs: HashMap<NodeId, f64> = HashMap::new();
            for &node in &nodes {
                let mut sum = 0.0;
                if let Some(outgoing) = out_edges.get(&node) {
                    for &to in outgoing {
                        sum += new_authorities.get(&to).unwrap_or(&0.0);
                    }
                }
                new_hubs.insert(node, sum);
            }

            // Hub 정규화 (L2 norm)
            let hub_norm: f64 = new_hubs.values().map(|&v| v * v).sum::<f64>().sqrt();
            if hub_norm > 1e-10 {
                for score in new_hubs.values_mut() {
                    *score /= hub_norm;
                }
            }

            // 3.3 수렴 체크
            let hub_diff: f64 = nodes
                .iter()
                .map(|&node| {
                    let old = hubs.get(&node).unwrap_or(&0.0);
                    let new = new_hubs.get(&node).unwrap_or(&0.0);
                    (old - new).abs()
                })
                .sum();

            let auth_diff: f64 = nodes
                .iter()
                .map(|&node| {
                    let old = authorities.get(&node).unwrap_or(&0.0);
                    let new = new_authorities.get(&node).unwrap_or(&0.0);
                    (old - new).abs()
                })
                .sum();

            hubs = new_hubs;
            authorities = new_authorities;

            if hub_diff < self.config.tolerance && auth_diff < self.config.tolerance {
                converged = true;
                break;
            }
        }

        // 4. 최종 정규화 (최대값을 1.0으로)
        if self.config.normalized {
            let max_hub = hubs.values().cloned().fold(0.0_f64, f64::max);
            if max_hub > 1e-10 {
                for score in hubs.values_mut() {
                    *score /= max_hub;
                }
            }

            let max_auth = authorities.values().cloned().fold(0.0_f64, f64::max);
            if max_auth > 1e-10 {
                for score in authorities.values_mut() {
                    *score /= max_auth;
                }
            }
        }

        Ok(HITSResult {
            hubs,
            authorities,
            iterations,
            converged,
        })
    }

    /// 단일 노드의 Hub 점수 계산
    pub async fn hub_score(&self, node_id: NodeId, rel_types: Option<&[String]>) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.hub_score(node_id).unwrap_or(0.0))
    }

    /// 단일 노드의 Authority 점수 계산
    pub async fn authority_score(
        &self,
        node_id: NodeId,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.authority_score(node_id).unwrap_or(0.0))
    }

    /// 그래프 구조 빌드 (노드 목록, in_edges, out_edges)
    async fn build_graph_structure(
        &self,
        rel_types: Option<&[String]>,
    ) -> Result<(
        Vec<NodeId>,
        HashMap<NodeId, Vec<NodeId>>,
        HashMap<NodeId, Vec<NodeId>>,
    )> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut in_edges: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut out_edges: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                in_edges.insert(node_id, Vec::new());
                out_edges.insert(node_id, Vec::new());
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

                // in_edges: to 노드로 들어오는 엣지 (from)
                in_edges.entry(to).or_default().push(from);
                // out_edges: from 노드에서 나가는 엣지 (to)
                out_edges.entry(from).or_default().push(to);
            }
        }

        Ok((nodes, in_edges, out_edges))
    }
}

/// 편의 함수: HITS 계산 (기본 설정)
pub async fn hits(engine: &GraphEngine) -> Result<HITSResult> {
    HITS::new(engine).calculate(None).await
}

/// 편의 함수: Hub 점수만 반환
pub async fn hub_scores(engine: &GraphEngine) -> Result<HashMap<NodeId, f64>> {
    let result = HITS::new(engine).calculate(None).await?;
    Ok(result.hubs)
}

/// 편의 함수: Authority 점수만 반환
pub async fn authority_scores(engine: &GraphEngine) -> Result<HashMap<NodeId, f64>> {
    let result = HITS::new(engine).calculate(None).await?;
    Ok(result.authorities)
}

/// 편의 함수: 단일 노드 Hub 점수
pub async fn hub_score(engine: &GraphEngine, node_id: NodeId) -> Result<f64> {
    HITS::new(engine).hub_score(node_id, None).await
}

/// 편의 함수: 단일 노드 Authority 점수
pub async fn authority_score(engine: &GraphEngine, node_id: NodeId) -> Result<f64> {
    HITS::new(engine).authority_score(node_id, None).await
}

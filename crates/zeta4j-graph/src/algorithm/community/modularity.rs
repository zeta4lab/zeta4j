// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Modularity Optimization 알고리즘
//!
//! 모듈러리티(Modularity)를 직접 최적화하는 커뮤니티 탐지 알고리즘입니다.
//! Louvain의 Phase 1만을 독립적으로 실행하여 모듈러리티를 최적화합니다.
//!
//! ## 알고리즘 개요
//! 1. 각 노드를 개별 커뮤니티로 초기화 (또는 seed communities 사용)
//! 2. 각 노드를 이웃 커뮤니티로 이동하며 모듈러리티 증가 확인
//! 3. 모듈러리티 개선이 없을 때까지 반복
//!
//! Louvain과의 차이점:
//! - Phase 2 (Aggregation) 없음: 커뮤니티를 단일 노드로 축소하지 않음
//! - 초기 커뮤니티 지정 가능: seed_communities 파라미터 지원

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Modularity Optimization 설정
#[derive(Debug, Clone)]
pub struct ModularityConfig {
    /// 최대 반복 횟수
    pub max_iterations: usize,
    /// 모듈러리티 개선 임계값 (이보다 작으면 수렴으로 판단)
    pub tolerance: f64,
    /// Resolution 파라미터 (1.0이 기본, 높을수록 작은 커뮤니티)
    pub resolution: f64,
    /// 초기 커뮤니티 할당 (None이면 각 노드가 개별 커뮤니티)
    pub seed_communities: Option<HashMap<NodeId, usize>>,
}

impl Default for ModularityConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tolerance: 1e-7,
            resolution: 1.0,
            seed_communities: None,
        }
    }
}

impl ModularityConfig {
    /// 최대 반복 횟수 설정
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 수렴 임계값 설정
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Resolution 파라미터 설정
    pub fn with_resolution(mut self, resolution: f64) -> Self {
        self.resolution = resolution;
        self
    }

    /// 초기 커뮤니티 할당 설정
    pub fn with_seed_communities(mut self, seeds: HashMap<NodeId, usize>) -> Self {
        self.seed_communities = Some(seeds);
        self
    }
}

/// Modularity Optimization 결과
#[derive(Debug, Clone)]
pub struct ModularityResult {
    /// 노드별 커뮤니티 ID
    pub communities: HashMap<NodeId, usize>,
    /// 커뮤니티 수
    pub community_count: usize,
    /// 최종 모듈러리티 값
    pub modularity: f64,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 수렴 여부
    pub converged: bool,
    /// 초기 모듈러리티 대비 개선량
    pub improvement: f64,
    /// 커뮤니티별 노드 수
    pub community_sizes: HashMap<usize, usize>,
}

impl ModularityResult {
    /// 특정 노드의 커뮤니티 ID 조회
    pub fn community_id(&self, node_id: NodeId) -> Option<usize> {
        self.communities.get(&node_id).copied()
    }

    /// 가장 큰 커뮤니티의 ID와 크기 반환
    pub fn largest_community(&self) -> Option<(usize, usize)> {
        self.community_sizes
            .iter()
            .max_by_key(|&(_, &size)| size)
            .map(|(&id, &size)| (id, size))
    }

    /// 특정 커뮤니티에 속한 노드들 반환
    pub fn nodes_in_community(&self, community_id: usize) -> Vec<NodeId> {
        self.communities
            .iter()
            .filter(|&(_, &cid)| cid == community_id)
            .map(|(&node_id, _)| node_id)
            .collect()
    }
}

/// 내부 그래프 표현
struct ModularityGraph {
    /// 노드 ID 목록 (인덱스 -> NodeId)
    nodes: Vec<NodeId>,
    /// NodeId -> 인덱스 매핑
    node_to_idx: HashMap<NodeId, usize>,
    /// 인접 리스트: 각 노드의 이웃과 가중치
    adjacency: Vec<Vec<(usize, f64)>>,
    /// 노드별 가중치 합 (degree * weight)
    node_weights: Vec<f64>,
    /// 총 엣지 가중치 (2m)
    total_weight: f64,
}

impl ModularityGraph {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_to_idx: HashMap::new(),
            adjacency: Vec::new(),
            node_weights: Vec::new(),
            total_weight: 0.0,
        }
    }

    fn neighbors(&self, node_idx: usize) -> &[(usize, f64)] {
        &self.adjacency[node_idx]
    }
}

/// Modularity Optimization 알고리즘
pub struct ModularityOptimization<'a> {
    engine: &'a GraphEngine,
    config: ModularityConfig,
}

impl<'a> ModularityOptimization<'a> {
    /// 새로운 Modularity Optimization 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ModularityConfig::default(),
        }
    }

    /// 설정과 함께 Modularity Optimization 생성
    pub fn with_config(engine: &'a GraphEngine, config: ModularityConfig) -> Self {
        Self { engine, config }
    }

    /// 모듈러리티 최적화 실행
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    ///
    /// # Returns
    /// 최적화 결과 (커뮤니티 할당, 모듈러리티, 반복 횟수 등)
    #[expect(clippy::needless_range_loop)]
    pub async fn optimize(&self, rel_types: Option<&[String]>) -> Result<ModularityResult> {
        // 1. 그래프 구조 빌드
        let graph = self.build_graph(rel_types).await?;

        if graph.nodes.is_empty() {
            return Ok(ModularityResult {
                communities: HashMap::new(),
                community_count: 0,
                modularity: 0.0,
                iterations: 0,
                converged: true,
                improvement: 0.0,
                community_sizes: HashMap::new(),
            });
        }

        let n = graph.nodes.len();

        // 2. 커뮤니티 초기화
        let mut node_to_community: Vec<usize> = if let Some(seeds) = &self.config.seed_communities {
            // seed communities 사용
            let mut assignments = vec![0; n];
            for (i, node_id) in graph.nodes.iter().enumerate() {
                assignments[i] = seeds.get(node_id).copied().unwrap_or(i);
            }
            assignments
        } else {
            // 각 노드가 개별 커뮤니티
            (0..n).collect()
        };

        // 커뮤니티별 가중치 합 초기화
        let max_comm = *node_to_community.iter().max().unwrap_or(&0);
        let mut community_weights: Vec<f64> = vec![0.0; max_comm + n + 1];
        let mut community_internal: Vec<f64> = vec![0.0; max_comm + n + 1];

        for i in 0..n {
            let comm = node_to_community[i];
            community_weights[comm] += graph.node_weights[i];

            // 자기 자신으로의 내부 엣지 (self-loop)
            for &(neighbor_idx, weight) in graph.neighbors(i) {
                if node_to_community[neighbor_idx] == comm {
                    community_internal[comm] += weight;
                }
            }
        }

        // 초기 모듈러리티 계산
        let initial_modularity = self.calculate_modularity(
            &graph,
            &node_to_community,
            &community_weights,
            &community_internal,
        );

        // 3. Local Moving Phase
        let mut iterations = 0;
        let mut converged = false;
        let mut current_modularity = initial_modularity;

        for _ in 0..self.config.max_iterations {
            iterations += 1;
            let mut improved = false;

            for i in 0..n {
                let current_comm = node_to_community[i];
                let node_weight = graph.node_weights[i];

                // 현재 커뮤니티에서 제거
                community_weights[current_comm] -= node_weight;
                let (ki_in_current, _) = self.ki_in(&graph, i, current_comm, &node_to_community);
                community_internal[current_comm] -= ki_in_current * 2.0; // 양방향

                // 이웃 커뮤니티들 수집
                let mut neighbor_comms: Vec<usize> = graph
                    .neighbors(i)
                    .iter()
                    .map(|&(neighbor_idx, _)| node_to_community[neighbor_idx])
                    .collect();
                neighbor_comms.sort_unstable();
                neighbor_comms.dedup();

                // 현재 커뮤니티도 후보에 포함
                if !neighbor_comms.contains(&current_comm) {
                    neighbor_comms.push(current_comm);
                }

                // 최적의 커뮤니티 찾기
                let mut best_comm = current_comm;
                let mut best_delta = 0.0;

                let m2 = graph.total_weight;
                for &target_comm in &neighbor_comms {
                    let (ki_in_target, _) = self.ki_in(&graph, i, target_comm, &node_to_community);
                    let sigma_tot = community_weights[target_comm];

                    // Δ modularity
                    let delta = ki_in_target / m2
                        - self.config.resolution * (sigma_tot * node_weight) / (m2 * m2) * 2.0;

                    if delta > best_delta {
                        best_delta = delta;
                        best_comm = target_comm;
                    }
                }

                // 커뮤니티 업데이트
                node_to_community[i] = best_comm;
                community_weights[best_comm] += node_weight;
                let (ki_in_best, _) = self.ki_in(&graph, i, best_comm, &node_to_community);
                community_internal[best_comm] += ki_in_best * 2.0;

                if best_comm != current_comm {
                    improved = true;
                }
            }

            // 모듈러리티 재계산
            let new_modularity = self.calculate_modularity(
                &graph,
                &node_to_community,
                &community_weights,
                &community_internal,
            );

            if !improved || (new_modularity - current_modularity).abs() < self.config.tolerance {
                converged = true;
                current_modularity = new_modularity;
                break;
            }

            current_modularity = new_modularity;
        }

        // 4. 결과 정리
        let mut communities: HashMap<NodeId, usize> = HashMap::new();
        let mut community_sizes: HashMap<usize, usize> = HashMap::new();

        // 커뮤니티 ID 연속화 (0, 1, 2, ...)
        let mut comm_remap: HashMap<usize, usize> = HashMap::new();
        let mut next_id = 0;

        for i in 0..n {
            let old_comm = node_to_community[i];
            let new_comm = *comm_remap.entry(old_comm).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            communities.insert(graph.nodes[i], new_comm);
            *community_sizes.entry(new_comm).or_insert(0) += 1;
        }

        Ok(ModularityResult {
            communities,
            community_count: next_id,
            modularity: current_modularity,
            iterations,
            converged,
            improvement: current_modularity - initial_modularity,
            community_sizes,
        })
    }

    /// 주어진 커뮤니티 할당에 대한 모듈러리티 계산 (최적화 없이)
    ///
    /// # Arguments
    /// * `communities` - 노드별 커뮤니티 할당
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate_modularity_from_assignment(
        &self,
        communities: &HashMap<NodeId, usize>,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let graph = self.build_graph(rel_types).await?;

        if graph.nodes.is_empty() {
            return Ok(0.0);
        }

        let n = graph.nodes.len();
        let mut node_to_community: Vec<usize> = vec![0; n];

        for (i, node_id) in graph.nodes.iter().enumerate() {
            node_to_community[i] = communities.get(node_id).copied().unwrap_or(i);
        }

        let max_comm = *node_to_community.iter().max().unwrap_or(&0);
        let mut community_weights: Vec<f64> = vec![0.0; max_comm + 1];
        let mut community_internal: Vec<f64> = vec![0.0; max_comm + 1];

        for i in 0..n {
            let comm = node_to_community[i];
            community_weights[comm] += graph.node_weights[i];

            for &(neighbor_idx, weight) in graph.neighbors(i) {
                if node_to_community[neighbor_idx] == comm {
                    community_internal[comm] += weight;
                }
            }
        }

        Ok(self.calculate_modularity(
            &graph,
            &node_to_community,
            &community_weights,
            &community_internal,
        ))
    }

    /// 노드 i가 커뮤니티 community에 대해 가지는 연결 가중치 합
    fn ki_in(
        &self,
        graph: &ModularityGraph,
        node_idx: usize,
        community: usize,
        node_to_community: &[usize],
    ) -> (f64, usize) {
        let mut ki_in = 0.0;
        let mut count = 0;

        for &(neighbor_idx, weight) in graph.neighbors(node_idx) {
            if node_to_community[neighbor_idx] == community {
                ki_in += weight;
                count += 1;
            }
        }

        (ki_in, count)
    }

    /// 모듈러리티 계산
    /// Q = (1/2m) * Σ[A_ij - (k_i * k_j)/(2m)] * δ(c_i, c_j)
    fn calculate_modularity(
        &self,
        graph: &ModularityGraph,
        node_to_community: &[usize],
        community_weights: &[f64],
        community_internal: &[f64],
    ) -> f64 {
        let m2 = graph.total_weight;
        if m2 == 0.0 {
            return 0.0;
        }

        let mut q = 0.0;
        let max_comm = node_to_community.iter().copied().max().unwrap_or(0);

        for comm in 0..=max_comm {
            if community_weights.len() > comm && community_weights[comm] > 0.0 {
                let sigma_tot = community_weights[comm];
                let sigma_in = community_internal.get(comm).copied().unwrap_or(0.0);

                // Q += sigma_in / 2m - resolution * (sigma_tot / 2m)²
                q += sigma_in / m2 - self.config.resolution * (sigma_tot / m2).powi(2);
            }
        }

        q
    }

    /// GraphEngine에서 그래프 구조 빌드
    async fn build_graph(&self, rel_types: Option<&[String]>) -> Result<ModularityGraph> {
        let mut graph = ModularityGraph::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                let idx = graph.nodes.len();
                graph.node_to_idx.insert(node_id, idx);
                graph.nodes.push(node_id);
                graph.adjacency.push(Vec::new());
                graph.node_weights.push(0.0);
            }
        }

        if graph.nodes.is_empty() {
            return Ok(graph);
        }

        // 엣지 수집 (무방향 그래프로 처리)
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

                if let (Some(&from_idx), Some(&to_idx)) =
                    (graph.node_to_idx.get(&from), graph.node_to_idx.get(&to))
                {
                    let weight = 1.0;

                    // 무방향 그래프: 양방향 추가
                    graph.adjacency[from_idx].push((to_idx, weight));
                    graph.node_weights[from_idx] += weight;
                    graph.total_weight += weight;

                    if from_idx != to_idx {
                        graph.adjacency[to_idx].push((from_idx, weight));
                        graph.node_weights[to_idx] += weight;
                        graph.total_weight += weight;
                    }
                }
            }
        }

        Ok(graph)
    }
}

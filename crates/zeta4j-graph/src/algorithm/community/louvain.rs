//! Louvain Community Detection 알고리즘
//!
//! 모듈러리티(Modularity) 최적화 기반의 커뮤니티 탐지 알고리즘입니다.
//! 대규모 그래프에서 효율적으로 커뮤니티를 발견할 수 있습니다.
//!
//! ## 알고리즘 개요
//! 1. Phase 1 (Local Moving): 각 노드를 이웃 커뮤니티로 이동하며 모듈러리티 증가 확인
//! 2. Phase 2 (Aggregation): 커뮤니티를 단일 노드로 축소하여 새 그래프 생성
//! 3. 수렴할 때까지 반복
//!
//! 시간 복잡도: O(n log n) 평균, 최악 O(n²)

use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Louvain 알고리즘 설정
#[derive(Debug, Clone)]
pub struct LouvainConfig {
    /// 최대 반복 횟수 (Phase 1 + Phase 2 사이클)
    pub max_iterations: usize,
    /// 모듈러리티 개선 임계값 (이보다 작으면 수렴으로 판단)
    pub tolerance: f64,
    /// Resolution 파라미터 (1.0이 기본, 높을수록 작은 커뮤니티)
    pub resolution: f64,
    /// 랜덤 시드 (노드 순서 셔플용, None이면 고정 순서)
    pub random_seed: Option<u64>,
}

impl Default for LouvainConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tolerance: 1e-7,
            resolution: 1.0,
            random_seed: None,
        }
    }
}

impl LouvainConfig {
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
}

/// Louvain 알고리즘 결과
#[derive(Debug, Clone)]
pub struct LouvainResult {
    /// 노드별 커뮤니티 ID
    pub communities: HashMap<NodeId, usize>,
    /// 커뮤니티 수
    pub community_count: usize,
    /// 최종 모듈러리티 값
    pub modularity: f64,
    /// 실제 반복 횟수
    pub iterations: usize,
    /// 커뮤니티별 노드 수
    pub community_sizes: HashMap<usize, usize>,
}

impl LouvainResult {
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

    /// 두 노드가 같은 커뮤니티에 속하는지 확인
    pub fn same_community(&self, node1: NodeId, node2: NodeId) -> bool {
        match (self.community_id(node1), self.community_id(node2)) {
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }
}

/// 내부 그래프 표현 (Louvain 알고리즘용)
struct LouvainGraph {
    /// 노드 목록
    nodes: Vec<NodeId>,
    /// 노드 ID -> 인덱스 매핑
    node_to_idx: HashMap<NodeId, usize>,
    /// 인접 리스트: node_idx -> [(neighbor_idx, weight)]
    adjacency: Vec<Vec<(usize, f64)>>,
    /// 노드별 가중치 합 (자기 루프 포함)
    node_weights: Vec<f64>,
    /// 전체 엣지 가중치 합 (2m)
    total_weight: f64,
}

impl LouvainGraph {
    /// 새 그래프 생성
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_to_idx: HashMap::new(),
            adjacency: Vec::new(),
            node_weights: Vec::new(),
            total_weight: 0.0,
        }
    }

    /// 노드 수
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 노드의 이웃과 가중치 조회
    fn neighbors(&self, node_idx: usize) -> &[(usize, f64)] {
        &self.adjacency[node_idx]
    }
}

/// Louvain Community Detection 계산기
pub struct Louvain<'a> {
    engine: &'a GraphEngine,
    config: LouvainConfig,
}

impl<'a> Louvain<'a> {
    /// 새로운 Louvain 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: LouvainConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: LouvainConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Louvain 커뮤니티 탐지 실행
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<LouvainResult> {
        // 1. 그래프 구조 빌드
        let graph = self.build_graph(rel_types).await?;

        if graph.node_count() == 0 {
            return Ok(LouvainResult {
                communities: HashMap::new(),
                community_count: 0,
                modularity: 0.0,
                iterations: 0,
                community_sizes: HashMap::new(),
            });
        }

        // 2. 초기 커뮤니티 할당 (각 노드가 자신만의 커뮤니티)
        let mut node_to_community: Vec<usize> = (0..graph.node_count()).collect();
        let mut community_weights: Vec<f64> = graph.node_weights.clone();
        let mut community_internal: Vec<f64> = vec![0.0; graph.node_count()];

        // 자기 루프 가중치를 내부 가중치에 추가
        for (node_idx, neighbors) in graph.adjacency.iter().enumerate() {
            for &(neighbor_idx, weight) in neighbors {
                if neighbor_idx == node_idx {
                    community_internal[node_idx] += weight;
                }
            }
        }

        let mut iterations = 0;
        let mut improved = true;

        // 3. Phase 1 + Phase 2 반복
        while improved && iterations < self.config.max_iterations {
            improved = false;
            iterations += 1;

            // Phase 1: Local Moving
            let phase1_improved = self.phase1_local_moving(
                &graph,
                &mut node_to_community,
                &mut community_weights,
                &mut community_internal,
            );

            if phase1_improved {
                improved = true;
            }
        }

        // 4. 결과 정리
        let mut communities: HashMap<NodeId, usize> = HashMap::new();
        let mut community_remap: HashMap<usize, usize> = HashMap::new();
        let mut next_community_id = 0;

        for (node_idx, &comm) in node_to_community.iter().enumerate() {
            let remapped = *community_remap.entry(comm).or_insert_with(|| {
                let id = next_community_id;
                next_community_id += 1;
                id
            });
            communities.insert(graph.nodes[node_idx], remapped);
        }

        // 커뮤니티별 크기 계산
        let mut community_sizes: HashMap<usize, usize> = HashMap::new();
        for &comm in communities.values() {
            *community_sizes.entry(comm).or_insert(0) += 1;
        }

        // 최종 모듈러리티 계산
        let modularity = self.calculate_modularity(
            &graph,
            &node_to_community,
            &community_weights,
            &community_internal,
        );

        Ok(LouvainResult {
            communities,
            community_count: next_community_id,
            modularity,
            iterations,
            community_sizes,
        })
    }

    /// Phase 1: 로컬 이동 최적화
    /// 각 노드를 이웃 커뮤니티로 이동하면서 모듈러리티 증가 확인
    fn phase1_local_moving(
        &self,
        graph: &LouvainGraph,
        node_to_community: &mut [usize],
        community_weights: &mut [f64],
        community_internal: &mut [f64],
    ) -> bool {
        let m2 = graph.total_weight; // 2m
        if m2 == 0.0 {
            return false;
        }

        let mut improved = false;
        let mut changed = true;

        while changed {
            changed = false;

            for node_idx in 0..graph.node_count() {
                let current_comm = node_to_community[node_idx];
                let node_weight = graph.node_weights[node_idx];

                // 현재 커뮤니티에서 노드 제거 시 모듈러리티 변화 계산
                let (ki_in_current, _) =
                    self.calculate_ki_in(graph, node_idx, current_comm, node_to_community);

                // 현재 커뮤니티에서 노드 제거
                community_weights[current_comm] -= node_weight;
                community_internal[current_comm] -= ki_in_current;

                // 이웃 커뮤니티들 수집
                let mut neighbor_communities: HashSet<usize> = HashSet::new();
                for &(neighbor_idx, _) in graph.neighbors(node_idx) {
                    if neighbor_idx != node_idx {
                        neighbor_communities.insert(node_to_community[neighbor_idx]);
                    }
                }
                neighbor_communities.insert(current_comm); // 현재 커뮤니티도 후보

                // 최적의 커뮤니티 찾기
                let mut best_comm = current_comm;
                let mut best_delta = 0.0;

                for &target_comm in &neighbor_communities {
                    let (ki_in_target, _) =
                        self.calculate_ki_in(graph, node_idx, target_comm, node_to_community);

                    let sigma_tot = community_weights[target_comm];

                    // 모듈러리티 증가량 계산
                    // ΔQ = [ki_in / m - resolution * (sigma_tot * ki) / (2m²)]
                    let delta = ki_in_target / m2
                        - self.config.resolution * (sigma_tot * node_weight) / (m2 * m2) * 2.0;

                    if delta > best_delta {
                        best_delta = delta;
                        best_comm = target_comm;
                    }
                }

                // 최적 커뮤니티로 이동
                if best_comm != current_comm {
                    node_to_community[node_idx] = best_comm;
                    changed = true;
                    improved = true;
                }

                // 새 커뮤니티에 노드 추가
                let (ki_in_new, _) = self.calculate_ki_in(
                    graph,
                    node_idx,
                    node_to_community[node_idx],
                    node_to_community,
                );
                community_weights[node_to_community[node_idx]] += node_weight;
                community_internal[node_to_community[node_idx]] += ki_in_new;
            }
        }

        improved
    }

    /// 노드와 특정 커뮤니티 사이의 연결 가중치 합 계산
    fn calculate_ki_in(
        &self,
        graph: &LouvainGraph,
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
        graph: &LouvainGraph,
        node_to_community: &[usize],
        community_weights: &[f64],
        community_internal: &[f64],
    ) -> f64 {
        let m2 = graph.total_weight;
        if m2 == 0.0 {
            return 0.0;
        }

        let mut q = 0.0;

        // 각 커뮤니티에 대해
        let max_comm = node_to_community.iter().copied().max().unwrap_or(0);
        for comm in 0..=max_comm {
            if community_weights[comm] > 0.0 {
                let sigma_tot = community_weights[comm];
                let sigma_in = community_internal[comm];

                // Q += sigma_in / 2m - resolution * (sigma_tot / 2m)²
                q += sigma_in / m2 - self.config.resolution * (sigma_tot / m2).powi(2);
            }
        }

        q
    }

    /// GraphEngine에서 Louvain용 그래프 구조 빌드
    async fn build_graph(&self, rel_types: Option<&[String]>) -> Result<LouvainGraph> {
        let mut graph = LouvainGraph::new();

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

                // 두 노드 모두 유효한지 확인
                if let (Some(&from_idx), Some(&to_idx)) =
                    (graph.node_to_idx.get(&from), graph.node_to_idx.get(&to))
                {
                    // Read edge weight property if present, otherwise default to 1.0
                    let weight = edge
                        .get_property("weight")
                        .and_then(|v| match v {
                            crate::core::Value::Float(f) => Some(*f),
                            crate::core::Value::Integer(i) => Some(*i as f64),
                            _ => None,
                        })
                        .unwrap_or(1.0);

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

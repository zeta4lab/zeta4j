//! 병렬 Louvain 커뮤니티 탐지 알고리즘
//!
//! Rayon을 사용하여 Louvain 알고리즘을 병렬화합니다.
//! Phase 1 (Local Moving)에서 모듈러리티 델타 계산을 병렬로 실행합니다.

use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

use super::config::ParallelConfig;
use crate::algorithm::community::{LouvainConfig, LouvainResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 Louvain 계산기
pub struct ParallelLouvain<'a> {
    engine: &'a GraphEngine,
    config: LouvainConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelLouvain<'a> {
    /// 새로운 병렬 Louvain 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: LouvainConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: LouvainConfig) -> Self {
        Self {
            engine,
            config,
            par_config: ParallelConfig::default(),
        }
    }

    /// 병렬 설정 지정
    pub fn with_parallel_config(mut self, par_config: ParallelConfig) -> Self {
        self.par_config = par_config;
        self
    }

    /// 병렬 Louvain 커뮤니티 탐지 실행
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<LouvainResult> {
        louvain_parallel(self.engine, &self.config, &self.par_config, rel_types).await
    }
}

/// 내부 그래프 표현
struct LouvainGraph {
    nodes: Vec<NodeId>,
    node_to_idx: HashMap<NodeId, usize>,
    adjacency: Vec<Vec<(usize, f64)>>,
    node_weights: Vec<f64>,
    total_weight: f64,
}

impl LouvainGraph {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_to_idx: HashMap::new(),
            adjacency: Vec::new(),
            node_weights: Vec::new(),
            total_weight: 0.0,
        }
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

/// 병렬 Louvain 커뮤니티 탐지
pub async fn louvain_parallel(
    engine: &GraphEngine,
    config: &LouvainConfig,
    par_config: &ParallelConfig,
    rel_types: Option<&[String]>,
) -> Result<LouvainResult> {
    // 1. 그래프 구조 빌드
    let graph = build_graph(engine, rel_types).await?;

    if graph.node_count() == 0 {
        return Ok(LouvainResult {
            communities: HashMap::new(),
            community_count: 0,
            modularity: 0.0,
            iterations: 0,
            community_sizes: HashMap::new(),
        });
    }

    let n = graph.node_count();

    // 2. 초기 커뮤니티 할당 (RwLock으로 공유)
    let node_to_community: RwLock<Vec<usize>> = RwLock::new((0..n).collect());
    let community_weights: RwLock<Vec<f64>> = RwLock::new(graph.node_weights.clone());
    let community_internal: RwLock<Vec<f64>> = RwLock::new(vec![0.0; n]);

    // 자기 루프 처리
    {
        let mut internal = community_internal.write();
        for (node_idx, neighbors) in graph.adjacency.iter().enumerate() {
            for &(neighbor_idx, weight) in neighbors {
                if neighbor_idx == node_idx {
                    internal[node_idx] += weight;
                }
            }
        }
    }

    let mut iterations = 0;
    let mut improved = true;

    // 3. 반복 최적화
    while improved && iterations < config.max_iterations {
        improved = false;
        iterations += 1;

        // Phase 1: Local Moving (병렬)
        let phase1_improved = parallel_local_moving(
            &graph,
            &node_to_community,
            &community_weights,
            &community_internal,
            config,
            par_config,
        );

        if phase1_improved {
            improved = true;
        }
    }

    // 4. 결과 정리
    let node_comm = node_to_community.read();
    let comm_weights = community_weights.read();
    let comm_internal = community_internal.read();

    let mut communities: HashMap<NodeId, usize> = HashMap::new();
    let mut community_remap: HashMap<usize, usize> = HashMap::new();
    let mut next_community_id = 0;

    for (node_idx, &comm) in node_comm.iter().enumerate() {
        let remapped = *community_remap.entry(comm).or_insert_with(|| {
            let id = next_community_id;
            next_community_id += 1;
            id
        });
        communities.insert(graph.nodes[node_idx], remapped);
    }

    let mut community_sizes: HashMap<usize, usize> = HashMap::new();
    for &comm in communities.values() {
        *community_sizes.entry(comm).or_insert(0) += 1;
    }

    let modularity = calculate_modularity(
        &graph,
        &node_comm,
        &comm_weights,
        &comm_internal,
        config.resolution,
    );

    Ok(LouvainResult {
        communities,
        community_count: next_community_id,
        modularity,
        iterations,
        community_sizes,
    })
}

/// 병렬 로컬 이동 최적화
fn parallel_local_moving(
    graph: &LouvainGraph,
    node_to_community: &RwLock<Vec<usize>>,
    community_weights: &RwLock<Vec<f64>>,
    community_internal: &RwLock<Vec<f64>>,
    config: &LouvainConfig,
    par_config: &ParallelConfig,
) -> bool {
    let m2 = graph.total_weight;
    if m2 == 0.0 {
        return false;
    }

    let n = graph.node_count();
    let mut improved = false;
    let mut changed = true;
    let mut inner_iter = 0;
    let max_inner_iter = config.max_iterations.max(n * 2);

    while changed && inner_iter < max_inner_iter {
        changed = false;
        inner_iter += 1;

        // 각 노드에 대해 최적 커뮤니티 계산 (병렬)
        let moves: Vec<(usize, usize)> = if par_config.should_parallelize(n) {
            (0..n)
                .into_par_iter()
                .filter_map(|node_idx| {
                    calculate_best_move(
                        node_idx,
                        graph,
                        node_to_community,
                        community_weights,
                        m2,
                        config.resolution,
                    )
                })
                .collect()
        } else {
            (0..n)
                .filter_map(|node_idx| {
                    calculate_best_move(
                        node_idx,
                        graph,
                        node_to_community,
                        community_weights,
                        m2,
                        config.resolution,
                    )
                })
                .collect()
        };

        // 이동 적용 (순차적으로 - 동기화 필요)
        if !moves.is_empty() {
            changed = true;
            improved = true;

            let mut node_comm = node_to_community.write();
            let mut comm_weights = community_weights.write();
            let mut comm_internal = community_internal.write();

            for (node_idx, new_comm) in moves {
                let old_comm = node_comm[node_idx];
                if old_comm == new_comm {
                    continue;
                }

                let node_weight = graph.node_weights[node_idx];

                // 이전 커뮤니티에서 제거
                let ki_in_old = calculate_ki_in(node_idx, old_comm, graph, &node_comm);
                comm_weights[old_comm] -= node_weight;
                comm_internal[old_comm] -= ki_in_old;

                // 새 커뮤니티로 이동
                node_comm[node_idx] = new_comm;

                // 새 커뮤니티에 추가
                let ki_in_new = calculate_ki_in(node_idx, new_comm, graph, &node_comm);
                comm_weights[new_comm] += node_weight;
                comm_internal[new_comm] += ki_in_new;
            }
        }
    }

    improved
}

/// 노드의 최적 이동 계산
fn calculate_best_move(
    node_idx: usize,
    graph: &LouvainGraph,
    node_to_community: &RwLock<Vec<usize>>,
    community_weights: &RwLock<Vec<f64>>,
    m2: f64,
    resolution: f64,
) -> Option<(usize, usize)> {
    let node_comm = node_to_community.read();
    let comm_weights = community_weights.read();

    let current_comm = node_comm[node_idx];
    let node_weight = graph.node_weights[node_idx];

    // 이웃 커뮤니티 수집
    let mut neighbor_communities: HashSet<usize> = HashSet::new();
    for &(neighbor_idx, _) in &graph.adjacency[node_idx] {
        if neighbor_idx != node_idx {
            neighbor_communities.insert(node_comm[neighbor_idx]);
        }
    }
    neighbor_communities.insert(current_comm);

    // 최적 커뮤니티 찾기
    let mut best_comm = current_comm;
    let mut best_delta = 0.0;

    for &target_comm in &neighbor_communities {
        let ki_in = calculate_ki_in(node_idx, target_comm, graph, &node_comm);
        let sigma_tot = comm_weights[target_comm];

        // 현재 커뮤니티가 아닌 경우 노드 가중치 제외
        let adjusted_sigma = if target_comm == current_comm {
            sigma_tot - node_weight
        } else {
            sigma_tot
        };

        let delta = ki_in / m2 - resolution * (adjusted_sigma * node_weight) / (m2 * m2) * 2.0;

        if delta > best_delta + 1e-10 {
            best_delta = delta;
            best_comm = target_comm;
        }
    }

    if best_comm != current_comm {
        Some((node_idx, best_comm))
    } else {
        None
    }
}

/// 노드와 커뮤니티 사이의 연결 가중치
fn calculate_ki_in(
    node_idx: usize,
    community: usize,
    graph: &LouvainGraph,
    node_to_community: &[usize],
) -> f64 {
    graph.adjacency[node_idx]
        .iter()
        .filter(|&&(neighbor_idx, _)| node_to_community[neighbor_idx] == community)
        .map(|&(_, weight)| weight)
        .sum()
}

/// 모듈러리티 계산
fn calculate_modularity(
    graph: &LouvainGraph,
    node_to_community: &[usize],
    community_weights: &[f64],
    community_internal: &[f64],
    resolution: f64,
) -> f64 {
    let m2 = graph.total_weight;
    if m2 == 0.0 {
        return 0.0;
    }

    let mut q = 0.0;
    let max_comm = node_to_community.iter().copied().max().unwrap_or(0);

    for comm in 0..=max_comm {
        if community_weights[comm] > 0.0 {
            let sigma_tot = community_weights[comm];
            let sigma_in = community_internal[comm];
            q += sigma_in / m2 - resolution * (sigma_tot / m2).powi(2);
        }
    }

    q
}

/// 그래프 구조 빌드
async fn build_graph(engine: &GraphEngine, rel_types: Option<&[String]>) -> Result<LouvainGraph> {
    let mut graph = LouvainGraph::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
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

    // 엣지 수집
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
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

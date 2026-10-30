//! 병렬 Closeness/Harmonic Centrality 알고리즘
//!
//! Rayon을 사용하여 Closeness 및 Harmonic Centrality 알고리즘을 병렬화합니다.
//! 각 노드에서의 BFS를 병렬로 실행하여 성능을 향상시킵니다.

use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

use super::config::ParallelConfig;
use crate::algorithm::centrality::{ClosenessConfig, ClosenessResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;

/// 병렬 Closeness Centrality 계산기
pub struct ParallelCloseness<'a> {
    engine: &'a GraphEngine,
    config: ClosenessConfig,
    par_config: ParallelConfig,
}

impl<'a> ParallelCloseness<'a> {
    /// 새로운 병렬 Closeness 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ClosenessConfig::default(),
            par_config: ParallelConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ClosenessConfig) -> Self {
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

    /// 병렬 Closeness Centrality 계산
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<ClosenessResult> {
        closeness_parallel(self.engine, &self.config, &self.par_config, rel_types).await
    }
}

/// 병렬 Harmonic Centrality 계산기
pub struct ParallelHarmonic<'a> {
    engine: &'a GraphEngine,
    normalized: bool,
    directed: bool,
    par_config: ParallelConfig,
}

impl<'a> ParallelHarmonic<'a> {
    /// 새로운 병렬 Harmonic Centrality 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            normalized: true,
            directed: false,
            par_config: ParallelConfig::default(),
        }
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

    /// 병렬 설정 지정
    pub fn with_parallel_config(mut self, par_config: ParallelConfig) -> Self {
        self.par_config = par_config;
        self
    }

    /// 병렬 Harmonic Centrality 계산
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<ClosenessResult> {
        harmonic_parallel(
            self.engine,
            self.normalized,
            self.directed,
            &self.par_config,
            rel_types,
        )
        .await
    }
}

/// 병렬 Closeness Centrality 계산 함수
///
/// # Arguments
/// * `engine` - 그래프 엔진
/// * `config` - Closeness 설정
/// * `par_config` - 병렬 처리 설정
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// Closeness Centrality 결과
pub async fn closeness_parallel(
    engine: &GraphEngine,
    config: &ClosenessConfig,
    par_config: &ParallelConfig,
    rel_types: Option<&[String]>,
) -> Result<ClosenessResult> {
    // 1. 그래프 구조 빌드
    let (nodes, adjacency) = build_graph_structure(engine, config.directed, rel_types).await?;

    if nodes.is_empty() {
        return Ok(ClosenessResult {
            scores: HashMap::new(),
        });
    }

    let n = nodes.len();

    // 2. 각 노드에서 Closeness 계산 (병렬)
    let scores: HashMap<NodeId, f64> = if par_config.should_parallelize(n) {
        nodes
            .par_iter()
            .map(|&source| {
                let (sum_distances, reachable_count) = bfs_distances(source, &adjacency);
                let score = calculate_closeness_score(sum_distances, reachable_count, n, config);
                (source, score)
            })
            .collect()
    } else {
        nodes
            .iter()
            .map(|&source| {
                let (sum_distances, reachable_count) = bfs_distances(source, &adjacency);
                let score = calculate_closeness_score(sum_distances, reachable_count, n, config);
                (source, score)
            })
            .collect()
    };

    Ok(ClosenessResult { scores })
}

/// 병렬 Harmonic Centrality 계산 함수
///
/// # Arguments
/// * `engine` - 그래프 엔진
/// * `normalized` - 정규화 여부
/// * `directed` - 방향성 여부
/// * `par_config` - 병렬 처리 설정
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// Harmonic Centrality 결과
pub async fn harmonic_parallel(
    engine: &GraphEngine,
    normalized: bool,
    directed: bool,
    par_config: &ParallelConfig,
    rel_types: Option<&[String]>,
) -> Result<ClosenessResult> {
    // 1. 그래프 구조 빌드
    let (nodes, adjacency) = build_graph_structure(engine, directed, rel_types).await?;

    if nodes.is_empty() {
        return Ok(ClosenessResult {
            scores: HashMap::new(),
        });
    }

    let n = nodes.len();

    // 2. 각 노드에서 Harmonic 계산 (병렬)
    let scores: HashMap<NodeId, f64> = if par_config.should_parallelize(n) {
        nodes
            .par_iter()
            .map(|&source| {
                let harmonic_sum = bfs_harmonic_sum(source, &adjacency);
                let score = if normalized && n > 1 {
                    harmonic_sum / (n - 1) as f64
                } else {
                    harmonic_sum
                };
                (source, score)
            })
            .collect()
    } else {
        nodes
            .iter()
            .map(|&source| {
                let harmonic_sum = bfs_harmonic_sum(source, &adjacency);
                let score = if normalized && n > 1 {
                    harmonic_sum / (n - 1) as f64
                } else {
                    harmonic_sum
                };
                (source, score)
            })
            .collect()
    };

    Ok(ClosenessResult { scores })
}

/// Closeness 점수 계산
fn calculate_closeness_score(
    sum_distances: u64,
    reachable_count: usize,
    n: usize,
    config: &ClosenessConfig,
) -> f64 {
    if reachable_count <= 1 || sum_distances == 0 {
        // 다른 노드에 도달할 수 없거나 자기 자신만 있는 경우
        0.0
    } else if config.wasserman_faust && config.normalized {
        // Wasserman-Faust 정규화: 비연결 그래프에서도 의미있는 값 제공
        let reachable_ratio = (reachable_count - 1) as f64 / (n - 1) as f64;
        let avg_distance = (reachable_count - 1) as f64 / sum_distances as f64;
        reachable_ratio * avg_distance
    } else if config.normalized {
        // 일반 정규화: (n - 1) / sum_distances
        (n - 1) as f64 / sum_distances as f64
    } else {
        // 비정규화: 1 / sum_distances
        1.0 / sum_distances as f64
    }
}

/// BFS로 소스 노드에서 모든 노드까지의 거리 합과 도달 가능한 노드 수 계산
fn bfs_distances(source: NodeId, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> (u64, usize) {
    let mut distance: HashMap<NodeId, u64> = HashMap::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();

    distance.insert(source, 0);
    queue.push_back(source);

    let mut sum_distances: u64 = 0;
    let mut reachable_count: usize = 1; // 자기 자신 포함

    while let Some(current) = queue.pop_front() {
        let current_dist = distance.get(&current).copied().unwrap_or(0);

        if let Some(neighbors) = adjacency.get(&current) {
            for &neighbor in neighbors {
                if let std::collections::hash_map::Entry::Vacant(e) = distance.entry(neighbor) {
                    let new_dist = current_dist + 1;
                    e.insert(new_dist);
                    sum_distances += new_dist;
                    reachable_count += 1;
                    queue.push_back(neighbor);
                }
            }
        }
    }

    (sum_distances, reachable_count)
}

/// BFS로 harmonic sum 계산: Σ 1/d(v, u)
fn bfs_harmonic_sum(source: NodeId, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> f64 {
    let mut distance: HashMap<NodeId, u64> = HashMap::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();

    distance.insert(source, 0);
    queue.push_back(source);

    let mut harmonic_sum: f64 = 0.0;

    while let Some(current) = queue.pop_front() {
        let current_dist = distance.get(&current).copied().unwrap_or(0);

        if let Some(neighbors) = adjacency.get(&current) {
            for &neighbor in neighbors {
                if let std::collections::hash_map::Entry::Vacant(e) = distance.entry(neighbor) {
                    let new_dist = current_dist + 1;
                    e.insert(new_dist);
                    harmonic_sum += 1.0 / new_dist as f64;
                    queue.push_back(neighbor);
                }
            }
        }
    }

    harmonic_sum
}

/// 그래프 구조 빌드
async fn build_graph_structure(
    engine: &GraphEngine,
    directed: bool,
    rel_types: Option<&[String]>,
) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    // 노드 수집
    let node_count = engine.node_count().await?;
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
            adjacency.insert(node_id, Vec::new());
        }
    }

    // 엣지 수집
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(types) = rel_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            let from = edge.from_node();
            let to = edge.to_node();

            adjacency.entry(from).or_default().push(to);

            if !directed {
                adjacency.entry(to).or_default().push(from);
            }
        }
    }

    Ok((nodes, adjacency))
}

//! Closeness Centrality 알고리즘
//!
//! 노드가 그래프의 다른 모든 노드에 얼마나 가까운지를 측정합니다.
//! Closeness(v) = (n-1) / Σ d(v, u) for all u ≠ v
//!
//! 비연결 그래프의 경우 Wasserman-Faust 정규화 또는 Harmonic Centrality를 사용합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, VecDeque};

/// Closeness Centrality 설정
#[derive(Debug, Clone)]
pub struct ClosenessConfig {
    /// 정규화 여부 (true면 0~1 범위로 정규화)
    pub normalized: bool,
    /// Wasserman-Faust 정규화 사용 (비연결 그래프용)
    /// true면: (reachable_count - 1) / (n - 1) * (reachable_count - 1) / sum_distances
    pub wasserman_faust: bool,
    /// 방향성 고려 여부
    pub directed: bool,
}

impl Default for ClosenessConfig {
    fn default() -> Self {
        Self {
            normalized: true,
            wasserman_faust: true,
            directed: false,
        }
    }
}

impl ClosenessConfig {
    /// 정규화 설정
    pub fn with_normalized(mut self, normalized: bool) -> Self {
        self.normalized = normalized;
        self
    }

    /// Wasserman-Faust 정규화 설정
    pub fn with_wasserman_faust(mut self, wasserman_faust: bool) -> Self {
        self.wasserman_faust = wasserman_faust;
        self
    }

    /// 방향성 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }
}

/// Closeness Centrality 결과
#[derive(Debug, Clone)]
pub struct ClosenessResult {
    /// 노드별 Closeness Centrality 점수
    pub scores: HashMap<NodeId, f64>,
}

impl ClosenessResult {
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

/// Closeness Centrality 계산기
pub struct ClosenessCentrality<'a> {
    engine: &'a GraphEngine,
    config: ClosenessConfig,
}

impl<'a> ClosenessCentrality<'a> {
    /// 새로운 Closeness Centrality 계산기 생성 (기본 설정)
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: ClosenessConfig::default(),
        }
    }

    /// 설정과 함께 계산기 생성
    pub fn with_config(engine: &'a GraphEngine, config: ClosenessConfig) -> Self {
        Self { engine, config }
    }

    /// 전체 그래프에 대해 Closeness Centrality 계산
    ///
    /// # Arguments
    /// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<ClosenessResult> {
        // 1. 그래프 구조 수집
        let (nodes, adjacency) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(ClosenessResult {
                scores: HashMap::new(),
            });
        }

        let n = nodes.len();
        let mut scores: HashMap<NodeId, f64> = HashMap::new();

        // 2. 각 노드에 대해 Closeness 계산
        for &source in &nodes {
            let (sum_distances, reachable_count) = self.bfs_distances(&source, &nodes, &adjacency);

            let closeness = if reachable_count <= 1 || sum_distances == 0 {
                // 다른 노드에 도달할 수 없거나 자기 자신만 있는 경우
                0.0
            } else if self.config.wasserman_faust && self.config.normalized {
                // Wasserman-Faust 정규화: 비연결 그래프에서도 의미있는 값 제공
                // C_WF(v) = (reachable - 1) / (n - 1) * (reachable - 1) / sum_distances
                let reachable_ratio = (reachable_count - 1) as f64 / (n - 1) as f64;
                let avg_distance = (reachable_count - 1) as f64 / sum_distances as f64;
                reachable_ratio * avg_distance
            } else if self.config.normalized {
                // 일반 정규화: (n - 1) / sum_distances
                (n - 1) as f64 / sum_distances as f64
            } else {
                // 비정규화: 1 / sum_distances
                1.0 / sum_distances as f64
            };

            scores.insert(source, closeness);
        }

        Ok(ClosenessResult { scores })
    }

    /// 단일 노드의 Closeness Centrality 점수 계산
    pub async fn calculate_single(
        &self,
        node_id: NodeId,
        rel_types: Option<&[String]>,
    ) -> Result<f64> {
        let result = self.calculate(rel_types).await?;
        Ok(result.score(node_id).unwrap_or(0.0))
    }

    /// BFS로 소스 노드에서 모든 노드까지의 거리 합과 도달 가능한 노드 수 계산
    fn bfs_distances(
        &self,
        source: &NodeId,
        _nodes: &[NodeId],
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
    ) -> (u64, usize) {
        let mut distance: HashMap<NodeId, u64> = HashMap::new();
        let mut queue: VecDeque<NodeId> = VecDeque::new();

        // 소스 노드 초기화
        distance.insert(*source, 0);
        queue.push_back(*source);

        let mut sum_distances: u64 = 0;
        let mut reachable_count: usize = 1; // 자기 자신 포함

        // BFS 탐색
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

                // 방향성 그래프: from -> to만 추가
                adjacency.entry(from).or_default().push(to);

                // 무방향 그래프: 양방향 추가
                if !self.config.directed {
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok((nodes, adjacency))
    }
}

/// Harmonic Centrality 계산기
///
/// 비연결 그래프에서 더 나은 중심성 측정을 제공합니다.
/// H(v) = Σ 1/d(v, u) for all u ≠ v (도달 불가능하면 0)
pub struct HarmonicCentrality<'a> {
    engine: &'a GraphEngine,
    normalized: bool,
    directed: bool,
}

impl<'a> HarmonicCentrality<'a> {
    /// 새로운 Harmonic Centrality 계산기 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            normalized: true,
            directed: false,
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

    /// Harmonic Centrality 계산
    pub async fn calculate(&self, rel_types: Option<&[String]>) -> Result<ClosenessResult> {
        let (nodes, adjacency) = self.build_graph_structure(rel_types).await?;

        if nodes.is_empty() {
            return Ok(ClosenessResult {
                scores: HashMap::new(),
            });
        }

        let n = nodes.len();
        let mut scores: HashMap<NodeId, f64> = HashMap::new();

        for &source in &nodes {
            let harmonic_sum = self.bfs_harmonic_sum(&source, &adjacency);

            let harmonic = if self.normalized && n > 1 {
                harmonic_sum / (n - 1) as f64
            } else {
                harmonic_sum
            };

            scores.insert(source, harmonic);
        }

        Ok(ClosenessResult { scores })
    }

    /// BFS로 harmonic sum 계산: Σ 1/d(v, u)
    fn bfs_harmonic_sum(&self, source: &NodeId, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> f64 {
        let mut distance: HashMap<NodeId, u64> = HashMap::new();
        let mut queue: VecDeque<NodeId> = VecDeque::new();

        distance.insert(*source, 0);
        queue.push_back(*source);

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
        &self,
        rel_types: Option<&[String]>,
    ) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<NodeId>>)> {
        let mut nodes: Vec<NodeId> = Vec::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adjacency.insert(node_id, Vec::new());
            }
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                if let Some(types) = rel_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                adjacency.entry(from).or_default().push(to);

                if !self.directed {
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok((nodes, adjacency))
    }
}

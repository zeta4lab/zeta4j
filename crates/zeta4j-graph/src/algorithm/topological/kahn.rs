//! Kahn's Topological Sort 알고리즘
//!
//! BFS 기반 위상 정렬 알고리즘입니다.
//! 진입차수(in-degree)가 0인 노드부터 시작하여 순서대로 제거합니다.
//!
//! ## 시간복잡도
//! - O(V + E)
//!
//! ## 특징
//! - 레벨별 처리 가능 (병렬화에 유리)
//! - 사이클 탐지 포함
//! - 모든 위상 순서 중 하나 반환

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Topological Sort 알고리즘 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologicalAlgorithm {
    Kahn,
    Dfs,
}

/// Topological Sort 설정
#[derive(Debug, Clone, Default)]
pub struct TopologicalConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 레벨 정보 포함 여부
    pub include_levels: bool,
}

impl TopologicalConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 레벨 정보 포함
    pub fn with_levels(mut self) -> Self {
        self.include_levels = true;
        self
    }
}

/// Topological Sort 결과
#[derive(Debug, Clone)]
pub struct TopologicalResult {
    /// 정렬된 노드 순서
    pub order: Vec<NodeId>,
    /// DAG 여부 (사이클 없음)
    pub is_dag: bool,
    /// 레벨별 노드 (옵션)
    pub levels: Option<Vec<Vec<NodeId>>>,
    /// 사용된 알고리즘
    pub algorithm: TopologicalAlgorithm,
    /// 처리된 노드 수
    pub processed_count: usize,
    /// 전체 노드 수
    pub total_nodes: usize,
}

impl TopologicalResult {
    /// 빈 결과 생성
    pub fn empty(algorithm: TopologicalAlgorithm) -> Self {
        Self {
            order: Vec::new(),
            is_dag: true,
            levels: None,
            algorithm,
            processed_count: 0,
            total_nodes: 0,
        }
    }

    /// 위상 정렬이 완전한지 (모든 노드 포함)
    pub fn is_complete(&self) -> bool {
        self.processed_count == self.total_nodes
    }

    /// 처리되지 않은 노드 수 (사이클에 포함된 노드)
    pub fn unprocessed_count(&self) -> usize {
        self.total_nodes.saturating_sub(self.processed_count)
    }

    /// 레벨 수
    pub fn level_count(&self) -> usize {
        self.levels.as_ref().map(|l| l.len()).unwrap_or(0)
    }

    /// 특정 레벨의 노드들
    pub fn nodes_at_level(&self, level: usize) -> Option<&Vec<NodeId>> {
        self.levels.as_ref().and_then(|l| l.get(level))
    }

    /// 노드의 레벨 찾기
    pub fn node_level(&self, node: NodeId) -> Option<usize> {
        self.levels
            .as_ref()
            .and_then(|levels| levels.iter().position(|level| level.contains(&node)))
    }
}

/// Kahn's Topological Sort
pub struct TopologicalSort<'a> {
    engine: &'a GraphEngine,
    config: TopologicalConfig,
}

impl<'a> TopologicalSort<'a> {
    /// 새로운 TopologicalSort 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: TopologicalConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: TopologicalConfig) -> Self {
        Self { engine, config }
    }

    /// Kahn's 알고리즘으로 위상 정렬
    pub async fn sort(&self) -> Result<TopologicalResult> {
        let (nodes, adj, in_degree) = self.build_graph_info().await?;

        if nodes.is_empty() {
            return Ok(TopologicalResult::empty(TopologicalAlgorithm::Kahn));
        }

        let total_nodes = nodes.len();
        let mut in_degree = in_degree;
        let mut queue: VecDeque<NodeId> = VecDeque::new();
        let mut order = Vec::new();
        let mut levels: Vec<Vec<NodeId>> = Vec::new();

        // 진입차수가 0인 노드들 큐에 추가
        for &node in &nodes {
            if *in_degree.get(&node).unwrap_or(&0) == 0 {
                queue.push_back(node);
            }
        }

        // 레벨별 처리
        while !queue.is_empty() {
            let level_size = queue.len();
            let mut current_level = Vec::new();

            for _ in 0..level_size {
                let node = match queue.pop_front() {
                    Some(n) => n,
                    None => break,
                };
                order.push(node);
                current_level.push(node);

                // 이웃 노드의 진입차수 감소
                if let Some(neighbors) = adj.get(&node) {
                    for &neighbor in neighbors {
                        if let Some(degree) = in_degree.get_mut(&neighbor) {
                            *degree -= 1;
                            if *degree == 0 {
                                queue.push_back(neighbor);
                            }
                        }
                    }
                }
            }

            if self.config.include_levels {
                levels.push(current_level);
            }
        }

        let order_len = order.len();
        let is_dag = order_len == total_nodes;

        Ok(TopologicalResult {
            order,
            is_dag,
            levels: if self.config.include_levels {
                Some(levels)
            } else {
                None
            },
            algorithm: TopologicalAlgorithm::Kahn,
            processed_count: total_nodes.min(order_len),
            total_nodes,
        })
    }

    /// 시작 노드들 (진입차수 0)
    pub async fn sources(&self) -> Result<Vec<NodeId>> {
        let (nodes, _, in_degree) = self.build_graph_info().await?;

        let sources: Vec<NodeId> = nodes
            .into_iter()
            .filter(|n| *in_degree.get(n).unwrap_or(&0) == 0)
            .collect();

        Ok(sources)
    }

    /// 종료 노드들 (진출차수 0)
    pub async fn sinks(&self) -> Result<Vec<NodeId>> {
        let (nodes, adj, _) = self.build_graph_info().await?;

        let sinks: Vec<NodeId> = nodes
            .into_iter()
            .filter(|n| adj.get(n).map(|v| v.is_empty()).unwrap_or(true))
            .collect();

        Ok(sinks)
    }

    /// 레벨별 노드 그룹화
    pub async fn levels(&self) -> Result<Vec<Vec<NodeId>>> {
        let mut config = self.config.clone();
        config.include_levels = true;

        let sorter = TopologicalSort::with_config(self.engine, config);
        let result = sorter.sort().await?;

        Ok(result.levels.unwrap_or_default())
    }

    /// 진입차수 맵 반환
    pub async fn in_degrees(&self) -> Result<HashMap<NodeId, usize>> {
        let (_, _, in_degree) = self.build_graph_info().await?;
        Ok(in_degree)
    }

    /// 진출차수 맵 반환
    pub async fn out_degrees(&self) -> Result<HashMap<NodeId, usize>> {
        let (nodes, adj, _) = self.build_graph_info().await?;

        let out_degree: HashMap<NodeId, usize> = nodes
            .into_iter()
            .map(|n| (n, adj.get(&n).map(|v| v.len()).unwrap_or(0)))
            .collect();

        Ok(out_degree)
    }

    /// 그래프 정보 구축 (노드, 인접리스트, 진입차수)
    async fn build_graph_info(
        &self,
    ) -> Result<(
        Vec<NodeId>,
        HashMap<NodeId, Vec<NodeId>>,
        HashMap<NodeId, usize>,
    )> {
        let mut nodes = Vec::new();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();

        // 모든 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adj.insert(node_id, Vec::new());
                in_degree.insert(node_id, 0);
            }
        }

        // 모든 간선 수집 (방향 그래프)
        let edge_count = self.engine.edge_count().await;
        let mut seen_edges = HashSet::new();

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

                if from == to {
                    continue;
                }

                // 중복 간선 방지
                let edge_key = (from, to);
                if seen_edges.contains(&edge_key) {
                    continue;
                }
                seen_edges.insert(edge_key);

                if adj.contains_key(&from) && adj.contains_key(&to) {
                    adj.entry(from).or_default().push(to);
                    *in_degree.entry(to).or_default() += 1;
                }
            }
        }

        Ok((nodes, adj, in_degree))
    }
}

/// 위상 정렬 편의 함수
pub async fn topological_sort(engine: &GraphEngine) -> Result<TopologicalResult> {
    TopologicalSort::new(engine).sort().await
}

/// 위상 정렬 (레벨 포함) 편의 함수
pub async fn topological_sort_with_levels(engine: &GraphEngine) -> Result<TopologicalResult> {
    TopologicalSort::with_config(engine, TopologicalConfig::default().with_levels())
        .sort()
        .await
}

/// 시작 노드 편의 함수
pub async fn dag_sources(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    TopologicalSort::new(engine).sources().await
}

/// 종료 노드 편의 함수
pub async fn dag_sinks(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    TopologicalSort::new(engine).sinks().await
}

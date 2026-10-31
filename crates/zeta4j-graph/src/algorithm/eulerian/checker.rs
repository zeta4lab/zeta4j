// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Eulerian Path/Circuit 존재 여부 판별
//!
//! 오일러 회로: 모든 엣지를 한 번씩 방문하고 시작점으로 돌아옴
//! 오일러 경로: 모든 엣지를 한 번씩 방문 (시작점 != 끝점 가능)
//!
//! 시간복잡도: O(V + E)

use super::EulerianConfig;
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Eulerian 판별 결과
#[derive(Debug, Clone)]
pub struct EulerianCheckResult {
    /// 오일러 회로 존재 여부
    pub has_eulerian_circuit: bool,
    /// 오일러 경로 존재 여부
    pub has_eulerian_path: bool,
    /// 경로 시작점 (홀수 차수 정점)
    pub path_start: Option<NodeId>,
    /// 경로 끝점 (홀수 차수 정점)
    pub path_end: Option<NodeId>,
    /// 홀수 차수 정점들
    pub odd_degree_nodes: Vec<NodeId>,
    /// 총 엣지 수
    pub edge_count: usize,
    /// 엣지가 있는 정점 수
    pub node_count: usize,
}

impl EulerianCheckResult {
    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            has_eulerian_circuit: true, // 빈 그래프는 자명하게 오일러 회로
            has_eulerian_path: true,
            path_start: None,
            path_end: None,
            odd_degree_nodes: Vec::new(),
            edge_count: 0,
            node_count: 0,
        }
    }
}

/// Eulerian 판별기
pub struct EulerianChecker<'a> {
    engine: &'a GraphEngine,
    config: EulerianConfig,
}

impl<'a> EulerianChecker<'a> {
    /// 새로운 EulerianChecker 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: EulerianConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: EulerianConfig) -> Self {
        Self { engine, config }
    }

    /// Eulerian 조건 확인
    pub async fn check(&self) -> Result<EulerianCheckResult> {
        if self.config.directed {
            self.check_directed().await
        } else {
            self.check_undirected().await
        }
    }

    /// 무방향 그래프 Eulerian 조건 확인
    async fn check_undirected(&self) -> Result<EulerianCheckResult> {
        let adjacency = self.build_undirected_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(EulerianCheckResult::empty());
        }

        // 엣지가 있는 정점들만 고려
        let nodes_with_edges: Vec<NodeId> = adjacency
            .iter()
            .filter(|(_, neighbors)| !neighbors.is_empty())
            .map(|(&node, _)| node)
            .collect();

        if nodes_with_edges.is_empty() {
            return Ok(EulerianCheckResult::empty());
        }

        // 연결성 확인
        let is_connected = self.is_connected_undirected(&adjacency, &nodes_with_edges);

        // 홀수 차수 정점 찾기
        let mut odd_degree_nodes = Vec::new();
        for &node in &nodes_with_edges {
            if let Some(neighbors) = adjacency.get(&node)
                && neighbors.len() % 2 == 1
            {
                odd_degree_nodes.push(node);
            }
        }
        odd_degree_nodes.sort();

        // 총 엣지 수 계산 (무방향이므로 /2)
        let total_degree: usize = adjacency.values().map(|n| n.len()).sum();
        let edge_count = total_degree / 2;

        // Eulerian 조건 판별
        let (has_circuit, has_path, start, end) = if !is_connected {
            (false, false, None, None)
        } else {
            match odd_degree_nodes.len() {
                0 => (
                    true,
                    true,
                    Some(nodes_with_edges[0]),
                    Some(nodes_with_edges[0]),
                ),
                2 => (
                    false,
                    true,
                    Some(odd_degree_nodes[0]),
                    Some(odd_degree_nodes[1]),
                ),
                _ => (false, false, None, None),
            }
        };

        Ok(EulerianCheckResult {
            has_eulerian_circuit: has_circuit,
            has_eulerian_path: has_path,
            path_start: start,
            path_end: end,
            odd_degree_nodes,
            edge_count,
            node_count: nodes_with_edges.len(),
        })
    }

    /// 방향 그래프 Eulerian 조건 확인
    async fn check_directed(&self) -> Result<EulerianCheckResult> {
        let (out_adj, in_degree) = self.build_directed_adjacency().await?;

        if out_adj.is_empty() {
            return Ok(EulerianCheckResult::empty());
        }

        // 모든 노드 수집
        let mut all_nodes: HashSet<NodeId> = HashSet::new();
        for (&node, neighbors) in &out_adj {
            if !neighbors.is_empty() {
                all_nodes.insert(node);
            }
            for &neighbor in neighbors {
                all_nodes.insert(neighbor);
            }
        }

        let nodes_with_edges: Vec<NodeId> = all_nodes.iter().copied().collect();

        if nodes_with_edges.is_empty() {
            return Ok(EulerianCheckResult::empty());
        }

        // 약한 연결성 확인
        let is_connected = self.is_weakly_connected(&out_adj, &nodes_with_edges);

        // in-degree와 out-degree 차이 확인
        let mut start_node: Option<NodeId> = None;
        let mut end_node: Option<NodeId> = None;
        let mut odd_nodes = Vec::new();
        let mut valid_for_path = true;
        let mut valid_for_circuit = true;

        for &node in &nodes_with_edges {
            let out_deg = out_adj.get(&node).map(|n| n.len()).unwrap_or(0);
            let in_deg = *in_degree.get(&node).unwrap_or(&0);

            let diff = out_deg as i64 - in_deg as i64;

            if diff != 0 {
                valid_for_circuit = false;
                odd_nodes.push(node);
            }

            if diff == 1 {
                if start_node.is_some() {
                    valid_for_path = false;
                }
                start_node = Some(node);
            } else if diff == -1 {
                if end_node.is_some() {
                    valid_for_path = false;
                }
                end_node = Some(node);
            } else if diff != 0 {
                valid_for_path = false;
            }
        }

        odd_nodes.sort();

        // 총 엣지 수
        let edge_count: usize = out_adj.values().map(|n| n.len()).sum();

        let (has_circuit, has_path, path_start, path_end) = if !is_connected {
            (false, false, None, None)
        } else if valid_for_circuit {
            (
                true,
                true,
                Some(nodes_with_edges[0]),
                Some(nodes_with_edges[0]),
            )
        } else if valid_for_path && start_node.is_some() && end_node.is_some() {
            (false, true, start_node, end_node)
        } else if valid_for_path && start_node.is_none() && end_node.is_none() {
            // 모두 균형 -> 회로
            (
                true,
                true,
                Some(nodes_with_edges[0]),
                Some(nodes_with_edges[0]),
            )
        } else {
            (false, false, None, None)
        };

        Ok(EulerianCheckResult {
            has_eulerian_circuit: has_circuit,
            has_eulerian_path: has_path,
            path_start,
            path_end,
            odd_degree_nodes: odd_nodes,
            edge_count,
            node_count: nodes_with_edges.len(),
        })
    }

    /// 무방향 그래프 연결성 확인
    fn is_connected_undirected(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        nodes_with_edges: &[NodeId],
    ) -> bool {
        if nodes_with_edges.is_empty() {
            return true;
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(nodes_with_edges[0]);
        visited.insert(nodes_with_edges[0]);

        while let Some(node) = queue.pop_front() {
            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    if !visited.contains(&neighbor) {
                        visited.insert(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // 엣지가 있는 모든 노드가 방문되었는지 확인
        nodes_with_edges.iter().all(|n| visited.contains(n))
    }

    /// 방향 그래프 약한 연결성 확인
    fn is_weakly_connected(
        &self,
        out_adj: &HashMap<NodeId, Vec<NodeId>>,
        nodes_with_edges: &[NodeId],
    ) -> bool {
        if nodes_with_edges.is_empty() {
            return true;
        }

        // 무방향으로 변환하여 연결성 확인
        let mut undirected: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        for (&node, neighbors) in out_adj {
            for &neighbor in neighbors {
                undirected.entry(node).or_default().insert(neighbor);
                undirected.entry(neighbor).or_default().insert(node);
            }
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(nodes_with_edges[0]);
        visited.insert(nodes_with_edges[0]);

        while let Some(node) = queue.pop_front() {
            if let Some(neighbors) = undirected.get(&node) {
                for &neighbor in neighbors {
                    if !visited.contains(&neighbor) {
                        visited.insert(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        nodes_with_edges.iter().all(|n| visited.contains(n))
    }

    /// 무방향 인접 리스트 구축
    async fn build_undirected_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
            }
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    adjacency.entry(from).or_default().push(to);
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok(adjacency)
    }

    /// 방향 인접 리스트 구축
    async fn build_directed_adjacency(
        &self,
    ) -> Result<(HashMap<NodeId, Vec<NodeId>>, HashMap<NodeId, usize>)> {
        let mut out_adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                out_adj.insert(node_id, Vec::new());
                in_degree.insert(node_id, 0);
            }
        }

        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from != to {
                    out_adj.entry(from).or_default().push(to);
                    *in_degree.entry(to).or_insert(0) += 1;
                }
            }
        }

        Ok((out_adj, in_degree))
    }
}

/// 오일러 회로 존재 여부 확인 (무방향)
pub async fn has_eulerian_circuit(engine: &GraphEngine) -> Result<bool> {
    let result = EulerianChecker::new(engine).check().await?;
    Ok(result.has_eulerian_circuit)
}

/// 오일러 경로 존재 여부 확인 (무방향)
pub async fn has_eulerian_path(engine: &GraphEngine) -> Result<bool> {
    let result = EulerianChecker::new(engine).check().await?;
    Ok(result.has_eulerian_path)
}

/// 오일러 조건 전체 확인 (무방향)
pub async fn eulerian_check(engine: &GraphEngine) -> Result<EulerianCheckResult> {
    EulerianChecker::new(engine).check().await
}

/// 방향 그래프 오일러 회로 존재 여부 확인
pub async fn has_eulerian_circuit_directed(engine: &GraphEngine) -> Result<bool> {
    let config = EulerianConfig::default().directed();
    let result = EulerianChecker::with_config(engine, config).check().await?;
    Ok(result.has_eulerian_circuit)
}

/// 방향 그래프 오일러 경로 존재 여부 확인
pub async fn has_eulerian_path_directed(engine: &GraphEngine) -> Result<bool> {
    let config = EulerianConfig::default().directed();
    let result = EulerianChecker::with_config(engine, config).check().await?;
    Ok(result.has_eulerian_path)
}

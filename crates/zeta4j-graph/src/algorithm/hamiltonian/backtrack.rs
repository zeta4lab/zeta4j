// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Hamiltonian Path/Circuit 백트래킹 알고리즘
//!
//! 모든 노드를 정확히 한 번씩 방문하는 경로를 찾습니다.
//! NP-Complete 문제이므로 최악의 경우 O(n!) 시간이 소요됩니다.
//!
//! 시간복잡도: O(n!) 최악, 가지치기로 개선

use super::{HamiltonianConfig, HamiltonianResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::{GraphError, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// 백트래킹 기반 Hamiltonian 탐색기
pub struct HamiltonianBacktrack<'a> {
    engine: &'a GraphEngine,
    config: HamiltonianConfig,
}

impl<'a> HamiltonianBacktrack<'a> {
    /// 새로운 HamiltonianBacktrack 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: HamiltonianConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: HamiltonianConfig) -> Self {
        Self { engine, config }
    }

    /// Hamiltonian Path 존재 여부 확인
    pub async fn has_path(&self) -> Result<bool> {
        match self.find_path().await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Hamiltonian Circuit 존재 여부 확인
    pub async fn has_circuit(&self) -> Result<bool> {
        match self.find_circuit().await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Hamiltonian Path 찾기
    pub async fn find_path(&self) -> Result<Option<HamiltonianResult>> {
        let adjacency = self.build_adjacency().await?;
        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        if nodes.is_empty() {
            return Ok(Some(HamiltonianResult::empty()));
        }

        if nodes.len() == 1 {
            return Ok(Some(HamiltonianResult {
                path: nodes,
                is_circuit: false,
                found: true,
                nodes_explored: 1,
            }));
        }

        // 노드 수 제한 확인
        if nodes.len() > self.config.max_nodes {
            return Err(GraphError::Validation {
                message: format!(
                    "Graph has {} nodes, exceeds limit of {}. Use config.max_nodes to increase.",
                    nodes.len(),
                    self.config.max_nodes
                ),
            });
        }

        // 연결성 확인
        if !self.is_connected(&adjacency, &nodes) {
            return Ok(None);
        }

        // 차수 0인 노드 확인 (Hamiltonian 불가)
        for neighbors in adjacency.values() {
            if neighbors.is_empty() && nodes.len() > 1 {
                return Ok(None);
            }
            // 차수 1인 노드는 시작점이나 끝점이어야 함
            if neighbors.len() == 1 && nodes.len() > 2 {
                // 차수 1 노드가 3개 이상이면 불가
                let degree_one_count = adjacency.values().filter(|n| n.len() == 1).count();
                if degree_one_count > 2 {
                    return Ok(None);
                }
            }
        }

        let start_time = Instant::now();
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let mut nodes_explored = 0usize;

        // 차수가 낮은 노드부터 시작점으로 시도 (휴리스틱)
        let mut start_nodes: Vec<NodeId> = nodes.clone();
        start_nodes.sort_by_key(|n| adjacency.get(n).map(|adj| adj.len()).unwrap_or(0));

        for &start in &start_nodes {
            let mut visited = HashSet::new();
            let mut path = vec![start];
            visited.insert(start);

            if let Some(result) = self.backtrack_path(
                &adjacency,
                &nodes,
                &mut visited,
                &mut path,
                start_time,
                timeout,
                &mut nodes_explored,
            )? {
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    /// Hamiltonian Circuit 찾기
    pub async fn find_circuit(&self) -> Result<Option<HamiltonianResult>> {
        let adjacency = self.build_adjacency().await?;
        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        if nodes.is_empty() {
            return Ok(Some(HamiltonianResult::empty_circuit()));
        }

        if nodes.len() == 1 {
            // 단일 노드는 self-loop가 있어야 circuit
            return Ok(None);
        }

        if nodes.len() == 2 {
            // 두 노드는 서로 연결되어 있어야 circuit
            let n1 = nodes[0];
            let n2 = nodes[1];
            if adjacency
                .get(&n1)
                .map(|adj| adj.contains(&n2))
                .unwrap_or(false)
            {
                return Ok(Some(HamiltonianResult {
                    path: vec![n1, n2, n1],
                    is_circuit: true,
                    found: true,
                    nodes_explored: 2,
                }));
            }
            return Ok(None);
        }

        // 노드 수 제한 확인
        if nodes.len() > self.config.max_nodes {
            return Err(GraphError::Validation {
                message: format!(
                    "Graph has {} nodes, exceeds limit of {}",
                    nodes.len(),
                    self.config.max_nodes
                ),
            });
        }

        // 연결성 확인
        if !self.is_connected(&adjacency, &nodes) {
            return Ok(None);
        }

        // 차수 1 이하인 노드가 있으면 circuit 불가
        for neighbors in adjacency.values() {
            if neighbors.len() < 2 {
                return Ok(None);
            }
        }

        let start_time = Instant::now();
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let mut nodes_explored = 0usize;

        // 임의의 시작점에서 시작 (circuit이므로 어디서 시작해도 동일)
        let start = nodes[0];
        let mut visited = HashSet::new();
        let mut path = vec![start];
        visited.insert(start);

        if let Some(mut result) = self.backtrack_circuit(
            &adjacency,
            &nodes,
            start,
            &mut visited,
            &mut path,
            start_time,
            timeout,
            &mut nodes_explored,
        )? {
            result.path.push(start); // 시작점으로 돌아옴
            return Ok(Some(result));
        }

        Ok(None)
    }

    /// 백트래킹으로 Hamiltonian Path 탐색
    #[expect(clippy::too_many_arguments)]
    fn backtrack_path(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_nodes: &[NodeId],
        visited: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
        start_time: Instant,
        timeout: Duration,
        nodes_explored: &mut usize,
    ) -> Result<Option<HamiltonianResult>> {
        *nodes_explored += 1;

        // 타임아웃 확인
        if start_time.elapsed() > timeout {
            return Err(GraphError::Validation {
                message: format!(
                    "Hamiltonian path search timed out after {}ms",
                    self.config.timeout_ms
                ),
            });
        }

        // 모든 노드 방문 완료
        if path.len() == all_nodes.len() {
            return Ok(Some(HamiltonianResult {
                path: path.clone(),
                is_circuit: false,
                found: true,
                nodes_explored: *nodes_explored,
            }));
        }

        let Some(&current) = path.last() else {
            return Ok(None);
        };

        // 다음 노드 후보들 (차수 낮은 순으로 정렬 - 휴리스틱)
        let mut candidates: Vec<NodeId> = adjacency
            .get(&current)
            .map(|neighbors| {
                neighbors
                    .iter()
                    .filter(|n| !visited.contains(n))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();

        candidates.sort_by_key(|n| adjacency.get(n).map(|adj| adj.len()).unwrap_or(0));

        for next in candidates {
            visited.insert(next);
            path.push(next);

            if let Some(result) = self.backtrack_path(
                adjacency,
                all_nodes,
                visited,
                path,
                start_time,
                timeout,
                nodes_explored,
            )? {
                return Ok(Some(result));
            }

            path.pop();
            visited.remove(&next);
        }

        Ok(None)
    }

    /// 백트래킹으로 Hamiltonian Circuit 탐색
    #[expect(clippy::too_many_arguments)]
    fn backtrack_circuit(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_nodes: &[NodeId],
        start: NodeId,
        visited: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
        start_time: Instant,
        timeout: Duration,
        nodes_explored: &mut usize,
    ) -> Result<Option<HamiltonianResult>> {
        *nodes_explored += 1;

        // 타임아웃 확인
        if start_time.elapsed() > timeout {
            return Err(GraphError::Validation {
                message: format!(
                    "Hamiltonian circuit search timed out after {}ms",
                    self.config.timeout_ms
                ),
            });
        }

        // 모든 노드 방문 완료
        if path.len() == all_nodes.len() {
            // 시작점으로 돌아갈 수 있는지 확인
            let Some(&current) = path.last() else {
                return Ok(None);
            };
            if adjacency
                .get(&current)
                .map(|adj| adj.contains(&start))
                .unwrap_or(false)
            {
                return Ok(Some(HamiltonianResult {
                    path: path.clone(),
                    is_circuit: true,
                    found: true,
                    nodes_explored: *nodes_explored,
                }));
            }
            return Ok(None);
        }

        let Some(&current) = path.last() else {
            return Ok(None);
        };

        // 다음 노드 후보들
        let mut candidates: Vec<NodeId> = adjacency
            .get(&current)
            .map(|neighbors| {
                neighbors
                    .iter()
                    .filter(|n| !visited.contains(n))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();

        candidates.sort_by_key(|n| adjacency.get(n).map(|adj| adj.len()).unwrap_or(0));

        for next in candidates {
            visited.insert(next);
            path.push(next);

            if let Some(result) = self.backtrack_circuit(
                adjacency,
                all_nodes,
                start,
                visited,
                path,
                start_time,
                timeout,
                nodes_explored,
            )? {
                return Ok(Some(result));
            }

            path.pop();
            visited.remove(&next);
        }

        Ok(None)
    }

    /// 연결성 확인
    fn is_connected(&self, adjacency: &HashMap<NodeId, Vec<NodeId>>, nodes: &[NodeId]) -> bool {
        if nodes.is_empty() {
            return true;
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(nodes[0]);
        visited.insert(nodes[0]);

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

        visited.len() == nodes.len()
    }

    /// 인접 리스트 구축 (무방향)
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
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
}

/// Hamiltonian Path 존재 여부 (무방향)
pub async fn has_hamiltonian_path(engine: &GraphEngine) -> Result<bool> {
    HamiltonianBacktrack::new(engine).has_path().await
}

/// Hamiltonian Circuit 존재 여부 (무방향)
pub async fn has_hamiltonian_circuit(engine: &GraphEngine) -> Result<bool> {
    HamiltonianBacktrack::new(engine).has_circuit().await
}

/// Hamiltonian Path 찾기 (무방향)
pub async fn hamiltonian_path(engine: &GraphEngine) -> Result<Option<HamiltonianResult>> {
    HamiltonianBacktrack::new(engine).find_path().await
}

/// Hamiltonian Circuit 찾기 (무방향)
pub async fn hamiltonian_circuit(engine: &GraphEngine) -> Result<Option<HamiltonianResult>> {
    HamiltonianBacktrack::new(engine).find_circuit().await
}

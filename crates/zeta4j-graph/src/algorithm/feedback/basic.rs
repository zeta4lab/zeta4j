// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 기본 Feedback Set 연산
//!
//! Feedback Vertex Set (FVS)과 Feedback Arc Set (FAS) 검증 및 Greedy 알고리즘
//!
//! 시간복잡도: is_fvs/is_fas O(V + E), greedy O(V × E)

use super::{FeedbackArcSetResult, FeedbackSetConfig, FeedbackVertexSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 기본 Feedback Set 탐지기
pub struct FeedbackSetDetector<'a> {
    engine: &'a GraphEngine,
    config: FeedbackSetConfig,
}

impl<'a> FeedbackSetDetector<'a> {
    /// 새로운 FeedbackSetDetector 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: FeedbackSetConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: FeedbackSetConfig) -> Self {
        Self { engine, config }
    }

    /// 주어진 정점들이 Feedback Vertex Set인지 확인
    ///
    /// 이 정점들을 제거하면 그래프가 비순환(DAG)이 되는지 확인
    pub async fn is_feedback_vertex_set(&self, nodes: &[NodeId]) -> Result<bool> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(true);
        }

        let remove_set: HashSet<NodeId> = nodes.iter().copied().collect();

        // 제거 후 남은 그래프에서 사이클 검사
        Ok(!self.has_cycle_excluding(&adjacency, &remove_set)?)
    }

    /// 주어진 엣지들이 Feedback Arc Set인지 확인
    ///
    /// 이 엣지들을 제거하면 그래프가 비순환이 되는지 확인
    pub async fn is_feedback_arc_set(&self, edges: &[(NodeId, NodeId)]) -> Result<bool> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(true);
        }

        let remove_edges: HashSet<(NodeId, NodeId)> = edges.iter().copied().collect();

        // 제거 후 남은 그래프에서 사이클 검사
        Ok(!self.has_cycle_excluding_edges(&adjacency, &remove_edges)?)
    }

    /// Greedy FVS: 사이클에 참여하는 정점 제거
    ///
    /// 반복적으로 사이클을 찾고, 사이클에서 최대 차수 정점 제거
    pub async fn greedy_fvs(&self) -> Result<FeedbackVertexSetResult> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(FeedbackVertexSetResult::empty());
        }

        let mut fvs: Vec<NodeId> = Vec::new();
        let mut removed: HashSet<NodeId> = HashSet::new();

        loop {
            // 현재 그래프에서 사이클 찾기
            let cycle = self.find_cycle_excluding(&adjacency, &removed);

            match cycle {
                Some(cycle_nodes) => {
                    // 사이클에서 최대 차수 정점 선택
                    let best_node = cycle_nodes
                        .iter()
                        .max_by_key(|&&node| adjacency.get(&node).map(|n| n.len()).unwrap_or(0))
                        .copied();

                    if let Some(node) = best_node {
                        fvs.push(node);
                        removed.insert(node);
                    }
                }
                None => break,
            }
        }

        fvs.sort();

        Ok(FeedbackVertexSetResult {
            size: fvs.len(),
            is_minimum: false,
            set: fvs,
        })
    }

    /// Greedy FAS: DFS 역방향 엣지 수집
    ///
    /// DFS 탐색 중 발견되는 back edge들이 FAS
    pub async fn greedy_fas(&self) -> Result<FeedbackArcSetResult> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(FeedbackArcSetResult::empty());
        }

        let mut fas: Vec<(NodeId, NodeId)> = Vec::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &start in adjacency.keys() {
            if !visited.contains(&start) {
                self.dfs_find_back_edges(&adjacency, start, &mut visited, &mut in_stack, &mut fas);
            }
        }

        fas.sort();

        Ok(FeedbackArcSetResult {
            size: fas.len(),
            is_minimum: false,
            edges: fas,
        })
    }

    /// DFS로 back edge 찾기
    fn dfs_find_back_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
        back_edges: &mut Vec<(NodeId, NodeId)>,
    ) {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if in_stack.contains(&neighbor) {
                    // Back edge 발견
                    back_edges.push((node, neighbor));
                } else if !visited.contains(&neighbor) {
                    self.dfs_find_back_edges(adjacency, neighbor, visited, in_stack, back_edges);
                }
            }
        }

        in_stack.remove(&node);
    }

    /// 제거된 정점을 제외하고 사이클 존재 여부 확인
    fn has_cycle_excluding(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        excluded: &HashSet<NodeId>,
    ) -> Result<bool> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &node in adjacency.keys() {
            if excluded.contains(&node) || visited.contains(&node) {
                continue;
            }

            if self.dfs_has_cycle(adjacency, node, excluded, &mut visited, &mut in_stack) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// DFS 사이클 검사
    fn dfs_has_cycle(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        excluded: &HashSet<NodeId>,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
    ) -> bool {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if excluded.contains(&neighbor) {
                    continue;
                }

                if in_stack.contains(&neighbor) {
                    return true;
                }

                if !visited.contains(&neighbor)
                    && self.dfs_has_cycle(adjacency, neighbor, excluded, visited, in_stack)
                {
                    return true;
                }
            }
        }

        in_stack.remove(&node);
        false
    }

    /// 제거된 엣지를 제외하고 사이클 존재 여부 확인
    fn has_cycle_excluding_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        excluded_edges: &HashSet<(NodeId, NodeId)>,
    ) -> Result<bool> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &node in adjacency.keys() {
            if visited.contains(&node) {
                continue;
            }

            if self.dfs_has_cycle_excluding_edges(
                adjacency,
                node,
                excluded_edges,
                &mut visited,
                &mut in_stack,
            ) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn dfs_has_cycle_excluding_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        excluded_edges: &HashSet<(NodeId, NodeId)>,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
    ) -> bool {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if excluded_edges.contains(&(node, neighbor)) {
                    continue;
                }

                if in_stack.contains(&neighbor) {
                    return true;
                }

                if !visited.contains(&neighbor)
                    && self.dfs_has_cycle_excluding_edges(
                        adjacency,
                        neighbor,
                        excluded_edges,
                        visited,
                        in_stack,
                    )
                {
                    return true;
                }
            }
        }

        in_stack.remove(&node);
        false
    }

    /// 제거된 정점을 제외하고 사이클 찾기
    fn find_cycle_excluding(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        excluded: &HashSet<NodeId>,
    ) -> Option<Vec<NodeId>> {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();
        let mut path: Vec<NodeId> = Vec::new();

        for &node in adjacency.keys() {
            if excluded.contains(&node) || visited.contains(&node) {
                continue;
            }

            if let Some(cycle) = self.dfs_find_cycle(
                adjacency,
                node,
                excluded,
                &mut visited,
                &mut in_stack,
                &mut path,
            ) {
                return Some(cycle);
            }
        }

        None
    }

    fn dfs_find_cycle(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        excluded: &HashSet<NodeId>,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
    ) -> Option<Vec<NodeId>> {
        visited.insert(node);
        in_stack.insert(node);
        path.push(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if excluded.contains(&neighbor) {
                    continue;
                }

                if in_stack.contains(&neighbor) {
                    // 사이클 발견 - neighbor부터 현재까지의 경로 반환
                    if let Some(start_idx) = path.iter().position(|&n| n == neighbor) {
                        return Some(path[start_idx..].to_vec());
                    }
                }

                if !visited.contains(&neighbor)
                    && let Some(cycle) =
                        self.dfs_find_cycle(adjacency, neighbor, excluded, visited, in_stack, path)
                {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        in_stack.remove(&node);
        None
    }

    /// 방향 그래프 인접 리스트 구축
    async fn build_directed_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
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
                }
            }
        }

        Ok(adjacency)
    }
}

/// 주어진 정점들이 FVS인지 확인
pub async fn is_feedback_vertex_set(engine: &GraphEngine, nodes: &[NodeId]) -> Result<bool> {
    FeedbackSetDetector::new(engine)
        .is_feedback_vertex_set(nodes)
        .await
}

/// 주어진 엣지들이 FAS인지 확인
pub async fn is_feedback_arc_set(engine: &GraphEngine, edges: &[(NodeId, NodeId)]) -> Result<bool> {
    FeedbackSetDetector::new(engine)
        .is_feedback_arc_set(edges)
        .await
}

/// Greedy FVS
pub async fn greedy_fvs(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(FeedbackSetDetector::new(engine).greedy_fvs().await?.set)
}

/// Greedy FAS
pub async fn greedy_fas(engine: &GraphEngine) -> Result<Vec<(NodeId, NodeId)>> {
    Ok(FeedbackSetDetector::new(engine).greedy_fas().await?.edges)
}

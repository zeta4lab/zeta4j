// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Minimum Feedback Set 알고리즘
//!
//! Branch and Bound 기반 최소 FVS/FAS 알고리즘
//!
//! NP-Complete 문제로 지수 시간 복잡도

use super::{FeedbackArcSetResult, FeedbackSetConfig, FeedbackVertexSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Minimum Feedback Set 알고리즘
pub struct MinimumFeedbackSet<'a> {
    engine: &'a GraphEngine,
    config: FeedbackSetConfig,
}

impl<'a> MinimumFeedbackSet<'a> {
    /// 새로운 MinimumFeedbackSet 생성
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

    /// 최소 Feedback Vertex Set 찾기 (Branch and Bound)
    pub async fn minimum_fvs(&self) -> Result<FeedbackVertexSetResult> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(FeedbackVertexSetResult::empty());
        }

        // 사이클이 없으면 빈 FVS
        if !self.has_cycle(&adjacency) {
            return Ok(FeedbackVertexSetResult::empty());
        }

        // Greedy로 상한 계산
        let mut best: HashSet<NodeId> = self.greedy_fvs_internal(&adjacency);
        let mut current: HashSet<NodeId> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        self.branch_and_bound_fvs(&adjacency, &mut current, &mut best, timeout, start);

        let mut result: Vec<NodeId> = best.into_iter().collect();
        result.sort();

        Ok(FeedbackVertexSetResult {
            size: result.len(),
            is_minimum: true,
            set: result,
        })
    }

    /// 최소 Feedback Arc Set 찾기 (Branch and Bound)
    pub async fn minimum_fas(&self) -> Result<FeedbackArcSetResult> {
        let adjacency = self.build_directed_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(FeedbackArcSetResult::empty());
        }

        // 사이클이 없으면 빈 FAS
        if !self.has_cycle(&adjacency) {
            return Ok(FeedbackArcSetResult::empty());
        }

        // 모든 엣지 수집
        let all_edges: Vec<(NodeId, NodeId)> = adjacency
            .iter()
            .flat_map(|(&from, tos)| tos.iter().map(move |&to| (from, to)))
            .collect();

        // Greedy로 상한 계산
        let mut best: HashSet<(NodeId, NodeId)> = self.greedy_fas_internal(&adjacency);
        let mut current: HashSet<(NodeId, NodeId)> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        self.branch_and_bound_fas(
            &adjacency,
            &all_edges,
            0,
            &mut current,
            &mut best,
            timeout,
            start,
        );

        let mut result: Vec<(NodeId, NodeId)> = best.into_iter().collect();
        result.sort();

        Ok(FeedbackArcSetResult {
            size: result.len(),
            is_minimum: true,
            edges: result,
        })
    }

    /// k-FVS 결정 문제
    pub async fn has_fvs_of_size(&self, k: usize) -> Result<bool> {
        let adjacency = self.build_directed_adjacency().await?;

        if !self.has_cycle(&adjacency) {
            return Ok(true); // 사이클 없으면 FVS 불필요
        }

        let all_nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        if k >= all_nodes.len() {
            return Ok(true);
        }

        let mut current: HashSet<NodeId> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        Ok(self.can_break_cycles_with_k_vertices(
            &adjacency,
            &all_nodes,
            0,
            &mut current,
            k,
            timeout,
            start,
        ))
    }

    /// k-FAS 결정 문제
    pub async fn has_fas_of_size(&self, k: usize) -> Result<bool> {
        let adjacency = self.build_directed_adjacency().await?;

        if !self.has_cycle(&adjacency) {
            return Ok(true);
        }

        let all_edges: Vec<(NodeId, NodeId)> = adjacency
            .iter()
            .flat_map(|(&from, tos)| tos.iter().map(move |&to| (from, to)))
            .collect();

        if k >= all_edges.len() {
            return Ok(true);
        }

        let mut current: HashSet<(NodeId, NodeId)> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        Ok(self.can_break_cycles_with_k_edges(
            &adjacency,
            &all_edges,
            0,
            &mut current,
            k,
            timeout,
            start,
        ))
    }

    /// Greedy FVS 내부 구현
    fn greedy_fvs_internal(&self, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> HashSet<NodeId> {
        let mut fvs: HashSet<NodeId> = HashSet::new();
        let mut removed: HashSet<NodeId> = HashSet::new();

        loop {
            let cycle = self.find_cycle_excluding(adjacency, &removed);
            match cycle {
                Some(cycle_nodes) => {
                    // 사이클에서 최대 차수 정점 선택
                    let best_node = cycle_nodes
                        .iter()
                        .max_by_key(|&&node| adjacency.get(&node).map(|n| n.len()).unwrap_or(0))
                        .copied();

                    if let Some(node) = best_node {
                        fvs.insert(node);
                        removed.insert(node);
                    }
                }
                None => break,
            }
        }

        fvs
    }

    /// Greedy FAS 내부 구현
    fn greedy_fas_internal(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
    ) -> HashSet<(NodeId, NodeId)> {
        let mut fas: HashSet<(NodeId, NodeId)> = HashSet::new();
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &start in adjacency.keys() {
            if !visited.contains(&start) {
                self.dfs_collect_back_edges(
                    adjacency,
                    start,
                    &mut visited,
                    &mut in_stack,
                    &mut fas,
                );
            }
        }

        fas
    }

    fn dfs_collect_back_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
        back_edges: &mut HashSet<(NodeId, NodeId)>,
    ) {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if in_stack.contains(&neighbor) {
                    back_edges.insert((node, neighbor));
                } else if !visited.contains(&neighbor) {
                    self.dfs_collect_back_edges(adjacency, neighbor, visited, in_stack, back_edges);
                }
            }
        }

        in_stack.remove(&node);
    }

    /// Branch and Bound FVS
    fn branch_and_bound_fvs(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        current: &mut HashSet<NodeId>,
        best: &mut HashSet<NodeId>,
        timeout: Option<Duration>,
        start: Instant,
    ) {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return;
        }

        if current.len() >= best.len() {
            return;
        }

        // 사이클 찾기
        let cycle = self.find_cycle_excluding(adjacency, current);

        match cycle {
            Some(cycle_nodes) => {
                // 사이클의 각 노드에 대해 분기
                for node in cycle_nodes {
                    current.insert(node);
                    self.branch_and_bound_fvs(adjacency, current, best, timeout, start);
                    current.remove(&node);

                    if let Some(t) = timeout
                        && start.elapsed() > t
                    {
                        return;
                    }
                }
            }
            None => {
                // 사이클 없음 - 현재 집합이 FVS
                if current.len() < best.len() {
                    *best = current.clone();
                }
            }
        }
    }

    /// Branch and Bound FAS
    #[expect(clippy::too_many_arguments)]
    fn branch_and_bound_fas(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_edges: &[(NodeId, NodeId)],
        index: usize,
        current: &mut HashSet<(NodeId, NodeId)>,
        best: &mut HashSet<(NodeId, NodeId)>,
        timeout: Option<Duration>,
        start: Instant,
    ) {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return;
        }

        if current.len() >= best.len() {
            return;
        }

        // 현재 상태에서 사이클 확인
        if !self.has_cycle_excluding_edges(adjacency, current) {
            if current.len() < best.len() {
                *best = current.clone();
            }
            return;
        }

        if index >= all_edges.len() {
            return;
        }

        let edge = all_edges[index];

        // 분기 1: 엣지 포함
        current.insert(edge);
        self.branch_and_bound_fas(
            adjacency,
            all_edges,
            index + 1,
            current,
            best,
            timeout,
            start,
        );
        current.remove(&edge);

        // 분기 2: 엣지 미포함
        self.branch_and_bound_fas(
            adjacency,
            all_edges,
            index + 1,
            current,
            best,
            timeout,
            start,
        );
    }

    /// k개 정점으로 사이클 제거 가능 여부
    #[expect(clippy::too_many_arguments)]
    fn can_break_cycles_with_k_vertices(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_nodes: &[NodeId],
        index: usize,
        current: &mut HashSet<NodeId>,
        k: usize,
        timeout: Option<Duration>,
        start: Instant,
    ) -> bool {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return false;
        }

        if !self.has_cycle_excluding(adjacency, current) {
            return true;
        }

        if current.len() >= k {
            return false;
        }

        if index >= all_nodes.len() {
            return false;
        }

        let node = all_nodes[index];

        // 분기 1: 노드 포함
        current.insert(node);
        if self.can_break_cycles_with_k_vertices(
            adjacency,
            all_nodes,
            index + 1,
            current,
            k,
            timeout,
            start,
        ) {
            current.remove(&node);
            return true;
        }
        current.remove(&node);

        // 분기 2: 노드 미포함
        self.can_break_cycles_with_k_vertices(
            adjacency,
            all_nodes,
            index + 1,
            current,
            k,
            timeout,
            start,
        )
    }

    /// k개 엣지로 사이클 제거 가능 여부
    #[expect(clippy::too_many_arguments)]
    fn can_break_cycles_with_k_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_edges: &[(NodeId, NodeId)],
        index: usize,
        current: &mut HashSet<(NodeId, NodeId)>,
        k: usize,
        timeout: Option<Duration>,
        start: Instant,
    ) -> bool {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return false;
        }

        if !self.has_cycle_excluding_edges(adjacency, current) {
            return true;
        }

        if current.len() >= k {
            return false;
        }

        if index >= all_edges.len() {
            return false;
        }

        let edge = all_edges[index];

        // 분기 1: 엣지 포함
        current.insert(edge);
        if self.can_break_cycles_with_k_edges(
            adjacency,
            all_edges,
            index + 1,
            current,
            k,
            timeout,
            start,
        ) {
            current.remove(&edge);
            return true;
        }
        current.remove(&edge);

        // 분기 2: 엣지 미포함
        self.can_break_cycles_with_k_edges(
            adjacency,
            all_edges,
            index + 1,
            current,
            k,
            timeout,
            start,
        )
    }

    /// 사이클 존재 여부
    fn has_cycle(&self, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> bool {
        self.has_cycle_excluding(adjacency, &HashSet::new())
    }

    fn has_cycle_excluding(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        excluded: &HashSet<NodeId>,
    ) -> bool {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &node in adjacency.keys() {
            if excluded.contains(&node) || visited.contains(&node) {
                continue;
            }

            if self.dfs_has_cycle(adjacency, node, excluded, &mut visited, &mut in_stack) {
                return true;
            }
        }

        false
    }

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

    fn has_cycle_excluding_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        excluded: &HashSet<(NodeId, NodeId)>,
    ) -> bool {
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut in_stack: HashSet<NodeId> = HashSet::new();

        for &node in adjacency.keys() {
            if visited.contains(&node) {
                continue;
            }

            if self.dfs_has_cycle_edges(adjacency, node, excluded, &mut visited, &mut in_stack) {
                return true;
            }
        }

        false
    }

    fn dfs_has_cycle_edges(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node: NodeId,
        excluded: &HashSet<(NodeId, NodeId)>,
        visited: &mut HashSet<NodeId>,
        in_stack: &mut HashSet<NodeId>,
    ) -> bool {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                if excluded.contains(&(node, neighbor)) {
                    continue;
                }

                if in_stack.contains(&neighbor) {
                    return true;
                }

                if !visited.contains(&neighbor)
                    && self.dfs_has_cycle_edges(adjacency, neighbor, excluded, visited, in_stack)
                {
                    return true;
                }
            }
        }

        in_stack.remove(&node);
        false
    }

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

                if in_stack.contains(&neighbor)
                    && let Some(start_idx) = path.iter().position(|&n| n == neighbor)
                {
                    return Some(path[start_idx..].to_vec());
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

/// 최소 FVS 찾기
pub async fn minimum_fvs(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(MinimumFeedbackSet::new(engine).minimum_fvs().await?.set)
}

/// 최소 FAS 찾기
pub async fn minimum_fas(engine: &GraphEngine) -> Result<Vec<(NodeId, NodeId)>> {
    Ok(MinimumFeedbackSet::new(engine).minimum_fas().await?.edges)
}

/// k-FVS 결정 문제
pub async fn has_fvs_of_size(engine: &GraphEngine, k: usize) -> Result<bool> {
    MinimumFeedbackSet::new(engine).has_fvs_of_size(k).await
}

/// k-FAS 결정 문제
pub async fn has_fas_of_size(engine: &GraphEngine, k: usize) -> Result<bool> {
    MinimumFeedbackSet::new(engine).has_fas_of_size(k).await
}

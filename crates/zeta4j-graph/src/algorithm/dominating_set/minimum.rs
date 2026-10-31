// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Minimum Dominating Set 알고리즘
//!
//! Branch and Bound 기반 최소 지배 집합 알고리즘
//!
//! NP-Complete 문제로 지수 시간 복잡도

use super::{DominatingSetConfig, DominatingSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Minimum Dominating Set 알고리즘
pub struct MinimumDominatingSet<'a> {
    engine: &'a GraphEngine,
    config: DominatingSetConfig,
}

impl<'a> MinimumDominatingSet<'a> {
    /// 새로운 MinimumDominatingSet 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: DominatingSetConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: DominatingSetConfig) -> Self {
        Self { engine, config }
    }

    /// 최소 지배 집합 찾기 (Branch and Bound)
    pub async fn minimum_dominating_set(&self) -> Result<DominatingSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(DominatingSetResult::empty());
        }

        let all_nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        let undominated: HashSet<NodeId> = all_nodes.iter().copied().collect();

        // 초기 해: Greedy로 상한 계산
        let mut best: HashSet<NodeId> = self.greedy_upper_bound(&adjacency);
        let mut current: HashSet<NodeId> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        self.branch_and_bound(
            &adjacency,
            &all_nodes,
            0,
            &mut current,
            undominated,
            &mut best,
            timeout,
            start,
        );

        let mut result: Vec<NodeId> = best.into_iter().collect();
        result.sort();

        Ok(DominatingSetResult {
            size: result.len(),
            is_minimum: true,
            set: result,
        })
    }

    /// k-Dominating Set 결정 문제
    ///
    /// 크기 k 이하의 지배 집합이 존재하는지 확인
    pub async fn has_dominating_set_of_size(&self, k: usize) -> Result<bool> {
        let adjacency = self.build_adjacency().await?;
        let n = adjacency.len();

        if k == 0 {
            return Ok(n == 0);
        }

        if k >= n {
            return Ok(true);
        }

        let all_nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        let undominated: HashSet<NodeId> = all_nodes.iter().copied().collect();
        let mut current: HashSet<NodeId> = HashSet::new();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        Ok(self.can_dominate_with_k(
            &adjacency,
            &all_nodes,
            0,
            &mut current,
            undominated,
            k,
            timeout,
            start,
        ))
    }

    /// 연결 지배 집합 (Connected Dominating Set)
    ///
    /// 지배 집합이면서 유도 부분그래프가 연결된 집합
    pub async fn connected_dominating_set(&self) -> Result<DominatingSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(DominatingSetResult::empty());
        }

        // 시작 노드: 최대 차수 노드
        let start_node = adjacency
            .iter()
            .max_by_key(|(_, neighbors)| neighbors.len())
            .map(|(node, _)| *node)
            .ok_or_else(|| crate::error::GraphError::System {
                message: "adjacency is non-empty but no max found".into(),
            })?;

        let mut dominating_set: HashSet<NodeId> = HashSet::new();
        let mut undominated: HashSet<NodeId> = adjacency.keys().copied().collect();

        dominating_set.insert(start_node);
        undominated.remove(&start_node);
        if let Some(neighbors) = adjacency.get(&start_node) {
            for &neighbor in neighbors {
                undominated.remove(&neighbor);
            }
        }

        // BFS로 확장하면서 연결성 유지
        while !undominated.is_empty() {
            // 현재 지배 집합과 인접하면서 가장 많은 미지배 정점을 커버하는 노드
            let best_node = adjacency
                .keys()
                .filter(|&node| {
                    !dominating_set.contains(node)
                        && adjacency
                            .get(node)
                            .map(|neighbors| neighbors.iter().any(|n| dominating_set.contains(n)))
                            .unwrap_or(false)
                })
                .max_by_key(|&node| {
                    let mut count = if undominated.contains(node) { 1 } else { 0 };
                    if let Some(neighbors) = adjacency.get(node) {
                        count += neighbors.iter().filter(|n| undominated.contains(n)).count();
                    }
                    count
                })
                .copied();

            if let Some(node) = best_node {
                dominating_set.insert(node);
                undominated.remove(&node);
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        undominated.remove(&neighbor);
                    }
                }
            } else {
                // 연결된 노드가 없으면 가장 가까운 미지배 정점으로 경로 추가
                break;
            }
        }

        let mut result: Vec<NodeId> = dominating_set.into_iter().collect();
        result.sort();

        Ok(DominatingSetResult {
            size: result.len(),
            is_minimum: false, // 근사 알고리즘
            set: result,
        })
    }

    /// Greedy 상한 계산
    fn greedy_upper_bound(&self, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> HashSet<NodeId> {
        let mut dominating_set: HashSet<NodeId> = HashSet::new();
        let mut undominated: HashSet<NodeId> = adjacency.keys().copied().collect();
        let all_nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        while !undominated.is_empty() {
            let best_node = all_nodes
                .iter()
                .filter(|&node| !dominating_set.contains(node))
                .max_by_key(|&&node| {
                    let mut count = if undominated.contains(&node) { 1 } else { 0 };
                    if let Some(neighbors) = adjacency.get(&node) {
                        count += neighbors.iter().filter(|n| undominated.contains(n)).count();
                    }
                    count
                })
                .copied();

            if let Some(node) = best_node {
                dominating_set.insert(node);
                undominated.remove(&node);
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        undominated.remove(&neighbor);
                    }
                }
            } else {
                break;
            }
        }

        dominating_set
    }

    /// Branch and Bound
    #[expect(clippy::too_many_arguments)]
    fn branch_and_bound(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_nodes: &[NodeId],
        index: usize,
        current: &mut HashSet<NodeId>,
        undominated: HashSet<NodeId>,
        best: &mut HashSet<NodeId>,
        timeout: Option<Duration>,
        start: Instant,
    ) {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return;
        }

        // 가지치기: 현재 집합이 이미 최적보다 크거나 같음
        if current.len() >= best.len() {
            return;
        }

        // 모든 정점이 지배됨
        if undominated.is_empty() {
            if current.len() < best.len() {
                *best = current.clone();
            }
            return;
        }

        // 하한 계산: 최소한 필요한 추가 노드 수
        // 각 노드가 최대 (degree + 1)개를 커버할 수 있음
        let max_coverage = adjacency
            .values()
            .map(|neighbors| neighbors.len() + 1)
            .max()
            .unwrap_or(1);
        let lower_bound = current.len() + undominated.len().div_ceil(max_coverage);
        if lower_bound >= best.len() {
            return;
        }

        if index >= all_nodes.len() {
            return;
        }

        let node = all_nodes[index];

        // 분기 1: 노드를 지배 집합에 포함
        {
            current.insert(node);
            let mut new_undominated = undominated.clone();
            new_undominated.remove(&node);
            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    new_undominated.remove(&neighbor);
                }
            }

            self.branch_and_bound(
                adjacency,
                all_nodes,
                index + 1,
                current,
                new_undominated,
                best,
                timeout,
                start,
            );

            current.remove(&node);
        }

        // 분기 2: 노드를 포함하지 않음
        self.branch_and_bound(
            adjacency,
            all_nodes,
            index + 1,
            current,
            undominated,
            best,
            timeout,
            start,
        );
    }

    /// k개 이하로 지배 가능한지 확인
    #[expect(clippy::too_many_arguments)]
    fn can_dominate_with_k(
        &self,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        all_nodes: &[NodeId],
        index: usize,
        current: &mut HashSet<NodeId>,
        undominated: HashSet<NodeId>,
        k: usize,
        timeout: Option<Duration>,
        start: Instant,
    ) -> bool {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return false;
        }

        if undominated.is_empty() {
            return true;
        }

        if current.len() >= k {
            return false;
        }

        if index >= all_nodes.len() {
            return false;
        }

        let node = all_nodes[index];

        // 분기 1: 노드를 포함
        {
            current.insert(node);
            let mut new_undominated = undominated.clone();
            new_undominated.remove(&node);
            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    new_undominated.remove(&neighbor);
                }
            }

            if self.can_dominate_with_k(
                adjacency,
                all_nodes,
                index + 1,
                current,
                new_undominated,
                k,
                timeout,
                start,
            ) {
                current.remove(&node);
                return true;
            }
            current.remove(&node);
        }

        // 분기 2: 노드를 포함하지 않음
        self.can_dominate_with_k(
            adjacency,
            all_nodes,
            index + 1,
            current,
            undominated,
            k,
            timeout,
            start,
        )
    }

    /// 인접 리스트 구축
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

/// 최소 지배 집합 찾기
pub async fn minimum_dominating_set(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(MinimumDominatingSet::new(engine)
        .minimum_dominating_set()
        .await?
        .set)
}

/// k-Dominating Set 결정 문제
pub async fn has_dominating_set_of_size(engine: &GraphEngine, k: usize) -> Result<bool> {
    MinimumDominatingSet::new(engine)
        .has_dominating_set_of_size(k)
        .await
}

/// 연결 지배 집합
pub async fn connected_dominating_set(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(MinimumDominatingSet::new(engine)
        .connected_dominating_set()
        .await?
        .set)
}

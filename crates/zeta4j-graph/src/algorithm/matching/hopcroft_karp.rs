// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Hopcroft-Karp 최대 이분 매칭 알고리즘
//!
//! 효율적인 최대 이분 매칭 알고리즘으로,
//! BFS로 레벨 그래프를 구축하고 DFS로 다중 증가 경로를 찾습니다.
//!
//! 시간복잡도: O(E × √V)

use super::bipartite::{BipartiteChecker, BipartiteConfig, MatchingResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Hopcroft-Karp 설정
#[derive(Debug, Clone, Default)]
pub struct HopcroftKarpConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl HopcroftKarpConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// NIL 노드를 나타내는 특수 값
const NIL: NodeId = 0;

/// Hopcroft-Karp 알고리즘
pub struct HopcroftKarp<'a> {
    engine: &'a GraphEngine,
    config: HopcroftKarpConfig,
}

impl<'a> HopcroftKarp<'a> {
    /// 새로운 HopcroftKarp 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: HopcroftKarpConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: HopcroftKarpConfig) -> Self {
        Self { engine, config }
    }

    /// 최대 매칭 찾기
    pub async fn find_matching(&self) -> Result<MatchingResult> {
        // 먼저 이분 그래프인지 확인
        let bipartite_config = BipartiteConfig::default()
            .with_relationship_types(self.config.relationship_types.clone().unwrap_or_default());

        let checker = if self.config.relationship_types.is_some() {
            BipartiteChecker::with_config(self.engine, bipartite_config)
        } else {
            BipartiteChecker::new(self.engine)
        };

        let bipartite_result = checker.check().await?;

        if !bipartite_result.is_bipartite {
            return Ok(MatchingResult::empty("Hopcroft-Karp"));
        }

        if bipartite_result.left_partition.is_empty() || bipartite_result.right_partition.is_empty()
        {
            return Ok(MatchingResult::empty("Hopcroft-Karp"));
        }

        let adjacency = self.build_adjacency().await?;
        let right_set: HashSet<NodeId> = bipartite_result.right_partition.iter().copied().collect();

        // pair_left[u] = u와 매칭된 오른쪽 노드 (없으면 NIL)
        // pair_right[v] = v와 매칭된 왼쪽 노드 (없으면 NIL)
        let mut pair_left: HashMap<NodeId, NodeId> = HashMap::new();
        let mut pair_right: HashMap<NodeId, NodeId> = HashMap::new();

        for &node in &bipartite_result.left_partition {
            pair_left.insert(node, NIL);
        }
        for &node in &bipartite_result.right_partition {
            pair_right.insert(node, NIL);
        }

        // dist[u] = BFS에서 u까지의 거리
        let mut dist: HashMap<NodeId, usize> = HashMap::new();

        // Hopcroft-Karp 반복
        while self.bfs(
            &bipartite_result.left_partition,
            &adjacency,
            &right_set,
            &pair_left,
            &pair_right,
            &mut dist,
        ) {
            for &left_node in &bipartite_result.left_partition {
                if pair_left.get(&left_node) == Some(&NIL) {
                    self.dfs(
                        left_node,
                        &adjacency,
                        &right_set,
                        &mut pair_left,
                        &mut pair_right,
                        &mut dist,
                    );
                }
            }
        }

        // 결과 구성
        let mut matches: Vec<(NodeId, NodeId)> = Vec::new();
        let mut matched_left: HashSet<NodeId> = HashSet::new();
        let mut matched_right: HashSet<NodeId> = HashSet::new();

        for (&left, &right) in &pair_left {
            if right != NIL {
                matches.push((left, right));
                matched_left.insert(left);
                matched_right.insert(right);
            }
        }

        matches.sort();

        let unmatched_left: Vec<NodeId> = bipartite_result
            .left_partition
            .iter()
            .filter(|n| !matched_left.contains(n))
            .copied()
            .collect();

        let unmatched_right: Vec<NodeId> = bipartite_result
            .right_partition
            .iter()
            .filter(|n| !matched_right.contains(n))
            .copied()
            .collect();

        Ok(MatchingResult {
            matching_size: matches.len(),
            matches,
            unmatched_left,
            unmatched_right,
            algorithm: "Hopcroft-Karp".to_string(),
        })
    }

    /// BFS로 레벨 그래프 구축
    fn bfs(
        &self,
        left_partition: &[NodeId],
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        right_set: &HashSet<NodeId>,
        pair_left: &HashMap<NodeId, NodeId>,
        pair_right: &HashMap<NodeId, NodeId>,
        dist: &mut HashMap<NodeId, usize>,
    ) -> bool {
        let mut queue: VecDeque<NodeId> = VecDeque::new();
        dist.clear();

        // 매칭되지 않은 왼쪽 노드들을 큐에 추가
        for &left_node in left_partition {
            if pair_left.get(&left_node) == Some(&NIL) {
                dist.insert(left_node, 0);
                queue.push_back(left_node);
            }
        }

        let mut found = false;

        while let Some(left_node) = queue.pop_front() {
            let left_dist = *dist.get(&left_node).unwrap_or(&usize::MAX);

            if let Some(neighbors) = adjacency.get(&left_node) {
                for &right_node in neighbors {
                    if !right_set.contains(&right_node) {
                        continue;
                    }

                    // right_node와 매칭된 왼쪽 노드 찾기
                    let matched_left = *pair_right.get(&right_node).unwrap_or(&NIL);

                    if matched_left == NIL {
                        // 증가 경로 발견
                        found = true;
                    } else if let std::collections::hash_map::Entry::Vacant(e) =
                        dist.entry(matched_left)
                    {
                        e.insert(left_dist + 1);
                        queue.push_back(matched_left);
                    }
                }
            }
        }

        found
    }

    /// DFS로 증가 경로 탐색 및 매칭 업데이트
    fn dfs(
        &self,
        left_node: NodeId,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        right_set: &HashSet<NodeId>,
        pair_left: &mut HashMap<NodeId, NodeId>,
        pair_right: &mut HashMap<NodeId, NodeId>,
        dist: &mut HashMap<NodeId, usize>,
    ) -> bool {
        let left_dist = *dist.get(&left_node).unwrap_or(&usize::MAX);

        if let Some(neighbors) = adjacency.get(&left_node) {
            for &right_node in neighbors {
                if !right_set.contains(&right_node) {
                    continue;
                }

                let matched_left = *pair_right.get(&right_node).unwrap_or(&NIL);

                if matched_left == NIL {
                    // 증가 경로 끝 - 매칭 업데이트
                    pair_left.insert(left_node, right_node);
                    pair_right.insert(right_node, left_node);
                    return true;
                } else if dist.get(&matched_left) == Some(&(left_dist + 1))
                    && self.dfs(
                        matched_left,
                        adjacency,
                        right_set,
                        pair_left,
                        pair_right,
                        dist,
                    )
                {
                    pair_left.insert(left_node, right_node);
                    pair_right.insert(right_node, left_node);
                    return true;
                }
            }
        }

        // 이 노드에서 증가 경로를 찾지 못함
        dist.remove(&left_node);
        false
    }

    /// 인접 리스트 구축
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        // 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, Vec::new());
            }
        }

        // 엣지 수집
        let edge_count = self.engine.edge_count().await;
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

                adjacency.entry(from).or_default().push(to);
                adjacency.entry(to).or_default().push(from); // 무방향
            }
        }

        Ok(adjacency)
    }
}

/// 편의 함수: Hopcroft-Karp 최대 매칭
pub async fn hopcroft_karp(engine: &GraphEngine) -> Result<MatchingResult> {
    HopcroftKarp::new(engine).find_matching().await
}

// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 이분 그래프 판별 및 Kuhn's 최대 매칭 알고리즘
//!
//! 이분 그래프(Bipartite Graph)는 노드를 두 개의 독립 집합으로 나눌 수 있고,
//! 모든 간선이 서로 다른 집합의 노드를 연결하는 그래프입니다.
//!
//! - is_bipartite: O(V + E) - BFS 2-coloring
//! - kuhn_matching: O(V × E) - 증가 경로 기반 최대 매칭

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// 이분 그래프 판별 결과
#[derive(Debug, Clone)]
pub struct BipartiteResult {
    /// 이분 그래프 여부
    pub is_bipartite: bool,
    /// 왼쪽 파티션 노드들
    pub left_partition: Vec<NodeId>,
    /// 오른쪽 파티션 노드들
    pub right_partition: Vec<NodeId>,
    /// 노드별 색상 (0: 왼쪽, 1: 오른쪽)
    pub coloring: HashMap<NodeId, usize>,
    /// 이분 그래프가 아닌 경우 홀수 사이클의 일부 노드들
    pub odd_cycle: Option<Vec<NodeId>>,
}

impl BipartiteResult {
    /// 빈 결과 (이분 그래프)
    pub fn empty_bipartite() -> Self {
        Self {
            is_bipartite: true,
            left_partition: Vec::new(),
            right_partition: Vec::new(),
            coloring: HashMap::new(),
            odd_cycle: None,
        }
    }

    /// 이분 그래프가 아닌 경우
    pub fn not_bipartite(odd_cycle: Vec<NodeId>) -> Self {
        Self {
            is_bipartite: false,
            left_partition: Vec::new(),
            right_partition: Vec::new(),
            coloring: HashMap::new(),
            odd_cycle: Some(odd_cycle),
        }
    }

    /// 특정 노드가 왼쪽 파티션인지 확인
    pub fn is_left(&self, node_id: NodeId) -> bool {
        self.coloring.get(&node_id) == Some(&0)
    }

    /// 특정 노드가 오른쪽 파티션인지 확인
    pub fn is_right(&self, node_id: NodeId) -> bool {
        self.coloring.get(&node_id) == Some(&1)
    }
}

/// 매칭 결과
#[derive(Debug, Clone)]
pub struct MatchingResult {
    /// 매칭된 쌍들 (left, right)
    pub matches: Vec<(NodeId, NodeId)>,
    /// 매칭 크기
    pub matching_size: usize,
    /// 매칭되지 않은 왼쪽 노드들
    pub unmatched_left: Vec<NodeId>,
    /// 매칭되지 않은 오른쪽 노드들
    pub unmatched_right: Vec<NodeId>,
    /// 사용된 알고리즘
    pub algorithm: String,
}

impl MatchingResult {
    /// 빈 결과
    pub fn empty(algorithm: &str) -> Self {
        Self {
            matches: Vec::new(),
            matching_size: 0,
            unmatched_left: Vec::new(),
            unmatched_right: Vec::new(),
            algorithm: algorithm.to_string(),
        }
    }

    /// 최대 매칭인지 확인 (모든 노드가 매칭됨)
    pub fn is_perfect(&self) -> bool {
        self.unmatched_left.is_empty() && self.unmatched_right.is_empty()
    }

    /// 특정 노드의 매칭 상대 찾기
    pub fn get_match(&self, node_id: NodeId) -> Option<NodeId> {
        for &(left, right) in &self.matches {
            if left == node_id {
                return Some(right);
            }
            if right == node_id {
                return Some(left);
            }
        }
        None
    }
}

/// 이분 그래프 설정
#[derive(Debug, Clone)]
pub struct BipartiteConfig {
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 방향성 무시 (true면 무방향 그래프로 처리)
    pub ignore_direction: bool,
}

impl Default for BipartiteConfig {
    fn default() -> Self {
        Self {
            relationship_types: None,
            ignore_direction: true,
        }
    }
}

impl BipartiteConfig {
    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 방향성 설정
    pub fn with_ignore_direction(mut self, ignore: bool) -> Self {
        self.ignore_direction = ignore;
        self
    }
}

/// 이분 그래프 검사 및 Kuhn's 매칭 알고리즘
pub struct BipartiteChecker<'a> {
    engine: &'a GraphEngine,
    config: BipartiteConfig,
}

impl<'a> BipartiteChecker<'a> {
    /// 새로운 BipartiteChecker 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: BipartiteConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: BipartiteConfig) -> Self {
        Self { engine, config }
    }

    /// 이분 그래프 여부 판별 (BFS 2-coloring)
    pub async fn check(&self) -> Result<BipartiteResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(BipartiteResult::empty_bipartite());
        }

        let mut coloring: HashMap<NodeId, usize> = HashMap::new();
        let mut left_partition = Vec::new();
        let mut right_partition = Vec::new();

        // 모든 연결 요소에 대해 BFS
        for &start_node in adjacency.keys() {
            if coloring.contains_key(&start_node) {
                continue;
            }

            // BFS로 2-coloring
            let mut queue = VecDeque::new();
            queue.push_back(start_node);
            coloring.insert(start_node, 0);

            while let Some(node) = queue.pop_front() {
                let node_color = coloring.get(&node).copied().unwrap_or(0);
                let neighbor_color = 1 - node_color;

                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        if let Some(&existing_color) = coloring.get(&neighbor) {
                            if existing_color == node_color {
                                // 홀수 사이클 발견 - 이분 그래프가 아님
                                let odd_cycle = self.find_odd_cycle(&adjacency, node, neighbor);
                                return Ok(BipartiteResult::not_bipartite(odd_cycle));
                            }
                        } else {
                            coloring.insert(neighbor, neighbor_color);
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        // 파티션 분리
        for (&node, &color) in &coloring {
            if color == 0 {
                left_partition.push(node);
            } else {
                right_partition.push(node);
            }
        }

        left_partition.sort();
        right_partition.sort();

        Ok(BipartiteResult {
            is_bipartite: true,
            left_partition,
            right_partition,
            coloring,
            odd_cycle: None,
        })
    }

    /// Kuhn's Algorithm으로 최대 매칭 찾기
    pub async fn kuhn_matching(&self) -> Result<MatchingResult> {
        // 먼저 이분 그래프인지 확인
        let bipartite_result = self.check().await?;

        if !bipartite_result.is_bipartite {
            return Ok(MatchingResult::empty("Kuhn"));
        }

        if bipartite_result.left_partition.is_empty() || bipartite_result.right_partition.is_empty()
        {
            return Ok(MatchingResult::empty("Kuhn"));
        }

        let adjacency = self.build_adjacency().await?;
        let left_set: HashSet<NodeId> = bipartite_result.left_partition.iter().copied().collect();
        let right_set: HashSet<NodeId> = bipartite_result.right_partition.iter().copied().collect();

        // 매칭: right_node -> left_node
        let mut match_right: HashMap<NodeId, NodeId> = HashMap::new();

        // 왼쪽 각 노드에서 증가 경로 탐색
        for &left_node in &bipartite_result.left_partition {
            let mut visited: HashSet<NodeId> = HashSet::new();
            self.dfs_kuhn(
                left_node,
                &adjacency,
                &left_set,
                &right_set,
                &mut visited,
                &mut match_right,
            );
        }

        // 결과 구성
        let mut matches: Vec<(NodeId, NodeId)> = Vec::new();
        let mut matched_left: HashSet<NodeId> = HashSet::new();
        let mut matched_right: HashSet<NodeId> = HashSet::new();

        for (&right, &left) in &match_right {
            matches.push((left, right));
            matched_left.insert(left);
            matched_right.insert(right);
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
            algorithm: "Kuhn".to_string(),
        })
    }

    /// Kuhn's DFS - 증가 경로 탐색
    fn dfs_kuhn(
        &self,
        left_node: NodeId,
        adjacency: &HashMap<NodeId, Vec<NodeId>>,
        left_set: &HashSet<NodeId>,
        right_set: &HashSet<NodeId>,
        visited: &mut HashSet<NodeId>,
        match_right: &mut HashMap<NodeId, NodeId>,
    ) -> bool {
        if let Some(neighbors) = adjacency.get(&left_node) {
            for &right_node in neighbors {
                // 오른쪽 노드만 고려
                if !right_set.contains(&right_node) || visited.contains(&right_node) {
                    continue;
                }

                visited.insert(right_node);

                // right_node가 매칭되지 않았거나, 매칭된 노드에서 다른 경로를 찾을 수 있으면
                if !match_right.contains_key(&right_node) {
                    match_right.insert(right_node, left_node);
                    return true;
                } else {
                    let matched_left = match_right.get(&right_node).copied().unwrap_or(0);
                    if left_set.contains(&matched_left)
                        && self.dfs_kuhn(
                            matched_left,
                            adjacency,
                            left_set,
                            right_set,
                            visited,
                            match_right,
                        )
                    {
                        match_right.insert(right_node, left_node);
                        return true;
                    }
                }
            }
        }
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

                // 무방향 그래프로 처리
                if self.config.ignore_direction {
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok(adjacency)
    }

    /// 홀수 사이클 찾기 (디버깅/진단용)
    fn find_odd_cycle(
        &self,
        _adjacency: &HashMap<NodeId, Vec<NodeId>>,
        node1: NodeId,
        node2: NodeId,
    ) -> Vec<NodeId> {
        // 간단히 두 노드만 반환 (실제 사이클 전체를 찾으려면 더 복잡한 로직 필요)
        vec![node1, node2]
    }
}

/// 편의 함수: 이분 그래프 여부 확인
pub async fn is_bipartite(engine: &GraphEngine) -> Result<bool> {
    let result = BipartiteChecker::new(engine).check().await?;
    Ok(result.is_bipartite)
}

/// 편의 함수: 이분 그래프 파티션
pub async fn bipartite_partition(engine: &GraphEngine) -> Result<BipartiteResult> {
    BipartiteChecker::new(engine).check().await
}

/// 편의 함수: Kuhn's 최대 매칭
pub async fn kuhn_matching(engine: &GraphEngine) -> Result<MatchingResult> {
    BipartiteChecker::new(engine).kuhn_matching().await
}

/// 편의 함수: 최대 매칭 (기본 알고리즘)
pub async fn maximum_matching(engine: &GraphEngine) -> Result<MatchingResult> {
    kuhn_matching(engine).await
}

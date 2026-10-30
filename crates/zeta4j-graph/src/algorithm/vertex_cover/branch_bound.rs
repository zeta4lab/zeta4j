//! Branch and Bound 알고리즘 - 최소 정점 커버 찾기
//!
//! 정확한 최소 정점 커버를 찾는 알고리즘
//!
//! 시간복잡도: O(2^k * n) where k = 커버 크기

use super::{VertexCoverConfig, VertexCoverResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Branch and Bound 알고리즘
pub struct BranchAndBound<'a> {
    engine: &'a GraphEngine,
    config: VertexCoverConfig,
}

impl<'a> BranchAndBound<'a> {
    /// 새로운 BranchAndBound 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: VertexCoverConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: VertexCoverConfig) -> Self {
        Self { engine, config }
    }

    /// 최소 정점 커버 찾기
    pub async fn minimum_vertex_cover(&self) -> Result<VertexCoverResult> {
        let edges = self.get_edges().await?;

        if edges.is_empty() {
            return Ok(VertexCoverResult::empty());
        }

        // 상한: Greedy 알고리즘 결과
        let greedy_cover = self.greedy_upper_bound(&edges);
        let mut best_cover = greedy_cover.clone();
        let mut best_size = greedy_cover.len();

        // 하한: 매칭 기반
        let lower_bound = self.matching_lower_bound(&edges);

        // 이미 최적이면 반환
        if best_size == lower_bound {
            return Ok(VertexCoverResult {
                cover: best_cover,
                size: best_size,
                is_minimum: true,
                uncovered_edges: 0,
            });
        }

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        // Branch and Bound 탐색
        let adjacency = self.build_adjacency(&edges);
        let mut current_cover: HashSet<NodeId> = HashSet::new();
        let mut remaining_edges: HashSet<(NodeId, NodeId)> = edges.into_iter().collect();

        self.branch_and_bound(
            &adjacency,
            &mut current_cover,
            &mut remaining_edges,
            &mut best_cover,
            &mut best_size,
            timeout,
            start,
        );

        let mut result_cover: Vec<NodeId> = best_cover.into_iter().collect();
        result_cover.sort();

        Ok(VertexCoverResult {
            size: result_cover.len(),
            is_minimum: true,
            uncovered_edges: 0,
            cover: result_cover,
        })
    }

    /// k-Vertex Cover 결정 문제
    pub async fn has_vertex_cover_of_size(&self, k: usize) -> Result<bool> {
        let edges = self.get_edges().await?;

        if edges.is_empty() {
            return Ok(true); // 빈 그래프는 빈 집합으로 커버 가능
        }

        // 하한 확인
        let lower_bound = self.matching_lower_bound(&edges);
        if k < lower_bound {
            return Ok(false);
        }

        let adjacency = self.build_adjacency(&edges);
        let mut current_cover: HashSet<NodeId> = HashSet::new();
        let remaining_edges: HashSet<(NodeId, NodeId)> = edges.into_iter().collect();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        Ok(self.can_cover_with_k(
            &adjacency,
            &mut current_cover,
            remaining_edges,
            k,
            timeout,
            start,
        ))
    }

    /// Branch and Bound 재귀
    #[expect(clippy::too_many_arguments)]
    fn branch_and_bound(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        current_cover: &mut HashSet<NodeId>,
        remaining_edges: &mut HashSet<(NodeId, NodeId)>,
        best_cover: &mut Vec<NodeId>,
        best_size: &mut usize,
        timeout: Option<Duration>,
        start: Instant,
    ) {
        // 타임아웃 확인
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return;
        }

        // 가지치기: 현재 커버가 이미 best보다 크거나 같으면 중단
        if current_cover.len() >= *best_size {
            return;
        }

        // 모든 엣지가 커버되었으면 갱신
        if remaining_edges.is_empty() {
            if current_cover.len() < *best_size {
                *best_size = current_cover.len();
                *best_cover = current_cover.iter().copied().collect();
            }
            return;
        }

        // 하한 확인: 현재 커버 + 남은 엣지의 매칭 하한
        let remaining_lower = self.matching_lower_bound_set(remaining_edges);
        if current_cover.len() + remaining_lower >= *best_size {
            return;
        }

        // 최대 차수 엣지 선택
        let (u, v) = self.select_edge(remaining_edges, adjacency);

        // 분기 1: u를 커버에 추가
        {
            current_cover.insert(u);
            let removed_edges: Vec<(NodeId, NodeId)> = remaining_edges
                .iter()
                .filter(|(a, b)| *a == u || *b == u)
                .copied()
                .collect();

            for edge in &removed_edges {
                remaining_edges.remove(edge);
            }

            self.branch_and_bound(
                adjacency,
                current_cover,
                remaining_edges,
                best_cover,
                best_size,
                timeout,
                start,
            );

            // 복원
            for edge in removed_edges {
                remaining_edges.insert(edge);
            }
            current_cover.remove(&u);
        }

        // 분기 2: v를 커버에 추가
        {
            current_cover.insert(v);
            let removed_edges: Vec<(NodeId, NodeId)> = remaining_edges
                .iter()
                .filter(|(a, b)| *a == v || *b == v)
                .copied()
                .collect();

            for edge in &removed_edges {
                remaining_edges.remove(edge);
            }

            self.branch_and_bound(
                adjacency,
                current_cover,
                remaining_edges,
                best_cover,
                best_size,
                timeout,
                start,
            );

            // 복원
            for edge in removed_edges {
                remaining_edges.insert(edge);
            }
            current_cover.remove(&v);
        }
    }

    /// k개 이하로 커버 가능한지 확인
    fn can_cover_with_k(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        current_cover: &mut HashSet<NodeId>,
        remaining_edges: HashSet<(NodeId, NodeId)>,
        k: usize,
        timeout: Option<Duration>,
        start: Instant,
    ) -> bool {
        // 타임아웃 확인
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return false;
        }

        // 모든 엣지가 커버됨
        if remaining_edges.is_empty() {
            return true;
        }

        // 더 이상 노드를 추가할 수 없음
        if current_cover.len() >= k {
            return false;
        }

        // 남은 예산으로 커버 불가능
        let remaining_budget = k - current_cover.len();
        let lower_bound = self.matching_lower_bound_set(&remaining_edges);
        if lower_bound > remaining_budget {
            return false;
        }

        // 엣지 선택
        let (u, v) = self.select_edge(&remaining_edges, adjacency);

        // 분기 1: u 추가
        {
            let mut new_remaining = remaining_edges.clone();
            new_remaining.retain(|(a, b)| *a != u && *b != u);
            current_cover.insert(u);

            if self.can_cover_with_k(adjacency, current_cover, new_remaining, k, timeout, start) {
                current_cover.remove(&u);
                return true;
            }
            current_cover.remove(&u);
        }

        // 분기 2: v 추가
        {
            let mut new_remaining = remaining_edges.clone();
            new_remaining.retain(|(a, b)| *a != v && *b != v);
            current_cover.insert(v);

            if self.can_cover_with_k(adjacency, current_cover, new_remaining, k, timeout, start) {
                current_cover.remove(&v);
                return true;
            }
            current_cover.remove(&v);
        }

        false
    }

    /// 최대 차수 엣지 선택
    fn select_edge(
        &self,
        edges: &HashSet<(NodeId, NodeId)>,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
    ) -> (NodeId, NodeId) {
        // 가장 높은 combined degree를 가진 엣지 선택
        edges
            .iter()
            .max_by_key(|(u, v)| {
                let deg_u = adjacency.get(u).map(|n| n.len()).unwrap_or(0);
                let deg_v = adjacency.get(v).map(|n| n.len()).unwrap_or(0);
                deg_u + deg_v
            })
            .copied()
            .unwrap_or((1, 2))
    }

    /// Greedy 상한
    fn greedy_upper_bound(&self, edges: &[(NodeId, NodeId)]) -> Vec<NodeId> {
        let mut cover: HashSet<NodeId> = HashSet::new();
        let mut uncovered: HashSet<(NodeId, NodeId)> = edges.iter().copied().collect();

        while !uncovered.is_empty() {
            // 최대 차수 노드 선택
            let mut degrees: HashMap<NodeId, usize> = HashMap::new();
            for (u, v) in &uncovered {
                *degrees.entry(*u).or_insert(0) += 1;
                *degrees.entry(*v).or_insert(0) += 1;
            }

            if let Some((&best, _)) = degrees.iter().max_by_key(|&(_, &d)| d) {
                cover.insert(best);
                uncovered.retain(|(u, v)| *u != best && *v != best);
            } else {
                break;
            }
        }

        cover.into_iter().collect()
    }

    /// 매칭 기반 하한 (Greedy Matching)
    fn matching_lower_bound(&self, edges: &[(NodeId, NodeId)]) -> usize {
        let mut matched: HashSet<NodeId> = HashSet::new();
        let mut matching_size = 0;

        for (u, v) in edges {
            if !matched.contains(u) && !matched.contains(v) {
                matched.insert(*u);
                matched.insert(*v);
                matching_size += 1;
            }
        }

        matching_size
    }

    /// Set에서 매칭 하한
    fn matching_lower_bound_set(&self, edges: &HashSet<(NodeId, NodeId)>) -> usize {
        let mut matched: HashSet<NodeId> = HashSet::new();
        let mut matching_size = 0;

        for (u, v) in edges {
            if !matched.contains(u) && !matched.contains(v) {
                matched.insert(*u);
                matched.insert(*v);
                matching_size += 1;
            }
        }

        matching_size
    }

    /// 인접 리스트 구축
    fn build_adjacency(&self, edges: &[(NodeId, NodeId)]) -> HashMap<NodeId, HashSet<NodeId>> {
        let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        for (u, v) in edges {
            adjacency.entry(*u).or_default().insert(*v);
            adjacency.entry(*v).or_default().insert(*u);
        }

        adjacency
    }

    /// 엣지 목록 가져오기
    async fn get_edges(&self) -> Result<Vec<(NodeId, NodeId)>> {
        let mut edges = Vec::new();
        let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();

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
                    let edge_key = if from < to { (from, to) } else { (to, from) };
                    if !seen.contains(&edge_key) {
                        seen.insert(edge_key);
                        edges.push(edge_key);
                    }
                }
            }
        }

        Ok(edges)
    }
}

/// 최소 정점 커버 찾기
pub async fn minimum_vertex_cover(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(BranchAndBound::new(engine)
        .minimum_vertex_cover()
        .await?
        .cover)
}

/// k-Vertex Cover 결정 문제
pub async fn has_vertex_cover_of_size(engine: &GraphEngine, k: usize) -> Result<bool> {
    BranchAndBound::new(engine)
        .has_vertex_cover_of_size(k)
        .await
}

//! Maximum Independent Set 알고리즘
//!
//! Vertex Cover와의 보완 관계를 활용한 최대 독립 집합 알고리즘
//!
//! 핵심 관계: Maximum Independent Set = V - Minimum Vertex Cover

use super::{IndependentSetConfig, IndependentSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Maximum Independent Set 알고리즘
pub struct MaximumIndependentSet<'a> {
    engine: &'a GraphEngine,
    config: IndependentSetConfig,
}

impl<'a> MaximumIndependentSet<'a> {
    /// 새로운 MaximumIndependentSet 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: IndependentSetConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: IndependentSetConfig) -> Self {
        Self { engine, config }
    }

    /// 최대 독립 집합 찾기
    ///
    /// V - Minimum Vertex Cover = Maximum Independent Set
    pub async fn maximum_independent_set(&self) -> Result<IndependentSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(IndependentSetResult::empty());
        }

        let all_nodes: HashSet<NodeId> = adjacency.keys().copied().collect();
        let edges = self.get_edges(&adjacency);

        if edges.is_empty() {
            // 엣지가 없으면 모든 노드가 독립 집합
            let mut set: Vec<NodeId> = all_nodes.into_iter().collect();
            set.sort();
            return Ok(IndependentSetResult {
                size: set.len(),
                is_maximum: true,
                set,
            });
        }

        // Minimum Vertex Cover 계산
        let min_vertex_cover = self.find_minimum_vertex_cover(&adjacency, &edges)?;

        // V - Vertex Cover = Independent Set
        let independent_set: Vec<NodeId> =
            all_nodes.difference(&min_vertex_cover).copied().collect();

        let mut result: Vec<NodeId> = independent_set;
        result.sort();

        Ok(IndependentSetResult {
            size: result.len(),
            is_maximum: true,
            set: result,
        })
    }

    /// k-Independent Set 결정 문제
    ///
    /// 크기 k 이상의 독립 집합이 존재하는지 확인
    pub async fn has_independent_set_of_size(&self, k: usize) -> Result<bool> {
        let adjacency = self.build_adjacency().await?;
        let n = adjacency.len();

        if k == 0 {
            return Ok(true);
        }

        if k > n {
            return Ok(false);
        }

        let edges = self.get_edges(&adjacency);

        if edges.is_empty() {
            // 엣지가 없으면 모든 노드가 독립 집합
            return Ok(n >= k);
        }

        // k-Independent Set ⟺ (n-k)-Vertex Cover
        // 크기 k의 독립 집합이 있으면, 나머지 n-k개가 Vertex Cover
        let vertex_cover_size = n - k;
        self.has_vertex_cover_of_size(&adjacency, &edges, vertex_cover_size)
    }

    /// 모든 극대 독립 집합 찾기
    pub async fn find_all_maximal_independent_sets(&self) -> Result<IndependentSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(IndependentSetResult::empty());
        }

        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        let mut all_maximal: Vec<Vec<NodeId>> = Vec::new();

        // Bron-Kerbosch 변형 (보완 그래프에서의 Clique = 원본 그래프에서의 Independent Set)
        let mut r: HashSet<NodeId> = HashSet::new();
        let mut p: HashSet<NodeId> = nodes.iter().copied().collect();
        let mut x: HashSet<NodeId> = HashSet::new();

        self.bron_kerbosch_independent(&adjacency, &mut r, &mut p, &mut x, &mut all_maximal);

        // 가장 큰 것 찾기
        let max_set = all_maximal
            .iter()
            .max_by_key(|s| s.len())
            .cloned()
            .unwrap_or_default();

        Ok(IndependentSetResult {
            size: max_set.len(),
            is_maximum: true,
            set: max_set,
        })
    }

    /// Bron-Kerbosch 변형 (Independent Set용)
    fn bron_kerbosch_independent(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        r: &mut HashSet<NodeId>,
        p: &mut HashSet<NodeId>,
        x: &mut HashSet<NodeId>,
        results: &mut Vec<Vec<NodeId>>,
    ) {
        if p.is_empty() && x.is_empty() {
            // R은 극대 독립 집합
            let mut set: Vec<NodeId> = r.iter().copied().collect();
            set.sort();
            results.push(set);
            return;
        }

        if p.is_empty() {
            return;
        }

        // 피벗 선택
        let Some(pivot) = p.union(x).next().copied() else {
            return;
        };
        let pivot_neighbors: HashSet<NodeId> = adjacency.get(&pivot).cloned().unwrap_or_default();

        // P에서 pivot의 비이웃들만 탐색 (독립 집합이므로)
        let candidates: Vec<NodeId> = p
            .iter()
            .filter(|&n| !pivot_neighbors.contains(n) && *n != pivot)
            .copied()
            .collect();

        // pivot도 후보에 포함
        let mut all_candidates = vec![pivot];
        all_candidates.extend(candidates);
        all_candidates.retain(|n| p.contains(n));

        for v in all_candidates {
            let neighbors: HashSet<NodeId> = adjacency.get(&v).cloned().unwrap_or_default();

            r.insert(v);

            // 새로운 P: 이웃이 아닌 노드들만 (독립 집합 유지)
            let new_p: HashSet<NodeId> = p
                .iter()
                .filter(|&&n| n != v && !neighbors.contains(&n))
                .copied()
                .collect();

            let new_x: HashSet<NodeId> = x
                .iter()
                .filter(|&&n| !neighbors.contains(&n))
                .copied()
                .collect();

            let mut new_p_mut = new_p;
            let mut new_x_mut = new_x;

            self.bron_kerbosch_independent(adjacency, r, &mut new_p_mut, &mut new_x_mut, results);

            r.remove(&v);
            p.remove(&v);
            x.insert(v);
        }
    }

    /// Minimum Vertex Cover 찾기 (Branch and Bound)
    fn find_minimum_vertex_cover(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        edges: &[(NodeId, NodeId)],
    ) -> Result<HashSet<NodeId>> {
        let mut best_cover: HashSet<NodeId> = adjacency.keys().copied().collect();
        let mut current_cover: HashSet<NodeId> = HashSet::new();
        let mut remaining_edges: HashSet<(NodeId, NodeId)> = edges.iter().copied().collect();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        self.branch_and_bound_vc(
            adjacency,
            &mut current_cover,
            &mut remaining_edges,
            &mut best_cover,
            timeout,
            start,
        );

        Ok(best_cover)
    }

    /// Branch and Bound for Vertex Cover
    #[allow(clippy::only_used_in_recursion)]
    fn branch_and_bound_vc(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        current: &mut HashSet<NodeId>,
        remaining: &mut HashSet<(NodeId, NodeId)>,
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

        if remaining.is_empty() {
            if current.len() < best.len() {
                *best = current.clone();
            }
            return;
        }

        // 엣지 선택
        let Some((u, v)) = remaining.iter().next().copied() else {
            return;
        };

        // 분기 1: u 추가
        {
            current.insert(u);
            let removed: Vec<(NodeId, NodeId)> = remaining
                .iter()
                .filter(|(a, b)| *a == u || *b == u)
                .copied()
                .collect();
            for e in &removed {
                remaining.remove(e);
            }

            self.branch_and_bound_vc(adjacency, current, remaining, best, timeout, start);

            for e in removed {
                remaining.insert(e);
            }
            current.remove(&u);
        }

        // 분기 2: v 추가
        {
            current.insert(v);
            let removed: Vec<(NodeId, NodeId)> = remaining
                .iter()
                .filter(|(a, b)| *a == v || *b == v)
                .copied()
                .collect();
            for e in &removed {
                remaining.remove(e);
            }

            self.branch_and_bound_vc(adjacency, current, remaining, best, timeout, start);

            for e in removed {
                remaining.insert(e);
            }
            current.remove(&v);
        }
    }

    /// k-Vertex Cover 존재 여부
    fn has_vertex_cover_of_size(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        edges: &[(NodeId, NodeId)],
        k: usize,
    ) -> Result<bool> {
        let mut current: HashSet<NodeId> = HashSet::new();
        let remaining: HashSet<(NodeId, NodeId)> = edges.iter().copied().collect();

        let timeout = self.config.timeout_ms.map(Duration::from_millis);
        let start = Instant::now();

        Ok(self.can_cover_with_k(adjacency, &mut current, remaining, k, timeout, start))
    }

    #[allow(clippy::only_used_in_recursion)]
    fn can_cover_with_k(
        &self,
        adjacency: &HashMap<NodeId, HashSet<NodeId>>,
        current: &mut HashSet<NodeId>,
        remaining: HashSet<(NodeId, NodeId)>,
        k: usize,
        timeout: Option<Duration>,
        start: Instant,
    ) -> bool {
        if let Some(t) = timeout
            && start.elapsed() > t
        {
            return false;
        }

        if remaining.is_empty() {
            return true;
        }

        if current.len() >= k {
            return false;
        }

        let Some((u, v)) = remaining.iter().next().copied() else {
            return true;
        };

        // 분기 1: u 추가
        {
            let mut new_remaining = remaining.clone();
            new_remaining.retain(|(a, b)| *a != u && *b != u);
            current.insert(u);

            if self.can_cover_with_k(adjacency, current, new_remaining, k, timeout, start) {
                current.remove(&u);
                return true;
            }
            current.remove(&u);
        }

        // 분기 2: v 추가
        {
            let mut new_remaining = remaining;
            new_remaining.retain(|(a, b)| *a != v && *b != v);
            current.insert(v);

            if self.can_cover_with_k(adjacency, current, new_remaining, k, timeout, start) {
                current.remove(&v);
                return true;
            }
            current.remove(&v);
        }

        false
    }

    /// 엣지 목록 추출
    fn get_edges(&self, adjacency: &HashMap<NodeId, HashSet<NodeId>>) -> Vec<(NodeId, NodeId)> {
        let mut edges = Vec::new();
        let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();

        for (&u, neighbors) in adjacency {
            for &v in neighbors {
                let key = if u < v { (u, v) } else { (v, u) };
                if !seen.contains(&key) {
                    seen.insert(key);
                    edges.push(key);
                }
            }
        }

        edges
    }

    /// 인접 리스트 구축
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, HashSet<NodeId>>> {
        let mut adjacency: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                adjacency.insert(node_id, HashSet::new());
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
                    adjacency.entry(from).or_default().insert(to);
                    adjacency.entry(to).or_default().insert(from);
                }
            }
        }

        Ok(adjacency)
    }
}

/// 최대 독립 집합 찾기
pub async fn maximum_independent_set(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(MaximumIndependentSet::new(engine)
        .maximum_independent_set()
        .await?
        .set)
}

/// k-Independent Set 결정 문제
pub async fn has_independent_set_of_size(engine: &GraphEngine, k: usize) -> Result<bool> {
    MaximumIndependentSet::new(engine)
        .has_independent_set_of_size(k)
        .await
}

/// 모든 극대 독립 집합 찾기
pub async fn find_all_maximal_independent_sets(
    engine: &GraphEngine,
) -> Result<IndependentSetResult> {
    MaximumIndependentSet::new(engine)
        .find_all_maximal_independent_sets()
        .await
}

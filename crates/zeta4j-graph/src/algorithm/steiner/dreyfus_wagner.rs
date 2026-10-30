//! Dreyfus-Wagner 알고리즘
//!
//! 정확한 최소 Steiner Tree를 찾는 동적 프로그래밍 알고리즘
//!
//! 시간복잡도: O(3^k × n² + 2^k × n³) where k = 터미널 수

use super::{SteinerConfig, SteinerResult};
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// Dreyfus-Wagner Steiner Tree 계산기
pub struct DreyfusWagner<'a> {
    engine: &'a GraphEngine,
    config: SteinerConfig,
}

impl<'a> DreyfusWagner<'a> {
    /// 새로운 DreyfusWagner 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: SteinerConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: SteinerConfig) -> Self {
        Self { engine, config }
    }

    /// 정확한 최소 Steiner Tree 찾기
    pub async fn steiner_tree(&self, terminals: &[NodeId]) -> Result<SteinerResult> {
        // 특수 케이스 처리
        if terminals.is_empty() {
            return Ok(SteinerResult::empty(vec![]));
        }

        if terminals.len() == 1 {
            return Ok(SteinerResult {
                edges: vec![],
                total_weight: 0.0,
                terminals: terminals.to_vec(),
                steiner_vertices: vec![],
                all_vertices: terminals.to_vec(),
                is_heuristic: false,
            });
        }

        let adjacency = self.build_weighted_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        // 모든 쌍 최단 거리 계산 (Floyd-Warshall)
        let (dist, next) = self.all_pairs_shortest_paths(&adjacency);

        // 터미널이 2개면 최단 경로
        if terminals.len() == 2 {
            return self.build_path_tree(terminals, &dist, &next);
        }

        // Dreyfus-Wagner DP
        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();
        let k = terminals.len();

        // 터미널 인덱스 매핑 (추후 확장용)
        let _terminal_idx: HashMap<NodeId, usize> =
            terminals.iter().enumerate().map(|(i, &t)| (t, i)).collect();

        // DP 테이블: dp[v][mask] = v를 루트로 하고 mask에 해당하는 터미널들을 연결하는 최소 비용
        let full_mask = (1 << k) - 1;
        let mut dp: HashMap<(NodeId, usize), f64> = HashMap::new();
        let mut parent: HashMap<(NodeId, usize), DPParent> = HashMap::new();

        // 초기화: 각 터미널에서 자기 자신만 연결
        for (i, &t) in terminals.iter().enumerate() {
            let mask = 1 << i;
            dp.insert((t, mask), 0.0);
        }

        // 단일 터미널에서 다른 노드로의 비용 (최단 경로)
        for (i, &t) in terminals.iter().enumerate() {
            let mask = 1 << i;
            for &v in &nodes {
                if v != t
                    && let Some(&d) = dist.get(&(t, v))
                    && d < f64::INFINITY
                {
                    let key = (v, mask);
                    if !dp.contains_key(&key) || dp[&key] > d {
                        dp.insert(key, d);
                        parent.insert(key, DPParent::Edge(t));
                    }
                }
            }
        }

        // DP: 작은 부분집합부터 큰 부분집합으로
        for size in 2..=k {
            // size 크기의 모든 부분집합
            for mask in 1..=full_mask {
                if (mask as u32).count_ones() != size as u32 {
                    continue;
                }

                for &v in &nodes {
                    let mut best = f64::INFINITY;
                    let mut best_parent: Option<DPParent> = None;

                    // 1. 분할: mask를 두 부분으로 나눔
                    let mut submask = mask;
                    while submask > 0 {
                        let complement = mask ^ submask;
                        if complement > 0
                            && submask < mask
                            && let (Some(&cost1), Some(&cost2)) =
                                (dp.get(&(v, submask)), dp.get(&(v, complement)))
                        {
                            let cost = cost1 + cost2;
                            if cost < best {
                                best = cost;
                                best_parent = Some(DPParent::Split(submask, complement));
                            }
                        }
                        submask = (submask - 1) & mask;
                        if submask == 0 {
                            break;
                        }
                    }

                    // 2. 확장: 다른 노드에서 엣지로 연결
                    for &u in &nodes {
                        if u != v
                            && let Some(&prev_cost) = dp.get(&(u, mask))
                            && let Some(&edge_cost) = dist.get(&(u, v))
                            && edge_cost < f64::INFINITY
                        {
                            let cost = prev_cost + edge_cost;
                            if cost < best {
                                best = cost;
                                best_parent = Some(DPParent::Edge(u));
                            }
                        }
                    }

                    if best < f64::INFINITY {
                        let key = (v, mask);
                        if !dp.contains_key(&key) || dp[&key] > best {
                            dp.insert(key, best);
                            if let Some(p) = best_parent {
                                parent.insert(key, p);
                            }
                        }
                    }
                }
            }
        }

        // 최적 루트 찾기
        let mut best_cost = f64::INFINITY;
        let mut best_root = terminals[0];

        for &v in &nodes {
            if let Some(&cost) = dp.get(&(v, full_mask))
                && cost < best_cost
            {
                best_cost = cost;
                best_root = v;
            }
        }

        if best_cost == f64::INFINITY {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        // 트리 복원
        let edges = self.reconstruct_tree(best_root, full_mask, &parent, &dist, &next, terminals);

        // 결과 구성
        let mut all_vertices: HashSet<NodeId> = HashSet::new();
        for (from, to, _) in &edges {
            all_vertices.insert(*from);
            all_vertices.insert(*to);
        }

        let terminal_set: HashSet<NodeId> = terminals.iter().copied().collect();
        let steiner_vertices: Vec<NodeId> = all_vertices
            .iter()
            .filter(|v| !terminal_set.contains(v))
            .copied()
            .collect();

        Ok(SteinerResult {
            edges,
            total_weight: best_cost,
            terminals: terminals.to_vec(),
            steiner_vertices,
            all_vertices: all_vertices.into_iter().collect(),
            is_heuristic: false,
        })
    }

    /// Steiner Tree 비용만 계산
    pub async fn steiner_cost(&self, terminals: &[NodeId]) -> Result<f64> {
        let result = self.steiner_tree(terminals).await?;
        Ok(result.total_weight)
    }

    /// 트리 복원 (재귀)
    #[allow(clippy::only_used_in_recursion)]
    fn reconstruct_tree(
        &self,
        root: NodeId,
        mask: usize,
        parent: &HashMap<(NodeId, usize), DPParent>,
        dist: &HashMap<(NodeId, NodeId), f64>,
        next: &HashMap<(NodeId, NodeId), NodeId>,
        terminals: &[NodeId],
    ) -> Vec<(NodeId, NodeId, f64)> {
        let mut edges: Vec<(NodeId, NodeId, f64)> = Vec::new();

        // 터미널 하나만 있으면 엣지 없음
        if (mask as u32).count_ones() <= 1 {
            return edges;
        }

        if let Some(p) = parent.get(&(root, mask)) {
            match p {
                DPParent::Split(mask1, mask2) => {
                    // 두 서브트리 복원
                    let edges1 = self.reconstruct_tree(root, *mask1, parent, dist, next, terminals);
                    let edges2 = self.reconstruct_tree(root, *mask2, parent, dist, next, terminals);
                    edges.extend(edges1);
                    edges.extend(edges2);
                }
                DPParent::Edge(from) => {
                    // 이전 노드에서 현재 노드로의 경로 추가
                    let path_edges = self.get_path_edges(*from, root, dist, next);
                    edges.extend(path_edges);

                    // 이전 상태에서 재귀
                    let prev_edges =
                        self.reconstruct_tree(*from, mask, parent, dist, next, terminals);
                    edges.extend(prev_edges);
                }
            }
        }

        // 중복 엣지 제거
        let mut unique_edges: Vec<(NodeId, NodeId, f64)> = Vec::new();
        let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();

        for (from, to, weight) in edges {
            let key = if from < to { (from, to) } else { (to, from) };
            if !seen.contains(&key) {
                seen.insert(key);
                unique_edges.push((from, to, weight));
            }
        }

        unique_edges
    }

    /// 경로의 엣지들 반환
    fn get_path_edges(
        &self,
        from: NodeId,
        to: NodeId,
        dist: &HashMap<(NodeId, NodeId), f64>,
        next: &HashMap<(NodeId, NodeId), NodeId>,
    ) -> Vec<(NodeId, NodeId, f64)> {
        let mut edges = Vec::new();
        let mut current = from;

        while current != to {
            if let Some(&nxt) = next.get(&(current, to)) {
                if let Some(&d) = dist.get(&(current, nxt)) {
                    edges.push((current, nxt, d));
                }
                current = nxt;
            } else {
                break;
            }
        }

        edges
    }

    /// 2개 터미널인 경우 최단 경로 트리 생성
    fn build_path_tree(
        &self,
        terminals: &[NodeId],
        dist: &HashMap<(NodeId, NodeId), f64>,
        next: &HashMap<(NodeId, NodeId), NodeId>,
    ) -> Result<SteinerResult> {
        let t1 = terminals[0];
        let t2 = terminals[1];

        let total_weight = dist.get(&(t1, t2)).copied().unwrap_or(f64::INFINITY);

        if total_weight == f64::INFINITY {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        let edges = self.get_path_edges(t1, t2, dist, next);

        let mut all_vertices: HashSet<NodeId> = HashSet::new();
        for (from, to, _) in &edges {
            all_vertices.insert(*from);
            all_vertices.insert(*to);
        }
        all_vertices.insert(t2); // 마지막 노드 추가

        let terminal_set: HashSet<NodeId> = terminals.iter().copied().collect();
        let steiner_vertices: Vec<NodeId> = all_vertices
            .iter()
            .filter(|v| !terminal_set.contains(v))
            .copied()
            .collect();

        Ok(SteinerResult {
            edges,
            total_weight,
            terminals: terminals.to_vec(),
            steiner_vertices,
            all_vertices: all_vertices.into_iter().collect(),
            is_heuristic: false,
        })
    }

    /// 모든 쌍 최단 거리 (Floyd-Warshall)
    #[expect(clippy::type_complexity)]
    fn all_pairs_shortest_paths(
        &self,
        adjacency: &HashMap<NodeId, Vec<(NodeId, f64)>>,
    ) -> (
        HashMap<(NodeId, NodeId), f64>,
        HashMap<(NodeId, NodeId), NodeId>,
    ) {
        let nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        let mut dist: HashMap<(NodeId, NodeId), f64> = HashMap::new();
        let mut next: HashMap<(NodeId, NodeId), NodeId> = HashMap::new();

        // 초기화
        for &u in &nodes {
            for &v in &nodes {
                if u == v {
                    dist.insert((u, v), 0.0);
                } else {
                    dist.insert((u, v), f64::INFINITY);
                }
            }
        }

        // 직접 엣지
        for (&u, neighbors) in adjacency {
            for &(v, w) in neighbors {
                dist.insert((u, v), w);
                next.insert((u, v), v);
            }
        }

        // Floyd-Warshall
        for &k in &nodes {
            for &i in &nodes {
                for &j in &nodes {
                    let ik = dist.get(&(i, k)).copied().unwrap_or(f64::INFINITY);
                    let kj = dist.get(&(k, j)).copied().unwrap_or(f64::INFINITY);
                    let ij = dist.get(&(i, j)).copied().unwrap_or(f64::INFINITY);

                    if ik + kj < ij {
                        dist.insert((i, j), ik + kj);
                        if let Some(&nxt) = next.get(&(i, k)) {
                            next.insert((i, j), nxt);
                        }
                    }
                }
            }
        }

        (dist, next)
    }

    /// 가중치 인접 리스트 구축
    async fn build_weighted_adjacency(&self) -> Result<HashMap<NodeId, Vec<(NodeId, f64)>>> {
        let mut adjacency: HashMap<NodeId, Vec<(NodeId, f64)>> = HashMap::new();

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
                let weight = edge
                    .properties()
                    .get("weight")
                    .and_then(|v| match v {
                        crate::Value::Float(f) => Some(*f),
                        crate::Value::Integer(i) => Some(*i as f64),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                if from != to {
                    adjacency.entry(from).or_default().push((to, weight));
                    adjacency.entry(to).or_default().push((from, weight));
                }
            }
        }

        Ok(adjacency)
    }
}

/// DP 부모 추적용 enum
#[derive(Debug, Clone)]
enum DPParent {
    Split(usize, usize), // 두 마스크로 분할
    Edge(NodeId),        // 엣지를 통해 도달
}

/// 최소 Steiner Tree 찾기
pub async fn steiner_tree(engine: &GraphEngine, terminals: &[NodeId]) -> Result<SteinerResult> {
    DreyfusWagner::new(engine).steiner_tree(terminals).await
}

/// Steiner Tree 비용 계산
pub async fn steiner_cost(engine: &GraphEngine, terminals: &[NodeId]) -> Result<f64> {
    DreyfusWagner::new(engine).steiner_cost(terminals).await
}

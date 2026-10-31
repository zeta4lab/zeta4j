// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Steiner Tree 휴리스틱 알고리즘
//!
//! MST 기반 2-근사 알고리즘 및 개선 기법
//!
//! 시간복잡도: O(k² × n + k² log k) where k = 터미널 수

use super::{SteinerConfig, SteinerResult};
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Steiner Tree 휴리스틱
pub struct SteinerHeuristic<'a> {
    engine: &'a GraphEngine,
    config: SteinerConfig,
}

impl<'a> SteinerHeuristic<'a> {
    /// 새로운 SteinerHeuristic 생성
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

    /// MST 기반 2-근사 Steiner Tree
    ///
    /// 1. 터미널 간 최단 경로로 완전 그래프 구성
    /// 2. 완전 그래프에서 MST 계산
    /// 3. MST 엣지를 원래 경로로 확장
    pub async fn mst_heuristic(&self, terminals: &[NodeId]) -> Result<SteinerResult> {
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
                is_heuristic: true,
            });
        }

        let adjacency = self.build_weighted_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        // 터미널 간 최단 경로 계산
        let (terminal_dist, terminal_paths) =
            self.compute_terminal_distances(terminals, &adjacency);

        // 터미널 완전 그래프에서 MST 계산
        let mst_edges = self.compute_terminal_mst(terminals, &terminal_dist);

        if mst_edges.is_empty() && terminals.len() > 1 {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        // MST 엣지를 원래 경로로 확장
        let mut all_edges: Vec<(NodeId, NodeId, f64)> = Vec::new();

        for (t1, t2, _) in mst_edges {
            if let Some(path) = terminal_paths.get(&(t1, t2)) {
                for &(from, to, weight) in path {
                    all_edges.push((from, to, weight));
                }
            }
        }

        // 중복 엣지 제거
        let edges = self.remove_duplicate_edges(all_edges);
        let total_weight: f64 = edges.iter().map(|(_, _, w)| w).sum();

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
            total_weight,
            terminals: terminals.to_vec(),
            steiner_vertices,
            all_vertices: all_vertices.into_iter().collect(),
            is_heuristic: true,
        })
    }

    /// Shortest Path 기반 휴리스틱 (순차 연결)
    pub async fn sp_heuristic(&self, terminals: &[NodeId]) -> Result<SteinerResult> {
        if terminals.len() <= 1 {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        let adjacency = self.build_weighted_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(SteinerResult::empty(terminals.to_vec()));
        }

        let mut all_edges: Vec<(NodeId, NodeId, f64)> = Vec::new();
        let mut connected: HashSet<NodeId> = HashSet::new();
        connected.insert(terminals[0]);

        // 나머지 터미널을 순차적으로 연결
        for &terminal in &terminals[1..] {
            if connected.contains(&terminal) {
                continue;
            }

            // 현재 연결된 노드 중 가장 가까운 것 찾기
            let mut best_dist = f64::INFINITY;
            let mut best_path: Vec<(NodeId, NodeId, f64)> = Vec::new();

            for &start in &connected {
                if let Some((dist, path)) = self.dijkstra_path(start, terminal, &adjacency)
                    && dist < best_dist
                {
                    best_dist = dist;
                    best_path = path;
                }
            }

            if !best_path.is_empty() {
                for (from, to, weight) in best_path {
                    all_edges.push((from, to, weight));
                    connected.insert(from);
                    connected.insert(to);
                }
            }
        }

        // 중복 엣지 제거
        let edges = self.remove_duplicate_edges(all_edges);
        let total_weight = edges.iter().map(|(_, _, w)| w).sum();

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
            total_weight,
            terminals: terminals.to_vec(),
            steiner_vertices,
            all_vertices: all_vertices.into_iter().collect(),
            is_heuristic: true,
        })
    }

    /// 터미널 간 최단 거리 및 경로 계산
    #[expect(clippy::type_complexity)]
    fn compute_terminal_distances(
        &self,
        terminals: &[NodeId],
        adjacency: &HashMap<NodeId, Vec<(NodeId, f64)>>,
    ) -> (
        HashMap<(NodeId, NodeId), f64>,
        HashMap<(NodeId, NodeId), Vec<(NodeId, NodeId, f64)>>,
    ) {
        let mut distances: HashMap<(NodeId, NodeId), f64> = HashMap::new();
        let mut paths: HashMap<(NodeId, NodeId), Vec<(NodeId, NodeId, f64)>> = HashMap::new();

        for (i, &t1) in terminals.iter().enumerate() {
            for &t2 in &terminals[i + 1..] {
                if let Some((dist, path)) = self.dijkstra_path(t1, t2, adjacency) {
                    distances.insert((t1, t2), dist);
                    distances.insert((t2, t1), dist);
                    paths.insert((t1, t2), path.clone());
                    // 역방향 경로
                    let reverse_path: Vec<(NodeId, NodeId, f64)> = path
                        .iter()
                        .rev()
                        .map(|&(from, to, w)| (to, from, w))
                        .collect();
                    paths.insert((t2, t1), reverse_path);
                }
            }
        }

        (distances, paths)
    }

    /// 터미널 완전 그래프에서 MST 계산 (Kruskal)
    fn compute_terminal_mst(
        &self,
        terminals: &[NodeId],
        dist: &HashMap<(NodeId, NodeId), f64>,
    ) -> Vec<(NodeId, NodeId, f64)> {
        let mut edges: Vec<(NodeId, NodeId, f64)> = Vec::new();

        // 모든 터미널 쌍의 엣지 수집
        for (i, &t1) in terminals.iter().enumerate() {
            for &t2 in &terminals[i + 1..] {
                if let Some(&d) = dist.get(&(t1, t2))
                    && d < f64::INFINITY
                {
                    edges.push((t1, t2, d));
                }
            }
        }

        // 가중치로 정렬
        edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal));

        // Union-Find로 MST 구성
        let mut uf = UnionFind::new(terminals);
        let mut mst: Vec<(NodeId, NodeId, f64)> = Vec::new();

        for (from, to, weight) in edges {
            if uf.find(from) != uf.find(to) {
                uf.union(from, to);
                mst.push((from, to, weight));
            }
        }

        mst
    }

    /// Dijkstra 최단 경로 (경로 반환)
    #[expect(clippy::type_complexity)]
    fn dijkstra_path(
        &self,
        start: NodeId,
        end: NodeId,
        adjacency: &HashMap<NodeId, Vec<(NodeId, f64)>>,
    ) -> Option<(f64, Vec<(NodeId, NodeId, f64)>)> {
        if start == end {
            return Some((0.0, vec![]));
        }

        let mut dist: HashMap<NodeId, f64> = HashMap::new();
        let mut prev: HashMap<NodeId, (NodeId, f64)> = HashMap::new();
        let mut heap = BinaryHeap::new();

        dist.insert(start, 0.0);
        heap.push(DijkstraState {
            cost: 0.0,
            node: start,
        });

        while let Some(DijkstraState { cost, node }) = heap.pop() {
            if node == end {
                // 경로 복원
                let mut path = Vec::new();
                let mut current = end;
                while let Some(&(prev_node, weight)) = prev.get(&current) {
                    path.push((prev_node, current, weight));
                    current = prev_node;
                }
                path.reverse();
                return Some((cost, path));
            }

            if cost > dist.get(&node).copied().unwrap_or(f64::INFINITY) {
                continue;
            }

            if let Some(neighbors) = adjacency.get(&node) {
                for &(next, weight) in neighbors {
                    let next_cost = cost + weight;
                    if next_cost < dist.get(&next).copied().unwrap_or(f64::INFINITY) {
                        dist.insert(next, next_cost);
                        prev.insert(next, (node, weight));
                        heap.push(DijkstraState {
                            cost: next_cost,
                            node: next,
                        });
                    }
                }
            }
        }

        None
    }

    /// 중복 엣지 제거
    fn remove_duplicate_edges(
        &self,
        edges: Vec<(NodeId, NodeId, f64)>,
    ) -> Vec<(NodeId, NodeId, f64)> {
        let mut seen: HashSet<(NodeId, NodeId)> = HashSet::new();
        let mut result = Vec::new();

        for (from, to, weight) in edges {
            let key = if from < to { (from, to) } else { (to, from) };
            if !seen.contains(&key) {
                seen.insert(key);
                result.push((from, to, weight));
            }
        }

        result
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

/// Dijkstra 힙 상태
#[derive(Clone, Copy)]
struct DijkstraState {
    cost: f64,
    node: NodeId,
}

impl PartialEq for DijkstraState {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Eq for DijkstraState {}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Union-Find 자료구조
struct UnionFind {
    parent: HashMap<NodeId, NodeId>,
    rank: HashMap<NodeId, usize>,
}

impl UnionFind {
    fn new(nodes: &[NodeId]) -> Self {
        let mut parent = HashMap::new();
        let mut rank = HashMap::new();
        for &node in nodes {
            parent.insert(node, node);
            rank.insert(node, 0);
        }
        Self { parent, rank }
    }

    fn find(&mut self, x: NodeId) -> NodeId {
        if self.parent[&x] != x {
            let root = self.find(self.parent[&x]);
            self.parent.insert(x, root);
        }
        self.parent[&x]
    }

    fn union(&mut self, x: NodeId, y: NodeId) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx != ry {
            let rank_x = self.rank[&rx];
            let rank_y = self.rank[&ry];
            if rank_x < rank_y {
                self.parent.insert(rx, ry);
            } else if rank_x > rank_y {
                self.parent.insert(ry, rx);
            } else {
                self.parent.insert(ry, rx);
                if let Some(r) = self.rank.get_mut(&rx) {
                    *r += 1;
                }
            }
        }
    }
}

/// MST 기반 휴리스틱 Steiner Tree
pub async fn steiner_tree_heuristic(
    engine: &GraphEngine,
    terminals: &[NodeId],
) -> Result<SteinerResult> {
    SteinerHeuristic::new(engine).mst_heuristic(terminals).await
}

/// 근사 Steiner Tree (MST 휴리스틱)
pub async fn steiner_tree_approx(
    engine: &GraphEngine,
    terminals: &[NodeId],
) -> Result<SteinerResult> {
    SteinerHeuristic::new(engine).mst_heuristic(terminals).await
}

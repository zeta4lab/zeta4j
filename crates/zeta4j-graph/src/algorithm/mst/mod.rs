//! Minimum Spanning Tree 알고리즘 모듈
//!
//! 그래프의 모든 노드를 최소 비용으로 연결하는 트리를 찾습니다.
//!
//! - Prim: 하나의 노드에서 시작하여 트리 확장
//! - Kruskal: 간선을 정렬하여 Union-Find로 구성
//! - Spanning Forest: 비연결 그래프의 MST 집합

mod kruskal;
mod prim;
mod union_find;

pub use prim::{Prim, PrimConfig, prim_mst, prim_mst_from};

pub use union_find::UnionFind;

pub use kruskal::{Kruskal, KruskalConfig, kruskal_mst};

use crate::core::NodeId;

/// MST 알고리즘 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MSTAlgorithm {
    Prim,
    Kruskal,
}

/// MST 결과
#[derive(Debug, Clone)]
pub struct MSTResult {
    /// 트리를 구성하는 간선들 (from, to, weight)
    pub edges: Vec<(NodeId, NodeId, f64)>,
    /// 총 가중치
    pub total_weight: f64,
    /// 포함된 노드 수
    pub node_count: usize,
    /// 사용된 알고리즘
    pub algorithm: MSTAlgorithm,
}

impl MSTResult {
    /// 새로운 MST 결과 생성
    pub fn new(edges: Vec<(NodeId, NodeId, f64)>, algorithm: MSTAlgorithm) -> Self {
        let total_weight = edges.iter().map(|(_, _, w)| w).sum();
        let mut nodes = std::collections::HashSet::new();
        for (from, to, _) in &edges {
            nodes.insert(*from);
            nodes.insert(*to);
        }
        let node_count = nodes.len();

        Self {
            edges,
            total_weight,
            node_count,
            algorithm,
        }
    }

    /// 빈 MST 결과 생성
    pub fn empty(algorithm: MSTAlgorithm) -> Self {
        Self {
            edges: Vec::new(),
            total_weight: 0.0,
            node_count: 0,
            algorithm,
        }
    }

    /// 단일 노드 MST 결과 생성
    pub fn single_node(_node: NodeId, algorithm: MSTAlgorithm) -> Self {
        Self {
            edges: Vec::new(),
            total_weight: 0.0,
            node_count: 1,
            algorithm,
        }
    }

    /// 간선 수
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// MST가 유효한지 확인 (N-1 간선)
    pub fn is_valid_tree(&self) -> bool {
        if self.node_count == 0 {
            return self.edges.is_empty();
        }
        self.edges.len() == self.node_count - 1
    }

    /// 특정 노드가 포함되어 있는지
    pub fn contains_node(&self, node: NodeId) -> bool {
        self.edges
            .iter()
            .any(|(from, to, _)| *from == node || *to == node)
    }

    /// 특정 간선이 포함되어 있는지
    pub fn contains_edge(&self, from: NodeId, to: NodeId) -> bool {
        self.edges
            .iter()
            .any(|(f, t, _)| (*f == from && *t == to) || (*f == to && *t == from))
    }

    /// 가장 무거운 간선 (bottleneck)
    pub fn max_weight_edge(&self) -> Option<(NodeId, NodeId, f64)> {
        self.edges
            .iter()
            .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
    }

    /// 가장 가벼운 간선
    pub fn min_weight_edge(&self) -> Option<(NodeId, NodeId, f64)> {
        self.edges
            .iter()
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
    }

    /// 평균 간선 가중치
    pub fn average_weight(&self) -> f64 {
        if self.edges.is_empty() {
            0.0
        } else {
            self.total_weight / self.edges.len() as f64
        }
    }

    /// 노드 목록
    pub fn nodes(&self) -> Vec<NodeId> {
        let mut nodes = std::collections::HashSet::new();
        for (from, to, _) in &self.edges {
            nodes.insert(*from);
            nodes.insert(*to);
        }
        nodes.into_iter().collect()
    }

    /// 인접 리스트로 변환
    pub fn to_adjacency_list(&self) -> std::collections::HashMap<NodeId, Vec<(NodeId, f64)>> {
        let mut adj = std::collections::HashMap::new();
        for (from, to, weight) in &self.edges {
            adj.entry(*from)
                .or_insert_with(Vec::new)
                .push((*to, *weight));
            adj.entry(*to)
                .or_insert_with(Vec::new)
                .push((*from, *weight));
        }
        adj
    }
}

/// Spanning Forest 결과 (비연결 그래프용)
#[derive(Debug, Clone)]
pub struct SpanningForestResult {
    /// 각 연결 요소의 MST
    pub trees: Vec<MSTResult>,
    /// 총 가중치
    pub total_weight: f64,
    /// 총 노드 수
    pub total_nodes: usize,
    /// 트리 수 (연결 요소 수)
    pub tree_count: usize,
}

impl SpanningForestResult {
    /// 새로운 Spanning Forest 결과 생성
    pub fn new(trees: Vec<MSTResult>) -> Self {
        let total_weight = trees.iter().map(|t| t.total_weight).sum();
        let total_nodes = trees.iter().map(|t| t.node_count).sum();
        let tree_count = trees.len();

        Self {
            trees,
            total_weight,
            total_nodes,
            tree_count,
        }
    }

    /// 빈 Spanning Forest
    pub fn empty() -> Self {
        Self {
            trees: Vec::new(),
            total_weight: 0.0,
            total_nodes: 0,
            tree_count: 0,
        }
    }

    /// 가장 큰 트리
    pub fn largest_tree(&self) -> Option<&MSTResult> {
        self.trees.iter().max_by_key(|t| t.node_count)
    }

    /// 가장 작은 트리
    pub fn smallest_tree(&self) -> Option<&MSTResult> {
        self.trees.iter().min_by_key(|t| t.node_count)
    }
}

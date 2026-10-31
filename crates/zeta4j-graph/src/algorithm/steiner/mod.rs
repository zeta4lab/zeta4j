// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Steiner Tree 알고리즘
//!
//! 주어진 터미널 정점들을 최소 비용으로 연결하는 트리를 찾습니다.
//!
//! ## Steiner Tree란?
//!
//! - **Steiner Tree**: 터미널 집합 T를 연결하는 최소 가중치 트리
//! - **Steiner Vertices**: 터미널이 아니지만 최적해에 포함되는 정점
//!
//! ## 알고리즘
//!
//! - **Dreyfus-Wagner**: 정확한 DP 알고리즘 O(3^k × n²)
//! - **MST Heuristic**: 2-근사 알고리즘
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::steiner::*;
//!
//! // 터미널 노드들을 연결하는 최소 Steiner Tree
//! let result = steiner_tree(&engine, &[1, 3, 5])?;
//! println!("Cost: {}", result.total_weight);
//! ```

mod dreyfus_wagner;
mod heuristic;

pub use dreyfus_wagner::{DreyfusWagner, steiner_cost, steiner_tree};
pub use heuristic::{SteinerHeuristic, steiner_tree_approx, steiner_tree_heuristic};

use crate::core::NodeId;
use std::collections::HashSet;

/// Steiner Tree 설정
#[derive(Debug, Clone, Default)]
pub struct SteinerConfig {
    /// 타임아웃 (밀리초)
    pub timeout_ms: Option<u64>,
    /// 특정 관계 타입만 고려
    pub relationship_types: Option<Vec<String>>,
    /// 휴리스틱 사용 여부 (터미널이 많을 때)
    pub use_heuristic_threshold: Option<usize>,
}

impl SteinerConfig {
    /// 타임아웃 설정
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 휴리스틱 임계값 설정 (터미널 수가 이 값 이상이면 휴리스틱 사용)
    pub fn with_heuristic_threshold(mut self, threshold: usize) -> Self {
        self.use_heuristic_threshold = Some(threshold);
        self
    }
}

/// Steiner Tree 결과
#[derive(Debug, Clone)]
pub struct SteinerResult {
    /// Steiner Tree의 엣지들 (from, to, weight)
    pub edges: Vec<(NodeId, NodeId, f64)>,
    /// 총 가중치
    pub total_weight: f64,
    /// 터미널 정점들
    pub terminals: Vec<NodeId>,
    /// Steiner 정점들 (터미널이 아닌 정점)
    pub steiner_vertices: Vec<NodeId>,
    /// 트리에 포함된 모든 정점
    pub all_vertices: Vec<NodeId>,
    /// 휴리스틱 사용 여부
    pub is_heuristic: bool,
}

impl SteinerResult {
    /// 빈 결과 생성
    pub fn empty(terminals: Vec<NodeId>) -> Self {
        Self {
            edges: Vec::new(),
            total_weight: 0.0,
            terminals,
            steiner_vertices: Vec::new(),
            all_vertices: Vec::new(),
            is_heuristic: false,
        }
    }

    /// 엣지 수
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 정점 수
    pub fn vertex_count(&self) -> usize {
        self.all_vertices.len()
    }
}

/// 주어진 엣지들이 터미널을 연결하는 유효한 Steiner Tree인지 확인
pub fn is_steiner_tree(edges: &[(NodeId, NodeId, f64)], terminals: &[NodeId]) -> bool {
    if terminals.is_empty() {
        return edges.is_empty();
    }

    if terminals.len() == 1 {
        return edges.is_empty();
    }

    // 엣지에서 정점 추출
    let mut vertices: HashSet<NodeId> = HashSet::new();
    for (from, to, _) in edges {
        vertices.insert(*from);
        vertices.insert(*to);
    }

    // 모든 터미널이 포함되어야 함
    for &t in terminals {
        if !vertices.contains(&t) {
            return false;
        }
    }

    // 연결성 검사 (BFS)
    if vertices.is_empty() {
        return false;
    }

    let Some(start) = vertices.iter().next().copied() else {
        return false;
    };
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut queue = vec![start];
    visited.insert(start);

    while let Some(node) = queue.pop() {
        for (from, to, _) in edges {
            if *from == node && !visited.contains(to) {
                visited.insert(*to);
                queue.push(*to);
            }
            if *to == node && !visited.contains(from) {
                visited.insert(*from);
                queue.push(*from);
            }
        }
    }

    // 모든 정점이 연결되어야 함 (트리)
    if visited.len() != vertices.len() {
        return false;
    }

    // 트리 조건: |E| = |V| - 1
    edges.len() == vertices.len() - 1
}

/// Steiner Tree 비용 계산
pub fn tree_cost(edges: &[(NodeId, NodeId, f64)]) -> f64 {
    edges.iter().map(|(_, _, w)| w).sum()
}

/// Steiner 정점 추출 (터미널이 아닌 정점)
pub fn find_steiner_vertices(edges: &[(NodeId, NodeId, f64)], terminals: &[NodeId]) -> Vec<NodeId> {
    let terminal_set: HashSet<NodeId> = terminals.iter().copied().collect();
    let mut vertices: HashSet<NodeId> = HashSet::new();

    for (from, to, _) in edges {
        vertices.insert(*from);
        vertices.insert(*to);
    }

    let mut steiner: Vec<NodeId> = vertices
        .into_iter()
        .filter(|v| !terminal_set.contains(v))
        .collect();
    steiner.sort();
    steiner
}

//! Tree Decomposition (트리 분해) 알고리즘
//!
//! 그래프의 트리 분해와 트리 너비(Treewidth)를 계산합니다.
//!
//! ## 트리 분해란?
//!
//! 트리 분해는 그래프 G = (V, E)를 트리 구조로 표현하는 방법입니다:
//! - 각 트리 노드는 "백(bag)"이라 불리는 정점 집합을 가짐
//! - 트리 너비는 (가장 큰 백의 크기 - 1)로 정의
//!
//! ## 트리 분해의 조건
//!
//! 1. **Vertex Coverage**: 모든 정점 v ∈ V는 최소 하나의 백에 포함
//! 2. **Edge Coverage**: 모든 엣지 (u, v) ∈ E에 대해, u와 v를 모두 포함하는 백이 존재
//! 3. **Coherence**: 정점 v를 포함하는 모든 백들은 연결된 부분 트리를 형성
//!
//! ## 알고리즘
//!
//! - **Min-Degree**: 최소 차수 정점 우선 제거
//! - **Min-Fill**: 최소 채움 엣지 정점 우선 제거
//!
//! ## 활용 분야
//!
//! - 그래프 알고리즘 최적화 (동적 프로그래밍)
//! - 제약 만족 문제 (CSP) 해결
//! - 확률적 추론 (베이지안 네트워크)
//!
//! ## 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::tree_decomposition::*;
//!
//! // Min-Degree 휴리스틱으로 트리 분해
//! let result = tree_decomposition(&engine, Heuristic::MinDegree)?;
//! println!("Treewidth: {}", result.width);
//!
//! // 트리 너비만 계산
//! let width = treewidth(&engine)?;
//! ```

mod decomposition;
mod heuristic;

pub use decomposition::{
    Bag, TreeDecomposition, TreeDecompositionConfig, TreeDecompositionResult, tree_decomposition,
    treewidth,
};
pub use heuristic::{EliminationOrdering, Heuristic, min_degree_ordering, min_fill_ordering};

use crate::core::NodeId;
use std::collections::HashSet;

/// 트리 분해가 유효한지 검증
pub fn is_valid_decomposition(
    decomposition: &TreeDecompositionResult,
    vertices: &[NodeId],
    edges: &[(NodeId, NodeId)],
) -> bool {
    verify_vertex_coverage(decomposition, vertices)
        && verify_edge_coverage(decomposition, edges)
        && verify_coherence(decomposition)
}

/// Vertex Coverage 검증: 모든 정점이 최소 하나의 백에 포함
pub fn verify_vertex_coverage(
    decomposition: &TreeDecompositionResult,
    vertices: &[NodeId],
) -> bool {
    let mut covered: HashSet<NodeId> = HashSet::new();

    for bag in &decomposition.bags {
        for &v in &bag.vertices {
            covered.insert(v);
        }
    }

    vertices.iter().all(|v| covered.contains(v))
}

/// Edge Coverage 검증: 모든 엣지의 양 끝점이 같은 백에 포함
pub fn verify_edge_coverage(
    decomposition: &TreeDecompositionResult,
    edges: &[(NodeId, NodeId)],
) -> bool {
    for (u, v) in edges {
        let mut found = false;
        for bag in &decomposition.bags {
            if bag.vertices.contains(u) && bag.vertices.contains(v) {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// Coherence (Running Intersection) 검증:
/// 정점 v를 포함하는 모든 백들이 연결된 부분 트리를 형성
pub fn verify_coherence(decomposition: &TreeDecompositionResult) -> bool {
    if decomposition.bags.is_empty() {
        return true;
    }

    // 각 정점에 대해 해당 정점을 포함하는 백들이 연결되어 있는지 확인
    let mut all_vertices: HashSet<NodeId> = HashSet::new();
    for bag in &decomposition.bags {
        for &v in &bag.vertices {
            all_vertices.insert(v);
        }
    }

    // 인접 리스트 구성
    let n = decomposition.bags.len();
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for &(i, j) in &decomposition.tree_edges {
        if i < n && j < n {
            adj[i].push(j);
            adj[j].push(i);
        }
    }

    for v in all_vertices {
        // v를 포함하는 백들의 인덱스
        let containing_bags: Vec<usize> = decomposition
            .bags
            .iter()
            .enumerate()
            .filter(|(_, bag)| bag.vertices.contains(&v))
            .map(|(i, _)| i)
            .collect();

        if containing_bags.is_empty() {
            continue;
        }

        // BFS로 연결성 확인
        let start = containing_bags[0];
        let mut visited: HashSet<usize> = HashSet::new();
        let mut queue = vec![start];
        visited.insert(start);

        while let Some(curr) = queue.pop() {
            for &next in &adj[curr] {
                if !visited.contains(&next) && decomposition.bags[next].vertices.contains(&v) {
                    visited.insert(next);
                    queue.push(next);
                }
            }
        }

        // 모든 v를 포함하는 백들이 방문되어야 함
        if visited.len() != containing_bags.len() {
            return false;
        }
    }

    true
}

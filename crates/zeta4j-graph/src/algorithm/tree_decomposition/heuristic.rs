// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 트리 분해를 위한 휴리스틱 알고리즘
//!
//! 제거 순서(Elimination Ordering)를 결정하는 휴리스틱:
//! - Min-Degree: 현재 차수가 가장 작은 정점 우선 제거
//! - Min-Fill: 채움 엣지가 가장 적게 추가되는 정점 우선 제거

use crate::core::NodeId;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// 휴리스틱 알고리즘 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Heuristic {
    /// 최소 차수 휴리스틱: 현재 차수가 가장 작은 정점 우선 제거
    #[default]
    MinDegree,
    /// 최소 채움 휴리스틱: fill edges가 가장 적게 추가되는 정점 우선 제거
    MinFill,
}

/// 제거 순서 결과
#[derive(Debug, Clone)]
pub struct EliminationOrdering {
    /// 제거 순서
    pub ordering: Vec<NodeId>,
    /// 각 정점의 제거 시점 차수
    pub elimination_degrees: HashMap<NodeId, usize>,
    /// 추가된 fill edges 수
    pub fill_edges_count: usize,
}

impl EliminationOrdering {
    /// 트리 너비 상한 (제거 시점 최대 차수)
    pub fn upper_bound_width(&self) -> usize {
        self.elimination_degrees
            .values()
            .copied()
            .max()
            .unwrap_or(0)
    }
}

/// Min-Degree 휴리스틱으로 제거 순서 계산
///
/// 각 단계에서 현재 차수가 가장 작은 정점을 선택하여 제거합니다.
/// 정점 제거 시 이웃들을 클릭으로 연결합니다.
///
/// # 시간 복잡도
/// O(n * m) where n = 정점 수, m = 엣지 수
pub fn min_degree_ordering(
    vertices: &[NodeId],
    adj: &HashMap<NodeId, HashSet<NodeId>>,
) -> Vec<NodeId> {
    if vertices.is_empty() {
        return vec![];
    }

    let mut ordering = Vec::with_capacity(vertices.len());
    let mut removed: HashSet<NodeId> = HashSet::new();

    // 작업용 인접 리스트 복사
    let mut current_adj: HashMap<NodeId, HashSet<NodeId>> = adj.clone();

    // 우선순위 큐: (차수, 정점) - 차수가 작은 순
    let mut pq: BinaryHeap<Reverse<(usize, NodeId)>> = BinaryHeap::new();

    // 초기화
    for &v in vertices {
        let degree = current_adj.get(&v).map(|n| n.len()).unwrap_or(0);
        pq.push(Reverse((degree, v)));
    }

    while ordering.len() < vertices.len() {
        // 차수가 가장 작은 정점 선택
        let v = loop {
            if let Some(Reverse((_, v))) = pq.pop() {
                if !removed.contains(&v) {
                    break v;
                }
            } else {
                // 남은 정점 중 아무거나 선택
                if let Some(&v) = vertices.iter().find(|v| !removed.contains(v)) {
                    break v;
                } else {
                    break vertices[0]; // fallback
                }
            }
        };

        removed.insert(v);
        ordering.push(v);

        // v의 이웃들을 클릭으로 연결
        let neighbors: Vec<NodeId> = current_adj
            .get(&v)
            .map(|n| {
                n.iter()
                    .filter(|&n| !removed.contains(n))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();

        // 이웃들 사이에 엣지 추가 (fill edges)
        for i in 0..neighbors.len() {
            for j in (i + 1)..neighbors.len() {
                let u = neighbors[i];
                let w = neighbors[j];
                current_adj.entry(u).or_default().insert(w);
                current_adj.entry(w).or_default().insert(u);
            }
        }

        // 이웃들의 차수 업데이트
        for &neighbor in &neighbors {
            let new_degree = current_adj
                .get(&neighbor)
                .map(|n| n.iter().filter(|&x| !removed.contains(x)).count())
                .unwrap_or(0);
            pq.push(Reverse((new_degree, neighbor)));
        }

        // v 제거 (인접 리스트에서)
        for &neighbor in &neighbors {
            if let Some(n) = current_adj.get_mut(&neighbor) {
                n.remove(&v);
            }
        }
        current_adj.remove(&v);
    }

    ordering
}

/// Min-Fill 휴리스틱으로 제거 순서 계산
///
/// 각 단계에서 fill edges가 가장 적게 추가되는 정점을 선택하여 제거합니다.
/// 일반적으로 Min-Degree보다 더 좋은 트리 너비를 제공하지만 더 느립니다.
///
/// # 시간 복잡도
/// O(n² * m) worst case
pub fn min_fill_ordering(
    vertices: &[NodeId],
    adj: &HashMap<NodeId, HashSet<NodeId>>,
) -> Vec<NodeId> {
    if vertices.is_empty() {
        return vec![];
    }

    let mut ordering = Vec::with_capacity(vertices.len());
    let mut removed: HashSet<NodeId> = HashSet::new();

    // 작업용 인접 리스트 복사
    let mut current_adj: HashMap<NodeId, HashSet<NodeId>> = adj.clone();

    while ordering.len() < vertices.len() {
        // 각 정점의 fill 수 계산
        let mut best_vertex: Option<NodeId> = None;
        let mut best_fill = usize::MAX;

        for &v in vertices {
            if removed.contains(&v) {
                continue;
            }

            let fill = count_fill_edges(&current_adj, v, &removed);
            if fill < best_fill {
                best_fill = fill;
                best_vertex = Some(v);
            }
        }

        let v = match best_vertex {
            Some(v) => v,
            None => break,
        };

        removed.insert(v);
        ordering.push(v);

        // v의 이웃들을 클릭으로 연결
        let neighbors: Vec<NodeId> = current_adj
            .get(&v)
            .map(|n| {
                n.iter()
                    .filter(|&n| !removed.contains(n))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();

        // fill edges 추가
        for i in 0..neighbors.len() {
            for j in (i + 1)..neighbors.len() {
                let u = neighbors[i];
                let w = neighbors[j];
                current_adj.entry(u).or_default().insert(w);
                current_adj.entry(w).or_default().insert(u);
            }
        }

        // v 제거
        for &neighbor in &neighbors {
            if let Some(n) = current_adj.get_mut(&neighbor) {
                n.remove(&v);
            }
        }
        current_adj.remove(&v);
    }

    ordering
}

/// 정점 v를 제거할 때 추가될 fill edges 수 계산
fn count_fill_edges(
    adj: &HashMap<NodeId, HashSet<NodeId>>,
    v: NodeId,
    removed: &HashSet<NodeId>,
) -> usize {
    let neighbors: Vec<NodeId> = adj
        .get(&v)
        .map(|n| {
            n.iter()
                .filter(|&n| !removed.contains(n))
                .copied()
                .collect()
        })
        .unwrap_or_default();

    let mut fill_count = 0;

    for i in 0..neighbors.len() {
        for j in (i + 1)..neighbors.len() {
            let u = neighbors[i];
            let w = neighbors[j];

            // u와 w 사이에 엣지가 없으면 fill 필요
            let has_edge = adj.get(&u).map(|n| n.contains(&w)).unwrap_or(false);

            if !has_edge {
                fill_count += 1;
            }
        }
    }

    fill_count
}

/// 제거 순서의 품질 평가 (트리 너비 상한)
#[expect(dead_code)]
pub fn evaluate_ordering(ordering: &[NodeId], adj: &HashMap<NodeId, HashSet<NodeId>>) -> usize {
    if ordering.is_empty() {
        return 0;
    }

    let mut removed: HashSet<NodeId> = HashSet::new();
    let mut current_adj = adj.clone();
    let mut max_degree = 0;

    for &v in ordering {
        // 제거 시점의 차수
        let degree = current_adj
            .get(&v)
            .map(|n| n.iter().filter(|&n| !removed.contains(n)).count())
            .unwrap_or(0);

        max_degree = max_degree.max(degree);
        removed.insert(v);

        // fill edges 추가
        let neighbors: Vec<NodeId> = current_adj
            .get(&v)
            .map(|n| {
                n.iter()
                    .filter(|&n| !removed.contains(n))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();

        for i in 0..neighbors.len() {
            for j in (i + 1)..neighbors.len() {
                let u = neighbors[i];
                let w = neighbors[j];
                current_adj.entry(u).or_default().insert(w);
                current_adj.entry(w).or_default().insert(u);
            }
        }
    }

    max_degree
}

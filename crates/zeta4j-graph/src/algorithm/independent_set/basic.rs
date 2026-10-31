// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 기본 Independent Set 연산
//!
//! 독립 집합 검증 및 Greedy 알고리즘
//!
//! 시간복잡도: is_independent_set O(k²), greedy O(V + E)

use super::{IndependentSetConfig, IndependentSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 기본 Independent Set 탐지기
pub struct IndependentSetDetector<'a> {
    engine: &'a GraphEngine,
    config: IndependentSetConfig,
}

impl<'a> IndependentSetDetector<'a> {
    /// 새로운 IndependentSetDetector 생성
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

    /// 주어진 노드들이 독립 집합인지 확인
    pub async fn is_independent_set(&self, nodes: &[NodeId]) -> Result<bool> {
        if nodes.len() <= 1 {
            return Ok(true);
        }

        let adjacency = self.build_adjacency().await?;

        // 집합 내 어떤 두 노드도 인접하지 않아야 함
        for (i, &u) in nodes.iter().enumerate() {
            for &v in nodes.iter().skip(i + 1) {
                let is_adjacent = adjacency
                    .get(&u)
                    .map(|neighbors| neighbors.contains(&v))
                    .unwrap_or(false);

                if is_adjacent {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Greedy 알고리즘으로 독립 집합 찾기
    ///
    /// 최소 차수 정점을 선택하는 전략
    pub async fn greedy_independent_set(&self) -> Result<IndependentSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(IndependentSetResult::empty());
        }

        let mut independent_set: Vec<NodeId> = Vec::new();
        let mut available: HashSet<NodeId> = adjacency.keys().copied().collect();

        while !available.is_empty() {
            // 최소 차수 정점 선택 (available 내에서)
            let Some(best_node) = available
                .iter()
                .min_by_key(|&&node| {
                    adjacency
                        .get(&node)
                        .map(|neighbors| neighbors.iter().filter(|n| available.contains(n)).count())
                        .unwrap_or(0)
                })
                .copied()
            else {
                break;
            };

            // 독립 집합에 추가
            independent_set.push(best_node);
            available.remove(&best_node);

            // 이웃들도 후보에서 제거
            if let Some(neighbors) = adjacency.get(&best_node) {
                for &neighbor in neighbors {
                    available.remove(&neighbor);
                }
            }
        }

        independent_set.sort();

        Ok(IndependentSetResult {
            size: independent_set.len(),
            is_maximum: false, // Greedy는 근사
            set: independent_set,
        })
    }

    /// 최대 차수 이웃 제거 Greedy
    ///
    /// 매번 최대 차수 정점을 제거하고 남은 정점 중 선택
    pub async fn greedy_max_degree_removal(&self) -> Result<IndependentSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(IndependentSetResult::empty());
        }

        let mut independent_set: Vec<NodeId> = Vec::new();
        let mut available: HashSet<NodeId> = adjacency.keys().copied().collect();
        let mut degrees: HashMap<NodeId, usize> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len()))
            .collect();

        while !available.is_empty() {
            // 최소 차수 정점 선택
            let Some(&min_node) = available
                .iter()
                .min_by_key(|&&node| degrees.get(&node).copied().unwrap_or(0))
            else {
                break;
            };

            independent_set.push(min_node);
            available.remove(&min_node);

            // 이웃들 제거 및 차수 업데이트
            if let Some(neighbors) = adjacency.get(&min_node) {
                for &neighbor in neighbors {
                    if available.remove(&neighbor) {
                        // 이웃의 이웃들 차수 감소
                        if let Some(nn) = adjacency.get(&neighbor) {
                            for &nn_node in nn {
                                if let Some(deg) = degrees.get_mut(&nn_node) {
                                    *deg = deg.saturating_sub(1);
                                }
                            }
                        }
                    }
                }
            }
        }

        independent_set.sort();

        Ok(IndependentSetResult {
            size: independent_set.len(),
            is_maximum: false,
            set: independent_set,
        })
    }

    /// 독립수 (최대 독립 집합 크기의 근사값)
    pub async fn independence_number(&self) -> Result<usize> {
        let result = self.greedy_independent_set().await?;
        Ok(result.size)
    }

    /// 인접 리스트 구축 (무방향)
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

/// 주어진 노드들이 독립 집합인지 확인
pub async fn is_independent_set(engine: &GraphEngine, nodes: &[NodeId]) -> Result<bool> {
    IndependentSetDetector::new(engine)
        .is_independent_set(nodes)
        .await
}

/// Greedy 알고리즘으로 독립 집합 찾기
pub async fn greedy_independent_set(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(IndependentSetDetector::new(engine)
        .greedy_independent_set()
        .await?
        .set)
}

/// 독립수 (최대 독립 집합 크기의 근사값)
pub async fn independence_number(engine: &GraphEngine) -> Result<usize> {
    IndependentSetDetector::new(engine)
        .independence_number()
        .await
}

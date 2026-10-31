// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 기본 Dominating Set 연산
//!
//! 지배 집합 검증 및 Greedy 알고리즘
//!
//! 시간복잡도: is_dominating_set O(V + E), greedy O(V²)

use super::{DominatingSetConfig, DominatingSetResult};
use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet};

/// 기본 Dominating Set 탐지기
pub struct DominatingSetDetector<'a> {
    engine: &'a GraphEngine,
    config: DominatingSetConfig,
}

impl<'a> DominatingSetDetector<'a> {
    /// 새로운 DominatingSetDetector 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: DominatingSetConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: DominatingSetConfig) -> Self {
        Self { engine, config }
    }

    /// 주어진 노드들이 지배 집합인지 확인
    ///
    /// 모든 정점이 S에 속하거나 S의 어떤 정점과 인접해야 함
    pub async fn is_dominating_set(&self, nodes: &[NodeId]) -> Result<bool> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(nodes.is_empty());
        }

        let dominating_set: HashSet<NodeId> = nodes.iter().copied().collect();

        // 모든 정점이 지배되는지 확인
        for &node in adjacency.keys() {
            // 노드 자체가 지배 집합에 있으면 OK
            if dominating_set.contains(&node) {
                continue;
            }

            // 이웃 중 하나라도 지배 집합에 있으면 OK
            let neighbors = adjacency.get(&node).map(|n| n.as_slice()).unwrap_or(&[]);
            let is_dominated = neighbors.iter().any(|n| dominating_set.contains(n));

            if !is_dominated {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Greedy 알고리즘으로 지배 집합 찾기
    ///
    /// 매번 가장 많은 미지배 정점을 커버하는 정점 선택
    pub async fn greedy_dominating_set(&self) -> Result<DominatingSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(DominatingSetResult::empty());
        }

        let mut dominating_set: Vec<NodeId> = Vec::new();
        let mut undominated: HashSet<NodeId> = adjacency.keys().copied().collect();
        let all_nodes: Vec<NodeId> = adjacency.keys().copied().collect();

        while !undominated.is_empty() {
            // 가장 많은 미지배 정점을 커버하는 정점 선택
            let best_node = all_nodes
                .iter()
                .filter(|&node| !dominating_set.contains(node))
                .max_by_key(|&&node| {
                    // N[v] ∩ U (자기 자신 포함)
                    let mut count = if undominated.contains(&node) { 1 } else { 0 };
                    if let Some(neighbors) = adjacency.get(&node) {
                        count += neighbors.iter().filter(|n| undominated.contains(n)).count();
                    }
                    count
                })
                .copied();

            if let Some(node) = best_node {
                dominating_set.push(node);

                // 지배된 정점들 제거 (자기 자신 + 이웃들)
                undominated.remove(&node);
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        undominated.remove(&neighbor);
                    }
                }
            } else {
                break;
            }
        }

        dominating_set.sort();

        Ok(DominatingSetResult {
            size: dominating_set.len(),
            is_minimum: false, // Greedy는 근사
            set: dominating_set,
        })
    }

    /// 최대 차수 우선 Greedy
    ///
    /// 차수가 높은 정점부터 선택
    pub async fn greedy_max_degree(&self) -> Result<DominatingSetResult> {
        let adjacency = self.build_adjacency().await?;

        if adjacency.is_empty() {
            return Ok(DominatingSetResult::empty());
        }

        // 차수 순으로 정렬
        let mut nodes_by_degree: Vec<(NodeId, usize)> = adjacency
            .iter()
            .map(|(&node, neighbors)| (node, neighbors.len()))
            .collect();
        nodes_by_degree.sort_by_key(|node| std::cmp::Reverse(node.1));

        let mut dominating_set: Vec<NodeId> = Vec::new();
        let mut undominated: HashSet<NodeId> = adjacency.keys().copied().collect();

        for (node, _) in nodes_by_degree {
            if undominated.is_empty() {
                break;
            }

            // 이 노드나 이웃 중 미지배 정점이 있으면 추가
            let covers_undominated = undominated.contains(&node)
                || adjacency
                    .get(&node)
                    .map(|neighbors| neighbors.iter().any(|n| undominated.contains(n)))
                    .unwrap_or(false);

            if covers_undominated {
                dominating_set.push(node);
                undominated.remove(&node);
                if let Some(neighbors) = adjacency.get(&node) {
                    for &neighbor in neighbors {
                        undominated.remove(&neighbor);
                    }
                }
            }
        }

        dominating_set.sort();

        Ok(DominatingSetResult {
            size: dominating_set.len(),
            is_minimum: false,
            set: dominating_set,
        })
    }

    /// 지배수 (최소 지배 집합 크기의 근사값)
    pub async fn domination_number(&self) -> Result<usize> {
        let result = self.greedy_dominating_set().await?;
        Ok(result.size)
    }

    /// 인접 리스트 구축 (무방향)
    async fn build_adjacency(&self) -> Result<HashMap<NodeId, Vec<NodeId>>> {
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

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

                if from != to {
                    adjacency.entry(from).or_default().push(to);
                    adjacency.entry(to).or_default().push(from);
                }
            }
        }

        Ok(adjacency)
    }
}

/// 주어진 노드들이 지배 집합인지 확인
pub async fn is_dominating_set(engine: &GraphEngine, nodes: &[NodeId]) -> Result<bool> {
    DominatingSetDetector::new(engine)
        .is_dominating_set(nodes)
        .await
}

/// Greedy 알고리즘으로 지배 집합 찾기
pub async fn greedy_dominating_set(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    Ok(DominatingSetDetector::new(engine)
        .greedy_dominating_set()
        .await?
        .set)
}

/// 지배수 (최소 지배 집합 크기의 근사값)
pub async fn domination_number(engine: &GraphEngine) -> Result<usize> {
    DominatingSetDetector::new(engine).domination_number().await
}
